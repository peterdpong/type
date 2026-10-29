use super::{MarkdownEditor, appearance};
use gpui::{
    AnyElement, AvailableSpace, Bounds, Context, MouseButton, PathBuilder, Pixels, Window, canvas,
    div, fill, point, prelude::*, px, rgb, size,
};

pub(super) fn bounds(line: &super::LaidLine) -> Bounds<Pixels> {
    Bounds::new(
        line.origin + point(px(-27.), (line.line_height - px(24.)) / 2.),
        size(px(24.), px(24.)),
    )
}

pub(super) fn toggle(editor: &mut MarkdownEditor, offset: usize, cx: &mut Context<MarkdownEditor>) {
    if editor.document.toggle_checkbox(offset) {
        editor.selecting = false;
        editor.changed(cx);
        editor.reveal_cursor = false;
    }
}

pub(super) fn show(editor: &MarkdownEditor, window: &mut Window, cx: &mut Context<MarkdownEditor>) {
    if editor.marked.is_some() {
        return;
    }
    let viewport = editor.content_bounds();
    for (line, projected) in editor.layout.iter().zip(&editor.projection.lines) {
        let Some(checkbox) = projected.checkbox else {
            continue;
        };
        let bounds = bounds(line);
        if bounds.top() < viewport.top() || bounds.bottom() > viewport.bottom() {
            continue;
        }
        let mut control: AnyElement = div()
            .id(("list-checkbox", checkbox.offset))
            .debug_selector(move || format!("list-checkbox-{}", checkbox.offset))
            .size(px(24.))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |editor, _, window, cx| {
                window.focus(&editor.focus);
                toggle(editor, checkbox.offset, cx);
                cx.stop_propagation();
            }))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        let box_bounds = Bounds::new(
                            bounds.center() - point(px(7.), px(7.)),
                            size(px(14.), px(14.)),
                        );
                        let color = if checkbox.checked {
                            appearance::MUTED
                        } else {
                            0xb3b3b3
                        };
                        window.paint_quad(fill(box_bounds, rgb(color)).corner_radii(px(3.)));
                        if checkbox.checked {
                            let mut path = PathBuilder::stroke(px(1.5));
                            let p = box_bounds.origin;
                            path.move_to(p + point(px(3.), px(7.)));
                            path.line_to(p + point(px(6.), px(10.)));
                            path.line_to(p + point(px(11.), px(4.)));
                            if let Ok(path) = path.build() {
                                window.paint_path(path, rgb(appearance::BACKGROUND));
                            }
                        } else {
                            window.paint_quad(
                                fill(
                                    Bounds::new(
                                        box_bounds.origin + point(px(1.), px(1.)),
                                        size(px(12.), px(12.)),
                                    ),
                                    rgb(appearance::BACKGROUND),
                                )
                                .corner_radii(px(2.)),
                            );
                        }
                    },
                )
                .size_full(),
            )
            .into_any_element();
        control.layout_as_root(AvailableSpace::min_size(), window, cx);
        window.defer_draw(control, bounds.origin, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{Format, ToggleTask, Undo, init};
    use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};

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

    #[gpui::test]
    fn mixed_nested_lists_wrap_at_their_own_text_column(cx: &mut TestAppContext) {
        let source = format!(
            "# Lists\n\n9. Parent\n   - Child\n     1. {}\n     1. Second\n10. Sibling",
            "wrapped words ".repeat(50)
        );
        let (editor, cx) = setup(cx, &source);
        cx.simulate_resize(size(px(500.), px(600.)));
        cx.run_until_parked();
        editor.read_with(cx, |e, _| {
            assert!(e.layout[2].origin.x < e.layout[3].origin.x);
            assert!(e.layout[3].origin.x < e.layout[4].origin.x);
            assert_eq!(e.layout[4].origin.x, e.layout[5].origin.x);
            assert_eq!(e.layout[2].origin.x, e.layout[6].origin.x);
            assert!(e.layout[4].shape.wrap_boundaries().len() > 2);
            assert_eq!(e.projection.lines[5].marker, "2.");
            for wrap in e.layout[4].shape.wrap_boundaries() {
                let index = e.layout[4].shape.runs()[wrap.run_ix].glyphs[wrap.glyph_ix].index;
                assert_eq!(e.layout[4].position(index).x, e.layout[4].origin.x);
            }
            assert_eq!(e.markdown(), source);
        });
    }

    #[gpui::test]
    fn tab_backspace_and_enter_edit_nested_items_as_undoable_steps(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "- First\n- Second");
        cx.simulate_keystrokes("cmd-down tab");
        assert_eq!(
            editor.read_with(cx, |e, _| e.markdown().to_string()),
            "- First\n  - Second"
        );
        cx.simulate_keystrokes("shift-tab");
        assert_eq!(
            editor.read_with(cx, |e, _| e.markdown().to_string()),
            "- First\n- Second"
        );
        cx.simulate_keystrokes("tab home backspace");
        assert_eq!(
            editor.read_with(cx, |e, _| e.markdown().to_string()),
            "- First\n- Second"
        );
        cx.simulate_keystrokes("backspace");
        assert_eq!(
            editor.read_with(cx, |e, _| e.markdown().to_string()),
            "- First\nSecond"
        );
        cx.dispatch_action(Undo);
        cx.simulate_keystrokes("end enter");
        cx.simulate_input("Third");
        assert_eq!(
            editor.read_with(cx, |e, _| e.markdown().to_string()),
            "- First\n- Second\n- Third"
        );
    }

    #[gpui::test]
    fn checkboxes_toggle_exact_source_without_moving_selection_or_scrolling(
        cx: &mut TestAppContext,
    ) {
        let source = "- [ ] **first**\n  1. [X] second\n\nOther text";
        let (editor, cx) = setup(cx, source);
        editor.update_in(cx, |e, _, cx| {
            e.document.select(8, 10);
            cx.notify();
        });
        cx.run_until_parked();
        let before = editor.read_with(cx, |e, _| (e.document.selection(), e.scroll));
        let checkbox = cx.debug_bounds("list-checkbox-3").unwrap();
        cx.simulate_click(checkbox.center(), Modifiers::default());
        cx.run_until_parked();
        editor.read_with(cx, |e, _| {
            assert_eq!(e.markdown(), source.replacen("[ ]", "[x]", 1));
            assert_eq!((e.document.selection(), e.scroll), before);
            assert!(
                e.projection.lines[0]
                    .spans
                    .iter()
                    .all(|span| span.face.strike)
            );
        });
        cx.dispatch_action(Undo);
        assert_eq!(
            editor.read_with(cx, |e, _| e.markdown().to_string()),
            source
        );
        editor.update_in(cx, |e, _, cx| {
            e.move_to(source.find("second").unwrap(), false, cx);
        });
        cx.dispatch_action(ToggleTask);
        assert_eq!(
            editor.read_with(cx, |e, _| e.markdown().to_string()),
            source.replace("[X]", "[ ]")
        );
    }

    #[gpui::test]
    fn formatting_and_typing_create_checklists_without_losing_nesting(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "- Parent\n  - Child");
        cx.simulate_keystrokes("cmd-down");
        editor.update_in(cx, |e, _, cx| e.format(Format::TaskList, cx));
        cx.run_until_parked();
        assert_eq!(
            editor.read_with(cx, |e, _| e.markdown().to_string()),
            "- Parent\n  - [ ] Child"
        );
        cx.simulate_keystrokes("cmd-enter enter");
        cx.simulate_input("Next");
        assert_eq!(
            editor.read_with(cx, |e, _| e.markdown().to_string()),
            "- Parent\n  - [x] Child\n  - [ ] Next"
        );
        cx.simulate_keystrokes("shift-enter");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.layout[2].origin.x, e.layout[3].origin.x);
            assert_eq!(e.projection.lines[3].source_index(0), e.document.cursor());
        });
        cx.simulate_input("continued");
        editor.read_with(cx, |e, _| {
            assert_eq!(e.projection.lines.last().unwrap().text, "continued");
            assert_eq!(e.layout[2].origin.x, e.layout[3].origin.x);
        });
    }
}
