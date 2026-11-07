//! Grass rendering study (docs/pending/grass-study.md; test builds only): a self-contained
//! copy of the MeadowDemo grass (same scatter, chunks, draw prefixes, blade shape and colour
//! as terrain_grass.rs / grass*.wgsl at main a91ac1e) drawn by alternative methods, timed per
//! pass on the GPU and compared image by image:
//!
//! - A: the live way (grass depth prepass + lit pass, 4× MSAA with alpha to coverage);
//!   A1: the same single-sample with stochastic coverage;
//! - B: a visibility buffer (blade id raster, then shading once per pixel in compute);
//! - C: B with a compute rasteriser for blades thinner than ~1.5 px;
//! - D: fewer, wider multi-leaf cards in the middle distance;
//! - E: blades only near the camera, a parallax grass layer in the ground pass beyond;
//! - F: B shaded at a quarter rate (rotating 2×2 anchor + TAA);
//! - G: a world-space cache of the sun's visibility instead of shadow taps per fragment;
//! - H: half the middle- and far-distance blades per frame, TAA fills in the rest.
//!
//! Every variant shares the study lighting (study_common.wgsl, a stand-in for the engine's
//! `shade_grass` at a similar cost) and the same TAA, so the comparisons isolate the method.

mod field;
mod gpu;
mod tests;
