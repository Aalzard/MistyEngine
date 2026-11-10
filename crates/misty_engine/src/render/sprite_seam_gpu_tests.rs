//! Headless render of tiles that share an atlas edge (skipped without an adapter): with MSAA a
//! pixel on the edge between two quads was shaded at its centre for the left quad too, where
//! that quad's uv reaches past its atlas cell into the next one, so a column of the next cell
//! showed through as a seam (the stone walls of the 2D Platformer template, whose next cell
//! is a torch with a transparent first column). Unlit draws sample at the centroid of the
//! covered samples now.

use glam::{Mat4, Quat, Vec3};

use crate::camera::{Camera, Projection};
use crate::render::gpu_tests::{headless, read_back};
use crate::render::{Environment, Frame, MaterialParams, MeshDraw, PostSettings, RenderingMode};
use crate::scene::PrimitiveMesh;

const W: u32 = 128;
const H: u32 = 32;
/// Tile width in pixels (and world units: one unit per pixel).
const TILE: f32 = 23.0;
/// Left edge of the first tile, a quarter pixel into column 10: each seam then lies a
/// quarter pixel into a column, which only one of the four MSAA samples of the left tile
/// covers.
const LEFT: f32 = 10.25;

fn tile(i: usize, texture: &str) -> MeshDraw {
    let x = LEFT + TILE * (i as f32 + 0.5) - W as f32 / 2.0;
    MeshDraw {
        mesh: PrimitiveMesh::Quad.into(),
        model: Mat4::from_scale_rotation_translation(
            Vec3::new(TILE, 16.0, 1.0),
            Quat::IDENTITY,
            Vec3::new(x, 0.0, 0.0),
        ),
        material: MaterialParams {
            color: [1.0; 4],
            albedo_map: Some(texture.to_owned()),
            // The left half of the atlas: one cell.
            tiling: [0.5, 1.0],
            offset: [0.0, 0.0],
            unlit: true,
            mode: RenderingMode::Fade,
            double_sided: true,
            ..Default::default()
        },
        cast_shadows: false,
        receive_shadows: false,
        outline: false,
        sorting: Default::default(),
        lod: None,
    }
}

#[test]
fn tiles_sharing_an_atlas_edge_show_no_seam_with_msaa() {
    let Some((device, queue)) = headless() else {
        return;
    };
    // A two-cell atlas: red cell, blue cell. Fine texels (about 22 per pixel): a centre
    // sample's overshoot lands ~5 texels into the blue cell, the centroid ~3 texels inside
    // the red one, both beyond the reach of bilinear filtering. As many texels per pixel
    // down as across (356 rows over the 16 px tall tiles), so the default anisotropic
    // sampler filters isotropically (an anisotropic footprint reaches across the edge).
    let texture = "runtime://sprite-seam-atlas";
    let image = image::RgbaImage::from_fn(1024, 356, |x, _| {
        if x < 512 {
            image::Rgba([255, 0, 0, 255])
        } else {
            image::Rgba([0, 0, 255, 255])
        }
    });
    crate::video::textures::publish(texture, &image);
    let cam = Camera {
        position: Vec3::new(0.0, 0.0, -10.0),
        rotation: Quat::IDENTITY,
        projection: Projection::Orthographic {
            size: H as f32 / 2.0,
        },
        near: 0.1,
        far: 100.0,
    };
    let mut env = Environment::default();
    env.settings.environment_lighting.intensity = 0.0;
    let meshes: Vec<MeshDraw> = (0..4).map(|i| tile(i, texture)).collect();
    let frame = Frame {
        camera: &cam,
        grid: None,
        skybox: false,
        background: [0.0; 3],
        environment: &env,
        post: PostSettings {
            bloom_intensity: 0.0,
            ..Default::default()
        },
        lines: &[],
        overlay: &[],
        meshes: &meshes,
        particles: &[],
        terrains: &[],
        water: &[],
        shaded: true,
        lights: &[],
    };
    let mut r = crate::render::Renderer::with_queue(&device, &queue);
    let target = r.create_target(&device, W, H);
    // A couple of frames: runtime textures upload on the first.
    for _ in 0..3 {
        r.render(&device, &queue, &target, &frame);
    }
    let image = read_back(&device, &queue, &target);
    let right = LEFT + 4.0 * TILE;
    let mut bad = Vec::new();
    for y in (H / 2 - 6)..(H / 2 + 6) {
        for x in (LEFT.ceil() as u32)..(right.floor() as u32) {
            let i = ((y * W + x) * 4) as usize;
            let (red, blue) = (image[i], image[i + 2]);
            if red < 150 || blue > 24 {
                bad.push((x, y, red, blue));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{} pixels show the next atlas cell, first (x, y, r, b): {:?}",
        bad.len(),
        &bad[..bad.len().min(8)]
    );
}
