use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag};
use std::ops::Range;

/// Source boundaries for one block. Fences are structure, never editable display rows.
#[derive(Clone, Debug)]
pub(crate) struct CodeBlock {
    pub source: Range<usize>,
    pub content: Range<usize>,
    pub opening: Option<Range<usize>>,
    pub closing: Option<Range<usize>>,
}
impl CodeBlock {
    pub fn contains_cursor(&self, offset: usize) -> bool {
        self.content.start <= offset && offset <= self.content.end
    }
    pub fn contains_line(&self, offset: usize, source_len: usize) -> bool {
        self.content.start <= offset
            && (offset < self.content.end
                || (offset == source_len
                    && self.content.end == source_len
                    && self.closing.is_none()))
    }
    pub fn is_fence_line(&self, offset: usize) -> bool {
        self.opening.as_ref().is_some_and(|r| r.start == offset)
            || self.closing.as_ref().is_some_and(|r| r.start == offset)
    }
    pub fn needs_empty_row(&self, source: &str) -> bool {
        self.content.is_empty() && (self.closing.is_some() || !source.ends_with('\n'))
    }
}
pub(crate) fn line_range(source: &str, offset: usize) -> Range<usize> {
    let start = source[..offset].rfind('\n').map_or(0, |i| i + 1);
    let end = source[offset..]
        .find('\n')
        .map_or(source.len(), |i| offset + i);
    start..end
}
pub(crate) fn blocks(source: &str) -> Vec<CodeBlock> {
    Parser::new(source)
        .into_offset_iter()
        .filter_map(|(event, range)| {
            let Event::Start(Tag::CodeBlock(kind)) = event else {
                return None;
            };
            let first = line_range(source, range.start);
            if !matches!(kind, CodeBlockKind::Fenced(_)) {
                return Some(CodeBlock {
                    source: first.start..range.end,
                    content: first.start..range.end,
                    opening: None,
                    closing: None,
                });
            }
            let opener = source[first.clone()].trim_start();
            let marker = opener.chars().next()?;
            if marker != '`' && marker != '~' {
                return None;
            }
            let count = opener.chars().take_while(|c| *c == marker).count();
            let body_start = (first.end + 1).min(source.len());
            let end = source[..range.end].trim_end_matches(['\n', '\r']).len();
            let last = line_range(source, end);
            let candidate = source[last.clone()].trim();
            let indent = source[last.clone()]
                .bytes()
                .take_while(|b| *b == b' ')
                .count();
            let closing = (last.start > first.start
                && indent <= 3
                && candidate.len() >= count
                && candidate.chars().all(|c| c == marker))
            .then_some(last);
            let body_end = closing.as_ref().map_or(range.end, |r| r.start);
            Some(CodeBlock {
                source: first.start..range.end,
                content: body_start..body_end,
                opening: Some(first),
                closing,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_fences_and_empty_blocks() {
        for source in [
            "```\n```",
            "```rust\n\n```",
            "~~~js\nx\n~~~\n",
            "````\n```literal\n````",
        ] {
            let blocks = blocks(source);
            assert_eq!(blocks.len(), 1, "{source}");
            assert!(blocks[0].closing.is_some(), "{source}");
            assert!(blocks[0].opening.is_some());
        }
        let b = blocks("```\n```").pop().unwrap();
        assert_eq!(b.content, 4..4);
        let b = blocks("````\n```literal\n````").pop().unwrap();
        assert_eq!(&"````\n```literal\n````"[b.content], "```literal\n");
    }
    #[test]
    fn unfinished_fences_have_a_body_at_eof() {
        for source in ["```", "```rust", "```\n", "```\nx\n"] {
            let b = blocks(source).pop().unwrap();
            assert!(b.closing.is_none());
            assert!(b.contains_cursor(source.len()));
        }
    }
    #[test]
    fn indented_fence_text_is_code_not_a_closer() {
        let source = "```\n    ```";
        let block = blocks(source).pop().unwrap();
        assert!(block.closing.is_none());
        assert_eq!(&source[block.content], "    ```");
    }
}
