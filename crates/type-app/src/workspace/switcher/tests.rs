use super::{TabId, Workspace, byte_size};
use crate::{Close, New, NextTab, PreviousTab};
use gpui::{
    Entity, Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext, VisualTestContext, point, px,
    size,
};

fn setup(cx: &mut TestAppContext, count: usize) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(crate::init);
    let (app, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _| window.activate_window());
    for index in 0..count {
        if index > 0 {
            cx.dispatch_action(New);
        }
        app.update_in(cx, |app, _, cx| {
            app.active_tab().editor.update(cx, |editor, cx| editor.set_markdown(
                format!("# Document {index}\n\nSome **bold** text and `code`.\n\n- [x] Done\n\n> A quote\n\n```rs\nlet x = 1;\n```\n\nLast paragraph"), cx));
        });
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-down"
        } else {
            "ctrl-down"
        });
    }
    cx.simulate_resize(size(px(960.), px(780.)));
    (app, cx)
}

fn hold(cx: &mut VisualTestContext) {
    cx.simulate_modifiers_change(Modifiers {
        control: true,
        ..Default::default()
    });
    cx.simulate_keystrokes("ctrl-tab");
}

fn destination(app: &Entity<Workspace>, cx: &mut VisualTestContext) -> TabId {
    app.read_with(cx, |app, _| {
        let switcher = app.switcher.as_ref().unwrap();
        switcher.items[switcher.selected].id
    })
}

#[gpui::test]
fn control_tab_previews_wraps_and_only_switches_on_control_release(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx, 3);
    hold(cx);
    assert_eq!(destination(&app, cx), 0);
    app.read_with(cx, |app, _| assert_eq!(app.active, 2));
    let panel = cx.debug_bounds("tab-switcher").unwrap();
    assert_eq!(panel.center(), point(px(480.), px(390.)));
    cx.simulate_keystrokes("ctrl-tab ctrl-tab ctrl-tab");
    assert_eq!(destination(&app, cx), 0);
    cx.simulate_keystrokes("ctrl-shift-tab");
    assert_eq!(destination(&app, cx), 2);
    cx.simulate_modifiers_change(Modifiers {
        control: true,
        ..Default::default()
    });
    assert_eq!(destination(&app, cx), 2);
    cx.simulate_keystrokes("ctrl-shift-tab");
    cx.simulate_modifiers_change(Modifiers::default());
    app.read_with(cx, |app, _| {
        assert_eq!(app.active, 1);
        assert!(app.switcher.is_none());
    });
    cx.simulate_input("!");
    app.read_with(cx, |app, cx| {
        assert!(app.tabs[1].editor.read(cx).markdown().ends_with('!'));
        assert!(!app.tabs[2].editor.read(cx).markdown().ends_with('!'));
    });
}

#[gpui::test]
fn escape_preserves_the_original_editor_caret_and_undo_history(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx, 2);
    cx.simulate_input("Tail");
    cx.simulate_keystrokes("left left");
    let original = app.read_with(cx, |app, cx| {
        app.active_tab().editor.read(cx).markdown().to_owned()
    });
    hold(cx);
    cx.simulate_input("never inserted");
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active_tab().editor.read(cx).markdown(), original)
    });
    cx.simulate_keystrokes("escape");
    cx.simulate_modifiers_change(Modifiers::default());
    app.read_with(cx, |app, _| {
        assert_eq!(app.active, 1);
        assert!(app.switcher.is_none());
    });
    cx.simulate_input("!");
    app.read_with(cx, |app, cx| {
        assert!(
            app.active_tab()
                .editor
                .read(cx)
                .markdown()
                .ends_with("Ta!il")
        )
    });
    cx.dispatch_action(markdown_core::Undo);
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active_tab().editor.read(cx).markdown(), original)
    });
}

#[gpui::test]
fn enter_click_and_outside_click_commit_or_cancel_without_extra_switches(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx, 3);
    hold(cx);
    cx.simulate_keystrokes("right enter");
    cx.simulate_modifiers_change(Modifiers::default());
    app.read_with(cx, |app, _| assert_eq!(app.active, 1));
    hold(cx);
    let card = cx.debug_bounds("switcher-card-0").unwrap().center();
    cx.simulate_click(
        card,
        Modifiers {
            control: true,
            ..Default::default()
        },
    );
    cx.simulate_modifiers_change(Modifiers::default());
    app.read_with(cx, |app, _| {
        assert_eq!(app.active, 0);
        assert!(app.switcher.is_none());
    });
    hold(cx);
    cx.simulate_click(point(px(12.), px(120.)), Modifiers::default());
    cx.simulate_modifiers_change(Modifiers::default());
    app.read_with(cx, |app, _| {
        assert_eq!(app.active, 0);
        assert!(app.switcher.is_none());
    });
}

#[gpui::test]
fn many_tabs_fit_scroll_and_reveal_the_destination_after_resize(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx, 15);
    cx.simulate_resize(size(px(560.), px(400.)));
    hold(cx);
    let panel = cx.debug_bounds("tab-switcher").unwrap();
    assert_eq!(panel.center(), point(px(280.), px(200.)));
    assert!(panel.left() >= px(24.) && panel.right() <= px(536.));
    assert!(panel.top() >= px(24.) && panel.bottom() <= px(376.));
    cx.simulate_keystrokes("ctrl-shift-tab");
    let grid = cx.debug_bounds("switcher-grid").unwrap();
    let last = cx.debug_bounds("switcher-card-14").unwrap();
    assert!(
        last.top() >= grid.top() && last.bottom() <= grid.bottom(),
        "{last:?} outside {grid:?}"
    );
    cx.simulate_resize(size(px(1100.), px(700.)));
    let grid = cx.debug_bounds("switcher-grid").unwrap();
    let last = cx.debug_bounds("switcher-card-14").unwrap();
    assert!(last.top() >= grid.top() && last.bottom() <= grid.bottom());
    cx.simulate_keystrokes("ctrl-tab");
    assert_eq!(destination(&app, cx), 0);
    let grid = cx.debug_bounds("switcher-grid").unwrap();
    cx.simulate_event(ScrollWheelEvent {
        position: grid.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-10000.))),
        ..Default::default()
    });
    app.read_with(cx, |app, _| {
        assert!(app.switcher.as_ref().unwrap().scroll.offset().y < px(0.))
    });
    let last = cx.debug_bounds("switcher-card-14").unwrap();
    assert!(last.top() >= grid.top() && last.bottom() <= grid.bottom());
    assert_eq!(destination(&app, cx), 0);
    cx.simulate_click(last.center(), Modifiers::default());
    app.read_with(cx, |app, _| {
        assert_eq!(app.active, 14);
        assert!(app.switcher.is_none());
    });
}

#[gpui::test]
fn leaving_the_window_cancels_and_other_tab_commands_still_switch_directly(
    cx: &mut TestAppContext,
) {
    let (app, cx) = setup(cx, 3);
    hold(cx);
    cx.deactivate_window();
    app.read_with(cx, |app, _| {
        assert_eq!(app.active, 2);
        assert!(app.switcher.is_none());
    });
    cx.update(|window, _| window.activate_window());
    cx.simulate_modifiers_change(Modifiers::default());
    cx.dispatch_action(NextTab);
    app.read_with(cx, |app, _| {
        assert_eq!(app.active, 0);
        assert!(app.switcher.is_none());
    });
    cx.dispatch_action(PreviousTab);
    app.read_with(cx, |app, _| assert_eq!(app.active, 2));
}

#[gpui::test]
fn single_tab_and_pending_close_do_not_open_a_switcher(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx, 1);
    hold(cx);
    app.read_with(cx, |app, _| assert!(app.switcher.is_none()));
    cx.simulate_modifiers_change(Modifiers::default());
    cx.dispatch_action(New);
    cx.simulate_input("Unsaved draft");
    cx.dispatch_action(Close);
    assert!(cx.has_pending_prompt());
    hold(cx);
    app.read_with(cx, |app, _| assert!(app.switcher.is_none()));
    cx.simulate_prompt_answer("Cancel");
    cx.simulate_modifiers_change(Modifiers::default());
}

#[test]
fn file_size_uses_readable_units() {
    assert_eq!(byte_size(0), "0 B");
    assert_eq!(byte_size(1024), "1.0 KB");
    assert_eq!(byte_size(1536), "1.5 KB");
    assert_eq!(byte_size(1024 * 1024), "1.0 MB");
}
