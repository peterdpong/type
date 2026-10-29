use super::image_source;
use crate::editor::{Find, MarkdownEditor, Undo, init};
use gpui::{Entity, ImageSource, Resource, TestAppContext, VisualTestContext, px};
use std::path::Path;

const TABLE: &str = "| Style | Sample | Count |\n| :--- | :---: | ---: |\n| Bold | **Téxt** | 1 |\n| Code | `value` | 2 |";
fn setup<'a>(
    cx: &'a mut TestAppContext,
    source: &str,
) -> (Entity<MarkdownEditor>, &'a mut VisualTestContext) {
    cx.update(init);
    cx.add_window_view(|window, cx| {
        let editor = MarkdownEditor::new(source, cx);
        window.focus(&editor.focus);
        editor
    })
}
fn place(editor: &Entity<MarkdownEditor>, line: usize, cx: &mut VisualTestContext) {
    editor.update_in(cx, |e, _, cx| {
        let source = e.projection.lines[line].source_index(0);
        e.move_to(source, false, cx);
    });
    cx.run_until_parked();
}

#[gpui::test]
fn table_cells_align_hit_test_select_edit_and_undo(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, TABLE);
    editor.read_with(cx, |e, _| {
        assert_eq!(e.table_rows.len(), 3);
        for row in &e.table_rows {
            let y = e.layout[row.cells.start].origin.y;
            for i in row.cells.clone() {
                assert_eq!(e.layout[i].origin.y, y);
                let p = e.layout[i].position(1);
                assert_eq!(e.hit_test(p), e.projection.lines[i].source_index(1));
                for rect in e.layout[i].display_rects(0..e.projection.lines[i].text.len()) {
                    assert!(row.bounds.contains(&rect.origin));
                }
            }
        }
        assert!(
            e.layout[1].origin.x
                > e.table_rows[0].bounds.left() + e.table_rows[0].bounds.size.width / 3. + px(12.)
        );
    });
    place(&editor, 4, cx);
    cx.simulate_keystrokes("tab");
    cx.simulate_input("42");
    editor.read_with(cx, |e, _| {
        assert_eq!(e.markdown(), TABLE.replace("| 1 |", "| 42 |"))
    });
    cx.dispatch_action(Undo);
    cx.dispatch_action(Undo);
    editor.read_with(cx, |e, _| assert_eq!(e.markdown(), TABLE));
    cx.simulate_keystrokes("shift-tab");
    editor.read_with(cx, |e, _| {
        assert_eq!(&e.markdown()[e.document.selection()], "Téxt")
    });
}

#[gpui::test]
fn table_arrows_enter_row_creation_and_exit_preserve_structure(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, TABLE);
    place(&editor, 4, cx);
    cx.simulate_keystrokes("down");
    editor.read_with(cx, |e, _| {
        assert_eq!(e.projection.line_for_source(e.document.cursor()), 7)
    });
    cx.simulate_keystrokes("up");
    editor.read_with(cx, |e, _| {
        assert_eq!(e.projection.line_for_source(e.document.cursor()), 4)
    });
    place(&editor, 8, cx);
    cx.simulate_keystrokes("tab");
    cx.simulate_input("new");
    editor.read_with(cx, |e, _| {
        assert_eq!(e.projection.tables[0].rows.len(), 4);
        assert_eq!(e.projection.lines[9].text, "new");
    });
    for _ in 0..4 {
        cx.dispatch_action(Undo);
    }
    editor.read_with(cx, |e, _| assert_eq!(e.markdown(), TABLE));
    place(&editor, 4, cx);
    cx.simulate_keystrokes("enter");
    editor.read_with(cx, |e, _| {
        assert_eq!(e.projection.line_for_source(e.document.cursor()), 7)
    });
    cx.simulate_keystrokes("cmd-enter");
    cx.simulate_input("After");
    editor.read_with(cx, |e, _| {
        assert!(e.markdown().ends_with("\n\nAfter"));
        assert!(e.projection.lines.last().unwrap().is_plain());
    });
}

#[gpui::test]
fn cell_boundaries_and_literal_pipes_do_not_damage_table_syntax(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, TABLE);
    place(&editor, 4, cx);
    cx.simulate_keystrokes("backspace");
    editor.read_with(cx, |e, _| assert_eq!(e.markdown(), TABLE));
    place(&editor, 7, cx);
    cx.simulate_input("a|🦀");
    editor.read_with(cx, |e, _| {
        assert_eq!(e.projection.tables[0].alignments.len(), 3);
        assert_eq!(e.projection.lines[7].text, "a|🦀value");
        assert!(e.markdown().contains("a\\|🦀"));
    });
}

#[gpui::test]
fn wrapped_cells_and_find_reveal_the_right_row(cx: &mut TestAppContext) {
    let source = format!(
        "| A | B |\n|---|---:|\n| {} | short |\n{}| final **match** | last |",
        "wrapped ".repeat(30),
        "| other | cell |\n".repeat(45)
    );
    let (editor, cx) = setup(cx, &source);
    editor.read_with(cx, |e, _| {
        assert_eq!(e.layout[2].origin.y, e.layout[3].origin.y);
        assert_eq!(e.layout[2].height, e.layout[3].height);
        assert!(e.layout[2].height > e.layout[2].line_height);
    });
    cx.dispatch_action(Find);
    cx.simulate_input("final match");
    editor.read_with(cx, |e, _| {
        assert_eq!(e.find.matches.len(), 1);
        let found = &e.find.matches[0];
        assert!(e.layout[found.line].origin.y >= e.content_bounds().top());
        assert!(e.layout[found.line].origin.y < e.content_bounds().bottom());
        assert_eq!(e.markdown(), source);
    });
}

#[gpui::test]
fn hidden_definitions_and_display_math_have_an_editable_paragraph_after_them(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = setup(cx, "");
    for source in [
        "[id]: https://example.com",
        "<!-- comment -->",
        "$$\na^2 + b^2 = c^2\n$$",
    ] {
        editor.update_in(cx, |e, _, cx| {
            e.set_markdown(source, cx);
            e.move_to(source.len(), false, cx);
        });
        cx.run_until_parked();
        cx.simulate_input("Next");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), format!("{source}\n\nNext"));
            assert!(e.projection.lines.last().unwrap().is_plain());
        });
        for _ in 0..4 {
            cx.dispatch_action(Undo);
        }
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), source));
    }
}

#[test]
fn image_paths_follow_the_document_and_handle_unicode_and_percent_escapes() {
    for (source, expected) in [
        ("./image%20one.png", "/notes/./image one.png"),
        ("写真.png", "/notes/写真.png"),
        ("%aé.png", "/notes/%aé.png"),
        ("/absolute.png", "/absolute.png"),
    ] {
        let ImageSource::Resource(Resource::Path(path)) =
            image_source(source, Some(Path::new("/notes/file.md")))
        else {
            panic!("expected a local image");
        };
        assert_eq!(path.as_ref(), Path::new(expected));
    }
}

#[gpui::test]
fn rendered_replacements_delete_atomically_and_hidden_comments_survive(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, "");
    for (source, marker, expected) in [
        ("A[^n].\n\n[^n]: note", ".", "A.\n\n[^n]: note"),
        ("$\\alpha$.", "$.", "$$."),
        ("x^2^", "^", "x"),
    ] {
        let cursor = if source == "x^2^" {
            3
        } else {
            source.find(marker).unwrap()
        };
        editor.update_in(cx, |e, _, cx| {
            e.set_markdown(source, cx);
            e.move_to(cursor, false, cx);
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("backspace");
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), expected));
        cx.dispatch_action(Undo);
        editor.read_with(cx, |e, _| assert_eq!(e.markdown(), source));
    }
    let source = "A\n\n<!-- preserved -->\n\nB";
    editor.update_in(cx, |e, _, cx| {
        e.set_markdown(source, cx);
        e.move_to(source.len() - 1, false, cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("backspace backspace backspace");
    editor.read_with(cx, |e, _| {
        assert!(e.markdown().contains("<!-- preserved -->"))
    });
}

#[gpui::test]
async fn local_image_loader_reads_the_fixture_and_reports_a_missing_asset(cx: &mut TestAppContext) {
    use gpui::{Asset, ImageAssetLoader};
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let valid = cx.update(|cx| {
        cx.background_executor()
            .clone()
            .spawn(ImageAssetLoader::load(
                directory.join("assets/sample.svg").into(),
                cx,
            ))
    });
    assert!(valid.await.is_ok());
    let missing = cx.update(|cx| {
        cx.background_executor()
            .clone()
            .spawn(ImageAssetLoader::load(
                directory.join("placeholder.png").into(),
                cx,
            ))
    });
    assert!(missing.await.is_err());
}

#[gpui::test]
fn incomplete_rows_materialize_only_when_entered_and_new_tables_start_in_first_cell(
    cx: &mut TestAppContext,
) {
    let source = "| A | B | C |\n|---|---|---|\n| x |\n\nAfter";
    let (editor, cx) = setup(cx, source);
    editor.read_with(cx, |e, _| {
        assert_eq!(e.markdown(), source);
        assert_eq!(e.table_rows.len(), 2);
        assert_eq!(e.table_rows[1].cells.len(), 3);
    });
    place(&editor, 3, cx);
    cx.simulate_keystrokes("tab");
    cx.simulate_input("middle");
    editor.read_with(cx, |e, _| {
        assert_eq!(e.projection.lines[4].text, "middle");
        assert_eq!(e.projection.lines[5].text, "");
        assert!(e.markdown().ends_with("\n\nAfter"));
    });
    let source = "| A | B | C |\n|---|---|---|";
    editor.update_in(cx, |e, _, cx| {
        e.set_markdown(source, cx);
        e.move_to(source.len(), false, cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.simulate_input("first");
    editor.read_with(cx, |e, _| {
        assert_eq!(
            e.projection.lines[3].text,
            "first",
            "{}; cursor {}",
            e.markdown(),
            e.document.cursor()
        )
    });
}

#[gpui::test]
fn typing_reference_definitions_comments_and_setext_underlines_never_loses_input(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = setup(cx, "");
    for source in [
        "[reference]: https://example.com \"A title\"",
        "<!-- keep this source -->",
    ] {
        editor.update_in(cx, |e, _, cx| e.set_markdown("", cx));
        cx.simulate_input(source);
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), source);
            assert!(e.projection.lines.iter().any(|line| line.text == source));
        });
        cx.simulate_keystrokes("enter");
        cx.simulate_input("Body");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), format!("{source}\nBody"));
            assert_eq!(e.projection.lines[0].text, "Body");
        });
    }
    editor.update_in(cx, |e, _, cx| e.set_markdown("Title\n", cx));
    editor.update_in(cx, |e, _, cx| e.move_to(e.markdown().len(), false, cx));
    cx.simulate_input("=====");
    editor.read_with(cx, |e, _| {
        assert_eq!(e.markdown(), "Title\n=====");
        assert_eq!(e.projection.lines[0].heading, 1);
    });
    cx.simulate_keystrokes("enter");
    cx.simulate_input("Body");
    editor.read_with(cx, |e, _| assert_eq!(e.markdown(), "Title\n=====\n\nBody"));
}

#[gpui::test]
fn leaving_tables_reuses_blank_paragraphs_and_preserves_neighboring_blocks(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = setup(cx, "");
    for after in [
        "\n\n",
        "\n\n# Heading",
        "\n\n$$\nx^2\n$$",
        "\n\n| Next |\n|---|\n| table |",
    ] {
        let source = format!("{TABLE}{after}");
        editor.update_in(cx, |e, _, cx| e.set_markdown(&source, cx));
        cx.run_until_parked();
        place(&editor, 8, cx);
        cx.simulate_keystrokes("down");
        editor.read_with(cx, |e, _| {
            assert!(
                e.projection.lines[e.projection.line_for_source(e.document.cursor())].is_plain()
            );
            if after == "\n\n" {
                assert_eq!(e.markdown(), source);
            } else {
                assert!(e.markdown().contains(after.trim_start()));
            }
        });
        cx.simulate_input("Outside");
        editor.read_with(cx, |e, _| {
            assert!(
                e.projection
                    .lines
                    .iter()
                    .any(|line| line.is_plain() && line.text == "Outside")
            )
        });
    }
}
