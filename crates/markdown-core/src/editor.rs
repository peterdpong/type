use crate::{
    Document, appearance,
    document::{from_utf16, to_utf16},
    projection::{Face, Line, Projection},
};
use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, Element, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
    FontWeight, GlobalElementId, InspectorElementId, KeyBinding, KeyDownEvent, LayoutId,
    MouseButton, MouseDownEvent, Pixels, Point, ScrollWheelEvent, StrikethroughStyle, Style,
    TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window, actions, div, fill, font, point,
    prelude::*, px, relative, rgb, rgba, size,
};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;
pub(crate) mod caret;
mod find;
mod list_controls;
mod outline;
mod preview;
mod selection;
mod structures;
mod styled_line;
use styled_line::StyledLine;
mod toolbelt;
pub use preview::MarkdownPreview;

actions!(
    markdown_core,
    [
        Undo,
        Redo,
        Copy,
        Cut,
        Paste,
        SelectAll,
        Bold,
        Italic,
        InlineCode,
        Heading1,
        Heading2,
        Heading3,
        Heading4,
        Heading5,
        Heading6,
        BulletList,
        OrderedList,
        TaskList,
        ToggleTask,
        Quote,
        Find,
        FindNext,
        FindPrevious
    ]
);

/// Install editor shortcuts once when starting a GPUI application.
pub fn init(cx: &mut App) {
    crate::text_input::init(cx);
    let command = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    macro_rules! bind {
        ($key:expr, $action:expr) => {
            KeyBinding::new(
                &format!("{command}-{}", $key),
                $action,
                Some("MarkdownEditor"),
            )
        };
    }
    cx.bind_keys([
        bind!("z", Undo),
        bind!("shift-z", Redo),
        bind!("c", Copy),
        bind!("x", Cut),
        bind!("v", Paste),
        bind!("a", SelectAll),
        bind!("b", Bold),
        bind!("i", Italic),
        bind!("e", InlineCode),
        bind!("alt-1", Heading1),
        bind!("alt-2", Heading2),
        bind!("alt-3", Heading3),
        bind!("alt-4", Heading4),
        bind!("alt-5", Heading5),
        bind!("alt-6", Heading6),
        bind!("shift-7", BulletList),
        bind!("shift-8", OrderedList),
        bind!("shift-l", TaskList),
        bind!("enter", ToggleTask),
        bind!("shift-9", Quote),
        bind!("f", Find),
        bind!("g", FindNext),
        bind!("shift-g", FindPrevious),
    ]);
}

#[derive(Clone, Copy, Debug)]
pub enum Format {
    Bold,
    Italic,
    Code,
    Heading1,
    Heading2,
    Heading3,
    Heading4,
    Heading5,
    Heading6,
    BulletList,
    OrderedList,
    TaskList,
    Quote,
}
#[derive(Clone, Copy, Debug)]
pub enum EditorEvent {
    Changed,
    SelectionChanged,
    FindVisibilityChanged,
}

/// An embeddable, native GPUI Markdown editing surface. It owns no files or dialogs.
pub struct MarkdownEditor {
    document: Document,
    projection: Projection,
    document_path: Option<std::path::PathBuf>,
    editing_metadata: Option<usize>,
    table_rows: Vec<structures::TableRow>,
    picture_panels: Vec<structures::PicturePanel>,
    find: find::State,
    focus: FocusHandle,
    marked: Option<Range<usize>>,
    selecting: bool,
    drag_selection: selection::Drag,
    toolbelt_dismissed: bool,
    // GPUI 0.2's debug selector map retains removed elements across frames.
    #[cfg(test)]
    toolbelt_bounds: Option<Bounds<Pixels>>,
    focus_subscriptions: Vec<gpui::Subscription>,
    outline: outline::Outline,
    caret_animation: caret::Caret,
    scroll: Pixels,
    max_scroll: Pixels,
    reveal_cursor: bool,
    preferred_x: Option<Pixels>,
    layout: Vec<LaidLine>,
    code_panels: Vec<CodePanel>,
    bounds: Bounds<Pixels>,
    layout_dirty: bool,
    layout_revision: u64,
    layout_scroll: Pixels,
}
impl EventEmitter<EditorEvent> for MarkdownEditor {}
impl Focusable for MarkdownEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

#[derive(Clone)]
struct LaidLine {
    shape: StyledLine,
    origin: Point<Pixels>,
    font_size: Pixels,
    baseline_offset: Pixels,
    line_height: Pixels,
    height: Pixels,
    width: Pixels,
}
#[derive(Clone)]
struct CodePanel {
    bounds: Bounds<Pixels>,
    rows: Range<usize>,
}
impl LaidLine {
    fn position(&self, index: usize) -> Point<Pixels> {
        if let Some(row) = self.shape.wrap_boundaries().iter().position(|boundary| {
            self.shape.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index == index
        }) {
            return self.origin + point(px(0.), self.line_height * (row + 1));
        }
        self.origin
            + self
                .shape
                .position_for_index(index, self.line_height)
                .unwrap_or_default()
    }
    fn index(&self, point: Point<Pixels>) -> usize {
        let local = point - self.origin;
        self.shape
            .closest_index_for_position(
                point::clamp_y(local, self.height, self.line_height),
                self.line_height,
            )
            .unwrap_or_else(|i| i)
    }
    fn selection_rects(&self, projected: &Line, selection: &Range<usize>) -> Vec<Bounds<Pixels>> {
        if selection.is_empty()
            || selection.start > projected.source.end
            || selection.end <= projected.source.start
        {
            return Vec::new();
        }
        let start = projected.display_index(selection.start.max(projected.source.start));
        let end = projected.display_index(selection.end.min(projected.source.end));
        let mut rects = self.display_rects(start..end);
        if selection.end > projected.source.end {
            rects.push(Bounds::new(
                self.position(end),
                size(px(5.), self.line_height),
            ));
        }
        rects
    }
    fn display_rects(&self, range: Range<usize>) -> Vec<Bounds<Pixels>> {
        let start = range.start;
        let end = range.end;
        let row_for_index = |index| {
            self.shape.wrap_boundaries().partition_point(|boundary| {
                self.shape.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index <= index
            })
        };
        let start_row = row_for_index(start);
        let end_row = row_for_index(end);
        let a = self.position(start);
        let b = self.position(end);
        let mut rects = Vec::new();
        for row in start_row..=end_row {
            let left = if row == start_row { a.x } else { self.origin.x };
            let right = if row == end_row {
                b.x
            } else {
                self.origin.x + self.width
            };
            if right > left {
                rects.push(Bounds::new(
                    point(left, self.origin.y + self.line_height * row),
                    size(right - left, self.line_height),
                ));
            }
        }
        rects
    }
}
// GPUI hit testing requires points within a visual line.
mod point {
    use gpui::*;
    pub fn clamp_y(p: Point<Pixels>, height: Pixels, line: Pixels) -> Point<Pixels> {
        point(p.x, p.y.max(px(0.)).min((height - line / 2.).max(px(0.))))
    }
}

impl MarkdownEditor {
    pub fn new(markdown: impl Into<String>, cx: &mut Context<Self>) -> Self {
        let document = Document::new(markdown);
        let projection = Projection::new(document.text(), false);
        let mut editor = Self {
            document,
            projection,
            find: find::State::new(cx),
            focus: cx.focus_handle(),
            marked: None,
            selecting: false,
            drag_selection: selection::Drag::default(),
            toolbelt_dismissed: false,
            #[cfg(test)]
            toolbelt_bounds: None,
            focus_subscriptions: Vec::new(),
            outline: outline::Outline::default(),
            caret_animation: caret::Caret::new(cx.background_executor().now()),
            scroll: px(0.),
            max_scroll: px(0.),
            reveal_cursor: true,
            preferred_x: None,
            layout: Vec::new(),
            code_panels: Vec::new(),
            document_path: None,
            editing_metadata: None,
            table_rows: Vec::new(),
            picture_panels: Vec::new(),
            bounds: Bounds::default(),
            layout_dirty: true,
            layout_revision: 0,
            layout_scroll: px(0.),
        };
        editor
            .document
            .move_to(editor.projection.lines[0].source_index(0), false);
        editor
    }
    /// Finish composition and hide transient controls before moving this editor out of view.
    /// The document, selection, undo history, typing styles, and scroll position are retained.
    pub fn deactivate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        find::hide(self, cx);
        self.finish_composition(window, cx);
        selection::stop(self);
        self.toolbelt_dismissed = true;
        self.outline.collapse_immediately();
        self.outline.clear_backdrop(window);
        self.caret_animation = caret::Caret::new(cx.background_executor().now());
        cx.notify();
    }

    /// Commit in-progress text input before saving or checking unsaved changes.
    pub fn finish_composition(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.marked.is_some() {
            self.unmark_text(window, cx);
        }
    }

    /// Resolve local image URLs relative to this document. File ownership stays with the host.
    pub fn set_document_path(&mut self, path: Option<std::path::PathBuf>, cx: &mut Context<Self>) {
        self.document_path = path;
        cx.notify();
    }

    pub fn markdown(&self) -> &str {
        self.document.text()
    }

    /// A small, inert snapshot of the current rendered document, including unsaved edits.
    pub fn preview(&self) -> MarkdownPreview {
        MarkdownPreview::new(&self.projection)
    }

    pub fn is_find_open(&self) -> bool {
        self.find.open
    }

    fn top_gutter(&self) -> f32 {
        appearance::TOP_GUTTER + if self.find.open { find::HEIGHT } else { 0. }
    }

    fn content_bounds(&self) -> Bounds<Pixels> {
        Bounds::from_corners(
            self.bounds.origin + point(px(0.), px(self.top_gutter()).min(self.bounds.size.height)),
            self.bounds.bottom_right(),
        )
    }
    pub fn format_active(&self, format: Format) -> bool {
        let selection = self.document.selection();
        if !selection.is_empty() {
            let mut seen = false;
            for line in &self.projection.lines {
                if selection.start > line.source.end || selection.end <= line.source.start {
                    continue;
                }
                let start = line.display_index(selection.start.max(line.source.start));
                let end = line.display_index(selection.end.min(line.source.end));
                for span in &line.spans {
                    if span.range.start >= end || span.range.end <= start {
                        continue;
                    }
                    seen = true;
                    let active = match format {
                        Format::Bold => span.face.bold,
                        Format::Italic => span.face.italic,
                        Format::Code => span.face.code && !line.code,
                        _ => false,
                    };
                    if !active {
                        return false;
                    }
                }
            }
            return seen;
        }
        let style = self.document.typing_style();
        match format {
            Format::Bold => style.bold,
            Format::Italic => style.italic,
            Format::Code => style.code,
            _ => false,
        }
    }
    pub fn word_count(&self) -> usize {
        self.projection
            .lines
            .iter()
            .map(|line| line.text.unicode_words().count())
            .sum()
    }
    /// Replace the complete document and reset history, selection, composition, and scroll.
    pub fn set_markdown(&mut self, markdown: impl Into<String>, cx: &mut Context<Self>) {
        self.document = Document::new(markdown);
        self.editing_metadata = None;
        self.caret_animation = caret::Caret::new(cx.background_executor().now());
        self.marked = None;
        selection::stop(self);
        self.outline.reset();
        self.scroll = px(0.);
        self.rebuild();
        self.document
            .move_to(self.projection.lines[0].source_index(0), false);
        self.layout.clear();
        self.code_panels.clear();
        self.changed(cx);
    }
    pub fn format(&mut self, format: Format, cx: &mut Context<Self>) {
        let line = &self.projection.lines[self.projection.line_for_source(self.document.cursor())];
        if self.editing_metadata.is_some()
            || line.code
            || line.math
            || (line.cell.is_some()
                && !matches!(format, Format::Bold | Format::Italic | Format::Code))
        {
            return;
        }
        self.marked = None;
        match format {
            Format::Bold => self.document.toggle_inline("**"),
            Format::Italic => self.document.toggle_inline("_"),
            Format::Code => self.document.toggle_inline("`"),
            Format::Heading1 => self.document.set_prefix("# "),
            Format::Heading2 => self.document.set_prefix("## "),
            Format::Heading3 => self.document.set_prefix("### "),
            Format::Heading4 => self.document.set_prefix("#### "),
            Format::Heading5 => self.document.set_prefix("##### "),
            Format::Heading6 => self.document.set_prefix("###### "),
            Format::BulletList => self.document.set_prefix("- "),
            Format::OrderedList => self.document.set_prefix("1. "),
            Format::TaskList => self.document.set_prefix("- [ ] "),
            Format::Quote => self.document.set_prefix("> "),
        }
        self.changed(cx);
    }
    fn rebuild(&mut self) {
        self.layout_dirty = true;
        self.projection = Projection::with_metadata_edit(
            self.document.text(),
            self.marked.is_some(),
            self.editing_metadata.map(|_| self.document.cursor()),
        );
        self.editing_metadata = self
            .projection
            .editing_metadata
            .then_some(self.document.cursor());
        self.find.refresh(&self.projection, self.document.cursor());
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.outline.document_changed();
        self.caret_animation
            .activity(cx.background_executor().now());
        self.outline.jumped_to = None;
        self.toolbelt_dismissed = false;
        self.rebuild();
        self.reveal_cursor = true;
        self.preferred_x = None;
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }
    fn move_to(&mut self, index: usize, extend: bool, cx: &mut Context<Self>) {
        self.caret_animation
            .activity(cx.background_executor().now());
        self.outline.jumped_to = None;
        self.toolbelt_dismissed = false;
        self.document.move_to(index, extend);
        if self.editing_metadata.is_some() {
            self.rebuild();
        }
        self.reveal_cursor = true;
        cx.emit(EditorEvent::SelectionChanged);
        cx.notify();
    }
    fn hit_test(&self, point: Point<Pixels>) -> usize {
        if self.layout.is_empty() {
            return 0;
        }
        if let Some(row) = self
            .table_rows
            .iter()
            .find(|row| row.bounds.top() <= point.y && point.y <= row.bounds.bottom())
        {
            let column = ((point.x - row.bounds.left())
                / (row.bounds.size.width / row.cells.len() as f32))
                .floor()
                .max(0.) as usize;
            let i = row.cells.start + column.min(row.cells.len() - 1);
            return self.projection.lines[i].source_index(self.layout[i].index(point));
        }
        if let Some(panel) = self
            .picture_panels
            .iter()
            .find(|panel| panel.bounds.contains(&point))
        {
            return self.projection.pictures[panel.picture].source.start;
        }
        let rows = self
            .code_panels
            .iter()
            .find(|panel| {
                point.x >= panel.bounds.left()
                    && point.x <= panel.bounds.right()
                    && point.y >= panel.bounds.top()
                    && point.y <= panel.bounds.bottom()
            })
            .map_or(0..self.layout.len(), |panel| panel.rows.clone());
        let line = self.layout[rows.clone()]
            .partition_point(|line| line.origin.y <= point.y)
            .saturating_sub(1)
            .min(rows.len() - 1)
            + rows.start;
        self.projection.lines[line].source_index(self.layout[line].index(point))
    }
    fn caret(&self) -> Option<(Point<Pixels>, Pixels)> {
        let i = self.projection.line_for_source(self.document.cursor());
        let line = self.layout.get(i)?;
        Some((
            line.position(self.projection.lines[i].display_index(self.document.cursor())),
            line.line_height,
        ))
    }
    fn horizontal(&self, right: bool) -> usize {
        let cursor = self.document.cursor();
        let i = self.projection.line_for_source(cursor);
        let line = &self.projection.lines[i];
        let display = line.display_index(cursor);
        if right {
            if let Some((n, _)) = line
                .text
                .grapheme_indices(true)
                .find(|(n, _)| *n > display && line.source_index(*n) > cursor)
            {
                return line.source_index(n);
            }
            if display < line.text.len() {
                return line.source_index(line.text.len());
            }
            self.projection
                .lines
                .get(i + 1)
                .map_or(self.document.text().len(), |l| l.source_index(0))
        } else {
            if let Some((n, _)) = line
                .text
                .grapheme_indices(true)
                .rev()
                .find(|(n, _)| *n < display && line.source_index(*n) < cursor)
            {
                return line.source_index(n);
            }
            if i > 0 {
                let prev = &self.projection.lines[i - 1];
                prev.source_index(prev.text.len())
            } else {
                line.source_index(0)
            }
        }
    }
    fn insert_exit(
        &mut self,
        offset: usize,
        addition: &str,
        cursor: usize,
        cx: &mut Context<Self>,
    ) {
        self.document.replace(offset..offset, addition);
        self.document.move_to(cursor, false);
        self.changed(cx);
    }
    fn plain_line_below(&mut self, index: usize, cx: &mut Context<Self>) {
        let next = &self.projection.lines[index];
        self.preferred_x = None;
        if next.is_plain() {
            self.move_to(next.source.start, false, cx);
        } else {
            let offset = self.projection.block_start(index);
            let eol = if self.document.text().contains("\r\n") {
                "\r\n"
            } else {
                "\n"
            };
            self.insert_exit(offset, eol, offset, cx);
        }
    }
    fn escape_block_down(&mut self, p: Point<Pixels>, h: Pixels, cx: &mut Context<Self>) -> bool {
        if !self.document.selection().is_empty() {
            return false;
        }
        let i = self.projection.line_for_source(self.document.cursor());
        let line = &self.projection.lines[i];
        let layout = &self.layout[i];
        if p.y + h < layout.origin.y + layout.height {
            return false;
        }
        let next = self.projection.lines.get(i + 1);
        let source = self.document.text();
        let eol = if source.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let end = source.len();
        if let Some(block_index) = line.code_block {
            if next.is_some_and(|next| next.code_block == Some(block_index)) {
                return false;
            }
            if next.is_some() {
                self.plain_line_below(i + 1, cx);
            } else {
                let block = &self.projection.blocks[block_index];
                let addition = if let Some(opening) = &block.opening {
                    // Unfinished fences need a closing delimiter before the next paragraph.
                    let opener = source[opening.clone()].trim_start();
                    let marker_character = opener.chars().next();
                    let marker: String = opener
                        .chars()
                        .take_while(|c| Some(*c) == marker_character)
                        .collect();
                    let prefix = if opening.end == end {
                        format!("{eol}{eol}")
                    } else if source.ends_with('\n') {
                        String::new()
                    } else {
                        eol.to_string()
                    };
                    format!("{prefix}{marker}{eol}")
                } else if source.ends_with('\n') {
                    eol.to_string()
                } else {
                    format!("{eol}{eol}")
                };
                self.insert_exit(end, &addition, end + addition.len(), cx);
            }
            return true;
        }
        if !line.quote
            || next.is_some_and(|next| {
                next.quote
                    && self.document.text()[next.source.clone()]
                        .trim_start()
                        .starts_with('>')
            })
        {
            return false;
        }
        if let Some(next) = next {
            if next.is_plain() && next.text.trim().is_empty() {
                // A blank line ends Markdown lazy quote continuation.
                if i + 2 < self.projection.lines.len() {
                    self.plain_line_below(i + 2, cx);
                } else {
                    self.insert_exit(end, eol, end + eol.len(), cx);
                }
            } else {
                let after = (self.document.line_range(line.source.start).end + 1).min(end);
                let addition = if next.is_plain()
                    || (next.quote
                        && !self.document.text()[next.source.clone()]
                            .trim_start()
                            .starts_with('>'))
                {
                    eol.to_string()
                } else {
                    format!("{eol}{eol}")
                };
                self.insert_exit(after, &addition, after + eol.len(), cx);
            }
        } else {
            let addition = format!("{eol}{eol}");
            self.insert_exit(end, &addition, end + addition.len(), cx);
        }
        true
    }
    fn delete(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.marked = None;
        let mut range = self.document.selection();
        let line_index = self.projection.line_for_source(self.document.cursor());
        let code = self.projection.lines[line_index].code;
        if range.is_empty() {
            let cursor = self.document.cursor();
            let line = &self.projection.lines[self.projection.line_for_source(cursor)];
            let display = line.display_index(cursor);
            if !code && !forward && display == 0 && line_index > 0 {
                let previous = &self.projection.lines[line_index - 1];
                if previous.code && previous.source.end + 1 < line.source.start {
                    self.move_to(previous.source_index(previous.text.len()), false, cx);
                    return;
                }
            }
            if !code
                && forward
                && display == line.text.len()
                && let Some(next) = self.projection.lines.get(line_index + 1)
                && next.code
                && line.source.end + 1 < next.source.start
            {
                self.move_to(next.source_index(0), false, cx);
                return;
            }
            if let Some(block_index) = line.code_block {
                let block = &self.projection.blocks[block_index];
                if !forward && display == 0 && cursor == block.content.start {
                    if self.document.text()[block.content.clone()]
                        .trim()
                        .is_empty()
                    {
                        self.document.replace(block.source.clone(), "");
                        self.changed(cx);
                    }
                    return;
                }
                if forward
                    && display == line.text.len()
                    && self
                        .projection
                        .lines
                        .get(line_index + 1)
                        .and_then(|l| l.code_block)
                        != Some(block_index)
                {
                    return;
                }
            }
            if forward {
                if let Some((i, g)) = line
                    .text
                    .grapheme_indices(true)
                    .find(|(i, _)| *i >= display)
                {
                    let start = line.source_index(i);
                    range = start..start + g.len();
                } else {
                    let end = line.source.end;
                    range = end..self.document.next_grapheme(end);
                }
            } else if display == 0 {
                if line.list_depth.is_some_and(|depth| depth > 0)
                    && !code
                    && self.document.indent_list(true) == Some(true)
                {
                    self.changed(cx);
                    return;
                }
                let prefix = if code {
                    ""
                } else {
                    crate::document::block_prefix(&self.document.text()[line.source.clone()])
                };
                if !prefix.is_empty() {
                    range = line.source.start..line.source.start + prefix.len();
                } else if line.source.start > 0 {
                    range = self.document.previous_grapheme(line.source.start)..line.source.start;
                }
            } else if let Some((i, g)) = line
                .text
                .grapheme_indices(true)
                .rev()
                .find(|(i, _)| *i < display)
            {
                let start = line.source_index(i);
                range = start..start + g.len();
            } else {
                range = self.document.previous_grapheme(cursor)..cursor;
            }
        }
        if !code && !range.is_empty() {
            range = self.projection.expand_escapes(range);
            for marker in ["**", "__", "~~", "==", "^", "_", "*", "`", "~"] {
                let n = marker.len();
                if range.start >= n
                    && self.document.text()[..range.start].ends_with(marker)
                    && self.document.text()[range.end..].starts_with(marker)
                    && (!matches!(marker, "~" | "*" | "**" | "_" | "__" | "==" | "^")
                        || (self
                            .projection
                            .is_inline_marker(range.start - n..range.start)
                            && self.projection.is_inline_marker(range.end..range.end + n)))
                {
                    range = range.start - n..range.end + n;
                }
            }
        }
        if self.document.selection().is_empty() {
            range = self.projection.protect_delete(range, forward);
        }
        if !range.is_empty() {
            self.document.input(range, "", true);
            self.changed(cx);
        }
    }
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.focus.is_focused(window) {
            return;
        }
        if event.keystroke.key == "escape" && self.find.open {
            find::close(self, window, cx);
            cx.stop_propagation();
            return;
        }
        if self.marked.is_some() {
            return;
        }
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        let extend = modifiers.shift;
        let command = modifiers.platform;
        let word = modifiers.alt || (modifiers.control && !cfg!(target_os = "macos"));
        match key {
            "escape" if self.outline.expanded || !self.document.selection().is_empty() => {
                self.outline.expand(false);
                self.toolbelt_dismissed = true;
                cx.notify();
            }
            "left" | "right" => {
                let right = key == "right";
                let range = self.document.selection();
                let index = if command {
                    let line = &self.projection.lines
                        [self.projection.line_for_source(self.document.cursor())];
                    line.source_index(if right { line.text.len() } else { 0 })
                } else if word {
                    let cursor = self.document.cursor();
                    if right {
                        self.document
                            .text()
                            .unicode_word_indices()
                            .map(|(i, w)| i + w.len())
                            .find(|i| *i > cursor)
                            .unwrap_or(self.document.text().len())
                    } else {
                        self.document
                            .text()
                            .unicode_word_indices()
                            .rev()
                            .map(|(i, _)| i)
                            .find(|i| *i < cursor)
                            .unwrap_or(0)
                    }
                } else if !extend && !range.is_empty() {
                    if right { range.end } else { range.start }
                } else {
                    let next = self.horizontal(right);
                    if extend {
                        next
                    } else {
                        self.projection
                            .arrow_stop(self.document.cursor(), next, right)
                    }
                };
                self.preferred_x = None;
                self.move_to(index, extend, cx);
            }
            "up" | "down" => {
                if command {
                    self.move_to(
                        if key == "up" {
                            0
                        } else {
                            self.document.text().len()
                        },
                        extend,
                        cx,
                    );
                } else if structures::vertical(self, key == "down", extend, cx) {
                } else if let Some((p, h)) = self.caret() {
                    if key == "down"
                        && !extend
                        && !word
                        && !modifiers.control
                        && self.escape_block_down(p, h, cx)
                    {
                        cx.stop_propagation();
                        return;
                    }
                    let x = self.preferred_x.unwrap_or(p.x);
                    self.preferred_x = Some(x);
                    let i = self.projection.line_for_source(self.document.cursor());
                    let line = &self.layout[i];
                    let y = if key == "up" && p.y <= line.origin.y && i > 0 {
                        let previous = &self.layout[i - 1];
                        previous.origin.y + previous.height - previous.line_height / 2.
                    } else if key == "down"
                        && p.y + h >= line.origin.y + line.height
                        && i + 1 < self.layout.len()
                    {
                        let next = &self.layout[i + 1];
                        next.origin.y + next.line_height / 2.
                    } else if key == "up" {
                        p.y - h / 2.
                    } else {
                        p.y + h * 1.5
                    };
                    self.move_to(self.hit_test(point(x, y)), extend, cx);
                }
            }
            "home" | "end" => {
                let line =
                    &self.projection.lines[self.projection.line_for_source(self.document.cursor())];
                self.move_to(
                    line.source_index(if key == "home" { 0 } else { line.text.len() }),
                    extend,
                    cx,
                );
            }
            "backspace" => {
                if !structures::delete_boundary(self, false, cx) {
                    self.delete(false, cx);
                }
            }
            "delete" => {
                if !structures::delete_boundary(self, true, cx) {
                    self.delete(true, cx);
                }
            }
            "enter" => {
                if structures::enter(self, command || modifiers.control, cx) {
                    cx.stop_propagation();
                    return;
                }
                let line =
                    &self.projection.lines[self.projection.line_for_source(self.document.cursor())];
                if extend && line.list_depth.is_some() && !line.code {
                    self.document.soft_newline();
                } else {
                    self.document.newline();
                }
                self.changed(cx);
            }
            "tab" if !command && !modifiers.control && !modifiers.alt => {
                if structures::tab(self, extend, cx) {
                    cx.stop_propagation();
                    return;
                }
                let code = self.projection.lines
                    [self.projection.line_for_source(self.document.cursor())]
                .code;
                match if code {
                    None
                } else {
                    self.document.indent_list(extend)
                } {
                    Some(true) => self.changed(cx),
                    Some(false) => {}
                    None if !extend => {
                        self.document.insert("    ");
                        self.changed(cx);
                    }
                    None => {}
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }
    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let selection = self.document.selection();
        if !selection.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.document.text()[selection].to_string(),
            ));
        }
    }
    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        self.copy(&Copy, window, cx);
        if !self.document.selection().is_empty() {
            self.document.insert("");
            self.changed(cx);
        }
    }
    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            structures::prepare_current_cell(self, cx);
            let text = structures::input_text(self, &self.document.selection(), &text);
            self.document.insert(&text);
            self.changed(cx);
        }
    }
    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.caret_animation
            .activity(cx.background_executor().now());
        window.focus(&self.focus);
        self.toolbelt_dismissed = false;
        self.selecting = true;
        self.drag_selection = selection::Drag::default();
        self.preferred_x = None;
        if let Some(cell) = structures::cell_at(self, event.position) {
            structures::prepare_cell(self, cell, cx);
        }
        let index = self.hit_test(event.position);
        if event.modifiers.platform
            && let Some(link) = self
                .projection
                .links
                .iter()
                .find(|link| link.source.contains(&index))
        {
            self.selecting = false;
            if let Some(target) = link.target {
                self.move_to(target, false, cx);
            } else if ["https://", "http://", "mailto:"]
                .iter()
                .any(|prefix| link.destination.starts_with(prefix))
            {
                cx.open_url(&link.destination);
            }
            cx.stop_propagation();
            return;
        }
        if event.click_count >= 3 {
            let line = self.document.line_range(index);
            self.document.select(line.start, line.end);
        } else if event.click_count == 2 {
            if let Some((start, word)) = self
                .document
                .text()
                .unicode_word_indices()
                .find(|(i, w)| *i <= index && index <= i + w.len())
            {
                self.document.select(start, start + word.len());
            }
        } else {
            self.document.move_to(index, event.modifiers.shift);
        }
        if self.editing_metadata.is_some() {
            self.rebuild();
        }
        cx.emit(EditorEvent::SelectionChanged);
        cx.notify();
    }
}

impl EntityInputHandler for MarkdownEditor {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let text = self.document.text();
        let range = from_utf16(text, range.start)..from_utf16(text, range.end);
        *actual = Some(to_utf16(text, range.start)..to_utf16(text, range.end));
        Some(text[range].to_string())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let range = self.document.selection();
        let text = self.document.text();
        Some(UTF16Selection {
            range: to_utf16(text, range.start)..to_utf16(text, range.end),
            reversed: self.document.cursor() < self.document.anchor(),
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked
            .as_ref()
            .map(|r| to_utf16(self.document.text(), r.start)..to_utf16(self.document.text(), r.end))
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(range) = self.marked.take() {
            let text = self.document.text()[range.clone()].to_string();
            self.document.input(range, &text, false);
        }
        self.changed(cx);
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let composing = self.marked.is_some();
        if !composing && range.is_none() {
            structures::prepare_current_cell(self, cx);
        }
        let range = range
            .map(|r| {
                from_utf16(self.document.text(), r.start)..from_utf16(self.document.text(), r.end)
            })
            .or(self.marked.take())
            .unwrap_or(self.document.selection());
        let input = structures::input_text(self, &range, text);
        self.document.input(range, &input, !composing);
        self.editing_metadata = Some(self.document.cursor());
        self.marked = None;
        self.changed(cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let composing = self.marked.is_some();
        if !composing && range.is_none() {
            structures::prepare_current_cell(self, cx);
        }
        let range = range
            .map(|r| {
                from_utf16(self.document.text(), r.start)..from_utf16(self.document.text(), r.end)
            })
            .or(self.marked.take())
            .unwrap_or(self.document.selection());
        let input = structures::input_text(self, &range, text);
        self.document.compose(range, &input, !composing);
        self.editing_metadata = Some(self.document.cursor());
        let start = self.document.cursor().saturating_sub(text.len());
        self.marked = (!text.is_empty()).then_some(start..start + text.len());
        if let Some(r) = selected {
            self.document.select_composition(
                start + from_utf16(text, r.start),
                start + from_utf16(text, r.end),
            );
        }
        self.changed(cx);
    }
    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let source = from_utf16(self.document.text(), range.start);
        let i = self.projection.line_for_source(source);
        let line = self.layout.get(i)?;
        let p = line.position(self.projection.lines[i].display_index(source));
        Some(Bounds::new(p, size(px(2.), line.line_height)))
    }
    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(to_utf16(self.document.text(), self.hit_test(point)))
    }
}

impl Render for MarkdownEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        find::subscribe_input(self, window, cx);
        if self.focus_subscriptions.is_empty() {
            self.focus_subscriptions
                .push(cx.on_release_in(window, |this, window, _| {
                    this.outline.clear_backdrop(window);
                }));
            self.focus_subscriptions
                .push(cx.on_focus(&self.focus, window, |this, _, cx| {
                    this.caret_animation
                        .activity(cx.background_executor().now());
                    cx.notify();
                }));
            self.focus_subscriptions.push(cx.observe_window_activation(
                window,
                |this, window, cx| {
                    if !window.is_window_active() {
                        selection::stop(this);
                    }
                    this.caret_animation
                        .activity(cx.background_executor().now());
                    cx.notify();
                },
            ));
            self.focus_subscriptions
                .push(cx.on_blur(&self.focus, window, |this, _, cx| {
                    selection::stop(this);
                    cx.notify();
                }));
        }
        div()
            .id("markdown-editor")
            .relative()
            .size_full()
            .overflow_hidden()
            .key_context("MarkdownEditor")
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(|this, _: &Find, window, cx| find::open(this, window, cx)))
            .on_action(
                cx.listener(|this, _: &FindNext, window, cx| find::step(this, false, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &FindPrevious, window, cx| {
                    find::step(this, true, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &Undo, _, cx| {
                this.marked = None;
                this.document.undo();
                this.changed(cx);
            }))
            .on_action(cx.listener(|this, _: &Redo, _, cx| {
                this.marked = None;
                this.document.redo();
                this.changed(cx);
            }))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| {
                this.caret_animation
                    .activity(cx.background_executor().now());
                this.toolbelt_dismissed = false;
                this.document.select(0, this.document.text().len());
                cx.emit(EditorEvent::SelectionChanged);
                cx.notify();
            }))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(|this, _: &Bold, _, cx| this.format(Format::Bold, cx)))
            .on_action(cx.listener(|this, _: &Italic, _, cx| this.format(Format::Italic, cx)))
            .on_action(cx.listener(|this, _: &InlineCode, _, cx| this.format(Format::Code, cx)))
            .on_action(cx.listener(|this, _: &Heading1, _, cx| this.format(Format::Heading1, cx)))
            .on_action(cx.listener(|this, _: &Heading2, _, cx| this.format(Format::Heading2, cx)))
            .on_action(cx.listener(|this, _: &Heading3, _, cx| this.format(Format::Heading3, cx)))
            .on_action(cx.listener(|this, _: &Heading4, _, cx| this.format(Format::Heading4, cx)))
            .on_action(cx.listener(|this, _: &Heading5, _, cx| this.format(Format::Heading5, cx)))
            .on_action(cx.listener(|this, _: &Heading6, _, cx| this.format(Format::Heading6, cx)))
            .on_action(
                cx.listener(|this, _: &BulletList, _, cx| this.format(Format::BulletList, cx)),
            )
            .on_action(
                cx.listener(|this, _: &OrderedList, _, cx| this.format(Format::OrderedList, cx)),
            )
            .on_action(cx.listener(|this, _: &TaskList, _, cx| this.format(Format::TaskList, cx)))
            .on_action(cx.listener(|this, _: &ToggleTask, _, cx| {
                let i = this.projection.line_for_source(this.document.cursor());
                if this.projection.lines[i].cell.is_some() {
                    structures::enter(this, true, cx);
                } else if let Some(checkbox) = this.projection.lines[i].checkbox {
                    list_controls::toggle(this, checkbox.offset, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &Quote, _, cx| this.format(Format::Quote, cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                this.caret_animation
                    .activity(cx.background_executor().now());
                this.outline.jumped_to = None;
                this.scroll = (this.scroll
                    - event.delta.pixel_delta(px(appearance::BODY_LINE_HEIGHT)).y)
                    .clamp(px(0.), this.max_scroll);
                this.reveal_cursor = false;
                selection::extend(this, cx);
                cx.notify();
            }))
            .child(EditorSurface {
                editor: cx.entity(),
            })
            .children(find::render(self, cx))
    }
}

struct EditorSurface {
    editor: Entity<MarkdownEditor>,
}
impl IntoElement for EditorSurface {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for EditorSurface {
    type RequestLayoutState = ();
    type PrepaintState = Option<caret::Visual>;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.editor.update(cx, |editor, cx| {
            if !editor.layout_dirty
                && !editor.reveal_cursor
                && !editor.find.reveal
                && editor.bounds == bounds
                && editor.layout_scroll == editor.scroll
            {
                structures::show_pictures(editor, window, cx);
                list_controls::show(editor, window, cx);
                outline::show(editor, window, cx);
                toolbelt::show(editor, window, cx);
                return caret::prepare(editor, window, cx);
            }
            if editor.marked.is_none() {
                editor.outline.has_headings =
                    editor.projection.lines.iter().any(|line| line.heading > 0);
            }
            let has_outline = editor.outline.has_headings;
            let left_margin = if has_outline { px(72.) } else { px(24.) };
            let width = (bounds.size.width - left_margin - px(24.))
                .clamp(px(80.), px(appearance::CONTENT_WIDTH));
            let x = bounds.left() + ((bounds.size.width - width) / 2.).max(left_margin);
            let mut y = bounds.top() + px(editor.top_gutter() + 8.);
            let mut previous_content_bottom = None;
            let mut layout: Vec<LaidLine> = Vec::new();
            let mut table_rows = Vec::new();
            let mut picture_panels = Vec::new();
            let mut table_row_start = (0, px(0.));
            let mut panels = Vec::new();
            let mut panel_start = (0, px(0.));
            let mut list_gutters = Vec::new();
            for line in &editor.projection.lines {
                if let Some(depth) = line.list_depth {
                    list_gutters.resize(list_gutters.len().max(depth + 1), px(24.));
                    if !line.marker.is_empty() && line.checkbox.is_none() {
                        let shape = window.text_system().shape_line(
                            line.marker.clone().into(),
                            px(appearance::BODY_SIZE),
                            &[text_run(line.marker.len(), Face::default())],
                            None,
                        );
                        list_gutters[depth] = list_gutters[depth].max(shape.width + px(8.));
                    }
                }
            }
            for (i, line) in editor.projection.lines.iter().enumerate() {
                if let Some(cell) = &line.cell {
                    if cell.column == 0 {
                        table_row_start = (i, y);
                    }
                    y = table_row_start.1 + px(8.);
                }
                if let Some(picture) = line.picture {
                    y += px(8.);
                    picture_panels.push(structures::PicturePanel {
                        bounds: Bounds::new(point(x, y), size(width, px(220.))),
                        picture,
                    });
                    y += px(228.);
                }
                if line.math {
                    y += px(8.);
                }
                let heading = appearance::heading_style(line.heading);
                if let (Some(style), Some(bottom)) = (heading, previous_content_bottom) {
                    y = y.max(bottom + px(style.space_before));
                }
                let starts_block = line.code
                    && (i == 0 || editor.projection.lines[i - 1].code_block != line.code_block);
                let ends_block = line.code
                    && editor
                        .projection
                        .lines
                        .get(i + 1)
                        .and_then(|l| l.code_block)
                        != line.code_block;
                if starts_block {
                    y += px(8.);
                    panel_start = (i, y);
                    y += px(appearance::CODE_PADDING);
                }
                let font_size = if line.code {
                    appearance::CODE_SIZE
                } else if line.math {
                    18.
                } else if line.picture.is_some() {
                    appearance::UI_SIZE
                } else {
                    heading.map_or(appearance::BODY_SIZE, |style| style.size)
                };
                let height = px(if line.code {
                    appearance::CODE_LINE_HEIGHT
                } else if let Some(style) = heading {
                    style.line_height
                } else if line.text.is_empty()
                    && line.is_plain()
                    && !editor.document.text().is_empty()
                {
                    appearance::PARAGRAPH_GAP
                } else {
                    appearance::BODY_LINE_HEIGHT
                });
                let list_indent = line
                    .list_depth
                    .map_or(px(0.), |depth| {
                        list_gutters[..=depth]
                            .iter()
                            .copied()
                            .fold(px(0.), |sum, width| sum + width)
                    })
                    .min((width - px(64.)).max(px(0.)));
                let indent = list_indent
                    + if line.quote {
                        (px(appearance::BODY_SIZE * 1.25 + 2.) * line.quote_depth)
                            .min((width - list_indent - px(64.)).max(px(0.)))
                    } else if line.code {
                        px(appearance::CODE_PADDING)
                    } else {
                        px(0.)
                    };
                let indent = if let Some(cell) = &line.cell {
                    width / cell.columns as f32 * cell.column + px(12.)
                } else if !line.marker.is_empty() && line.list_depth.is_none() {
                    indent + px(28.)
                } else {
                    indent
                };
                let mut runs = Vec::new();
                for span in &line.spans {
                    let mut run = heading_text_run(span.range.len(), span.face, line.heading);
                    if line.code {
                        run.background_color = None;
                    }
                    if line.quote || line.checkbox.is_some_and(|checkbox| checkbox.checked) {
                        run.color = rgb(appearance::MUTED).into();
                    }
                    let a = line.source_index(span.range.start);
                    let b = line.source_index(span.range.end);
                    if editor
                        .marked
                        .as_ref()
                        .is_some_and(|r| a < r.end && b > r.start)
                    {
                        run.underline = Some(UnderlineStyle {
                            color: None,
                            thickness: px(1.),
                            wavy: false,
                        });
                    }
                    runs.push(run);
                }
                let text_width = if let Some(cell) = &line.cell {
                    (width / cell.columns as f32 - px(24.)).max(px(8.))
                } else {
                    width
                        - indent
                        - if line.code {
                            px(appearance::CODE_PADDING)
                        } else {
                            px(0.)
                        }
                };
                let scripts = line
                    .spans
                    .iter()
                    .map(|span| span.face.script)
                    .collect::<Vec<_>>();
                let shape = StyledLine::new(
                    line.text.clone().into(),
                    px(font_size),
                    &runs,
                    &scripts,
                    text_width,
                    window,
                )
                .expect("Markdown text shaping failed");
                let total_height = shape.size(height).height;
                let spare = (text_width - shape.size(height).width).max(px(0.));
                let align = if line.math {
                    spare / 2.
                } else {
                    line.cell
                        .as_ref()
                        .map_or(px(0.), |cell| match cell.alignment {
                            pulldown_cmark::Alignment::Center => spare / 2.,
                            pulldown_cmark::Alignment::Right => spare,
                            _ => px(0.),
                        })
                };
                // Empty native layouts have no glyph runs for font metrics.
                let (ascent, descent) = if line.text.is_empty() {
                    let probe = window.text_system().shape_line(
                        " ".into(),
                        px(font_size),
                        &[heading_text_run(
                            1,
                            Face {
                                code: line.code,
                                ..Default::default()
                            },
                            line.heading,
                        )],
                        None,
                    );
                    (probe.ascent, probe.descent)
                } else {
                    (shape.ascent(), shape.descent())
                };
                layout.push(LaidLine {
                    shape,
                    origin: point(x + indent + align, y),
                    font_size: px(font_size),
                    baseline_offset: (height - ascent - descent) / 2. + ascent,
                    line_height: height,
                    height: total_height,
                    width: text_width - align,
                });
                y += total_height;
                if line.math {
                    y += px(8.);
                }
                if let Some(cell) = &line.cell
                    && cell.column + 1 == cell.columns
                {
                    let rows = table_row_start.0..i + 1;
                    let height = layout[rows.clone()]
                        .iter()
                        .map(|line| line.height)
                        .max()
                        .unwrap();
                    for line in &mut layout[rows.clone()] {
                        line.height = height;
                    }
                    y = table_row_start.1 + height + px(16.);
                    table_rows.push(structures::TableRow {
                        bounds: Bounds::new(
                            point(x, table_row_start.1),
                            size(width, height + px(16.)),
                        ),
                        cells: rows,
                        header: cell.row == 0,
                    });
                }
                if !line.text.is_empty() || !line.is_plain() {
                    previous_content_bottom = Some(y);
                }
                if ends_block {
                    y += px(appearance::CODE_PADDING);
                    previous_content_bottom = Some(y);
                    panels.push(CodePanel {
                        bounds: Bounds::new(
                            point(x, panel_start.1),
                            size(width, y - panel_start.1),
                        ),
                        rows: panel_start.0..i + 1,
                    });
                    y += px(8.);
                }
            }
            editor.max_scroll = (y - bounds.bottom() + px(100.)).max(px(0.));
            editor.scroll = editor.scroll.clamp(px(0.), editor.max_scroll);
            if editor.reveal_cursor {
                let i = editor.projection.line_for_source(editor.document.cursor());
                if let Some(line) = layout.get(i) {
                    let p = line.position(
                        editor.projection.lines[i].display_index(editor.document.cursor()),
                    );
                    if p.y - editor.scroll < bounds.top() + px(editor.top_gutter() + 8.) {
                        editor.scroll =
                            (p.y - bounds.top() - px(editor.top_gutter() + 8.)).max(px(0.));
                    }
                    if p.y + line.line_height - editor.scroll > bounds.bottom() - px(20.) {
                        editor.scroll = (p.y + line.line_height - bounds.bottom() + px(20.))
                            .min(editor.max_scroll);
                    }
                }
                editor.reveal_cursor = false;
            }
            find::reveal(editor, &layout, bounds);
            for line in &mut layout {
                line.origin.y -= editor.scroll;
            }
            for panel in &mut panels {
                panel.bounds.origin.y -= editor.scroll;
            }
            for row in &mut table_rows {
                row.bounds.origin.y -= editor.scroll;
            }
            for picture in &mut picture_panels {
                picture.bounds.origin.y -= editor.scroll;
            }
            editor.table_rows = table_rows;
            editor.picture_panels = picture_panels;
            editor.layout = layout;
            editor.code_panels = panels;
            editor.bounds = bounds;
            editor.layout_dirty = false;
            editor.layout_revision = editor.layout_revision.wrapping_add(1);
            editor.layout_scroll = editor.scroll;
            structures::show_pictures(editor, window, cx);
            list_controls::show(editor, window, cx);
            outline::show(editor, window, cx);
            toolbelt::show(editor, window, cx);
            caret::prepare(editor, window, cx)
        })
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        caret: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        selection::listen(&self.editor, window);
        let editor = self.editor.read(cx);
        let focus = editor.focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.editor.clone()),
            cx,
        );
        let editor = self.editor.read(cx);
        let selection = editor.document.selection();
        let layout = editor.layout.clone();
        let projection = editor.projection.clone();
        let panels = editor.code_panels.clone();
        let table_rows = editor.table_rows.clone();
        let empty = editor.document.text().is_empty();
        let max_scroll = editor.max_scroll;
        let scroll = editor.scroll;
        let bounds = editor.content_bounds();
        let show_selection = !editor.find.open || editor.focus.is_focused(window);
        let first = layout.partition_point(|line| line.origin.y + line.height < bounds.top());
        let last = layout.partition_point(|line| line.origin.y <= bounds.bottom());
        let highlights = if editor.find.open {
            let matches = &editor.find.matches;
            let start = matches.partition_point(|found| found.line < first);
            let end = matches.partition_point(|found| found.line < last);
            matches[start..end]
                .iter()
                .enumerate()
                .map(|(i, found)| {
                    (
                        found.line,
                        found.range.clone(),
                        editor.find.active == Some(start + i),
                    )
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            structures::paint_tables(&table_rows, window);
            for panel in &panels {
                if panel.bounds.bottom() >= bounds.top() && panel.bounds.top() <= bounds.bottom() {
                    window.paint_quad(
                        fill(panel.bounds, rgb(appearance::SUBTLE))
                            .corner_radii(px(6.))
                            .border_widths(px(1.))
                            .border_color(rgb(appearance::BORDER)),
                    );
                }
            }
            for (i, line) in layout.iter().enumerate() {
                if line.origin.y + line.height < bounds.top() || line.origin.y > bounds.bottom() {
                    continue;
                }
                let projected = &projection.lines[i];
                for depth in 0..projected.quote_depth {
                    window.paint_quad(fill(
                        Bounds::new(
                            point(
                                line.origin.x - px(appearance::BODY_SIZE * 1.25 + 2.) * (depth + 1),
                                line.origin.y,
                            ),
                            size(px(2.), line.height),
                        ),
                        rgb(appearance::BORDER),
                    ));
                }
                if projected.rule {
                    window.paint_quad(fill(
                        Bounds::new(
                            point(line.origin.x, line.origin.y + px(11.)),
                            size(line.width, px(1.)),
                        ),
                        rgb(appearance::BORDER),
                    ));
                }
                if !projected.marker.is_empty() && projected.checkbox.is_none() {
                    let run = text_run(projected.marker.len(), Face::default());
                    let marker = window.text_system().shape_line(
                        projected.marker.clone().into(),
                        px(appearance::BODY_SIZE),
                        &[run],
                        None,
                    );
                    let _ = marker.paint(
                        point(line.origin.x - marker.width - px(8.), line.origin.y),
                        line.line_height,
                        window,
                        cx,
                    );
                }
                let _ = line.shape.paint_background(
                    line.origin,
                    line.line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
                for (_, range, active) in highlights
                    .iter()
                    .skip(highlights.partition_point(|(line, _, _)| *line < i))
                    .take_while(|(line, _, _)| *line == i)
                {
                    for rect in line.display_rects(range.clone()) {
                        window
                            .paint_quad(fill(rect, rgb(if *active { 0xffc96a } else { 0xffedb0 })));
                    }
                }
                if show_selection {
                    for rect in line.selection_rects(projected, &selection) {
                        window.paint_quad(fill(rect, rgb(appearance::SELECTION)));
                    }
                }
                let _ = line.shape.paint(line.origin, line.line_height, window, cx);
            }
            if empty {
                let run = TextRun {
                    color: rgb(appearance::MUTED).into(),
                    ..text_run(22, Face::default())
                };
                let line = window.text_system().shape_line(
                    "Start writing…".into(),
                    px(appearance::BODY_SIZE),
                    &[TextRun {
                        len: "Start writing…".len(),
                        ..run
                    }],
                    None,
                );
                if let Some(first) = layout.first() {
                    let _ = line.paint(first.origin, layout[0].line_height, window, cx);
                }
            }
            if let Some(caret) = caret.as_ref().filter(|caret| caret.opacity > 0.) {
                window.paint_quad(fill(
                    caret.bounds,
                    rgba((appearance::TEXT << 8) | (caret.opacity * 255.) as u32),
                ));
            }
            if max_scroll > px(0.) {
                let track = bounds.size.height;
                let thumb = (track * (track / (track + max_scroll))).max(px(28.));
                let y = bounds.top() + (track - thumb) * (scroll / max_scroll);
                window.paint_quad(fill(
                    Bounds::new(point(bounds.right() - px(5.), y), size(px(3.), thumb)),
                    rgba(0x70707066),
                ));
            }
        });
    }
}
fn heading_text_run(len: usize, face: Face, level: usize) -> TextRun {
    let mut run = text_run(len, face);
    if let Some(style) = appearance::heading_style(level) {
        run.font.weight = FontWeight::SEMIBOLD;
        run.color = rgb(style.color).into();
    }
    run
}

fn text_run(len: usize, face: Face) -> TextRun {
    let mut font = font(if face.code || face.keyboard {
        "Menlo"
    } else {
        appearance::FONT_FAMILY
    });
    if face.bold {
        font.weight = FontWeight::SEMIBOLD;
    }
    if face.italic {
        font = font.italic();
    }
    TextRun {
        len,
        font,
        color: rgb(appearance::TEXT).into(),
        background_color: if face.highlight {
            Some(rgb(0xffecab).into())
        } else {
            (face.code || face.keyboard).then(|| rgb(appearance::SUBTLE).into())
        },
        underline: (face.link && face.script == 0).then_some(UnderlineStyle {
            color: Some(rgb(appearance::MUTED).into()),
            thickness: px(1.),
            wavy: false,
        }),
        strikethrough: face.strike.then_some(StrikethroughStyle {
            color: None,
            thickness: px(1.),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};
    fn setup<'a>(
        cx: &'a mut TestAppContext,
        text: &str,
    ) -> (Entity<MarkdownEditor>, &'a mut VisualTestContext) {
        cx.update(init);
        cx.add_window_view(|window, cx| {
            let editor = MarkdownEditor::new(text, cx);
            window.focus(&editor.focus);
            editor
        })
    }
    #[gpui::test]
    fn keyboard_navigation_keeps_the_caret_below_the_header(cx: &mut TestAppContext) {
        let source = "A paragraph\n".repeat(80);
        let (editor, cx) = setup(cx, &source);
        editor.update_in(cx, |e, _, cx| {
            e.document.move_to(source.len(), false);
            e.changed(cx);
        });
        cx.run_until_parked();
        for _ in 0..60 {
            cx.simulate_keystrokes("up");
            editor.read_with(cx, |e, _| {
                assert!(e.caret().unwrap().0.y >= e.content_bounds().top())
            });
        }
    }

    #[gpui::test]
    fn deactivation_preserves_selection_scroll_and_history_and_hides_transient_controls(
        cx: &mut TestAppContext,
    ) {
        let source = format!("# Heading\n\n{}", "A paragraph\n\n".repeat(100));
        let (editor, cx) = setup(cx, &source);
        editor.update_in(cx, |e, window, cx| {
            e.document.input(source.len()..source.len(), "!", true);
            e.changed(cx);
            e.document.select(9, 4);
            e.scroll = px(200.);
            e.reveal_cursor = false;
            e.outline.expand(true);
            e.selecting = true;
            e.deactivate(window, cx);
            assert_eq!(e.document.cursor(), 4);
            assert_eq!(e.document.anchor(), 9);
            assert_eq!(e.scroll, px(200.));
            assert!(!e.outline.is_open());
            assert!(!e.selecting);
        });
        cx.run_until_parked();
        editor.read_with(cx, |e, _| {
            assert_eq!(e.scroll, px(200.));
            assert_eq!(e.document.selection(), 4..9);
            assert!(e.toolbelt_bounds.is_none());
        });
        cx.dispatch_action(Undo);
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), source));
    }

    #[gpui::test]
    fn deactivation_finishes_ime_composition_in_the_original_document(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "");
        cx.dispatch_action(Bold);
        editor.update_in(cx, |e, window, cx| {
            e.replace_and_mark_text_in_range(None, "かな", Some(2..2), window, cx);
            assert!(e.marked.is_some());
            e.deactivate(window, cx);
            assert!(e.marked.is_none());
            assert_eq!(e.markdown(), "**かな**");
        });
        cx.run_until_parked();
        cx.dispatch_action(Undo);
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), ""));
    }

    #[gpui::test]
    fn remaining_heading_shortcuts_switch_toggle_and_undo(cx: &mut TestAppContext) {
        let title = "Café 👋";
        let tail = "\r\n\r\nBody";
        let (editor, cx) = setup(cx, &format!("# {title}{tail}"));
        let command = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        cx.simulate_keystrokes("end");
        for level in 3..=6 {
            let shortcut = format!("{command}-alt-{level}");
            let prefix = format!("{} ", "#".repeat(level));
            let expected = format!("{prefix}{title}{tail}");
            cx.simulate_keystrokes(&shortcut);
            editor.read_with(cx, |e, _| {
                assert_eq!(e.markdown(), expected);
                assert_eq!(e.projection.lines[0].heading, level);
                assert_eq!(e.projection.lines[0].text, title);
                assert_eq!(e.document.cursor(), prefix.len() + title.len());
            });
            cx.simulate_keystrokes(&shortcut);
            editor.read_with(cx, |e, _| {
                assert_eq!(e.markdown(), format!("{title}{tail}"));
                assert_eq!(e.projection.lines[0].heading, 0);
            });
            cx.dispatch_action(Undo);
            editor.read_with(cx, |e, _| assert_eq!(e.markdown(), expected));
        }
        let code = "```\n### literal code\n```";
        editor.update_in(cx, |e, _, cx| e.set_markdown(code, cx));
        for level in 3..=6 {
            cx.simulate_keystrokes(&format!("{command}-alt-{level}"));
            editor.read_with(cx, |e, _| assert_eq!(e.markdown(), code));
        }
    }

    #[gpui::test]
    fn heading_styling_waits_for_a_space_after_the_markers(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "");
        for level in 1..=6 {
            editor.update_in(cx, |e, _, cx| e.set_markdown("", cx));
            for count in 1..=level {
                cx.simulate_input("#");
                editor.read_with(cx, |e, _| {
                    assert_eq!(e.projection.lines[0].heading, 0);
                    assert_eq!(e.projection.lines[0].text, "#".repeat(count));
                    assert_eq!(e.layout[0].font_size, px(appearance::BODY_SIZE));
                    assert!(!e.outline.has_headings);
                });
            }
            cx.simulate_input(" ");
            editor.read_with(cx, |e, _| {
                assert_eq!(e.markdown(), format!("{} ", "#".repeat(level)));
                assert_eq!(e.projection.lines[0].heading, level);
                assert!(e.projection.lines[0].text.is_empty());
                assert_eq!(e.document.cursor(), level + 1);
            });
            cx.dispatch_action(Undo);
            editor.read_with(cx, |e, _| {
                assert_eq!(e.markdown(), "#".repeat(level));
                assert_eq!(e.projection.lines[0].text, "#".repeat(level));
                assert_eq!(e.projection.lines[0].heading, 0);
                assert!(!e.outline.has_headings);
            });
            cx.dispatch_action(Redo);
            cx.simulate_input("Héllø");
            editor.read_with(cx, |e, _| {
                assert_eq!(e.projection.lines[0].heading, level);
                assert_eq!(e.projection.lines[0].text, "Héllø");
            });
        }
    }

    #[gpui::test]
    fn typing_remaining_headings_hides_markers_and_enter_returns_to_body(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "");
        for level in 3..=6 {
            editor.update_in(cx, |e, _, cx| e.set_markdown("", cx));
            let heading = format!("{} Héllø", "#".repeat(level));
            cx.simulate_input(&heading);
            editor.read_with(cx, |e, _| {
                assert_eq!(e.projection.lines[0].heading, level);
                assert_eq!(e.projection.lines[0].text, "Héllø");
            });
            cx.simulate_keystrokes("enter");
            cx.simulate_input("Body");
            editor.read_with(cx, |e, _| {
                assert_eq!(e.markdown(), format!("{heading}\nBody"));
                assert_eq!(e.projection.lines[1].heading, 0);
                assert_eq!(e.projection.lines[1].text, "Body");
            });
        }
    }

    #[gpui::test]
    fn native_input_navigation_and_undo(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "");
        cx.simulate_input("Hello 🦀");
        cx.simulate_keystrokes("left backspace");
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "Hello🦀"));
        cx.dispatch_action(Undo);
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "Hello 🦀"));
        cx.dispatch_action(SelectAll);
        cx.simulate_input("# Heading");
        cx.simulate_keystrokes("enter");
        cx.simulate_input("Body text");
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "# Heading\nBody text"));
    }
    #[gpui::test]
    fn formatting_and_clipboard(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "hello");
        cx.dispatch_action(SelectAll);
        cx.dispatch_action(Bold);
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "**hello**"));
        cx.dispatch_action(Bold);
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "hello"));
        cx.dispatch_action(Copy);
        cx.simulate_keystrokes("right");
        cx.dispatch_action(Paste);
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "hellohello"));
    }
    #[gpui::test]
    fn delete_does_not_damage_hidden_delimiters(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "**ab** end");
        cx.simulate_keystrokes("right right backspace");
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "**a** end"));
    }
    #[gpui::test]
    fn wraps_and_hit_tests_real_text_layout(cx: &mut TestAppContext) {
        let (editor, cx) = setup(
            cx,
            "# Title\n\nA line of text with **bold words** and an emoji 🦀.",
        );
        editor.read_with(cx, |e, _| {
            assert_eq!(e.layout.len(), 3);
            let line = &e.layout[2];
            let point = line.position(2);
            assert_eq!(e.hit_test(point), e.projection.lines[2].source_index(2));
        });
    }

    #[gpui::test]
    fn deleting_last_formatted_character_removes_empty_markers(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "**a**");
        cx.simulate_keystrokes("right backspace");
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), ""));
        cx.dispatch_action(Undo);
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "**a**"));
    }

    #[gpui::test]
    fn delete_at_rendered_line_end_keeps_closing_markers(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "**ab**\nnext");
        cx.simulate_keystrokes("right right delete");
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "**ab**next"));
    }

    #[gpui::test]
    fn ime_composition_uses_relative_utf16_ranges_and_one_undo(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "prefix ");
        cx.simulate_keystrokes("end");
        editor.update_in(cx, |e, window, cx| {
            e.replace_and_mark_text_in_range(None, "a", Some(1..1), window, cx);
            e.replace_and_mark_text_in_range(None, "🦀é", Some(2..3), window, cx);
            assert_eq!(e.document.selection(), 11..13);
            assert_eq!(e.marked_text_range(window, cx), Some(7..10));
            e.replace_text_in_range(None, "🦀é", window, cx);
        });
        cx.dispatch_action(Undo);
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "prefix "));
    }

    #[gpui::test]
    fn inline_shortcuts_toggle_future_input_without_inserting_text(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "");
        let command = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        cx.simulate_keystrokes(&format!("{command}-b"));
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), "");
            assert!(e.format_active(Format::Bold));
        });
        cx.simulate_input("Hello");
        cx.simulate_input(" ");
        cx.simulate_input("world");
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "**Hello world**"));
        cx.simulate_keystrokes(&format!("{command}-b"));
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "**Hello world**"));
        cx.simulate_input(" plain");
        cx.simulate_keystrokes(&format!("{command}-i"));
        cx.simulate_input(" italic");
        cx.simulate_keystrokes(&format!("{command}-i"));
        cx.simulate_input(" normal");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), "**Hello world** plain *italic* normal")
        });
    }

    #[gpui::test]
    fn combined_formats_and_double_toggle(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "");
        cx.dispatch_action(Bold);
        cx.dispatch_action(Bold);
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), ""));
        cx.simulate_input("plain ");
        cx.dispatch_action(Bold);
        cx.dispatch_action(Italic);
        cx.simulate_input("both");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), "plain ***both***");
            assert!(
                e.projection.lines[0]
                    .spans
                    .iter()
                    .any(|span| span.face.bold && span.face.italic)
            );
        });
        cx.dispatch_action(Bold);
        cx.simulate_input(" italic");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), "plain ***both*** *italic*");
            assert!(
                e.projection.lines[0]
                    .spans
                    .iter()
                    .any(|span| !span.face.bold && span.face.italic)
            );
        });
    }

    #[gpui::test]
    fn pending_format_applies_to_composition_and_undo_restores_the_mode(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "");
        cx.dispatch_action(Bold);
        editor.update_in(cx, |e, window, cx| {
            e.replace_and_mark_text_in_range(None, "e", Some(1..1), window, cx);
            e.replace_text_in_range(None, "é", window, cx);
            assert_eq!(e.markdown(), "**é**");
        });
        cx.dispatch_action(Undo);
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), "");
            assert!(e.format_active(Format::Bold));
        });
        cx.simulate_input("again");
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "**again**"));
    }

    #[gpui::test]
    fn deleting_while_typing_keeps_the_enabled_style(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "");
        cx.dispatch_action(Bold);
        cx.simulate_input("Hello");
        cx.simulate_keystrokes("backspace");
        cx.simulate_input("o");
        cx.simulate_input(" ");
        cx.simulate_input("world");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.projection.lines[0].text, "Hello world");
            assert!(e.format_active(Format::Bold));
            assert!(
                e.projection.lines[0]
                    .spans
                    .iter()
                    .filter(|span| !e.projection.lines[0].text[span.range.clone()]
                        .trim()
                        .is_empty())
                    .all(|span| span.face.bold)
            );
        });
    }
    #[gpui::test]
    fn typing_a_fence_starts_an_editable_block_immediately(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "");
        cx.simulate_input("`");
        cx.simulate_input("`");
        cx.simulate_input("`");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), "```\n\n```\n");
            assert_eq!(e.document.cursor(), 4);
            assert_eq!(e.code_panels.len(), 1);
            assert!(e.projection.lines[0].code);
        });
        cx.simulate_input("let x = 1;");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), "```\nlet x = 1;\n```\n");
            assert_eq!(e.projection.lines[0].text, "let x = 1;");
        });
        for _ in "let x = 1;".chars() {
            cx.dispatch_action(Undo);
        }
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "```\n\n```\n"));
        cx.dispatch_action(Undo);
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "``"));
    }

    #[gpui::test]
    fn clicking_empty_block_padding_accepts_input_without_enter(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "Before\n\n```rust\n```\n\nAfter");
        let position = editor.read_with(cx, |e, _| {
            point(
                e.code_panels[0].bounds.left() + px(8.),
                e.code_panels[0].bounds.top() + px(5.),
            )
        });
        cx.simulate_click(position, gpui::Modifiers::default());
        cx.simulate_input("fn main() {}");
        editor.read_with(cx, |e, _| {
            assert_eq!(
                e.markdown(),
                "Before\n\n```rust\nfn main() {}\n```\n\nAfter"
            )
        });
    }

    #[gpui::test]
    fn code_panel_has_one_background_consistent_rows_and_padding(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "```rust\nlet x = 1;\n\n    x + 1\n```\n");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.code_panels.len(), 1);
            let panel = &e.code_panels[0];
            assert_eq!(panel.rows, 0..3);
            assert_eq!(panel.bounds.size.height, px(106.25));
            for (index, line) in e.layout[..3].iter().enumerate() {
                assert_eq!(line.line_height, px(24.75));
                assert_eq!(line.shape.font_size(), px(12.75));
                assert_eq!(line.origin.x - panel.bounds.left(), px(16.));
                assert_eq!(
                    line.origin.y - panel.bounds.top(),
                    px(16. + 24.75 * index as f32)
                );
                assert_eq!(line.width, panel.bounds.size.width - px(32.));
            }
        });
    }

    #[gpui::test]
    fn code_newlines_keep_indentation_and_literal_markdown(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "```\n    # literal\n```\n");
        cx.simulate_keystrokes("end enter");
        cx.simulate_input("**not bold**");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), "```\n    # literal\n    **not bold**\n```\n");
            assert!(
                e.projection.lines[1]
                    .spans
                    .iter()
                    .all(|span| span.face.code && !span.face.bold)
            );
        });
        cx.simulate_keystrokes("home delete");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.projection.lines[1].text, "   **not bold**")
        });
    }

    #[gpui::test]
    fn vertical_navigation_skips_fences_and_panel_padding(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "Before\n```\nfirst\nlast\n```\nAfter");
        cx.simulate_keystrokes("down");
        editor.read_with(cx, |e, _| assert_eq!(e.document.cursor(), 11));
        cx.simulate_keystrokes("down down");
        editor.read_with(cx, |e, _| {
            assert_eq!(&e.markdown()[e.document.cursor()..], "After")
        });
        cx.simulate_keystrokes("up");
        editor.read_with(cx, |e, _| {
            assert_eq!(&e.markdown()[e.document.cursor()..], "last\n```\nAfter")
        });
    }

    #[gpui::test]
    fn tabs_keep_alignment_and_delete_as_one_source_character(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "```\n\tlet x = 1;\n```\n");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.projection.lines[0].text, "    let x = 1;")
        });
        cx.simulate_keystrokes("right backspace");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), "```\nlet x = 1;\n```\n")
        });
    }
    #[gpui::test]
    fn wrapped_code_stays_inside_its_panel_and_moves_down(cx: &mut TestAppContext) {
        let source = format!("```\n{}\n```\n", "let value = some_function(); ".repeat(12));
        let (editor, cx) = setup(cx, &source);
        let rows = editor.read_with(cx, |e, _| {
            let line = &e.layout[0];
            assert!(line.height > line.line_height);
            assert_eq!(e.code_panels[0].bounds.size.height, line.height + px(32.));
            line.shape.wrap_boundaries().len() + 1
        });
        let mut previous = 0;
        for _ in 1..rows {
            cx.simulate_keystrokes("down");
            let cursor = editor.read_with(cx, |e, _| e.document.cursor());
            assert!(cursor > previous);
            previous = cursor;
        }
    }

    #[gpui::test]
    fn deleting_at_code_boundaries_preserves_fences(cx: &mut TestAppContext) {
        let source = "```\ncode\n```\nAfter";
        let (editor, cx) = setup(cx, source);
        cx.simulate_keystrokes("backspace end delete down home backspace");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), source);
            assert_eq!(e.document.cursor(), 8);
        });
    }

    #[gpui::test]
    fn composing_in_an_unfinished_block_keeps_the_body_visible(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "```");
        editor.update_in(cx, |e, window, cx| {
            e.replace_and_mark_text_in_range(None, "e", Some(1..1), window, cx);
            assert_eq!(e.marked, Some(4..5));
            assert_eq!(e.projection.lines[0].text, "e");
            assert!(e.projection.lines[0].code);
            e.replace_text_in_range(None, "é", window, cx);
            assert_eq!(e.markdown(), "```\né");
        });
    }

    #[gpui::test]
    fn inline_arrows_stop_on_both_sides_of_markers(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "");
        for (marker, format) in [
            ("`", Format::Code),
            ("``", Format::Code),
            ("**", Format::Bold),
            ("__", Format::Bold),
            ("*", Format::Italic),
            ("_", Format::Italic),
        ] {
            let source = format!("before {marker}é🦀{marker} after");
            let start = 7 + marker.len();
            let end = start + "é🦀".len();
            editor.update_in(cx, |e, _, cx| {
                e.set_markdown(source.clone(), cx);
                e.move_to(start, false, cx);
            });
            cx.run_until_parked();
            let position = editor.read_with(cx, |e, _| e.caret().unwrap().0);
            cx.simulate_keystrokes("left");
            editor.read_with(cx, |e, _| {
                assert_eq!(e.document.cursor(), 7, "{source}");
                assert!(!e.format_active(format));
                assert_eq!(e.caret().unwrap().0, position);
            });
            cx.simulate_keystrokes("right");
            editor.read_with(cx, |e, _| {
                assert_eq!(e.document.cursor(), start);
                assert!(e.format_active(format));
            });
            cx.simulate_keystrokes("right right");
            editor.read_with(cx, |e, _| {
                assert_eq!(e.document.cursor(), end);
                assert!(e.format_active(format));
            });
            let position = editor.read_with(cx, |e, _| e.caret().unwrap().0);
            cx.simulate_keystrokes("right");
            editor.read_with(cx, |e, _| {
                assert_eq!(e.document.cursor(), end + marker.len());
                assert!(!e.format_active(format));
                assert_eq!(e.caret().unwrap().0, position);
            });
            cx.simulate_keystrokes("left");
            editor.read_with(cx, |e, _| {
                assert_eq!(e.document.cursor(), end);
                assert!(e.format_active(format));
                assert_eq!(e.markdown(), source);
            });
        }
    }

    #[gpui::test]
    fn typing_after_arrows_respects_inline_boundary_side(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "`ab` tail");
        cx.simulate_keystrokes("right right right");
        cx.simulate_input("X");
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "`ab`X tail"));
        cx.simulate_keystrokes("left left");
        cx.simulate_input("Y");
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "`abY`X tail"));
        cx.simulate_keystrokes("home left");
        cx.simulate_input("Z");
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "Z`abY`X tail"));
        cx.simulate_keystrokes("right");
        cx.simulate_input("Q");
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "Z`QabY`X tail"));
    }

    #[gpui::test]
    fn typing_outside_inline_styles_preserves_the_existing_rendering(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "");
        for source in [
            "**ab**",
            "*ab*",
            "__ab__",
            "_ab_",
            "`ab`",
            "``ab``",
            "~ab~",
            "~~ab~~",
            "[ab](https://example.com)",
        ] {
            editor.update_in(cx, |e, _, cx| e.set_markdown(source, cx));
            cx.run_until_parked();
            cx.simulate_keystrokes("end right");
            cx.simulate_input("X");
            editor.read_with(cx, |e, _| {
                let line = &e.projection.lines[0];
                assert_eq!(line.text, "abX", "{source}");
                assert_ne!(line.spans[0].face, Face::default());
                assert_eq!(line.spans.last().unwrap().face, Face::default());
            });
            cx.dispatch_action(Undo);
            editor.read_with(cx, |e, _| assert_eq!(e.markdown(), source));
            cx.simulate_keystrokes("home left");
            cx.simulate_input("X");
            editor.read_with(cx, |e, _| {
                let line = &e.projection.lines[0];
                assert_eq!(line.text, "Xab", "{source}");
                assert_eq!(line.spans[0].face, Face::default());
                assert_ne!(line.spans.last().unwrap().face, Face::default());
            });
        }
    }

    #[gpui::test]
    fn composition_outside_underscore_formatting_is_plain_and_undoable(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "__ab__");
        cx.simulate_keystrokes("end right");
        editor.update_in(cx, |e, window, cx| {
            e.replace_and_mark_text_in_range(None, "e", Some(1..1), window, cx);
            e.replace_text_in_range(None, "é", window, cx);
            assert_eq!(e.markdown(), "**ab**é");
            assert_eq!(e.document.cursor(), "**ab**é".len());
            assert_eq!(e.projection.lines[0].text, "abé");
            assert_eq!(
                e.projection.lines[0].spans.last().unwrap().face,
                Face::default()
            );
        });
        cx.dispatch_action(Undo);
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "__ab__"));
    }

    #[gpui::test]
    fn inline_boundaries_work_for_nested_and_adjacent_styles(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "***ab***");
        cx.simulate_keystrokes("left");
        editor.read_with(cx, |e, _| {
            assert!(!e.format_active(Format::Bold));
            assert!(e.format_active(Format::Italic));
        });
        cx.simulate_keystrokes("left");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.document.cursor(), 0);
            assert!(!e.format_active(Format::Italic));
        });
        cx.simulate_keystrokes("left");
        editor.read_with(cx, |e, _| assert_eq!(e.document.cursor(), 0));
        cx.simulate_keystrokes("right right right right right right");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.document.cursor(), 8);
            assert!(!e.format_active(Format::Bold));
            assert!(!e.format_active(Format::Italic));
            assert_eq!(e.markdown(), "***ab***");
        });
        cx.simulate_keystrokes("right");
        editor.read_with(cx, |e, _| assert_eq!(e.document.cursor(), 8));
        editor.update_in(cx, |e, _, cx| e.set_markdown("`a`**b**", cx));
        cx.run_until_parked();
        cx.simulate_keystrokes("right right");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.document.cursor(), 3);
            assert!(!e.format_active(Format::Code));
            assert!(!e.format_active(Format::Bold));
        });
        cx.simulate_keystrokes("right");
        editor.read_with(cx, |e, _| assert!(e.format_active(Format::Bold)));
        cx.simulate_keystrokes("left left");
        editor.read_with(cx, |e, _| assert!(e.format_active(Format::Code)));
    }

    #[gpui::test]
    fn down_leaves_closed_code_without_requiring_enter(cx: &mut TestAppContext) {
        let source = "```\nabc\n```";
        let (editor, cx) = setup(cx, source);
        cx.simulate_keystrokes("end down");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.document.cursor(), source.len());
            assert_eq!(e.markdown(), source);
            assert!(
                e.projection.lines[e.projection.line_for_source(e.document.cursor())].is_plain()
            );
        });
        cx.simulate_input("plain");
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "```\nabc\n```\nplain"));
    }

    #[gpui::test]
    fn down_closes_unfinished_code_and_preserves_line_endings(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "");
        for (source, expected) in [
            ("```", "```\n\n```\n"),
            ("````rust\nabc", "````rust\nabc\n````\n"),
            ("~~~js\r\nabc", "~~~js\r\nabc\r\n~~~\r\n"),
            ("    abc", "    abc\n\n"),
        ] {
            editor.update_in(cx, |e, _, cx| e.set_markdown(source, cx));
            cx.run_until_parked();
            cx.simulate_keystrokes("end down");
            editor.read_with(cx, |e, _| {
                assert_eq!(e.markdown(), expected);
                assert_eq!(e.document.cursor(), expected.len());
                assert!(
                    e.projection.lines[e.projection.line_for_source(e.document.cursor())]
                        .is_plain()
                );
            });
            cx.simulate_input("x");
            editor.read_with(cx, |e, _| assert_eq!(e.markdown(), format!("{expected}x")));
            cx.dispatch_action(Undo);
            cx.dispatch_action(Undo);
            editor.read_with(cx, |e, _| assert_eq!(e.markdown(), source));
            cx.dispatch_action(Redo);
            editor.read_with(cx, |e, _| assert_eq!(e.markdown(), expected));
        }
    }

    #[gpui::test]
    fn down_leaves_quotes_as_real_plain_markdown(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "");
        for (source, expected) in [
            ("> quote", "> quote\n\n"),
            ("> quote\n", "> quote\n\n"),
            ("> quote\n\n", "> quote\n\n"),
            ("> quote\r\n", "> quote\r\n\r\n"),
        ] {
            editor.update_in(cx, |e, _, cx| e.set_markdown(source, cx));
            cx.run_until_parked();
            cx.simulate_keystrokes("end down");
            editor.read_with(cx, |e, _| {
                assert_eq!(e.markdown(), expected);
                assert_eq!(e.document.cursor(), expected.len());
            });
            cx.simulate_input("plain");
            editor.read_with(cx, |e, _| {
                assert_eq!(e.markdown(), format!("{expected}plain"));
                let quote_end = pulldown_cmark::Parser::new(e.markdown())
                    .into_offset_iter()
                    .find_map(|(event, range)| {
                        matches!(
                            event,
                            pulldown_cmark::Event::Start(pulldown_cmark::Tag::BlockQuote(_))
                        )
                        .then_some(range.end)
                    })
                    .unwrap();
                assert!(quote_end < expected.len());
            });
        }
    }

    #[gpui::test]
    fn down_stays_in_multiline_and_wrapped_quotes_until_last_row(cx: &mut TestAppContext) {
        let source = format!("> first\n> {}", "long quote ".repeat(30));
        let (editor, cx) = setup(cx, &source);
        cx.simulate_keystrokes("down");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.projection.line_for_source(e.document.cursor()), 1);
            assert_eq!(e.markdown(), source);
            assert!(e.layout[1].height > e.layout[1].line_height);
        });
        cx.simulate_keystrokes("down");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.projection.line_for_source(e.document.cursor()), 1);
            assert_eq!(e.markdown(), source);
        });
        cx.simulate_keystrokes("end down");
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), format!("{source}\n\n")));
    }

    #[gpui::test]
    fn down_reuses_paragraphs_and_leaves_neighboring_blocks_intact(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "");
        for (source, expected, cursor) in [
            ("> q\n\nAfter", "> q\n\nAfter", 5),
            ("> q\nAfter", "> q\n\nAfter", 5),
            ("> q\n# Header", "> q\n\n\n# Header", 5),
            ("> q\n\n```\nx\n```", "> q\n\n\n```\nx\n```", 5),
            ("```\nx\n```\n> q", "```\nx\n```\n\n> q", 10),
        ] {
            editor.update_in(cx, |e, _, cx| e.set_markdown(source, cx));
            cx.run_until_parked();
            cx.simulate_keystrokes("end down");
            editor.read_with(cx, |e, _| {
                assert_eq!(e.markdown(), expected);
                assert_eq!(e.document.cursor(), cursor);
                assert!(e.projection.lines[e.projection.line_for_source(cursor)].is_plain());
            });
        }
    }

    #[gpui::test]
    fn shift_arrows_select_without_creating_block_exits_or_boundary_stops(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "> quote");
        cx.simulate_keystrokes("end shift-down");
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "> quote"));
        editor.update_in(cx, |e, _, cx| e.set_markdown("`ab` x", cx));
        cx.run_until_parked();
        cx.simulate_keystrokes("shift-right shift-right");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.document.selection(), 1..4);
            assert_eq!(e.markdown(), "`ab` x");
        });
    }

    #[gpui::test]
    fn selection_is_continuous_across_inline_styles_and_inside_code_blocks(
        cx: &mut TestAppContext,
    ) {
        let (editor, cx) = setup(cx, "");
        for source in [
            "before code after",
            "before `code` after",
            "before **code** after",
            "```\nbefore code after\n```",
        ] {
            editor.update_in(cx, |e, _, cx| e.set_markdown(source, cx));
            cx.run_until_parked();
            cx.simulate_keystrokes(&format!("home {}", "shift-right ".repeat(11)));
            editor.read_with(cx, |e, _| {
                let line = &e.layout[0];
                let projected = &e.projection.lines[0];
                let rects = line.selection_rects(projected, &e.document.selection());
                assert_eq!(rects.len(), 1, "{source}");
                assert_eq!(rects[0].origin, line.origin);
                assert_eq!(rects[0].right(), line.position(11).x);
                assert_eq!(rects[0].size.height, line.line_height);
                assert_eq!(e.markdown(), source);
                let cursor = e.document.cursor();
                assert!(
                    line.selection_rects(projected, &(cursor..cursor))
                        .is_empty()
                );
            });
        }
    }

    #[gpui::test]
    fn wrapped_selection_ends_at_the_selected_glyph_without_rounding_drift(
        cx: &mut TestAppContext,
    ) {
        let source = format!("before `{}` after", "some inline code ".repeat(100));
        let (editor, cx) = setup(cx, &source);
        editor.read_with(cx, |e, _| {
            let line = &e.layout[0];
            let projected = &e.projection.lines[0];
            assert!(line.shape.wrap_boundaries().len() > 8);
            let wrapped_index = |row: usize| {
                let boundary = &line.shape.wrap_boundaries()[row - 1];
                line.shape.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index
            };
            let start = wrapped_index(1) + 2;
            for row in 3..=8 {
                let end = wrapped_index(row) + 4;
                let selection = projected.source_index(start)..projected.source_index(end);
                let rects = line.selection_rects(projected, &selection);
                assert_eq!(rects.len(), row);
                assert_eq!(rects[0].left(), line.position(start).x);
                let last = rects.last().unwrap();
                assert_eq!(last.right(), line.position(end).x);
                assert!(last.size.width < line.width);
                for (index, rect) in rects.iter().enumerate() {
                    assert_eq!(rect.top(), line.origin.y + line.line_height * (index + 1));
                    assert_eq!(rect.size.height, line.line_height);
                }
            }
            let boundary = wrapped_index(2);
            let rects = line.selection_rects(
                projected,
                &(projected.source_index(start)..projected.source_index(boundary)),
            );
            assert_eq!(
                rects.len(),
                1,
                "a soft-wrap edge must not select the next row"
            );
        });
    }

    #[gpui::test]
    fn code_selection_includes_tabs_and_empty_lines_within_the_panel(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "```\r\n\tlet é = 🦀;\r\n\r\nend\r\n```\r\n");
        cx.dispatch_action(SelectAll);
        editor.read_with(cx, |e, _| {
            let panel = &e.code_panels[0];
            for index in panel.rows.clone() {
                let rects = e.layout[index]
                    .selection_rects(&e.projection.lines[index], &e.document.selection());
                assert!(!rects.is_empty(), "code row {index} has no selection");
                for rect in rects {
                    assert!(rect.size.width > px(0.));
                    assert!(rect.left() >= panel.bounds.left());
                    assert!(rect.right() <= panel.bounds.right());
                    assert!(rect.top() >= panel.bounds.top());
                    assert!(rect.bottom() <= panel.bounds.bottom());
                }
            }
            let projected = &e.projection.lines[0];
            let line = &e.layout[0];
            let tab = projected.source_index(0);
            let rects = line.selection_rects(projected, &(tab..tab + 1));
            assert_eq!(rects.len(), 1);
            assert_eq!(rects[0].right(), line.position(4).x);
            assert_eq!(rects[0].left(), line.origin.x);
        });
    }

    #[gpui::test]
    fn toolbelt_follows_drag_selection_and_keeps_it_when_formatting(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "Hello world");
        let (start, end) = editor.read_with(cx, |e, _| {
            let line = &e.layout[0];
            (
                line.position(0) + point(px(0.), line.line_height / 2.),
                line.position(5) + point(px(0.), line.line_height / 2.),
            )
        });
        cx.simulate_mouse_down(start, MouseButton::Left, gpui::Modifiers::default());
        cx.simulate_mouse_move(end, MouseButton::Left, gpui::Modifiers::default());
        assert!(editor.read_with(cx, |e, _| e.toolbelt_bounds.is_none()));
        cx.simulate_mouse_up(end, MouseButton::Left, gpui::Modifiers::default());
        assert!(cx.debug_bounds("selection-toolbelt").is_some());
        let bold = cx.debug_bounds("toolbelt-bold").unwrap().center();
        cx.simulate_click(bold, gpui::Modifiers::default());
        editor.update_in(cx, |e, window, _| {
            assert_eq!(e.markdown(), "**Hello** world");
            assert_eq!(e.document.selection(), 2..7);
            assert!(e.format_active(Format::Bold));
            assert!(e.focus.is_focused(window));
        });
        let italic = cx.debug_bounds("toolbelt-italic").unwrap().center();
        cx.simulate_click(italic, gpui::Modifiers::default());
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), "**_Hello_** world");
            assert!(e.format_active(Format::Bold));
            assert!(e.format_active(Format::Italic));
        });
        let dismissed_button = cx.debug_bounds("toolbelt-bold").unwrap().center();
        cx.simulate_keystrokes("escape");
        assert!(editor.read_with(cx, |e, _| e.toolbelt_bounds.is_none()));
        editor.read_with(cx, |e, _| assert!(!e.document.selection().is_empty()));
        cx.simulate_click(dismissed_button, gpui::Modifiers::default());
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), "**_Hello_** world");
            assert!(e.document.selection().is_empty());
        });
        cx.simulate_keystrokes("home shift-right");
        assert!(editor.read_with(cx, |e, _| e.toolbelt_bounds.is_some()));
    }

    #[gpui::test]
    fn toolbelt_toggles_existing_inline_styles_and_preserves_whitespace(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "");
        for (source, button, expected) in [
            ("**word**", "toolbelt-bold", "word"),
            ("__word__", "toolbelt-bold", "word"),
            ("*word*", "toolbelt-italic", "word"),
            ("_word_", "toolbelt-italic", "word"),
            ("`word`", "toolbelt-code", "word"),
            ("**_word_**", "toolbelt-italic", "**word**"),
            ("_**word**_", "toolbelt-bold", "_word_"),
        ] {
            editor.update_in(cx, |e, _, cx| e.set_markdown(source, cx));
            cx.dispatch_action(SelectAll);
            let control = cx.debug_bounds(button).unwrap().center();
            cx.simulate_click(control, gpui::Modifiers::default());
            editor.read_with(cx, |e, _| {
                assert_eq!(e.markdown(), expected, "{source}");
                assert_eq!(&e.markdown()[e.document.selection()], "word");
            });
            cx.dispatch_action(Undo);
            editor.read_with(cx, |e, _| assert_eq!(e.markdown(), source));
        }
        editor.update_in(cx, |e, _, cx| e.set_markdown("word ", cx));
        cx.dispatch_action(SelectAll);
        let bold = cx.debug_bounds("toolbelt-bold").unwrap().center();
        cx.simulate_click(bold, gpui::Modifiers::default());
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), "**word** ");
            assert_eq!(e.projection.lines[0].text, "word ");
            assert!(e.format_active(Format::Bold));
        });
    }

    #[gpui::test]
    fn toolbelt_can_remove_a_style_from_part_of_a_unicode_span(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "**aé🦀z**");
        editor.update_in(cx, |e, _, cx| {
            e.document.select(3, 9);
            cx.notify();
        });
        cx.run_until_parked();
        let bold = cx.debug_bounds("toolbelt-bold").unwrap().center();
        cx.simulate_click(bold, gpui::Modifiers::default());
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), "**a**é🦀**z**");
            assert_eq!(&e.markdown()[e.document.selection()], "é🦀");
            assert_eq!(e.projection.lines[0].text, "aé🦀z");
            assert!(!e.format_active(Format::Bold));
        });
        cx.dispatch_action(Undo);
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "**aé🦀z**"));
    }

    #[gpui::test]
    fn toolbelt_copies_code_without_exposing_inline_format_actions(cx: &mut TestAppContext) {
        let source = "```\ncode\n```\nAfter";
        let (editor, cx) = setup(cx, source);
        cx.dispatch_action(SelectAll);
        assert!(cx.debug_bounds("toolbelt-bold").is_none());
        let copy = cx.debug_bounds("toolbelt-copy").unwrap().center();
        cx.simulate_click(copy, gpui::Modifiers::default());
        editor.read_with(cx, |e, cx| {
            assert_eq!(e.markdown(), source);
            assert_eq!(e.document.selection(), 0..source.len());
            assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), source);
        });
        cx.simulate_keystrokes("right");
        assert!(editor.read_with(cx, |e, _| e.toolbelt_bounds.is_none()));
    }

    #[gpui::test]
    fn toolbelt_repositions_for_wrapping_and_scroll_and_hides_offscreen(cx: &mut TestAppContext) {
        let source = "wrapped selection ".repeat(100);
        let (editor, cx) = setup(cx, &source);
        cx.simulate_resize(size(px(360.), px(260.)));
        editor.update_in(cx, |e, _, cx| {
            e.document.select(0, source.len());
            e.reveal_cursor = false;
            cx.notify();
        });
        cx.run_until_parked();
        let before = cx.debug_bounds("selection-toolbelt").unwrap();
        assert!(before.left() >= px(8.) && before.right() <= px(352.));
        assert!(before.top() >= px(56.) && before.bottom() <= px(252.));
        editor.update_in(cx, |e, _, cx| {
            e.scroll = px(200.);
            e.reveal_cursor = false;
            cx.notify();
        });
        cx.run_until_parked();
        let after = cx.debug_bounds("selection-toolbelt").unwrap();
        assert_ne!(before.top(), after.top());
        assert!(after.top() >= px(56.) && after.bottom() <= px(252.));
        editor.update_in(cx, |e, _, cx| {
            e.document.select(0, 4);
            e.reveal_cursor = false;
            cx.notify();
        });
        cx.run_until_parked();
        assert!(editor.read_with(cx, |e, _| e.toolbelt_bounds.is_none()));
    }

    #[gpui::test]
    fn toolbelt_hides_when_focus_leaves_and_when_typing_replaces_selection(
        cx: &mut TestAppContext,
    ) {
        let (editor, cx) = setup(cx, "Selected");
        cx.dispatch_action(SelectAll);
        assert!(cx.debug_bounds("selection-toolbelt").is_some());
        let other = cx.update(|_, cx| cx.focus_handle());
        cx.update(|window, _| window.focus(&other));
        cx.run_until_parked();
        assert!(editor.read_with(cx, |e, _| e.toolbelt_bounds.is_none()));
        editor.update_in(cx, |e, window, _| window.focus(&e.focus));
        cx.run_until_parked();
        assert!(cx.debug_bounds("selection-toolbelt").is_some());
        cx.simulate_input("Replaced");
        assert!(editor.read_with(cx, |e, _| e.toolbelt_bounds.is_none()));
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "Replaced"));
    }
    #[gpui::test]
    fn deleting_the_last_struck_character_removes_its_markers_and_undo_restores_it(
        cx: &mut TestAppContext,
    ) {
        let (editor, cx) = setup(cx, "");
        for marker in ["~", "~~"] {
            for body in ["a", "é", "👩🏽‍💻"] {
                let source = format!("{marker}{body}{marker}");
                editor.update_in(cx, |e, _, cx| e.set_markdown(&source, cx));
                cx.run_until_parked();
                assert_eq!(
                    editor.read_with(cx, |e, _| e.projection.lines[0].text.clone()),
                    body
                );
                cx.simulate_keystrokes("end backspace");
                assert_eq!(
                    editor.read_with(cx, |e, _| e.markdown().to_string()),
                    "",
                    "{source}"
                );
                cx.dispatch_action(Undo);
                editor.read_with(cx, |e, _| {
                    assert_eq!(e.markdown(), source);
                    assert_eq!(e.projection.lines[0].text, body);
                });
            }
        }
    }
    #[gpui::test]
    fn typing_bold_waits_for_both_closing_asterisks_and_survives_undo(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "");
        for ch in "**asd*".chars() {
            cx.simulate_input(&ch.to_string());
            editor.read_with(cx, |e, _| {
                assert_eq!(e.projection.lines[0].text, e.markdown());
                assert!(
                    e.projection.lines[0]
                        .spans
                        .iter()
                        .all(|span| !span.face.italic && !span.face.bold)
                );
                assert!(!e.document.typing_style().italic);
                assert_eq!(e.document.cursor(), e.markdown().len());
            });
        }
        cx.simulate_input("*");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), "**asd**");
            assert_eq!(e.projection.lines[0].text, "asd");
            assert!(
                e.projection.lines[0]
                    .spans
                    .iter()
                    .all(|span| span.face.bold && !span.face.italic)
            );
        });
        cx.dispatch_action(Undo);
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), r"\*\*asd\*\*");
            assert_eq!(e.projection.lines[0].text, "**asd**");
            assert!(
                e.projection.lines[0]
                    .spans
                    .iter()
                    .all(|span| !span.face.bold && !span.face.italic)
            );
        });
        cx.dispatch_action(Redo);
        assert_eq!(
            editor.read_with(cx, |e, _| e.projection.lines[0].text.clone()),
            "asd"
        );
        cx.dispatch_action(Undo);
        cx.dispatch_action(Undo);
        cx.simulate_keystrokes("backspace");
        assert_eq!(
            editor.read_with(cx, |e, _| e.projection.lines[0].text.clone()),
            "**asd"
        );
        cx.simulate_input("*");
        editor.update_in(cx, |e, _, cx| {
            e.move_to(3, false, cx);
        });
        assert!(!editor.read_with(cx, |e, _| e.format_active(Format::Italic)));
        editor.update_in(cx, |e, _, cx| {
            e.document.select(2, 5);
            cx.notify();
        });
        cx.simulate_keystrokes("backspace");
        assert_eq!(editor.read_with(cx, |e, _| e.markdown().to_string()), "***");
    }
    #[gpui::test]
    fn rejected_inline_markup_can_be_selected_copied_deleted_and_edited(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "");
        for ch in "**é🦀**".chars() {
            cx.simulate_input(&ch.to_string());
        }
        cx.dispatch_action(Undo);
        editor.read_with(cx, |e, _| {
            assert_eq!(e.projection.lines[0].text, "**é🦀**");
            assert!(!e.format_active(Format::Bold));
            assert_eq!(e.document.cursor(), e.markdown().len());
        });
        cx.simulate_keystrokes("home shift-right");
        assert_eq!(editor.read_with(cx, |e, _| e.document.selection()), 0..2);
        cx.dispatch_action(Copy);
        assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), r"\*");
        cx.simulate_keystrokes("backspace");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.projection.lines[0].text, "*é🦀**");
            assert!(
                !e.projection.lines[0]
                    .spans
                    .iter()
                    .any(|span| span.face.bold || span.face.italic)
            );
        });
        cx.dispatch_action(Undo);
        cx.simulate_keystrokes("end backspace");
        assert_eq!(
            editor.read_with(cx, |e, _| e.projection.lines[0].text.clone()),
            "**é🦀*"
        );
        cx.dispatch_action(Undo);
        cx.simulate_keystrokes("end");
        cx.simulate_input(" more");
        let saved = editor.read_with(cx, |e, _| e.markdown().to_string());
        editor.update_in(cx, |e, _, cx| e.set_markdown(saved, cx));
        assert_eq!(
            editor.read_with(cx, |e, _| e.projection.lines[0].text.clone()),
            "**é🦀** more"
        );
    }

    #[gpui::test]
    fn committing_a_composed_closing_marker_allows_undoing_the_styling(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "**a*");
        cx.simulate_keystrokes("end");
        editor.update_in(cx, |e, window, cx| {
            e.replace_and_mark_text_in_range(None, "*", Some(1..1), window, cx);
            e.replace_text_in_range(None, "*", window, cx);
        });
        assert_eq!(
            editor.read_with(cx, |e, _| e.projection.lines[0].text.clone()),
            "a"
        );
        cx.dispatch_action(Undo);
        assert_eq!(
            editor.read_with(cx, |e, _| e.projection.lines[0].text.clone()),
            "**a**"
        );
        cx.dispatch_action(Undo);
        assert_eq!(
            editor.read_with(cx, |e, _| e.markdown().to_string()),
            "**a*"
        );
        cx.dispatch_action(Redo);
        cx.dispatch_action(Redo);
        assert_eq!(
            editor.read_with(cx, |e, _| e.projection.lines[0].text.clone()),
            "a"
        );
    }
}
