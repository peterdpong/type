use super::super::{CloseTarget, Workspace};
use crate::{Close, New, Rename, Save};
use gpui::{
    EntityInputHandler, Focusable, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent,
    TestAppContext,
};

fn setup(cx: &mut TestAppContext) -> (gpui::Entity<Workspace>, &mut gpui::VisualTestContext) {
    cx.update(crate::init);
    let (app, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    (app, cx)
}
fn double_click_tab(cx: &mut gpui::VisualTestContext, selector: &'static str) {
    let p = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_click(p, Modifiers::default());
    cx.simulate_event(MouseDownEvent {
        position: p,
        button: MouseButton::Left,
        click_count: 2,
        ..Default::default()
    });
    cx.simulate_event(MouseUpEvent {
        position: p,
        button: MouseButton::Left,
        click_count: 2,
        ..Default::default()
    });
}
fn select_all(cx: &mut gpui::VisualTestContext) {
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    });
}

#[gpui::test]
fn double_click_renames_disk_file_and_keeps_document_edits_and_history(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("note.md");
    let new = dir.path().join("Café 🦀.md");
    std::fs::write(&old, "Saved").unwrap();
    let (app, cx) = setup(cx);
    app.update_in(cx, |app, window, cx| {
        app.open_paths(vec![old.clone()], window, cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("end");
    cx.simulate_input(" draft");
    double_click_tab(cx, "tab-0");
    app.update_in(cx, |app, window, cx| {
        let input = app.rename.as_ref().unwrap().input.clone();
        assert!(input.focus_handle(cx).is_focused(window));
        input.update(cx, |input, cx| {
            assert_eq!(
                input.selected_text_range(false, window, cx).unwrap().range,
                0..4
            )
        });
    });
    cx.simulate_input("Café 🦀");
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "Saved draft")
    });
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(!old.exists());
    assert_eq!(std::fs::read_to_string(&new).unwrap(), "Saved");
    app.update_in(cx, |app, window, cx| {
        assert!(app.rename.is_none());
        assert_eq!(app.active_tab().path, Some(crate::files::identity(&new)));
        assert!(app.active_tab().dirty(cx));
        assert!(app.active_tab().editor.focus_handle(cx).is_focused(window));
    });
    cx.dispatch_action(markdown_core::Undo);
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "Saved draf")
    });
    cx.dispatch_action(markdown_core::Redo);
    cx.dispatch_action(Save);
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(new).unwrap(), "Saved draft");
    assert!(!old.exists());
}

#[gpui::test]
fn renamed_blank_draft_survives_close_cancel_and_can_be_saved(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Project plan.md");
    let (app, cx) = setup(cx);
    cx.simulate_keystrokes("f2");
    cx.simulate_keystrokes("enter");
    app.read_with(cx, |app, cx| {
        assert!(app.active_tab().draft_name.is_none());
        assert!(app.active_tab().empty(cx));
    });
    cx.dispatch_action(Rename);
    cx.simulate_input("Unconfirmed");
    cx.dispatch_action(Close);
    app.update_in(cx, |app, window, cx| {
        assert_eq!(app.active, 0);
        assert!(app.active_tab().empty(cx));
        assert!(app.active_tab().editor.focus_handle(cx).is_focused(window));
    });
    double_click_tab(cx, "tab-0");
    cx.simulate_input("Project plan");
    cx.simulate_keystrokes("enter");
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active_tab().name(), "Project plan.md");
        assert!(app.active_tab().dirty(cx));
        assert!(!app.active_tab().empty(cx));
        assert!(app.active_tab().path.is_none());
    });
    cx.dispatch_action(Close);
    assert!(cx.pending_prompt().unwrap().0.contains("Project plan.md"));
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    cx.dispatch_action(Save);
    cx.simulate_new_path_selection(|_| Some(path.clone()));
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "");
    app.read_with(cx, |app, cx| assert!(!app.active_tab().dirty(cx)));
}

#[gpui::test]
fn escape_click_away_and_switching_tabs_cancel_name_edits(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx);
    cx.simulate_input("Keep the document");
    cx.dispatch_action(Rename);
    cx.simulate_input("Cancel me");
    cx.simulate_keystrokes("escape");
    app.read_with(cx, |app, cx| {
        assert!(app.rename.is_none());
        assert_eq!(app.active_tab().name(), "Untitled");
        assert_eq!(
            app.active_tab().editor.read(cx).markdown(),
            "Keep the document"
        );
    });
    cx.dispatch_action(Rename);
    cx.simulate_input("Not accepted");
    let body = cx.debug_bounds("document").unwrap().center();
    cx.simulate_click(body, Modifiers::default());
    cx.run_until_parked();
    app.read_with(cx, |app, _| assert!(app.rename.is_none()));
    cx.dispatch_action(Rename);
    cx.simulate_input("Leave the app");
    cx.deactivate_window();
    cx.run_until_parked();
    app.read_with(cx, |app, _| assert!(app.rename.is_none()));
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    app.update_in(cx, |app, window, cx| {
        assert!(app.active_tab().editor.focus_handle(cx).is_focused(window));
    });
    cx.dispatch_action(Rename);
    cx.simulate_input("Still not accepted");
    cx.dispatch_action(New);
    app.read_with(cx, |app, _| {
        assert_eq!(app.tabs[0].name(), "Untitled");
        assert_eq!(app.active, 1);
        assert!(app.rename.is_none());
    });
}

#[gpui::test]
fn conflicting_and_invalid_names_leave_files_intact_and_the_field_editable(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("original.md");
    let other = dir.path().join("existing.md");
    std::fs::write(&old, "Original").unwrap();
    std::fs::write(&other, "Other").unwrap();
    let (app, cx) = setup(cx);
    app.update_in(cx, |app, window, cx| {
        app.open_paths(vec![old.clone()], window, cx)
    });
    cx.run_until_parked();
    cx.dispatch_action(Rename);
    select_all(cx);
    cx.simulate_input("existing.md");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active_tab().name(), "original.md");
        assert_eq!(
            app.rename.as_ref().unwrap().input.read(cx).text(),
            "existing.md"
        );
    });
    assert_eq!(std::fs::read_to_string(&old).unwrap(), "Original");
    assert_eq!(std::fs::read_to_string(other).unwrap(), "Other");
    select_all(cx);
    cx.simulate_input("../escape.md");
    cx.simulate_keystrokes("enter");
    app.read_with(cx, |app, _| assert!(app.rename.is_some()));
    assert!(old.exists());
    select_all(cx);
    cx.simulate_input("accepted");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(dir.path().join("accepted.md").exists());
}

#[gpui::test]
fn filename_input_supports_ime_and_its_own_undo_without_markdown_conversion(
    cx: &mut TestAppContext,
) {
    let (app, cx) = setup(cx);
    cx.simulate_input("Document");
    cx.dispatch_action(Rename);
    let input = app.read_with(cx, |app, _| app.rename.as_ref().unwrap().input.clone());
    input.update_in(cx, |input, window, cx| {
        input.replace_and_mark_text_in_range(None, "日🦀", Some(1..3), window, cx)
    });
    cx.simulate_event(gpui::KeyDownEvent {
        keystroke: gpui::Keystroke::parse("enter").unwrap(),
        is_held: false,
    });
    app.read_with(cx, |app, cx| {
        assert!(app.rename.is_some());
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "Document");
    });
    input.update_in(cx, |input, window, cx| {
        input.replace_text_in_range(None, "日本", window, cx)
    });
    cx.dispatch_action(markdown_core::Undo);
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_owned()),
        "Untitled.md"
    );
    select_all(cx);
    cx.simulate_input("**literal**.md");
    cx.simulate_keystrokes("enter");
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active_tab().name(), "**literal**.md");
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "Document");
    });
}

#[gpui::test]
fn rename_completion_does_not_steal_focus_and_close_waits_for_the_move(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("old.md");
    std::fs::write(&old, "Saved").unwrap();
    let (app, cx) = setup(cx);
    app.update_in(cx, |app, window, cx| {
        app.open_paths(vec![old.clone()], window, cx)
    });
    cx.run_until_parked();
    cx.dispatch_action(Rename);
    cx.simulate_input("new");
    app.update_in(cx, |app, window, cx| {
        app.confirm_rename(None, window, cx);
        assert!(app.tabs[0].renaming_to.is_some());
        app.new_tab(window, cx);
        app.request_close(CloseTarget::Tab(0), window, cx);
        assert_eq!(app.tabs.len(), 2);
    });
    cx.run_until_parked();
    assert!(!old.exists());
    assert!(dir.path().join("new.md").exists());
    app.update_in(cx, |app, window, cx| {
        assert_eq!(app.active, 1);
        assert_eq!(app.tabs.len(), 1);
        assert!(app.active_tab().editor.focus_handle(cx).is_focused(window));
    });
}

#[gpui::test]
fn save_while_editing_a_name_confirms_it_before_writing(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("old.md");
    let new = dir.path().join("new.md");
    std::fs::write(&old, "Saved").unwrap();
    let (app, cx) = setup(cx);
    app.update_in(cx, |app, window, cx| {
        app.open_paths(vec![old.clone()], window, cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("end");
    cx.simulate_input(" changes");
    cx.dispatch_action(Rename);
    cx.simulate_input("new");
    cx.dispatch_action(Save);
    cx.run_until_parked();
    assert!(!old.exists());
    assert_eq!(std::fs::read_to_string(new).unwrap(), "Saved changes");
    app.read_with(cx, |app, cx| assert!(!app.active_tab().dirty(cx)));
}
