//! Light fields of Rectangle lights: size, two-sided emission and the visible emitter.
//! Each is left out of scene files at its default, so older scenes re-save unchanged.

/// Default width and height in metres.
pub const SIZE: f32 = 1.0;

pub fn is_size(v: &f32) -> bool {
    *v == SIZE
}

pub fn is_yes(v: &bool) -> bool {
    *v
}
