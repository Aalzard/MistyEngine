//! Header parser: `// @…` comment lines declaring the shader model, render state and properties.
//!
//! ```text
//! // @shader surface            (or unlit)
//! // @blend alpha               (off, alpha, premultiply, additive)
//! // @cull off                  (back, front, off)
//! // @zwrite off                (on, off)
//! // @queue transparent         (geometry, alphatest, transparent, overlay or a number)
//! // @property _Color color "Tint" = (1, 1, 1, 1)
//! // @property _Speed float range(0, 10) = 1   // tooltip shown in the Inspector
//! // @property _MainTex texture2d = white
//! ```

use super::{
    Blend, Cull, PropertyKind, PropertyValue, ShaderAsset, ShaderError, ShaderModel,
    ShaderProperty, ShaderSettings, TextureDefault,
};

fn err(line: usize, message: impl Into<String>) -> ShaderError {
    ShaderError {
        line,
        column: 1,
        message: message.into(),
    }
}

/// Parse a shader file's header. The WGSL itself is checked when the renderer compiles it.
pub fn parse(source: &str) -> Result<ShaderAsset, ShaderError> {
    let mut settings = ShaderSettings::default();
    let (mut zwrite, mut queue) = (None, None);
    let mut properties: Vec<ShaderProperty> = Vec::new();
    for (i, raw) in source.lines().enumerate() {
        let line = i + 1;
        let Some(rest) = raw.trim_start().strip_prefix("//") else {
            continue;
        };
        let Some(directive) = rest.trim_start().strip_prefix('@') else {
            continue;
        };
        let (word, args) = directive
            .split_once(char::is_whitespace)
            .unwrap_or((directive, ""));
        let args = args.trim();
        let value = args.to_ascii_lowercase();
        match word {
            "shader" => {
                settings.model = match value.as_str() {
                    "surface" | "lit" => ShaderModel::Surface,
                    "unlit" => ShaderModel::Unlit,
                    _ => return Err(err(line, format!("unknown shader model '{args}'"))),
                }
            }
            "blend" => {
                settings.blend = match value.as_str() {
                    "off" | "opaque" => Blend::Off,
                    "alpha" | "fade" => Blend::Alpha,
                    "premultiply" | "premultiplied" | "transparent" => Blend::Premultiply,
                    "additive" | "add" => Blend::Additive,
                    _ => return Err(err(line, format!("unknown blend mode '{args}'"))),
                }
            }
            "cull" => {
                settings.cull = match value.as_str() {
                    "back" => Cull::Back,
                    "front" => Cull::Front,
                    "off" | "none" => Cull::Off,
                    _ => return Err(err(line, format!("unknown cull mode '{args}'"))),
                }
            }
            "zwrite" => {
                zwrite = Some(match value.as_str() {
                    "on" | "true" => true,
                    "off" | "false" => false,
                    _ => return Err(err(line, "zwrite takes on or off")),
                })
            }
            "queue" => queue = Some(parse_queue(&value).ok_or_else(|| err(line, "bad queue"))?),
            "property" => {
                let p = parse_property(args, line)?;
                if properties.iter().any(|q| q.name == p.name) {
                    return Err(err(line, format!("property {} declared twice", p.name)));
                }
                properties.push(p);
            }
            _ => return Err(err(line, format!("unknown directive @{word}"))),
        }
    }
    let blended = settings.blend != Blend::Off;
    settings.zwrite = zwrite.unwrap_or(!blended);
    settings.queue = queue.unwrap_or(if blended { 3000 } else { 2000 });
    let code = strip_comments(source);
    Ok(ShaderAsset {
        source: source.to_owned(),
        settings,
        properties,
        has_vertex: defines_fn(&code, "vertex"),
        uses_scene: SCENE_NAMES.iter().any(|w| has_word(&code, w)),
    })
}

/// Prelude names that read the opaque scene copy.
const SCENE_NAMES: [&str; 6] = [
    "scene_color",
    "scene_depth",
    "scene_color_at",
    "scene_depth_at",
    "scene_world_pos",
    "scene_distance",
];

/// `geometry`, `alphatest`, `transparent`, `overlay`, a number, or a name with `+n` / `-n`.
fn parse_queue(v: &str) -> Option<u32> {
    let v = v.replace(' ', "");
    let (name, delta) = match v.find(['+', '-']) {
        Some(i) if i > 0 => (&v[..i], v[i..].parse::<i64>().ok()?),
        _ => (v.as_str(), 0),
    };
    let base: i64 = match name {
        "background" => 1000,
        "geometry" => 2000,
        "alphatest" => 2450,
        "transparent" => 3000,
        "overlay" => 4000,
        n => n.parse().ok()?,
    };
    u32::try_from(base + delta).ok()
}

fn valid_ident(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name != "_"
        && !name.starts_with("__")
}

/// `NAME TYPE ["Display"] = DEFAULT [// tooltip]`.
fn parse_property(args: &str, line: usize) -> Result<ShaderProperty, ShaderError> {
    let (decl, tooltip) = match args.split_once("//") {
        Some((d, t)) => (
            d.trim(),
            Some(t.trim().to_owned()).filter(|t| !t.is_empty()),
        ),
        None => (args.trim(), None),
    };
    let (left, default) = decl
        .split_once('=')
        .map(|(l, d)| (l.trim(), Some(d.trim())))
        .unwrap_or((decl, None));
    let (name, rest) = left
        .split_once(char::is_whitespace)
        .ok_or_else(|| err(line, "a property needs a name and a type"))?;
    if !valid_ident(name) {
        return Err(err(line, format!("'{name}' is not a valid WGSL name")));
    }
    // A quoted display name may sit anywhere after the name.
    let (display, ty) = match (rest.find('"'), rest.rfind('"')) {
        (Some(a), Some(b)) if b > a => (
            Some(rest[a + 1..b].to_owned()),
            format!("{} {}", &rest[..a], &rest[b + 1..]),
        ),
        _ => (None, rest.to_owned()),
    };
    let ty = ty.trim().to_ascii_lowercase().replace(' ', "");
    let kind = parse_kind(&ty).ok_or_else(|| err(line, format!("unknown property type '{ty}'")))?;
    let default = parse_default(kind, default)
        .ok_or_else(|| err(line, format!("bad default value for {name}")))?;
    let kind = match (kind, &default) {
        (PropertyKind::Texture(_), PropertyValue::Texture(Some(d))) => {
            PropertyKind::Texture(texture_default(d).unwrap_or_default())
        }
        _ => kind,
    };
    let default = match default {
        PropertyValue::Texture(_) => PropertyValue::Texture(None),
        v => v,
    };
    Ok(ShaderProperty {
        name: name.to_owned(),
        display: display.unwrap_or_else(|| name.trim_start_matches('_').to_owned()),
        kind,
        default,
        tooltip,
        line,
    })
}

fn parse_kind(ty: &str) -> Option<PropertyKind> {
    let ty = ty
        .strip_prefix("float")
        .filter(|r| !r.is_empty())
        .unwrap_or(ty);
    Some(match ty {
        "float" | "number" => PropertyKind::Float,
        "color" | "colour" => PropertyKind::Color,
        "vector" | "vec4" => PropertyKind::Vector,
        "texture2d" | "texture" | "2d" | "tex2d" => PropertyKind::Texture(TextureDefault::White),
        _ => {
            let inner = ty.strip_prefix("range(")?.strip_suffix(')')?;
            let (a, b) = inner.split_once(',')?;
            let (a, b): (f32, f32) = (a.parse().ok()?, b.parse().ok()?);
            PropertyKind::Range(a.min(b), a.max(b))
        }
    })
}

fn texture_default(name: &str) -> Option<TextureDefault> {
    Some(
        match name.trim().trim_matches('"').to_ascii_lowercase().as_str() {
            "white" => TextureDefault::White,
            "black" => TextureDefault::Black,
            "grey" | "gray" => TextureDefault::Grey,
            "normal" | "bump" => TextureDefault::Normal,
            _ => return None,
        },
    )
}

fn parse_default(kind: PropertyKind, text: Option<&str>) -> Option<PropertyValue> {
    match kind {
        PropertyKind::Float | PropertyKind::Range(..) => {
            let v = match text {
                Some(t) => t.parse().ok()?,
                None => match kind {
                    PropertyKind::Range(lo, _) => lo,
                    _ => 0.0,
                },
            };
            Some(PropertyValue::Float(v))
        }
        PropertyKind::Color | PropertyKind::Vector => {
            let pad = if kind == PropertyKind::Color {
                1.0
            } else {
                0.0
            };
            let Some(t) = text else {
                return Some(PropertyValue::Vector([pad, pad, pad, pad]));
            };
            let inner = t.trim_matches(|c| matches!(c, '(' | ')' | '[' | ']'));
            let parts: Vec<f32> = inner
                .split(',')
                .map(|p| p.trim().parse().ok())
                .collect::<Option<_>>()?;
            if parts.is_empty() || parts.len() > 4 {
                return None;
            }
            let mut v = [0.0, 0.0, 0.0, pad];
            v[..parts.len()].copy_from_slice(&parts);
            Some(PropertyValue::Vector(v))
        }
        PropertyKind::Texture(_) => match text {
            None => Some(PropertyValue::Texture(None)),
            Some(t) => texture_default(t).map(|d| PropertyValue::Texture(Some(d.name().into()))),
        },
    }
}

/// The code with `//` and `/* */` comments blanked out (line breaks kept).
pub(crate) fn strip_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut block = 0usize;
    while let Some(c) = chars.next() {
        if block > 0 {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                block -= 1;
            } else if c == '/' && chars.peek() == Some(&'*') {
                chars.next();
                block += 1;
            } else if c == '\n' {
                out.push('\n');
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            for n in chars.by_ref() {
                if n == '\n' {
                    out.push('\n');
                    break;
                }
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            block = 1;
        } else {
            out.push(c);
        }
    }
    out
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `word` appears as a whole identifier.
fn has_word(code: &str, word: &str) -> bool {
    code.match_indices(word).any(|(i, _)| {
        let before = code[..i].chars().next_back();
        let after = code[i + word.len()..].chars().next();
        !before.is_some_and(is_ident_char) && !after.is_some_and(is_ident_char)
    })
}

/// `fn name(` is declared.
fn defines_fn(code: &str, name: &str) -> bool {
    code.match_indices("fn").any(|(i, _)| {
        let before = code[..i].chars().next_back();
        if before.is_some_and(is_ident_char) {
            return false;
        }
        let rest = &code[i + 2..];
        let trimmed = rest.trim_start();
        trimmed.len() < rest.len()
            && trimmed
                .strip_prefix(name)
                .is_some_and(|r| r.trim_start().starts_with('('))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "\
// A test shader.
// @shader surface
// @blend alpha
// @cull off
// @property _Color color \"Tint Color\" = (1, 0.5, 0.25)
// @property _Speed float range(0,10) = 2.5 // How fast the stripes scroll
// @property _Height range(-1, 1)
// @property _Offset vector = [1, 2]
// @property _MainTex texture2d = black
// @property _Bumps texture = normal
fn surface(input: SurfaceInput) -> SurfaceOutput {
    // scene_color in a comment does not count
    return default_output(input);
}
";

    #[test]
    fn properties_and_settings_parse() {
        let s = parse(HEADER).unwrap();
        assert_eq!(s.settings.model, ShaderModel::Surface);
        assert_eq!(s.settings.blend, Blend::Alpha);
        assert_eq!(s.settings.cull, Cull::Off);
        assert!(
            !s.settings.zwrite,
            "blended shaders default to no depth writes"
        );
        assert_eq!(s.settings.queue, 3000);
        assert!(!s.has_vertex && !s.uses_scene);
        let names: Vec<&str> = s.properties.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "_Color", "_Speed", "_Height", "_Offset", "_MainTex", "_Bumps"
            ]
        );
        let color = &s.properties[0];
        assert_eq!(color.display, "Tint Color");
        assert_eq!(color.kind, PropertyKind::Color);
        assert_eq!(color.default, PropertyValue::Vector([1.0, 0.5, 0.25, 1.0]));
        assert_eq!(color.line, 5);
        let speed = &s.properties[1];
        assert_eq!(speed.kind, PropertyKind::Range(0.0, 10.0));
        assert_eq!(speed.default, PropertyValue::Float(2.5));
        assert_eq!(speed.display, "Speed");
        assert_eq!(
            speed.tooltip.as_deref(),
            Some("How fast the stripes scroll")
        );
        assert_eq!(s.properties[2].default, PropertyValue::Float(-1.0));
        assert_eq!(
            s.properties[3].default,
            PropertyValue::Vector([1.0, 2.0, 0.0, 0.0])
        );
        assert_eq!(
            s.properties[4].kind,
            PropertyKind::Texture(TextureDefault::Black)
        );
        assert_eq!(s.properties[4].default, PropertyValue::Texture(None));
        assert_eq!(
            s.properties[5].kind,
            PropertyKind::Texture(TextureDefault::Normal)
        );
    }

    #[test]
    fn defaults_queue_and_detection() {
        let s = parse("// @shader unlit\n// @queue transparent+10\nfn  vertex (v: VertexData) -> VertexData { return v; }\nlet x = scene_depth;").unwrap();
        assert_eq!(s.settings.model, ShaderModel::Unlit);
        assert_eq!(s.settings.queue, 3010);
        assert!(s.settings.zwrite && s.settings.blend == Blend::Off);
        assert!(s.has_vertex && s.uses_scene);
        assert_eq!(parse("").unwrap().settings, ShaderSettings::default());
        assert!(!parse("fn vertexish() {}").unwrap().has_vertex);
    }

    #[test]
    fn header_errors_name_their_line() {
        for (src, line, text) in [
            ("// @blend sideways", 1, "blend"),
            ("\n// @property 9lives float = 1", 2, "valid WGSL"),
            ("\n\n// @property _A float = x", 3, "default"),
            ("// @property _A float\n// @property _A color", 2, "twice"),
            ("// @frobnicate", 1, "unknown directive"),
            ("// @property _A matrix", 1, "type"),
        ] {
            let e = parse(src).unwrap_err();
            assert_eq!(e.line, line, "{src}");
            assert!(e.message.contains(text), "{}", e.message);
        }
    }

    #[test]
    fn values_round_trip_through_json() {
        let values = [
            PropertyValue::Float(2.0),
            PropertyValue::Vector([1.0, 0.0, 0.0, 1.0]),
            PropertyValue::Texture(Some("Assets/Textures/Grid.png".into())),
            PropertyValue::Texture(None),
        ];
        for v in values {
            let text = serde_json::to_string(&v).unwrap();
            assert_eq!(serde_json::from_str::<PropertyValue>(&text).unwrap(), v);
        }
        assert!(PropertyValue::Float(1.0).fits(PropertyKind::Range(0.0, 1.0)));
        assert!(!PropertyValue::Float(1.0).fits(PropertyKind::Color));
    }
}
