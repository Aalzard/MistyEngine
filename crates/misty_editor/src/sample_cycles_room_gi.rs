//! Misty GI in the Cycles room: comparison renders against the path-traced lightmap (the
//! engine's "Cycles-like" reference) and the shipped realtime look (baked room probes + SSGI).
//! Ignored; run on purpose:
//! `cargo test -p misty_editor render_cycles_room_misty_gi -- --ignored --nocapture`
//!
//! Writes `docs/screenshots/gfx/gi_cycles_room_*.png`: the white room (probes / Misty GI /
//! Misty GI + SSGI / baked) and a variant with a red left and a green right wall (SSGI only /
//! Misty GI / baked) for colour bleeding.

use misty_engine::render::MistyGiSettings;
use misty_engine::scene::MistyGlobalIlluminationOverride;

use super::*;

const RED: &str = "Assets/Materials/CyclesRoom/Red Wall.mat";
const GREEN: &str = "Assets/Materials/CyclesRoom/Green Wall.mat";

/// The demo's materials plus a red and a green wall (test only, never written).
struct ColourMaterials;

impl MaterialSource for ColourMaterials {
    fn params(&self, rel: Option<&str>) -> MaterialParams {
        let tint = |c: [f32; 3]| MaterialParams {
            color: [c[0], c[1], c[2], 1.0],
            smoothness: 0.1,
            ..Default::default()
        };
        match rel {
            Some(RED) => tint([0.8, 0.1, 0.08]),
            Some(GREEN) => tint([0.12, 0.7, 0.12]),
            _ => DemoMaterials.params(rel),
        }
    }
}

/// `s` with the left wall red and the right wall green.
fn coloured(s: Scene) -> Scene {
    for (name, mat) in [("Left Wall", RED), ("Right Wall", GREEN)] {
        let e = s.find_by_name(name).expect("wall");
        let mut renderer = s.world.get::<&mut MeshRenderer>(e).expect("renderer");
        renderer.material = Some(mat.into());
    }
    s
}

/// Misty GI (High) on, SSGI as asked.
fn misty(view: &mut GameView, ssgi: bool) {
    view.post.misty_gi = MistyGiSettings::from_override(&MistyGlobalIlluminationOverride {
        active: true,
        ..Default::default()
    });
    view.post.ssgi.enabled = ssgi;
}

fn shot(gpu: &Gpu, name: &str, view: &GameView, frames: u32) {
    let rgba = render_rgba(gpu, view, frames);
    patches(name, &rgba);
    save_png(
        &screenshots().join(format!("gi_cycles_room_{name}.png")),
        rgba,
    );
}

#[test]
#[ignore = "writes comparison renders into docs/screenshots/gfx; run on purpose"]
fn render_cycles_room_misty_gi() {
    let gpu = crate::sample_render::headless().expect("a GPU adapter");
    let empty = lightmap::LightmapSet::default();

    // The shipped realtime look: room probes (baked) + SSGI.
    let realtime = demo_scene();
    let probe = bake_probe(&gpu, &realtime, &empty);
    shot(&gpu, "probes", &view_of(&realtime, &empty, &probe), 24);

    // Misty GI instead of the probes' diffuse, with the baked variant's exposure and AO (the
    // same post-processing as the reference). The probes still give the floor its reflection.
    let unbaked = scene(Variant::Baked);
    let mut view = view_of(&unbaked, &empty, &probe);
    misty(&mut view, false);
    shot(&gpu, "misty", &view, 60);
    misty(&mut view, true);
    shot(&gpu, "misty_ssgi", &view, 60);

    // Reference: bounce light path-traced into the lightmap.
    let baked = baked_scene();
    let maps = lightmap_set(&bake_lightmaps());
    let baked_probe = bake_probe(&gpu, &baked, &maps);
    shot(&gpu, "baked", &view_of(&baked, &maps, &baked_probe), 24);

    // Colour bleeding: red left wall, green right wall.
    let colour = coloured(scene(Variant::Baked));
    let mut view = GameView::new(&colour, &ColourMaterials).expect("camera");
    shot(&gpu, "color_ssgi", &view, 24);
    misty(&mut view, false);
    shot(&gpu, "color_misty", &view, 60);
    let input = bake::input::gather(&colour, &ColourMaterials);
    let colour_maps = lightmap_set(&bake::bake(&input, &bake::Progress::default()).unwrap().maps);
    let colour_baked = coloured(baked_scene());
    let mut view = GameView::new(&colour_baked, &ColourMaterials).expect("camera");
    view.environment.lightmaps = colour_maps;
    shot(&gpu, "color_baked", &view, 24);
}
