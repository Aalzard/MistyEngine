//! The small Markdown subset the User Guide is written in: `#`–`###` headings, paragraphs,
//! `-` and `1.` lists, fenced code blocks, `![alt](path)` images on their own line, pipe tables,
//! `> [!NOTE]` / `[!TIP]` / `[!WARNING]` / `[!PROGRAMMERS]` boxes (other quotes are notes), and
//! inline `**bold**`, `*italic*`, `` `code` `` and `[links](target)`.

/// Kind of a note box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteKind {
    Note,
    Tip,
    Warning,
    Programmers,
}

impl NoteKind {
    fn parse(tag: &str) -> Option<Self> {
        Some(match tag.to_ascii_uppercase().as_str() {
            "NOTE" | "IMPORTANT" => Self::Note,
            "TIP" => Self::Tip,
            "WARNING" | "CAUTION" => Self::Warning,
            "PROGRAMMERS" => Self::Programmers,
            _ => return None,
        })
    }
}

/// One block of a chapter.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading {
        level: u8,
        text: String,
    },
    Paragraph(String),
    List {
        ordered: bool,
        /// Items; an item's continuation lines are joined with spaces, nested items (indented
        /// by two or more spaces) are kept as their own items with a depth.
        items: Vec<(usize, String)>,
    },
    Code {
        lang: String,
        text: String,
    },
    Image {
        alt: String,
        path: String,
    },
    Table {
        header: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Note {
        kind: NoteKind,
        blocks: Vec<Block>,
    },
}

/// A run of inline text.
#[derive(Clone, Debug, PartialEq)]
pub enum Span {
    Text(String),
    Bold(String),
    Italic(String),
    Code(String),
    Link { text: String, target: String },
}

/// The plain text of some inline Markdown (formatting marks and link targets removed).
pub fn plain(text: &str) -> String {
    inline(text)
        .into_iter()
        .map(|s| match s {
            Span::Text(t) | Span::Bold(t) | Span::Italic(t) | Span::Code(t) => t,
            Span::Link { text, .. } => text,
        })
        .collect()
}

/// Inline Markdown → spans.
pub fn inline(text: &str) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    let flush = |buf: &mut String, out: &mut Vec<Span>| {
        if !buf.is_empty() {
            out.push(Span::Text(std::mem::take(buf)));
        }
    };
    let find = |from: usize, pat: &[char]| -> Option<usize> {
        (from..chars.len().saturating_sub(pat.len() - 1)).find(|&j| chars[j..].starts_with(pat))
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '`'
            && let Some(end) = find(i + 1, &['`'])
        {
            flush(&mut buf, &mut out);
            out.push(Span::Code(chars[i + 1..end].iter().collect()));
            i = end + 1;
            continue;
        }
        if c == '*' && chars.get(i + 1) == Some(&'*') {
            if let Some(end) = find(i + 2, &['*', '*'])
                && end > i + 2
            {
                flush(&mut buf, &mut out);
                out.push(Span::Bold(plain(
                    &chars[i + 2..end].iter().collect::<String>(),
                )));
                i = end + 2;
                continue;
            }
        } else if c == '*'
            && chars.get(i + 1).is_some_and(|n| !n.is_whitespace())
            && let Some(end) = find(i + 1, &['*'])
            && end > i + 1
        {
            flush(&mut buf, &mut out);
            out.push(Span::Italic(plain(
                &chars[i + 1..end].iter().collect::<String>(),
            )));
            i = end + 1;
            continue;
        }
        if c == '['
            && let Some(close) = find(i + 1, &[']', '('])
            && let Some(end) = find(close + 2, &[')'])
        {
            flush(&mut buf, &mut out);
            out.push(Span::Link {
                text: plain(&chars[i + 1..close].iter().collect::<String>()),
                target: chars[close + 2..end].iter().collect(),
            });
            i = end + 1;
            continue;
        }
        buf.push(c);
        i += 1;
    }
    flush(&mut buf, &mut out);
    out
}

fn list_item(line: &str) -> Option<(bool, usize, &str)> {
    let indent = line.len() - line.trim_start().len();
    let t = line.trim_start();
    if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
        return Some((false, indent / 2, rest));
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    if digits > 0
        && let Some(rest) = t[digits..].strip_prefix(". ")
    {
        return Some((true, indent / 2, rest));
    }
    None
}

fn image(line: &str) -> Option<(String, String)> {
    let t = line.trim();
    let rest = t.strip_prefix("![")?;
    let (alt, rest) = rest.split_once("](")?;
    let path = rest.strip_suffix(')')?;
    Some((alt.to_owned(), path.to_owned()))
}

fn cells(line: &str) -> Vec<String> {
    let t = line.trim().trim_start_matches('|').trim_end_matches('|');
    t.split('|').map(|c| c.trim().to_owned()).collect()
}

fn is_rule(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// A chapter's Markdown → blocks.
pub fn parse(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim();
        if t.is_empty() {
            i += 1;
            continue;
        }
        if let Some(fence) = t.strip_prefix("```") {
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with("```") {
                body.push(lines[i]);
                i += 1;
            }
            i += 1;
            out.push(Block::Code {
                lang: fence.trim().to_owned(),
                text: body.join("\n"),
            });
            continue;
        }
        if t.starts_with('#') {
            let level = t.chars().take_while(|c| *c == '#').count().min(3) as u8;
            out.push(Block::Heading {
                level,
                text: t.trim_start_matches('#').trim().to_owned(),
            });
            i += 1;
            continue;
        }
        if t.starts_with('>') {
            let mut inner = Vec::new();
            while i < lines.len() && lines[i].trim().starts_with('>') {
                let l = lines[i].trim().trim_start_matches('>');
                inner.push(l.strip_prefix(' ').unwrap_or(l));
                i += 1;
            }
            let mut kind = NoteKind::Note;
            if let Some(first) = inner.first()
                && let Some(tag) = first
                    .trim()
                    .strip_prefix("[!")
                    .and_then(|r| r.strip_suffix(']'))
                && let Some(k) = NoteKind::parse(tag)
            {
                kind = k;
                inner.remove(0);
            }
            out.push(Block::Note {
                kind,
                blocks: parse(&inner.join("\n")),
            });
            continue;
        }
        if let Some((alt, path)) = image(t) {
            out.push(Block::Image { alt, path });
            i += 1;
            continue;
        }
        if t.starts_with('|') && i + 1 < lines.len() && is_rule(lines[i + 1]) {
            let header = cells(t);
            i += 2;
            let mut rows = Vec::new();
            while i < lines.len() && lines[i].trim().starts_with('|') {
                rows.push(cells(lines[i]));
                i += 1;
            }
            out.push(Block::Table { header, rows });
            continue;
        }
        if let Some((ordered, _, _)) = list_item(line) {
            let mut items: Vec<(usize, String)> = Vec::new();
            while i < lines.len() {
                let l = lines[i];
                if l.trim().is_empty() {
                    // A blank line ends the list unless another item follows.
                    if lines
                        .get(i + 1)
                        .is_some_and(|n| list_item(n).is_some_and(|(o, _, _)| o == ordered))
                    {
                        i += 1;
                        continue;
                    }
                    break;
                }
                if let Some((_, depth, rest)) = list_item(l) {
                    items.push((depth, rest.trim().to_owned()));
                } else if l.starts_with(' ') && !items.is_empty() {
                    let last = &mut items.last_mut().expect("checked").1;
                    last.push(' ');
                    last.push_str(l.trim());
                } else {
                    break;
                }
                i += 1;
            }
            out.push(Block::List { ordered, items });
            continue;
        }
        // A paragraph: lines up to a blank line or another block.
        let mut para = Vec::new();
        while i < lines.len() {
            let l = lines[i].trim();
            if l.is_empty()
                || l.starts_with('#')
                || l.starts_with("```")
                || l.starts_with('>')
                || image(l).is_some()
                || (l.starts_with('|') && i + 1 < lines.len() && is_rule(lines[i + 1]))
                || (!para.is_empty() && list_item(lines[i]).is_some())
            {
                break;
            }
            para.push(l);
            i += 1;
        }
        out.push(Block::Paragraph(para.join(" ")));
    }
    out
}

/// `heading text` → the anchor GitHub gives it (`#the-editor-layout`).
pub fn anchor(heading: &str) -> String {
    let mut out = String::new();
    for c in plain(heading).chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if c == ' ' || c == '-' {
            out.push('-');
        }
    }
    out
}

/// Every link target and image path of some blocks (link and image checks).
#[cfg(test)]
pub fn references(blocks: &[Block]) -> (Vec<String>, Vec<String>) {
    let mut links = Vec::new();
    let mut images = Vec::new();
    let spans = |text: &str, links: &mut Vec<String>| {
        for s in inline(text) {
            if let Span::Link { target, .. } = s {
                links.push(target);
            }
        }
    };
    for b in blocks {
        match b {
            Block::Heading { text, .. } | Block::Paragraph(text) => spans(text, &mut links),
            Block::List { items, .. } => {
                for (_, t) in items {
                    spans(t, &mut links);
                }
            }
            Block::Table { header, rows } => {
                for c in header.iter().chain(rows.iter().flatten()) {
                    spans(c, &mut links);
                }
            }
            Block::Image { path, .. } => images.push(path.clone()),
            Block::Note { blocks, .. } => {
                let (l, i) = references(blocks);
                links.extend(l);
                images.extend(i);
            }
            Block::Code { .. } => {}
        }
    }
    (links, images)
}
