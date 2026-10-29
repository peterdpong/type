use super::{MarkdownEditor, appearance};
use gpui::{prelude::*, *};
use std::{
    ops::Range,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub(super) struct TableRow {
    pub bounds: Bounds<Pixels>,
    pub cells: Range<usize>,
    pub header: bool,
}
#[derive(Clone)]
pub(super) struct PicturePanel {
    pub bounds: Bounds<Pixels>,
    pub picture: usize,
}

pub(super) fn paint_tables(rows: &[TableRow], window: &mut Window) {
    for row in rows {
        window.paint_quad(
            fill(
                row.bounds,
                rgb(if row.header {
                    appearance::CHROME
                } else {
                    appearance::BACKGROUND
                }),
            )
            .border_widths(px(0.5))
            .border_color(rgb(appearance::BORDER)),
        );
        let width = row.bounds.size.width / row.cells.len() as f32;
        for column in 1..row.cells.len() {
            window.paint_quad(fill(
                Bounds::new(
                    row.bounds.origin + point(width * column, px(0.)),
                    size(px(1.), row.bounds.size.height),
                ),
                rgb(appearance::BORDER),
            ));
        }
    }
}

pub(super) fn show_pictures(
    editor: &MarkdownEditor,
    window: &mut Window,
    cx: &mut Context<MarkdownEditor>,
) {
    let viewport = editor.content_bounds();
    for panel in &editor.picture_panels {
        if panel.bounds.intersect(&viewport).is_empty() {
            continue;
        }
        let picture = &editor.projection.pictures[panel.picture];
        let destination = picture.destination.clone();
        let source = image_source(&destination, editor.document_path.as_deref());
        let title = if picture.title.is_empty() {
            destination.clone()
        } else {
            picture.title.clone()
        };
        let selected = editor.document.selection().start <= picture.source.start
            && editor.document.selection().end >= picture.source.end;
        let range = picture.source.clone();
        let mut element = div()
            .id(("markdown-picture", panel.picture))
            .w(panel.bounds.size.width)
            .h(panel.bounds.size.height)
            .rounded(px(6.))
            .overflow_hidden()
            .border_1()
            .border_color(rgb(if selected {
                0x7baddd
            } else {
                appearance::BORDER
            }))
            .cursor(CursorStyle::Arrow)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |editor, _, window, cx| {
                    window.focus(&editor.focus);
                    super::selection::stop(editor);
                    editor.document.select(range.start, range.end);
                    editor.toolbelt_dismissed = true;
                    cx.emit(super::EditorEvent::SelectionChanged);
                    cx.notify();
                    cx.stop_propagation();
                }),
            )
            .child(
                img(source)
                    .size_full()
                    .object_fit(ObjectFit::Contain)
                    .with_loading(|| placeholder("Loading image…", "").into_any_element())
                    .with_fallback(move || {
                        placeholder("Image unavailable", &title).into_any_element()
                    }),
            )
            .into_any_element();
        element.layout_as_root(
            size(
                AvailableSpace::Definite(panel.bounds.size.width),
                AvailableSpace::Definite(panel.bounds.size.height),
            ),
            window,
            cx,
        );
        window.with_content_mask(Some(ContentMask { bounds: viewport }), |window| {
            window.defer_draw(element, panel.bounds.origin, 0)
        });
    }
}
fn placeholder(label: &str, detail: &str) -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .justify_center()
        .items_center()
        .gap_2()
        .p_4()
        .bg(rgb(appearance::SUBTLE))
        .text_color(rgb(appearance::MUTED))
        .font_family(appearance::FONT_FAMILY)
        .text_size(px(appearance::UI_SIZE))
        .child(label.to_string())
        .child(
            div()
                .max_w_full()
                .truncate()
                .text_size(px(11.))
                .child(detail.to_string()),
        )
}
fn image_source(destination: &str, document: Option<&Path>) -> ImageSource {
    if destination.starts_with("https://") || destination.starts_with("http://") {
        return destination.to_string().into();
    }
    let destination = destination.strip_prefix("file://").unwrap_or(destination);
    let mut bytes = Vec::new();
    let mut i = 0;
    while i < destination.len() {
        if destination.as_bytes()[i] == b'%'
            && i + 2 < destination.len()
            && let (Some(a), Some(b)) = (
                (destination.as_bytes()[i + 1] as char).to_digit(16),
                (destination.as_bytes()[i + 2] as char).to_digit(16),
            )
        {
            bytes.push((a * 16 + b) as u8);
            i += 3;
        } else {
            bytes.push(destination.as_bytes()[i]);
            i += 1;
        }
    }
    let path = PathBuf::from(String::from_utf8(bytes).unwrap_or_else(|_| destination.to_string()));
    if path.is_absolute() {
        path.into()
    } else {
        document
            .and_then(Path::parent)
            .unwrap_or(Path::new("."))
            .join(path)
            .into()
    }
}

pub(super) fn tab(
    editor: &mut MarkdownEditor,
    backwards: bool,
    cx: &mut Context<MarkdownEditor>,
) -> bool {
    let i = editor.projection.line_for_source(editor.document.cursor());
    let Some(cell) = editor.projection.lines[i].cell.clone() else {
        return false;
    };
    let next = if backwards {
        i.checked_sub(1)
    } else {
        Some(i + 1)
    };
    if let Some(next) = next.filter(|next| {
        editor
            .projection
            .lines
            .get(*next)
            .is_some_and(|line| line.cell.as_ref().is_some_and(|c| c.table == cell.table))
    }) {
        let next = prepare_cell(editor, next, cx);
        let line = &editor.projection.lines[next];
        editor
            .document
            .select(line.source_index(0), line.source_index(line.text.len()));
        editor.reveal_cursor = true;
        editor.toolbelt_dismissed = true;
        cx.emit(super::EditorEvent::SelectionChanged);
        cx.notify();
    } else if !backwards {
        add_row(editor, cell.table, 0, cx);
    }
    true
}
fn add_row(
    editor: &mut MarkdownEditor,
    table: usize,
    column: usize,
    cx: &mut Context<MarkdownEditor>,
) {
    let table_index = table;
    let table = &editor.projection.tables[table];
    let row_index = table.rows.len();
    let at = table.source.end;
    let newline = if editor.markdown()[..at].contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let prefix = if editor.markdown()[..at].ends_with('\n') {
        ""
    } else {
        newline
    };
    let row = format!(
        "{prefix}|{}{newline}",
        "   |".repeat(table.alignments.len())
    );
    editor.document.replace_with_history(at..at, &row, true);
    editor
        .document
        .move_to(at + prefix.len() + 2 + column * 4, false);
    editor.changed(cx);
    if let Some(line) = editor.projection.lines.iter().find(|line| {
        line.cell.as_ref().is_some_and(|cell| {
            cell.table == table_index && cell.row == row_index && cell.column == column
        })
    }) {
        editor.document.move_to(line.source_index(0), false);
    }
}

pub(super) fn enter(
    editor: &mut MarkdownEditor,
    escape: bool,
    cx: &mut Context<MarkdownEditor>,
) -> bool {
    let i = editor.projection.line_for_source(editor.document.cursor());
    let line = &editor.projection.lines[i];
    if let Some(cell) = line.cell.clone() {
        if escape {
            leave(editor, editor.projection.tables[cell.table].source.end, cx);
        } else if let Some(next) = editor.projection.lines.iter().position(|line| {
            line.cell.as_ref().is_some_and(|c| {
                c.table == cell.table && c.row == cell.row + 1 && c.column == cell.column
            })
        }) {
            let next = prepare_cell(editor, next, cx);
            editor.move_to(editor.projection.lines[next].source_index(0), false, cx);
        } else {
            let column = if editor.document.cursor()
                >= editor.projection.tables[cell.table].rows[cell.row].end
            {
                0
            } else {
                cell.column
            };
            add_row(editor, cell.table, column, cx);
        }
        return true;
    }
    if let Some(picture) = line.picture {
        let end = editor
            .document
            .line_range(editor.projection.pictures[picture].source.end)
            .end;
        leave(editor, end, cx);
        return true;
    }
    if line.heading > 0
        && !editor.markdown()[line.source.clone()]
            .trim_start()
            .starts_with('#')
        && editor.document.cursor() >= line.source_index(line.text.len())
    {
        let next = editor
            .document
            .line_range((line.source.end + 1).min(editor.markdown().len()));
        let text = editor.markdown()[next.clone()].trim();
        if !text.is_empty() && text.bytes().all(|b| b == b'=' || b == b'-') {
            leave(editor, next.end, cx);
            return true;
        }
    }
    false
}
fn leave(editor: &mut MarkdownEditor, mut end: usize, cx: &mut Context<MarkdownEditor>) {
    if editor.markdown().as_bytes().get(end) == Some(&b'\r') {
        end += 1;
    }
    if editor.markdown().as_bytes().get(end) == Some(&b'\n') {
        end += 1;
    }
    if end < editor.markdown().len() {
        if let Some(index) = editor
            .projection
            .lines
            .iter()
            .position(|line| line.source.end >= end)
        {
            editor.plain_line_below(index, cx);
        } else {
            editor.move_to(end, false, cx);
        }
    } else if editor.markdown().strip_suffix('\n').is_some_and(|text| {
        text.rsplit('\n')
            .next()
            .is_some_and(|line| line.trim().is_empty())
    }) {
        editor.move_to(end, false, cx);
    } else {
        let newline = if editor.markdown().contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let prefix = newline.repeat(if editor.markdown().ends_with('\n') {
            1
        } else {
            2
        });
        editor
            .document
            .replace_with_history(end..end, &prefix, true);
        editor.changed(cx);
    }
}

pub(super) fn vertical(
    editor: &mut MarkdownEditor,
    down: bool,
    extend: bool,
    cx: &mut Context<MarkdownEditor>,
) -> bool {
    let i = editor.projection.line_for_source(editor.document.cursor());
    let projected = &editor.projection.lines[i];
    let Some(cell) = projected.cell.clone() else {
        return false;
    };
    let Some(line) = editor.layout.get(i) else {
        return false;
    };
    let position = line.position(projected.display_index(editor.document.cursor()));
    let height = line.shape.size(line.line_height).height;
    if (down && position.y + line.line_height < line.origin.y + height)
        || (!down && position.y > line.origin.y)
    {
        return false;
    }
    let row = if down {
        Some(cell.row + 1)
    } else {
        cell.row.checked_sub(1)
    };
    if let Some(next) = row.and_then(|row| {
        editor.projection.lines.iter().position(|line| {
            line.cell
                .as_ref()
                .is_some_and(|c| c.table == cell.table && c.row == row && c.column == cell.column)
        })
    }) {
        let next = prepare_cell(editor, next, cx);
        let next_line = &editor.layout[next];
        let x = editor.preferred_x.unwrap_or(position.x);
        let y = next_line.origin.y
            + if down {
                px(0.)
            } else {
                next_line.shape.size(next_line.line_height).height - next_line.line_height
            };
        let target = editor.projection.lines[next].source_index(next_line.index(point(x, y)));
        editor.preferred_x = Some(x);
        editor.move_to(target, extend, cx);
    } else if down && !extend {
        leave(editor, editor.projection.tables[cell.table].source.end, cx);
    } else {
        let next = if down {
            editor
                .projection
                .lines
                .iter()
                .rposition(|line| line.cell.as_ref().is_some_and(|c| c.table == cell.table))
                .unwrap()
                + 1
        } else {
            i.saturating_sub(cell.column + 1)
        };
        if let Some(line) = editor.projection.lines.get(next) {
            editor.move_to(line.source_index(0), extend, cx);
        }
    }
    true
}

pub(super) fn delete_boundary(
    editor: &mut MarkdownEditor,
    forward: bool,
    cx: &mut Context<MarkdownEditor>,
) -> bool {
    if !editor.document.selection().is_empty() {
        return false;
    }
    let i = editor.projection.line_for_source(editor.document.cursor());
    let line = &editor.projection.lines[i];
    if line.cell.is_none() {
        return false;
    }
    if forward && editor.document.cursor() >= line.source_index(line.text.len()) {
        if let Some(next) = editor.projection.lines.get(i + 1) {
            editor.move_to(next.source_index(0), false, cx);
        }
        true
    } else if !forward && editor.document.cursor() <= line.source_index(0) {
        if let Some(previous) = i
            .checked_sub(1)
            .and_then(|i| editor.projection.lines.get(i))
        {
            editor.move_to(previous.source_index(previous.text.len()), false, cx);
        }
        true
    } else {
        false
    }
}

pub(super) fn input_text(editor: &MarkdownEditor, range: &Range<usize>, text: &str) -> String {
    let line = &editor.projection.lines[editor.projection.line_for_source(range.start)];
    if line.cell.is_some() && line.source.start <= range.start && range.end <= line.source.end {
        let mut escaped = String::new();
        for (i, ch) in text.char_indices() {
            if ch == '|' && text[..i].bytes().rev().take_while(|b| *b == b'\\').count() % 2 == 0 {
                escaped.push('\\');
            }
            if ch != '\r' {
                escaped.push(if ch == '\n' { ' ' } else { ch });
            }
        }
        return escaped;
    }
    if !text.is_empty()
        && range.is_empty()
        && range.start == editor.markdown().len()
        && line.virtual_after
    {
        let newline = if editor.markdown().contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        format!("{newline}{newline}{text}")
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests;

pub(super) fn cell_at(editor: &MarkdownEditor, position: Point<Pixels>) -> Option<usize> {
    let row = editor
        .table_rows
        .iter()
        .find(|row| row.bounds.contains(&position))?;
    let column = ((position.x - row.bounds.left())
        / (row.bounds.size.width / row.cells.len() as f32))
        .floor()
        .max(0.) as usize;
    Some(row.cells.start + column.min(row.cells.len() - 1))
}

/// Add missing GFM cell separators when entering an omitted cell.
pub(super) fn prepare_cell(
    editor: &mut MarkdownEditor,
    index: usize,
    cx: &mut Context<MarkdownEditor>,
) -> usize {
    let Some(cell) = editor.projection.lines[index]
        .cell
        .clone()
        .filter(|c| c.missing)
    else {
        return index;
    };
    let missing = editor
        .projection
        .lines
        .iter()
        .filter(|line| {
            line.cell
                .as_ref()
                .is_some_and(|c| c.table == cell.table && c.row == cell.row && c.missing)
        })
        .count();
    let row = &editor.projection.tables[cell.table].rows[cell.row];
    let raw = editor.markdown()[row.clone()].trim_end_matches(['\r', '\n']);
    let end = row.start + raw.len();
    let trimmed = raw.trim_end();
    let trailing_pipe = trimmed.ends_with('|')
        && trimmed[..trimmed.len() - 1]
            .bytes()
            .rev()
            .take_while(|b| *b == b'\\')
            .count()
            % 2
            == 0;
    let addition = format!(
        "{}{}",
        if trailing_pipe { "" } else { " |" },
        "   |".repeat(missing)
    );
    editor
        .document
        .replace_with_history(end..end, &addition, true);
    editor.changed(cx);
    editor
        .projection
        .lines
        .iter()
        .position(|line| {
            line.cell.as_ref().is_some_and(|c| {
                c.table == cell.table && c.row == cell.row && c.column == cell.column
            })
        })
        .unwrap_or(index)
}

pub(super) fn prepare_current_cell(editor: &mut MarkdownEditor, cx: &mut Context<MarkdownEditor>) {
    if !editor.document.selection().is_empty() {
        return;
    }
    let index = editor.projection.line_for_source(editor.document.cursor());
    if editor.projection.lines[index]
        .cell
        .as_ref()
        .is_some_and(|cell| cell.missing)
    {
        let index = prepare_cell(editor, index, cx);
        editor
            .document
            .move_to(editor.projection.lines[index].source_index(0), false);
    }
}
