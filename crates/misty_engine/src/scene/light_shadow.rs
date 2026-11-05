//! Light ▸ Shadows fields: source size for soft (contact-hardening) shadows and the biases.
//! Each is left out of scene files at its default, so older scenes re-save unchanged.

/// The sun's angular diameter in degrees.
pub const ANGLE: f32 = 0.53;
/// Spot / point light source radius in metres.
pub const RADIUS: f32 = 0.05;
/// Depth bias in shadow-map texels.
pub const BIAS: f32 = 1.0;
/// Normal offset in shadow-map texels.
pub const NORMAL_BIAS: f32 = 1.5;
/// Penumbra multiplier (1 = the light's physical size).
pub const SOFTNESS: f32 = 1.0;

pub fn is_angle(v: &f32) -> bool {
    *v == ANGLE
}
pub fn is_radius(v: &f32) -> bool {
    *v == RADIUS
}
pub fn is_bias(v: &f32) -> bool {
    *v == BIAS
}
pub fn is_normal_bias(v: &f32) -> bool {
    *v == NORMAL_BIAS
}
pub fn is_softness(v: &f32) -> bool {
    *v == SOFTNESS
}
