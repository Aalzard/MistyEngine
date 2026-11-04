//! WGSL support for `.shader` files: highlighting (keywords, types, built-in functions, the
//! engine's shader API, `// @` header directives), brace folding, and naga errors as squiggles.

use super::diagnostics::Diagnostic;
use super::lexer::{LineState, Structure, Token, TokenKind};

/// Files the Code Editor treats as WGSL.
pub fn is_wgsl(path: &str) -> bool {
    path.ends_with(".shader") || path.ends_with(".wgsl")
}

const KEYWORDS: &[&str] = &[
    "fn",
    "let",
    "var",
    "const",
    "struct",
    "return",
    "if",
    "else",
    "for",
    "while",
    "loop",
    "break",
    "continue",
    "continuing",
    "switch",
    "case",
    "default",
    "discard",
    "alias",
    "override",
    "enable",
    "requires",
];

const TYPES: &[&str] = &[
    "f32",
    "i32",
    "u32",
    "f16",
    "bool",
    "vec2",
    "vec3",
    "vec4",
    "mat2x2",
    "mat3x3",
    "mat4x4",
    "mat2x3",
    "mat3x2",
    "mat2x4",
    "mat4x2",
    "mat3x4",
    "mat4x3",
    "array",
    "atomic",
    "ptr",
    "sampler",
    "sampler_comparison",
    "texture_2d",
    "texture_3d",
    "texture_cube",
    "texture_2d_array",
    "texture_depth_2d",
    "texture_storage_2d",
    "vec2f",
    "vec3f",
    "vec4f",
    "vec2i",
    "vec3i",
    "vec4i",
    "vec2u",
    "vec3u",
    "vec4u",
    "mat4x4f",
    "mat3x3f",
];

const BUILTINS: &[&str] = &[
    "abs",
    "acos",
    "all",
    "any",
    "asin",
    "atan",
    "atan2",
    "ceil",
    "clamp",
    "cos",
    "cross",
    "degrees",
    "determinant",
    "distance",
    "dot",
    "dpdx",
    "dpdy",
    "exp",
    "exp2",
    "faceForward",
    "floor",
    "fma",
    "fract",
    "fwidth",
    "inverseSqrt",
    "length",
    "log",
    "log2",
    "max",
    "min",
    "mix",
    "modf",
    "normalize",
    "pow",
    "radians",
    "reflect",
    "refract",
    "round",
    "saturate",
    "select",
    "sign",
    "sin",
    "smoothstep",
    "sqrt",
    "step",
    "tan",
    "tanh",
    "transpose",
    "trunc",
    "textureSample",
    "textureSampleLevel",
    "textureSampleGrad",
    "textureSampleBias",
    "textureLoad",
    "textureDimensions",
    "arrayLength",
    "bitcast",
    "pack4x8unorm",
    "unpack4x8unorm",
];

/// The engine's shader API (prelude names and helpers).
const ENGINE: &[&str] = &[
    "view",
    "globals",
    "material",
    "draws",
    "lights",
    "tex_sampler",
    "scene_color",
    "scene_depth",
    "scene_sampler",
    "SurfaceInput",
    "SurfaceOutput",
    "VertexData",
    "surface",
    "fragment",
    "vertex",
    "default_output",
    "unpack_normal",
    "tangent_to_world",
    "object_to_world",
    "apply_fog",
    "scene_color_at",
    "scene_depth_at",
    "scene_world_pos",
    "scene_distance",
    "sky_color",
    "srgb_to_linear",
    "fog_visibility",
    "fog_distance",
    "unproject",
];

fn tok(kind: TokenKind, start: usize, end: usize) -> Token {
    Token { kind, start, end }
}

/// Tokens of one line; block comments carry over in [`LineState::LongComment`].
pub fn tokenize_line(line: &str, state: LineState) -> (Vec<Token>, LineState) {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut tokens = Vec::new();
    let mut i = 0;
    if let LineState::LongComment(_) = state {
        match find_close(&chars, 0) {
            Some(end) => {
                tokens.push(tok(TokenKind::Comment, 0, end));
                i = end;
            }
            None => {
                if n > 0 {
                    tokens.push(tok(TokenKind::Comment, 0, n));
                }
                return (tokens, state);
            }
        }
    }
    while i < n {
        let c = chars[i];
        let start = i;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            // `// @property …` header lines stand out from plain comments.
            let rest: String = chars[i + 2..].iter().collect();
            let kind = if rest.trim_start().starts_with('@') {
                TokenKind::EngineGlobal
            } else {
                TokenKind::Comment
            };
            tokens.push(tok(kind, start, n));
            return (tokens, LineState::Normal);
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            match find_close(&chars, i + 2) {
                Some(end) => {
                    tokens.push(tok(TokenKind::Comment, start, end));
                    i = end;
                    continue;
                }
                None => {
                    tokens.push(tok(TokenKind::Comment, start, n));
                    return (tokens, LineState::LongComment(0));
                }
            }
        }
        if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            while i < n && (chars[i].is_ascii_alphanumeric() || chars[i] == '.') {
                // Exponent signs: 1e-3.
                if matches!(chars[i], 'e' | 'E') && matches!(chars.get(i + 1), Some('+' | '-')) {
                    i += 1;
                }
                i += 1;
            }
            tokens.push(tok(TokenKind::Number, start, i));
            continue;
        }
        if c == '@' {
            // Attributes: @vertex, @group(0), @builtin(position).
            i += 1;
            while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            tokens.push(tok(TokenKind::Keyword, start, i));
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let next = chars[i..].iter().find(|c| !c.is_whitespace());
            let after_fn = tokens
                .last()
                .is_some_and(|t: &Token| chars[t.start..t.end].iter().collect::<String>() == "fn");
            let kind = if KEYWORDS.contains(&word.as_str()) {
                TokenKind::Keyword
            } else if matches!(word.as_str(), "true" | "false") {
                TokenKind::Literal
            } else if TYPES.contains(&word.as_str()) {
                TokenKind::SelfKw
            } else if ENGINE.contains(&word.as_str()) {
                TokenKind::EngineGlobal
            } else if BUILTINS.contains(&word.as_str()) {
                TokenKind::Builtin
            } else if after_fn || matches!(next, Some('(')) {
                TokenKind::Function
            } else {
                TokenKind::Identifier
            };
            tokens.push(tok(kind, start, i));
            continue;
        }
        i += 1;
        let kind = match c {
            '(' | ')' | '[' | ']' | '{' | '}' => TokenKind::Bracket,
            ',' | ';' | '.' | ':' => TokenKind::Punct,
            _ => {
                if chars
                    .get(i)
                    .is_some_and(|n| matches!(n, '=' | '>' | '<' | '&' | '|'))
                {
                    i += 1;
                }
                TokenKind::Operator
            }
        };
        tokens.push(tok(kind, start, i));
    }
    (tokens, LineState::Normal)
}

fn find_close(chars: &[char], from: usize) -> Option<usize> {
    (from..chars.len().saturating_sub(1))
        .find(|&i| chars[i] == '*' && chars[i + 1] == '/')
        .map(|i| i + 2)
}

pub fn tokenize(lines: &[String]) -> Vec<Vec<Token>> {
    let mut state = LineState::Normal;
    lines
        .iter()
        .map(|l| {
            let (t, s) = tokenize_line(l, state);
            state = s;
            t
        })
        .collect()
}

/// Indent from `{ ( [` nesting, folds for blocks spanning several lines.
pub fn structure(lines: &[String], tokens: &[Vec<Token>]) -> Structure {
    let mut stack: Vec<usize> = Vec::new();
    let mut indent = Vec::with_capacity(lines.len());
    let mut folds = Vec::new();
    for (l, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let brackets: Vec<char> = tokens[l]
            .iter()
            .filter(|t| t.kind == TokenKind::Bracket)
            .map(|t| chars[t.start])
            .collect();
        let leading = brackets
            .iter()
            .take_while(|c| matches!(c, '}' | ')' | ']'))
            .count();
        let first_is_bracket = tokens[l]
            .first()
            .is_some_and(|t| t.kind == TokenKind::Bracket);
        let dedent = if first_is_bracket { leading } else { 0 };
        indent.push(stack.len().saturating_sub(dedent));
        for c in brackets {
            match c {
                '{' | '(' | '[' => stack.push(l),
                _ => {
                    if let Some(start) = stack.pop()
                        && l > start
                    {
                        folds.push((start, l));
                    }
                }
            }
        }
    }
    folds.sort();
    folds.dedup();
    Structure { indent, folds }
}

/// Compile the buffer as the renderer would (header, wrapping, naga); `None` when it's valid.
pub fn check(source: &str, lines: &[String]) -> Option<Diagnostic> {
    if !source.contains("@shader")
        && !source.contains("fn surface")
        && !source.contains("fn fragment")
    {
        // A plain `.wgsl` include: just parse it.
        let e = misty_engine::wgpu::naga::front::wgsl::parse_str(source).err()?;
        let line = e.location(source).map_or(1, |l| l.line_number as usize);
        return Some(place(lines, line, e.message().to_owned()));
    }
    let e = misty_engine::render::custom_shader::check(source).err()?;
    Some(place(lines, e.line.max(1), e.message))
}

fn place(lines: &[String], line: usize, message: String) -> Diagnostic {
    let l = line.saturating_sub(1).min(lines.len().saturating_sub(1));
    let src = lines.get(l).map(String::as_str).unwrap_or("");
    let indent = src.chars().count() - src.trim_start().chars().count();
    Diagnostic {
        line: l,
        start: indent,
        end: src.trim_end().chars().count().max(indent + 1),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.split('\n').map(str::to_owned).collect()
    }

    #[test]
    fn highlights_wgsl_and_header_lines() {
        let l = lines(
            "// @property _Speed float = 1\nfn surface(input: SurfaceInput) -> SurfaceOutput { /* a\nb */ let x = vec3<f32>(1.0); }",
        );
        let t = tokenize(&l);
        assert_eq!(t[0][0].kind, TokenKind::EngineGlobal, "header directive");
        let kinds: Vec<TokenKind> = t[1].iter().map(|t| t.kind).collect();
        assert_eq!(kinds[0], TokenKind::Keyword);
        assert_eq!(kinds[1], TokenKind::EngineGlobal, "surface");
        assert_eq!(
            t[1].last().unwrap().kind,
            TokenKind::Comment,
            "block comment opens"
        );
        assert_eq!(t[2][0].kind, TokenKind::Comment, "and carries over");
        assert!(t[2].iter().any(|t| t.kind == TokenKind::SelfKw), "types");
        let s = structure(&l, &t);
        assert_eq!(s.indent, vec![0, 0, 1]);
        assert_eq!(s.folds, vec![(1, 2)]);
    }

    #[test]
    fn naga_errors_become_squiggles_on_their_line() {
        let src = "// @shader unlit\nfn fragment(input: SurfaceInput) -> vec4<f32> {\n    return nope;\n}\n";
        let d = check(src, &lines(src)).unwrap();
        assert_eq!(d.line, 2);
        assert!(d.message.contains("nope"), "{}", d.message);
        let ok = misty_engine::shader_asset::templates::UNLIT;
        assert!(check(ok, &lines(ok)).is_none());
        assert!(is_wgsl("Assets/Shaders/A.shader") && !is_wgsl("Assets/A.lua"));
    }
}
