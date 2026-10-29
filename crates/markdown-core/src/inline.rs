//! Inline parsing rules for an editor where delimiter pairs are still being typed.
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::ops::Range;

/// Treat unfinished ** and __ openers as literal text.
pub(crate) fn events(
    source: &str,
    options: Options,
) -> impl Iterator<Item = (Event<'_>, Range<usize>)> + '_ {
    let bytes = source.as_bytes();
    let mut delimiters = vec![false; source.len()];
    let mut emphasis = Vec::new();
    Parser::new_ext(source, options)
        .into_offset_iter()
        .filter_map(move |(event, range)| {
            match &event {
                Event::Start(Tag::Strong) => {
                    delimiters[range.start..range.start + 2].fill(true);
                    delimiters[range.end - 2..range.end].fill(true);
                }
                Event::Start(Tag::Emphasis) => {
                    let borrows_bold_opener = matches!(bytes[range.start], b'*' | b'_')
                        && range.start > 0
                        && bytes[range.start - 1] == bytes[range.start]
                        && !delimiters[range.start - 1]
                        && source[..range.start - 1]
                            .bytes()
                            .rev()
                            .take_while(|b| *b == b'\\')
                            .count()
                            % 2
                            == 0;
                    emphasis.push(!borrows_bold_opener);
                    if borrows_bold_opener {
                        return None;
                    }
                    delimiters[range.start] = true;
                    delimiters[range.end - 1] = true;
                }
                Event::End(TagEnd::Emphasis) if !emphasis.pop().unwrap_or(true) => return None,
                _ => {}
            }
            Some((event, range))
        })
}
