//! GPU checks of Decal Projectors on meshes and terrain (skipped without an adapter).
//! `MISTY_DECAL_TEST_DIR=dir` saves the images.

use glam::{Mat4, Quat, Vec3};

use super::DecalDraw;
use crate::camera::{Camera, Projection};
use crate::render::gpu_tests::{headless, read_back, test_lights, test_meshes};
use crate::render::{Environment, Frame, MeshDraw, PostSettings, Renderer};
use crate::scene::DecalProjector;
use crate::terrain::{TerrainData, TerrainDraw, store};

const W: u32 = 320;
const H: u32 = 200;

fn camera() -> Camera {
    Camera {
        position: Vec3::new(0.0, 7.0, -12.0),
        rotation: Quat::from_rotation_x(28f32.to_radians()),
        projection: Projection::Perspective { fov_y: 50.0 },
        near: 0.3,
        far: 80.0,
    }
}

fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    decals: Vec<DecalDraw>,
    meshes: &[MeshDraw],
    terrains: &[TerrainDraw],
) -> Vec<u8> {
    let cam = camera();
    let lights = test_lights();
    let environment = Environment {
        decals,
        ..Default::default()
    };
    let frame = Frame {
        camera: &cam,
        grid: None,
        skybox: true,
        background: [0.1; 3],
        environment: &environment,
        post: PostSettings::default(),
        lines: &[],
        overlay: &[],
        meshes,
        particles: &[],
        terrains,
        shaded: true,
        lights: &lights,
        water: &[],
    };
    let mut renderer = Renderer::with_queue(device, queue);
    let target = renderer.create_target(device, W, H);
    renderer.render(device, queue, &target, &frame);
    renderer.render(device, queue, &target, &frame);
    let px = read_back(device, queue, &target);
    assert!(renderer.take_warnings().is_empty());
    px
}

fn save(name: &str, px: &[u8]) {
    if let Some(dir) = std::env::var_os("MISTY_DECAL_TEST_DIR") {
        let img = image::RgbaImage::from_raw(W, H, px.to_vec()).expect("size");
        img.save(std::path::Path::new(&dir).join(name))
            .expect("png");
    }
}

/// A projector 1.5 m above `at` pointing straight down, `size` wide (its 3 m deep box is
/// centred on `at`).
fn down(at: Vec3, size: f32, decal: DecalProjector) -> DecalDraw {
    let above = at + Vec3::Y * 1.5;
    DecalDraw {
        object: Mat4::from_rotation_translation(Quat::from_rotation_x(90f32.to_radians()), above),
        decal: DecalProjector {
            size: [size, size, 3.0],
            pivot: [0.0, 0.0, 1.5],
            ..decal
        },
    }
}

/// The pixel (x, y) a world point lands on.
fn pixel(p: Vec3) -> (u32, u32) {
    let (uv, _) = camera()
        .world_to_screen(W as f32 / H as f32, p)
        .expect("in view");
    ((uv.x * W as f32) as u32, (uv.y * H as f32) as u32)
}

fn at(px: &[u8], (x, y): (u32, u32)) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [px[i], px[i + 1], px[i + 2]]
}

fn changed(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4)
        .zip(b.chunks(4))
        .filter(|(p, q)| p.iter().zip(*q).any(|(x, y)| x.abs_diff(*y) > 8))
        .count()
}

/// Ground only, so the decal's footprint is easy to find.
fn ground() -> Vec<MeshDraw> {
    test_meshes().into_iter().take(1).collect()
}

fn solid(color: [f32; 4]) -> DecalProjector {
    DecalProjector {
        color,
        ..Default::default()
    }
}

#[test]
fn decals_paint_meshes_in_priority_order() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let meshes = ground();
    let plain = render(&device, &queue, Vec::new(), &meshes, &[]);
    // A disabled decal changes nothing (not even the prepass it would need).
    let off = DecalProjector {
        enabled: false,
        ..solid([0.0, 0.0, 1.0, 1.0])
    };
    assert!(
        render(
            &device,
            &queue,
            vec![down(Vec3::ZERO, 3.0, off)],
            &meshes,
            &[]
        ) == plain
    );
    let centre = Vec3::new(0.0, 0.0, 0.0);
    let blue = render(
        &device,
        &queue,
        vec![down(centre, 3.0, solid([0.1, 0.2, 1.0, 1.0]))],
        &meshes,
        &[],
    );
    save("decal_blue.png", &blue);
    let (before, after) = (at(&plain, pixel(centre)), at(&blue, pixel(centre)));
    assert!(before[0] > before[2], "red ground {before:?}");
    assert!(after[2] > after[0], "blue decal {after:?}");
    // Only its footprint changes: a 3 m square, a few thousand pixels, not the screen.
    let n = changed(&plain, &blue);
    assert!(n > 300 && n < (W * H / 4) as usize, "{n} pixels");
    // Outside the box the ground keeps its colour.
    let outside = Vec3::new(4.0, 0.0, 0.0);
    assert_eq!(at(&plain, pixel(outside)), at(&blue, pixel(outside)));
    // Half opacity lands between the two.
    let half = render(
        &device,
        &queue,
        vec![down(centre, 3.0, solid([0.1, 0.2, 1.0, 0.5]))],
        &meshes,
        &[],
    );
    let mid = at(&half, pixel(centre));
    assert!(mid[2] > before[2] && mid[2] < after[2], "{mid:?}");
    // Overlapping: the higher priority is on top whatever the list order.
    let green = DecalProjector {
        priority: 1,
        ..solid([0.1, 1.0, 0.1, 1.0])
    };
    for order in [false, true] {
        let mut list = vec![
            down(centre, 3.0, solid([0.1, 0.2, 1.0, 1.0])),
            down(centre + Vec3::X, 3.0, green.clone()),
        ];
        if order {
            list.reverse();
        }
        let both = render(&device, &queue, list, &meshes, &[]);
        let c = at(&both, pixel(centre + Vec3::X * 0.5));
        assert!(c[1] > c[2] && c[1] > c[0], "green on top {c:?}");
    }
    // Angle fade: the ground faces the projector (0°), so a fade from 10° to 20° keeps it,
    // a projector tilted 60° loses it.
    let tilted = DecalDraw {
        object: Mat4::from_rotation_translation(
            Quat::from_rotation_x(30f32.to_radians()),
            Vec3::new(0.0, 0.5, 0.0),
        ),
        decal: DecalProjector {
            size: [3.0, 3.0, 3.0],
            pivot: [0.0, 0.0, 0.0],
            angle_fade: [10.0, 20.0],
            ..solid([0.1, 0.2, 1.0, 1.0])
        },
    };
    let faded = render(&device, &queue, vec![tilted], &meshes, &[]);
    assert!(changed(&plain, &faded) < 20);
}

#[test]
fn decal_textures_and_surface_maps() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let dir = std::env::temp_dir().join(format!("misty_decal_gpu_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // A white disc with a transparent outside.
    let disc = image::RgbaImage::from_fn(64, 64, |x, y| {
        let d = ((x as f32 - 31.5).powi(2) + (y as f32 - 31.5).powi(2)).sqrt();
        image::Rgba([255, 255, 255, if d < 24.0 { 255 } else { 0 }])
    });
    let base = dir.join("disc.png");
    disc.save(&base).unwrap();
    // A strongly tilted normal map.
    let normal = dir.join("normal.png");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([230, 128, 160, 255]))
        .save(&normal)
        .unwrap();
    let meshes = ground();
    let plain = render(&device, &queue, Vec::new(), &meshes, &[]);
    let textured = DecalProjector {
        base_map: Some(base.to_string_lossy().into_owned()),
        affects_normal: false,
        affects_mask: false,
        ..solid([0.1, 0.2, 1.0, 1.0])
    };
    let image = render(
        &device,
        &queue,
        vec![down(Vec3::ZERO, 3.0, textured)],
        &meshes,
        &[],
    );
    save("decal_disc.png", &image);
    // Inside the disc blue, the box corners (transparent texels) untouched.
    assert!(at(&image, pixel(Vec3::ZERO))[2] > at(&plain, pixel(Vec3::ZERO))[2] + 40);
    let corner = Vec3::new(1.4, 0.0, 1.4);
    assert_eq!(at(&image, pixel(corner)), at(&plain, pixel(corner)));
    // Normal only: the colour stays, the shading changes.
    let bumpy = DecalProjector {
        normal_map: Some(normal.to_string_lossy().into_owned()),
        affects_albedo: false,
        affects_mask: false,
        ..Default::default()
    };
    let lit = render(
        &device,
        &queue,
        vec![down(Vec3::ZERO, 3.0, bumpy)],
        &meshes,
        &[],
    );
    save("decal_normal.png", &lit);
    assert!(
        changed(&plain, &lit) > 300,
        "the normal relights the ground"
    );
    // Mask only: a smooth metal patch.
    let metal = DecalProjector {
        metallic: 1.0,
        smoothness: 0.95,
        affects_albedo: false,
        affects_normal: false,
        ..Default::default()
    };
    let shiny = render(
        &device,
        &queue,
        vec![down(Vec3::ZERO, 3.0, metal)],
        &meshes,
        &[],
    );
    assert!(
        changed(&plain, &shiny) > 300,
        "metallic and smoothness change the shading"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn decals_paint_terrain() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let key = std::env::temp_dir()
        .join(format!(
            "misty_decal_terrain_{}.terrain",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned();
    // A gentle slope, no grass.
    let mut data = TerrainData::new(65, Vec3::new(40.0, 4.0, 40.0), 64);
    for z in 0..data.heightmap_resolution {
        for x in 0..data.heightmap_resolution {
            data.set_height(x, z, x as f32 / 64.0 * 0.3);
        }
    }
    store::insert(&key, data);
    let (data, version) = store::get(&key).expect("inserted");
    let terrains = [TerrainDraw {
        key: key.clone(),
        version,
        data,
        position: Vec3::new(-20.0, -1.0, -20.0),
        cast_shadows: true,
    }];
    let plain = render(&device, &queue, Vec::new(), &[], &terrains);
    let decal = down(Vec3::new(0.0, -0.5, 0.0), 3.0, solid([1.0, 0.9, 0.1, 1.0]));
    let painted = render(&device, &queue, vec![decal], &[], &terrains);
    save("decal_terrain.png", &painted);
    let n = changed(&plain, &painted);
    assert!(n > 300 && n < (W * H / 4) as usize, "{n} pixels");
    let c = at(&painted, pixel(Vec3::new(0.0, -0.5, 0.0)));
    assert!(
        c[0] > 120 && c[1] > 100 && c[2] < c[1],
        "yellow on the terrain {c:?}"
    );
}
