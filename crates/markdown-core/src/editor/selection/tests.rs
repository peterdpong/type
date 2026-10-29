use super::{FRAME, velocity};
use crate::editor::{MarkdownEditor, init};
use gpui::{
    Bounds, Context, Entity, Focusable, IntoElement, Modifiers, MouseButton, Pixels, Point, Render,
    TestAppContext, VisualTestContext, Window, div, point, prelude::*, px, size,
};

struct Host {
    editor: Entity<MarkdownEditor>,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .child(div().flex_1().min_h_0().w_full().child(self.editor.clone()))
            .child(
                div()
                    .h(px(30.))
                    .flex_shrink_0()
                    .w_full()
                    .on_mouse_move(|_, _, cx| cx.stop_propagation())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h(px(crate::appearance::TOP_GUTTER))
                    .on_mouse_move(|_, _, cx| cx.stop_propagation())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
            )
    }
}

fn setup(cx: &mut TestAppContext, rows: usize) -> (Entity<MarkdownEditor>, &mut VisualTestContext) {
    cx.update(init);
    let source = (0..rows)
        .map(|i| format!("{i:03} A paragraph with **bold**, `code`, and café 🦀.\n"))
        .collect::<String>();
    let (host, cx) = cx.add_window_view(|window, cx| {
        let editor = cx.new(|cx| MarkdownEditor::new(source, cx));
        window.focus(&editor.focus_handle(cx));
        Host { editor }
    });
    let editor = host.read_with(cx, |host, _| host.editor.clone());
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    (editor, cx)
}

fn advance(cx: &mut VisualTestContext, frames: usize) {
    for _ in 0..frames {
        cx.executor().advance_clock(FRAME);
        cx.run_until_parked();
    }
}

fn start(editor: &Entity<MarkdownEditor>, cx: &mut VisualTestContext) -> usize {
    let position = editor.read_with(cx, |e, _| e.content_bounds().center());
    cx.simulate_mouse_down(position, MouseButton::Left, Modifiers::default());
    editor.read_with(cx, |e, _| e.document.anchor())
}

fn drag(cx: &mut VisualTestContext, position: Point<Pixels>) {
    cx.simulate_mouse_move(position, MouseButton::Left, Modifiers::default());
}

fn scroll(editor: &Entity<MarkdownEditor>, cx: &mut VisualTestContext) -> Pixels {
    editor.read_with(cx, |e, _| e.scroll)
}

#[test]
fn edge_speed_keeps_its_direction_far_outside_the_viewport() {
    let bounds = Bounds::new(point(px(0.), px(56.)), size(px(800.), px(600.)));
    assert_eq!(velocity(bounds, bounds.center()), 0.);
    for distance in [0., 10., 100., 1000., 10000.] {
        assert!(velocity(bounds, point(px(-500.), bounds.top() - px(distance))) < 0.);
        assert!(velocity(bounds, point(px(2000.), bounds.bottom() + px(distance))) > 0.);
    }
}

#[gpui::test]
fn selection_scrolls_over_chrome_and_far_outside_while_pointer_is_stationary(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = setup(cx, 180);
    let source = editor.read_with(cx, |e, _| e.markdown().to_string());
    let bounds = editor.read_with(cx, |e, _| e.content_bounds());
    let anchor = start(&editor, cx);
    for y in [
        bounds.bottom() - px(4.),
        bounds.bottom() + px(15.),
        bounds.bottom() + px(1000.),
    ] {
        drag(cx, point(bounds.center().x, y));
        let before = scroll(&editor, cx);
        let cursor = editor.read_with(cx, |e, _| e.document.cursor());
        advance(cx, 10);
        editor.read_with(cx, |e, _| {
            assert!(e.scroll > before, "stalled at {y:?}");
            assert!(e.document.cursor() > cursor);
            assert_eq!(e.document.anchor(), anchor);
            assert!(e.markdown().is_char_boundary(e.document.cursor()));
            assert!(e.toolbelt_bounds.is_none());
        });
    }
    for y in [px(20.), px(-1000.)] {
        drag(cx, point(bounds.center().x, y));
        let before = scroll(&editor, cx);
        let cursor = editor.read_with(cx, |e, _| e.document.cursor());
        advance(cx, 5);
        editor.read_with(cx, |e, _| {
            assert!(e.scroll < before, "stalled at {y:?}");
            assert!(e.document.cursor() < cursor);
            assert_eq!(e.document.anchor(), anchor);
            assert_eq!(e.markdown(), source);
        });
    }
}

#[gpui::test]
fn returning_to_content_and_releasing_outside_stop_scrolling(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, 180);
    let bounds = editor.read_with(cx, |e, _| e.content_bounds());
    let outside = point(bounds.center().x, bounds.bottom() + px(800.));
    start(&editor, cx);
    drag(cx, outside);
    advance(cx, 5);
    drag(cx, bounds.center());
    let paused = scroll(&editor, cx);
    advance(cx, 10);
    assert_eq!(scroll(&editor, cx), paused);
    drag(cx, outside);
    advance(cx, 5);
    assert!(scroll(&editor, cx) > paused);
    cx.simulate_mouse_up(outside, MouseButton::Left, Modifiers::default());
    let released = editor.read_with(cx, |e, _| {
        assert!(!e.selecting);
        assert!(e.drag_selection.task.is_none());
        (e.scroll, e.document.selection())
    });
    advance(cx, 10);
    cx.simulate_mouse_move(point(px(-200.), px(-500.)), None, Modifiers::default());
    editor.read_with(cx, |e, _| {
        assert_eq!((e.scroll, e.document.selection()), released)
    });
}

#[gpui::test]
fn drag_reaches_both_document_ends_without_changing_source(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, 45);
    let source = editor.read_with(cx, |e, _| e.markdown().to_string());
    editor.update_in(cx, |e, _, cx| {
        e.scroll = e.max_scroll - px(40.);
        e.reveal_cursor = false;
        cx.notify();
    });
    cx.run_until_parked();
    let bounds = editor.read_with(cx, |e, _| e.content_bounds());
    let anchor = start(&editor, cx);
    drag(cx, point(bounds.center().x, bounds.bottom() + px(1000.)));
    advance(cx, 5);
    editor.read_with(cx, |e, _| {
        assert_eq!(e.scroll, e.max_scroll);
        assert_eq!(e.document.cursor(), source.len());
        assert_eq!(e.document.anchor(), anchor);
        assert!(e.drag_selection.task.is_none());
    });
    drag(cx, point(bounds.center().x, px(-1000.)));
    advance(cx, 120);
    editor.read_with(cx, |e, _| {
        assert_eq!(e.scroll, px(0.));
        assert_eq!(e.document.cursor(), 0);
        assert_eq!(e.document.anchor(), anchor);
        assert_eq!(e.markdown(), source);
        assert!(e.drag_selection.task.is_none());
    });
}

#[gpui::test]
fn lost_button_release_and_editor_deactivation_cancel_drag(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, 180);
    let outside = editor.read_with(cx, |e, _| {
        e.bounds.bottom_right() + point(px(100.), px(100.))
    });
    start(&editor, cx);
    drag(cx, outside);
    advance(cx, 3);
    cx.simulate_mouse_move(outside, None, Modifiers::default());
    let before = scroll(&editor, cx);
    advance(cx, 5);
    editor.read_with(cx, |e, _| {
        assert!(!e.selecting);
        assert!(e.drag_selection.task.is_none());
        assert_eq!(e.scroll, before);
    });
    start(&editor, cx);
    drag(cx, outside);
    advance(cx, 3);
    editor.update_in(cx, |e, window, cx| e.deactivate(window, cx));
    let before = scroll(&editor, cx);
    advance(cx, 5);
    editor.read_with(cx, |e, _| {
        assert!(!e.selecting);
        assert!(e.drag_selection.task.is_none());
        assert_eq!(e.scroll, before);
    });
}

#[gpui::test]
fn window_deactivation_cancels_stationary_drag(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, 180);
    let outside = editor.read_with(cx, |e, _| e.bounds.bottom_right() + point(px(0.), px(100.)));
    start(&editor, cx);
    drag(cx, outside);
    advance(cx, 3);
    cx.deactivate_window();
    let before = scroll(&editor, cx);
    advance(cx, 5);
    editor.read_with(cx, |e, _| {
        assert!(!e.selecting);
        assert!(e.drag_selection.task.is_none());
        assert_eq!(e.scroll, before);
    });
}
