use crate::extensions::{self, Cell, Extensions, Link, Picture, Table};
use crate::{
    code_block::{self, CodeBlock},
    document::block_prefix,
    lists::{self, Checkbox},
};
use pulldown_cmark::{Event, Tag};
use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Face {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    pub link: bool,
    pub highlight: bool,
    pub keyboard: bool,
    pub script: i8,
}
#[derive(Clone, Debug)]
pub(crate) struct Span {
    pub range: Range<usize>,
    pub face: Face,
}
#[derive(Clone, Debug)]
pub(crate) struct Line {
    pub source: Range<usize>,
    pub text: String,
    pub map: Vec<usize>,
    pub spans: Vec<Span>,
    pub heading: usize,
    pub quote: bool,
    pub quote_depth: usize,
    pub cell: Option<Cell>,
    pub picture: Option<usize>,
    pub math: bool,
    pub virtual_after: bool,
    pub code: bool,
    pub marker: String,
    pub list_depth: Option<usize>,
    pub checkbox: Option<Checkbox>,
    pub rule: bool,
    pub code_block: Option<usize>,
}
impl Line {
    pub fn is_plain(&self) -> bool {
        self.cell.is_none()
            && self.picture.is_none()
            && !self.math
            && !self.code
            && !self.quote
            && self.heading == 0
            && self.list_depth.is_none()
            && self.marker.is_empty()
            && !self.rule
    }
    fn empty(offset: usize, code_block: Option<usize>) -> Self {
        Self {
            source: offset..offset,
            text: String::new(),
            map: vec![offset],
            spans: Vec::new(),
            heading: 0,
            quote: false,
            quote_depth: 0,
            cell: None,
            picture: None,
            math: false,
            virtual_after: false,
            code: code_block.is_some(),
            marker: String::new(),
            list_depth: None,
            checkbox: None,
            rule: false,
            code_block,
        }
    }
    pub fn source_index(&self, display: usize) -> usize {
        self.map[display.min(self.map.len() - 1)]
    }
    pub fn display_index(&self, source: usize) -> usize {
        let mut i = self
            .map
            .partition_point(|offset| *offset < source)
            .min(self.text.len());
        while !self.text.is_char_boundary(i) {
            i -= 1;
        }
        i
    }
}

/// Rendered text and styles mapped to the original Markdown source.
#[derive(Clone, Debug)]
pub(crate) struct Projection {
    pub lines: Vec<Line>,
    pub editing_metadata: bool,
    pub blocks: Vec<CodeBlock>,
    pub tables: Vec<Table>,
    pub math_blocks: Vec<Range<usize>>,
    pub pictures: Vec<Picture>,
    pub links: Vec<Link>,
    inline_markers: Vec<Range<usize>>,
    escapes: Vec<Range<usize>>,
    atomic: Vec<Range<usize>>,
    protected: Vec<Range<usize>>,
}
impl Projection {
    pub fn new(source: &str, raw: bool) -> Self {
        Self::with_metadata_edit(source, raw, None)
    }
    pub fn with_metadata_edit(source: &str, raw: bool, editing: Option<usize>) -> Self {
        let mut hidden = vec![false; source.len()];
        let mut faces = vec![Face::default(); source.len()];
        let code_blocks = code_block::blocks(source);
        let list_items = if raw {
            Vec::new()
        } else {
            lists::items(source)
        };
        let mut rules = Vec::new();
        let mut inline_markers = Vec::new();
        let mut escapes = Vec::new();
        let mut extensions = if raw {
            Extensions::default()
        } else {
            Extensions::new(source, &mut hidden, &mut faces, &mut inline_markers)
        };
        let mut editing_metadata = false;
        if let Some(cursor) = editing {
            for range in &extensions.metadata {
                if range.start <= cursor && cursor <= range.end {
                    hidden[range.clone()].fill(false);
                    extensions.skipped.retain(|r| r != range);
                    editing_metadata = true;
                }
            }
        }
        if !raw {
            for (event, range) in crate::inline::events(source, extensions::options()) {
                let slice = &source[range.clone()];
                match event {
                    Event::Start(Tag::Strong) => {
                        for face in &mut faces[range.clone()] {
                            face.bold = true;
                        }
                        hide_pair(&mut hidden, &mut inline_markers, range, 2);
                    }
                    Event::Start(Tag::Emphasis) => {
                        for face in &mut faces[range.clone()] {
                            face.italic = true;
                        }
                        hide_pair(&mut hidden, &mut inline_markers, range, 1);
                    }
                    Event::Start(Tag::Strikethrough) => {
                        for face in &mut faces[range.clone()] {
                            face.strike = face.script != -1;
                        }
                        let width = if slice.starts_with("~~") { 2 } else { 1 };
                        hide_pair(&mut hidden, &mut inline_markers, range, width);
                    }
                    Event::Code(_) => {
                        if extensions
                            .tables
                            .iter()
                            .any(|table| table.source.contains(&range.start))
                        {
                            for (i, _) in slice.match_indices("\\|") {
                                hidden[range.start + i] = true;
                                escapes.push(range.start + i..range.start + i + 2);
                            }
                        }
                        let ticks = slice.bytes().take_while(|b| *b == b'`').count();
                        for face in &mut faces[range.clone()] {
                            face.code = true;
                        }
                        hide_pair(&mut hidden, &mut inline_markers, range, ticks);
                    }
                    Event::Start(Tag::CodeBlock(_)) => {
                        for face in &mut faces[range] {
                            face.code = true;
                        }
                    }
                    Event::Text(_)
                        if range.start > 0
                            && range.start < source.len()
                            && !faces[range.start].code
                            && source.as_bytes()[range.start].is_ascii_punctuation()
                            && source[..range.start]
                                .bytes()
                                .rev()
                                .take_while(|b| *b == b'\\')
                                .count()
                                % 2
                                == 1 =>
                    {
                        hidden[range.start - 1] = true;
                        escapes.push(range.start - 1..range.start + 1);
                    }
                    Event::Rule => rules.push(range),
                    _ => {}
                }
            }
        }
        escapes.sort_by_key(|range| range.end);
        let mut start = 0;
        let mut lines = Vec::new();
        for raw_line in source.split('\n') {
            let end = start + raw_line.len();
            let content = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            let source_range = start..end;
            if extensions.skip_line(&(start..start + content.len())) {
                start = end + 1;
                continue;
            }
            if let Some((index, block)) = code_blocks
                .iter()
                .enumerate()
                .find(|(_, block)| block.is_fence_line(start))
            {
                if block.opening.as_ref().is_some_and(|r| r.start == start)
                    && block.needs_empty_row(source)
                {
                    lines.push(Line::empty(block.content.start, Some(index)));
                }
                start = end + 1;
                continue;
            }
            let block_index = code_blocks
                .iter()
                .position(|block| block.contains_line(start, source.len()));
            let code = block_index.is_some();
            let item = lists::at_line(&list_items, start).map(|i| &list_items[i]);
            let list_depth = item.map(|item| item.depth);
            let first = item.filter(|item| item.head.start == start && !code);
            let checkbox = first
                .and_then(|item| item.prefix.checkbox)
                .map(|mut checkbox| {
                    checkbox.offset += start;
                    checkbox
                });
            let prefix = if let Some(item) = first {
                &content[..item.prefix.content.min(content.len())]
            } else if raw || code || lists::prefix(content).is_some() {
                ""
            } else {
                block_prefix(content)
            };
            let heading = extensions
                .headings
                .iter()
                .find(|(r, _)| r.start <= end && start < r.end)
                .map_or(0, |(_, level)| *level);
            let quote_depth = extensions
                .quotes
                .iter()
                .filter(|r| r.start <= end && start < r.end)
                .count();
            let quote = quote_depth > 0;
            let math = extensions
                .math
                .iter()
                .any(|r| r.start <= end && start < r.end);
            let rule = rules.iter().any(|r| r.start == start);
            let marker = if let Some((_, number, _)) = extensions
                .footnotes
                .iter()
                .find(|(r, _, _)| start <= r.start && r.start <= end)
            {
                format!("{number}.")
            } else if let Some(checkbox) = checkbox {
                if checkbox.checked { "☑" } else { "☐" }.to_string()
            } else if let Some(item) = first {
                item.number.map_or_else(
                    || ["•", "◦", "▪"][item.depth % 3].to_string(),
                    |number| format!("{number}{}", item.prefix.marker.chars().last().unwrap()),
                )
            } else {
                String::new()
            };
            if !prefix.is_empty() {
                hidden[start..start + prefix.len()].fill(true);
            }
            if rule {
                hidden[start..end].fill(true);
            }
            let mut partial_tab = None;
            let mut structural_end = start + prefix.len();
            if first.is_none()
                && !code
                && let Some(item) = item
            {
                let (indent, column) = lists::whitespace(content, item.prefix.content_column);
                hidden[start..start + indent].fill(true);
                structural_end = start + indent;
                if content.as_bytes().get(indent) == Some(&b'\t') {
                    let remaining = column + 4 - column % 4 - item.prefix.content_column;
                    partial_tab = Some((start + indent, remaining));
                }
            }
            if block_index.is_some_and(|i| code_blocks[i].opening.is_none())
                && content.starts_with("    ")
            {
                hidden[start..start + 4].fill(true);
            }
            for part in extensions.parts(source_range.clone()) {
                let mut text = String::new();
                let mut map = Vec::new();
                let mut spans: Vec<Span> = Vec::new();
                let mut last_end = structural_end.clamp(part.source.start, part.source.end);
                let part_content =
                    &source[part.source.start..part.source.end.min(start + content.len())];
                for (i, ch) in part_content.char_indices() {
                    let source_index = part.source.start + i;
                    let replacement = extensions
                        .replacements
                        .binary_search_by_key(&source_index, |r| r.source.start)
                        .ok()
                        .map(|index| &extensions.replacements[index]);
                    if hidden[source_index] && replacement.is_none() {
                        continue;
                    }
                    let display = text.len();
                    if let Some(replacement) = replacement {
                        text.push_str(&replacement.text);
                        map.extend(std::iter::repeat_n(source_index, replacement.text.len()));
                    } else if (code && ch == '\t')
                        || partial_tab.is_some_and(|(offset, _)| offset == source_index)
                    {
                        let spaces = partial_tab
                            .filter(|(offset, _)| *offset == source_index)
                            .map_or_else(|| 4 - text.chars().count() % 4, |(_, spaces)| spaces);
                        text.extend(std::iter::repeat_n(' ', spaces));
                        map.extend(std::iter::repeat_n(source_index, spaces));
                    } else {
                        text.push(ch);
                        let mapped = escapes
                            .binary_search_by_key(&(source_index + 1), |range| range.end)
                            .map_or(source_index, |index| escapes[index].start);
                        for byte in 0..ch.len_utf8() {
                            map.push(mapped + byte);
                        }
                    }
                    last_end = replacement.map_or(source_index + ch.len_utf8(), |r| r.source.end);
                    let mut face = if code {
                        Face {
                            code: true,
                            ..Default::default()
                        }
                    } else {
                        faces[source_index]
                    };
                    face.strike |= checkbox.is_some_and(|checkbox| checkbox.checked);
                    if let Some(span) = spans.last_mut().filter(|span| span.face == face) {
                        span.range.end = text.len();
                    } else {
                        spans.push(Span {
                            range: display..text.len(),
                            face,
                        });
                    }
                }
                map.push(last_end);
                lines.push(Line {
                    source: part.source,
                    text,
                    map,
                    spans,
                    heading,
                    quote,
                    quote_depth,
                    cell: part.cell,
                    picture: part.picture,
                    math,
                    virtual_after: false,
                    code,
                    marker: marker.clone(),
                    list_depth,
                    checkbox,
                    rule,
                    code_block: block_index,
                });
            }
            start = end + 1;
        }
        if code_blocks.last().is_some_and(|block| {
            block
                .closing
                .as_ref()
                .is_some_and(|r| r.end == source.len())
        }) {
            lines.push(Line::empty(source.len(), None));
        }
        let last_start = source.rfind('\n').map_or(0, |i| i + 1);
        if lines.is_empty()
            || (!source.ends_with('\n')
                && extensions.skip_line(&(last_start..source.len()))
                && !extensions
                    .tables
                    .iter()
                    .any(|table| table.source.end == source.len())
                && !extensions
                    .headings
                    .iter()
                    .any(|(range, _)| range.end == source.len()))
        {
            let mut line = Line::empty(source.len(), None);
            line.virtual_after = !source.is_empty();
            lines.push(line);
        }
        Self {
            lines,
            editing_metadata,
            blocks: code_blocks,
            tables: extensions.tables,
            math_blocks: extensions.math,
            pictures: extensions.pictures,
            links: extensions.links,
            inline_markers,
            escapes,
            atomic: extensions
                .replacements
                .iter()
                .map(|r| r.source.clone())
                .collect(),
            protected: extensions
                .skipped
                .iter()
                .map(|r| {
                    let start = if r.start > 0 && source.as_bytes()[r.start - 1] == b'\n' {
                        r.start - 1
                    } else {
                        r.start
                    };
                    start..r.end
                })
                .collect(),
        }
    }
    /// A visible escaped character and its backslash are one editing unit.
    pub fn expand_escapes(&self, mut range: Range<usize>) -> Range<usize> {
        for escape in self.escapes.iter().chain(&self.atomic) {
            if escape.start < range.start && range.start < escape.end {
                range.start = escape.start;
            }
            if escape.start < range.end && range.end < escape.end {
                range.end = escape.end;
            }
        }
        range
    }
    /// A collapsed-caret delete must not consume invisible definitions or block delimiters.
    pub fn protect_delete(&self, mut range: Range<usize>, forward: bool) -> Range<usize> {
        let mut blocks = self
            .protected
            .iter()
            .filter(|r| r.start < range.end && range.start < r.end)
            .collect::<Vec<_>>();
        blocks.sort_by_key(|r| r.start);
        if !forward {
            blocks.reverse();
        }
        for block in blocks {
            if forward {
                if range.start < block.start {
                    range.end = range.end.min(block.start);
                    break;
                }
                range.start = range.start.max(block.end).min(range.end);
            } else {
                if block.end < range.end {
                    range.start = range.start.max(block.end);
                    break;
                }
                range.end = range.end.min(block.start).max(range.start);
            }
        }
        range
    }
    pub fn block_start(&self, index: usize) -> usize {
        let line = &self.lines[index];
        if let Some(block) = line.code_block {
            self.blocks[block].source.start
        } else if let Some(cell) = &line.cell {
            self.tables[cell.table].source.start
        } else if let Some(picture) = line.picture {
            self.pictures[picture].source.start
        } else if let Some(math) = self
            .math_blocks
            .iter()
            .find(|range| range.start <= line.source.start && line.source.start < range.end)
        {
            math.start
        } else {
            line.source.start
        }
    }
    pub fn line_for_source(&self, source: usize) -> usize {
        let index = self
            .lines
            .partition_point(|line| line.source.start <= source)
            .saturating_sub(1);
        if let Some(cell) = self.lines[index].cell.as_ref().filter(|cell| cell.missing) {
            return self.lines[..=index]
                .iter()
                .rposition(|line| {
                    line.cell
                        .as_ref()
                        .is_some_and(|c| c.table == cell.table && c.row == cell.row && !c.missing)
                })
                .unwrap_or(index.saturating_sub(cell.column));
        }
        index
    }
    pub fn is_inline_marker(&self, range: Range<usize>) -> bool {
        self.inline_markers.contains(&range)
    }
    /// Cross a hidden marker before advancing to the next visible grapheme.
    pub fn arrow_stop(&self, cursor: usize, next: usize, right: bool) -> usize {
        let next = if right {
            next.max(cursor)
        } else {
            next.min(cursor)
        };
        let crossings = self.inline_markers.iter().filter_map(|r| {
            if right {
                (r.start == cursor).then_some(r.end)
            } else {
                (r.end == cursor).then_some(r.start)
            }
        });
        if let Some(target) = if right {
            crossings.min()
        } else {
            crossings.max()
        } {
            return target;
        }
        let stops = self.inline_markers.iter().flat_map(|r| [r.start, r.end]);
        if right {
            stops
                .filter(|n| cursor < *n && *n <= next)
                .min()
                .unwrap_or(next)
        } else {
            stops
                .filter(|n| next <= *n && *n < cursor)
                .max()
                .unwrap_or(next)
        }
    }
}
fn hide_pair(hidden: &mut [bool], markers: &mut Vec<Range<usize>>, range: Range<usize>, n: usize) {
    if range.len() >= n * 2 {
        hidden[range.start..range.start + n].fill(true);
        hidden[range.end - n..range.end].fill(true);
        markers.push(range.start..range.start + n);
        markers.push(range.end - n..range.end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders_blocks_and_nested_inline_without_changing_source() {
        let source = "# Title\n\nA **bold _word_** and `code`.\n- [x] done\n> quote";
        let p = Projection::new(source, false);
        assert_eq!(p.lines[0].text, "Title");
        assert_eq!(p.lines[0].heading, 1);
        assert_eq!(p.lines[2].text, "A bold word and code.");
        assert!(
            p.lines[2]
                .spans
                .iter()
                .any(|s| s.face.bold && s.face.italic)
        );
        assert_eq!(p.lines[3].text, "done");
        assert_eq!(p.lines[3].marker, "☑");
        assert!(p.lines[4].quote);
        for line in &p.lines {
            for (i, _) in line.text.char_indices() {
                assert!(source.is_char_boundary(line.source_index(i)));
            }
        }
    }
    #[test]
    fn unicode_mapping_and_links() {
        let p = Projection::new("**é🦀** [link](https://example.com)", false);
        let l = &p.lines[0];
        assert_eq!(l.text, "é🦀 link");
        assert_eq!(l.source_index(2), 4);
        assert_eq!(l.display_index(4), 2);
    }
    #[test]
    fn code_does_not_parse_markdown() {
        let p = Projection::new("```rust\n# literal **text**\n```\n", false);
        assert_eq!(p.lines[0].text, "# literal **text**");
        assert!(p.lines[0].code);
        assert_eq!(p.lines[0].code_block, Some(0));
        assert_eq!(p.lines.len(), 2);
    }
    #[test]
    fn composition_projection_preserves_source() {
        let source = "# hi\n**bold** ![alt](image.png)\n";
        let p = Projection::new(source, true);
        assert_eq!(
            p.lines
                .iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            source
        );
    }
    #[test]
    fn strikethrough_hides_only_the_actual_delimiters_and_maps_every_character() {
        for marker in ["~", "~~"] {
            for body in ["a", "ab", "word", "é🦀", "日本語", "👩🏽‍💻"] {
                let source = format!("before {marker}{body}{marker} after");
                let p = Projection::new(&source, false);
                let line = &p.lines[0];
                assert_eq!(line.text, format!("before {body} after"), "{source}");
                let start = 7 + marker.len();
                let end = start + body.len();
                for (offset, _) in body.char_indices() {
                    assert_eq!(line.source_index(7 + offset), start + offset, "{source}");
                    assert!(
                        line.spans
                            .iter()
                            .any(|span| span.range.contains(&(7 + offset)) && span.face.strike)
                    );
                }
                assert_eq!(
                    p.arrow_stop(7, start + body.chars().next().unwrap().len_utf8(), true),
                    start
                );
                assert_eq!(p.arrow_stop(end + marker.len(), end, false), end);
                assert_eq!(
                    p.arrow_stop(end, end + marker.len() + 1, true),
                    end + marker.len()
                );
            }
        }
    }

    #[test]
    fn single_and_double_tildes_work_with_nested_styles_and_stay_literal_in_code() {
        let p = Projection::new("~**first** and *last*~ + ~~double~~ + `~literal~`", false);
        let line = &p.lines[0];
        assert_eq!(line.text, "first and last + double + ~literal~");
        assert!(
            line.spans
                .iter()
                .any(|span| span.face.bold && span.face.strike)
        );
        assert!(
            line.spans
                .iter()
                .any(|span| span.face.italic && span.face.strike)
        );
        assert!(
            line.spans
                .iter()
                .filter(|span| span.face.code)
                .all(|span| !span.face.strike)
        );
        let p = Projection::new("~~~markdown\n~literal~ ~~literal~~\n~~~", false);
        assert_eq!(p.lines[0].text, "~literal~ ~~literal~~");
        assert!(p.lines[0].spans.iter().all(|span| !span.face.strike));
    }
    #[test]
    fn incomplete_bold_does_not_consume_an_asterisk_as_italics() {
        for source in ["**asd*", "before **é🦀* after", "**first\nlast*"] {
            let p = Projection::new(source, false);
            assert_eq!(
                p.lines
                    .iter()
                    .map(|line| line.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                source
            );
            for line in &p.lines {
                assert!(
                    line.spans
                        .iter()
                        .all(|span| !span.face.italic && !span.face.bold)
                );
                for (offset, _) in line.text.char_indices() {
                    assert_eq!(line.source_index(offset), line.source.start + offset);
                }
            }
        }
        let p = Projection::new("- **asd*", false);
        assert_eq!(p.lines[0].text, "**asd*");
        assert!(p.lines[0].spans.iter().all(|span| !span.face.italic));
    }

    #[test]
    fn complete_nested_and_adjacent_emphasis_still_renders() {
        for (source, expected) in [
            ("**asd**", "asd"),
            ("*asd*", "asd"),
            ("***asd***", "asd"),
            ("**bold *italic* end**", "bold italic end"),
            ("*italic **bold** end*", "italic bold end"),
            ("**bold***italic*", "bolditalic"),
            ("*italic***bold**", "italicbold"),
            ("`**asd*`", "**asd*"),
        ] {
            let p = Projection::new(source, false);
            assert_eq!(p.lines[0].text, expected, "{source}");
            assert!(
                p.lines[0]
                    .spans
                    .iter()
                    .all(|span| span.face.bold || span.face.italic || span.face.code)
            );
        }
        let p = Projection::new(r"\**italic*", false);
        assert!(p.lines[0].spans.iter().any(|span| span.face.italic));
    }
    #[test]
    fn escaped_punctuation_has_literal_styles_and_atomic_source_ranges() {
        let source = r"\*\*é🦀\*\* \\ \_ \~ \`";
        let p = Projection::new(source, false);
        let line = &p.lines[0];
        assert_eq!(line.text, "**é🦀** \\ _ ~ `");
        assert!(line.spans.iter().all(|span| span.face == Face::default()));
        assert_eq!(line.source_index(0), 0);
        assert_eq!(line.source_index(1), 2);
        assert_eq!(p.expand_escapes(0..1), 0..2);
        assert_eq!(p.expand_escapes(2..3), 2..4);
        assert_eq!(p.expand_escapes(0..2), 0..2);
        assert_eq!(Projection::new(r"`\*`", false).lines[0].text, r"\*");
        assert_eq!(
            Projection::new("```\n\\*\\*\n```", false).lines[0].text,
            r"\*\*"
        );
    }
}
