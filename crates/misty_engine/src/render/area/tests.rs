use glam::{DVec3, Vec2, Vec3};

use super::ggx::Ggx;
use super::ltc_fit::{self, SIZE};
use super::*;
use crate::scene::Shadows;

fn rect(position: Vec3, right: Vec3, up: Vec3, size: Vec2, two_sided: bool) -> LightData {
    LightData {
        kind: LightType::Rectangle,
        position,
        direction: right.cross(up),
        color: [1.0; 3],
        intensity: 1.0,
        range: 100.0,
        spot_angle: 0.0,
        shadows: Shadows::None,
        shadow: Default::default(),
        volumetric: Default::default(),
        contact_shadows: false,
        area: AreaShape {
            size,
            right,
            up,
            two_sided,
            show_emitter: false,
            glow: Default::default(),
        },
    }
}

/// Brute force ∫ g(l) dω over the rectangle, by a grid over its area (f64).
fn over_rect(l: &LightData, p: Vec3, g: impl Fn(DVec3) -> f64) -> f64 {
    let (hx, hy) = l.area.half_axes();
    let (c, hx, hy) = (l.position.as_dvec3(), hx.as_dvec3(), hy.as_dvec3());
    let normal = hx.cross(hy).normalize();
    let area = 4.0 * hx.length() * hy.length();
    let n = 400;
    let mut sum = 0.0;
    for k in 0..n * n {
        let s = ((k % n) as f64 + 0.5) / n as f64 * 2.0 - 1.0;
        let t = ((k / n) as f64 + 0.5) / n as f64 * 2.0 - 1.0;
        let q = c + hx * s + hy * t;
        let d = q - p.as_dvec3();
        let dist2 = d.length_squared();
        let w = d / dist2.sqrt();
        // dω = cos(at the light) dA / d².
        let cos_light = w.dot(normal).abs();
        sum += g(w) * cos_light / dist2;
    }
    sum * area / (n * n) as f64
}

#[test]
fn diffuse_integral_is_the_form_factor() {
    let l = rect(
        Vec3::new(0.3, 2.0, -0.2),
        Vec3::X,
        Vec3::Z,
        Vec2::new(1.5, 0.6),
        false,
    );
    // right × up = X × Z = -Y: it shines down onto the floor.
    for p in [
        Vec3::ZERO,
        Vec3::new(1.2, 0.0, 0.5),
        Vec3::new(-2.0, 0.0, 1.0),
    ] {
        let n = Vec3::Y;
        let (diffuse, _, _) = integrals(&l, p, n, Vec3::new(0.0, 1.0, 1.0).normalize(), 0.5);
        let reference = over_rect(&l, p, |w| w.dot(DVec3::Y).max(0.0) / std::f64::consts::PI);
        assert!(
            (diffuse as f64 - reference).abs() < 2e-3 + 0.01 * reference,
            "{p:?}: {diffuse} vs {reference}"
        );
    }
}

#[test]
fn one_sided_lights_are_dark_behind_and_clip_to_the_horizon() {
    let mut l = rect(Vec3::new(0.0, 2.0, 0.0), Vec3::X, Vec3::Z, Vec2::ONE, false);
    let v = Vec3::Y;
    // Above the light (behind its face): nothing; two-sided lights shine both ways.
    let above = Vec3::new(0.0, 4.0, 0.0);
    assert_eq!(integrals(&l, above, Vec3::NEG_Y, v, 0.5).0, 0.0);
    l.area.two_sided = true;
    assert!(integrals(&l, above, Vec3::NEG_Y, v, 0.5).0 > 0.01);
    // A wall half under the light sees only the part above its horizon.
    let wall = Vec3::new(0.0, 2.0, 1.5);
    let (half, _, _) = integrals(&l, wall, Vec3::NEG_Z, Vec3::NEG_Z, 0.5);
    let reference = over_rect(&l, wall, |w| {
        w.dot(DVec3::NEG_Z).max(0.0) / std::f64::consts::PI
    });
    assert!(
        (half as f64 - reference).abs() < 2e-3,
        "{half} vs {reference}"
    );
}

/// The table's lobe × its magnitude against brute-force GGX over rectangles.
#[test]
fn ltc_specular_matches_ggx() {
    let cases = [
        (0.35, Vec3::new(0.0, 1.0, -1.0)),
        (0.6, Vec3::new(0.0, 1.0, -0.4)),
        (0.9, Vec3::new(0.5, 1.0, 0.8)),
    ];
    let l = rect(
        Vec3::new(0.0, 1.5, 1.2),
        Vec3::X,
        Vec3::Z,
        Vec2::new(2.0, 1.0),
        false,
    );
    let p = Vec3::ZERO;
    let n = Vec3::Y;
    for (perceptual, view) in cases {
        let v = view.normalize();
        let (_, spec, [scale, bias]) = integrals(&l, p, n, v, perceptual);
        let ltc = spec as f64 * (scale + bias) as f64;
        // GGX in the shading frame (normal = +Z).
        let frame = polygon::shading_frame(n, v).as_dmat3();
        let brdf = Ggx {
            alpha: (perceptual as f64 * perceptual as f64).max(ltc_fit::MIN_ALPHA),
        };
        let vl = frame * v.as_dvec3();
        let reference = over_rect(&l, p, |w| brdf.eval(vl, frame * w).0);
        let err = (ltc - reference).abs() / reference;
        assert!(err < 0.12, "roughness {perceptual}: {ltc} vs {reference}");
    }
}

#[test]
fn table_magnitudes_are_directional_albedos() {
    // Smooth GGX at normal incidence reflects almost everything; rough grazing much less.
    let smooth = table::cell(SIZE / 4, 0);
    assert!(smooth.scale + smooth.bias > 0.9, "{smooth:?}");
    // Rough lobes agree with a plain uniform-hemisphere estimate of ∫ f·cos.
    for j in [0, SIZE / 2, SIZE - 8] {
        let rough = table::cell(SIZE - 1, j);
        let brdf = Ggx { alpha: 1.0 };
        let v = ltc_fit::view_at(j);
        let n = 600;
        let mut sum = 0.0;
        for k in 0..n * n {
            let z = ((k % n) as f64 + 0.5) / n as f64;
            let phi = ((k / n) as f64 + 0.5) / n as f64 * std::f64::consts::TAU;
            let r = (1.0 - z * z).sqrt();
            sum += brdf.eval(v, DVec3::new(r * phi.cos(), r * phi.sin(), z)).0;
        }
        let uniform = sum * std::f64::consts::TAU / (n * n) as f64;
        let table = rough.scale + rough.bias;
        assert!(
            (table - uniform).abs() < 0.02,
            "row {j}: {table} vs {uniform}"
        );
    }
    // Rough lobes at normal incidence are close to a cosine.
    let c = table::cell(SIZE - 1, 0);
    assert!((c.ltc.a - 1.0).abs() < 0.3 && c.ltc.e > 0.5, "{c:?}");
}

/// The committed cells are what the fitter produces from their neighbours.
#[test]
fn ltc_table_matches_its_generator() {
    assert_eq!(
        table::texels().len(),
        2 * SIZE * SIZE,
        "regenerate with write_ltc_table"
    );
    for (i, j) in [(10, 1), (40, 20), (63, 50)] {
        let start = table::cell(i, j - 1).ltc;
        let fitted = ltc_fit::fit_cell(i, j, start);
        let stored = table::cell(i, j);
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-4 * a.abs().max(1.0);
        let (f, s) = (fitted.ltc, stored.ltc);
        assert!(
            close(f.a, s.a) && close(f.b, s.b) && close(f.d, s.d) && close(f.e, s.e),
            "cell {i},{j}: {f:?} vs {s:?}"
        );
        assert!(close(fitted.scale, stored.scale) && close(fitted.bias, stored.bias));
    }
}

/// `cargo test --release -p misty_engine -- --ignored write_ltc_table`
#[test]
#[ignore = "fits the whole LTC table (about a minute in release) and rewrites ltc_ggx.bin"]
fn write_ltc_table() {
    let started = std::time::Instant::now();
    let cells = table::build();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/render/area")
        .join(table::FILE);
    std::fs::write(&path, table::encode(&cells)).unwrap();
    eprintln!("LTC table fitted in {:.1?}", started.elapsed());
}

#[test]
fn emitter_quads_face_the_lit_side() {
    let mut l = rect(
        Vec3::new(0.0, 3.0, 0.0),
        Vec3::X,
        Vec3::Z,
        Vec2::new(2.0, 0.5),
        false,
    );
    assert!(emitter_draws(&[l]).is_empty(), "off by default");
    l.area.show_emitter = true;
    let draws = emitter_draws(&[l]);
    assert_eq!(draws.len(), 1);
    let d = &draws[0];
    // The quad's normal (-Z) ends up along the light's forward axis (-Y here).
    let normal = d.model.inverse().transpose().transform_vector3(Vec3::NEG_Z);
    assert!(normal.normalize().dot(Vec3::NEG_Y) > 0.999, "{normal:?}");
    let corner = d.model.transform_point3(Vec3::new(0.5, 0.5, 0.0));
    assert!((corner.x.abs() - 1.0).abs() < 1e-5 && (corner.z.abs() - 0.25).abs() < 1e-5);
    assert!(d.material.emission_intensity == 1.0 && !d.cast_shadows);
}
