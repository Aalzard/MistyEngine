//! Keyboard input of the focused text area: shortcuts, chords, caret keys, typing, clipboard and
//! the autocomplete popup.

use egui::{Event, Key, Modifiers, Ui};

use super::Document;
use super::completion;
use super::history::EditKind;
use super::keys::{self, Cmd, Lookup};
use super::text::{Pos, Selection};
use super::view::{Completion, ViewCtx, ViewOutput};

/// Handle this frame's key, text and clipboard events (they are taken from the input queue).
pub fn keyboard(ui: &Ui, doc: &mut Document, cx: &mut ViewCtx<'_>, out: &mut ViewOutput) {
    let events: Vec<Event> = ui.input_mut(|i| {
        let mine = i
            .events
            .iter()
            .filter(|e| ours(e) && !matches!(e, Event::Key { pressed: false, .. }))
            .cloned()
            .collect();
        i.events.retain(|e| !ours(e));
        mine
    });
    let mut suppress_text = false;
    for ev in events {
        match ev {
            Event::Key { key, modifiers, .. } => {
                suppress_text = false;
                if completion_key(doc, cx, key, modifiers) {
                    continue;
                }
                let in_chord = cx.chord.is_some();
                match keys::lookup(modifiers, key, cx.chord) {
                    Lookup::Command(cmd) => {
                        suppress_text = in_chord || modifiers.alt;
                        run(doc, cx, out, cmd);
                    }
                    Lookup::ChordStarted(k) => {
                        out.message = Some(format!(
                            "(Ctrl+{}) was pressed. Waiting for second key of chord...",
                            k.name()
                        ));
                    }
                    Lookup::NotACommand(msg) => {
                        suppress_text = true;
                        out.message = Some(msg);
                    }
                    Lookup::None => caret_key(doc, cx, out, key, modifiers),
                }
            }
            Event::Text(t) => {
                if std::mem::take(&mut suppress_text) || t.chars().all(char::is_control) {
                    continue;
                }
                type_text(doc, cx, &t);
            }
            // Ctrl+K, Ctrl+C arrives as a Copy event; inside a chord it's the second key.
            Event::Copy | Event::Cut | Event::Paste(_) if cx.chord.is_some() => {
                let key = match ev {
                    Event::Copy => Key::C,
                    Event::Cut => Key::X,
                    _ => Key::V,
                };
                match keys::lookup(Modifiers::COMMAND, key, cx.chord) {
                    Lookup::Command(cmd) => run(doc, cx, out, cmd),
                    Lookup::NotACommand(msg) => out.message = Some(msg),
                    _ => {}
                }
            }
            Event::Copy => copy(doc, cx, out, false),
            Event::Cut => copy(doc, cx, out, true),
            Event::Paste(text) => {
                let line_mode = cx.line_clipboard.as_deref() == Some(text.as_str());
                doc.editor.paste(&text, line_mode);
                *cx.completion = None;
            }
            _ => {}
        }
    }
}

fn ours(e: &Event) -> bool {
    matches!(
        e,
        Event::Key { .. } | Event::Text(_) | Event::Copy | Event::Cut | Event::Paste(_)
    )
}

fn copy(doc: &mut Document, cx: &mut ViewCtx<'_>, out: &mut ViewOutput, cut: bool) {
    let (text, lines) = if cut {
        doc.editor.cut()
    } else {
        doc.editor.copy_text()
    };
    *cx.line_clipboard = lines.then(|| text.clone());
    out.copy = Some(text);
}

/// Keys of the open autocomplete popup; true when used.
fn completion_key(doc: &mut Document, cx: &mut ViewCtx<'_>, key: Key, mods: Modifiers) -> bool {
    let Some(c) = cx.completion.as_mut() else {
        return false;
    };
    if !mods.is_none() {
        return false;
    }
    let n = c.items.len();
    match key {
        Key::ArrowDown => c.selected = (c.selected + 1) % n,
        Key::ArrowUp => c.selected = (c.selected + n - 1) % n,
        Key::PageDown => c.selected = (c.selected + 8).min(n - 1),
        Key::PageUp => c.selected = c.selected.saturating_sub(8),
        Key::Escape => *cx.completion = None,
        Key::Enter | Key::Tab => {
            let c = cx.completion.take().expect("open");
            let item = &c.items[c.selected];
            // Enter on a fully typed name keeps its normal meaning (new line).
            if key == Key::Enter && item.label == c.ctx.prefix {
                return false;
            }
            accept(doc, &c, &item.label);
        }
        _ => return false,
    }
    true
}

/// Replace the typed prefix at every caret with `label`.
fn accept(doc: &mut Document, c: &Completion, label: &str) {
    let n = c.ctx.prefix.chars().count();
    let ed = &mut doc.editor;
    let edits = ed
        .sels
        .iter()
        .map(|s| {
            let h = s.head;
            (
                Pos::new(h.line, h.col.saturating_sub(n)),
                h,
                label.to_owned(),
            )
        })
        .collect();
    let ranges = ed.apply(EditKind::Other, edits);
    let sels = ranges
        .into_iter()
        .map(|(_, e)| Selection::caret(e))
        .collect();
    ed.set_selections(sels);
}

/// Open or update the popup for the text before the caret (`force`: Ctrl+Space).
pub fn refresh_completion(doc: &mut Document, completion: &mut Option<Completion>, force: bool) {
    let head = doc.editor.caret();
    let before: String = doc
        .editor
        .buf
        .line(head.line)
        .chars()
        .take(head.col)
        .collect();
    let Some(ctx) = completion::context(&before) else {
        *completion = None;
        return;
    };
    let member = !matches!(ctx.target, completion::Target::Name);
    if ctx.prefix.is_empty() && !member && !force {
        *completion = None;
        return;
    }
    let items = completion::complete(&ctx, doc.editor.buf.lines(), head.line);
    if items.is_empty() {
        *completion = None;
        return;
    }
    let keep = completion
        .as_ref()
        .and_then(|c| c.items.get(c.selected))
        .and_then(|old| items.iter().position(|i| i.label == old.label))
        .filter(|_| !ctx.prefix.is_empty());
    *completion = Some(Completion {
        ctx,
        items,
        selected: keep.unwrap_or(0),
    });
}

fn type_text(doc: &mut Document, cx: &mut ViewCtx<'_>, t: &str) {
    if t.chars().count() == 1 {
        let c = t.chars().next().unwrap_or(' ');
        doc.editor.type_char(c, cx.settings.auto_close);
        if super::text::is_word(c) || matches!(c, '.' | ':' | '"' | '\'') {
            refresh_completion(doc, cx.completion, false);
        } else {
            *cx.completion = None;
        }
    } else {
        doc.editor.type_text(t);
        *cx.completion = None;
    }
}

/// Map a line and a row step to the visible (unfolded) line that many rows away.
fn visible_step(visible: &[usize], line: usize, delta: isize) -> usize {
    let i = match visible.binary_search(&line) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    };
    let j = (i as isize + delta).clamp(0, visible.len() as isize - 1) as usize;
    visible[j]
}

/// Caret movement and editing keys that aren't table shortcuts.
fn caret_key(
    doc: &mut Document,
    cx: &mut ViewCtx<'_>,
    out: &mut ViewOutput,
    key: Key,
    m: Modifiers,
) {
    let (ctrl, shift, alt) = (m.command || m.ctrl, m.shift, m.alt);
    let visible = doc.visible_lines();
    let step = |line: usize, delta: isize| visible_step(&visible, line, delta);
    let page = doc.view.page_rows as isize;
    let ed = &mut doc.editor;
    let mut keep_popup = false;
    match key {
        Key::ArrowLeft | Key::ArrowRight if alt && shift => {
            ed.box_extend(0, if key == Key::ArrowLeft { -1 } else { 1 })
        }
        Key::ArrowUp | Key::ArrowDown if alt && shift => {
            ed.box_extend(if key == Key::ArrowUp { -1 } else { 1 }, 0)
        }
        Key::ArrowLeft => ed.move_horizontal(false, ctrl, shift),
        Key::ArrowRight => ed.move_horizontal(true, ctrl, shift),
        Key::ArrowUp => ed.move_vertical(-1, shift, &step),
        Key::ArrowDown => ed.move_vertical(1, shift, &step),
        Key::PageUp => ed.move_vertical(-page, shift, &step),
        Key::PageDown => ed.move_vertical(page, shift, &step),
        Key::Home if ctrl => ed.doc_start(shift),
        Key::End if ctrl => ed.doc_end(shift),
        Key::Home => ed.home(shift),
        Key::End => ed.end(shift),
        Key::Backspace => {
            ed.backspace(ctrl);
            keep_popup = cx.completion.is_some();
        }
        Key::Delete if shift => copy(doc, cx, out, true),
        Key::Delete => ed.delete(ctrl),
        Key::Enter if !ctrl => ed.newline(),
        Key::Tab if !ctrl => {
            if shift {
                ed.indent_lines(true);
            } else {
                ed.tab();
            }
        }
        Key::Escape => {
            if ed.sels.len() > 1 {
                ed.single_caret();
            } else if !ed.primary().is_empty() {
                let h = ed.caret();
                ed.set_caret(h);
            }
        }
        // Ctrl+C / Ctrl+X / Ctrl+Insert when the platform sends keys instead of clipboard events.
        Key::C | Key::Insert if ctrl => copy(doc, cx, out, false),
        Key::X if ctrl => copy(doc, cx, out, true),
        _ => keep_popup = cx.completion.is_some() && !is_motion(key),
    }
    if keep_popup {
        refresh_completion(doc, cx.completion, false);
    } else {
        *cx.completion = None;
    }
}

fn is_motion(key: Key) -> bool {
    matches!(
        key,
        Key::ArrowLeft
            | Key::ArrowRight
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::Home
            | Key::End
            | Key::PageUp
            | Key::PageDown
            | Key::Escape
            | Key::Enter
    )
}

/// Run a command that only needs this document; window commands go to `out.cmds`.
pub fn run(doc: &mut Document, cx: &mut ViewCtx<'_>, out: &mut ViewOutput, cmd: Cmd) {
    *cx.completion = None;
    let ed = &mut doc.editor;
    match cmd {
        Cmd::Undo => {
            ed.undo();
        }
        Cmd::Redo => {
            ed.redo();
        }
        Cmd::SelectAll => ed.select_all(),
        Cmd::Duplicate => ed.duplicate(),
        Cmd::CutLine => {
            let text = ed.lines_text();
            ed.delete_lines();
            *cx.line_clipboard = Some(text.clone());
            out.copy = Some(text);
        }
        Cmd::DeleteLine => ed.delete_lines(),
        Cmd::MoveLinesUp => {
            ed.move_lines(true);
        }
        Cmd::MoveLinesDown => {
            ed.move_lines(false);
        }
        Cmd::InsertLineBelow => ed.insert_line(false),
        Cmd::InsertLineAbove => ed.insert_line(true),
        Cmd::Comment => ed.comment(true),
        Cmd::Uncomment => ed.comment(false),
        Cmd::ToggleComment => ed.toggle_comment(),
        Cmd::FormatDocument => ed.format_document(),
        Cmd::Complete => refresh_completion(doc, cx.completion, true),
        Cmd::NextOccurrence => ed.select_next_occurrence(),
        Cmd::AllOccurrences => ed.select_all_occurrences(),
        Cmd::AddCaretAbove | Cmd::AddCaretBelow => {
            let p = ed.primary();
            let goal = p.goal.unwrap_or(p.head.col);
            let line = if cmd == Cmd::AddCaretAbove {
                p.head.line.checked_sub(1)
            } else {
                (p.head.line + 1 < ed.buf.line_count()).then_some(p.head.line + 1)
            };
            if let Some(l) = line {
                let mut s = Selection::caret(ed.buf.clamp(Pos::new(l, goal)));
                s.goal = Some(goal);
                ed.add_selection(s);
            }
        }
        Cmd::Lowercase => ed.replace_selections(EditKind::Other, str::to_lowercase),
        Cmd::Uppercase => ed.replace_selections(EditKind::Other, str::to_uppercase),
        Cmd::GoToBrace => {
            let tokens = doc.analysis().tokens.clone();
            if let Some((_, partner)) = doc.editor.matching_bracket(&tokens) {
                doc.editor.set_caret(partner);
            }
        }
        Cmd::GoToDefinition => {
            let head = ed.caret();
            let found = ed
                .buf
                .word_at(head)
                .map(|(a, b)| ed.buf.slice(a, b))
                .and_then(|w| ed.definition_of(&w));
            match found {
                Some(p) => {
                    ed.set_caret(p);
                    doc.view.center = true;
                }
                None => out.message = Some("Cannot navigate to the symbol under the caret.".into()),
            }
        }
        Cmd::ToggleFold => {
            let line = ed.caret().line;
            let regions = doc.analysis().structure.folds.clone();
            let inner = regions
                .iter()
                .filter(|r| r.0 <= line && line <= r.1)
                .max_by_key(|r| r.0);
            if let Some(&(s, _)) = inner {
                doc.toggle_fold(s);
                move_out_of_folds(doc);
            }
        }
        Cmd::CollapseAll => {
            doc.collapse_all();
            move_out_of_folds(doc);
        }
        Cmd::ExpandAll => doc.editor.markers.clear(),
        Cmd::ToggleAllFolds => {
            if doc.editor.markers.is_empty() {
                let regions = doc.analysis().structure.folds.clone();
                doc.editor.markers = regions.iter().map(|r| Pos::new(r.0, 0)).collect();
                move_out_of_folds(doc);
            } else {
                doc.editor.markers.clear();
            }
        }
        Cmd::ScrollLineUp => doc.view.scroll_delta -= doc.view.row_h,
        Cmd::ScrollLineDown => doc.view.scroll_delta += doc.view.row_h,
        other => out.cmds.push(other),
    }
}

/// Carets inside a collapsed block move to its first line (otherwise it would reopen).
fn move_out_of_folds(doc: &mut Document) {
    let folded = doc.folded_ranges();
    let ed = &mut doc.editor;
    let mut moved = false;
    for s in &mut ed.sels {
        if let Some(&(start, _)) = folded
            .iter()
            .filter(|(a, b)| s.head.line > *a && s.head.line <= *b)
            .min_by_key(|r| r.0)
        {
            let p = Pos::new(start, ed.buf.line_len(start));
            *s = Selection::caret(p);
            moved = true;
        }
    }
    if moved {
        ed.normalize();
    }
}
