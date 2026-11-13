//! Tile assets (`.tile`, JSON): what a painted cell shows and how it collides.
//!
//! - **Tile**: one sprite with a tint.
//! - **Rule Tile**: tiling rules checked in order; each rule lists, for the eight neighbours,
//!   "this" (the same Rule Tile), "not this" or "don't care", and picks a sprite (single, random
//!   or animated) when the neighbourhood matches. Rules can also match rotated or mirrored, which
//!   turns the output sprite the same way. No rule matching = the default sprite.
//! - **Animated Tile**: frames played at a speed (frames per second), offset by a start time.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Instant, SystemTime};

use glam::IVec2;
use serde::{Deserialize, Serialize};

use super::{TileTransform, Tilemap};

/// File extension of tile assets.
pub const EXTENSION: &str = "tile";

pub fn is_tile(path: &str) -> bool {
    path.rsplit('.')
        .next()
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TileKind {
    #[default]
    Tile,
    RuleTile,
    AnimatedTile,
}

impl TileKind {
    pub const ALL: [Self; 3] = [Self::Tile, Self::RuleTile, Self::AnimatedTile];

    pub fn label(self) -> &'static str {
        match self {
            Self::Tile => "Tile",
            Self::RuleTile => "Rule Tile",
            Self::AnimatedTile => "Animated Tile",
        }
    }
}

/// The shape a tile gives a Tilemap Collider 2D.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ColliderType {
    None,
    /// The sprite's rectangle.
    #[default]
    Sprite,
    /// The whole cell.
    Grid,
}

impl ColliderType {
    pub const ALL: [Self; 3] = [Self::None, Self::Sprite, Self::Grid];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Sprite => "Sprite",
            Self::Grid => "Grid",
        }
    }
}

/// What a tiling rule requires of one neighbour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Neighbor {
    #[default]
    DontCare,
    /// The neighbour holds this same tile.
    This,
    /// The neighbour is empty or holds another tile.
    NotThis,
}

impl Neighbor {
    /// Next state when the rule grid cell is clicked (don't care → this → not this).
    pub fn cycle(self) -> Self {
        match self {
            Self::DontCare => Self::This,
            Self::This => Self::NotThis,
            Self::NotThis => Self::DontCare,
        }
    }
}

/// Neighbour offsets in rule order: top-left, top, top-right, left, right, bottom-left, bottom,
/// bottom-right.
pub const NEIGHBORS: [IVec2; 8] = [
    IVec2::new(-1, 1),
    IVec2::new(0, 1),
    IVec2::new(1, 1),
    IVec2::new(-1, 0),
    IVec2::new(1, 0),
    IVec2::new(-1, -1),
    IVec2::new(0, -1),
    IVec2::new(1, -1),
];

/// Extra orientations a rule also matches in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RuleTransform {
    #[default]
    Fixed,
    /// Also matches turned by 90°, 180° and 270° (the sprite turns with it).
    Rotated,
    /// Also matches mirrored left-right.
    MirrorX,
    /// Also matches mirrored top-bottom.
    MirrorY,
    /// Also matches mirrored either way or both.
    MirrorXY,
}

impl RuleTransform {
    pub const ALL: [Self; 5] = [
        Self::Fixed,
        Self::Rotated,
        Self::MirrorX,
        Self::MirrorY,
        Self::MirrorXY,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Fixed => "Fixed",
            Self::Rotated => "Rotated",
            Self::MirrorX => "Mirror X",
            Self::MirrorY => "Mirror Y",
            Self::MirrorXY => "Mirror XY",
        }
    }

    /// Orientations tried, in order.
    fn candidates(self) -> &'static [TileTransform] {
        const F: TileTransform = TileTransform(0);
        const X: TileTransform = TileTransform(TileTransform::FLIP_X);
        const Y: TileTransform = TileTransform(TileTransform::FLIP_Y);
        const XY: TileTransform = TileTransform(TileTransform::FLIP_X | TileTransform::FLIP_Y);
        match self {
            Self::Fixed => &[F],
            Self::Rotated => &[F, TileTransform(1), TileTransform(2), TileTransform(3)],
            Self::MirrorX => &[F, X],
            Self::MirrorY => &[F, Y],
            Self::MirrorXY => &[F, X, Y, XY],
        }
    }
}

/// How a matched rule picks its sprite.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RuleOutput {
    #[default]
    Single,
    /// One of the sprites, chosen per cell (stable).
    Random,
    /// The sprites in turn, at the rule's animation speed.
    Animation,
}

impl RuleOutput {
    pub const ALL: [Self; 3] = [Self::Single, Self::Random, Self::Animation];
}

/// One rule of a Rule Tile.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TilingRule {
    pub neighbors: [Neighbor; 8],
    pub sprites: Vec<String>,
    pub output: RuleOutput,
    pub transform: RuleTransform,
    pub collider: ColliderType,
    /// Frames per second of Animation output.
    pub animation_speed: f32,
}

impl Default for TilingRule {
    fn default() -> Self {
        Self {
            neighbors: [Neighbor::DontCare; 8],
            sprites: Vec::new(),
            output: RuleOutput::Single,
            transform: RuleTransform::Fixed,
            collider: ColliderType::Sprite,
            animation_speed: 1.0,
        }
    }
}

impl TilingRule {
    /// The rule with neighbours from a 3×3 picture: rows top to bottom, `#` = this, `.` = not
    /// this, anything else = don't care; the centre character is ignored.
    pub fn pattern(rows: [&str; 3], sprite: &str) -> Self {
        let mut neighbors = [Neighbor::DontCare; 8];
        let mut i = 0;
        for (r, row) in rows.iter().enumerate() {
            for (c, ch) in row.chars().take(3).enumerate() {
                if r == 1 && c == 1 {
                    continue;
                }
                neighbors[i] = match ch {
                    '#' => Neighbor::This,
                    '.' => Neighbor::NotThis,
                    _ => Neighbor::DontCare,
                };
                i += 1;
            }
        }
        Self {
            neighbors,
            sprites: vec![sprite.to_owned()],
            ..Default::default()
        }
    }

    /// Does the neighbourhood (`is_this(offset)`) match in orientation `t`?
    fn matches(&self, t: TileTransform, is_this: &impl Fn(IVec2) -> bool) -> bool {
        let m = t.matrix();
        self.neighbors.iter().zip(NEIGHBORS).all(|(n, o)| {
            let world = (m * o.as_vec2()).round().as_ivec2();
            match n {
                Neighbor::DontCare => true,
                Neighbor::This => is_this(world),
                Neighbor::NotThis => !is_this(world),
            }
        })
    }
}

/// A `.tile` asset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TileAsset {
    pub kind: TileKind,
    /// Tile: its sprite. Rule Tile: the default sprite. Animated Tile: unused.
    pub sprite: Option<String>,
    /// sRGB tint + alpha.
    pub color: [f32; 4],
    pub collider: ColliderType,
    /// Rule Tile rules, first match wins.
    pub rules: Vec<TilingRule>,
    /// Animated Tile frames.
    pub frames: Vec<String>,
    /// Animated Tile frames per second.
    pub speed: f32,
    /// Animated Tile start offset in seconds.
    pub start_time: f32,
}

impl Default for TileAsset {
    fn default() -> Self {
        Self {
            kind: TileKind::Tile,
            sprite: None,
            color: [1.0; 4],
            collider: ColliderType::Sprite,
            rules: Vec::new(),
            frames: Vec::new(),
            speed: 4.0,
            start_time: 0.0,
        }
    }
}

impl TileAsset {
    /// A plain tile showing `sprite`.
    pub fn sprite(sprite: &str) -> Self {
        Self {
            sprite: Some(sprite.to_owned()),
            ..Default::default()
        }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = crate::vfs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&crate::asset_db::resolve_json(&text)).map_err(|e| e.to_string())
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, crate::asset_db::embed_json(&text)).map_err(|e| e.to_string())
    }

    /// The sprite shown in palettes and pickers.
    pub fn preview_sprite(&self) -> Option<&str> {
        match self.kind {
            TileKind::AnimatedTile => self.frames.first().map(String::as_str),
            _ => self.sprite.as_deref().or_else(|| {
                self.rules
                    .iter()
                    .find_map(|r| r.sprites.first())
                    .map(String::as_str)
            }),
        }
    }

    /// Every sprite the tile can show.
    pub fn sprites(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.sprite.iter().map(String::as_str).collect();
        out.extend(self.frames.iter().map(String::as_str));
        for r in &self.rules {
            out.extend(r.sprites.iter().map(String::as_str));
        }
        out
    }
}

/// What a cell shows right now.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub sprite: Option<String>,
    /// Tile tint × Tilemap colour is applied by the renderer; this is the tile's own.
    pub color: [f32; 4],
    /// Cell transform combined with the matched rule's orientation.
    pub transform: TileTransform,
    pub collider: ColliderType,
    /// The sprite changes over time (drawn on its own, not baked into chunks).
    pub animated: bool,
}

/// Small stable hash of a cell (Random rule output).
fn cell_hash(c: IVec2) -> u32 {
    let mut h = (c.x as u32).wrapping_mul(0x9E37_79B1) ^ (c.y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^ (h >> 12)
}

/// Frame index of an animation with `n` frames at `fps`.
pub fn frame_index(n: usize, fps: f32, time: f32) -> usize {
    if n == 0 {
        return 0;
    }
    let f = (time * fps.max(0.0)).floor();
    (f.max(0.0) as usize) % n
}

/// What the cell `c` of `map` shows at `time` (seconds), when it holds a tile.
pub fn resolve(map: &Tilemap, c: IVec2, time: f32) -> Option<Resolved> {
    let cell = map.get(c)?;
    let asset = get(&cell.tile)?;
    Some(resolve_with(
        &asset,
        &cell.tile,
        map,
        c,
        cell.transform,
        time,
    ))
}

/// [`resolve`] with the asset already loaded.
pub fn resolve_with(
    asset: &TileAsset,
    path: &str,
    map: &Tilemap,
    c: IVec2,
    transform: TileTransform,
    time: f32,
) -> Resolved {
    let base = Resolved {
        sprite: asset.sprite.clone(),
        color: asset.color,
        transform,
        collider: asset.collider,
        animated: false,
    };
    let speed = map.animation_frame_rate.max(0.0);
    match asset.kind {
        TileKind::Tile => base,
        TileKind::AnimatedTile => {
            let t = time * speed + asset.start_time;
            let i = frame_index(asset.frames.len(), asset.speed, t);
            Resolved {
                sprite: asset.frames.get(i).cloned(),
                animated: asset.frames.len() > 1,
                ..base
            }
        }
        TileKind::RuleTile => {
            let is_this = |o: IVec2| map.tile_at(c + o) == Some(path);
            for rule in &asset.rules {
                let Some(t) = rule
                    .transform
                    .candidates()
                    .iter()
                    .find(|t| rule.matches(**t, &is_this))
                else {
                    continue;
                };
                let n = rule.sprites.len();
                let (sprite, animated) = match rule.output {
                    RuleOutput::Single => (rule.sprites.first().cloned(), false),
                    RuleOutput::Random if n > 0 => {
                        (rule.sprites.get(cell_hash(c) as usize % n).cloned(), false)
                    }
                    RuleOutput::Random => (None, false),
                    RuleOutput::Animation => {
                        let i = frame_index(n, rule.animation_speed, time * speed);
                        (rule.sprites.get(i).cloned(), n > 1)
                    }
                };
                return Resolved {
                    sprite,
                    collider: rule.collider,
                    transform: t.then(transform),
                    animated,
                    ..base
                };
            }
            base
        }
    }
}

// ---- asset cache --------------------------------------------------------------------------------

struct Entry {
    checked: Instant,
    stamp: Option<SystemTime>,
    /// Registered in memory (tests, previews): never re-read.
    pinned: bool,
    asset: Option<Arc<TileAsset>>,
    /// [`GENERATION`] when this asset was loaded or registered: what chunks and colliders
    /// that use the tile hash (see [`version`]).
    version: u64,
}

static CACHE: LazyLock<Mutex<HashMap<String, Entry>>> = LazyLock::new(Default::default);
static GENERATION: AtomicU64 = AtomicU64::new(1);

/// Bumped whenever a tile asset is (re)loaded or registered, so cached chunks rebuild.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// The tile asset at a project path (re-read when the file changes; checked twice a second).
pub fn get(rel: &str) -> Option<Arc<TileAsset>> {
    lookup(rel).0
}

/// Version of the tile asset at `rel`: changes when it is re-read, registered or invalidated.
/// Chunk meshes and colliders hash the versions of the tiles they use, not [`generation`]: a
/// tile loaded or registered anywhere (another tilemap's palette, a test running alongside)
/// no longer rebuilds every tilemap's chunks.
pub fn version(rel: &str) -> u64 {
    lookup(rel).1
}

fn lookup(rel: &str) -> (Option<Arc<TileAsset>>, u64) {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    if let Some(e) = cache.get_mut(rel) {
        if e.pinned || now.duration_since(e.checked).as_secs_f32() < 0.5 {
            return (e.asset.clone(), e.version);
        }
        e.checked = now;
        let stamp = crate::vfs::modified(&crate::assets::absolute(rel));
        if stamp == e.stamp {
            return (e.asset.clone(), e.version);
        }
    }
    let abs = crate::assets::absolute(rel);
    let asset = TileAsset::load(&abs).ok().map(Arc::new);
    let version = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    cache.insert(
        rel.to_owned(),
        Entry {
            checked: now,
            stamp: crate::vfs::modified(&abs),
            pinned: false,
            asset: asset.clone(),
            version,
        },
    );
    (asset, version)
}

/// Use `asset` for `rel` without a file (tests, generated content).
pub fn register(rel: &str, asset: TileAsset) {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let version = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    cache.insert(
        rel.to_owned(),
        Entry {
            checked: Instant::now(),
            stamp: None,
            pinned: true,
            asset: Some(Arc::new(asset)),
            version,
        },
    );
}

/// Forget a cached asset (the editor saved a new version).
pub fn invalidate(rel: &str) {
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).remove(rel);
    GENERATION.fetch_add(1, Ordering::Relaxed);
}
