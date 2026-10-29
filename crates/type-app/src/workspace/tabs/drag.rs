use super::{LEFT, TAB_GAP, TabId, Workspace};
use gpui::*;
use std::time::Duration;

const FRAME: Duration = Duration::from_millis(16);
const EDGE: f32 = 32.;

pub(super) struct Drag {
    pub id: TabId,
    pub moving: bool,
    start: Point<Pixels>,
    pointer: Point<Pixels>,
    grab: Pixels,
    order: Vec<TabId>,
}

pub(super) fn listen(workspace: &Entity<Workspace>, window: &mut Window) {
    let moving = workspace.downgrade();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase != DispatchPhase::Capture {
            return;
        }
        let _ = moving.update(cx, |this, cx| {
            let Some(drag) = &mut this.tab_header.drag else {
                return;
            };
            if event.pressed_button != Some(MouseButton::Left) {
                this.finish_tab_drag(true, window, cx);
                return;
            }
            drag.pointer = event.position;
            if !drag.moving {
                let distance = event.position - drag.start;
                if distance.x.abs().max(distance.y.abs()) < px(5.) {
                    return;
                }
                drag.moving = true;
                window.focus(&this.focus);
            }
            this.reorder_dragged_tab(window, cx);
            this.schedule_tab_scroll(window, cx);
            cx.notify();
            cx.stop_propagation();
        });
    });
    let released = workspace.downgrade();
    window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
        if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
            return;
        }
        let _ = released.update(cx, |this, cx| {
            let Some(drag) = &mut this.tab_header.drag else {
                return;
            };
            let moved = drag.moving;
            if moved {
                drag.pointer = event.position;
                this.reorder_dragged_tab(window, cx);
            }
            this.finish_tab_drag(false, window, cx);
            if moved {
                cx.stop_propagation();
            }
        });
    });
}

impl Workspace {
    pub(super) fn press_tab(
        &mut self,
        id: TabId,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dialog_busy || self.closing.is_some() {
            return;
        }
        if event.click_count == 2 {
            self.begin_rename(id, window, cx);
            return;
        }
        if self.rename.as_ref().is_some_and(|rename| rename.id == id) {
            return;
        }
        let (_, width) = self.tab_metrics(window);
        let now = cx.background_executor().now();
        let Some(visual) = self.tab_header.visuals.get(&id) else {
            return;
        };
        let grab =
            (event.position.x - px(LEFT) - self.tab_scroll.offset().x - px(visual.x.value(now)))
                .clamp(px(0.), width);
        self.activate(id, window, cx);
        if self.tabs.len() > 1 {
            self.tab_header.drag = Some(Drag {
                id,
                moving: false,
                start: event.position,
                pointer: event.position,
                grab,
                order: self.tabs.iter().map(|tab| tab.id).collect(),
            });
        }
    }

    pub(super) fn drag_tab_left(&self, window: &Window) -> Option<Pixels> {
        let drag = self.tab_header.drag.as_ref().filter(|drag| drag.moving)?;
        let (viewport, width) = self.tab_metrics(window);
        Some(
            (drag.pointer.x - px(LEFT) - drag.grab).clamp(px(0.), (viewport - width).max(px(0.)))
                - self.tab_scroll.offset().x,
        )
    }

    fn reorder_dragged_tab(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(left) = self.drag_tab_left(window) else {
            return;
        };
        let id = self.tab_header.drag.as_ref().unwrap().id;
        let Some(from) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        let (_, width) = self.tab_metrics(window);
        let to = (((left + width / 2.) / (width + px(TAB_GAP))).floor() as usize)
            .min(self.tabs.len() - 1);
        if from != to {
            let tab = self.tabs.remove(from);
            self.tabs.insert(to, tab);
            cx.notify();
        }
    }

    pub(in crate::workspace) fn finish_tab_drag(
        &mut self,
        cancel: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tab_header.scroll_task = None;
        let Some(drag) = self.tab_header.drag.take() else {
            return;
        };
        if drag.moving {
            if cancel {
                self.tabs.sort_by_key(|tab| {
                    drag.order
                        .iter()
                        .position(|id| *id == tab.id)
                        .unwrap_or(usize::MAX)
                });
            }
            self.tab_header.hover = None;
            self.reveal_active_tab(window);
            self.focus_editor(window, cx);
        }
        cx.notify();
    }

    pub(in crate::workspace) fn tab_drag_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self
            .tab_header
            .drag
            .as_ref()
            .is_some_and(|drag| drag.moving)
        {
            return false;
        }
        if event.keystroke.key == "escape" {
            self.finish_tab_drag(true, window, cx);
        }
        window.prevent_default();
        cx.stop_propagation();
        true
    }

    fn tab_scroll_speed(&self, window: &Window) -> f32 {
        let Some(drag) = self.tab_header.drag.as_ref().filter(|drag| drag.moving) else {
            return 0.;
        };
        let (viewport, width) = self.tab_metrics(window);
        let edge = px(EDGE).min(viewport / 2.);
        let x = drag.pointer.x - px(LEFT);
        let speed = if x < edge {
            -((edge - x) / px(1.) * 12.).min(1200.)
        } else if x > viewport - edge {
            ((x - viewport + edge) / px(1.) * 12.).min(1200.)
        } else {
            0.
        };
        let minimum =
            (viewport - ((width + px(TAB_GAP)) * self.tabs.len() - px(TAB_GAP))).min(px(0.));
        let offset = self.tab_scroll.offset().x;
        if (speed < 0. && offset >= px(0.)) || (speed > 0. && offset <= minimum) {
            0.
        } else {
            speed
        }
    }

    fn schedule_tab_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab_scroll_speed(window) == 0. {
            self.tab_header.scroll_task = None;
            return;
        }
        if self.tab_header.scroll_task.is_some() {
            return;
        }
        let mut previous = cx.background_executor().now();
        self.tab_header.scroll_task = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(FRAME).await;
                let keep_scrolling = this.update_in(cx, |this, window, cx| {
                    let now = cx.background_executor().now();
                    let elapsed = now
                        .saturating_duration_since(previous)
                        .min(Duration::from_millis(50));
                    previous = now;
                    let (viewport, width) = this.tab_metrics(window);
                    let minimum = (viewport
                        - ((width + px(TAB_GAP)) * this.tabs.len() - px(TAB_GAP)))
                    .min(px(0.));
                    let offset = (this.tab_scroll.offset().x
                        - px(this.tab_scroll_speed(window) * elapsed.as_secs_f32()))
                    .clamp(minimum, px(0.));
                    this.tab_scroll.set_offset(point(offset, px(0.)));
                    this.reorder_dragged_tab(window, cx);
                    cx.notify();
                    if this.tab_scroll_speed(window) == 0. {
                        this.tab_header.scroll_task = None;
                        false
                    } else {
                        true
                    }
                });
                if !keep_scrolling.unwrap_or(false) {
                    break;
                }
            }
        }));
    }
}
