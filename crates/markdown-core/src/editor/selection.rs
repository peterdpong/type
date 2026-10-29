use super::MarkdownEditor;
use gpui::{
    Bounds, Context, DispatchPhase, Entity, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, Task, Window, point, px,
};
use std::time::Duration;

const FRAME: Duration = Duration::from_millis(16);
const EDGE: f32 = 32.;
const MAX_SPEED: f32 = 1600.;

#[derive(Default)]
pub(super) struct Drag {
    pointer: Option<Point<Pixels>>,
    task: Option<Task<()>>,
}

pub(super) fn stop(editor: &mut MarkdownEditor) {
    editor.selecting = false;
    editor.drag_selection = Drag::default();
}

// Element mouse-move handlers only receive events inside their hitboxes.
pub(super) fn listen(editor: &Entity<MarkdownEditor>, window: &mut Window) {
    let moving = editor.downgrade();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
        if phase != DispatchPhase::Capture {
            return;
        }
        let _ = moving.update(cx, |editor, cx| {
            if !editor.selecting {
                return;
            }
            if event.pressed_button != Some(MouseButton::Left) {
                stop(editor);
                cx.notify();
                return;
            }
            editor.drag_selection.pointer = Some(event.position);
            extend(editor, cx);
            schedule(editor, cx);
            cx.stop_propagation();
        });
    });
    let released = editor.downgrade();
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
        if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
            return;
        }
        let _ = released.update(cx, |editor, cx| {
            if editor.selecting {
                if editor.drag_selection.pointer.is_some() {
                    editor.drag_selection.pointer = Some(event.position);
                    extend(editor, cx);
                }
                stop(editor);
                cx.notify();
                cx.stop_propagation();
            }
        });
    });
}

fn velocity(bounds: Bounds<Pixels>, pointer: Point<Pixels>) -> f32 {
    let edge = px(EDGE).min(bounds.size.height / 2.);
    if pointer.y < bounds.top() + edge {
        -((bounds.top() + edge - pointer.y) / px(1.) * 12.).min(MAX_SPEED)
    } else if pointer.y > bounds.bottom() - edge {
        ((pointer.y - bounds.bottom() + edge) / px(1.) * 12.).min(MAX_SPEED)
    } else {
        0.
    }
}

fn speed(editor: &MarkdownEditor) -> f32 {
    if !editor.selecting || editor.content_bounds().is_empty() {
        return 0.;
    }
    let Some(pointer) = editor.drag_selection.pointer else {
        return 0.;
    };
    let speed = velocity(editor.content_bounds(), pointer);
    if (speed < 0. && editor.scroll <= px(0.)) || (speed > 0. && editor.scroll >= editor.max_scroll)
    {
        0.
    } else {
        speed
    }
}

// Layout has the previous scroll offset until the next frame.
pub(super) fn extend(editor: &mut MarkdownEditor, cx: &mut Context<MarkdownEditor>) {
    if !editor.selecting || editor.layout_dirty || editor.layout.is_empty() {
        return;
    }
    let Some(pointer) = editor.drag_selection.pointer else {
        return;
    };
    let bounds = editor.content_bounds();
    if bounds.is_empty() {
        return;
    }
    let index = if pointer.y < bounds.top() && editor.scroll <= px(0.) {
        0
    } else if pointer.y >= bounds.bottom() && editor.scroll >= editor.max_scroll {
        editor.document.text().len()
    } else {
        editor.hit_test(point(
            pointer.x,
            pointer.y.clamp(
                bounds.top(),
                bounds.bottom() - px(1.).min(bounds.size.height),
            ) + editor.scroll
                - editor.layout_scroll,
        ))
    };
    if index != editor.document.cursor() {
        editor.move_to(index, true, cx);
    }
    editor.reveal_cursor = false;
}

fn schedule(editor: &mut MarkdownEditor, cx: &mut Context<MarkdownEditor>) {
    if speed(editor) == 0. {
        editor.drag_selection.task = None;
        return;
    }
    if editor.drag_selection.task.is_some() {
        return;
    }
    let mut previous = cx.background_executor().now();
    editor.drag_selection.task = Some(cx.spawn(async move |editor, cx| {
        loop {
            cx.background_executor().timer(FRAME).await;
            let keep_scrolling = editor.update(cx, |editor, cx| {
                let now = cx.background_executor().now();
                let elapsed = now
                    .saturating_duration_since(previous)
                    .min(Duration::from_millis(50));
                previous = now;
                editor.scroll = (editor.scroll + px(speed(editor) * elapsed.as_secs_f32()))
                    .clamp(px(0.), editor.max_scroll);
                extend(editor, cx);
                cx.notify();
                if speed(editor) == 0. {
                    editor.drag_selection.task = None;
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

#[cfg(test)]
mod tests;
