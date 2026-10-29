mod input;

use super::{EditorEvent, LaidLine, MarkdownEditor, Projection, appearance};
use gpui::{prelude::*, *};
use regex::{Regex, RegexBuilder};
use std::ops::Range;

pub(super) const HEIGHT: f32 = 52.;

pub(super) struct Match {
    pub line: usize,
    pub range: Range<usize>,
    pub source: Range<usize>,
}

pub(super) struct State {
    pub open: bool,
    pub matches: Vec<Match>,
    pub active: Option<usize>,
    pub reveal: bool,
    query: String,
    case_sensitive: bool,
    pattern: Option<Regex>,
    input: Entity<input::Input>,
    subscription: Option<Subscription>,
    pub(super) focus: FocusHandle,
}

impl State {
    pub fn new(cx: &mut Context<MarkdownEditor>) -> Self {
        let input = cx.new(|cx| input::Input::new("find-input", "Find in document", cx));
        let focus = input.read(cx).focus.clone();
        Self {
            open: false,
            matches: Vec::new(),
            active: None,
            reveal: false,
            query: String::new(),
            case_sensitive: false,
            pattern: None,
            input,
            subscription: None,
            focus,
        }
    }

    fn compile(&mut self) {
        self.pattern = (!self.query.is_empty())
            .then(|| {
                RegexBuilder::new(&regex::escape(&self.query))
                    .case_insensitive(!self.case_sensitive)
                    .build()
                    .ok()
            })
            .flatten();
    }

    pub fn refresh(&mut self, projection: &Projection, fallback: usize) {
        let anchor = self
            .active
            .and_then(|i| self.matches.get(i))
            .map_or(fallback, |found| found.source.start);
        self.matches.clear();
        self.active = None;
        if !self.open {
            return;
        }
        if let Some(pattern) = &self.pattern {
            for (index, line) in projection.lines.iter().enumerate() {
                self.matches
                    .extend(pattern.find_iter(&line.text).map(|found| Match {
                        line: index,
                        range: found.range(),
                        source: line.source_index(found.start())..line.source_index(found.end()),
                    }));
            }
        }
        if !self.matches.is_empty() {
            self.active = Some(
                self.matches
                    .iter()
                    .position(|found| found.source.start >= anchor)
                    .unwrap_or(0),
            );
        }
    }
}

pub(super) fn subscribe_input(
    editor: &mut MarkdownEditor,
    window: &Window,
    cx: &mut Context<MarkdownEditor>,
) {
    if editor.find.subscription.is_none() {
        editor.find.subscription = Some(cx.subscribe_in(
            &editor.find.input,
            window,
            |editor, _, event: &crate::TextInputEvent, window, cx| match event {
                crate::TextInputEvent::Changed(query) => query_changed(editor, query.clone(), cx),
                crate::TextInputEvent::Submit { backwards }
                | crate::TextInputEvent::Step { backwards } => step(editor, *backwards, window, cx),
                crate::TextInputEvent::Cancel => close(editor, window, cx),
                crate::TextInputEvent::Blur => {}
            },
        ));
    }
}

pub(super) fn open(
    editor: &mut MarkdownEditor,
    window: &mut Window,
    cx: &mut Context<MarkdownEditor>,
) {
    editor.finish_composition(window, cx);
    if !editor.find.open {
        let selection = editor.document.selection();
        if !selection.is_empty() {
            let line = &editor.projection.lines[editor.projection.line_for_source(selection.start)];
            if selection.end <= line.source.end {
                editor.find.query = line.text
                    [line.display_index(selection.start)..line.display_index(selection.end)]
                    .to_string();
            }
        }
        editor.find.open = true;
        editor.find.compile();
        editor
            .find
            .refresh(&editor.projection, editor.document.cursor());
        editor.layout_dirty = true;
        cx.emit(EditorEvent::FindVisibilityChanged);
    }
    editor.find.input.update(cx, |input, cx| {
        input.set_text(&editor.find.query, 0..editor.find.query.len(), cx)
    });
    super::selection::stop(editor);
    editor.toolbelt_dismissed = true;
    editor.reveal_cursor = false;
    editor.find.reveal = editor.find.active.is_some();
    window.focus(&editor.find.focus);
    cx.notify();
}

fn query_changed(editor: &mut MarkdownEditor, query: String, cx: &mut Context<MarkdownEditor>) {
    editor.find.query = query;
    editor.find.active = None;
    editor.find.compile();
    editor
        .find
        .refresh(&editor.projection, editor.document.cursor());
    editor.find.reveal = editor.find.active.is_some();
    editor.reveal_cursor = false;
    cx.notify();
}

pub(super) fn step(
    editor: &mut MarkdownEditor,
    backwards: bool,
    window: &mut Window,
    cx: &mut Context<MarkdownEditor>,
) {
    if !editor.find.open {
        open(editor, window, cx);
        return;
    }
    let len = editor.find.matches.len();
    if len > 0 {
        editor.find.active =
            Some((editor.find.active.unwrap_or(0) + if backwards { len - 1 } else { 1 }) % len);
        editor.find.reveal = true;
        editor.reveal_cursor = false;
    }
    window.focus(&editor.find.focus);
    cx.notify();
}

pub(super) fn hide(editor: &mut MarkdownEditor, cx: &mut Context<MarkdownEditor>) {
    if editor.find.open {
        editor.find.open = false;
        editor.find.reveal = false;
        editor.find.matches.clear();
        editor.find.active = None;
        editor.layout_dirty = true;
        cx.emit(EditorEvent::FindVisibilityChanged);
        cx.notify();
    }
}

pub(super) fn close(
    editor: &mut MarkdownEditor,
    window: &mut Window,
    cx: &mut Context<MarkdownEditor>,
) {
    if let Some(found) = editor.find.active.and_then(|i| editor.find.matches.get(i)) {
        editor.document.select(found.source.start, found.source.end);
        editor.reveal_cursor = true;
        cx.emit(EditorEvent::SelectionChanged);
    }
    hide(editor, cx);
    editor.toolbelt_dismissed = true;
    window.focus(&editor.focus);
}

pub(super) fn reveal(editor: &mut MarkdownEditor, layout: &[LaidLine], bounds: Bounds<Pixels>) {
    if !std::mem::take(&mut editor.find.reveal) {
        return;
    }
    let Some(found) = editor.find.active.and_then(|i| editor.find.matches.get(i)) else {
        return;
    };
    let Some(line) = layout.get(found.line) else {
        return;
    };
    let start = line.position(found.range.start).y;
    let end = line.position(found.range.end).y + line.line_height;
    let top = bounds.top() + px(editor.top_gutter() + 8.);
    let bottom = bounds.bottom() - px(20.);
    if start - editor.scroll < top || end - editor.scroll > bottom {
        editor.scroll = (start - top - (bottom - top - (end - start)).max(px(0.)) / 2.)
            .clamp(px(0.), editor.max_scroll);
    }
}

fn button(id: &'static str, label: &'static str, selected: bool, enabled: bool) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .size(px(28.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .text_size(px(13.))
        .text_color(rgb(appearance::TEXT))
        .opacity(if enabled { 1. } else { 0.35 })
        .when(selected, |view| view.bg(rgb(appearance::SELECTION)))
        .when(enabled, |view| {
            view.cursor_pointer()
                .hover(|view| view.bg(rgb(appearance::CHROME_HOVER)))
        })
        .child(label)
}

pub(super) fn render(
    editor: &MarkdownEditor,
    cx: &mut Context<MarkdownEditor>,
) -> Option<AnyElement> {
    if !editor.find.open {
        return None;
    }
    let has_matches = !editor.find.matches.is_empty();
    let count = if editor.find.query.is_empty() {
        String::new()
    } else if !has_matches {
        "No results".into()
    } else {
        format!(
            "{} / {}",
            editor.find.active.unwrap_or(0) + 1,
            editor.find.matches.len()
        )
    };
    Some(
        div()
            .id("document-find")
            .debug_selector(|| "document-find".into())
            .absolute()
            .top(px(appearance::TOP_GUTTER + 8.))
            .right(px(20.))
            .w(px(420.))
            .max_w((editor.bounds.size.width - px(40.)).max(px(0.)))
            .h(px(44.))
            .flex()
            .items_center()
            .gap(px(4.))
            .p(px(6.))
            .rounded(px(appearance::CHROME_RADIUS))
            .border_1()
            .border_color(rgb(appearance::CHROME_BORDER))
            .bg(rgb(appearance::CHROME))
            .font_family(appearance::FONT_FAMILY)
            .text_size(px(12.))
            .cursor(CursorStyle::Arrow)
            .occlude()
            .shadow(vec![BoxShadow {
                color: rgba(0x00000018).into(),
                offset: point(px(0.), px(4.)),
                blur_radius: px(16.),
                spread_radius: px(0.),
            }])
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|editor, _, _, cx| {
                    super::selection::stop(editor);
                    cx.stop_propagation();
                }),
            )
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(30.))
                    .child(editor.find.input.clone()),
            )
            .child(
                div()
                    .w(px(64.))
                    .flex_shrink_0()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(rgb(if !has_matches && !editor.find.query.is_empty() {
                        0x9b483b
                    } else {
                        appearance::MUTED
                    }))
                    .child(count),
            )
            .child(
                button("find-case", "Aa", editor.find.case_sensitive, true).on_click(cx.listener(
                    |editor, _, window, cx| {
                        editor.find.case_sensitive = !editor.find.case_sensitive;
                        editor.find.compile();
                        editor
                            .find
                            .refresh(&editor.projection, editor.document.cursor());
                        editor.find.reveal = true;
                        editor.reveal_cursor = false;
                        window.focus(&editor.find.focus);
                        cx.notify();
                    },
                )),
            )
            .child(
                button("find-previous", "↑", false, has_matches)
                    .on_click(cx.listener(|editor, _, window, cx| step(editor, true, window, cx))),
            )
            .child(
                button("find-next", "↓", false, has_matches)
                    .on_click(cx.listener(|editor, _, window, cx| step(editor, false, window, cx))),
            )
            .child(
                button("find-close", "×", false, true)
                    .on_click(cx.listener(|editor, _, window, cx| close(editor, window, cx))),
            )
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests;
