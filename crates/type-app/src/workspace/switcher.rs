use super::{TabId, Workspace};
use gpui::{prelude::*, *};
use markdown_core::{MarkdownPreview, appearance};

const CARD_WIDTH: f32 = 168.;
const CARD_HEIGHT: f32 = 152.;
const GAP: f32 = 12.;
const PADDING: f32 = 16.;
const FRAME_HEIGHT: f32 = 158.;

struct Item {
    id: TabId,
    preview: MarkdownPreview,
}

pub(super) struct TabSwitcher {
    items: Vec<Item>,
    selected: usize,
    scroll: ScrollHandle,
    revealed: Option<(Size<Pixels>, usize)>,
}

struct Metrics {
    columns: usize,
    width: Pixels,
    height: Pixels,
    rows_height: Pixels,
}

impl TabSwitcher {
    fn metrics(&self, window: &Window) -> Metrics {
        let viewport = window.viewport_size();
        let columns =
            (((f32::from(viewport.width) - 50. - PADDING * 2. + GAP) / (CARD_WIDTH + GAP)).floor()
                as usize)
                .clamp(1, 5)
                .min(self.items.len());
        let rows = self.items.len().div_ceil(columns);
        let content_height = px(rows as f32 * (CARD_HEIGHT + GAP) - GAP);
        let rows_height =
            content_height.min((viewport.height - px(48. + FRAME_HEIGHT)).max(px(CARD_HEIGHT)));
        Metrics {
            columns,
            width: px(columns as f32 * (CARD_WIDTH + GAP) - GAP + PADDING * 2. + 2.),
            height: rows_height + px(FRAME_HEIGHT),
            rows_height,
        }
    }

    fn reveal_selection(&mut self, window: &Window) {
        let key = (window.viewport_size(), self.selected);
        if self.revealed == Some(key) {
            return;
        }
        self.revealed = Some(key);
        let Metrics {
            columns,
            rows_height,
            ..
        } = self.metrics(window);
        let rows = self.items.len().div_ceil(columns);
        let content_height = px(rows as f32 * (CARD_HEIGHT + GAP) - GAP);
        let top = px((self.selected / columns) as f32 * (CARD_HEIGHT + GAP));
        let mut offset = self
            .scroll
            .offset()
            .y
            .clamp((rows_height - content_height).min(px(0.)), px(0.));
        if top + offset < px(0.) {
            offset = -top;
        }
        if top + px(CARD_HEIGHT) + offset > rows_height {
            offset = rows_height - top - px(CARD_HEIGHT);
        }
        self.scroll.set_offset(point(px(0.), offset));
    }
}

fn byte_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024. * 1024.))
    }
}

impl Workspace {
    pub(super) fn step_switcher(
        &mut self,
        backwards: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.len() < 2 || self.dialog_busy || self.closing.is_some() {
            return;
        }
        if self.switcher.is_none() {
            self.active_tab()
                .editor
                .update(cx, |editor, cx| editor.deactivate(window, cx));
            self.switcher = Some(TabSwitcher {
                items: self
                    .tabs
                    .iter()
                    .map(|tab| Item {
                        id: tab.id,
                        preview: tab.editor.read(cx).preview(),
                    })
                    .collect(),
                selected: self
                    .tabs
                    .iter()
                    .position(|tab| tab.id == self.active)
                    .unwrap(),
                scroll: ScrollHandle::new(),
                revealed: None,
            });
            window.focus(&self.focus);
        }
        self.move_switcher(if backwards { -1 } else { 1 }, cx);
    }

    fn move_switcher(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(switcher) = &mut self.switcher {
            switcher.selected = (switcher.selected as isize + delta)
                .rem_euclid(switcher.items.len() as isize) as usize;
            cx.notify();
        }
    }

    pub(super) fn commit_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(switcher) = self.switcher.take() {
            self.activate(switcher.items[switcher.selected].id, window, cx);
        }
    }

    pub(super) fn cancel_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.switcher.take().is_some() {
            self.focus_editor(window, cx);
            cx.notify();
        }
    }

    pub(super) fn switcher_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(switcher) = &self.switcher else {
            return;
        };
        let columns = switcher.metrics(window).columns as isize;
        match event.keystroke.key.as_str() {
            "escape" => self.cancel_switcher(window, cx),
            "enter" => self.commit_switcher(window, cx),
            "tab" => self.move_switcher(
                if event.keystroke.modifiers.shift {
                    -1
                } else {
                    1
                },
                cx,
            ),
            "left" => self.move_switcher(-1, cx),
            "right" => self.move_switcher(1, cx),
            "up" => self.move_switcher(-columns, cx),
            "down" => self.move_switcher(columns, cx),
            _ => {}
        }
        window.prevent_default();
        cx.stop_propagation();
    }

    pub(super) fn render_switcher(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.switcher.as_mut()?.reveal_selection(window);
        let switcher = self.switcher.as_ref()?;
        let metrics = switcher.metrics(window);
        let selected_id = switcher.items[switcher.selected].id;
        let selected = self.tabs.iter().find(|tab| tab.id == selected_id)?;
        let editor = selected.editor.read(cx);
        let words = editor.word_count();
        let status = if selected.saving_to.is_some() {
            "Saving…"
        } else if selected.dirty(cx) {
            "Unsaved changes"
        } else if selected.path.is_some() {
            "Saved"
        } else {
            "Not saved"
        };
        let location = selected
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "Unsaved document".into());
        let detail = format!(
            "{words} {} · {} · {status}",
            if words == 1 { "word" } else { "words" },
            byte_size(editor.markdown().len())
        );
        let rows = switcher.items.chunks(metrics.columns).map(|items| {
            div()
                .flex()
                .gap(px(GAP))
                .h(px(CARD_HEIGHT))
                .flex_shrink_0()
                .children(items.iter().filter_map(|item| {
                    let tab = self.tabs.iter().find(|tab| tab.id == item.id)?;
                    let id = item.id;
                    let selected = selected_id == id;
                    Some(
                        div()
                            .id(("switcher-card", id))
                            .debug_selector(move || format!("switcher-card-{id}"))
                            .w(px(CARD_WIDTH))
                            .h(px(CARD_HEIGHT))
                            .flex_shrink_0()
                            .p(px(8.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .rounded(px(10.))
                            .border_2()
                            .border_color(if selected {
                                rgb(0x718dab).into()
                            } else {
                                transparent_black()
                            })
                            .bg(rgb(if selected {
                                0xf6f8fb
                            } else {
                                appearance::CHROME
                            }))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(appearance::BACKGROUND)))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.activate(id, window, cx);
                            }))
                            .child(
                                div()
                                    .relative()
                                    .w_full()
                                    .h(px(104.))
                                    .flex_shrink_0()
                                    .overflow_hidden()
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(rgb(appearance::CHROME_BORDER))
                                    .bg(rgb(appearance::BACKGROUND))
                                    .child(item.preview.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .w_full()
                                    .min_w_0()
                                    .h(px(20.))
                                    .font_weight(if selected {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .truncate()
                                            .child(self.tab_label(tab)),
                                    )
                                    .when(tab.dirty(cx), |view| {
                                        view.child(
                                            div()
                                                .size(px(5.))
                                                .rounded_full()
                                                .flex_shrink_0()
                                                .bg(rgb(appearance::MUTED)),
                                        )
                                    }),
                            ),
                    )
                }))
        });
        let panel = div()
            .id("tab-switcher")
            .debug_selector(|| "tab-switcher".into())
            .w(metrics.width)
            .h(metrics.height)
            .flex()
            .flex_col()
            .gap(px(GAP))
            .p(px(PADDING))
            .rounded(px(16.))
            .bg(rgb(appearance::CHROME))
            .border_1()
            .border_color(rgb(appearance::CHROME_BORDER))
            .shadow(vec![BoxShadow {
                color: rgba(0x00000026).into(),
                offset: point(px(0.), px(12.)),
                blur_radius: px(40.),
                spread_radius: px(0.),
            }])
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .h(px(28.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Open documents"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(appearance::MUTED))
                            .child(format!(
                                "{} of {}",
                                switcher.selected + 1,
                                switcher.items.len()
                            )),
                    ),
            )
            .child(
                div()
                    .id("switcher-grid")
                    .debug_selector(|| "switcher-grid".into())
                    .w_full()
                    .h(metrics.rows_height)
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap(px(GAP))
                    .overflow_y_scroll()
                    .track_scroll(&switcher.scroll)
                    .children(rows),
            )
            .child(
                div()
                    .h(px(72.))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap(px(3.))
                    .text_size(px(12.))
                    .child(
                        div()
                            .truncate()
                            .text_color(rgb(appearance::TEXT))
                            .child(location),
                    )
                    .child(div().text_color(rgb(appearance::MUTED)).child(detail))
                    .child(
                        div()
                            .mt(px(5.))
                            .text_size(px(11.))
                            .text_color(rgb(appearance::MUTED))
                            .child("Release Control to open · ⇧Tab to go back · Esc to cancel"),
                    ),
            );
        Some(
            div()
                .id("tab-switcher-overlay")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(0x11111124))
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        this.cancel_switcher(window, cx);
                    }),
                )
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .child(panel)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests;
