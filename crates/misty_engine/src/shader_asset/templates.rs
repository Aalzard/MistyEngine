//! Starting points for Assets ▸ Create ▸ Shader, and the built-in error shader.

/// Assets ▸ Create ▸ Shader ▸ Surface Shader.
pub const SURFACE: &str = r#"// Surface shader: the engine lights it (lights, shadows, ambient, reflections, fog).
// Fill in SurfaceOutput; see docs/shaders.md for the inputs and helpers.
// @shader surface
// @property _Color color "Color" = (1, 1, 1, 1)
// @property _MainTex texture2d "Albedo" = white
// @property _Metallic range(0, 1) = 0
// @property _Smoothness range(0, 1) = 0.5
// @property _Emission color "Emission" = (0, 0, 0, 1)

fn surface(input: SurfaceInput) -> SurfaceOutput {
    var o = default_output(input);
    let tex = textureSample(_MainTex, tex_sampler, input.uv);
    o.albedo = material._Color.rgb * tex.rgb;
    o.alpha = material._Color.a * tex.a;
    o.metallic = material._Metallic;
    o.smoothness = material._Smoothness;
    o.emission = material._Emission.rgb;
    return o;
}
"#;

/// Assets ▸ Create ▸ Shader ▸ Unlit Shader.
pub const UNLIT: &str = r#"// Unlit shader: return the final linear colour and alpha yourself.
// @shader unlit
// @property _Color color "Color" = (1, 1, 1, 1)
// @property _MainTex texture2d "Texture" = white
// @property _Scroll vector "Scroll (UV / s)" = (0, 0, 0, 0)

fn fragment(input: SurfaceInput) -> vec4<f32> {
    let uv = input.uv + material._Scroll.xy * globals.time.x;
    let c = material._Color * textureSample(_MainTex, tex_sampler, uv);
    return vec4<f32>(apply_fog(c.rgb, input.world_pos), c.a);
}
"#;

/// Drawn instead of a shader that does not compile: flat magenta, like the reference engine.
pub const ERROR: &str = r#"// @shader unlit
fn fragment(input: SurfaceInput) -> vec4<f32> {
    return vec4<f32>(1.0, 0.0, 1.0, 1.0);
}
"#;
