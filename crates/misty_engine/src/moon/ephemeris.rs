//! Where the sun and moon are for a date, time and place: low-precision ephemerides after
//! Meeus, *Astronomical Algorithms* (2nd ed.): the sun from chapter 25 (≈ 0.01°), the moon
//! from the main terms of chapter 47 (≈ 0.05°, enough for a sky), the moon's parallax for
//! the observer (up to 1°, it matters for eclipses and the phase), sidereal time (chapter 12)
//! and the equatorial → horizontal conversion (chapter 13).
//!
//! World axes: +Z north, +X east, +Y up (the engine's left-handed, Unity-like frame).
//! Times are treated as both UT and TT (ΔT, about a minute, moves the moon by 0.01°).

use glam::{DVec3, Vec3};

/// Julian Day of a calendar date (Gregorian) at `hours` UT.
pub fn julian_day(year: i32, month: u32, day: u32, hours: f64) -> f64 {
    let (mut y, mut m) = (year as f64, month as f64);
    if month <= 2 {
        y -= 1.0;
        m += 12.0;
    }
    let a = (y / 100.0).floor();
    let b = 2.0 - a + (a / 4.0).floor();
    (365.25 * (y + 4716.0)).floor() + (30.6001 * (m + 1.0)).floor() + day as f64 + b - 1524.5
        + hours / 24.0
}

/// Julian centuries since J2000.0.
fn centuries(jd: f64) -> f64 {
    (jd - 2_451_545.0) / 36_525.0
}

fn norm_deg(a: f64) -> f64 {
    a.rem_euclid(360.0)
}

fn sin_d(a: f64) -> f64 {
    a.to_radians().sin()
}

fn cos_d(a: f64) -> f64 {
    a.to_radians().cos()
}

/// Mean obliquity of the ecliptic (degrees).
fn obliquity(t: f64) -> f64 {
    23.439_291_1 - 0.013_004_2 * t
}

/// Right ascension and declination (degrees).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Equatorial {
    pub ra: f64,
    pub dec: f64,
    /// Distance from the Earth's centre in km (0 = very far).
    pub distance_km: f64,
}

fn ecliptic_to_equatorial(lambda: f64, beta: f64, t: f64) -> (f64, f64) {
    let e = obliquity(t);
    let ra = (sin_d(lambda) * cos_d(e) - beta.to_radians().tan() * sin_d(e))
        .atan2(cos_d(lambda))
        .to_degrees();
    let dec = (sin_d(beta) * cos_d(e) + cos_d(beta) * sin_d(e) * sin_d(lambda))
        .asin()
        .to_degrees();
    (norm_deg(ra), dec)
}

/// The sun's apparent ecliptic longitude (degrees).
pub fn sun_longitude(jd: f64) -> f64 {
    let t = centuries(jd);
    let l0 = 280.466_46 + 36_000.769_83 * t + 0.000_303_2 * t * t;
    let m = 357.529_11 + 35_999.050_29 * t - 0.000_153_7 * t * t;
    let c = (1.914_602 - 0.004_817 * t - 0.000_014 * t * t) * sin_d(m)
        + (0.019_993 - 0.000_101 * t) * sin_d(2.0 * m)
        + 0.000_289 * sin_d(3.0 * m);
    let omega = 125.04 - 1934.136 * t;
    norm_deg(l0 + c - 0.005_69 - 0.004_78 * sin_d(omega))
}

/// The sun's geocentric position.
pub fn sun(jd: f64) -> Equatorial {
    let t = centuries(jd);
    let (ra, dec) = ecliptic_to_equatorial(sun_longitude(jd), 0.0, t);
    Equatorial {
        ra,
        dec,
        distance_km: 0.0,
    }
}

/// Periodic terms of the moon's longitude and distance (Meeus table 47.A, the largest):
/// multiples of D, M, M', F; Σl in 1e-6°; Σr in 1e-3 km.
const LR: &[(i8, i8, i8, i8, f64, f64)] = &[
    (0, 0, 1, 0, 6_288_774.0, -20_905_355.0),
    (2, 0, -1, 0, 1_274_027.0, -3_699_111.0),
    (2, 0, 0, 0, 658_314.0, -2_955_968.0),
    (0, 0, 2, 0, 213_618.0, -569_925.0),
    (0, 1, 0, 0, -185_116.0, 48_888.0),
    (0, 0, 0, 2, -114_332.0, -3_149.0),
    (2, 0, -2, 0, 58_793.0, 246_158.0),
    (2, -1, -1, 0, 57_066.0, -152_138.0),
    (2, 0, 1, 0, 53_322.0, -170_733.0),
    (2, -1, 0, 0, 45_758.0, -204_586.0),
    (0, 1, -1, 0, -40_923.0, -129_620.0),
    (1, 0, 0, 0, -34_720.0, 108_743.0),
    (0, 1, 1, 0, -30_383.0, 104_755.0),
    (2, 0, 0, -2, 15_327.0, 10_321.0),
    (0, 0, 1, 2, -12_528.0, 0.0),
    (0, 0, 1, -2, 10_980.0, 79_661.0),
    (4, 0, -1, 0, 10_675.0, -34_782.0),
    (0, 0, 3, 0, 10_034.0, -23_210.0),
    (4, 0, -2, 0, 8_548.0, -21_636.0),
    (2, 1, -1, 0, -7_888.0, 24_208.0),
    (2, 1, 0, 0, -6_766.0, 30_824.0),
    (1, 0, -1, 0, -5_163.0, -8_379.0),
    (1, 1, 0, 0, 4_987.0, -16_675.0),
    (2, -1, 1, 0, 4_036.0, -12_831.0),
    (2, 0, 2, 0, 3_994.0, -10_445.0),
    (4, 0, 0, 0, 3_861.0, -11_650.0),
    (2, 0, -3, 0, 3_665.0, 14_403.0),
    (0, 1, -2, 0, -2_689.0, -7_003.0),
    (2, 0, -1, 2, -2_602.0, 0.0),
    (2, -1, -2, 0, 2_390.0, 10_056.0),
    (1, 0, 1, 0, -2_348.0, 6_322.0),
    (2, -2, 0, 0, 2_236.0, -9_884.0),
    (0, 1, 2, 0, -2_120.0, 5_751.0),
    (0, 2, 0, 0, -2_069.0, 0.0),
];

/// Periodic terms of the moon's latitude (Meeus table 47.B, the largest), Σb in 1e-6°.
const B: &[(i8, i8, i8, i8, f64)] = &[
    (0, 0, 0, 1, 5_128_122.0),
    (0, 0, 1, 1, 280_602.0),
    (0, 0, 1, -1, 277_693.0),
    (2, 0, 0, -1, 173_237.0),
    (2, 0, -1, 1, 55_413.0),
    (2, 0, -1, -1, 46_271.0),
    (2, 0, 0, 1, 32_573.0),
    (0, 0, 2, 1, 17_198.0),
    (2, 0, 1, -1, 9_266.0),
    (0, 0, 2, -1, 8_822.0),
    (2, -1, 0, -1, 8_216.0),
    (2, 0, -2, -1, 4_324.0),
    (2, 0, 1, 1, 4_200.0),
    (2, 1, 0, -1, -3_359.0),
    (2, -1, -1, 1, 2_463.0),
    (2, -1, 0, 1, 2_211.0),
    (2, -1, -1, -1, 2_065.0),
    (0, 1, -1, -1, -1_870.0),
    (4, 0, -1, -1, 1_828.0),
    (0, 1, 0, 1, -1_794.0),
    (0, 0, 0, 3, -1_749.0),
    (0, 1, -1, 1, -1_565.0),
    (1, 0, 0, 1, -1_491.0),
    (0, 1, 1, 1, -1_475.0),
    (0, 1, 1, -1, -1_410.0),
    (0, 1, 0, -1, -1_344.0),
    (1, 0, 0, -1, -1_335.0),
    (0, 0, 3, 1, 1_107.0),
];

/// The moon's geocentric ecliptic longitude, latitude (degrees) and distance (km).
pub fn moon_ecliptic(jd: f64) -> (f64, f64, f64) {
    let t = centuries(jd);
    let (t2, t3, t4) = (t * t, t * t * t, t * t * t * t);
    let lp = 218.316_447_7 + 481_267.881_234_21 * t - 0.001_578_6 * t2 + t3 / 538_841.0
        - t4 / 65_194_000.0;
    let d = 297.850_192_1 + 445_267.111_403_4 * t - 0.001_881_9 * t2 + t3 / 545_868.0
        - t4 / 113_065_000.0;
    let m = 357.529_109_2 + 35_999.050_290_9 * t - 0.000_153_6 * t2 + t3 / 24_490_000.0;
    let mp = 134.963_396_4 + 477_198.867_505_5 * t + 0.008_741_4 * t2 + t3 / 69_699.0
        - t4 / 14_712_000.0;
    let f = 93.272_095_0 + 483_202.017_523_3 * t - 0.003_653_9 * t2 - t3 / 3_526_000.0
        + t4 / 863_310_000.0;
    let a1 = 119.75 + 131.849 * t;
    let a2 = 53.09 + 479_264.290 * t;
    let a3 = 313.45 + 481_266.484 * t;
    let e = 1.0 - 0.002_516 * t - 0.000_007_4 * t2;
    let ecc = |mm: i8| match mm.abs() {
        1 => e,
        2 => e * e,
        _ => 1.0,
    };
    let arg = |dd: i8, mm: i8, mpp: i8, ff: i8| {
        dd as f64 * d + mm as f64 * m + mpp as f64 * mp + ff as f64 * f
    };
    let (mut sl, mut sr, mut sb) = (0.0, 0.0, 0.0);
    for &(dd, mm, mpp, ff, l, r) in LR {
        let a = arg(dd, mm, mpp, ff);
        sl += l * ecc(mm) * sin_d(a);
        sr += r * ecc(mm) * cos_d(a);
    }
    for &(dd, mm, mpp, ff, b) in B {
        sb += b * ecc(mm) * sin_d(arg(dd, mm, mpp, ff));
    }
    sl += 3958.0 * sin_d(a1) + 1962.0 * sin_d(lp - f) + 318.0 * sin_d(a2);
    sb += -2235.0 * sin_d(lp)
        + 382.0 * sin_d(a3)
        + 175.0 * sin_d(a1 - f)
        + 175.0 * sin_d(a1 + f)
        + 127.0 * sin_d(lp - mp)
        - 115.0 * sin_d(lp + mp);
    (norm_deg(lp + sl / 1e6), sb / 1e6, 385_000.56 + sr / 1000.0)
}

/// The moon's geocentric position.
pub fn moon(jd: f64) -> Equatorial {
    let (lambda, beta, distance_km) = moon_ecliptic(jd);
    let (ra, dec) = ecliptic_to_equatorial(lambda, beta, centuries(jd));
    Equatorial {
        ra,
        dec,
        distance_km,
    }
}

/// Greenwich mean sidereal time (degrees).
pub fn sidereal_time(jd: f64) -> f64 {
    let t = centuries(jd);
    norm_deg(
        280.460_618_37 + 360.985_647_366_29 * (jd - 2_451_545.0) + 0.000_387_933 * t * t
            - t * t * t / 38_710_000.0,
    )
}

/// Altitude and azimuth (degrees; azimuth from north towards east) of a body at hour angle
/// `h` (degrees, westward from the meridian) and declination `dec`, seen from latitude `lat`.
pub fn horizontal(h: f64, dec: f64, lat: f64) -> (f64, f64) {
    let alt = (sin_d(lat) * sin_d(dec) + cos_d(lat) * cos_d(dec) * cos_d(h))
        .asin()
        .to_degrees();
    // Meeus 13.5 measures from the south, westward.
    let a = sin_d(h)
        .atan2(cos_d(h) * sin_d(lat) - dec.to_radians().tan() * cos_d(lat))
        .to_degrees();
    (alt, norm_deg(a + 180.0))
}

/// Unit world vector for an altitude / azimuth (degrees).
pub fn direction(alt: f64, az: f64) -> Vec3 {
    let (a, z) = (alt.to_radians(), az.to_radians());
    DVec3::new(z.sin() * a.cos(), a.sin(), z.cos() * a.cos()).as_vec3()
}

/// Equatorial radius of the Earth (km), for the moon's parallax.
const EARTH_KM: f64 = 6378.14;

/// Topocentric position: seen from the observer at latitude `lat`, local sidereal time
/// `lst` (degrees) instead of from the Earth's centre (the moon shifts by up to 1°).
fn topocentric(p: Equatorial, lat: f64, lst: f64) -> Equatorial {
    if p.distance_km <= 0.0 {
        return p;
    }
    let at = |ra: f64, dec: f64, r: f64| {
        DVec3::new(
            r * cos_d(dec) * cos_d(ra),
            r * cos_d(dec) * sin_d(ra),
            r * sin_d(dec),
        )
    };
    let v = at(p.ra, p.dec, p.distance_km) - at(lst, lat, EARTH_KM);
    Equatorial {
        ra: norm_deg(v.y.atan2(v.x).to_degrees()),
        dec: (v.z / v.length()).asin().to_degrees(),
        distance_km: v.length(),
    }
}

/// Where the sun and moon are in an observer's sky.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sky {
    /// World unit vectors towards each.
    pub sun: Vec3,
    pub moon: Vec3,
    /// Fraction of the moon's disc that is lit (0 new .. 1 full), from the geocentric
    /// positions (Meeus 48.1/48.2 with the elongation).
    pub illumination: f32,
    /// True while the lit fraction grows (the moon east of the sun).
    pub waxing: bool,
}

/// The sky at a local date and time: `hours` local time, `utc_offset` hours east of UT,
/// `lat` / `lon` degrees (north and east positive).
pub fn sky(
    year: i32,
    month: u32,
    day: u32,
    hours: f64,
    utc_offset: f64,
    lat: f64,
    lon: f64,
) -> Sky {
    let jd = julian_day(year, month, day, hours - utc_offset);
    let lst = sidereal_time(jd) + lon;
    let s = sun(jd);
    let m_geo = moon(jd);
    let m = topocentric(m_geo, lat, lst);
    let (sa, sz) = horizontal(lst - s.ra, s.dec, lat);
    let (ma, mz) = horizontal(lst - m.ra, m.dec, lat);
    // Elongation (geocentric) → phase angle → lit fraction.
    let cos_psi =
        sin_d(s.dec) * sin_d(m_geo.dec) + cos_d(s.dec) * cos_d(m_geo.dec) * cos_d(s.ra - m_geo.ra);
    let psi = cos_psi.clamp(-1.0, 1.0).acos();
    let r_sun = 149_597_870.7;
    let i = (r_sun * psi.sin()).atan2(m_geo.distance_km - r_sun * psi.cos());
    let illumination = ((1.0 + i.cos()) / 2.0) as f32;
    // Waxing while the moon's longitude is ahead of the sun's (0..180° east).
    let (lm, _, _) = moon_ecliptic(jd);
    let waxing = norm_deg(lm - sun_longitude(jd)) < 180.0;
    Sky {
        sun: direction(sa, sz),
        moon: direction(ma, mz),
        illumination,
        waxing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        let d = (a - b).rem_euclid(360.0);
        d.min(360.0 - d) <= tol
    }

    #[test]
    fn julian_days_match_meeus() {
        // Meeus examples 7.a and 7.b.
        assert!((julian_day(1957, 10, 4, 0.81 * 24.0) - 2_436_116.31).abs() < 1e-6);
        assert_eq!(julian_day(2000, 1, 1, 12.0), 2_451_545.0);
        assert_eq!(julian_day(1987, 6, 19, 12.0), 2_446_966.0);
    }

    /// Meeus example 12.a: mean sidereal time at Greenwich on 1987 April 10, 0h UT is
    /// 13h10m46.3668s.
    #[test]
    fn sidereal_time_matches_meeus() {
        let st = sidereal_time(julian_day(1987, 4, 10, 0.0));
        let want = (13.0 + 10.0 / 60.0 + 46.3668 / 3600.0) * 15.0;
        assert!(close(st, want, 1e-4), "{st} vs {want}");
    }

    /// Meeus example 13.b: Venus from Washington on 1987 April 10, 19:21 UT is 15.1249° high at
    /// azimuth 68.0337° west of south (= 248.0337° from north).
    #[test]
    fn horizontal_coordinates_match_meeus() {
        let theta0 = 128.736_887_5;
        let l_east = -(77.0 + 3.0 / 60.0 + 56.0 / 3600.0);
        let ra = (23.0 + 9.0 / 60.0 + 16.641 / 3600.0) * 15.0;
        let dec = -(6.0 + 43.0 / 60.0 + 11.61 / 3600.0);
        let lat = 38.0 + 55.0 / 60.0 + 17.0 / 3600.0;
        let (alt, az) = horizontal(theta0 + l_east - ra, dec, lat);
        assert!(close(alt, 15.1249, 0.01), "{alt}");
        assert!(close(az, 248.0337, 0.01), "{az}");
    }

    /// Meeus example 25.a: the sun on 1992 October 13, 0h TD: apparent longitude 199.90895°,
    /// α = 198.38083°, δ = −7.78507°.
    #[test]
    fn sun_matches_meeus() {
        let jd = 2_448_908.5;
        assert!(close(sun_longitude(jd), 199.908_95, 0.002));
        let s = sun(jd);
        assert!(close(s.ra, 198.380_83, 0.01), "{}", s.ra);
        assert!((s.dec - -7.785_07).abs() < 0.01, "{}", s.dec);
    }

    /// Meeus examples 47.a and 48.a: the moon on 1992 April 12, 0h TD: λ = 133.162655°,
    /// β = −3.229126°, Δ = 368409.7 km, α = 134.688470°, δ = 13.768368°, lit fraction 0.6786.
    #[test]
    fn moon_matches_meeus() {
        let jd = 2_448_724.5;
        let (l, b, r) = moon_ecliptic(jd);
        assert!(close(l, 133.162_655, 0.02), "{l}");
        assert!((b - -3.229_126).abs() < 0.02, "{b}");
        assert!((r - 368_409.7).abs() < 30.0, "{r}");
        let m = moon(jd);
        // (Meeus's values include nutation, 0.005°.)
        assert!(close(m.ra, 134.688_470, 0.03), "{}", m.ra);
        assert!((m.dec - 13.768_368).abs() < 0.03, "{}", m.dec);
        // Lit fraction: any place, the geocentric value.
        let k = sky(1992, 4, 12, 0.0, 0.0, 0.0, 0.0).illumination;
        assert!((k - 0.6786).abs() < 0.003, "{k}");
    }

    /// The total solar eclipse of 2024 April 8, seen from Dallas at 18:42 UT: the moon covers
    /// the sun (only with the parallax for the observer), near noon, about 64.7° up
    /// (90° − 32.8° latitude + 7.5° declination).
    #[test]
    fn the_moon_covers_the_sun_during_the_dallas_eclipse() {
        let s = sky(2024, 4, 8, 18.7, 0.0, 32.78, -96.80);
        let sep = s.sun.angle_between(s.moon).to_degrees();
        assert!(sep < 0.2, "separation {sep}°");
        let alt = s.sun.y.asin().to_degrees();
        assert!((alt - 64.7).abs() < 1.0, "{alt}");
        assert!(s.illumination < 0.001);
        // Full moon of 2024 April 23, 23:49 UT.
        let full = sky(2024, 4, 23, 23.8, 0.0, 51.5, 0.0);
        assert!(full.illumination > 0.995, "{}", full.illumination);
        // First quarter of 2024 April 15, 19:13 UT: half lit and waxing.
        let q = sky(2024, 4, 15, 19.2, 0.0, 51.5, 0.0);
        assert!((q.illumination - 0.5).abs() < 0.02 && q.waxing, "{q:?}");
        let last = sky(2024, 5, 1, 11.5, 0.0, 51.5, 0.0);
        assert!(
            (last.illumination - 0.5).abs() < 0.03 && !last.waxing,
            "{last:?}"
        );
    }
}
