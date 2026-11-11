//! The console store: bounded memory, counters and indices across dropped lines, and the rows
//! kept for the panel (filter, search, Collapse).

use super::*;

/// What the list shows, worked out from scratch the plain way (the panel did this each frame
/// before the rows were kept).
fn rows_from_scratch(c: &Console) -> Vec<Row> {
    let needle = c.search.to_lowercase();
    let mut rows: Vec<Row> = Vec::new();
    for (index, e) in c.since(0) {
        let level_ok = match e.level {
            LogLevel::Info => c.show_info,
            LogLevel::Warning => c.show_warnings,
            LogLevel::Error => c.show_errors,
        };
        if !level_ok || !(needle.is_empty() || e.message.to_lowercase().contains(&needle)) {
            continue;
        }
        if c.collapse
            && let Some(r) = rows.iter_mut().find(|r| {
                let o = c.get(r.index).unwrap();
                o.level == e.level && o.message == e.message
            })
        {
            r.count += 1;
            continue;
        }
        rows.push(Row { index, count: 1 });
    }
    rows
}

fn rows_now(c: &mut Console) -> Vec<Row> {
    c.rows().iter().copied().collect()
}

/// Bytes the store holds: the ring, the texts and the rows.
fn held_bytes(c: &Console) -> usize {
    c.entries.capacity() * std::mem::size_of::<LogEntry>()
        + c.entries
            .iter()
            .map(|e| e.message.capacity() + e.time.capacity())
            .sum::<usize>()
        + c.rows.rows.capacity() * std::mem::size_of::<Row>()
        + c.rows.seen.capacity() * 2 * std::mem::size_of::<u64>()
}

fn line(i: usize) -> String {
    format!("Player position 12.5, 3.0, 7.25 (frame {i})")
}

/// A small repeatable random sequence.
struct Rng(u64);

impl Rng {
    fn next(&mut self, below: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % below
    }
}

#[test]
fn memory_stays_bounded_however_much_is_logged() {
    let mut c = Console::default();
    assert_eq!(c.capacity(), DEFAULT_CAPACITY);
    c.rows();
    let mut after_first_fill = 0;
    for i in 0..200_000 {
        c.info(line(i));
        if i == 2 * DEFAULT_CAPACITY {
            after_first_fill = held_bytes(&c);
        }
    }
    assert_eq!(c.entries().len(), DEFAULT_CAPACITY);
    assert!(c.entries().capacity() <= 2 * DEFAULT_CAPACITY);
    assert_eq!(c.rows().len(), DEFAULT_CAPACITY);
    let held = held_bytes(&c);
    // A few hundred bytes a line of this length; ten times the lines hold no more.
    assert!(held < 4_000_000, "{held} bytes");
    assert!(
        held <= after_first_fill + after_first_fill / 10,
        "{held} bytes after 200k lines, {after_first_fill} after 20k"
    );
    // The newest lines are the ones kept.
    assert_eq!(c.last().unwrap().message, line(199_999));
    assert_eq!(c.entries()[0].message, line(190_000));
    // Collapse keeps its table bounded too.
    c.collapse = true;
    for i in 0..50_000 {
        c.info(line(i));
        c.rows();
    }
    assert!(c.rows.seen.len() <= DEFAULT_CAPACITY);
    assert!(held_bytes(&c) < 4_500_000, "{} bytes", held_bytes(&c));
}

#[test]
fn counters_count_dropped_lines_too() {
    let mut c = Console::default();
    c.set_capacity(1_000);
    for i in 0..30_000 {
        match i % 10 {
            0 => c.log(LogLevel::Error, format!("error {i}")),
            1 | 2 => c.warn(format!("warning {i}")),
            _ => c.info(line(i)),
        }
    }
    assert_eq!(c.entries().len(), 1_000);
    assert_eq!(c.count(LogLevel::Error), 3_000);
    assert_eq!(c.count(LogLevel::Warning), 6_000);
    assert_eq!(c.count(LogLevel::Info), 21_000);
    assert_eq!(c.total(), 30_000);
    // Clear starts the counters again; the indices go on.
    c.clear();
    assert_eq!(c.total(), 0);
    assert!(c.entries().is_empty() && c.last().is_none());
    assert_eq!((c.first_index(), c.next_index()), (30_000, 30_000));
    c.warn("after clear");
    assert_eq!(c.count(LogLevel::Warning), 1);
    assert_eq!(c.get(30_000).unwrap().message, "after clear");
}

#[test]
fn indices_never_change_when_lines_are_dropped() {
    let mut c = Console::default();
    c.set_capacity(100);
    for i in 0..250 {
        c.info(format!("line {i}"));
    }
    assert_eq!((c.first_index(), c.next_index()), (150, 250));
    assert!(c.get(149).is_none(), "dropped");
    assert_eq!(c.get(150).unwrap().message, "line 150");
    assert_eq!(c.get(249).unwrap().message, "line 249");
    assert!(c.get(250).is_none(), "not logged yet");
    // From a dropped index: what is left, from the oldest kept line.
    let all: Vec<u64> = c.since(0).map(|(i, _)| i).collect();
    assert_eq!(all, (150..250).collect::<Vec<u64>>());
    let (i, e) = c.since(200).next().unwrap();
    assert_eq!((i, e.message.as_str()), (200, "line 200"));
    assert_eq!(c.since(250).count(), 0);
    assert_eq!(c.since(u64::MAX).count(), 0);
    // A smaller size drops the oldest lines at once; the size has a floor.
    c.set_capacity(1_000);
    c.set_capacity(10);
    assert_eq!(c.capacity(), 100);
    c.selected = Some(150);
    c.info("one more");
    assert_eq!(c.first_index(), 151);
    assert_eq!(c.selected, None, "the selected line was dropped");
}

#[test]
fn level_toggles_and_search_pick_the_rows() {
    let mut c = Console::default();
    c.info("Loaded scene Main");
    c.warn("Texture has no mipmaps");
    c.log(
        LogLevel::Error,
        "Assets/Scripts/Player.lua:12: nil value\nstack traceback:",
    );
    c.info("loaded SCENE Menu");
    let index = |c: &mut Console| rows_now(c).iter().map(|r| r.index).collect::<Vec<_>>();
    assert_eq!(index(&mut c), [0, 1, 2, 3]);
    c.show_info = false;
    assert_eq!(index(&mut c), [1, 2]);
    c.show_errors = false;
    assert_eq!(index(&mut c), [1]);
    c.show_warnings = false;
    assert!(index(&mut c).is_empty());
    (c.show_info, c.show_warnings, c.show_errors) = (true, true, true);
    // Search: any case, anywhere in the message (later lines too).
    c.search = "scene".into();
    assert_eq!(index(&mut c), [0, 3]);
    c.search = "TRACEBACK".into();
    assert_eq!(index(&mut c), [2]);
    c.search = "scene".into();
    c.show_info = false;
    assert!(index(&mut c).is_empty());
    c.show_info = true;
    // New lines join the rows only when they pass.
    c.warn("Scene saved with warnings");
    c.info("unrelated");
    assert_eq!(index(&mut c), [0, 3, 4]);
    c.search.clear();
    assert_eq!(index(&mut c).len(), 6);
}

#[test]
fn collapse_merges_identical_messages() {
    let mut c = Console::default();
    c.collapse = true;
    c.info("tick");
    c.warn("tick");
    c.info("tock");
    c.info("tick");
    c.info("tick");
    let rows = rows_now(&mut c);
    let row = |index, count| Row { index, count };
    // The same text at another level is another message.
    assert_eq!(rows, [row(0, 3), row(1, 1), row(2, 1)]);
    c.collapse = false;
    assert_eq!(rows_now(&mut c).len(), 5);
}

#[test]
fn rows_are_kept_not_rebuilt_each_frame() {
    for collapse in [false, true] {
        let mut c = Console::default();
        c.set_capacity(500);
        c.collapse = collapse;
        c.search = "frame".into();
        c.rows();
        let built = c.row_rebuilds();
        // A script printing every frame, far past the size of the store, drawn every frame.
        for i in 0..5_000 {
            c.info(line(i));
            c.warn("slow frame");
            c.log(LogLevel::Error, "not shown");
            c.rows();
        }
        assert_eq!(c.row_rebuilds(), built, "collapse {collapse}");
        assert_eq!(rows_now(&mut c), rows_from_scratch(&c));
        // A change of filter or search builds them once.
        c.search = "slow".into();
        c.rows();
        c.rows();
        assert_eq!(c.row_rebuilds(), built + 1);
        c.show_warnings = false;
        assert!(c.rows().is_empty());
        assert_eq!(c.row_rebuilds(), built + 2);
        // So does Clear (nothing to go through).
        c.clear();
        assert!(c.rows().is_empty());
        c.show_warnings = true;
        c.warn("slow again");
        assert_eq!(rows_now(&mut c).len(), 1);
    }
}

#[test]
fn kept_rows_match_rows_built_from_scratch() {
    let mut rng = Rng(39);
    for round in 0..20 {
        let mut c = Console::default();
        c.set_capacity(100 + rng.next(150) as usize);
        c.collapse = round % 2 == 0;
        for step in 0..1_000 {
            match rng.next(40) {
                0 => c.show_info = !c.show_info,
                1 => c.show_warnings = !c.show_warnings,
                2 => c.show_errors = !c.show_errors,
                3 => c.search = ["", "a", "B", "message 1", "zzz"][rng.next(5) as usize].into(),
                4 => c.collapse = !c.collapse,
                5 if rng.next(8) == 0 => c.clear(),
                6 => c.set_capacity(100 + rng.next(150) as usize),
                _ => {
                    let level =
                        [LogLevel::Info, LogLevel::Warning, LogLevel::Error][rng.next(3) as usize];
                    // Few different texts, so Collapse has plenty to merge and to move.
                    let text = match rng.next(4) {
                        0 => "a burst".to_owned(),
                        1 => format!("Message {}", rng.next(12)),
                        2 => format!("b {}", rng.next(400)),
                        _ => format!("unique {step}"),
                    };
                    c.log(level, text);
                }
            }
            // The panel is not always open: look at the rows only now and then.
            if rng.next(3) != 0 {
                assert_eq!(
                    rows_now(&mut c),
                    rows_from_scratch(&c),
                    "round {round} step {step}"
                );
            }
        }
        assert_eq!(rows_now(&mut c), rows_from_scratch(&c));
        let counted: usize = c.rows().iter().map(|r| r.count).sum();
        assert!(counted <= c.entries().len());
    }
}
