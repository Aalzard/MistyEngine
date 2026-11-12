//! Background loads (round 44): with them on, a frame draws what is ready and the shader
//! variants and textures it needed for the first time arrive from the background queues on
//! later frames; once they have, the image is the one a render without them makes.

use std::time::Duration;

use glam::{Quat, Vec3};

use super::gpu_tests::{headless, read_back, test_lights, test_meshes};
use super::texture::{TextureCache, TextureKind};
use super::*;
use crate::camera::{Camera, Projection};

const WAIT: Duration = Duration::from_secs(180);

fn camera() -> Camera {
    Camera {
        position: Vec3::new(0.0, 7.0, -12.0),
        rotation: Quat::from_rotation_x(28f32.to_radians()),
        projection: Projection::Perspective { fov_y: 50.0 },
        near: 0.3,
        far: 60.0,
    }
}

/// One frame of the test scene into a new target (no history from earlier frames).
fn frame_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut Renderer,
) -> (Vec<u8>, RenderStats) {
    let camera = camera();
    let (meshes, lights) = (test_meshes(), test_lights());
    let environment = Environment::default();
    let frame = Frame {
        camera: &camera,
        grid: None,
        skybox: true,
        background: [0.1; 3],
        environment: &environment,
        post: PostSettings::default(),
        lines: &[],
        overlay: &[],
        meshes: &meshes,
        particles: &[],
        terrains: &[],
        shaded: true,
        lights: &lights,
        water: &[],
    };
    let target = renderer.create_target(device, 320, 200);
    renderer.render(device, queue, &target, &frame);
    (read_back(device, queue, &target), renderer.stats())
}

fn differing(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4).zip(b.chunks(4)).filter(|(x, y)| x != y).count()
}

#[test]
fn queued_shader_variants_swap_in_and_the_image_ends_identical() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut reference = Renderer::with_queue(&device, &queue);
    let (expected, _) = frame_pixels(&device, &queue, &mut reference);
    drop(reference);

    let mut renderer = Renderer::with_queue(&device, &queue);
    renderer.set_background_loads(true);
    // First frame: the variants were only queued, nothing waited for them.
    let (first, stats) = frame_pixels(&device, &queue, &mut renderer);
    assert!(stats.loading > 0, "variants pending: {stats:?}");
    assert!(
        differing(&first, &expected) > 0,
        "the first frame left out what was still compiling"
    );
    assert!(
        crate::load_queue::shaders().wait_idle(WAIT),
        "the shader queue finished"
    );
    // The next frame picks them up; then nothing is pending and the image is the sync one.
    let _ = frame_pixels(&device, &queue, &mut renderer);
    assert_eq!(renderer.background_pending(), 0);
    let (done, stats) = frame_pixels(&device, &queue, &mut renderer);
    assert_eq!(stats.loading, 0, "{stats:?}");
    let n = differing(&done, &expected);
    assert_eq!(
        n, 0,
        "{n} pixels differ from a render without background loads"
    );
}

#[test]
fn a_texture_decoded_in_the_background_arrives_and_a_broken_one_reports_an_error() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let dir = std::env::temp_dir().join(format!("misty_bg_tex_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let good = dir.join("good.png");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 40, 40, 255]))
        .save(&good)
        .unwrap();
    let broken = dir.join("broken.png");
    std::fs::write(&broken, b"not a png").unwrap();
    let (good, broken) = (good.to_string_lossy(), broken.to_string_lossy());

    let mut cache = TextureCache::new(&device);
    cache.set_background(true);
    let layout = Renderer::new(&device).pipelines.material_layout.clone();
    let set = super::texture::TextureSet::pair(Some(&good), Some(&broken));
    cache.prepare_set(&device, &queue, &layout, &set);
    // Bound with the 1×1 stand-ins meanwhile.
    assert!(cache.get_set(&set).is_some());
    assert_eq!(cache.pending(), 2);
    assert!(crate::load_queue::imports().wait_idle(WAIT));
    assert!(cache.take_loaded(), "both finished");
    assert_eq!(cache.pending(), 0);
    // The set is made again with the real texture.
    assert!(cache.get_set(&set).is_none());
    cache.prepare_set(&device, &queue, &layout, &set);
    assert!(cache.is_loaded(&good, TextureKind::Color));
    assert!(!cache.is_loaded(&broken, TextureKind::Normal));
    let warnings = cache.take_warnings();
    assert!(
        warnings
            .iter()
            .any(|w| w.starts_with("Could not load texture") && w.contains("broken.png")),
        "{warnings:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
