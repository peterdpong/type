//! List structure and source-preserving edits shared by projection and input.
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Checkbox {
    /// The single ASCII character between the brackets.
    pub offset: usize,
    pub checked: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Prefix {
    pub indent: usize,
    pub columns: usize,
    pub content: usize,
    pub content_column: usize,
    pub marker: String,
    pub checkbox: Option<Checkbox>,
}

pub(crate) fn whitespace(text: &str, limit: usize) -> (usize, usize) {
    let mut columns = 0;
    let mut bytes = 0;
    for ch in text.bytes() {
        let next = match ch {
            b' ' => columns + 1,
            b'\t' => columns + 4 - columns % 4,
            _ => break,
        };
        if next > limit {
            break;
        }
        columns = next;
        bytes += 1;
    }
    (bytes, columns)
}

pub(crate) fn prefix(line: &str) -> Option<Prefix> {
    let (indent, columns) = whitespace(line, usize::MAX);
    let rest = &line[indent..];
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let marker_len = if matches!(rest.as_bytes().first(), Some(b'-' | b'*' | b'+')) {
        1
    } else if (1..=9).contains(&digits) && matches!(rest.as_bytes().get(digits), Some(b'.' | b')'))
    {
        digits + 1
    } else {
        return None;
    };
    if !matches!(rest.as_bytes().get(marker_len), Some(b' ' | b'\t')) {
        return None;
    }
    let marker = rest[..marker_len].to_string();
    let marker_end = indent + marker_len;
    let mut content = marker_end;
    let mut content_column = columns + marker_len;
    while let Some(ch @ (b' ' | b'\t')) = line.as_bytes().get(content) {
        content_column += if *ch == b'\t' {
            4 - content_column % 4
        } else {
            1
        };
        content += 1;
    }
    // Five or more spaces after a marker start indented code; only one is structural.
    if content_column - columns - marker_len > 4 {
        content = marker_end + 1;
        content_column = columns + marker_len + 1;
    }
    let mut checkbox = None;
    let tail = &line[content..];
    if tail.starts_with('[')
        && matches!(tail.as_bytes().get(1), Some(b' ' | b'x' | b'X'))
        && tail.as_bytes().get(2) == Some(&b']')
        && (tail.len() == 3 || matches!(tail.as_bytes().get(3), Some(b' ' | b'\t' | b'\r')))
    {
        checkbox = Some(Checkbox {
            offset: content + 1,
            checked: tail.as_bytes()[1] != b' ',
        });
        content += 3;
        content += line[content..]
            .bytes()
            .take_while(|b| matches!(b, b' ' | b'\t'))
            .count();
    }
    Some(Prefix {
        indent,
        columns,
        content,
        content_column,
        marker,
        checkbox,
    })
}

#[derive(Clone, Debug)]
pub(crate) struct Item {
    pub range: Range<usize>,
    pub head: Range<usize>,
    pub prefix: Prefix,
    pub depth: usize,
    pub parent: Option<usize>,
    pub number: Option<u64>,
}
impl Item {
    pub fn continuation(&self, source: &str) -> String {
        let marker = self.number.map_or_else(
            || self.prefix.marker.clone(),
            |number| {
                format!(
                    "{}{}",
                    number.saturating_add(1).min(999_999_999),
                    self.prefix.marker.chars().last().unwrap()
                )
            },
        );
        format!(
            "{}{marker} {}",
            &source[self.head.start..self.head.start + self.prefix.indent],
            if self.prefix.checkbox.is_some() {
                "[ ] "
            } else {
                ""
            }
        )
    }
}

pub(crate) fn items(source: &str) -> Vec<Item> {
    let mut lists = Vec::new();
    let mut parents = Vec::new();
    let mut items = Vec::new();
    for (event, range) in Parser::new_ext(source, crate::extensions::options()).into_offset_iter() {
        match event {
            Event::Start(Tag::List(number)) => lists.push(number),
            Event::End(TagEnd::List(_)) => {
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                // Tab-indented item ranges can start at the preceding newline.
                let marker = range.start
                    + source[range.clone()]
                        .bytes()
                        .take_while(u8::is_ascii_whitespace)
                        .count();
                let head = crate::code_block::line_range(source, marker);
                let number = lists.last_mut().and_then(|n: &mut Option<u64>| {
                    let current = *n;
                    *n = n.map(|n| n.saturating_add(1));
                    current
                });
                if let Some(prefix) = prefix(source[head.clone()].trim_end_matches('\r')) {
                    let index = items.len();
                    items.push(Item {
                        range: head.start..range.end,
                        head,
                        prefix,
                        depth: lists.len().saturating_sub(1),
                        parent: parents.last().copied().flatten(),
                        number,
                    });
                    parents.push(Some(index));
                } else {
                    parents.push(None);
                }
            }
            Event::End(TagEnd::Item) => {
                parents.pop();
            }
            _ => {}
        }
    }
    items
}

pub(crate) fn at_line(items: &[Item], line_start: usize) -> Option<usize> {
    items
        .iter()
        .rposition(|item| item.range.contains(&line_start))
}

pub(crate) type Edit = (Range<usize>, String);

/// Move selected items, including their children and continuation paragraphs, as a unit.
/// None means the selection is outside a list; an empty edit is a handled no-op.
pub(crate) fn indent(source: &str, selection: Range<usize>, outdent: bool) -> Option<Vec<Edit>> {
    let items = items(source);
    let first = crate::code_block::line_range(source, selection.start).start;
    let last = crate::code_block::line_range(
        source,
        if selection.is_empty() {
            selection.end
        } else {
            selection.end.saturating_sub(1)
        },
    )
    .start;
    let initial = at_line(&items, first)?;
    let mut roots: Vec<usize> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if (i == initial || (first <= item.head.start && item.head.start <= last))
            && !roots
                .iter()
                .any(|root| items[*root].range.contains(&item.head.start))
        {
            roots.push(i);
        }
    }
    let mut edits = Vec::new();
    let mut next_numbers = std::collections::HashMap::new();
    for index in &roots {
        let item = &items[*index];
        let (target, target_parent) = if outdent {
            let Some(parent) = item.parent else {
                continue;
            };
            (items[parent].prefix.columns, items[parent].parent)
        } else {
            let Some(previous) = (0..*index)
                .rev()
                .find(|i| items[*i].parent == item.parent && !roots.contains(i))
            else {
                return Some(Vec::new());
            };
            (items[previous].prefix.content_column, Some(previous))
        };
        let mut marker_delta = 0isize;
        if item.number.is_some() {
            let delimiter = item.prefix.marker.chars().last().unwrap();
            let next = next_numbers
                .entry((target_parent, delimiter))
                .or_insert_with(|| {
                    (0..*index)
                        .rev()
                        .find(|i| items[*i].parent == target_parent && !roots.contains(i))
                        .filter(|i| items[*i].prefix.marker.ends_with(delimiter))
                        .and_then(|i| items[i].number)
                        .map_or(1, |n| n.saturating_add(1).min(999_999_999))
                });
            let number = next.to_string();
            *next = next.saturating_add(1).min(999_999_999);
            let digits = item.prefix.marker.len() - 1;
            marker_delta = number.len() as isize - digits as isize;
            let start = item.head.start + item.prefix.indent;
            edits.push((start..start + digits, number));
        }
        let delta = target as isize - item.prefix.columns as isize;
        let mut start = item.range.start;
        for line in source[item.range.clone()].split_inclusive('\n') {
            // Renumbering can change the prefix width (e.g. 11. to 1.).
            let delta = delta
                + if start == item.head.start {
                    0
                } else {
                    marker_delta
                };
            if !line.trim().is_empty() && delta != 0 {
                if delta < 0 {
                    let (bytes, columns) = whitespace(line, usize::MAX);
                    edits.push((
                        start..start + bytes,
                        " ".repeat(columns.saturating_add_signed(delta)),
                    ));
                } else {
                    edits.push((start..start, " ".repeat(delta as usize)));
                }
            }
            start += line.len();
        }
    }
    Some(edits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, projection::Projection};

    #[test]
    fn mixed_list_tree_uses_markdown_structure_and_counts_repeated_numbers() {
        let source = "8. Parent\n   - Child\n     1) One\n     1) Two\n   + [X] Done\n9. Next\n\n```\n- [ ] Literal\n```\n\n    - also code";
        let p = Projection::new(source, false);
        let entries = items(source);
        assert_eq!(
            entries.iter().map(|item| item.depth).collect::<Vec<_>>(),
            [0, 1, 2, 2, 1, 0]
        );
        assert_eq!(
            entries.iter().map(|item| item.parent).collect::<Vec<_>>(),
            [None, Some(0), Some(1), Some(1), Some(0), None]
        );
        assert_eq!(p.lines[2].marker, "1)");
        assert_eq!(p.lines[3].marker, "2)");
        assert_eq!(p.lines[4].text, "Done");
        assert!(p.lines[4].checkbox.unwrap().checked);
        assert!(
            p.lines
                .iter()
                .filter(|line| line.code)
                .all(|line| line.checkbox.is_none() && line.marker.is_empty())
        );
        assert_eq!(p.lines[0].source_index(0), 3);
        assert_eq!(p.lines[4].source_index(0), source.find("Done").unwrap());
    }

    #[test]
    fn nested_subtree_edits_preserve_selection_crlf_and_undo() {
        let source = "10. First\r\n11. 🦀 parent\r\n    - child\r\n      continuation\r\n12. Last";
        let mut d = Document::new(source);
        let selected = source.find("🦀").unwrap()..source.find("parent").unwrap() + 6;
        d.select(selected.end, selected.start);
        assert_eq!(d.indent_list(false), Some(true));
        assert_eq!(
            d.text(),
            "10. First\r\n    1. 🦀 parent\r\n       - child\r\n         continuation\r\n12. Last"
        );
        assert_eq!(&d.text()[d.selection()], "🦀 parent");
        assert!(d.anchor() > d.cursor());
        assert_eq!(
            items(d.text())
                .iter()
                .map(|item| item.depth)
                .collect::<Vec<_>>(),
            [0, 1, 2, 0]
        );
        assert_eq!(d.indent_list(true), Some(true));
        assert_eq!(d.text(), source);
        assert!(d.undo());
        assert!(d.text().contains("       - child"));
        assert!(d.undo());
        assert_eq!(d.text(), source);
        assert_eq!(d.selection(), selected);
    }

    #[test]
    fn selection_moves_sibling_subtrees_once_and_first_item_is_a_noop() {
        let source = "- First\n- Parent\n  - Child\n- Sibling\n- Last";
        let mut d = Document::new(source);
        d.move_to(2, false);
        assert_eq!(d.indent_list(false), Some(false));
        assert!(!d.undo());
        d.select(
            source.find("Parent").unwrap(),
            source.find("- Last").unwrap(),
        );
        assert_eq!(d.indent_list(false), Some(true));
        assert_eq!(
            d.text(),
            "- First\n  - Parent\n    - Child\n  - Sibling\n- Last"
        );
        assert_eq!(d.indent_list(true), Some(true));
        assert_eq!(d.text(), source);
    }

    #[test]
    fn tab_indentation_can_be_outdented_without_changing_the_body() {
        let mut d = Document::new("- Parent\n\t- Child\n\t\tcontinuation");
        d.move_to(d.text().find("Child").unwrap(), false);
        assert_eq!(d.indent_list(true), Some(true));
        assert_eq!(d.text(), "- Parent\n- Child\n    continuation");
        assert!(d.undo());
        assert_eq!(d.text(), "- Parent\n\t- Child\n\t\tcontinuation");
    }

    #[test]
    fn enter_continues_numbering_nesting_and_unchecked_tasks() {
        for (source, ending) in [
            ("- Parent\n  1) One\n  1) Two", "\n  3) "),
            ("- Parent\n  + [X] 🦀 done", "\n  + [ ] "),
            ("1. [x] Done", "\n2. [ ] "),
            ("* Parent\r\n    * [ ] Todo", "\r\n    * [ ] "),
        ] {
            let mut d = Document::new(source);
            d.move_to(source.len(), false);
            d.newline();
            assert_eq!(d.text(), format!("{source}{ending}"));
            d.insert("Next");
            assert_eq!(
                Projection::new(d.text(), false).lines.last().unwrap().text,
                "Next"
            );
            d.undo();
            d.undo();
            assert_eq!(d.text(), source);
        }
    }

    #[test]
    fn empty_nested_items_step_out_then_exit_and_preserve_undo() {
        let mut d = Document::new("- Parent\n  - [ ] ");
        d.move_to(d.text().len(), false);
        d.newline();
        assert_eq!(d.text(), "- Parent\n- [ ] ");
        d.newline();
        assert_eq!(d.text(), "- Parent\n");
        d.undo();
        d.undo();
        assert_eq!(d.text(), "- Parent\n  - [ ] ");
    }

    #[test]
    fn list_syntax_stays_outside_pending_inline_style() {
        let mut d = Document::new("- ");
        d.move_to(2, false);
        d.toggle_inline("**");
        d.insert("First");
        d.newline();
        d.insert("Second");
        assert_eq!(d.text(), "- **First**\n- **Second**");
        assert_eq!(items(d.text()).len(), 2);
    }

    #[test]
    fn soft_breaks_continue_the_same_item_and_checkbox_toggles_preserve_selection() {
        let source = "- [X] **héllo**\r\n  continuation\r\n- [ ] second";
        let mut d = Document::new(source);
        d.select(
            source.find("héllo").unwrap(),
            source.find("second").unwrap(),
        );
        let selected = d.selection();
        assert!(d.toggle_checkbox(3));
        assert_eq!(d.text(), source.replacen("[X]", "[ ]", 1));
        assert_eq!(d.selection(), selected);
        d.undo();
        assert_eq!(d.text(), source);
        d.move_to(source.len(), false);
        d.soft_newline();
        d.insert("more");
        let p = Projection::new(d.text(), false);
        assert_eq!(items(d.text()).len(), 2);
        assert_eq!(p.lines.last().unwrap().text, "more");
        assert_eq!(p.lines.last().unwrap().list_depth, Some(0));
        assert!(p.lines.last().unwrap().checkbox.is_none());
        assert!(!Document::new("```\n- [ ] literal\n```").toggle_checkbox(7));
    }

    #[test]
    fn incomplete_checkbox_text_and_non_list_numbers_remain_visible() {
        let source = "Text\n2. ordinary text\n\n- [x]no space\n- [ ]";
        let p = Projection::new(source, false);
        assert_eq!(p.lines[1].text, "2. ordinary text");
        assert_eq!(p.lines[3].text, "[x]no space");
        assert!(p.lines[3].checkbox.is_none());
        assert_eq!(p.lines[4].text, "");
        assert!(p.lines[4].checkbox.is_some());
    }
    #[test]
    fn splitting_formatted_list_text_keeps_each_item_valid() {
        let mut d = Document::new("- **ab*cd*ef**");
        d.move_to(8, false);
        d.newline();
        let p = Projection::new(d.text(), false);
        assert_eq!(p.lines[0].text, "abc");
        assert_eq!(p.lines[1].text, "def");
        assert!(
            p.lines
                .iter()
                .all(|line| line.spans.iter().all(|span| span.face.bold))
        );
        d.insert("X");
        assert_eq!(Projection::new(d.text(), false).lines[1].text, "Xdef");
        d.undo();
        d.undo();
        assert_eq!(d.text(), "- **ab*cd*ef**");
    }
    #[test]
    fn changing_list_kind_keeps_children_attached_and_toggles_checked_lists_off() {
        let source = "- Parent\n  - Child\n    - Grandchild";
        let mut d = Document::new(source);
        d.move_to(4, false);
        d.set_prefix("1. ");
        assert_eq!(d.text(), "1. Parent\n   - Child\n     - Grandchild");
        assert_eq!(
            items(d.text())
                .iter()
                .map(|item| item.depth)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
        d.set_prefix("- [ ] ");
        assert_eq!(d.text(), "- [ ] Parent\n  - Child\n    - Grandchild");
        d.toggle_checkbox(3);
        d.set_prefix("- [ ] ");
        assert_eq!(d.text(), "Parent\n- Child\n  - Grandchild");
        for _ in 0..4 {
            assert!(d.undo());
        }
        assert_eq!(d.text(), source);
    }
}
