//! Built-in packages: the engine's feature modules. They are always there (they can't be
//! removed); the Packages window lists them with their versions, and other packages can depend
//! on them by name.

use std::collections::BTreeMap;

use super::manifest::PackageJson;

/// (name, display name, description).
const MODULES: [(&str, &str, &str); 18] = [
    (
        "com.misty.modules.animation",
        "Animation",
        "Animation clips, Animator state machines, rigging and skinning.",
    ),
    (
        "com.misty.modules.asset-groups",
        "Asset Groups",
        "Load assets by address or label; bundles, catalogs and remote content.",
    ),
    (
        "com.misty.modules.audio",
        "Audio",
        "Audio Sources, Listeners and Mixers.",
    ),
    (
        "com.misty.modules.lighting",
        "Lighting",
        "Lights, lightmaps, light probes, reflection probes and occlusion culling.",
    ),
    (
        "com.misty.modules.mesh-builder",
        "Mesh Builder",
        "Model and edit meshes inside the editor.",
    ),
    (
        "com.misty.modules.navigation",
        "Navigation",
        "Nav mesh baking and Nav Mesh Agents.",
    ),
    (
        "com.misty.modules.particles",
        "Particle System",
        "CPU particle systems.",
    ),
    (
        "com.misty.modules.physics",
        "Physics",
        "3D rigid bodies, colliders, joints and character controllers.",
    ),
    (
        "com.misty.modules.physics2d",
        "Physics 2D",
        "2D rigid bodies and colliders.",
    ),
    (
        "com.misty.modules.scripting",
        "Lua Scripting",
        "Lua 5.4 behaviours with a component-style lifecycle.",
    ),
    (
        "com.misty.modules.shader-graph",
        "Shader Graph",
        "Build shaders from nodes.",
    ),
    (
        "com.misty.modules.splines",
        "Splines",
        "Spline paths, extrusion and animation along splines.",
    ),
    (
        "com.misty.modules.terrain",
        "Terrain",
        "Terrains, the World Builder and trees.",
    ),
    (
        "com.misty.modules.tilemap",
        "Tilemap",
        "2D tilemaps, tiles and palettes.",
    ),
    (
        "com.misty.modules.timeline",
        "Timeline",
        "Sequenced animation, audio and activation tracks.",
    ),
    (
        "com.misty.modules.ui",
        "UI",
        "Canvases, text, images, buttons and layout.",
    ),
    (
        "com.misty.modules.vfx",
        "Effect Graph",
        "GPU visual effects built from graphs.",
    ),
    (
        "com.misty.modules.water",
        "Water",
        "Water surfaces with waves and buoyancy.",
    ),
];

/// Every built-in package at the engine's version.
pub fn all() -> Vec<PackageJson> {
    MODULES
        .iter()
        .map(|(name, title, desc)| PackageJson {
            name: (*name).into(),
            display_name: (*title).into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: (*desc).into(),
            dependencies: BTreeMap::new(),
            author: super::manifest::Author::Name("Misty Engine".into()),
            license: "GPL-3.0-or-later with the Misty Engine runtime exception".into(),
            samples: Vec::new(),
        })
        .collect()
}

pub fn is_builtin(name: &str) -> bool {
    MODULES.iter().any(|(n, _, _)| *n == name)
}
