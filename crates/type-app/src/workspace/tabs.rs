mod drag;

use super::{CloseTarget, Tab, TabId, Workspace, motion::Motion};
use gpui::{prelude::*, *};
use markdown_core::appearance;
use std::{collections::HashMap, time::Duration};

const TAB_HEIGHT: f32 = 28.;
const TAB_GAP: f32 = 4.;
const LEFT: f32 = if cfg!(target_os = "macos") { 84. } else { 12. };
const RIGHT: f32 = 12.;
const SLIDE: Duration = Duration::from_millis(120);
const FADE: Duration = Duration::from_millis(100);

struct Visual {
    x: Motion,
    active: Motion,
    hover: Motion,
    opacity: Motion,
}

#[derive(Default)]
pub(super) struct Header {
    visuals: HashMap<TabId, Visual>,
    metrics: Option<(Pixels, Pixels, usize)>,
    hover: Option<TabId>,
    drag: Option<drag::Drag>,
    scroll_task: Option<Task<()>>,
}

impl Header {
    pub(super) fn is_dragging(&self) -> bool {
        self.drag.as_ref().is_some_and(|drag| drag.moving)
    }
}

impl Workspace {
    pub(super) fn prepare_tab_header(&mut self, window: &Window, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let (viewport, width) = self.tab_metrics(window);
        let metrics = (viewport, width, self.tabs.len());
        let initial = self.tab_header.metrics.is_none();
        let resized = self.tab_header.metrics != Some(metrics);
        let dragged = self
            .tab_header
            .drag
            .as_ref()
            .filter(|drag| drag.moving)
            .map(|drag| drag.id);
        let drag_x = self.drag_tab_left(window);
        self.tab_header.metrics = Some(metrics);
        self.tab_header
            .visuals
            .retain(|id, _| self.tabs.iter().any(|tab| tab.id == *id));
        for (index, tab) in self.tabs.iter().enumerate() {
            let x = f32::from((width + px(TAB_GAP)) * index);
            let active = if tab.id == self.active { 1. } else { 0. };
            let hover = if self.tab_header.hover == Some(tab.id) && dragged.is_none() {
                1.
            } else {
                0.
            };
            let visual = self
                .tab_header
                .visuals
                .entry(tab.id)
                .or_insert_with(|| Visual {
                    x: Motion::new(x, x, now, SLIDE),
                    active: Motion::new(active, active, now, FADE),
                    hover: Motion::new(0., 0., now, FADE),
                    opacity: Motion::new(if initial { 1. } else { 0. }, 1., now, SLIDE),
                });
            if dragged == Some(tab.id) {
                let x = f32::from(drag_x.unwrap_or(px(x)));
                visual.x = Motion::new(x, x, now, SLIDE);
            } else if resized {
                visual.x = Motion::new(x, x, now, SLIDE);
            } else {
                visual.x.retarget(x, now, SLIDE);
            }
            visual.active.retarget(active, now, FADE);
            visual.hover.retarget(hover, now, FADE);
            if [&visual.x, &visual.active, &visual.hover, &visual.opacity]
                .iter()
                .any(|motion| motion.moving(now))
            {
                window.request_animation_frame();
            }
        }
    }

    fn tab_metrics(&self, window: &Window) -> (Pixels, Pixels) {
        let available =
            (window.viewport_size().width - px(LEFT + RIGHT + TAB_GAP + TAB_HEIGHT)).max(px(0.));
        let width = ((available - px(TAB_GAP * self.tabs.len().saturating_sub(1) as f32))
            / self.tabs.len() as f32)
            .clamp(px(128.), px(200.));
        let content =
            width * self.tabs.len() + px(TAB_GAP * self.tabs.len().saturating_sub(1) as f32);
        (available.min(content), width)
    }

    pub(super) fn reveal_active_tab(&self, window: &Window) {
        // GPUI 0.2's scroll-to-item uses the previous frame's viewport bounds.
        let (available, width) = self.tab_metrics(window);
        let index = self
            .tabs
            .iter()
            .position(|tab| tab.id == self.active)
            .unwrap();
        let left = (width + px(TAB_GAP)) * index;
        let right = left + width;
        let content = (width + px(TAB_GAP)) * self.tabs.len() - px(TAB_GAP);
        let mut offset = self
            .tab_scroll
            .offset()
            .x
            .clamp((available - content).min(px(0.)), px(0.));
        if left + offset < px(0.) {
            offset = -left;
        }
        if right + offset > available {
            offset = available - right;
        }
        self.tab_scroll
            .set_offset(point(offset.min(px(0.)), px(0.)));
    }

    pub(super) fn tab_label(&self, tab: &Tab) -> String {
        let name = tab.name();
        let Some(parent) = tab.path.as_ref().and_then(|path| path.parent()) else {
            return name;
        };
        let others = self
            .tabs
            .iter()
            .filter(|other| other.id != tab.id && other.name() == name)
            .filter_map(|other| other.path.as_ref().and_then(|path| path.parent()))
            .collect::<Vec<_>>();
        if others.is_empty() {
            return name;
        }
        let components = parent.components().collect::<Vec<_>>();
        for start in (0..components.len()).rev() {
            let suffix = components[start..].iter().collect::<std::path::PathBuf>();
            if others.iter().all(|other| !other.ends_with(&suffix)) {
                return format!("{name} · {}", suffix.display());
            }
        }
        name
    }

    pub(super) fn tab_bar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (viewport, width) = self.tab_metrics(window);
        let now = cx.background_executor().now();
        let dragged = self
            .tab_header
            .drag
            .as_ref()
            .filter(|drag| drag.moving)
            .map(|drag| drag.id);
        let mut ordered = self.tabs.iter().collect::<Vec<_>>();
        ordered.sort_by_key(|tab| tab.id == dragged.unwrap_or(self.active));
        let header = cx.entity();
        let content_width = (width + px(TAB_GAP)) * self.tabs.len() - px(TAB_GAP);
        let controls_width = viewport + px(TAB_GAP + TAB_HEIGHT);
        div()
            .id("tab-bar")
            .debug_selector(|| "tab-bar".into())
            .absolute()
            .top(px((appearance::TOP_GUTTER - TAB_HEIGHT) / 2.))
            .left(px(LEFT))
            .right(px(RIGHT))
            .h(px(TAB_HEIGHT))
            .flex()
            .items_center()
            .gap(px(TAB_GAP))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.start_window_move();
                cx.stop_propagation();
            })
            .child(
                div()
                    .id("tabs")
                    .debug_selector(|| "tabs".into())
                    .flex()
                    .w(viewport)
                    .flex_shrink_0()
                    .min_w_0()
                    .h_full()
                    .gap(px(TAB_GAP))
                    .overflow_x_scroll()
                    .track_scroll(&self.tab_scroll)
                    .child(
                        div()
                            .relative()
                            .w(content_width)
                            .h_full()
                            .flex_shrink_0()
                            .children(ordered.into_iter().map(|tab| {
                                let id = tab.id;
                                let active = id == self.active;
                                let label = self.tab_label(tab);
                                let visual = &self.tab_header.visuals[&id];
                                let active_amount = visual.active.value(now);
                                let hover_amount = visual.hover.value(now);
                                let base: Hsla = rgb(appearance::CHROME).into();
                                let hover: Hsla = rgb(appearance::CHROME_HOVER).into();
                                let selected: Hsla = rgb(appearance::BACKGROUND).into();
                                let border: Hsla = rgb(appearance::CHROME_BORDER).into();
                                let muted: Hsla = rgb(appearance::MUTED).into();
                                let text: Hsla = rgb(appearance::TEXT).into();
                                div()
                                    .id(("tab", id))
                                    .debug_selector(move || format!("tab-{id}"))
                                    .absolute()
                                    .left(px(visual.x.value(now)))
                                    .top_0()
                                    .opacity(if dragged == Some(id) {
                                        1.
                                    } else {
                                        visual.opacity.value(now)
                                    })
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .w(width)
                                    .h_full()
                                    .flex_shrink_0()
                                    .pl(px(10.))
                                    .pr_1()
                                    .rounded(px(appearance::CHROME_RADIUS))
                                    .border_1()
                                    .border_color(border.opacity(active_amount))
                                    .bg(base
                                        .blend(hover.opacity(hover_amount))
                                        .blend(selected.opacity(active_amount)))
                                    .text_color(muted.blend(text.opacity(active_amount)))
                                    .font_weight(if active {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .cursor(if dragged == Some(id) {
                                        CursorStyle::ClosedHand
                                    } else {
                                        CursorStyle::PointingHand
                                    })
                                    .when(dragged == Some(id), |view| {
                                        view.shadow(vec![BoxShadow {
                                            color: rgba(0x00000020).into(),
                                            offset: point(px(0.), px(2.)),
                                            blur_radius: px(8.),
                                            spread_radius: px(0.),
                                        }])
                                    })
                                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                        if *hovered {
                                            this.tab_header.hover = Some(id);
                                        } else if this.tab_header.hover == Some(id) {
                                            this.tab_header.hover = None;
                                        }
                                        cx.notify();
                                    }))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(
                                            move |this, event: &MouseDownEvent, window, cx| {
                                                cx.stop_propagation();
                                                this.press_tab(id, event, window, cx);
                                            },
                                        ),
                                    )
                                    .child(
                                        if let Some(rename) =
                                            self.rename.as_ref().filter(|r| r.id == id)
                                        {
                                            div()
                                                .min_w_0()
                                                .flex_1()
                                                .h(px(22.))
                                                .child(rename.input.clone())
                                                .into_any_element()
                                        } else {
                                            div()
                                                .min_w_0()
                                                .flex_1()
                                                .truncate()
                                                .child(label)
                                                .into_any_element()
                                        },
                                    )
                                    .when(tab.dirty(cx), |view| {
                                        view.child(
                                            div()
                                                .size(px(5.))
                                                .flex_shrink_0()
                                                .rounded_full()
                                                .bg(rgb(appearance::MUTED)),
                                        )
                                    })
                                    .when(self.can_close_tab(id, cx), |view| {
                                        view.child(
                                            div()
                                                .id(("close-tab", id))
                                                .debug_selector(move || format!("close-tab-{id}"))
                                                .size(px(24.))
                                                .flex_shrink_0()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .rounded_md()
                                                .text_color(rgb(appearance::MUTED))
                                                .text_size(px(16.))
                                                .hover(|style| {
                                                    style.bg(rgb(appearance::CHROME_HOVER))
                                                })
                                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                    cx.stop_propagation()
                                                })
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        cx.stop_propagation();
                                                        this.request_close(
                                                            CloseTarget::Tab(id),
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                ))
                                                .child("×"),
                                        )
                                    })
                            })),
                    ),
            )
            .child(
                div()
                    .id("new-tab")
                    .debug_selector(|| "new-tab".into())
                    .w(px(TAB_HEIGHT))
                    .h_full()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(appearance::CHROME_RADIUS))
                    .text_size(px(18.))
                    .text_color(rgb(appearance::MUTED))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(appearance::CHROME_HOVER)))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, window, cx| this.new_tab(window, cx)))
                    .child("+"),
            )
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        crate::native_header::update(
                            window,
                            Bounds::new(bounds.origin, size(controls_width, bounds.size.height)),
                        );
                        drag::listen(&header, window);
                    },
                )
                .absolute()
                .inset_0()
                .size_full(),
            )
    }
}

#[cfg(test)]
mod tests;
