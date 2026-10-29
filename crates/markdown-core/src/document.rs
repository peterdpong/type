use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    text: String,
    anchor: usize,
    cursor: usize,
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
        self.replace(self.selection(), text);
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
    pub fn line_range(&self, offset: usize) -> Range<usize> {
        let offset = floor_boundary(self.text(), offset);
        let start = self.text()[..offset].rfind('\n').map_or(0, |i| i + 1);
        let end = self.text()[offset..]
            .find('\n')
            .map_or(self.text().len(), |i| offset + i);
        start..end
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utf16_never_splits_surrogates() {
        let s = "a🦀é";
        assert_eq!(from_utf16(s, 2), 1);
        assert_eq!(from_utf16(s, 3), 5);
        assert_eq!(to_utf16(s, 5), 3);
        assert_eq!(from_utf16(s, 100), s.len());
    }
}
