use crate::editor::{Find, FindNext, MarkdownEditor, SelectAll, Undo, init};
use gpui::{
    Entity, EntityInputHandler, KeyDownEvent, Keystroke, Modifiers, TestAppContext,
    VisualTestContext, px, size,
};

fn setup<'a>(
    cx: &'a mut TestAppContext,
    source: &str,
) -> (Entity<MarkdownEditor>, &'a mut VisualTestContext) {
    cx.update(init);
    let (editor, cx) = cx.add_window_view(|window, cx| {
        let editor = MarkdownEditor::new(source, cx);
        window.focus(&editor.focus);
        editor
    });
    cx.update(|window, _| window.activate_window());
    (editor, cx)
}

#[gpui::test]
fn find_searches_visible_styles_and_code_with_case_toggle_and_wraparound(cx: &mut TestAppContext) {
    let source = "# Alpha\n\nALPHA **alpha** and `alpha`.\n\n```rs\nAlpha();\n```";
    let (editor, cx) = setup(cx, source);
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-f"
    } else {
        "ctrl-f"
    });
    cx.simulate_input("alpha");
    editor.read_with(cx, |editor, _| {
        assert!(editor.find.open);
        assert_eq!(editor.find.query, "alpha");
        assert_eq!(editor.find.matches.len(), 5);
        assert_eq!(editor.find.active, Some(0));
        assert_eq!(editor.markdown(), source);
        for found in &editor.find.matches {
            let line = &editor.projection.lines[found.line];
            assert_eq!(line.text[found.range.clone()].to_lowercase(), "alpha");
            assert!(
                !editor.layout[found.line]
                    .display_rects(found.range.clone())
                    .is_empty()
            );
        }
    });
    cx.simulate_keystrokes("enter");
    editor.read_with(cx, |editor, _| assert_eq!(editor.find.active, Some(1)));
    cx.simulate_keystrokes("shift-enter shift-enter");
    editor.read_with(cx, |editor, _| assert_eq!(editor.find.active, Some(4)));
    let toggle = cx.debug_bounds("find-case").unwrap().center();
    cx.simulate_click(toggle, Modifiers::default());
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.find.matches.len(), 2);
        assert!(editor.find.case_sensitive);
    });
    let next = cx.debug_bounds("find-next").unwrap().center();
    cx.simulate_click(next, Modifiers::default());
    editor.read_with(cx, |editor, _| assert_eq!(editor.find.active, Some(1)));
    cx.simulate_keystrokes("escape");
    editor.update_in(cx, |editor, window, _| {
        assert!(!editor.find.open);
        assert!(editor.find.matches.is_empty());
        assert!(editor.focus.is_focused(window));
        assert!(!editor.document.selection().is_empty());
        assert_eq!(editor.markdown(), source);
        assert!(!editor.document.undo());
    });
}

#[gpui::test]
fn query_editing_and_undo_do_not_edit_or_format_the_document(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, "alpha beta");
    cx.simulate_input("!");
    let source = editor.read_with(cx, |editor, _| editor.markdown().to_owned());
    cx.dispatch_action(Find);
    cx.simulate_input("alpha");
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-b cmd-z"
    } else {
        "ctrl-b ctrl-z"
    });
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.find.query, "alph");
        assert_eq!(editor.markdown(), source);
    });
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    });
    cx.simulate_input("missing");
    cx.simulate_keystrokes("enter shift-enter");
    editor.read_with(cx, |editor, _| {
        assert!(editor.find.matches.is_empty());
        assert_eq!(editor.find.active, None);
        assert_eq!(editor.markdown(), source);
    });
    cx.dispatch_action(SelectAll);
    cx.simulate_keystrokes("backspace enter");
    editor.read_with(cx, |editor, _| {
        assert!(editor.find.query.is_empty());
        assert!(editor.find.matches.is_empty());
    });
    cx.simulate_keystrokes("escape");
    cx.dispatch_action(Undo);
    editor.read_with(cx, |editor, _| assert_eq!(editor.markdown(), "alpha beta"));
}

#[gpui::test]
fn find_seeds_visible_selection_and_treats_queries_as_literal_unicode(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, "**Café** CAFÉ café . ( a[0] 🦀");
    editor.update_in(cx, |editor, _, cx| {
        editor.document.select(0, "**Café**".len());
        cx.notify();
    });
    cx.dispatch_action(Find);
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.find.query, "Café");
        assert_eq!(editor.find.matches.len(), 3);
    });
    for query in [".", "(", "a[0]", "🦀"] {
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-f"
        } else {
            "ctrl-f"
        });
        cx.simulate_input(query);
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.find.query, query);
            assert_eq!(editor.find.matches.len(), 1);
            let found = &editor.find.matches[0];
            assert_eq!(
                &editor.projection.lines[found.line].text[found.range.clone()],
                query
            );
            assert!(editor.markdown().is_char_boundary(found.source.start));
            assert!(editor.markdown().is_char_boundary(found.source.end));
        });
    }
}

#[gpui::test]
fn native_ime_query_and_grapheme_deletion_stay_in_the_search_field(cx: &mut TestAppContext) {
    let source = "日本 日🦀 é é 👨‍👩‍👧‍👦";
    let (editor, cx) = setup(cx, source);
    cx.dispatch_action(Find);
    let input = editor.read_with(cx, |editor, _| editor.find.input.clone());
    input.update_in(cx, |input, window, cx| {
        input.replace_and_mark_text_in_range(None, "日🦀", Some(1..3), window, cx);
        assert_eq!(
            input.selected_text_range(false, window, cx).unwrap().range,
            1..3
        );
        assert_eq!(input.marked_text_range(window, cx), Some(0..3));
    });
    cx.run_until_parked();
    // simulate_keystrokes would also insert a newline before IME commit.
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
        is_held: false,
    });
    editor.read_with(cx, |editor, _| assert_eq!(editor.markdown(), source));
    input.update_in(cx, |input, window, cx| {
        input.replace_text_in_range(None, "日本", window, cx)
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.find.query, "日本");
        assert_eq!(editor.find.matches.len(), 1);
        assert_eq!(editor.markdown(), source);
    });
    cx.dispatch_action(Undo);
    editor.read_with(cx, |editor, _| assert_eq!(editor.find.query, ""));
    cx.simulate_input("é👨‍👩‍👧‍👦");
    cx.simulate_keystrokes("backspace");
    editor.read_with(cx, |editor, _| assert_eq!(editor.find.query, "é"));
    cx.simulate_keystrokes("backspace");
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.find.query, "");
        assert_eq!(editor.markdown(), source);
    });
}

#[gpui::test]
fn offscreen_and_wrapped_matches_are_visible_below_the_find_panel(cx: &mut TestAppContext) {
    let source = format!("target\n\n{}\ntarget", "Some body text.\n".repeat(90));
    let (editor, cx) = setup(cx, &source);
    cx.simulate_resize(size(px(560.), px(400.)));
    cx.dispatch_action(Find);
    cx.simulate_input("target");
    let panel = cx.debug_bounds("document-find").unwrap();
    assert!(panel.left() >= px(20.) && panel.right() <= px(540.));
    editor.read_with(cx, |editor, _| {
        assert!(editor.layout[0].origin.y >= panel.bottom())
    });
    cx.dispatch_action(FindNext);
    editor.read_with(cx, |editor, _| {
        let found = &editor.find.matches[editor.find.active.unwrap()];
        let line = &editor.layout[found.line];
        assert!(editor.scroll > px(0.));
        let rect = line.display_rects(found.range.clone())[0];
        assert!(rect.top() >= editor.content_bounds().top());
        assert!(rect.bottom() <= editor.bounds.bottom());
    });
    let query = "alpha beta gamma ".repeat(20);
    editor.update_in(cx, |editor, _, cx| editor.set_markdown(query.clone(), cx));
    cx.dispatch_action(Find);
    let input = editor.read_with(cx, |editor, _| editor.find.input.clone());
    input.update_in(cx, |input, window, cx| {
        input.replace_text_in_range(None, &query, window, cx)
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.find.matches.len(), 1);
        let found = &editor.find.matches[0];
        assert!(
            editor.layout[found.line]
                .display_rects(found.range.clone())
                .len()
                > 1
        );
    });
    let field = cx.debug_bounds("find-input").unwrap();
    input.update_in(cx, |input, window, cx| {
        let cursor = input.selected_text_range(false, window, cx).unwrap().range;
        let caret = input.bounds_for_range(cursor, field, window, cx).unwrap();
        assert!(caret.left() >= field.left() && caret.right() <= field.right());
    });
}

#[gpui::test]
fn matches_refresh_when_editing_the_document_and_undoing(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, "alpha");
    cx.dispatch_action(Find);
    cx.simulate_input("alpha");
    editor.update_in(cx, |editor, window, cx| {
        editor.move_to(editor.markdown().len(), false, cx);
        window.focus(&editor.focus);
    });
    cx.simulate_input(" alpha");
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.find.matches.len(), 2);
        assert_eq!(editor.find.query, "alpha");
        assert_eq!(editor.markdown(), "alpha alpha");
    });
    cx.dispatch_action(Undo);
    editor.read_with(cx, |editor, _| assert_eq!(editor.find.matches.len(), 1));
}
