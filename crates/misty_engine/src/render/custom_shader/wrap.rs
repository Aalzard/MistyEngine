//! Turn a shader asset into a full WGSL module: engine code (common + mesh lighting + prelude),
//! the generated `material` uniform and texture bindings, the user's code, then the entry points
//! that call `surface` / `fragment` (and `vertex` when defined). naga validates the result and
//! errors are mapped back to lines of the user's file.

use std::fmt::Write;

use wgpu::naga;

use crate::shader_asset::{
    self, Blend, PropertyKind, ShaderAsset, ShaderError, ShaderModel, TextureDefault,
};

use super::super::pipelines::{COMMON, MESH_SRC};

const PRELUDE: &str = include_str!("prelude.wgsl");

/// Group 2 bindings: the material uniform, the sampler, then one per texture property.
pub const UNIFORM_BINDING: u32 = 16;
pub const SAMPLER_BINDING: u32 = 17;
pub const FIRST_TEXTURE_BINDING: u32 = 18;

/// One value field of the `MaterialProperties` uniform.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    pub offset: usize,
    pub kind: PropertyKind,
}

/// Where each property lives on the GPU.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropsLayout {
    pub fields: Vec<Field>,
    /// Uniform size in bytes (a multiple of 16).
    pub size: usize,
    /// (name, binding, fallback) per texture property.
    pub textures: Vec<(String, u32, TextureDefault)>,
}

/// WGSL uniform layout: f32 aligns to 4, vec4 to 16; the struct rounds up to 16. A reserved vec4
/// comes first so the struct is never empty.
pub fn layout(asset: &ShaderAsset) -> PropsLayout {
    let mut out = PropsLayout {
        size: 16,
        ..Default::default()
    };
    for p in &asset.properties {
        match p.kind {
            PropertyKind::Texture(default) => {
                let binding = FIRST_TEXTURE_BINDING + out.textures.len() as u32;
                out.textures.push((p.name.clone(), binding, default));
            }
            kind => {
                let (align, size) = if kind.wgsl() == Some("f32") {
                    (4, 4)
                } else {
                    (16, 16)
                };
                let offset = out.size.div_ceil(align) * align;
                out.fields.push(Field {
                    name: p.name.clone(),
                    offset,
                    kind,
                });
                out.size = offset + size;
            }
        }
    }
    out.size = out.size.div_ceil(16) * 16;
    out
}

/// A generated module.
pub struct Wrapped {
    /// Everything after common.wgsl (the pipeline builder prepends it).
    pub body: String,
    /// Lines before the user's first line in `COMMON + "\n" + body`.
    pub user_first_line: usize,
    pub user_lines: usize,
    pub props: PropsLayout,
}

impl Wrapped {
    pub fn full(&self) -> String {
        format!("{COMMON}\n{}", self.body)
    }
}

fn bindings(props: &PropsLayout, asset: &ShaderAsset) -> String {
    let mut s = String::from("struct MaterialProperties {\n    misty_reserved: vec4<f32>,\n");
    for f in &props.fields {
        let ty = f.kind.wgsl().unwrap_or("f32");
        let _ = writeln!(s, "    {}: {ty},", f.name);
    }
    s.push_str("};\n");
    let _ = writeln!(
        s,
        "@group(2) @binding({UNIFORM_BINDING}) var<uniform> material: MaterialProperties;"
    );
    let _ = writeln!(
        s,
        "@group(2) @binding({SAMPLER_BINDING}) var tex_sampler: sampler;"
    );
    for (name, binding, _) in &props.textures {
        let _ = writeln!(
            s,
            "@group(2) @binding({binding}) var {name}: texture_2d<f32>;"
        );
    }
    let _ = asset;
    s
}

fn entries(asset: &ShaderAsset) -> String {
    let vertex_call = if asset.has_vertex {
        "    v = vertex(v);\n"
    } else {
        ""
    };
    let mut s = format!(
        r#"
@vertex
fn vs_custom(in: MeshIn, @builtin(instance_index) instance: u32) -> MeshOut {{
    let draw_index = slots[instance];
    let d = draws[draw_index];
    var v: VertexData;
    v.position = in.position;
    v.normal = in.normal;
    v.uv = in.uv;
    v.tangent = in.tangent;
    v.draw = draw_index;
{vertex_call}    let world = d.model * vec4<f32>(v.position, 1.0);
    var out: MeshOut;
    out.clip = view.view_proj * world;
    out.world = world.xyz;
    out.normal = (d.normal * vec4<f32>(v.normal, 0.0)).xyz;
    out.uv = v.uv * d.uv_st.xy + d.uv_st.zw;
    out.draw = draw_index;
    out.tangent = vec4<f32>((d.model * vec4<f32>(v.tangent.xyz, 0.0)).xyz, v.tangent.w * d.emission.w);
    return out;
}}
"#
    );
    match asset.settings.model {
        ShaderModel::Surface => {
            let premultiply = asset.settings.blend == Blend::Premultiply;
            let _ = write!(
                s,
                r#"
@fragment
fn fs_custom(in: MeshOut, @builtin(front_facing) front: bool) -> MeshTargets {{
    let o = surface(misty_surface_input(in, front));
    var s: Surface;
    s.albedo = o.albedo;
    s.alpha = o.alpha;
    s.metallic = o.metallic;
    s.smoothness = o.smoothness;
    s.normal = normalize(o.normal);
    s.emission = o.emission;
    s.receive_shadows = draws[in.draw].params.z > 0.5;
    s.occlusion = o.occlusion;
    s.premultiply = {premultiply};
    return shade(in.world, s);
}}
"#
            );
        }
        ShaderModel::Unlit => s.push_str(
            r#"
@fragment
fn fs_custom(in: MeshOut, @builtin(front_facing) front: bool) -> MeshTargets {
    let c = fragment(misty_surface_input(in, front));
    var out: MeshTargets;
    out.color = c;
    out.glow = vec4<f32>(0.0, 0.0, 0.0, c.a);
    return out;
}
"#,
        ),
    }
    s
}

/// Generate the module for a shader.
pub fn wrap(asset: &ShaderAsset) -> Wrapped {
    let props = layout(asset);
    let head = format!(
        "{MESH_SRC}\n{PRELUDE}\n{}\n// ---- user shader ----\n",
        bindings(&props, asset)
    );
    let user_first_line = COMMON.matches('\n').count() + 1 + head.matches('\n').count();
    let mut body = head;
    body.push_str(&asset.source);
    if !asset.source.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(&entries(asset));
    Wrapped {
        body,
        user_first_line,
        user_lines: asset.source.lines().count().max(1),
        props,
    }
}

/// A line of the generated module (1-based) as a line of the user's file (0 = not in it).
fn user_line(w: &Wrapped, generated: u32) -> usize {
    let g = generated as usize;
    if g > w.user_first_line && g <= w.user_first_line + w.user_lines {
        g - w.user_first_line
    } else {
        0
    }
}

fn located(w: &Wrapped, loc: Option<naga::SourceLocation>, message: String) -> ShaderError {
    let (line, column) = match loc {
        Some(l) => match user_line(w, l.line_number) {
            0 => (0, 0),
            n => (n, l.line_position as usize),
        },
        None => (0, 0),
    };
    let message = if line == 0 {
        format!(
            "{message} (in the engine wrapper: check that the shader defines the expected \
             function, e.g. fn surface(input: SurfaceInput) -> SurfaceOutput)"
        )
    } else {
        message
    };
    ShaderError {
        line,
        column,
        message,
    }
}

/// Full error text of a validation error and its causes.
fn chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut src = e.source();
    while let Some(inner) = src {
        let _ = write!(s, ": {inner}");
        src = inner.source();
    }
    s
}

/// Parse and validate the generated module with naga.
pub fn validate(w: &Wrapped) -> Result<(), ShaderError> {
    let full = w.full();
    let module = naga::front::wgsl::parse_str(&full)
        .map_err(|e| located(w, e.location(&full), e.message().to_owned()))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .map_err(|e| {
        // The innermost span that lies in the user's code, else the first one.
        let spans: Vec<naga::SourceLocation> = e
            .spans()
            .filter(|(s, _)| s.is_defined())
            .map(|(s, _)| s.location(&full))
            .collect();
        let loc = spans
            .iter()
            .rev()
            .find(|l| user_line(w, l.line_number) > 0)
            .or(spans.first())
            .copied();
        located(w, loc, chain(e.as_inner()))
    })?;
    Ok(())
}

/// Check shader source as the renderer would compile it (header, then WGSL).
pub fn check(source: &str) -> Result<(), ShaderError> {
    let asset = shader_asset::parse(source)?;
    validate(&wrap(&asset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader_asset::templates;

    #[test]
    fn templates_wrap_and_validate() {
        for (name, src) in [
            ("surface", templates::SURFACE),
            ("unlit", templates::UNLIT),
            ("error", templates::ERROR),
        ] {
            check(src).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    #[test]
    fn vertex_and_scene_helpers_compile() {
        let src = "\
// @shader surface
// @blend alpha
// @property _Amount range(0, 1) = 0.5
fn vertex(v: VertexData) -> VertexData {
    var o = v;
    o.position.y += sin(globals.time.x + v.position.x) * material._Amount;
    return o;
}
fn surface(input: SurfaceInput) -> SurfaceOutput {
    var o = default_output(input);
    o.albedo = scene_color_at(input.screen_uv) * 0.5;
    o.alpha = clamp(scene_distance(input.screen_uv) - distance(input.world_pos, view.camera_pos.xyz), 0.0, 1.0);
    return o;
}
";
        check(src).unwrap();
        let asset = shader_asset::parse(src).unwrap();
        assert!(asset.has_vertex && asset.uses_scene);
        assert!(wrap(&asset).body.contains("v = vertex(v);"));
    }

    #[test]
    fn uniform_layout_follows_wgsl_rules() {
        let asset = shader_asset::parse(
            "// @property _A float = 1\n// @property _B color\n// @property _T texture2d\n// @property _C range(0,1)\n// @property _D float",
        )
        .unwrap();
        let l = layout(&asset);
        let offsets: Vec<(&str, usize)> = l
            .fields
            .iter()
            .map(|f| (f.name.as_str(), f.offset))
            .collect();
        // reserved vec4 at 0, then _A at 16, _B aligned to 32, _C at 48, _D at 52.
        assert_eq!(offsets, [("_A", 16), ("_B", 32), ("_C", 48), ("_D", 52)]);
        assert_eq!(l.size, 64);
        assert_eq!(
            l.textures,
            [(
                "_T".to_owned(),
                FIRST_TEXTURE_BINDING,
                TextureDefault::White
            )]
        );
    }

    #[test]
    fn errors_point_at_user_lines() {
        // Line 4 uses an undefined name.
        let src = "// @shader surface\nfn surface(input: SurfaceInput) -> SurfaceOutput {\n    var o = default_output(input);\n    o.albedo = undefined_thing;\n    return o;\n}\n";
        let e = check(src).unwrap_err();
        assert_eq!(e.line, 4, "{e}");
        assert!(e.message.contains("undefined_thing"), "{e}");

        // Wrong return type: reported by the validator.
        let src = "// @shader unlit\nfn fragment(input: SurfaceInput) -> vec4<f32> {\n    let x: f32 = 1.0;\n    return x;\n}\n";
        let e = check(src).unwrap_err();
        assert!((3..=4).contains(&e.line), "{e}");

        // Missing entry function: the wrapper's call fails, outside the user's lines.
        let e = check("// @shader surface\nfn not_surface() {}\n").unwrap_err();
        assert_eq!(e.line, 0);
        assert!(e.message.contains("surface"), "{e}");

        // Header errors keep their line.
        assert_eq!(check("\n// @cull sideways").unwrap_err().line, 2);
    }
}
