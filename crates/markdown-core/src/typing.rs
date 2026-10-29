use pulldown_cmark::{Event, Tag};
use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct InlineStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
}
impl InlineStyle {
    pub fn toggle(&mut self, marker: &str) {
        match marker {
            "**" | "__" => self.bold = !self.bold,
            "*" | "_" => self.italic = !self.italic,
            "`" => self.code = !self.code,
            _ => {}
        }
    }
    fn without(self, other: Self) -> Self {
        Self {
            bold: self.bold && !other.bold,
            italic: self.italic && !other.italic,
            code: self.code && !other.code,
        }
    }
    fn includes(self, other: Self) -> bool {
        (!other.bold || self.bold) && (!other.italic || self.italic) && (!other.code || self.code)
    }
}

#[derive(Clone, Debug)]
struct Wrapper {
    range: Range<usize>,
    content: Range<usize>,
    marker: String,
    style: InlineStyle,
}
fn all_wrappers(source: &str) -> Vec<Wrapper> {
    let mut result = Vec::new();
    for (event, range) in crate::inline::events(source, crate::extensions::options()) {
        let (count, style) = match event {
            Event::Start(Tag::Strong) => (
                2,
                InlineStyle {
                    bold: true,
                    ..Default::default()
                },
            ),
            Event::Start(Tag::Emphasis) => (
                1,
                InlineStyle {
                    italic: true,
                    ..Default::default()
                },
            ),
            Event::Code(_) => (
                source[range.clone()]
                    .bytes()
                    .take_while(|b| *b == b'`')
                    .count(),
                InlineStyle {
                    code: true,
                    ..Default::default()
                },
            ),
            _ => continue,
        };
        let content = range.start + count..range.end - count;
        result.push(Wrapper {
            marker: source[range.start..content.start].to_string(),
            range,
            content,
            style,
        });
    }
    result.sort_by_key(|w| (w.range.start, std::cmp::Reverse(w.range.end)));
    result
}
fn wrappers(source: &str, cursor: usize) -> Vec<Wrapper> {
    all_wrappers(source)
        .into_iter()
        .filter(|w| w.content.start <= cursor && cursor <= w.content.end)
        .collect()
}

/// Keep existing formatting when adjacent input makes a delimiter intraword:
/// underscores become stars, and single tildes become double tildes.
pub(crate) fn boundary_edit(
    source: &str,
    range: &Range<usize>,
    text: &str,
) -> Option<(Range<usize>, String, usize)> {
    let touches_left = source[..range.start].ends_with(['_', '~'])
        && text.chars().next().is_some_and(|c| !c.is_whitespace());
    let touches_right = source[range.end..].starts_with(['_', '~'])
        && text.chars().next_back().is_some_and(|c| !c.is_whitespace());
    if !touches_left && !touches_right {
        return None;
    }
    let adjacent = |span: &Range<usize>| {
        (touches_left && span.end == range.start) || (touches_right && span.start == range.end)
    };
    let mut edits = Vec::new();
    for span in all_wrappers(source)
        .into_iter()
        .filter(|span| span.marker.starts_with('_') && adjacent(&span.range))
    {
        let marker = "*".repeat(span.marker.len());
        edits.push((span.range.start..span.content.start, marker.clone()));
        edits.push((span.content.end..span.range.end, marker));
    }
    for (event, span) in crate::inline::events(source, crate::extensions::options()) {
        if matches!(event, Event::Start(Tag::Strikethrough))
            && adjacent(&span)
            && !source[span.clone()].starts_with("~~")
            && !crate::extensions::subscript_opener(source, span.start)
        {
            edits.push((span.start..span.start + 1, "~~".into()));
            edits.push((span.end - 1..span.end, "~~".into()));
        }
    }
    if edits.is_empty() {
        return None;
    }
    let cursor = range.start
        + text.len()
        + edits
            .iter()
            .filter(|(edit, _)| edit.end <= range.start)
            .map(|(edit, replacement)| replacement.len() - edit.len())
            .sum::<usize>();
    edits.push((range.clone(), text.to_string()));
    edits.sort_by_key(|(range, _)| (range.start, range.end));
    let start = edits.first().unwrap().0.start;
    let end = edits.iter().map(|(range, _)| range.end).max().unwrap();
    let mut replacement = source[start..end].to_string();
    for (edit, text) in edits.into_iter().rev() {
        replacement.replace_range(edit.start - start..edit.end - start, &text);
    }
    Some((start..end, replacement, cursor))
}

pub(crate) fn style_at(source: &str, cursor: usize) -> InlineStyle {
    wrappers(source, cursor)
        .iter()
        .fold(InlineStyle::default(), |mut style, w| {
            style.bold |= w.style.bold;
            style.italic |= w.style.italic;
            style.code |= w.style.code;
            style
        })
}

/// A continuous input run keeps closing delimiters after the caret, including when
/// the user types trailing whitespace. It is discarded on explicit navigation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TypingRun {
    pub source: Range<usize>,
    pub text: String,
    pub cursor: usize,
    pub style: InlineStyle,
    pub wrapping: InlineStyle,
}
pub(crate) struct InputEdit {
    pub range: Range<usize>,
    pub replacement: String,
    pub run: TypingRun,
}

// Emphasis delimiters exclude trailing whitespace and paragraph boundaries.
fn encode(text: &str, style: InlineStyle) -> (String, usize) {
    let mut result = String::new();
    let mut cursor = 0;
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            result.push('\n');
        }
        let leading = line.len() - line.trim_start().len();
        let body = line.trim();
        if body.is_empty() {
            result.push_str(line);
            cursor = result.len();
            continue;
        }
        let trailing = line.len() - line.trim_end().len();
        let marker = match (style.bold, style.italic) {
            (true, true) => "***",
            (true, false) => "**",
            (false, true) => "*",
            _ => "",
        };
        let ticks = if style.code {
            "`".repeat(body.split('`').map(str::len).count().max(1))
        } else {
            String::new()
        };
        result.push_str(&line[..leading]);
        result.push_str(marker);
        result.push_str(&ticks);
        result.push_str(body);
        cursor = result.len();
        result.push_str(&ticks);
        result.push_str(marker);
        if trailing > 0 {
            result.push_str(&line[line.len() - trailing..]);
            cursor = result.len();
        }
    }
    (result, cursor)
}
fn wrap_fragment(content: &str, marker: &str) -> String {
    let body = content.trim();
    if body.is_empty() {
        return content.to_string();
    }
    let leading = content.len() - content.trim_start().len();
    let trailing = content.len() - content.trim_end().len();
    // Underscores cannot delimit intraword emphasis.
    let marker = if marker.contains('_') {
        if marker.len() == 2 { "**" } else { "*" }
    } else {
        marker
    };
    format!(
        "{}{marker}{body}{marker}{}",
        &content[..leading],
        &content[content.len() - trailing..]
    )
}

pub(crate) fn remove_selected_style(
    source: &str,
    selection: &Range<usize>,
    marker: &str,
) -> Option<(Range<usize>, String, Range<usize>)> {
    let mut style = InlineStyle::default();
    style.toggle(marker);
    let wrappers = all_wrappers(source);
    let span = wrappers.iter().find(|span| {
        let mut boundary = span.range.clone();
        while let Some(outer) = wrappers.iter().find(|outer| outer.content == boundary) {
            boundary = outer.range.clone();
        }
        span.style == style
            && boundary.start <= selection.start
            && selection.end <= boundary.end
            && selection.start < span.content.end
            && selection.end > span.content.start
    })?;
    let start = selection.start.max(span.content.start);
    let end = selection.end.min(span.content.end);
    let left = wrap_fragment(&source[span.content.start..start], &span.marker);
    let right = wrap_fragment(&source[end..span.content.end], &span.marker);
    let selected = span.range.start + left.len()..span.range.start + left.len() + end - start;
    Some((
        span.range.clone(),
        format!("{left}{}{right}", &source[start..end]),
        selected,
    ))
}
fn split_wrappers(source: &str, spans: &[Wrapper], cursor: usize) -> (String, String) {
    let outer = &spans[0];
    let (left, right) = if let Some(inner) = spans.get(1) {
        let (left, right) = split_wrappers(source, &spans[1..], cursor);
        (
            format!("{}{left}", &source[outer.content.start..inner.range.start]),
            format!("{right}{}", &source[inner.range.end..outer.content.end]),
        )
    } else {
        (
            source[outer.content.start..cursor].to_string(),
            source[cursor..outer.content.end].to_string(),
        )
    };
    (
        wrap_fragment(&left, &outer.marker),
        wrap_fragment(&right, &outer.marker),
    )
}

/// Close active inline spans on each side of a structural list break.
pub(crate) fn split_inline_line(
    source: &str,
    cursor: usize,
    separator: &str,
) -> Option<(Range<usize>, String, usize)> {
    let spans = wrappers(source, cursor);
    let outer = spans.first()?;
    let (left, right) = split_wrappers(source, &spans, cursor);
    let right_start = crate::projection::Projection::new(&right, false).lines[0].source_index(0);
    let cursor = outer.range.start + left.len() + separator.len() + right_start;
    Some((
        outer.range.clone(),
        format!("{left}{separator}{right}"),
        cursor,
    ))
}

pub(crate) fn input_edit(
    source: &str,
    range: Range<usize>,
    text: &str,
    desired: InlineStyle,
    run: Option<&TypingRun>,
) -> InputEdit {
    if let Some(run) = run.filter(|run| {
        range.is_empty()
            && range.start == run.cursor
            && run.style == desired
            && (run.wrapping != InlineStyle::default() || run.style == InlineStyle::default())
    }) {
        let mut run = run.clone();
        run.text.push_str(text);
        let (replacement, cursor) = encode(&run.text, run.wrapping);
        let range = run.source.clone();
        run.source.end = run.source.start + replacement.len();
        run.cursor = run.source.start + cursor;
        return InputEdit {
            range,
            replacement,
            run,
        };
    }
    let spans = wrappers(source, range.start);
    let actual = style_at(source, range.start);
    let mut range = range;
    let mut left = String::new();
    let mut right = String::new();
    let mut wrapping = desired.without(actual);
    if range.is_empty() && !desired.includes(actual) && !spans.is_empty() {
        (left, right) = split_wrappers(source, &spans, range.start);
        range = spans[0].range.clone();
        wrapping = desired;
    } else if range.is_empty()
        && text.starts_with(char::is_whitespace)
        && spans.iter().all(|w| {
            source[range.start..w.content.end]
                .chars()
                .all(|c| matches!(c, '*' | '_' | '`'))
        })
        && !spans.is_empty()
    {
        range = spans[0].range.end..spans[0].range.end;
        wrapping = desired;
    }
    let (encoded, cursor) = encode(text, wrapping);
    let start = range.start + left.len();
    let run = TypingRun {
        source: start..start + encoded.len(),
        text: text.to_string(),
        cursor: start + cursor,
        style: desired,
        wrapping,
    };
    InputEdit {
        range,
        replacement: format!("{left}{encoded}{right}"),
        run,
    }
}

#[cfg(test)]
mod tests {
    use crate::{Document, projection::Projection};

    #[test]
    fn adjacent_typing_runs_keep_their_styles() {
        for first in 0..4 {
            for second in 0..4 {
                let mut doc = Document::new("");
                if first & 1 != 0 {
                    doc.toggle_inline("**");
                }
                if first & 2 != 0 {
                    doc.toggle_inline("_");
                }
                doc.insert("A");
                if (first ^ second) & 1 != 0 {
                    doc.toggle_inline("**");
                }
                if (first ^ second) & 2 != 0 {
                    doc.toggle_inline("_");
                }
                doc.insert("B");
                let projection = Projection::new(doc.text(), false);
                assert_eq!(
                    projection.lines[0].text,
                    "AB",
                    "{first} → {second}: {}",
                    doc.text()
                );
                for (index, style) in [first, second].into_iter().enumerate() {
                    let face = projection.lines[0]
                        .spans
                        .iter()
                        .find(|s| s.range.contains(&index))
                        .unwrap()
                        .face;
                    assert_eq!(
                        (face.bold, face.italic),
                        (style & 1 != 0, style & 2 != 0),
                        "{first} → {second}: {}",
                        doc.text()
                    );
                }
            }
        }
    }

    #[test]
    fn toggles_leave_source_and_history_untouched_and_navigation_resets_them() {
        let mut doc = Document::new("existing");
        doc.toggle_inline("**");
        doc.toggle_inline("_");
        assert_eq!(doc.text(), "existing");
        assert!(!doc.undo());
        doc.move_to(doc.text().len(), false);
        doc.insert(" plain");
        assert_eq!(doc.text(), "existing plain");
    }

    #[test]
    fn turning_off_formatting_inside_existing_text_splits_the_span() {
        let mut doc = Document::new("**abcd**");
        doc.move_to(4, false);
        doc.toggle_inline("**");
        assert_eq!(doc.text(), "**abcd**");
        doc.insert("X");
        assert_eq!(doc.text(), "**ab**X**cd**");
        assert_eq!(Projection::new(doc.text(), false).lines[0].text, "abXcd");
    }
    #[test]
    fn adjacent_input_preserves_single_tilde_styles_and_cursor_offsets() {
        for (source, cursor, input, expected, after) in [
            ("~é~", 4, "X", "~~é~~X", 7),
            ("~é~", 0, "X", "X~~é~~", 1),
            ("~a~_b_", 3, "é", "~~a~~é*b*", 7),
            ("~a~", 3, " ", "~a~ ", 4),
            ("~a", 2, "X", "~aX", 3),
        ] {
            let mut doc = Document::new(source);
            doc.move_to(cursor, false);
            doc.insert(input);
            assert_eq!(doc.text(), expected);
            assert_eq!(doc.cursor(), after);
            doc.undo();
            assert_eq!(doc.text(), source);
            assert_eq!(doc.cursor(), cursor);
        }
        let mut doc = Document::new("~a~");
        doc.move_to(3, false);
        doc.toggle_inline("**");
        doc.insert("é");
        doc.insert("Z");
        assert_eq!(doc.text(), "~~a~~**éZ**");
        assert_eq!(doc.cursor(), 10);
        doc.undo();
        doc.undo();
        assert_eq!(doc.text(), "~a~");
        assert_eq!(doc.cursor(), 3);
    }
}
