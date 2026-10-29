mod files;
mod notice;

use gpui::{prelude::*, *};
use markdown_core::{EditorEvent, MarkdownEditor, appearance};
use notice::{Kind, Notice};
use std::path::PathBuf;

actions!(type_app, [New, Open, Save, SaveAs, Close]);
#[derive(Clone, Copy)]
enum Next {
    New,
    Open,
    Close,
}
struct Workspace {
    editor: Entity<MarkdownEditor>,
    path: Option<PathBuf>,
    saved: String,
    busy: bool,
    loading: bool,
    notice: Entity<Notice>,
    focus: FocusHandle,
    _subscription: Subscription,
}
impl Workspace {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| MarkdownEditor::new("", cx));
        let notice = cx.new(|_| Notice::default());
        let subscription = cx.subscribe(&editor, |_, _, _: &EditorEvent, cx| cx.notify());
        window.focus(&editor.focus_handle(cx));
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| {
                this.request(Next::Close, window, cx);
            })
            .ok();
            false
        });
        Self {
            editor,
            path: None,
            saved: String::new(),
            busy: false,
            loading: false,
            notice,
            focus: cx.focus_handle(),
            _subscription: subscription,
        }
    }
    fn dirty(&self, cx: &App) -> bool {
        self.editor.read(cx).markdown() != self.saved
    }
    fn request(&mut self, next: Next, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if !self.dirty(cx) {
            self.proceed(next, window, cx);
            return;
        }
        self.busy = true;
        let answer = window.prompt(
            PromptLevel::Warning,
            "Save your changes?",
            Some("Your document has unsaved changes."),
            &["Save", "Discard Changes", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.await.unwrap_or(2);
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match answer {
                    0 => this.save(false, Some(next), window, cx),
                    1 => this.proceed(next, window, cx),
                    _ => {
                        window.focus(&this.editor.focus_handle(cx));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
    fn proceed(&mut self, next: Next, window: &mut Window, cx: &mut Context<Self>) {
        self.notice.update(cx, |notice, cx| notice.clear(cx));
        match next {
            Next::New => {
                self.path = None;
                self.editor
                    .update(cx, |editor, cx| editor.set_document_path(None, cx));
                self.saved.clear();
                self.editor
                    .update(cx, |editor, cx| editor.set_markdown("", cx));
                window.focus(&self.editor.focus_handle(cx));
                self.status(Kind::Success, "New document", None, cx);
            }
            Next::Close => {
                window.remove_window();
                cx.quit();
            }
            Next::Open => {
                self.busy = true;
                let paths = cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: false,
                    prompt: Some("Open Markdown".into()),
                });
                cx.spawn_in(window, async move |this, cx| {
                    let result = paths.await;
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.busy = false;
                        match result {
                            Ok(Ok(Some(paths))) => {
                                if let Some(path) = paths.into_iter().next() {
                                    this.open_path(path, window, cx);
                                }
                            }
                            Ok(Ok(None)) => {
                                window.focus(&this.editor.focus_handle(cx));
                                cx.notify();
                            }
                            Ok(Err(error)) => {
                                this.fail("Couldn't open", error.to_string(), window, cx)
                            }
                            Err(error) => this.fail("Couldn't open", error.to_string(), window, cx),
                        }
                    });
                })
                .detach();
            }
        }
        cx.notify();
    }
    fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.busy = true;
        self.loading = true;
        self.status(Kind::Progress, "Opening…", None, cx);
        window.focus(&self.focus);
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { files::read(&path) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                this.loading = false;
                match result {
                    Ok((path, text)) => {
                        this.path = Some(path);
                        this.editor.update(cx, |editor, cx| {
                            editor.set_document_path(this.path.clone(), cx)
                        });
                        this.saved = text.clone();
                        this.editor
                            .update(cx, |editor, cx| editor.set_markdown(text, cx));
                        window.focus(&this.editor.focus_handle(cx));
                        this.status(Kind::Success, "Opened", None, cx);
                        cx.notify();
                    }
                    Err(error) => this.fail("Couldn't open", format!("{error:#}"), window, cx),
                }
            });
        })
        .detach();
    }
    fn save(
        &mut self,
        save_as: bool,
        next: Option<Next>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        if !save_as && self.path.is_some() {
            self.write(self.path.clone().unwrap(), next, window, cx);
            return;
        }
        self.busy = true;
        let dir = self
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let suggested = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|p| p.to_str())
            .unwrap_or("Untitled.md")
            .to_string();
        let path = cx.prompt_for_new_path(&dir, Some(&suggested));
        cx.spawn_in(window, async move |this, cx| {
            let result = path.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(Ok(Some(mut path))) => {
                        if path.extension().is_none() {
                            path.set_extension("md");
                        }
                        this.write(path, next, window, cx);
                    }
                    Ok(Ok(None)) => {
                        window.focus(&this.editor.focus_handle(cx));
                        cx.notify();
                    }
                    Ok(Err(error)) => this.fail("Couldn't save", error.to_string(), window, cx),
                    Err(error) => this.fail("Couldn't save", error.to_string(), window, cx),
                }
            });
        })
        .detach();
    }
    fn write(
        &mut self,
        path: PathBuf,
        next: Option<Next>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.busy = true;
        self.status(Kind::Progress, "Saving…", None, cx);
        window.focus(&self.editor.focus_handle(cx));
        cx.notify();
        let text = self.editor.read(cx).markdown().to_string();
        let same = self
            .path
            .as_ref()
            .is_some_and(|p| *p == path || path.canonicalize().ok().as_ref() == Some(p));
        let expected = same.then(|| self.saved.clone());
        let written = text.clone();
        let task = cx
            .background_executor()
            .spawn(async move { files::save(&path, &written, expected.as_deref()) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(path) => {
                        this.path = Some(path);
                        this.editor.update(cx, |editor, cx| {
                            editor.set_document_path(this.path.clone(), cx)
                        });
                        this.saved = text;
                        window.focus(&this.editor.focus_handle(cx));
                        let message = if this.dirty(cx) {
                            "Saved · new edits remain"
                        } else {
                            "Saved"
                        };
                        this.status(Kind::Success, message, None, cx);
                        if let Some(next) = next {
                            this.request(next, window, cx);
                        }
                        cx.notify();
                    }
                    Err(error) => this.fail("Couldn't save", format!("{error:#}"), window, cx),
                }
            });
        })
        .detach();
    }
    fn status(
        &mut self,
        kind: Kind,
        title: &'static str,
        detail: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.notice
            .update(cx, |notice, cx| notice.show(kind, title, detail, cx));
    }
    fn fail(
        &mut self,
        title: &'static str,
        error: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.busy = false;
        self.loading = false;
        self.status(Kind::Error, title, Some(error), cx);
        window.focus(&self.editor.focus_handle(cx));
        cx.notify();
    }
}
impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dirty = self.dirty(cx);
        let filename = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string());
        window.set_window_title(&format!(
            "{}{} — Type",
            filename,
            if dirty { " •" } else { "" }
        ));
        let words = self.editor.read(cx).word_count();
        let island_left = if cfg!(target_os = "macos") { 96. } else { 20. };
        let island_width =
            ((window.viewport_size().width - px(island_left + 40.)) / 2.).min(px(300.));
        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(appearance::BACKGROUND))
            .text_color(rgb(appearance::TEXT))
            .font_family(appearance::FONT_FAMILY)
            .text_size(px(appearance::UI_SIZE))
            .line_height(relative(1.5))
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &New, w, cx| this.request(Next::New, w, cx)))
            .on_action(cx.listener(|this, _: &Open, w, cx| this.request(Next::Open, w, cx)))
            .on_action(cx.listener(|this, _: &Save, w, cx| this.save(false, None, w, cx)))
            .on_action(cx.listener(|this, _: &SaveAs, w, cx| this.save(true, None, w, cx)))
            .on_action(cx.listener(|this, _: &Close, w, cx| this.request(Next::Close, w, cx)))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .debug_selector(|| "document".into())
                    .child(self.editor.clone())
                    .when(self.loading, |view| {
                        view.child(
                            div()
                                .absolute()
                                .inset_0()
                                .occlude()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .items_center()
                    .h(px(30.))
                    .flex_shrink_0()
                    .px_6()
                    .text_color(rgb(appearance::MUTED))
                    .child(format!(
                        "{words} {}",
                        if words == 1 { "word" } else { "words" }
                    )),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h(px(56.))
                    .window_control_area(WindowControlArea::Drag)
                    .on_mouse_down(MouseButton::Left, |_, window, cx| {
                        window.start_window_move();
                        cx.stop_propagation();
                    }),
            )
            .child(
                div()
                    .id("file-island")
                    .debug_selector(|| "file-island".into())
                    .absolute()
                    .top(px(12.))
                    .left(px(island_left))
                    .max_w(island_width)
                    .h(px(36.))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(appearance::BORDER))
                    .bg(rgb(0xffffff))
                    .window_control_area(WindowControlArea::Drag)
                    .child(div().min_w_0().truncate().child(filename))
                    .when(dirty, |view| {
                        view.child(
                            div()
                                .size(px(5.))
                                .flex_shrink_0()
                                .rounded_full()
                                .bg(rgb(appearance::MUTED)),
                        )
                    }),
            )
            .child(self.notice.clone())
    }
}
fn init(cx: &mut App) {
    markdown_core::init(cx);
    let command = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.bind_keys([
        KeyBinding::new(&format!("{command}-n"), New, None),
        KeyBinding::new(&format!("{command}-o"), Open, None),
        KeyBinding::new(&format!("{command}-s"), Save, None),
        KeyBinding::new(&format!("{command}-shift-s"), SaveAs, None),
        KeyBinding::new(&format!("{command}-w"), Close, None),
        KeyBinding::new(&format!("{command}-q"), Close, None),
    ]);
    cx.set_menus(vec![
        Menu {
            name: "Type".into(),
            items: vec![MenuItem::action("Quit Type", Close)],
        },
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action("New", New),
                MenuItem::action("Open…", Open),
                MenuItem::separator(),
                MenuItem::action("Save", Save),
                MenuItem::action("Save As…", SaveAs),
                MenuItem::separator(),
                MenuItem::action("Close", Close),
            ],
        },
        Menu {
            name: "Edit".into(),
            items: vec![
                MenuItem::action("Undo", markdown_core::Undo),
                MenuItem::action("Redo", markdown_core::Redo),
                MenuItem::separator(),
                MenuItem::action("Cut", markdown_core::Cut),
                MenuItem::action("Copy", markdown_core::Copy),
                MenuItem::action("Paste", markdown_core::Paste),
                MenuItem::action("Select All", markdown_core::SelectAll),
            ],
        },
        Menu {
            name: "Format".into(),
            items: vec![
                MenuItem::action("Bold", markdown_core::Bold),
                MenuItem::action("Italic", markdown_core::Italic),
                MenuItem::action("Inline Code", markdown_core::InlineCode),
                MenuItem::separator(),
                MenuItem::action("Heading 1", markdown_core::Heading1),
                MenuItem::action("Heading 2", markdown_core::Heading2),
                MenuItem::action("Heading 3", markdown_core::Heading3),
                MenuItem::action("Heading 4", markdown_core::Heading4),
                MenuItem::action("Heading 5", markdown_core::Heading5),
                MenuItem::action("Heading 6", markdown_core::Heading6),
                MenuItem::separator(),
                MenuItem::action("Bullet List", markdown_core::BulletList),
                MenuItem::action("Quote", markdown_core::Quote),
            ],
        },
    ]);
}

fn main() {
    let path = std::env::args_os().nth(1).map(PathBuf::from);
    Application::new().run(move |cx: &mut App| {
        init(cx);
        let bounds = Bounds::centered(None, size(px(960.), px(780.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(560.), px(400.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Type".into()),
                    appears_transparent: cfg!(target_os = "macos"),
                    traffic_light_position: cfg!(target_os = "macos")
                        .then_some(point(px(20.), px(20.))),
                }),
                ..Default::default()
            },
            move |window, cx| {
                let view = cx.new(|cx| Workspace::new(window, cx));
                if let Some(path) = path {
                    view.update(cx, |this, cx| this.open_path(path, window, cx));
                }
                view
            },
        )
        .expect("Could not open Type window");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::{New, Save, SaveAs, Workspace};
    use gpui::TestAppContext;
    fn setup(cx: &mut TestAppContext) -> (gpui::Entity<Workspace>, &mut gpui::VisualTestContext) {
        cx.update(super::init);
        cx.add_window_view(Workspace::new)
    }
    #[gpui::test]
    fn cancelling_new_retains_unsaved_document(cx: &mut TestAppContext) {
        let (app, cx) = setup(cx);
        cx.simulate_input("Keep this draft 🦀");
        cx.dispatch_action(New);
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        app.read_with(cx, |app, cx| {
            assert_eq!(app.editor.read(cx).markdown(), "Keep this draft 🦀");
            assert!(app.dirty(cx));
            assert!(!app.busy);
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
            assert!(!app.dirty(cx));
            assert_eq!(app.path, Some(path.canonicalize().unwrap()));
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
            assert_eq!(app.editor.read(cx).markdown(), "Keep this");
            assert!(app.dirty(cx));
            assert!(!app.busy);
        });
    }
    #[gpui::test]
    fn failed_open_retains_document(cx: &mut TestAppContext) {
        let (app, cx) = setup(cx);
        cx.simulate_input("Keep this");
        app.update_in(cx, |app, window, cx| {
            app.open_path(
                std::path::PathBuf::from("/nonexistent-markdown-test.md"),
                window,
                cx,
            )
        });
        cx.run_until_parked();
        app.read_with(cx, |app, cx| {
            assert_eq!(app.editor.read(cx).markdown(), "Keep this");
            assert_eq!(app.notice.read(cx).kind(), Some(super::Kind::Error));
            assert!(app.dirty(cx));
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
            assert!(!app.dirty(cx));
        });
        cx.simulate_input(" ");
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-b"
        } else {
            "ctrl-b"
        });
        cx.simulate_input("bold");
        app.read_with(cx, |app, cx| {
            assert_eq!(app.editor.read(cx).markdown(), "Draft **bold**")
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
            app.write(path.clone(), None, window, cx);
            assert!(app.editor.focus_handle(cx).is_focused(window));
            assert!(!app.loading);
            assert_eq!(app.notice.read(cx).kind(), Some(super::Kind::Progress));
            app.editor
                .update(cx, |editor, cx| editor.set_markdown("Newer edits", cx));
        });
        cx.run_until_parked();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "Snapshot");
        app.read_with(cx, |app, cx| {
            assert_eq!(app.saved, "Snapshot");
            assert_eq!(app.editor.read(cx).markdown(), "Newer edits");
            assert!(app.dirty(cx));
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
            app.open_path(path.clone(), window, cx)
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
            assert_eq!(app.editor.read(cx).markdown(), "Original edited again");
            assert!(app.dirty(cx));
            assert_eq!(app.notice.read(cx).kind(), Some(super::Kind::Error));
        });
    }

    #[gpui::test]
    fn filename_and_notice_fit_a_narrow_window_without_document_reflow(cx: &mut TestAppContext) {
        let (app, cx) = setup(cx);
        cx.simulate_resize(gpui::size(gpui::px(560.), gpui::px(400.)));
        cx.simulate_input("Document stays in place");
        let document = cx.debug_bounds("document").unwrap();
        app.update_in(cx, |app, _, cx| {
            app.path = Some(std::path::PathBuf::from(format!(
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
        let island = cx.debug_bounds("file-island").unwrap();
        let notice = cx.debug_bounds("file-notice").unwrap();
        assert!(
            island.right() < notice.left(),
            "{island:?} overlaps {notice:?}"
        );
        assert!(notice.right() <= gpui::px(540.));
        assert_eq!(island.size.height, gpui::px(36.));
        assert_eq!(cx.debug_bounds("document"), Some(document));
    }
}
