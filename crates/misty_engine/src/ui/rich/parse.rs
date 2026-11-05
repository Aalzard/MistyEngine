//! Rich text markup: `<b> <i> <u> <s> <color> <size> <alpha> <mark> <sup> <sub> <br> <sprite>
//! <link> <align> <noparse>` turned into characters that each carry their style.
//!
//! Like the reference engine, a tag that isn't recognised (or is malformed) stays in the text
//! as typed, closing tags pop their own kind only, and `<noparse>…</noparse>` shows its content
//! literally. `&lt;`, `&gt;` and `&amp;` are the other way to write the markup characters.

/// A font size change: `<size=20>`, `<size=150%>`, `<size=+4>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SizeSpec {
    /// Canvas units.
    Abs(f32),
    /// Factor of the text's base size (not compounding, like the reference engine).
    Rel(f32),
    /// Canvas units added to the base size.
    Add(f32),
}

impl SizeSpec {
    pub fn resolve(self, base: f32) -> f32 {
        match self {
            SizeSpec::Abs(v) => v,
            SizeSpec::Rel(f) => base * f,
            SizeSpec::Add(d) => base + d,
        }
        .max(0.5)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Script {
    #[default]
    Normal,
    Super,
    Sub,
}

/// Horizontal alignment override of `<align=…>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HAlign {
    Left,
    Center,
    Right,
}

/// Style of one character.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    /// sRGB colour + alpha replacing the text colour.
    pub color: Option<[f32; 4]>,
    /// Alpha multiplier (`<alpha=#80>`).
    pub alpha: Option<f32>,
    pub size: Option<SizeSpec>,
    /// Highlight behind the character.
    pub mark: Option<[f32; 4]>,
    pub script: Script,
    /// Index into [`RichText::links`].
    pub link: Option<u16>,
    pub align: Option<HAlign>,
}

/// Which sprite of the Sprite Asset an inline `<sprite>` shows.
#[derive(Clone, Debug, PartialEq)]
pub enum SpriteKey {
    Name(String),
    Index(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpriteTag {
    pub key: SpriteKey,
    /// `color=#…` tint (white otherwise; `tint=1` uses the text colour).
    pub color: Option<[f32; 4]>,
    pub tint: bool,
}

/// One character of the parsed text. `'\n'` breaks the line; [`OBJECT`] stands for a sprite.
#[derive(Clone, Debug, PartialEq)]
pub struct RichChar {
    pub ch: char,
    pub style: Style,
    /// Index into [`RichText::sprites`] when `ch` is [`OBJECT`].
    pub sprite: Option<u16>,
}

/// The character standing in for an inline sprite.
pub const OBJECT: char = '\u{FFFC}';

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichText {
    pub chars: Vec<RichChar>,
    /// Link ids by [`Style::link`] index.
    pub links: Vec<String>,
    pub sprites: Vec<SpriteTag>,
}

impl RichText {
    /// The visible characters as a string (sprites as U+FFFC).
    pub fn plain(&self) -> String {
        self.chars.iter().map(|c| c.ch).collect()
    }
}

/// Named colours `<color=name>` understands.
pub fn named_color(name: &str) -> Option<[f32; 4]> {
    let rgb: [u8; 3] = match name.to_ascii_lowercase().as_str() {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 255, 0],
        "blue" => [0, 0, 255],
        "yellow" => [255, 255, 0],
        "cyan" | "aqua" => [0, 255, 255],
        "magenta" | "fuchsia" => [255, 0, 255],
        "orange" => [255, 128, 0],
        "purple" => [160, 32, 240],
        "grey" | "gray" => [128, 128, 128],
        "lightblue" => [173, 216, 230],
        "brown" => [165, 42, 42],
        "pink" => [255, 192, 203],
        "lime" => [0, 255, 0],
        "navy" => [0, 0, 128],
        "teal" => [0, 128, 128],
        "olive" => [128, 128, 0],
        "maroon" => [128, 0, 0],
        "silver" => [192, 192, 192],
        _ => return None,
    };
    Some([
        rgb[0] as f32 / 255.0,
        rgb[1] as f32 / 255.0,
        rgb[2] as f32 / 255.0,
        1.0,
    ])
}

/// `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA` or a colour name.
pub fn parse_color(v: &str) -> Option<[f32; 4]> {
    let Some(hex) = v.strip_prefix('#') else {
        return named_color(v);
    };
    let nib = |c: char| c.to_digit(16).map(|d| d as f32);
    let chars: Vec<char> = hex.chars().collect();
    let vals: Vec<f32> = match chars.len() {
        3 | 4 => chars
            .iter()
            .map(|c| nib(*c).map(|d| d * 17.0 / 255.0))
            .collect::<Option<_>>()?,
        6 | 8 => chars
            .chunks(2)
            .map(|p| Some((nib(p[0])? * 16.0 + nib(p[1])?) / 255.0))
            .collect::<Option<_>>()?,
        _ => return None,
    };
    Some([
        vals[0],
        vals[1],
        vals[2],
        vals.get(3).copied().unwrap_or(1.0),
    ])
}

/// `<alpha=#80>` (also `#8` and a plain 0–1 number).
fn parse_alpha(v: &str) -> Option<f32> {
    match v.strip_prefix('#') {
        Some(h) if h.len() == 2 => u8::from_str_radix(h, 16).ok().map(|a| a as f32 / 255.0),
        Some(h) if h.len() == 1 => u8::from_str_radix(h, 16).ok().map(|a| a as f32 / 15.0),
        Some(_) => None,
        None => v.parse::<f32>().ok().map(|a| a.clamp(0.0, 1.0)),
    }
}

fn parse_size(v: &str) -> Option<SizeSpec> {
    let v = v.trim();
    if let Some(p) = v.strip_suffix('%') {
        return p.parse::<f32>().ok().map(|p| SizeSpec::Rel(p / 100.0));
    }
    let num = v.strip_suffix("px").unwrap_or(v);
    if num.starts_with('+') || num.starts_with('-') {
        return num.parse::<f32>().ok().map(SizeSpec::Add);
    }
    num.parse::<f32>().ok().map(SizeSpec::Abs)
}

fn unquote(v: &str) -> &str {
    let v = v.trim();
    for q in ['"', '\''] {
        if let Some(inner) = v.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return inner;
        }
    }
    v
}

/// A tag's name, its `=value` and `key=value` attributes.
struct Tag<'a> {
    closing: bool,
    name: String,
    value: Option<&'a str>,
    attrs: Vec<(String, &'a str)>,
}

/// Split `inner` (between `<` and `>`) into name, value and attributes; quoted values may hold
/// spaces.
fn parse_tag(inner: &str) -> Option<Tag<'_>> {
    let (closing, body) = match inner.strip_prefix('/') {
        Some(r) => (true, r),
        None => (false, inner),
    };
    if body.is_empty() || body.starts_with(char::is_whitespace) {
        return None;
    }
    // Tokens: words separated by spaces, keeping quoted runs together.
    let mut tokens: Vec<&str> = Vec::new();
    let bytes = body.as_bytes();
    let (mut i, mut start, mut quote) = (0, None::<usize>, None::<u8>);
    while i <= bytes.len() {
        let c = bytes.get(i).copied();
        match (c, quote) {
            (Some(q), Some(open)) if q == open => quote = None,
            (Some(_), Some(_)) => {}
            (Some(b'"' | b'\''), None) => {
                quote = c;
                start.get_or_insert(i);
            }
            (Some(b' ' | b'\t'), None) | (None, _) => {
                if let Some(s) = start.take() {
                    tokens.push(&body[s..i]);
                }
            }
            (Some(_), None) => {
                start.get_or_insert(i);
            }
        }
        i += 1;
    }
    let first = tokens.first()?;
    let (name, value) = match first.split_once('=') {
        Some((n, v)) => (n, Some(unquote(v))),
        None => (*first, None),
    };
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let attrs = tokens[1..]
        .iter()
        .filter_map(|t| t.split_once('='))
        .map(|(k, v)| (k.to_ascii_lowercase(), unquote(v)))
        .collect();
    Some(Tag {
        closing,
        name: name.to_ascii_lowercase(),
        value,
        attrs,
    })
}

#[derive(Default)]
struct State {
    bold: u32,
    italic: u32,
    underline: u32,
    strike: u32,
    colors: Vec<[f32; 4]>,
    alpha: Option<f32>,
    sizes: Vec<SizeSpec>,
    marks: Vec<[f32; 4]>,
    scripts: Vec<Script>,
    links: Vec<u16>,
    aligns: Vec<HAlign>,
}

impl State {
    fn style(&self) -> Style {
        Style {
            bold: self.bold > 0,
            italic: self.italic > 0,
            underline: self.underline > 0,
            strike: self.strike > 0,
            color: self.colors.last().copied(),
            alpha: self.alpha,
            size: self.sizes.last().copied(),
            mark: self.marks.last().copied(),
            script: self.scripts.last().copied().unwrap_or_default(),
            link: self.links.last().copied(),
            align: self.aligns.last().copied(),
        }
    }

    /// Apply a tag; false when it isn't one we know (it then shows as text).
    fn apply(&mut self, tag: &Tag, out: &mut RichText) -> bool {
        let dec = |n: &mut u32| *n = n.saturating_sub(1);
        if tag.closing {
            match tag.name.as_str() {
                "b" => dec(&mut self.bold),
                "i" => dec(&mut self.italic),
                "u" => dec(&mut self.underline),
                "s" => dec(&mut self.strike),
                "color" => {
                    self.colors.pop();
                }
                "alpha" => self.alpha = None,
                "size" => {
                    self.sizes.pop();
                }
                "mark" => {
                    self.marks.pop();
                }
                "sup" | "sub" => {
                    self.scripts.pop();
                }
                "link" => {
                    self.links.pop();
                }
                "align" => {
                    self.aligns.pop();
                }
                _ => return false,
            }
            return true;
        }
        let value = tag.value;
        match tag.name.as_str() {
            "b" => self.bold += 1,
            "i" => self.italic += 1,
            "u" => self.underline += 1,
            "s" => self.strike += 1,
            "sup" => self.scripts.push(Script::Super),
            "sub" => self.scripts.push(Script::Sub),
            "color" => match value.and_then(parse_color) {
                Some(c) => self.colors.push(c),
                None => return false,
            },
            "alpha" => match value.and_then(parse_alpha) {
                Some(a) => self.alpha = Some(a),
                None => return false,
            },
            "size" => match value.and_then(parse_size) {
                Some(s) => self.sizes.push(s),
                None => return false,
            },
            "mark" => match value.and_then(parse_color) {
                // A mark without its own alpha is see-through, like a highlighter.
                Some(mut c) => {
                    if value.is_some_and(|v| v.len() == 7 || v.len() == 4) {
                        c[3] = 0.5;
                    }
                    self.marks.push(c)
                }
                None => return false,
            },
            "link" => {
                let id = value
                    .or_else(|| tag.attrs.iter().find(|(k, _)| k == "id").map(|(_, v)| *v))
                    .unwrap_or("");
                out.links.push(id.to_owned());
                self.links.push((out.links.len() - 1) as u16);
            }
            "align" => {
                let a = match value.map(str::to_ascii_lowercase).as_deref() {
                    Some("left") | Some("justified") | Some("flush") => HAlign::Left,
                    Some("center") => HAlign::Center,
                    Some("right") => HAlign::Right,
                    _ => return false,
                };
                self.aligns.push(a);
            }
            "br" => out.chars.push(RichChar {
                ch: '\n',
                style: self.style(),
                sprite: None,
            }),
            "sprite" => {
                let attr = |k: &str| tag.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| *v);
                let key = if let Some(n) = attr("name") {
                    SpriteKey::Name(n.to_owned())
                } else if let Some(i) = attr("index").or(value) {
                    match i.parse::<usize>() {
                        Ok(i) => SpriteKey::Index(i),
                        Err(_) => SpriteKey::Name(i.to_owned()),
                    }
                } else {
                    return false;
                };
                out.sprites.push(SpriteTag {
                    key,
                    color: attr("color").and_then(parse_color),
                    tint: attr("tint").is_some_and(|t| t == "1" || t == "true"),
                });
                out.chars.push(RichChar {
                    ch: OBJECT,
                    style: self.style(),
                    sprite: Some((out.sprites.len() - 1) as u16),
                });
            }
            _ => return false,
        }
        true
    }
}

/// Parse markup into styled characters.
pub fn parse(text: &str) -> RichText {
    let mut out = RichText::default();
    let mut st = State::default();
    let mut rest = text;
    let mut noparse = false;
    while let Some(c) = rest.chars().next() {
        if noparse {
            if rest.len() >= 10 && rest[..10].eq_ignore_ascii_case("</noparse>") {
                noparse = false;
                rest = &rest[10..];
                continue;
            }
        } else if c == '<'
            && let Some(end) = rest.find('>')
        {
            let inner = &rest[1..end];
            // A '<' inside (`a < b <b>`) means this one isn't a tag.
            if !inner.contains('<') {
                if inner.eq_ignore_ascii_case("noparse") {
                    noparse = true;
                    rest = &rest[end + 1..];
                    continue;
                }
                if let Some(tag) = parse_tag(inner)
                    && st.apply(&tag, &mut out)
                {
                    rest = &rest[end + 1..];
                    continue;
                }
            }
        } else if c == '&' {
            let entity = [("&lt;", '<'), ("&gt;", '>'), ("&amp;", '&')]
                .into_iter()
                .find(|(e, _)| rest.starts_with(e));
            if let Some((e, ch)) = entity {
                out.chars.push(RichChar {
                    ch,
                    style: st.style(),
                    sprite: None,
                });
                rest = &rest[e.len()..];
                continue;
            }
        }
        out.chars.push(RichChar {
            ch: c,
            style: st.style(),
            sprite: None,
        });
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// Does `text` contain anything that parses as markup (so it needs the rich text path)?
pub fn has_markup(text: &str) -> bool {
    (text.contains('<') || text.contains('&')) && {
        let r = parse(text);
        r.chars.len() != text.chars().count() || r.chars.iter().any(|c| c.style != Style::default())
    }
}
