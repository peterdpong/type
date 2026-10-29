use super::{CloseTarget, Workspace};
use crate::notice::Kind;
use crate::{Close, New, NextTab, PreviousTab, Quit, Save, SaveAs};
use gpui::{Modifiers, TestAppContext, px, size};
fn setup(cx: &mut TestAppContext) -> (gpui::Entity<Workspace>, &mut gpui::VisualTestContext) {
    cx.update(crate::init);
    cx.add_window_view(Workspace::new)
}

#[gpui::test]
fn find_is_per_document_keeps_files_clean_and_leaves_room_for_save_notices(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("find.md");
    std::fs::write(&path, "First alpha").unwrap();
    let (app, cx) = setup(cx);
    app.update_in(cx, |app, window, cx| {
        app.open_paths(vec![path.clone()], window, cx)
    });
    cx.run_until_parked();
    cx.dispatch_action(New);
    cx.simulate_input("Second document");
    cx.dispatch_action(markdown_core::Find);
    cx.simulate_input("Second");
    cx.dispatch_action(NextTab);
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active, 0);
        assert!(
            app.tabs
                .iter()
                .all(|tab| !tab.editor.read(cx).is_find_open())
        );
        assert_eq!(app.tabs[1].editor.read(cx).markdown(), "Second document");
    });
    cx.dispatch_action(markdown_core::Find);
    cx.simulate_input("alpha");
    cx.dispatch_action(Save);
    cx.run_until_parked();
    let find = cx.debug_bounds("document-find").unwrap();
    let notice = cx.debug_bounds("file-notice").unwrap();
    assert!(
        find.bottom() <= notice.top(),
        "{find:?} overlaps {notice:?}"
    );
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "First alpha");
        assert!(!app.active_tab().dirty(cx));
    });
    assert_eq!(std::fs::read_to_string(path).unwrap(), "First alpha");
    cx.simulate_keystrokes("ctrl-tab");
    cx.simulate_modifiers_change(Modifiers::default());
    cx.dispatch_action(markdown_core::Find);
    cx.dispatch_action(markdown_core::Copy);
    cx.update(|_, cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "Second"));
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active, 1);
        assert_eq!(
            app.active_tab().editor.read(cx).markdown(),
            "Second document"
        );
    });
}
#[gpui::test]
fn cancelling_close_retains_unsaved_document(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx);
    cx.simulate_input("Keep this draft 🦀");
    cx.dispatch_action(Close);
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    app.read_with(cx, |app, cx| {
        assert_eq!(
            app.active_tab().editor.read(cx).markdown(),
            "Keep this draft 🦀"
        );
        assert!(app.active_tab().dirty(cx));
        assert!(!app.dialog_busy);
    });
}
#[gpui::test]
fn save_dialog_writes_file_and_clears_dirty_state(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("test.md");
    let (app, cx) = setup(cx);
    cx.simulate_input("# Saved\n\nExact **Markdown** 🦀");
    cx.dispatch_action(Save);
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| Some(path.clone()));
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# Saved\n\nExact **Markdown** 🦀"
    );
    app.read_with(cx, |app, cx| {
        assert!(!app.active_tab().dirty(cx));
        assert_eq!(app.active_tab().path, Some(path.canonicalize().unwrap()));
    });
}
#[gpui::test]
fn cancelling_save_as_retains_changes(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx);
    cx.simulate_input("Keep this");
    cx.dispatch_action(SaveAs);
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "Keep this");
        assert!(app.active_tab().dirty(cx));
        assert!(!app.dialog_busy);
    });
}
#[gpui::test]
fn failed_open_retains_document(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx);
    cx.simulate_input("Keep this");
    app.update_in(cx, |app, window, cx| {
        app.open_paths(
            vec![std::path::PathBuf::from("/nonexistent-markdown-test.md")],
            window,
            cx,
        )
    });
    cx.run_until_parked();
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "Keep this");
        assert_eq!(app.notice.read(cx).kind(), Some(super::Kind::Error));
        assert!(app.active_tab().dirty(cx));
    });
}

#[gpui::test]
fn save_shortcut_shows_feedback_without_moving_the_document_or_focus(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("shortcut.md");
    let (app, cx) = setup(cx);
    cx.simulate_input("Draft");
    let document = cx.debug_bounds("document").unwrap();
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-s"
    } else {
        "ctrl-s"
    });
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| Some(path.clone()));
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "Draft");
    assert_eq!(cx.debug_bounds("document"), Some(document));
    assert!(cx.debug_bounds("file-notice").is_some());
    app.read_with(cx, |app, cx| {
        assert_eq!(app.notice.read(cx).kind(), Some(super::Kind::Success));
        assert!(!app.active_tab().dirty(cx));
    });
    cx.simulate_input(" ");
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-b"
    } else {
        "ctrl-b"
    });
    cx.simulate_input("bold");
    app.read_with(cx, |app, cx| {
        assert_eq!(
            app.active_tab().editor.read(cx).markdown(),
            "Draft **bold**"
        )
    });
}

#[gpui::test]
fn editing_during_save_preserves_new_changes_and_the_saved_baseline(cx: &mut TestAppContext) {
    use gpui::Focusable;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("draft.md");
    let (app, cx) = setup(cx);
    cx.simulate_input("Snapshot");
    app.update_in(cx, |app, window, cx| {
        app.write(app.active, path.clone(), window, cx);
        assert!(app.active_tab().editor.focus_handle(cx).is_focused(window));
        assert!(app.active_tab().saving_to.is_some());
        assert_eq!(app.notice.read(cx).kind(), Some(super::Kind::Progress));
        app.active_tab()
            .editor
            .update(cx, |editor, cx| editor.set_markdown("Newer edits", cx));
    });
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "Snapshot");
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active_tab().saved, "Snapshot");
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "Newer edits");
        assert!(app.active_tab().dirty(cx));
        assert_eq!(app.notice.read(cx).kind(), Some(super::Kind::Success));
    });
}

#[gpui::test]
fn save_conflict_keeps_a_visible_error_and_the_draft_editable(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("conflict.md");
    std::fs::write(&path, "Original").unwrap();
    let (app, cx) = setup(cx);
    app.update_in(cx, |app, window, cx| {
        app.open_paths(vec![path.clone()], window, cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("end");
    cx.simulate_input(" edited");
    std::fs::write(&path, "External changes").unwrap();
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-s"
    } else {
        "ctrl-s"
    });
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(10));
    cx.run_until_parked();
    assert!(cx.debug_bounds("file-notice").is_some());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "External changes");
    cx.simulate_input(" again");
    app.read_with(cx, |app, cx| {
        assert_eq!(
            app.active_tab().editor.read(cx).markdown(),
            "Original edited again"
        );
        assert!(app.active_tab().dirty(cx));
        assert_eq!(app.notice.read(cx).kind(), Some(super::Kind::Error));
    });
}

#[gpui::test]
fn tabs_and_notice_fit_a_narrow_window_without_document_reflow(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx);
    cx.simulate_resize(gpui::size(gpui::px(560.), gpui::px(400.)));
    cx.simulate_input("Document stays in place");
    let document = cx.debug_bounds("document").unwrap();
    app.update_in(cx, |app, _, cx| {
        app.tabs[0].path = Some(std::path::PathBuf::from(format!(
            "{}.md",
            "a-long-filename-".repeat(12)
        )));
        app.status(
            super::Kind::Error,
            "Couldn't save",
            Some("The file changed on disk. Your edits are still here.".into()),
            cx,
        );
        cx.notify();
    });
    cx.run_until_parked();
    let island = cx.debug_bounds("tab-bar").unwrap();
    let notice = cx.debug_bounds("file-notice").unwrap();
    assert!(
        island.bottom() <= notice.top(),
        "{island:?} overlaps {notice:?}"
    );
    assert!(notice.right() <= gpui::px(540.));
    assert_eq!(island.size.height, gpui::px(28.));
    assert_eq!(cx.debug_bounds("document"), Some(document));
}

#[gpui::test]
fn new_tabs_preserve_caret_undo_and_keyboard_focus(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx);
    cx.simulate_input("first");
    cx.simulate_keystrokes("left left");
    cx.dispatch_action(New);
    assert!(!cx.has_pending_prompt());
    cx.simulate_input("second");
    cx.simulate_keystrokes("ctrl-shift-tab");
    cx.simulate_modifiers_change(Modifiers::default());
    cx.simulate_input("!");
    app.read_with(cx, |app, cx| {
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "fir!st");
    });
    cx.dispatch_action(markdown_core::Undo);
    cx.simulate_keystrokes("ctrl-tab");
    cx.simulate_modifiers_change(Modifiers::default());
    app.read_with(cx, |app, cx| {
        assert_eq!(app.tabs[0].editor.read(cx).markdown(), "first");
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "second");
    });
    cx.dispatch_action(markdown_core::Undo);
    app.read_with(cx, |app, cx| {
        assert_eq!(app.tabs[0].editor.read(cx).markdown(), "first");
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "secon");
    });
}

#[gpui::test]
fn tabs_keep_pending_formatting_independent(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx);
    cx.dispatch_action(markdown_core::Bold);
    cx.dispatch_action(New);
    cx.simulate_input("plain");
    cx.dispatch_action(PreviousTab);
    cx.simulate_input("bold");
    app.read_with(cx, |app, cx| {
        assert_eq!(app.tabs[0].editor.read(cx).markdown(), "**bold**");
        assert_eq!(app.tabs[1].editor.read(cx).markdown(), "plain");
    });
}

#[gpui::test]
fn open_many_files_reuses_empty_tab_and_reopening_preserves_edits(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("one.md");
    let second = directory.path().join("two.md");
    std::fs::write(&first, "One").unwrap();
    std::fs::write(&second, "Two").unwrap();
    let (app, cx) = setup(cx);
    app.update_in(cx, |app, window, cx| {
        app.open_paths(vec![first.clone(), second.clone()], window, cx)
    });
    cx.run_until_parked();
    app.read_with(cx, |app, cx| {
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.tabs[0].editor.read(cx).markdown(), "One");
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "Two");
        assert!(app.tabs.iter().all(|tab| !tab.dirty(cx)));
    });
    cx.simulate_keystrokes("end");
    cx.simulate_input(" edited");
    cx.dispatch_action(PreviousTab);
    app.update_in(cx, |app, window, cx| {
        app.open_paths(vec![second.clone(), second], window, cx)
    });
    cx.run_until_parked();
    app.read_with(cx, |app, cx| {
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "Two edited");
        assert!(app.active_tab().dirty(cx));
    });
}

#[gpui::test]
fn opening_while_editing_or_switching_keeps_the_draft_and_focus(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("read.md");
    std::fs::write(&path, "On disk").unwrap();
    let (app, cx) = setup(cx);
    app.update_in(cx, |app, window, cx| {
        app.open_paths(vec![path], window, cx);
        app.tabs[0].editor.update(cx, |editor, cx| {
            editor.set_markdown("Typed during read", cx)
        });
        app.new_tab(window, cx);
    });
    cx.run_until_parked();
    cx.simulate_input("Current draft");
    app.read_with(cx, |app, cx| {
        assert_eq!(app.tabs.len(), 3);
        assert_eq!(app.tabs[0].editor.read(cx).markdown(), "Typed during read");
        assert_eq!(app.active, 1);
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "Current draft");
        assert_eq!(app.tabs[2].editor.read(cx).markdown(), "On disk");
    });
}

#[gpui::test]
fn partial_open_failure_keeps_successful_files_and_unsaved_drafts(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("read.md");
    std::fs::write(&path, "On disk").unwrap();
    let (app, cx) = setup(cx);
    cx.simulate_input("Unsaved draft");
    app.update_in(cx, |app, window, cx| {
        app.open_paths(vec![directory.path().join("missing.md"), path], window, cx)
    });
    cx.run_until_parked();
    app.read_with(cx, |app, cx| {
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.tabs[0].editor.read(cx).markdown(), "Unsaved draft");
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "On disk");
        assert_eq!(app.notice.read(cx).kind(), Some(Kind::Error));
    });
}

#[gpui::test]
fn background_save_updates_its_own_tab_without_stealing_focus(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("first.md");
    let (app, cx) = setup(cx);
    cx.simulate_input("First snapshot");
    app.update_in(cx, |app, window, cx| {
        app.write(0, path.clone(), window, cx);
        app.tabs[0].editor.update(cx, |editor, cx| {
            editor.set_markdown("First with newer edits", cx)
        });
        app.new_tab(window, cx);
    });
    cx.run_until_parked();
    cx.simulate_input("Second draft");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "First snapshot");
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active, 1);
        assert_eq!(app.tabs[0].path, Some(path.canonicalize().unwrap()));
        assert_eq!(app.tabs[0].saved, "First snapshot");
        assert!(app.tabs[0].dirty(cx));
        assert!(app.tabs[1].path.is_none());
        assert_eq!(app.tabs[1].editor.read(cx).markdown(), "Second draft");
    });
}

#[gpui::test]
fn save_as_does_not_overwrite_another_open_file(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("existing.md");
    std::fs::write(&path, "Original").unwrap();
    let (app, cx) = setup(cx);
    app.update_in(cx, |app, window, cx| {
        app.open_paths(vec![path.clone()], window, cx)
    });
    cx.run_until_parked();
    cx.dispatch_action(New);
    cx.simulate_input("Different document");
    cx.dispatch_action(SaveAs);
    cx.simulate_new_path_selection(|_| Some(path.clone()));
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "Original");
    app.read_with(cx, |app, cx| {
        assert_eq!(app.tabs.len(), 2);
        assert!(app.active_tab().path.is_none());
        assert!(app.active_tab().dirty(cx));
        assert_eq!(app.notice.read(cx).kind(), Some(Kind::Error));
    });
}

#[gpui::test]
fn closing_an_inactive_tab_targets_that_file_and_preserves_the_active_draft(
    cx: &mut TestAppContext,
) {
    let (app, cx) = setup(cx);
    cx.simulate_input("First");
    cx.dispatch_action(New);
    cx.simulate_input("Second");
    let close = cx.debug_bounds("close-tab-0").unwrap().center();
    cx.simulate_click(close, Modifiers::default());
    assert!(cx.pending_prompt().unwrap().0.contains("Untitled"));
    cx.simulate_prompt_answer("Discard Changes");
    cx.run_until_parked();
    app.read_with(cx, |app, cx| {
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.active, 1);
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "Second");
        assert!(app.active_tab().dirty(cx));
    });
    cx.simulate_input(" retained");
    app.read_with(cx, |app, cx| {
        assert_eq!(
            app.active_tab().editor.read(cx).markdown(),
            "Second retained"
        )
    });
}

#[gpui::test]
fn close_waits_for_save_and_checks_newer_edits_again(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("first.md");
    let (app, cx) = setup(cx);
    cx.simulate_input("Snapshot");
    app.update_in(cx, |app, window, cx| {
        app.write(0, path.clone(), window, cx);
        app.request_close(CloseTarget::Tab(0), window, cx);
        assert!(app.closing.is_some());
        app.tabs[0]
            .editor
            .update(cx, |editor, cx| editor.set_markdown("Newer edits", cx));
    });
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "Snapshot");
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    app.read_with(cx, |app, cx| {
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.tabs[0].editor.read(cx).markdown(), "Newer edits");
        assert!(app.tabs[0].dirty(cx));
        assert!(app.closing.is_none());
    });
}

#[gpui::test]
fn quit_checks_all_drafts_and_cancel_keeps_even_previously_discarded_tabs(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx);
    cx.simulate_input("First");
    cx.dispatch_action(New);
    cx.simulate_input("Second");
    cx.dispatch_action(Quit);
    assert!(cx.pending_prompt().unwrap().0.contains("Untitled?"));
    cx.simulate_prompt_answer("Discard Changes");
    cx.run_until_parked();
    assert!(cx.pending_prompt().unwrap().0.contains("Untitled 2"));
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    app.read_with(cx, |app, cx| {
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.tabs[0].editor.read(cx).markdown(), "First");
        assert_eq!(app.tabs[1].editor.read(cx).markdown(), "Second");
        assert!(app.tabs.iter().all(|tab| tab.dirty(cx)));
        assert!(app.closing.is_none());
    });
}

#[gpui::test]
fn cancelling_save_during_close_leaves_all_tabs_open(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx);
    cx.simulate_input("First");
    cx.dispatch_action(New);
    cx.simulate_input("Second");
    cx.dispatch_action(Quit);
    cx.simulate_prompt_answer("Save");
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    app.read_with(cx, |app, cx| {
        assert_eq!(app.tabs.len(), 2);
        assert!(app.tabs.iter().all(|tab| tab.dirty(cx)));
        assert!(app.closing.is_none());
        assert!(!app.dialog_busy);
    });
}

#[gpui::test]
fn mouse_tabs_new_button_and_overflow_keep_active_tab_visible(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx);
    cx.simulate_resize(size(px(560.), px(400.)));
    let create = cx.debug_bounds("new-tab").unwrap().center();
    cx.simulate_click(create, Modifiers::default());
    cx.simulate_input("Second");
    let first_tab = cx.debug_bounds("tab-0").unwrap().center();
    cx.simulate_click(first_tab, Modifiers::default());
    cx.simulate_input("First");
    for _ in 0..6 {
        cx.dispatch_action(New);
    }
    cx.run_until_parked();
    let viewport = cx.debug_bounds("tabs").unwrap();
    let active = cx.debug_bounds("tab-7").unwrap();
    assert!(
        active.left() >= viewport.left() && active.right() <= viewport.right(),
        "{active:?} outside {viewport:?}"
    );
    cx.dispatch_action(NextTab);
    cx.run_until_parked();
    let first = cx.debug_bounds("tab-0").unwrap();
    assert!(
        first.left() >= viewport.left() && first.right() <= viewport.right(),
        "{first:?} outside {viewport:?}"
    );
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active, 0);
        assert_eq!(app.tabs[0].editor.read(cx).markdown(), "First");
        assert_eq!(app.tabs[1].editor.read(cx).markdown(), "Second");
    });
}

#[gpui::test]
fn last_empty_untitled_tab_keeps_its_identity_and_undo_history(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx);
    let identity = app.read_with(cx, |app, _| {
        (app.active, app.next_id, app.active_tab().editor.entity_id())
    });
    assert!(cx.debug_bounds("close-tab-0").is_none());
    for _ in 0..3 {
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-w"
        } else {
            "ctrl-w"
        });
    }
    app.read_with(cx, |app, _| {
        assert_eq!(
            (app.active, app.next_id, app.active_tab().editor.entity_id()),
            identity
        );
        assert_eq!(app.active_tab().name(), "Untitled");
        assert!(app.closing.is_none());
    });
    cx.simulate_input("Draft");
    assert!(cx.debug_bounds("close-tab-0").is_some());
    cx.dispatch_action(markdown_core::SelectAll);
    cx.simulate_keystrokes("backspace");
    // GPUI retains debug bounds for removed elements.
    app.read_with(cx, |app, cx| assert!(!app.can_close_tab(0, cx)));
    cx.dispatch_action(Close);
    cx.dispatch_action(markdown_core::Undo);
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "Draft");
        assert_eq!(
            (app.active, app.next_id, app.active_tab().editor.entity_id()),
            identity
        );
    });
}

#[gpui::test]
fn empty_tabs_close_normally_until_only_one_remains(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx);
    cx.dispatch_action(New);
    let close = cx.debug_bounds("close-tab-1").unwrap().center();
    cx.simulate_click(close, Modifiers::default());
    app.read_with(cx, |app, cx| assert!(!app.can_close_tab(0, cx)));
    cx.dispatch_action(New);
    let close = cx.debug_bounds("close-tab-0").unwrap().center();
    cx.simulate_click(close, Modifiers::default());
    cx.dispatch_action(Close);
    app.read_with(cx, |app, _| {
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.active, 2);
        assert_eq!(app.next_id, 3);
        assert_eq!(app.active_tab().name(), "Untitled 3");
    });
    app.read_with(cx, |app, cx| assert!(!app.can_close_tab(2, cx)));
}

#[gpui::test]
fn a_saved_empty_file_can_still_be_closed(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("empty.md");
    std::fs::write(&path, "").unwrap();
    let (app, cx) = setup(cx);
    app.update_in(cx, |app, window, cx| {
        app.open_paths(vec![path.clone()], window, cx)
    });
    cx.run_until_parked();
    let file_id = app.read_with(cx, |app, _| app.active);
    cx.dispatch_action(Close);
    app.read_with(cx, |app, cx| {
        assert!(app.active_tab().empty(cx));
        assert_ne!(app.active, file_id);
        assert_eq!(app.tabs.len(), 1);
    });
    assert_eq!(std::fs::read_to_string(path).unwrap(), "");
}

#[gpui::test]
fn plus_follows_the_last_tab_and_stays_at_the_right_during_overflow(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx);
    cx.simulate_resize(size(px(1000.), px(600.)));
    let header = cx.debug_bounds("window-header").unwrap();
    assert_eq!(header.top(), px(0.));
    assert_eq!(
        header.size,
        size(px(1000.), px(markdown_core::appearance::TOP_GUTTER))
    );
    for (id, selector) in ["tab-0", "tab-1", "tab-2"].into_iter().enumerate() {
        if id > 0 {
            cx.dispatch_action(New);
        }
        let last = cx.debug_bounds(selector).unwrap();
        let plus = cx.debug_bounds("new-tab").unwrap();
        assert_eq!(plus.left(), last.right() + px(4.));
        assert!(plus.right() < cx.debug_bounds("tab-bar").unwrap().right());
    }
    for _ in 0..5 {
        cx.dispatch_action(New);
    }
    let plus = cx.debug_bounds("new-tab").unwrap();
    assert_eq!(plus.right(), cx.debug_bounds("tab-bar").unwrap().right());
    assert_eq!(
        plus.left(),
        cx.debug_bounds("tabs").unwrap().right() + px(4.)
    );
    let last = cx.debug_bounds("tab-7").unwrap();
    assert!(last.right() <= plus.left() - px(4.));
    assert_eq!(cx.debug_bounds("window-header"), Some(header));
    cx.simulate_resize(size(px(2200.), px(600.)));
    let last = cx.debug_bounds("tab-7").unwrap();
    let plus = cx.debug_bounds("new-tab").unwrap();
    assert_eq!(plus.left(), last.right() + px(4.));
    assert!(plus.right() < cx.debug_bounds("tab-bar").unwrap().right());
    app.read_with(cx, |app, _| assert_eq!(app.tab_scroll.offset().x, px(0.)));
    for _ in 0..6 {
        cx.dispatch_action(Close);
    }
    let last = cx.debug_bounds("tab-1").unwrap();
    assert_eq!(
        cx.debug_bounds("new-tab").unwrap().left(),
        last.right() + px(4.)
    );
}

#[gpui::test]
fn closing_last_tab_after_saving_leaves_a_fresh_editable_document(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("saved.md");
    let (app, cx) = setup(cx);
    cx.simulate_input("Keep on disk");
    cx.dispatch_action(Close);
    cx.simulate_prompt_answer("Save");
    cx.run_until_parked();
    cx.simulate_new_path_selection(|_| Some(path.clone()));
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "Keep on disk");
    cx.simulate_input("Fresh draft");
    app.read_with(cx, |app, cx| {
        assert_eq!(app.tabs.len(), 1);
        assert_ne!(app.active, 0);
        assert!(app.active_tab().path.is_none());
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "Fresh draft");
    });
}

#[gpui::test]
fn save_as_dialog_remains_bound_to_its_original_tab(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("first.md");
    let (app, cx) = setup(cx);
    cx.simulate_input("First");
    cx.dispatch_action(New);
    cx.simulate_input("Second");
    cx.dispatch_action(PreviousTab);
    cx.dispatch_action(SaveAs);
    cx.dispatch_action(NextTab);
    cx.simulate_new_path_selection(|_| Some(path.clone()));
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "First");
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active, 1);
        assert_eq!(app.tabs[0].path, Some(path.canonicalize().unwrap()));
        assert!(app.tabs[1].path.is_none());
        assert_eq!(app.tabs[1].editor.read(cx).markdown(), "Second");
    });
}

#[gpui::test]
fn failed_save_aborts_close_and_preserves_draft_and_external_changes(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("conflict.md");
    std::fs::write(&path, "Original").unwrap();
    let (app, cx) = setup(cx);
    app.update_in(cx, |app, window, cx| {
        app.open_paths(vec![path.clone()], window, cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("end");
    cx.simulate_input(" edited");
    std::fs::write(&path, "External changes").unwrap();
    cx.dispatch_action(Close);
    cx.simulate_prompt_answer("Save");
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "External changes");
    app.read_with(cx, |app, cx| {
        assert_eq!(app.tabs.len(), 1);
        assert!(app.closing.is_none());
        assert!(app.active_tab().dirty(cx));
        assert_eq!(
            app.active_tab().editor.read(cx).markdown(),
            "Original edited"
        );
        assert_eq!(app.notice.read(cx).kind(), Some(Kind::Error));
    });
}

#[cfg(unix)]
#[gpui::test]
fn aliases_share_a_tab_and_reopening_a_deleted_file_keeps_the_open_draft(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("actual.md");
    let alias = directory.path().join("alias.md");
    std::fs::write(&path, "Original").unwrap();
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    let (app, cx) = setup(cx);
    app.update_in(cx, |app, window, cx| {
        app.open_paths(vec![path.clone(), alias], window, cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("end");
    cx.simulate_input(" edited");
    cx.dispatch_action(New);
    std::fs::remove_file(&path).unwrap();
    app.update_in(cx, |app, window, cx| app.open_paths(vec![path], window, cx));
    cx.run_until_parked();
    app.read_with(cx, |app, cx| {
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active, 0);
        assert_eq!(
            app.active_tab().editor.read(cx).markdown(),
            "Original edited"
        );
        assert_eq!(app.notice.read(cx).kind(), Some(Kind::Success));
    });
}

#[gpui::test]
fn selection_survives_switching_tabs(cx: &mut TestAppContext) {
    let (app, cx) = setup(cx);
    cx.simulate_input("abcdef");
    cx.simulate_keystrokes("left left shift-left shift-left");
    cx.dispatch_action(New);
    cx.simulate_input("other");
    cx.dispatch_action(PreviousTab);
    cx.simulate_input("X");
    app.read_with(cx, |app, cx| {
        assert_eq!(app.tabs[0].editor.read(cx).markdown(), "abXef");
        assert_eq!(app.tabs[1].editor.read(cx).markdown(), "other");
    });
}

#[gpui::test]
fn equal_filenames_are_distinguished_and_resize_reveals_active_tab(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("one/project/note.md");
    let second = directory.path().join("two/project/note.md");
    for path in [&first, &second] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "Contents").unwrap();
    }
    let (app, cx) = setup(cx);
    cx.simulate_resize(size(px(960.), px(780.)));
    app.update_in(cx, |app, window, cx| {
        app.open_paths(vec![first, second], window, cx)
    });
    cx.run_until_parked();
    app.read_with(cx, |app, _| {
        assert_eq!(app.tab_label(&app.tabs[0]), "note.md · one/project");
        assert_eq!(app.tab_label(&app.tabs[1]), "note.md · two/project");
    });
    for _ in 0..6 {
        cx.dispatch_action(New);
    }
    cx.simulate_resize(size(px(560.), px(400.)));
    let viewport = cx.debug_bounds("tabs").unwrap();
    let active = cx.debug_bounds("tab-7").unwrap();
    assert!(
        active.left() >= viewport.left() && active.right() <= viewport.right(),
        "{active:?} outside {viewport:?}"
    );
    app.update_in(cx, |app, window, cx| {
        app.request_close(CloseTarget::Tab(2), window, cx)
    });
    cx.run_until_parked();
    let active = cx.debug_bounds("tab-7").unwrap();
    assert!(
        active.left() >= viewport.left() && active.right() <= viewport.right(),
        "{active:?} outside {viewport:?}"
    );
}

#[gpui::test]
fn saving_and_closing_commit_composition_before_writing(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler;
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first.md");
    let second = directory.path().join("second.md");
    let (app, cx) = setup(cx);
    for (path, close) in [(first, false), (second, true)] {
        cx.dispatch_action(markdown_core::Bold);
        app.update_in(cx, |app, window, cx| {
            app.active_tab().editor.update(cx, |editor, cx| {
                editor.replace_and_mark_text_in_range(None, "かな", Some(2..2), window, cx);
            });
        });
        if close {
            cx.dispatch_action(Close);
            cx.simulate_prompt_answer("Save");
            cx.run_until_parked();
        } else {
            cx.dispatch_action(Save);
        }
        cx.simulate_new_path_selection(|_| Some(path.clone()));
        cx.run_until_parked();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "**かな**");
        if !close {
            cx.dispatch_action(New);
        }
    }
}

#[gpui::test]
fn rejected_inline_formatting_saves_reopens_and_tracks_dirty_state(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("literal.md");
    let (app, cx) = setup(cx);
    for ch in "**é🦀**".chars() {
        cx.simulate_input(&ch.to_string());
    }
    cx.dispatch_action(markdown_core::Undo);
    let literal = app.read_with(cx, |app, cx| {
        app.active_tab().editor.read(cx).markdown().to_string()
    });
    assert_eq!(literal, r"\*\*é🦀\*\*");
    cx.dispatch_action(Save);
    cx.simulate_new_path_selection(|_| Some(path.clone()));
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), literal);
    assert!(!app.read_with(cx, |app, cx| app.active_tab().dirty(cx)));
    cx.dispatch_action(markdown_core::Redo);
    app.read_with(cx, |app, cx| {
        assert_eq!(app.active_tab().editor.read(cx).markdown(), "**é🦀**");
        assert!(app.active_tab().dirty(cx));
    });
    cx.dispatch_action(markdown_core::Undo);
    assert!(!app.read_with(cx, |app, cx| app.active_tab().dirty(cx)));
    cx.dispatch_action(Close);
    cx.run_until_parked();
    app.update_in(cx, |app, window, cx| app.open_paths(vec![path], window, cx));
    cx.run_until_parked();
    cx.dispatch_action(markdown_core::SelectAll);
    app.read_with(cx, |app, cx| {
        let editor = app.active_tab().editor.read(cx);
        assert_eq!(editor.markdown(), literal);
        assert!(!editor.format_active(markdown_core::Format::Bold));
        assert!(!app.active_tab().dirty(cx));
    });
}
