use crate::typing::{
    InlineStyle, TypingRun, boundary_edit, input_edit, remove_selected_style, style_at,
};
use crate::{autoformat, code_block, lists};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    text: String,
    anchor: usize,
    cursor: usize,
    pending_style: Option<InlineStyle>,
    typing_run: Option<TypingRun>,
}

/// Markdown source text, selection, and edit history.
#[derive(Clone, Debug)]
pub struct Document {
    state: Snapshot,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

impl Document {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            state: Snapshot {
                text: text.into(),
                anchor: 0,
                cursor: 0,
                pending_style: None,
                typing_run: None,
            },
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
    pub fn text(&self) -> &str {
        &self.state.text
    }
    pub fn cursor(&self) -> usize {
        self.state.cursor
    }
    pub fn anchor(&self) -> usize {
        self.state.anchor
    }
    pub fn selection(&self) -> Range<usize> {
        self.state.anchor.min(self.state.cursor)..self.state.anchor.max(self.state.cursor)
    }
    pub fn select(&mut self, anchor: usize, cursor: usize) {
        self.state.pending_style = None;
        self.state.typing_run = None;
        self.state.anchor = floor_boundary(self.text(), anchor);
        self.state.cursor = floor_boundary(self.text(), cursor);
    }
    pub fn move_to(&mut self, offset: usize, extend: bool) {
        self.select(if extend { self.anchor() } else { offset }, offset);
    }
    fn checkpoint(&mut self) {
        self.push_undo(self.state.clone());
        self.redo.clear();
    }
    fn push_undo(&mut self, state: Snapshot) {
        if self.undo.len() >= 200 {
            self.undo.remove(0);
        }
        self.undo.push(state);
    }
    pub fn replace(&mut self, range: Range<usize>, text: &str) {
        self.replace_with_history(range, text, true);
    }
    pub(crate) fn replace_with_history(
        &mut self,
        range: Range<usize>,
        text: &str,
        checkpoint: bool,
    ) {
        let start = floor_boundary(self.text(), range.start);
        let end = floor_boundary(self.text(), range.end.max(start));
        if self.text()[start..end] == *text {
            self.move_to(start + text.len(), false);
            return;
        }
        if checkpoint {
            self.checkpoint();
        }
        self.state.text.replace_range(start..end, text);
        self.move_to(start + text.len(), false);
    }
    pub fn insert(&mut self, text: &str) {
        self.input(self.selection(), text, true);
    }
    pub(crate) fn typing_style(&self) -> InlineStyle {
        self.state
            .pending_style
            .unwrap_or_else(|| style_at(self.text(), self.cursor()))
    }
    pub(crate) fn input(&mut self, range: Range<usize>, text: &str, checkpoint: bool) {
        if self.code_input(range.clone(), text, checkpoint) {
            return;
        }
        let pending = self.state.pending_style;
        let before = (pending.is_none_or(|style| style == InlineStyle::default())
            && autoformat::is_closing_input(text)
            && (range.is_empty() || !checkpoint))
            .then(|| {
                if checkpoint {
                    self.text().to_string()
                } else {
                    self.undo
                        .last()
                        .map_or_else(|| self.text().to_string(), |state| state.text.clone())
                }
            });
        if let Some(style) = pending.filter(|_| !text.is_empty()) {
            let mut edit = input_edit(
                self.text(),
                range,
                text,
                style,
                self.state.typing_run.as_ref(),
            );
            let shift = self.replace_input(edit.range, &edit.replacement, checkpoint);
            edit.run.source.start += shift;
            edit.run.source.end += shift;
            edit.run.cursor += shift;
            self.move_to(edit.run.cursor, false);
            self.state.typing_run = Some(edit.run);
        } else {
            self.replace_input(range, text, checkpoint);
        }
        self.state.pending_style = pending;
        if !text.is_empty() && !text.contains('\n') {
            self.expand_fence();
        }
        if let Some(before) = before
            && let Some((text, cursor)) = autoformat::rejected(&before, self.text(), self.cursor())
        {
            let mut literal = self.state.clone();
            literal.text = text;
            literal.anchor = cursor;
            literal.cursor = cursor;
            literal.pending_style = None;
            literal.typing_run = None;
            self.push_undo(literal);
        }
    }
    pub(crate) fn compose(&mut self, range: Range<usize>, text: &str, checkpoint: bool) {
        if self.code_input(range.clone(), text, checkpoint) {
            return;
        }
        let pending = self.state.pending_style;
        self.replace_input(range, text, checkpoint);
        self.state.pending_style = pending;
    }
    fn replace_input(&mut self, range: Range<usize>, text: &str, checkpoint: bool) -> usize {
        if let Some((replacement_range, replacement, cursor)) =
            boundary_edit(self.text(), &range, text)
        {
            let shift = cursor - (range.start + text.len());
            self.replace_with_history(replacement_range, &replacement, checkpoint);
            self.move_to(cursor, false);
            shift
        } else {
            self.replace_with_history(range, text, checkpoint);
            0
        }
    }
    fn code_input(&mut self, mut range: Range<usize>, text: &str, checkpoint: bool) -> bool {
        let blocks = code_block::blocks(self.text());
        let block = blocks.iter().find(|block| {
            (block.contains_cursor(range.start) && block.contains_cursor(range.end))
                || (range.is_empty()
                    && block
                        .opening
                        .as_ref()
                        .is_some_and(|r| r.contains(&range.start)))
        });
        let eol = if self.text().contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        if let Some(block) = block {
            if range.is_empty() && range.start < block.content.start {
                range = block.content.start..block.content.start;
            }
            // Empty fences have a virtual input row until the first edit.
            let prefix = if block
                .opening
                .as_ref()
                .is_some_and(|r| r.end == self.text().len())
            {
                eol
            } else {
                ""
            };
            let suffix = if block.content.is_empty() && block.closing.is_some() {
                eol
            } else {
                ""
            };
            if range.is_empty() && text.is_empty() {
                return true;
            }
            let cursor = range.start + prefix.len() + text.len();
            self.replace_with_history(range, &format!("{prefix}{text}{suffix}"), checkpoint);
            self.move_to(cursor, false);
            return true;
        }
        if range.is_empty()
            && !text.is_empty()
            && range.start == self.text().len()
            && blocks
                .last()
                .is_some_and(|block| block.closing.as_ref().is_some_and(|r| r.end == range.start))
        {
            self.replace_with_history(range, &format!("{eol}{text}"), checkpoint);
            return true;
        }
        false
    }
    fn expand_fence(&mut self) {
        let line = self.line_range(self.cursor());
        let marker = self.text()[line.clone()].trim_end_matches('\r');
        let end = line.start + marker.len();
        if (marker != "```" && marker != "~~~") || self.cursor() != end {
            return;
        }
        let eol = if self.text().contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let ending = if end == self.text().len() { eol } else { "" };
        let addition = format!("{eol}{eol}{marker}{ending}");
        self.replace_with_history(end..end, &addition, false);
        self.move_to(end + eol.len(), false);
    }
    pub(crate) fn select_composition(&mut self, anchor: usize, cursor: usize) {
        let pending = self.state.pending_style;
        self.select(anchor, cursor);
        self.state.pending_style = pending;
    }
    pub fn undo(&mut self) -> bool {
        if let Some(state) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.state, state));
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self) -> bool {
        if let Some(state) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.state, state));
            true
        } else {
            false
        }
    }
    /// Toggle selected text, or store a style for future input at an empty selection.
    pub fn toggle_inline(&mut self, marker: &str) {
        let range = self.selection();
        if range.is_empty() {
            let mut style = self.typing_style();
            style.toggle(marker);
            self.state.pending_style = Some(style);
            return;
        }
        if let Some((range, replacement, selected)) =
            remove_selected_style(self.text(), &range, marker)
        {
            self.replace(range, &replacement);
            self.select(selected.start, selected.end);
            return;
        }
        let n = marker.len();
        if range.start >= n
            && self.text()[..range.start].ends_with(marker)
            && self.text()[range.end..].starts_with(marker)
        {
            let text = self.text()[range.clone()].to_string();
            self.replace(range.start - n..range.end + n, &text);
            self.select(range.start - n, range.end - n);
        } else {
            let selected = &self.text()[range.clone()];
            let body = selected.trim();
            if body.is_empty() {
                return;
            }
            let leading = selected.len() - selected.trim_start().len();
            let trailing = selected.len() - selected.trim_end().len();
            let len = body.len();
            let text = format!(
                "{}{marker}{body}{marker}{}",
                &selected[..leading],
                &selected[selected.len() - trailing..]
            );
            self.replace(range.clone(), &text);
            self.select(range.start + leading + n, range.start + leading + n + len);
        }
    }
    pub fn set_prefix(&mut self, prefix: &str) {
        let start = self.line_range(self.cursor()).start;
        let line = &self.text()[self.line_range(self.cursor())];
        let old = block_prefix(line).len();
        let list = lists::prefix(line);
        let indentation = list.as_ref().map_or("", |list| &line[..list.indent]);
        let target = lists::prefix(prefix);
        let same = match (&list, &target) {
            (Some(current), Some(target)) => {
                current.checkbox.is_some() == target.checkbox.is_some()
                    && (target.checkbox.is_some()
                        || current.marker.as_bytes()[0].is_ascii_digit()
                            == target.marker.as_bytes()[0].is_ascii_digit())
            }
            _ => {
                line[indentation.len()..].starts_with(prefix)
                    && old == indentation.len() + prefix.len()
            }
        };
        let replacement = if same {
            String::new()
        } else if lists::prefix(prefix).is_some() {
            format!("{indentation}{prefix}")
        } else {
            prefix.to_string()
        };
        let cursor = self.cursor();
        let next = start + cursor.saturating_sub(start + old) + replacement.len();
        let new_column = lists::prefix(&replacement).map_or(0, |prefix| prefix.content_column);
        let mut edits = vec![(start..start + old, replacement)];
        if let Some(current) = list {
            let delta = new_column as isize - current.content_column as isize;
            if delta != 0
                && let Some(item) = lists::items(self.text())
                    .into_iter()
                    .find(|item| item.head.start == start)
            {
                let mut line_start = (item.head.end + 1).min(item.range.end);
                for line in self.text()[line_start..item.range.end].split_inclusive('\n') {
                    if !line.trim().is_empty() {
                        let (bytes, columns) = lists::whitespace(line, usize::MAX);
                        edits.push((
                            line_start..line_start + bytes,
                            " ".repeat(columns.saturating_add_signed(delta)),
                        ));
                    }
                    line_start += line.len();
                }
            }
        }
        self.structural_edit(edits);
        self.move_to(next, false);
    }
    /// Apply structural changes as a single undo step while retaining the selected text.
    pub(crate) fn structural_edit(&mut self, mut edits: Vec<lists::Edit>) -> bool {
        edits.retain(|(range, value)| self.text()[range.clone()] != *value);
        if edits.is_empty() {
            return false;
        }
        edits.sort_by_key(|(range, _)| (range.start, range.end));
        let remap = |offset: usize| {
            let mut delta = 0isize;
            for (range, replacement) in &edits {
                if offset < range.start {
                    break;
                }
                if offset < range.end {
                    return (range.start as isize + delta) as usize
                        + (offset - range.start).min(replacement.len());
                }
                delta += replacement.len() as isize - range.len() as isize;
            }
            offset.saturating_add_signed(delta)
        };
        let (anchor, cursor) = (remap(self.anchor()), remap(self.cursor()));
        self.checkpoint();
        for (range, value) in edits.into_iter().rev() {
            self.state.text.replace_range(range, &value);
        }
        self.state.anchor = anchor;
        self.state.cursor = cursor;
        self.state.typing_run = None;
        true
    }
    pub(crate) fn indent_list(&mut self, outdent: bool) -> Option<bool> {
        let edits = lists::indent(self.text(), self.selection(), outdent)?;
        Some(self.structural_edit(edits))
    }
    pub(crate) fn toggle_checkbox(&mut self, offset: usize) -> bool {
        let Some(checkbox) = lists::items(self.text()).iter().find_map(|item| {
            item.prefix
                .checkbox
                .map(|mut checkbox| {
                    checkbox.offset += item.head.start;
                    checkbox
                })
                .filter(|checkbox| checkbox.offset == offset)
        }) else {
            return false;
        };
        self.structural_edit(vec![(
            offset..offset + 1,
            if checkbox.checked { " " } else { "x" }.into(),
        )])
    }
    pub(crate) fn soft_newline(&mut self) {
        let line = self.line_range(self.cursor());
        let items = lists::items(self.text());
        let Some(item) = lists::at_line(&items, line.start).map(|i| &items[i]) else {
            self.newline();
            return;
        };
        let eol = if self.text().contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        self.structural_newline(&format!("{eol}{}", " ".repeat(item.prefix.content_column)));
    }
    fn structural_newline(&mut self, text: &str) {
        let pending = self.state.pending_style;
        if self.selection().is_empty()
            && let Some((range, replacement, cursor)) =
                crate::typing::split_inline_line(self.text(), self.cursor(), text)
        {
            let style = self.typing_style();
            self.replace(range, &replacement);
            self.move_to(cursor, false);
            self.state.pending_style = Some(style);
            return;
        }
        // The line break and list prefix share one undo step.
        let split = text.find('\n').unwrap() + 1;
        self.insert(&text[..split]);
        let cursor = self.cursor();
        self.replace_with_history(cursor..cursor, &text[split..], false);
        self.state.pending_style = pending;
    }
    pub fn line_range(&self, offset: usize) -> Range<usize> {
        let offset = floor_boundary(self.text(), offset);
        let start = self.text()[..offset].rfind('\n').map_or(0, |i| i + 1);
        let end = self.text()[offset..]
            .find('\n')
            .map_or(self.text().len(), |i| offset + i);
        start..end
    }
    pub fn newline(&mut self) {
        let range = self.line_range(self.cursor());
        let line = &self.text()[range.clone()];
        let prefix = block_prefix(line);
        let eol = if self.text().contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        if code_block::blocks(self.text())
            .iter()
            .any(|block| block.contains_cursor(self.cursor()))
        {
            let indentation = line
                .chars()
                .take_while(|ch| matches!(ch, ' ' | '\t'))
                .collect::<String>();
            self.insert(&format!("{eol}{indentation}"));
            return;
        }
        let items = lists::items(self.text());
        if let Some(item) = lists::at_line(&items, range.start).map(|i| &items[i]) {
            if range.start == item.head.start && line[item.prefix.content..].trim().is_empty() {
                if item.parent.is_some() {
                    self.indent_list(true);
                } else {
                    self.replace(range.start..range.start + item.prefix.content, "");
                }
            } else {
                let continuation = item.continuation(self.text());
                self.structural_newline(&format!("{eol}{continuation}"));
            }
            return;
        }
        if !prefix.is_empty() && line[prefix.len()..].trim().is_empty() {
            self.replace(range.start..self.cursor(), "");
            return;
        }
        let continuation = if prefix.starts_with('#') {
            String::new()
        } else if prefix.starts_with("- [") || prefix.starts_with("* [") {
            format!("{}[ ] ", &prefix[..2])
        } else if let Some((number, _)) = prefix.split_once(". ") {
            number
                .parse::<usize>()
                .map(|n| format!("{}. ", n + 1))
                .unwrap_or_default()
        } else {
            prefix.to_string()
        };
        self.insert(&format!("{eol}{continuation}"));
    }
    pub fn previous_grapheme(&self, offset: usize) -> usize {
        self.text()
            .grapheme_indices(true)
            .rev()
            .find(|(i, _)| *i < offset)
            .map_or(0, |(i, _)| i)
    }
    pub fn next_grapheme(&self, offset: usize) -> usize {
        self.text()
            .grapheme_indices(true)
            .find(|(i, _)| *i > offset)
            .map_or(self.text().len(), |(i, _)| i)
    }
}

pub(crate) fn floor_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}
pub(crate) fn from_utf16(text: &str, offset: usize) -> usize {
    let mut units = 0;
    for (i, ch) in text.char_indices() {
        if units + ch.len_utf16() > offset {
            return i;
        }
        units += ch.len_utf16();
    }
    text.len()
}
pub(crate) fn to_utf16(text: &str, offset: usize) -> usize {
    text[..floor_boundary(text, offset)].encode_utf16().count()
}

pub(crate) fn block_prefix(line: &str) -> &str {
    if let Some(prefix) = lists::prefix(line) {
        return &line[..prefix.content];
    }
    for prefix in [
        "- [ ] ", "- [x] ", "- [X] ", "* [ ] ", "* [x] ", "- ", "* ", "+ ", "> ", "###### ",
        "##### ", "#### ", "### ", "## ", "# ",
    ] {
        if line.starts_with(prefix) {
            return &line[..prefix.len()];
        }
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && line[digits..].starts_with(". ") {
        &line[..digits + 2]
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn undo_restores_unicode_and_selection() {
        let mut d = Document::new("a👩🏽‍💻é");
        d.select(1, d.next_grapheme(1));
        let selection = d.selection();
        d.insert("hello");
        assert_eq!(d.text(), "ahelloé");
        assert!(d.undo());
        assert_eq!(d.selection(), selection);
        assert_eq!(d.text(), "a👩🏽‍💻é");
        assert!(d.redo());
        assert_eq!(d.text(), "ahelloé");
    }
    #[test]
    fn formatting_round_trips() {
        let mut d = Document::new("hello world");
        d.select(6, 11);
        d.toggle_inline("**");
        assert_eq!(d.text(), "hello **world**");
        d.toggle_inline("**");
        assert_eq!(d.text(), "hello world");
    }
    #[test]
    fn list_continuation_and_exit() {
        let mut d = Document::new("- [x] done");
        d.move_to(d.text().len(), false);
        d.newline();
        assert_eq!(d.text(), "- [x] done\n- [ ] ");
        d.newline();
        assert_eq!(d.text(), "- [x] done\n");
        let mut d = Document::new("9. item");
        d.move_to(7, false);
        d.newline();
        assert_eq!(d.text(), "9. item\n10. ");
    }
    #[test]
    fn utf16_never_splits_surrogates() {
        let s = "a🦀é";
        assert_eq!(from_utf16(s, 2), 1);
        assert_eq!(from_utf16(s, 3), 5);
        assert_eq!(to_utf16(s, 5), 3);
        assert_eq!(from_utf16(s, 100), s.len());
    }
    #[test]
    fn preserves_crlf_and_unknown_markdown() {
        let mut d = Document::new("| a | b |\r\n|---|---|\r\n");
        let original = d.text().to_string();
        d.move_to(d.text().len(), false);
        d.newline();
        assert!(d.text().ends_with("\r\n\r\n"));
        d.undo();
        assert_eq!(d.text(), original);
    }
    #[test]
    fn imported_empty_fences_accept_immediate_input_without_rewriting_on_load() {
        for (source, cursor, expected) in [
            ("```", 3, "```\ncode"),
            ("```rust", 7, "```rust\ncode"),
            ("```\n```", 4, "```\ncode\n```"),
            ("```\r\n```", 5, "```\r\ncode\r\n```"),
        ] {
            let mut doc = Document::new(source);
            doc.move_to(cursor, false);
            assert_eq!(doc.text(), source);
            doc.insert("code");
            assert_eq!(doc.text(), expected);
            doc.undo();
            assert_eq!(doc.text(), source);
        }
    }
}
