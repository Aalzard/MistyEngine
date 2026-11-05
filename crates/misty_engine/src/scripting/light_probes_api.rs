//! Lua `LightProbes.GetInterpolatedProbe(position)`: the baked light probes blended at a world
//! position, as a table with `coefficients` (9 linear Colors, irradiance / π SH), `color` (the
//! average diffuse light), `baked` (false = no baked probes: the scene's ambient light instead)
//! and `Evaluate(direction)` (the diffuse light arriving from around `direction` as a Color).

use glam::Vec3;
use mlua::{Lua, Result, Table};

use super::api::{LVec3, with_world};
use super::ui_api::LColor;
use crate::render::probes::sh;
use crate::scene::Scene;

fn color(c: Vec3) -> LColor {
    LColor([c.x, c.y, c.z, 1.0])
}

/// The scene's ambient light as SH (what objects without probes get).
fn ambient(scene: &Scene) -> [Vec3; 9] {
    let lights = crate::render_scene::lights(scene);
    crate::render::Environment::new(scene.render_settings.clone(), &lights).ambient_sh()
}

fn probe_table(lua: &Lua, coefficients: [Vec3; 9], baked: bool) -> Result<Table> {
    let t = lua.create_table()?;
    let list = lua.create_table()?;
    for (i, c) in coefficients.iter().enumerate() {
        list.set(i + 1, color(*c))?;
    }
    t.set("coefficients", list)?;
    t.set("baked", baked)?;
    t.set("color", color(coefficients[0] * 0.282_095))?;
    t.set(
        "Evaluate",
        lua.create_function(move |_, (_this, dir): (Table, LVec3)| {
            Ok(color(sh::eval(&coefficients, dir.0).max(Vec3::ZERO)))
        })?,
    )?;
    Ok(t)
}

/// Register the `LightProbes` global.
pub fn register(lua: &Lua) -> Result<()> {
    let probes = lua.create_table()?;
    probes.set(
        "GetInterpolatedProbe",
        lua.create_function(|lua, position: LVec3| {
            let (sh, baked) = with_world(lua, |w| {
                match crate::light_probes::interpolated(&w.scene, position.0) {
                    Some(sh) => (sh, true),
                    None => (ambient(&w.scene), false),
                }
            })?;
            probe_table(lua, sh, baked)
        })?,
    )?;
    lua.globals().set("LightProbes", probes)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use glam::Vec3;

    use crate::light_probes::{LightProbeData, asset_path_for};
    use crate::physics::Physics;
    use crate::runtime::Time;
    use crate::scene::{Scene, ScriptRef, Scripts};
    use crate::scripting::{Input, ScriptHost};

    fn project(source: &str) -> PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("misty_lp_script_{n}"));
        std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
        std::fs::write(root.join("Assets/Scripts/Probe.lua"), source).unwrap();
        root
    }

    fn run(root: &std::path::Path, scene: &mut Scene) -> Vec<String> {
        let e = scene.spawn("Runner", None);
        scene.add(
            e,
            Scripts {
                list: vec![ScriptRef {
                    path: "Assets/Scripts/Probe.lua".into(),
                    ..Default::default()
                }],
            },
        );
        let physics = Physics::build(scene, Vec3::ZERO);
        let mut host = ScriptHost::start(scene, physics, root.to_owned());
        let mut time = Time::default();
        time.advance(1.0 / 60.0);
        host.update(scene, &time, Input::default());
        host.take_logs().into_iter().map(|(_, t)| t).collect()
    }

    /// Red probes on the +X side, blue on the −X side: the middle is purple, the sides tinted;
    /// without probes the result is the ambient light.
    #[test]
    fn scripts_sample_the_baked_probes() {
        let src = r#"
local P = {}
function P:Start()
    for _, x in ipairs({-2, 0, 2}) do
        local p = LightProbes.GetInterpolatedProbe(Vector3.new(x, 1, 0))
        local c = p:Evaluate(Vector3.new(0, 1, 0))
        Debug.Log(tostring(p.baked), string.format("%.2f %.2f", c.r, c.b), #p.coefficients)
    end
end
return P
"#;
        let root = project(src);
        let mut positions = Vec::new();
        let mut sh = Vec::new();
        for x in [-2.0, 2.0] {
            for y in [0.0, 2.0] {
                for z in [-1.0, 1.0] {
                    positions.push(Vec3::new(x, y, z));
                    let mut c = [Vec3::ZERO; 9];
                    let tint = if x > 0.0 { Vec3::X } else { Vec3::Z };
                    c[0] = tint / 0.282_095;
                    sh.push(c);
                }
            }
        }
        let rel = asset_path_for("Assets/Scenes/Probe.scene");
        LightProbeData::new(positions, &sh)
            .save(&root.join(&rel))
            .unwrap();
        let mut scene = Scene::new("Probe");
        scene.render_settings.light_probes = Some(root.join(&rel).to_string_lossy().into_owned());
        let logs = run(&root, &mut scene);
        assert_eq!(
            logs,
            ["true 0.00 1.00 9", "true 0.50 0.50 9", "true 1.00 0.00 9"],
            "{logs:?}"
        );

        let mut plain = Scene::new("Plain");
        let logs = run(&root, &mut plain);
        assert_eq!(logs.len(), 3);
        assert!(logs.iter().all(|l| l.starts_with("false")), "{logs:?}");
    }
}
