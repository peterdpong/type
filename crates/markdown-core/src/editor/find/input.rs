use super::{MarkdownEditor, appearance};
use crate::{
    Document,
    document::{from_utf16, to_utf16},
    editor::*,
};
use gpui::*;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

pub(super) struct Input {
    pub focus: FocusHandle,
    parent: WeakEntity<MarkdownEditor>,
    document: Document,
    marked: Option<Range<usize>>,
    selecting: bool,
    line: Option<ShapedLine>,
    bounds: Bounds<Pixels>,
    scroll: Pixels,
    subscriptions: Vec<Subscription>,
}

impl Input {
    pub fn new(parent: WeakEntity<MarkdownEditor>, cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            parent,
            document: Document::new(""),
            marked: None,
            selecting: false,
            line: None,
            bounds: Bounds::default(),
            scroll: px(0.),
            subscriptions: Vec::new(),
        }
    }

    pub fn select_query(&mut self, query: &str, cx: &mut Context<Self>) {
        if query != self.document.text() {
            self.document = Document::new(query);
        }
        self.marked = None;
        self.document.select(0, query.len());
        cx.notify();
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        let query = self.document.text().to_owned();
        let _ = self
            .parent
            .update(cx, |editor, cx| super::query_changed(editor, query, cx));
        cx.notify();
    }

    fn native_range(&self, range: Range<usize>) -> Range<usize> {
        from_utf16(self.document.text(), range.start)..from_utf16(self.document.text(), range.end)
    }

    fn hit_test(&self, position: Point<Pixels>) -> usize {
        self.line
            .as_ref()
            .map_or(0, |line| {
                line.closest_index_for_x(position.x - self.bounds.left() - px(1.) + self.scroll)
            })
            .min(self.document.text().len())
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.marked.is_some() {
            return;
        }
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        let selection = self.document.selection();
        let cursor = self.document.cursor();
        let command = if cfg!(target_os = "macos") {
            modifiers.platform
        } else {
            modifiers.control
        };
        let word = modifiers.alt || (!cfg!(target_os = "macos") && modifiers.control);
        match key {
            "enter" | "up" | "down" => {
                let backwards = key == "up" || (key == "enter" && modifiers.shift);
                let _ = self
                    .parent
                    .update(cx, |editor, cx| super::step(editor, backwards, window, cx));
            }
            "escape" => {
                let _ = self
                    .parent
                    .update(cx, |editor, cx| super::close(editor, window, cx));
            }
            "left" | "right" | "home" | "end" => {
                let right = key == "right" || key == "end";
                let offset = if command || key == "home" || key == "end" {
                    if right { self.document.text().len() } else { 0 }
                } else if !modifiers.shift && !selection.is_empty() {
                    if right {
                        selection.end
                    } else {
                        selection.start
                    }
                } else if word {
                    if right {
                        self.document
                            .text()
                            .unicode_word_indices()
                            .map(|(i, word)| i + word.len())
                            .find(|i| *i > cursor)
                            .unwrap_or(self.document.text().len())
                    } else {
                        self.document
                            .text()
                            .unicode_word_indices()
                            .map(|(i, _)| i)
                            .take_while(|i| *i < cursor)
                            .last()
                            .unwrap_or(0)
                    }
                } else if right {
                    self.document.next_grapheme(cursor)
                } else {
                    self.document.previous_grapheme(cursor)
                };
                self.document.move_to(offset, modifiers.shift);
                cx.notify();
            }
            "backspace" | "delete" => {
                let range = if selection.is_empty() {
                    if key == "delete" {
                        cursor..self.document.next_grapheme(cursor)
                    } else {
                        self.document.previous_grapheme(cursor)..cursor
                    }
                } else {
                    selection
                };
                if !range.is_empty() {
                    self.document.replace(range, "");
                    self.changed(cx);
                }
            }
            "tab" => {}
            _ => return,
        }
        cx.stop_propagation();
        window.prevent_default();
    }

    fn copy(&self, cx: &mut Context<Self>) {
        let selection = self.document.selection();
        if !selection.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.document.text()[selection].to_owned(),
            ));
        }
    }
}

impl EntityInputHandler for Input {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.native_range(range);
        *actual = Some(
            to_utf16(self.document.text(), range.start)..to_utf16(self.document.text(), range.end),
        );
        Some(self.document.text()[range].to_owned())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let range = self.document.selection();
        Some(UTF16Selection {
            range: to_utf16(self.document.text(), range.start)
                ..to_utf16(self.document.text(), range.end),
            reversed: self.document.cursor() < self.document.anchor(),
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|range| {
            to_utf16(self.document.text(), range.start)..to_utf16(self.document.text(), range.end)
        })
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let checkpoint = self.marked.is_none();
        let range = range
            .map(|range| self.native_range(range))
            .or(self.marked.take())
            .unwrap_or(self.document.selection());
        self.document.replace_with_history(
            range,
            &text.replace(['\r', '\n', '\t'], " "),
            checkpoint,
        );
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
        let checkpoint = self.marked.is_none();
        let range = range
            .map(|range| self.native_range(range))
            .or(self.marked.take())
            .unwrap_or(self.document.selection());
        let start = range.start;
        let text = text.replace(['\r', '\n', '\t'], " ");
        self.document.replace_with_history(range, &text, checkpoint);
        self.marked = (!text.is_empty()).then_some(start..start + text.len());
        if let Some(selected) = selected {
            self.document.select(
                start + from_utf16(&text, selected.start),
                start + from_utf16(&text, selected.end),
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
        let line = self.line.as_ref()?;
        let range = self.native_range(range);
        Some(Bounds::new(
            point(
                self.bounds.left() + px(1.) + line.x_for_index(range.start) - self.scroll,
                self.bounds.top(),
            ),
            size(
                (line.x_for_index(range.end) - line.x_for_index(range.start)).max(px(1.)),
                self.bounds.size.height,
            ),
        ))
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

impl Render for Input {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.subscriptions.is_empty() {
            self.subscriptions
                .push(cx.on_focus(&self.focus, window, |_, _, cx| cx.notify()));
            self.subscriptions
                .push(cx.on_blur(&self.focus, window, |this, _, cx| {
                    this.selecting = false;
                    cx.notify();
                }));
        }
        let prepaint = cx.entity();
        let paint = cx.entity();
        let mut view = div()
            .id("find-input")
            .debug_selector(|| "find-input".into())
            .size_full()
            .px(px(7.))
            .rounded_md()
            .border_1()
            .border_color(rgb(if self.focus.is_focused(window) {
                0x718dab
            } else {
                appearance::CHROME_BORDER
            }))
            .bg(rgb(appearance::BACKGROUND))
            .cursor(CursorStyle::IBeam)
            .overflow_hidden()
            .key_context("FindInput")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| {
                this.document.select(0, this.document.text().len());
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Copy, _, cx| this.copy(cx)))
            .on_action(cx.listener(|this, _: &Cut, window, cx| {
                this.copy(cx);
                if !this.document.selection().is_empty() {
                    this.replace_text_in_range(None, "", window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &Paste, window, cx| {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    this.replace_text_in_range(None, &text, window, cx);
                }
            }))
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    window.focus(&this.focus);
                    this.selecting = true;
                    let index = this.hit_test(event.position);
                    if event.click_count >= 3 {
                        this.document.select(0, this.document.text().len());
                    } else if event.click_count == 2 {
                        if let Some((start, word)) = this
                            .document
                            .text()
                            .unicode_word_indices()
                            .find(|(i, word)| *i <= index && index <= i + word.len())
                        {
                            this.document.select(start, start + word.len());
                        }
                    } else {
                        this.document.move_to(index, event.modifiers.shift);
                    }
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.selecting {
                    this.document.move_to(this.hit_test(event.position), true);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.selecting = false),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.selecting = false),
            );
        macro_rules! block { ($($action:ty),*) => { $(view = view.on_action(|_: &$action, _, cx| cx.stop_propagation());)* }; }
        block!(
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
            Quote
        );
        view.child(
            canvas(
                move |bounds, window, cx| {
                    prepaint.update(cx, |input, _| {
                        let empty = input.document.text().is_empty();
                        let text: SharedString = if empty {
                            "Find in document".into()
                        } else {
                            input.document.text().to_owned().into()
                        };
                        let run = TextRun {
                            len: text.len(),
                            font: font(appearance::FONT_FAMILY),
                            color: rgb(if empty {
                                appearance::MUTED
                            } else {
                                appearance::TEXT
                            })
                            .into(),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let runs = if let Some(marked) = &input.marked {
                            vec![
                                TextRun {
                                    len: marked.start,
                                    ..run.clone()
                                },
                                TextRun {
                                    len: marked.len(),
                                    underline: Some(UnderlineStyle {
                                        color: None,
                                        thickness: px(1.),
                                        wavy: false,
                                    }),
                                    ..run.clone()
                                },
                                TextRun {
                                    len: text.len() - marked.end,
                                    ..run
                                },
                            ]
                            .into_iter()
                            .filter(|run| run.len > 0)
                            .collect()
                        } else {
                            vec![run]
                        };
                        let line = window.text_system().shape_line(
                            text,
                            px(appearance::UI_SIZE),
                            &runs,
                            None,
                        );
                        let x = line.x_for_index(input.document.cursor());
                        let available = (bounds.size.width - px(2.)).max(px(1.));
                        if x - input.scroll > available {
                            input.scroll = x - available;
                        }
                        if x - input.scroll < px(0.) {
                            input.scroll = x.max(px(0.));
                        }
                        input.scroll = input.scroll.min((line.width - available).max(px(0.)));
                        input.bounds = bounds;
                        input.line = Some(line);
                    })
                },
                move |bounds, _, window, cx| {
                    let input = paint.read(cx);
                    let focus = input.focus.clone();
                    let line = input.line.clone().unwrap();
                    let selection = input.document.selection();
                    let cursor = input.document.cursor();
                    let origin = bounds.origin + point(px(1.) - input.scroll, px(0.));
                    window.handle_input(
                        &focus,
                        ElementInputHandler::new(bounds, paint.clone()),
                        cx,
                    );
                    window.with_content_mask(Some(ContentMask { bounds }), |window| {
                        if focus.is_focused(window) && !selection.is_empty() {
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(
                                        origin.x + line.x_for_index(selection.start),
                                        bounds.top() + px(4.),
                                    ),
                                    size(
                                        line.x_for_index(selection.end)
                                            - line.x_for_index(selection.start),
                                        bounds.size.height - px(8.),
                                    ),
                                ),
                                rgb(appearance::SELECTION),
                            ));
                        }
                        let _ = line.paint(origin, bounds.size.height, window, cx);
                        if focus.is_focused(window) && selection.is_empty() {
                            let text = window.text_system();
                            let font = text.resolve_font(&font(appearance::FONT_FAMILY));
                            let cap = text.cap_height(font, px(appearance::UI_SIZE));
                            let descent = text.descent(font, px(appearance::UI_SIZE)).abs();
                            let baseline = text.baseline_offset(
                                font,
                                px(appearance::UI_SIZE),
                                bounds.size.height,
                            );
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(
                                        origin.x + line.x_for_index(cursor) - px(1.),
                                        origin.y + baseline - cap,
                                    ),
                                    size(px(1.), cap + descent),
                                ),
                                rgb(appearance::TEXT),
                            ));
                        }
                    });
                },
            )
            .size_full(),
        )
    }
}
