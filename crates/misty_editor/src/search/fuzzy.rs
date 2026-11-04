//! Fuzzy matching shared by the search palette, the Hierarchy and the Project window.
//!
//! A query matches when its characters appear in order (case-insensitive). The score rewards
//! matches at word starts, runs of consecutive characters and prefixes, and penalises gaps, so
//! "dl" ranks "Directional Light" above "Model".

/// A successful match: higher `score` is better, `indices` are the matched char positions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Match {
    pub score: i32,
    pub indices: Vec<usize>,
}

/// How name words are compared: plain substring (Hierarchy / Project boxes) or fuzzy (palette).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Contains,
    Fuzzy,
}

const MATCH: i32 = 16;
const START_BONUS: i32 = 24;
const BOUNDARY_BONUS: i32 = 20;
const CAMEL_BONUS: i32 = 16;
const CONSECUTIVE_BONUS: i32 = 18;
const GAP_START: i32 = 5;
const GAP_EXTEND: i32 = 1;
/// Longer candidates are cut off for the DP (names are short; paths are not matched).
const MAX_LEN: usize = 256;

/// Bonus for a match at `j` given the original characters.
fn position_bonus(chars: &[char], j: usize) -> i32 {
    if j == 0 {
        return START_BONUS;
    }
    let (prev, cur) = (chars[j - 1], chars[j]);
    if matches!(prev, ' ' | '_' | '-' | '.' | '/' | '\\' | '(' | '[') {
        BOUNDARY_BONUS
    } else if prev.is_lowercase() && cur.is_uppercase() || !prev.is_numeric() && cur.is_numeric() {
        CAMEL_BONUS
    } else {
        0
    }
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Fuzzy-match one word (no spaces needed) against `text`.
pub fn fuzzy(query: &str, text: &str) -> Option<Match> {
    let q: Vec<char> = query.chars().map(lower).collect();
    if q.is_empty() {
        return Some(Match::default());
    }
    let chars: Vec<char> = text.chars().take(MAX_LEN).collect();
    let low: Vec<char> = chars.iter().map(|c| lower(*c)).collect();
    let (n, m) = (q.len(), low.len());
    if n > m {
        return None;
    }
    // Quick reject: the query must be a subsequence.
    let mut qi = 0;
    for c in &low {
        if qi < n && *c == q[qi] {
            qi += 1;
        }
    }
    if qi < n {
        return None;
    }
    // best[i][j]: best score with query[..=i] matched and query[i] at text[j].
    const NONE: i32 = i32::MIN / 2;
    let mut best = vec![NONE; n * m];
    let mut from = vec![usize::MAX; n * m];
    for i in 0..n {
        // Best "previous match + gap penalty" over k < j-1, updated as j moves right (O(m)).
        let mut run = NONE;
        let mut run_arg = usize::MAX;
        for j in i..m {
            if i > 0 && j >= 2 {
                run = run.saturating_sub(GAP_EXTEND);
                let opened = best[(i - 1) * m + j - 2];
                if opened > NONE && opened - GAP_START > run {
                    run = opened - GAP_START;
                    run_arg = j - 2;
                }
            }
            if low[j] != q[i] {
                continue;
            }
            let here = MATCH + position_bonus(&chars, j);
            if i == 0 {
                // Leading skipped characters cost a little (prefixes win).
                best[j] = here - (j as i32).min(10);
                continue;
            }
            let (mut top, mut arg) = (run, run_arg);
            if j >= 1 {
                let adjacent = best[(i - 1) * m + j - 1];
                if adjacent > NONE && adjacent + CONSECUTIVE_BONUS >= top {
                    top = adjacent + CONSECUTIVE_BONUS;
                    arg = j - 1;
                }
            }
            if top > NONE / 2 {
                best[i * m + j] = top + here;
                from[i * m + j] = arg;
            }
        }
    }
    let last = (n - 1) * m;
    let (end, score) = (0..m)
        .filter(|j| best[last + j] > NONE)
        .map(|j| (j, best[last + j]))
        .max_by_key(|(j, s)| (*s, std::cmp::Reverse(*j)))?;
    let mut indices = vec![0; n];
    let mut j = end;
    for i in (0..n).rev() {
        indices[i] = j;
        j = from[i * m + j];
    }
    // Exact names and shorter names rank first among equals.
    let exact = if n == m { 40 } else { 0 };
    let length = ((m - n) as i32).min(30) / 3;
    Some(Match {
        score: score + exact - length,
        indices,
    })
}

/// Case-insensitive substring match (the Hierarchy / Project search boxes).
pub fn contains(word: &str, text: &str) -> Option<Match> {
    let low = text.to_lowercase();
    let w = word.to_lowercase();
    let byte = low.find(&w)?;
    let start = low[..byte].chars().count();
    let len = w.chars().count();
    Some(Match {
        score: MATCH * len as i32 - start as i32,
        indices: (start..start + len).collect(),
    })
}

/// All `words` must match `text`; scores add up and highlights are merged.
pub fn match_words<S: AsRef<str>>(words: &[S], text: &str, mode: Mode) -> Option<Match> {
    let mut total = Match::default();
    for w in words {
        let m = match mode {
            Mode::Contains => contains(w.as_ref(), text)?,
            Mode::Fuzzy => fuzzy(w.as_ref(), text)?,
        };
        total.score += m.score;
        total.indices.extend(m.indices);
    }
    total.indices.sort_unstable();
    total.indices.dedup();
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(q: &str, t: &str) -> i32 {
        fuzzy(q, t).map(|m| m.score).unwrap_or(i32::MIN)
    }

    #[test]
    fn subsequence_required() {
        assert!(fuzzy("cube", "Cube").is_some());
        assert!(fuzzy("cbe", "Cube").is_some());
        assert!(fuzzy("ebuc", "Cube").is_none());
        assert!(fuzzy("cubes", "Cube").is_none());
        assert_eq!(fuzzy("", "x"), Some(Match::default()));
    }

    #[test]
    fn indices_point_at_matched_chars() {
        let m = fuzzy("dl", "Directional Light").unwrap();
        assert_eq!(m.indices, vec![0, 12], "word starts preferred over 'Di'");
        let m = fuzzy("lig", "Point Light").unwrap();
        assert_eq!(m.indices, vec![6, 7, 8]);
        // Non-ASCII names index by char, not byte.
        let m = fuzzy("é", "Café").unwrap();
        assert_eq!(m.indices, vec![3]);
    }

    #[test]
    fn ranking() {
        // Prefix beats middle.
        assert!(score("cam", "Camera") > score("cam", "Main Camera"));
        // Consecutive beats scattered.
        assert!(score("light", "Light Probe") > score("light", "Left Igloo Gate Hut"));
        // Word starts beat inner letters.
        assert!(score("pl", "Point Light") > score("pl", "Apple"));
        // CamelCase boundaries count as word starts.
        assert!(score("mr", "MeshRenderer") > score("mr", "Mirror"));
        // Exact match beats a longer name.
        assert!(score("cube", "Cube") > score("cube", "Cube (1)"));
    }

    #[test]
    fn words_and_modes() {
        let m = match_words(&["red", "mat"], "Red Metal.mat", Mode::Fuzzy).unwrap();
        assert!(m.indices.contains(&0) && m.indices.len() >= 5);
        assert!(match_words(&["red", "zzz"], "Red Metal.mat", Mode::Fuzzy).is_none());
        // Contains mode is a plain substring check.
        assert!(match_words(&["cam"], "Main Camera", Mode::Contains).is_some());
        assert!(match_words(&["mcam"], "Main Camera", Mode::Contains).is_none());
        let m = contains("cam", "Main Camera").unwrap();
        assert_eq!(m.indices, vec![5, 6, 7]);
        assert!(match_words::<&str>(&[], "anything", Mode::Fuzzy).is_some());
    }
}
