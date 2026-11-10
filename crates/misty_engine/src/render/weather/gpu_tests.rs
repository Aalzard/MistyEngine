//! Weather render tests (GPU; skipped without an adapter):
//! - rain and snow passes draw (and a disabled Weather volume changes no pixel);
//! - the occlusion map keeps the floor under a roof free of snow while open ground turns white;
//! - (ignored) GPU timings at 1080p and the docs screenshots of WeatherDemo.

use glam::Vec3;

use crate::quality::QualityLevel;
use crate::render::Renderer;
use crate::render::gpu_tests::{headless, read_back};
use crate::render_scene::{GameView, MaterialSource};
use crate::scene::{
    Camera as CameraComponent, Light, MeshFilter, MeshRenderer, PrimitiveMesh, Scene, Transform,
};
use crate::weather::{Weather, WeatherPreset};

struct NoMaterials;

impl MaterialSource for NoMaterials {
    fn params(&self, _: Option<&str>) -> crate::render::MaterialParams {
        crate::render::MaterialParams::default()
    }
}

fn block(s: &mut Scene, name: &str, kind: PrimitiveMesh, position: Vec3, scale: Vec3) {
    let e = s.spawn(name, None);
    s.set_transform(
        e,
        Transform {
            position,
            scale,
            ..Default::default()
        },
    );
    s.add(e, MeshFilter::primitive(kind));
    s.add(e, MeshRenderer::default());
}

/// Ground, a big roof at 3 m over x < 0, and a camera under the roof's edge looking along +x.
fn shelter_scene(camera: Vec3, euler: Vec3) -> Scene {
    let mut s = Scene::default_3d("WeatherTest");
    let cam = s.find_by_name("Main Camera").unwrap();
    s.set_transform(cam, Transform::from_position(camera).with_euler(euler));
    s.world.get::<&mut CameraComponent>(cam).unwrap().far = 400.0;
    let sun = s.find_by_name("Directional Light").unwrap();
    s.world.get::<&mut Light>(sun).unwrap().intensity = 1.2;
    block(&mut s, "Ground", PrimitiveMesh::Plane, Vec3::ZERO, Vec3::new(10.0, 1.0, 10.0));
    block(
        &mut s,
        "Roof",
        PrimitiveMesh::Cube,
        Vec3::new(-15.0, 3.0, 0.0),
        Vec3::new(30.0, 0.2, 30.0),
    );
    s
}

fn render(device: &wgpu::Device, queue: &wgpu::Queue, scene: &mut Scene, frames: usize, size: (u32, u32), quality: &QualityLevel, weather: bool) -> (Vec<u8>, Renderer) {
    let mut r = Renderer::with_queue(device, queue);
    r.set_quality(device, quality);
    let target = r.create_target(device, size.0, size.1);
    for _ in 0..frames {
        let eye = crate::render_scene::game_camera(scene).unwrap().0.position;
        if weather {
            crate::weather::update_scene(scene, 1.0 / 30.0, eye);
        }
        let view = GameView::new(scene, &NoMaterials).unwrap();
        let frame = view.frame();
        r.render(device, queue, &target, &frame);
    }
    (read_back(device, queue, &target), r)
}

fn mean_diff(a: &[u8], b: &[u8]) -> f32 {
    let total: u64 = a
        .iter()
        .zip(b)
        .enumerate()
        .filter(|(i, _)| i % 4 != 3)
        .map(|(_, (x, y))| u64::from(x.abs_diff(*y)))
        .sum();
    total as f32 / (a.len() as f32 * 0.75)
}

/// Mean brightness of a pixel rectangle (x0..x1, y0..y1).
fn brightness(rgba: &[u8], width: u32, x: (u32, u32), y: (u32, u32)) -> f32 {
    let mut sum = 0.0;
    let mut n = 0.0;
    for yy in y.0..y.1 {
        for xx in x.0..x.1 {
            let i = ((yy * width + xx) * 4) as usize;
            sum += (f32::from(rgba[i]) + f32::from(rgba[i + 1]) + f32::from(rgba[i + 2])) / 3.0;
            n += 1.0;
        }
    }
    sum / n
}

#[test]
fn rain_and_snow_passes_draw_and_off_changes_nothing() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let size = (320, 180);
    let q = QualityLevel::high();
    let make = || shelter_scene(Vec3::new(6.0, 1.7, -8.0), Vec3::new(5.0, 0.0, 0.0));
    let (clear, _) = render(&device, &queue, &mut make(), 3, size, &q, false);
    // A disabled volume: not a pixel changes (weather is off by default for every scene).
    let mut off = make();
    let e = off.spawn("Weather", None);
    off.add(
        e,
        Weather {
            enabled: false,
            ..Weather::with_preset(WeatherPreset::Downpour)
        },
    );
    let (off_img, _) = render(&device, &queue, &mut off, 3, size, &q, true);
    assert_eq!(mean_diff(&clear, &off_img), 0.0, "a disabled Weather volume draws nothing");
    // Rain.
    let mut rain = make();
    let e = rain.spawn("Weather", None);
    rain.add(e, Weather::with_preset(WeatherPreset::Downpour));
    let (rain_img, r) = render(&device, &queue, &mut rain, 8, size, &q, true);
    assert!(r.weather.active(), "the weather passes ran");
    let d = mean_diff(&clear, &rain_img);
    assert!(d > 2.0, "rain changes the picture: {d}");
    assert!(rain_img.chunks(4).all(|p| p[3] == 255));
    // Snow.
    let mut snow = make();
    let e = snow.spawn("Weather", None);
    snow.add(e, Weather::with_preset(WeatherPreset::Blizzard));
    let (snow_img, _) = render(&device, &queue, &mut snow, 8, size, &q, true);
    let d = mean_diff(&clear, &snow_img);
    assert!(d > 4.0, "a blizzard changes the picture: {d}");
    // Brighter overall (white flakes, white haze).
    let b0 = brightness(&clear, size.0, (0, size.0), (0, size.1));
    let b1 = brightness(&snow_img, size.0, (0, size.0), (0, size.1));
    assert!(b1 > b0, "{b1} vs {b0}");
}

#[test]
fn no_snow_settles_under_a_roof() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let size = (320, 180);
    let q = QualityLevel::high();
    // Standing under the roof near its edge (x = 0), looking out and down along +x: the near
    // floor (bottom of the picture) is under the roof, the far floor is open ground.
    let mut s = shelter_scene(Vec3::new(-4.0, 1.6, 0.0), Vec3::new(18.0, 90.0, 0.0));
    let e = s.spawn("Weather", None);
    let mut w = Weather::with_preset(WeatherPreset::Clear);
    w.surfaces.max_snow = 1.0;
    s.add(e, w);
    crate::weather::update_scene(&mut s, 0.1, Vec3::new(-4.0, 1.6, 0.0));
    s.weather.set_surfaces(Some(0.0), Some(1.0));
    // Snow cover is kept (no melting during the frames: melt time is long).
    let (img, _) = render(&device, &queue, &mut s, 4, size, &q, false);
    let under = brightness(&img, size.0, (120, 200), (165, 178));
    let open = brightness(&img, size.0, (120, 200), (85, 100));
    assert!(open > under + 40.0, "open ground white ({open}), sheltered floor not ({under})");
    // Without the occlusion map the sheltered floor is white too.
    let mut s2 = shelter_scene(Vec3::new(-4.0, 1.6, 0.0), Vec3::new(18.0, 90.0, 0.0));
    let e = s2.spawn("Weather", None);
    let mut w = Weather::with_preset(WeatherPreset::Clear);
    w.effects.occlusion = false;
    s2.add(e, w);
    crate::weather::update_scene(&mut s2, 0.1, Vec3::new(-4.0, 1.6, 0.0));
    s2.weather.set_surfaces(Some(0.0), Some(1.0));
    let (img2, _) = render(&device, &queue, &mut s2, 4, size, &q, false);
    let under2 = brightness(&img2, size.0, (120, 200), (165, 178));
    assert!(under2 > under + 40.0, "{under2} vs {under}");
}

fn project() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn weather_demo() -> Option<Scene> {
    let root = project();
    crate::assets::set_project_root(&root);
    let (scene, _) = Scene::load(&root.join("Assets/Scenes/WeatherDemo.scene")).ok()?;
    Some(scene)
}

/// Render WeatherDemo `frames` times advancing the weather; returns the image.
fn render_demo(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    r: &mut Renderer,
    target: &crate::render::RenderTarget,
    scene: &mut Scene,
    frames: usize,
    dt: f32,
) -> Vec<u8> {
    let materials = crate::assets::AssetCache::open(&project());
    for _ in 0..frames {
        let eye = crate::render_scene::game_camera(scene).unwrap().0.position;
        crate::weather::update_scene(scene, dt, eye);
        let view = GameView::new(scene, &materials).unwrap();
        r.render(device, queue, target, &view.frame());
    }
    read_back(device, queue, target)
}

/// A WeatherDemo state for timings and screenshots.
fn demo_state(scene: &mut Scene, preset: WeatherPreset, wet: f32, snow: f32) {
    let e = scene.find_by_name("Weather").unwrap();
    {
        let mut w = scene.world.get::<&mut Weather>(e).unwrap();
        w.timeline.mode = crate::weather::TimelineMode::Off;
        w.state = preset.params();
        w.surfaces.melt_time = 100_000.0;
        w.surfaces.dry_time = 100_000.0;
    }
    scene.weather.reset();
    crate::weather::update_scene(scene, 0.0, Vec3::ZERO);
    scene.weather.set_surfaces(Some(wet), Some(snow));
}

fn night(scene: &mut Scene) {
    let sun = scene.find_by_name("Directional Light").unwrap();
    let mut l = scene.world.get::<&mut Light>(sun).unwrap();
    // Moonlight.
    l.color = [0.55, 0.65, 1.0];
    l.intensity = 0.06;
    scene.render_settings.environment_lighting.intensity = 0.15;
}

/// GPU time of the weather at 1080p on High: Heavy Rain with splashes and wet surfaces, a
/// Thunderstorm, a Blizzard with snow cover; against the same view without weather.
#[test]
#[ignore = "GPU timings; run on purpose in release under the gpu-timing lock"]
fn weather_gpu_timings() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let Some(base) = weather_demo() else {
        eprintln!("WeatherDemo missing: skipped");
        return;
    };
    let cases = [
        ("clear (no weather)", None, 0.0, 0.0),
        ("heavy rain + splashes + wetness", Some(WeatherPreset::HeavyRain), 1.0, 0.0),
        ("downpour", Some(WeatherPreset::Downpour), 1.0, 0.0),
        ("thunderstorm", Some(WeatherPreset::Thunderstorm), 1.0, 0.0),
        ("blizzard + snow cover", Some(WeatherPreset::Blizzard), 0.0, 1.0),
    ];
    for (name, preset, wet, snow) in cases {
        let mut scene = base.clone();
        match preset {
            Some(p) => demo_state(&mut scene, p, wet, snow),
            None => {
                let e = scene.find_by_name("Weather").unwrap();
                scene.world.get::<&mut Weather>(e).unwrap().enabled = false;
            }
        }
        let mut r = Renderer::with_queue(&device, &queue);
        r.set_quality(&device, &QualityLevel::high());
        let target = r.create_target(&device, 1920, 1080);
        render_demo(&device, &queue, &mut r, &target, &mut scene, 20, 1.0 / 60.0);
        let timed = r.set_pass_timing(&device, &queue, true);
        let mut total = 0.0;
        let mut weather = 0.0;
        let mut occlusion = 0.0;
        let mut main = 0.0;
        let n = 30;
        let materials = crate::assets::AssetCache::open(&project());
        for _ in 0..n {
            let eye = crate::render_scene::game_camera(&scene).unwrap().0.position;
            crate::weather::update_scene(&mut scene, 1.0 / 60.0, eye);
            let view = GameView::new(&scene, &materials).unwrap();
            r.render(&device, &queue, &target, &view.frame());
            if timed {
                for &(label, ms) in r.pass_times() {
                    total += ms;
                    match label {
                        "Weather" => weather += ms,
                        "Weather occlusion" => occlusion += ms,
                        l if l.starts_with("Main pass") || l.starts_with("Main:") => main += ms,
                        _ => {}
                    }
                }
            }
        }
        let k = 1.0 / n as f32;
        eprintln!(
            "{name:34} frame {:.2} ms | weather composite {:.3} ms | occlusion {:.3} ms | main pass {:.2} ms",
            total * k,
            weather * k,
            occlusion * k,
            main * k
        );
    }
}

/// The docs screenshots (docs/screenshots/r34/*.jpg) of WeatherDemo.
#[test]
#[ignore = "writes docs/screenshots/r34; run on purpose under the gpu-timing lock"]
fn weather_screenshots() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let Some(base) = weather_demo() else {
        eprintln!("WeatherDemo missing: skipped");
        return;
    };
    let out = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots/r34");
    std::fs::create_dir_all(&out).unwrap();
    let only = std::env::var("MISTY_WEATHER_SHOTS").unwrap_or_default();
    type Setup = fn(&mut Scene);
    let shots: [(&str, WeatherPreset, f32, f32, Option<Setup>); 7] = [
        ("light_rain", WeatherPreset::LightRain, 0.5, 0.0, None),
        ("downpour_mist_splashes", WeatherPreset::Downpour, 1.0, 0.0, None),
        ("thunderstorm_night", WeatherPreset::Thunderstorm, 1.0, 0.0, Some(night)),
        ("wet_street_puddles", WeatherPreset::Clear, 1.0, 0.0, Some(|s: &mut Scene| {
            let cam = s.find_by_name("Main Camera").unwrap();
            s.set_transform(cam, Transform::from_position(Vec3::new(-2.0, 1.2, -6.0)).with_euler(Vec3::new(12.0, 20.0, 0.0)));
        })),
        ("snow_falling", WeatherPreset::Snow, 0.0, 0.35, None),
        ("blizzard", WeatherPreset::Blizzard, 0.0, 0.7, None),
        ("snow_covered", WeatherPreset::Clear, 0.0, 1.0, None),
    ];
    for (name, preset, wet, snow, setup) in shots {
        if !only.is_empty() && !only.split(',').any(|o| name.contains(o)) {
            continue;
        }
        let mut scene = base.clone();
        if let Some(f) = setup {
            f(&mut scene);
        }
        demo_state(&mut scene, preset, wet, snow);
        if name == "wet_street_puddles" {
            let e = scene.find_by_name("Weather").unwrap();
            scene.world.get::<&mut Weather>(e).unwrap().surfaces.puddles = 0.7;
            crate::weather::update_scene(&mut scene, 0.0, Vec3::ZERO);
            scene.weather.set_surfaces(Some(1.0), Some(0.0));
        }
        let mut r = Renderer::with_queue(&device, &queue);
        r.set_quality(&device, &QualityLevel::high());
        let target = r.create_target(&device, 1600, 900);
        let mut img = render_demo(&device, &queue, &mut r, &target, &mut scene, 40, 1.0 / 60.0);
        if name == "thunderstorm_night" {
            // A strike in view, caught at its first flash.
            let cam = crate::render_scene::game_camera(&scene).unwrap().0;
            let at = cam.position + cam.forward() * 900.0 + cam.right() * 250.0;
            scene.weather.strike(Vec3::new(at.x, cam.position.y, at.z), 1.0);
            img = render_demo(&device, &queue, &mut r, &target, &mut scene, 2, 0.005);
        }
        let rgba = image::RgbaImage::from_raw(1600, 900, img).unwrap();
        let rgb = image::DynamicImage::ImageRgba8(rgba).to_rgb8();
        let path = out.join(format!("{name}.jpg"));
        let file = std::fs::File::create(&path).unwrap();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), 88);
        enc.encode_image(&rgb).unwrap();
        eprintln!("wrote {}", path.display());
    }
}
