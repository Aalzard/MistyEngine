//! Float functions that give the same bits on every OS.
//!
//! `f32::sin`, `cos`, `tan`, `atan`, `atan2` and `powf` call the system math library, which
//! differs in the last bit between Windows and Linux. Erosion and the slope rules amplify that
//! into visibly different worlds, so the same seed would build a different terrain per OS. World
//! Builder uses these pure-Rust versions (the `libm` crate, a port of musl's) instead.

pub fn sin(x: f32) -> f32 {
    libm::sinf(x)
}

pub fn cos(x: f32) -> f32 {
    libm::cosf(x)
}

pub fn sin_cos(x: f32) -> (f32, f32) {
    libm::sincosf(x)
}

pub fn tan(x: f32) -> f32 {
    libm::tanf(x)
}

pub fn atan(x: f32) -> f32 {
    libm::atanf(x)
}

pub fn atan2(y: f32, x: f32) -> f32 {
    libm::atan2f(y, x)
}

pub fn powf(x: f32, y: f32) -> f32 {
    libm::powf(x, y)
}

#[cfg(test)]
mod tests {
    #[test]
    fn agrees_with_std_to_an_ulp() {
        for i in -200..200 {
            let x = i as f32 * 0.037;
            let close = |a: f32, b: f32| (a - b).abs() <= 2.0 * f32::EPSILON * a.abs().max(1.0);
            assert!(close(super::sin(x), x.sin()), "{x}");
            assert!(close(super::cos(x), x.cos()), "{x}");
            assert!(close(super::atan(x), x.atan()), "{x}");
            assert!(close(super::atan2(x, 0.7), x.atan2(0.7)), "{x}");
            assert!(close(super::powf(x.abs(), 1.6), x.abs().powf(1.6)), "{x}");
            let (s, c) = super::sin_cos(x);
            assert_eq!((s, c), (super::sin(x), super::cos(x)));
        }
    }
}
