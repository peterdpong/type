use super::{SLIDE, TabId, Workspace};
use crate::{New, NextTab, Save};
use gpui::{
    Entity, Focusable, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, TestAppContext, VisualTestContext, point, px, size,
};
use std::time::Duration;

fn advance(cx: &mut VisualTestContext, duration: Duration) {
    cx.executor().advance_clock(duration);
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

fn setup(
    cx: &mut TestAppContext,
    count: usize,
    width: f32,
) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(crate::init);
    let (app, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _| window.activate_window());
    for i in 0..count {
        if i > 0 {
            cx.dispatch_action(New);
        }
        app.update_in(cx, |app, _, cx| {
            app.active_tab().editor.update(cx, |editor, cx| {
                editor.set_markdown(format!("Document {i}"), cx)
            })
        });
    }
    cx.simulate_resize(size(px(width), px(600.)));
    advance(cx, SLIDE);
    (app, cx)
}

fn order(app: &Entity<Workspace>, cx: &VisualTestContext) -> Vec<TabId> {
    app.read_with(cx, |app, _| app.tabs.iter().map(|tab| tab.id).collect())
}

fn down(cx: &mut VisualTestContext, point: Point<Pixels>) {
    cx.simulate_event(MouseDownEvent {
        position: point,
        button: MouseButton::Left,
        click_count: 1,
        ..Default::default()
    });
}
fn move_to(cx: &mut VisualTestContext, point: Point<Pixels>) {
    cx.simulate_event(MouseMoveEvent {
        position: point,
        pressed_button: Some(MouseButton::Left),
        ..Default::default()
    });
}
fn up(cx: &mut VisualTestContext, point: Point<Pixels>) {
    cx.simulate_event(MouseUpEvent {
        position: point,
        button: MouseButton::Left,
        click_count: 1,
        ..Default::default()
    });
}

#[gpui::test]
fn reordering_keeps_files_carets_and_undo_and_updates_keyboard_order(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("first.md");
    std::fs::write(&path, "abcdef").unwrap();
    let (app, cx) = setup(cx, 1, 960.);
    app.update_in(cx, |app, _, cx| {
        app.tabs[0]
            .editor
            .update(cx, |editor, cx| editor.set_markdown("", cx))
    });
    app.update_in(cx, |app, window, cx| {
        app.open_paths(vec![path.clone()], window, cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("end left left");
    cx.dispatch_action(New);
    cx.simulate_input("Second");
    cx.dispatch_action(New);
    cx.simulate_input("Third");
    advance(cx, SLIDE);
    let editors = app.read_with(cx, |app, _| {
        app.tabs
            .iter()
            .map(|tab| (tab.id, tab.editor.entity_id()))
            .collect::<Vec<_>>()
    });
    let start = cx.debug_bounds("tab-0").unwrap().center();
    let end = cx.debug_bounds("tab-2").unwrap().center();
    down(cx, start);
    move_to(cx, end);
    assert_eq!(order(&app, cx), vec![1, 2, 0]);
    up(cx, end);
    app.update_in(cx, |app, window, cx| {
        assert_eq!(app.active, 0);
        assert!(app.active_tab().editor.focus_handle(cx).is_focused(window));
        for (id, editor) in &editors {
            assert_eq!(
                app.tabs
                    .iter()
                    .find(|tab| tab.id == *id)
                    .unwrap()
                    .editor
                    .entity_id(),
                *editor
            );
        }
    });
    cx.simulate_input("!");
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "abcd!ef")
    });
    cx.dispatch_action(markdown_core::Undo);
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "abcdef")
    });
    cx.dispatch_action(markdown_core::Redo);
    cx.dispatch_action(Save);
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "abcd!ef");
    cx.dispatch_action(NextTab);
    app.read_with(cx, |app, _| assert_eq!(app.active, 1));
    cx.simulate_modifiers_change(Modifiers {
        control: true,
        ..Default::default()
    });
    cx.simulate_keystrokes("ctrl-tab");
    cx.simulate_modifiers_change(Modifiers::default());
    app.read_with(cx, |app, _| assert_eq!(app.active, 2));
}

#[gpui::test]
fn a_click_does_not_reorder_and_escape_or_deactivation_cancels_a_drag(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx, 3, 960.);
    let start = cx.debug_bounds("tab-0").unwrap().center();
    let end = cx.debug_bounds("tab-2").unwrap().center();
    down(cx, start);
    move_to(cx, start + point(px(2.), px(1.)));
    up(cx, start);
    assert_eq!(order(&app, cx), vec![0, 1, 2]);
    down(cx, start);
    move_to(cx, end);
    assert_eq!(order(&app, cx), vec![1, 2, 0]);
    cx.simulate_keystrokes("escape");
    up(cx, end);
    assert_eq!(order(&app, cx), vec![0, 1, 2]);
    app.update_in(cx, |app, window, cx| {
        assert!(app.tab_header.drag.is_none());
        assert!(app.active_tab().editor.focus_handle(cx).is_focused(window));
    });
    advance(cx, SLIDE);
    down(cx, start);
    move_to(cx, end);
    cx.deactivate_window();
    cx.run_until_parked();
    assert_eq!(order(&app, cx), vec![0, 1, 2]);
    app.read_with(cx, |app, _| {
        assert!(app.tab_header.drag.is_none());
        assert!(app.tab_header.scroll_task.is_none());
    });
}

#[gpui::test]
fn overflow_drag_scrolls_with_a_stationary_pointer_outside_either_edge(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx, 8, 560.);
    cx.dispatch_action(NextTab);
    let start = cx.debug_bounds("tab-0").unwrap().center();
    let right = point(px(1400.), px(20.));
    down(cx, start);
    move_to(cx, right);
    for _ in 0..75 {
        advance(cx, Duration::from_millis(16));
    }
    assert_eq!(order(&app, cx), vec![1, 2, 3, 4, 5, 6, 7, 0]);
    up(cx, right);
    advance(cx, SLIDE);
    let viewport = cx.debug_bounds("tabs").unwrap();
    let last = cx.debug_bounds("tab-0").unwrap();
    assert!(last.left() >= viewport.left() && last.right() <= viewport.right());
    app.read_with(cx, |app, _| {
        assert!(app.tab_header.drag.is_none());
        assert!(app.tab_header.scroll_task.is_none());
    });
    down(cx, last.center());
    let left = point(px(-800.), px(20.));
    move_to(cx, left);
    for _ in 0..75 {
        advance(cx, Duration::from_millis(16));
    }
    up(cx, left);
    assert_eq!(order(&app, cx), vec![0, 1, 2, 3, 4, 5, 6, 7]);
    let offset = app.read_with(cx, |app, _| app.tab_scroll.offset());
    advance(cx, Duration::from_secs(1));
    app.read_with(cx, |app, _| {
        assert_eq!(app.tab_scroll.offset(), offset);
        assert_eq!(offset.x, px(0.));
    });
}

#[gpui::test]
fn reversing_a_drag_retargets_neighbor_motion_without_a_jump(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx, 3, 960.);
    let start = cx.debug_bounds("tab-0").unwrap().center();
    let end = cx.debug_bounds("tab-2").unwrap().center();
    down(cx, start);
    move_to(cx, end);
    advance(cx, SLIDE / 2);
    let midway = app.read_with(cx, |app, cx| {
        app.tab_header.visuals[&1]
            .x
            .value(cx.background_executor().now())
    });
    assert!(midway > 0. && midway < 204.);
    move_to(cx, start);
    app.read_with(cx, |app, cx| {
        assert_eq!(
            app.tab_header.visuals[&1]
                .x
                .value(cx.background_executor().now()),
            midway
        )
    });
    advance(cx, SLIDE);
    assert_eq!(order(&app, cx), vec![0, 1, 2]);
    app.read_with(cx, |app, cx| {
        assert_eq!(
            app.tab_header.visuals[&1]
                .x
                .value(cx.background_executor().now()),
            204.
        );
    });
    up(cx, start);
}
