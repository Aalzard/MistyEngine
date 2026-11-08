//! Lua `WorldPartition`: the streaming state and waiting for an area to load.
//!
//! ```lua
//! WorldPartition.enabled        -- the scene has a World Partition
//! WorldPartition.loadedCells    -- cells with every object in
//! WorldPartition.pendingLoads   -- cells being read, waiting or being added
//! WorldPartition.cellCount, WorldPartition.memory (bytes), WorldPartition.lastFrameMs,
//! WorldPartition.maxFrameMs, WorldPartition.budgetMs, WorldPartition.overBudgetFrames
//! WorldPartition.IsAreaLoaded(position, radius)   -- every cell within radius loaded?
//! WorldPartition.CellAt(position)                 -- x, z of the cell under a point
//! WorldPartition.GetCellState(x, z)               -- "Unloaded", "Reading", …, "Loaded"
//! WorldPartition.LoadCell(x, z) / UnloadCell(x, z) -- by hand (stays loaded until unloaded)
//! -- Coroutine style: WorldPartition.Run starts a coroutine the engine resumes every frame.
//! WorldPartition.Run(function()
//!     WorldPartition.WaitForArea(spawnPoint, 50)   -- yields until the area is in
//!     player:SetActive(true)
//! end)
//! WorldPartition.OnAreaLoaded(spawnPoint, 50, function() print("ready") end)
//! ```

use mlua::{Lua, MetaMethod, Result, Table, Value};

use super::api::{LVec3, with_world};
use super::{LogKind, ScriptHost};
use crate::world_partition::{self, CellCoord, CellPhase, StreamingStats};

/// The streaming state scripts see (copied in by the Runtime each frame) and their requests.
#[derive(Clone, Debug, Default)]
pub struct ScriptStreaming {
    pub stats: StreamingStats,
    pub phases: Vec<(CellCoord, CellPhase)>,
    /// Cells scripts asked to load (`true`) or unload (`false`) this frame.
    pub requests: Vec<(CellCoord, bool)>,
}

const PRELUDE: &str = r#"
local WP = WorldPartition
local running = {}
function WP.WaitForArea(position, radius)
    local _, main = coroutine.running()
    if main then
        error("WaitForArea must run inside a coroutine: wrap the code in WorldPartition.Run(function() ... end)", 2)
    end
    while not WP.IsAreaLoaded(position, radius) do coroutine.yield() end
end
function WP.Run(fn, ...)
    local co = coroutine.create(fn)
    local ok, err = coroutine.resume(co, ...)
    if not ok then error(err, 2) end
    if coroutine.status(co) ~= "dead" then running[#running + 1] = co end
    return co
end
function WP.OnAreaLoaded(position, radius, fn)
    return WP.Run(function()
        WP.WaitForArea(position, radius)
        fn()
    end)
end
-- Called by the engine every frame.
function WP.__tick()
    local errors = {}
    local still = {}
    for _, co in ipairs(running) do
        local ok, err = coroutine.resume(co)
        if not ok then
            errors[#errors + 1] = "WorldPartition.Run: " .. tostring(err)
        elseif coroutine.status(co) ~= "dead" then
            still[#still + 1] = co
        end
    end
    running = still
    return errors
end
"#;

fn field(lua: &Lua, key: &str) -> Result<Value> {
    with_world(lua, |w| {
        let s = &w.streaming.stats;
        let enabled = world_partition::find(&w.scene).is_some();
        Ok(match key {
            "enabled" => Value::Boolean(enabled),
            "loadedCells" => Value::Integer(s.loaded as i64),
            "pendingLoads" => Value::Integer(s.pending() as i64),
            "cellCount" => Value::Integer(s.cells as i64),
            "memory" => Value::Integer(s.memory_bytes as i64),
            "lastFrameMs" => Value::Number(s.last_ms as f64),
            "maxFrameMs" => Value::Number(s.max_ms as f64),
            "budgetMs" => Value::Number(s.budget_ms as f64),
            "overBudgetFrames" => Value::Integer(s.over_budget as i64),
            _ => Value::Nil,
        })
    })?
}

/// Register the `WorldPartition` global.
pub fn register(lua: &Lua) -> Result<()> {
    let wp: Table = lua.create_table()?;
    wp.set(
        "IsAreaLoaded",
        lua.create_function(|lua, (p, radius): (LVec3, Option<f32>)| {
            with_world(lua, |w| match world_partition::find(&w.scene) {
                Some((_, part)) => {
                    world_partition::area_loaded(&w.scene, &part, p.0, radius.unwrap_or(0.0))
                }
                None => true,
            })
        })?,
    )?;
    wp.set(
        "CellAt",
        lua.create_function(|lua, p: LVec3| {
            with_world(lua, |w| {
                world_partition::find(&w.scene).map_or((0, 0), |(_, part)| part.cell_of(p.0))
            })
        })?,
    )?;
    wp.set(
        "GetCellState",
        lua.create_function(|lua, (x, z): (i32, i32)| {
            with_world(lua, |w| {
                w.streaming
                    .phases
                    .iter()
                    .find(|(c, _)| *c == (x, z))
                    .map_or("Unloaded", |(_, p)| p.name())
                    .to_owned()
            })
        })?,
    )?;
    wp.set(
        "LoadCell",
        lua.create_function(|lua, (x, z): (i32, i32)| {
            with_world(lua, |w| w.streaming.requests.push(((x, z), true)))
        })?,
    )?;
    wp.set(
        "UnloadCell",
        lua.create_function(|lua, (x, z): (i32, i32)| {
            with_world(lua, |w| w.streaming.requests.push(((x, z), false)))
        })?,
    )?;
    let meta = lua.create_table()?;
    meta.set(
        MetaMethod::Index.name(),
        lua.create_function(|lua, (_, key): (Table, String)| field(lua, &key))?,
    )?;
    wp.set_metatable(Some(meta))?;
    lua.globals().set("WorldPartition", wp)?;
    lua.load(PRELUDE).set_name("=WorldPartition").exec()
}

impl ScriptHost {
    /// Resume `WorldPartition.Run` coroutines (after the streamer's work, before Start).
    pub(super) fn world_partition_tick(&mut self) {
        let tick = self
            .lua
            .globals()
            .get::<Table>("WorldPartition")
            .and_then(|t| t.raw_get::<mlua::Function>("__tick"));
        let Ok(tick) = tick else {
            return;
        };
        match tick.call::<Vec<String>>(()) {
            Ok(errors) => {
                for e in errors {
                    self.log(LogKind::Error, e);
                }
            }
            Err(e) => self.log(LogKind::Error, format!("WorldPartition: {e}")),
        }
        self.flush_spawned();
    }

    /// The streaming state scripts read, and take the cells they asked for.
    pub fn exchange_streaming(&mut self, state: ScriptStreaming) -> Vec<(CellCoord, bool)> {
        self.with_world(|w| {
            let requests = std::mem::take(&mut w.streaming.requests);
            w.streaming = ScriptStreaming {
                requests: Vec::new(),
                ..state
            };
            requests
        })
    }
}
