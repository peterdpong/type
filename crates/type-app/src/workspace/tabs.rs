use super::{CloseTarget, Tab, Workspace};
use gpui::{prelude::*, *};
use markdown_core::appearance;

const TAB_HEIGHT: f32 = 28.;
const TAB_GAP: f32 = 4.;
const LEFT: f32 = if cfg!(target_os = "macos") { 84. } else { 12. };
const RIGHT: f32 = 12.;

impl Workspace {
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
                    .children(self.tabs.iter().map(|tab| {
                        let id = tab.id;
                        let active = id == self.active;
                        let label = self.tab_label(tab);
                        div()
                            .id(("tab", id))
                            .debug_selector(move || format!("tab-{id}"))
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
                            .border_color(if active {
                                rgb(appearance::CHROME_BORDER).into()
                            } else {
                                transparent_black()
                            })
                            .when(active, |view| view.bg(rgb(appearance::BACKGROUND)))
                            .text_color(rgb(if active {
                                appearance::TEXT
                            } else {
                                appearance::MUTED
                            }))
                            .font_weight(if active {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .cursor_pointer()
                            .when(!active, |view| {
                                view.hover(|style| style.bg(rgb(appearance::CHROME_HOVER)))
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                    cx.stop_propagation();
                                    if event.click_count == 2 {
                                        this.begin_rename(id, window, cx);
                                    }
                                }),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if !this.rename.as_ref().is_some_and(|r| r.id == id) {
                                    this.activate(id, window, cx);
                                }
                            }))
                            .child(
                                if let Some(rename) = self.rename.as_ref().filter(|r| r.id == id) {
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
                                        .hover(|style| style.bg(rgb(appearance::CHROME_HOVER)))
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            cx.stop_propagation();
                                            this.request_close(CloseTarget::Tab(id), window, cx);
                                        }))
                                        .child("×"),
                                )
                            })
                    })),
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
    }
}
