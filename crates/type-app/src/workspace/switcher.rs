use super::{TabId, Workspace, motion::Motion};
use gpui::{prelude::*, *};
use markdown_core::{MarkdownPreview, appearance};
use std::time::{Duration, Instant};

const CARD_WIDTH: f32 = 168.;
const CARD_HEIGHT: f32 = 152.;
const GAP: f32 = 12.;
const PADDING: f32 = 16.;
const FRAME_HEIGHT: f32 = PADDING * 2. + 2.;
const OPEN: Duration = Duration::from_millis(140);
const CLOSE: Duration = Duration::from_millis(90);
const HIGHLIGHT: Duration = Duration::from_millis(90);

struct Item {
    id: TabId,
    title: SharedString,
    preview: MarkdownPreview,
    highlight: Motion,
}

pub(super) struct TabSwitcher {
    items: Vec<Item>,
    selected: usize,
    scroll: ScrollHandle,
    revealed: Option<(Size<Pixels>, usize)>,
    visibility: Motion,
}

struct Metrics {
    columns: usize,
    width: Pixels,
    height: Pixels,
    rows_height: Pixels,
}

impl TabSwitcher {
    fn select(&mut self, index: usize, now: Instant) {
        self.selected = index;
        for (i, item) in self.items.iter_mut().enumerate() {
            item.highlight = Motion::new(
                item.highlight.value(now),
                if i == index { 1. } else { 0. },
                now,
                HIGHLIGHT,
            );
        }
    }

    fn moving(&self, now: Instant) -> bool {
        self.visibility.moving(now) || self.items.iter().any(|item| item.highlight.moving(now))
    }

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
        self.finish_tab_drag(true, window, cx);
        self.cancel_rename(false, window, cx);
        if self.switcher.is_none() {
            let now = cx.background_executor().now();
            let visibility = self
                .fading_switcher
                .take()
                .map_or(0., |switcher| switcher.visibility.value(now));
            self.active_tab()
                .editor
                .update(cx, |editor, cx| editor.deactivate(window, cx));
            self.switcher = Some(TabSwitcher {
                items: self
                    .tabs
                    .iter()
                    .map(|tab| Item {
                        id: tab.id,
                        title: tab.name().into(),
                        preview: tab.editor.read(cx).preview(),
                        highlight: Motion::new(0., 0., now, HIGHLIGHT),
                    })
                    .collect(),
                selected: self
                    .tabs
                    .iter()
                    .position(|tab| tab.id == self.active)
                    .unwrap(),
                scroll: ScrollHandle::new(),
                revealed: None,
                visibility: Motion::new(visibility, 1., now, OPEN),
            });
            window.focus(&self.focus);
        }
        self.move_switcher(if backwards { -1 } else { 1 }, cx);
    }

    fn move_switcher(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(switcher) = &mut self.switcher {
            let index = (switcher.selected as isize + delta)
                .rem_euclid(switcher.items.len() as isize) as usize;
            switcher.select(index, cx.background_executor().now());
            cx.notify();
        }
    }

    pub(super) fn commit_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(switcher) = &self.switcher {
            self.activate(switcher.items[switcher.selected].id, window, cx);
        }
    }

    pub(super) fn cancel_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.switcher.is_some() {
            self.dismiss_switcher(cx);
            self.focus_editor(window, cx);
            cx.notify();
        }
    }

    pub(super) fn dismiss_switcher(&mut self, cx: &mut Context<Self>) {
        if let Some(mut switcher) = self.switcher.take() {
            let now = cx.background_executor().now();
            switcher.visibility = Motion::new(switcher.visibility.value(now), 0., now, CLOSE);
            self.fading_switcher = Some(switcher);
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
        let now = cx.background_executor().now();
        if self
            .fading_switcher
            .as_ref()
            .is_some_and(|s| !s.visibility.moving(now))
        {
            self.fading_switcher = None;
        }
        let interactive = self.switcher.is_some();
        let switcher = self.switcher.as_mut().or(self.fading_switcher.as_mut())?;
        switcher.reveal_selection(window);
        if switcher.moving(now) {
            window.request_animation_frame();
        }
        let visibility = switcher.visibility.value(now);
        let metrics = switcher.metrics(window);
        let rows = switcher
            .items
            .chunks(metrics.columns)
            .enumerate()
            .map(|(row, items)| {
                div()
                    .flex()
                    .gap(px(GAP))
                    .h(px(CARD_HEIGHT))
                    .flex_shrink_0()
                    .children(items.iter().enumerate().map(|(column, item)| {
                        let id = item.id;
                        let index = row * metrics.columns + column;
                        let selected = switcher.selected == index;
                        let highlight = item.highlight.value(now);
                        let accent: Hsla = rgb(0x718dab).into();
                        let tint: Hsla = rgb(0xf6f8fb).into();
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
                            .border_color(accent.opacity(highlight))
                            .bg(tint.opacity(highlight))
                            .when(interactive, |view| {
                                view.cursor_pointer()
                                    .when(!selected, |view| {
                                        view.hover(|style| style.bg(rgb(appearance::BACKGROUND)))
                                    })
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        if let Some(switcher) = &mut this.switcher {
                                            switcher.select(index, cx.background_executor().now());
                                        }
                                        this.commit_switcher(window, cx);
                                    }))
                            })
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
                                    .w_full()
                                    .min_w_0()
                                    .h(px(20.))
                                    .text_center()
                                    .truncate()
                                    .child(item.title.clone()),
                            )
                    }))
            });
        let panel = div()
            .id("tab-switcher")
            .debug_selector(|| "tab-switcher".into())
            .relative()
            .top(px((1. - visibility) * 6.))
            .w(metrics.width)
            .h(metrics.height)
            .flex()
            .flex_col()
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
            .when(interactive, |view| {
                view.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            })
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
                    .overflow_hidden()
                    .when(interactive, |view| view.overflow_y_scroll())
                    .track_scroll(&switcher.scroll)
                    .children(rows),
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
                .opacity(visibility)
                .when(interactive, |view| {
                    view.occlude()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.cancel_switcher(window, cx);
                            }),
                        )
                        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                })
                .child(panel)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests;
