//! Undoable acceptance of inline syntax typed directly into the document.
use crate::inline;
use pulldown_cmark::{Event, Options, Tag};
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bold,
    Italic,
    Strike,
    Code,
    Link,
}

fn spans(source: &str) -> Vec<(Kind, Range<usize>)> {
    inline::events(source, Options::ENABLE_STRIKETHROUGH)
        .filter_map(|(event, range)| {
            let kind = match event {
                Event::Start(Tag::Strong) => Kind::Bold,
                Event::Start(Tag::Emphasis) => Kind::Italic,
                Event::Start(Tag::Strikethrough) => Kind::Strike,
                Event::Code(_) => Kind::Code,
                Event::Start(Tag::Link { .. })
                    if source[range.clone()].starts_with('[')
                        && source[range.clone()].contains("](") =>
                {
                    Kind::Link
                }
                _ => return None,
            };
            Some((kind, range))
        })
        .collect()
}

pub(crate) fn is_closing_input(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| matches!(b, b'*' | b'_' | b'~' | b'`' | b')'))
}

/// Escape the completed span as literal Markdown, with the caret after it.
pub(crate) fn rejected(before: &str, after: &str, cursor: usize) -> Option<(String, usize)> {
    let previous = spans(before);
    let completed = spans(after);
    let (kind, range) = completed
        .iter()
        .filter(|(kind, range)| {
            range.end == cursor
                && !previous
                    .iter()
                    .any(|(old_kind, old_range)| old_kind == kind && old_range.start == range.start)
        })
        .min_by_key(|(_, range)| range.start)?;
    let mut escaped = Vec::new();
    match kind {
        Kind::Bold | Kind::Italic | Kind::Strike => {
            let marker = after.as_bytes()[range.start];
            let width = after[range.clone()]
                .bytes()
                .take_while(|b| *b == marker)
                .count();
            let closing = after[range.clone()]
                .bytes()
                .rev()
                .take_while(|b| *b == marker)
                .count();
            escaped.extend(range.start..range.start + width);
            escaped.extend(range.end - closing..range.end);
        }
        Kind::Code => {
            escaped.extend(
                after[range.clone()]
                    .bytes()
                    .enumerate()
                    .filter(|(_, byte)| byte.is_ascii_punctuation())
                    .map(|(i, _)| range.start + i),
            );
        }
        Kind::Link => {
            let separator = range.start + after[range.clone()].rfind("](")?;
            escaped.push(range.start);
            escaped.push(separator);
            escaped.extend(
                after[separator + 1..range.end]
                    .bytes()
                    .enumerate()
                    .filter(|(_, byte)| byte.is_ascii_punctuation())
                    .map(|(i, _)| separator + 1 + i),
            );
        }
    }
    escaped.sort_unstable();
    escaped.dedup();
    let caret = cursor + escaped.iter().filter(|offset| **offset < cursor).count();
    let mut literal = after.to_string();
    for offset in escaped.into_iter().rev() {
        literal.insert(offset, '\\');
    }
    Some((literal, caret))
}

#[cfg(test)]
mod tests {
    use crate::{Document, projection::Projection};

    fn type_source(doc: &mut Document, text: &str) {
        for ch in text.chars() {
            doc.insert(&ch.to_string());
        }
    }

    #[test]
    fn undo_rejects_completed_inline_styles_without_losing_any_typed_characters() {
        for source in [
            "**asd**",
            "*é🦀*",
            "__bold__",
            "_italic_",
            "~strike~",
            "~~strike~~",
            "`code`",
            "``a`b``",
            "***both***",
            "[label](https://example.com)",
        ] {
            let mut doc = Document::new("");
            type_source(&mut doc, source);
            assert_ne!(Projection::new(doc.text(), false).lines[0].text, source);
            assert!(doc.undo(), "{source}");
            let p = Projection::new(doc.text(), false);
            assert_eq!(p.lines[0].text, source, "{}", doc.text());
            assert!(p.lines[0].spans.iter().all(|span| !span.face.bold
                && !span.face.italic
                && !span.face.strike
                && !span.face.code
                && !span.face.link));
            assert_eq!(doc.cursor(), doc.text().len());
            assert_eq!(doc.anchor(), doc.cursor());
            assert!(doc.redo());
            assert_eq!(doc.text(), source);
            assert_ne!(Projection::new(doc.text(), false).lines[0].text, source);
        }
    }

    #[test]
    fn undo_redo_walks_the_styling_and_input_as_separate_steps() {
        let mut doc = Document::new("**asd*");
        doc.move_to(doc.text().len(), false);
        doc.insert("*");
        doc.undo();
        assert_eq!(Projection::new(doc.text(), false).lines[0].text, "**asd**");
        doc.undo();
        assert_eq!(doc.text(), "**asd*");
        doc.redo();
        assert_eq!(Projection::new(doc.text(), false).lines[0].text, "**asd**");
        doc.redo();
        assert_eq!(doc.text(), "**asd**");
        assert_eq!(Projection::new(doc.text(), false).lines[0].text, "asd");
    }

    #[test]
    fn rejection_survives_more_input_and_reloading_and_leaves_other_styles_alone() {
        let prefix = "Already **bold**. ";
        let mut doc = Document::new(prefix);
        doc.move_to(prefix.len(), false);
        type_source(&mut doc, "*new*");
        doc.undo();
        doc.insert(" more");
        assert!(!doc.redo());
        let reloaded = Document::new(doc.text());
        let p = Projection::new(reloaded.text(), false);
        assert_eq!(p.lines[0].text, "Already bold. *new* more");
        assert!(p.lines[0].spans.iter().any(|span| span.face.bold));
        assert!(!p.lines[0].spans.iter().any(|span| span.face.italic));
    }

    #[test]
    fn rejecting_code_keeps_nested_markup_and_backslashes_literal() {
        let source = r"`**bold** \*literal\*`";
        let mut doc = Document::new("");
        type_source(&mut doc, source);
        doc.undo();
        let p = Projection::new(doc.text(), false);
        assert_eq!(p.lines[0].text, source);
        assert!(
            p.lines[0]
                .spans
                .iter()
                .all(|span| !span.face.bold && !span.face.italic && !span.face.code)
        );
    }

    #[test]
    fn imported_pasted_shortcut_and_existing_formatting_keep_normal_undo() {
        let mut doc = Document::new("**existing**");
        assert!(!doc.undo());
        doc.move_to(5, false);
        doc.insert("x");
        doc.undo();
        assert_eq!(doc.text(), "**existing**");
        let mut doc = Document::new("");
        doc.insert("**pasted**");
        doc.undo();
        assert_eq!(doc.text(), "");
        doc.toggle_inline("**");
        doc.insert("shortcut");
        doc.undo();
        assert_eq!(doc.text(), "");
        let mut doc = Document::new("");
        doc.toggle_inline("**");
        doc.toggle_inline("**");
        type_source(&mut doc, "**manual**");
        doc.undo();
        assert_eq!(
            Projection::new(doc.text(), false).lines[0].text,
            "**manual**"
        );
        let mut doc = Document::new("plain");
        doc.select(0, 5);
        doc.toggle_inline("**");
        doc.undo();
        assert_eq!(doc.text(), "plain");
    }

    #[test]
    fn literal_markdown_in_code_blocks_does_not_gain_a_formatting_undo_step() {
        let mut doc = Document::new("```\n**asd*\n```\n");
        doc.move_to(10, false);
        doc.insert("*");
        doc.undo();
        assert_eq!(doc.text(), "```\n**asd*\n```\n");
        assert!(!doc.undo());
    }
}
