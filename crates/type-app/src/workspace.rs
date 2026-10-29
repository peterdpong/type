mod motion;
mod rename;
mod switcher;
mod tabs;

use crate::notice::{Kind, Notice};
use crate::{
    Close, CloseWindow, New, NextTab, Open, PreviousTab, Quit, Save, SaveAs, SwitchNextTab,
    SwitchPreviousTab, files,
};
use gpui::{prelude::*, *};
use markdown_core::{EditorEvent, MarkdownEditor, appearance};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

type TabId = u64;

struct Tab {
    id: TabId,
    editor: Entity<MarkdownEditor>,
    path: Option<PathBuf>,
    draft_name: Option<String>,
    renaming_to: Option<PathBuf>,
    saved: String,
    saving_to: Option<PathBuf>,
    _subscription: Subscription,
}

impl Tab {
    fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .or_else(|| self.draft_name.clone())
            .unwrap_or_else(|| {
                if self.id == 0 {
                    "Untitled".into()
                } else {
                    format!("Untitled {}", self.id + 1)
                }
            })
    }

    fn dirty(&self, cx: &App) -> bool {
        (self.path.is_none() && self.draft_name.is_some())
            || self.editor.read(cx).markdown() != self.saved
    }

    fn empty(&self, cx: &App) -> bool {
        self.path.is_none()
            && self.draft_name.is_none()
            && !self.busy()
            && self.editor.read(cx).markdown().is_empty()
    }

    fn busy(&self) -> bool {
        self.saving_to.is_some() || self.renaming_to.is_some()
    }

    fn owns(&self, path: &Path) -> bool {
        self.path.as_deref() == Some(path)
            || self.saving_to.as_deref() == Some(path)
            || self.renaming_to.as_deref() == Some(path)
    }
}

#[derive(Clone, Copy)]
enum CloseTarget {
    Tab(TabId),
    Window,
}

struct Closing {
    target: CloseTarget,
    // Discard approval applies only to this exact document content.
    discarded: HashMap<TabId, String>,
}

pub(crate) struct Workspace {
    tabs: Vec<Tab>,
    active: TabId,
    next_id: TabId,
    focus_revision: u64,
    dialog_busy: bool,
    closing: Option<Closing>,
    tab_scroll: ScrollHandle,
    tab_header: tabs::Header,
    tab_viewport_width: Pixels,
    notice: Entity<Notice>,
    focus: FocusHandle,
    switcher: Option<switcher::TabSwitcher>,
    fading_switcher: Option<switcher::TabSwitcher>,
    rename: Option<rename::Rename>,
    _resize_subscription: Subscription,
    _activation_subscription: Subscription,
}

impl Workspace {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let tab = Self::make_tab(0, None, String::new(), cx);
        window.focus(&tab.editor.focus_handle(cx));
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| {
                this.request_close(CloseTarget::Window, window, cx)
            })
            .ok();
            false
        });
        Self {
            tabs: vec![tab],
            active: 0,
            next_id: 1,
            focus_revision: 0,
            dialog_busy: false,
            closing: None,
            tab_scroll: ScrollHandle::new(),
            tab_header: tabs::Header::default(),
            tab_viewport_width: px(0.),
            notice: cx.new(|_| Notice::default()),
            focus: cx.focus_handle(),
            switcher: None,
            fading_switcher: None,
            rename: None,
            _resize_subscription: cx.observe_window_bounds(window, |_, _, cx| cx.notify()),
            _activation_subscription: cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.finish_tab_drag(true, window, cx);
                    this.cancel_switcher(window, cx);
                    this.cancel_rename(true, window, cx);
                }
            }),
        }
    }

    fn make_tab(id: TabId, path: Option<PathBuf>, text: String, cx: &mut Context<Self>) -> Tab {
        let editor = cx.new(|cx| {
            let mut editor = MarkdownEditor::new(&text, cx);
            editor.set_document_path(path.clone(), cx);
            editor
        });
        let subscription = cx.subscribe(&editor, |_, _, event: &EditorEvent, cx| {
            if matches!(
                event,
                EditorEvent::Changed | EditorEvent::FindVisibilityChanged
            ) {
                cx.notify();
            }
        });
        Tab {
            id,
            editor,
            path,
            draft_name: None,
            renaming_to: None,
            saved: text,
            saving_to: None,
            _subscription: subscription,
        }
    }

    fn active_tab(&self) -> &Tab {
        self.tabs.iter().find(|tab| tab.id == self.active).unwrap()
    }

    fn focus_editor(&self, window: &mut Window, cx: &App) {
        if self.rename.is_none() && !self.tab_header.is_dragging() {
            window.focus(&self.active_tab().editor.focus_handle(cx));
        }
    }

    fn activate(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tabs.iter().any(|tab| tab.id == id) {
            return;
        }
        self.finish_tab_drag(true, window, cx);
        self.cancel_rename(false, window, cx);
        self.dismiss_switcher(cx);
        if self.active != id {
            self.active_tab()
                .editor
                .update(cx, |editor, cx| editor.deactivate(window, cx));
        }
        self.active = id;
        self.focus_revision += 1;
        self.reveal_active_tab(window);
        self.focus_editor(window, cx);
        cx.notify();
    }

    fn cycle(&mut self, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        let index = self
            .tabs
            .iter()
            .position(|tab| tab.id == self.active)
            .unwrap();
        let offset = if backwards { self.tabs.len() - 1 } else { 1 };
        self.activate(self.tabs[(index + offset) % self.tabs.len()].id, window, cx);
    }

    fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog_busy || self.closing.is_some() {
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.notice.update(cx, |notice, cx| notice.clear(cx));
        self.tabs.push(Self::make_tab(id, None, String::new(), cx));
        self.activate(id, window, cx);
    }

    fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_tab_drag(true, window, cx);
        self.cancel_rename(false, window, cx);
        if self.dialog_busy || self.closing.is_some() {
            return;
        }
        self.cancel_switcher(window, cx);
        self.dialog_busy = true;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Open Markdown".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = paths.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.dialog_busy = false;
                match result {
                    Ok(Ok(Some(paths))) => this.open_paths(paths, window, cx),
                    Ok(Ok(None)) => this.focus_editor(window, cx),
                    Ok(Err(error)) => this.fail("Couldn't open", error.to_string(), cx),
                    Err(error) => this.fail("Couldn't open", error.to_string(), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn open_paths(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        let mut reuse = self.active_tab().empty(cx).then_some(self.active);
        self.focus_revision += 1;
        let revision = self.focus_revision;
        self.status(Kind::Progress, "Opening…", None, cx);
        let task = cx.background_executor().spawn(async move {
            paths
                .into_iter()
                .map(|path| {
                    let identity = files::identity(&path);
                    let result = files::read(&path);
                    (identity, result)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn_in(window, async move |this, cx| {
            let results = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let mut last = None;
                let mut errors = Vec::new();
                for (requested, result) in results {
                    if let Some(tab) = this.tabs.iter().find(|tab| tab.owns(&requested)) {
                        last = Some(tab.id);
                        continue;
                    }
                    match result {
                        Ok((path, text)) => {
                            if let Some(tab) = this.tabs.iter().find(|tab| tab.owns(&path)) {
                                last = Some(tab.id);
                                continue;
                            }
                            if let Some(index) = reuse.and_then(|id| {
                                this.tabs
                                    .iter()
                                    .position(|tab| tab.id == id && tab.empty(cx))
                            }) {
                                let id = this.tabs[index].id;
                                this.tabs[index] = Self::make_tab(id, Some(path), text, cx);
                                reuse = None;
                                last = Some(id);
                            } else {
                                let id = this.next_id;
                                this.next_id += 1;
                                this.tabs.push(Self::make_tab(id, Some(path), text, cx));
                                last = Some(id);
                            }
                        }
                        Err(error) => errors.push(format!("{}: {error:#}", requested.display())),
                    }
                }
                if revision == this.focus_revision
                    && let Some(id) = last
                {
                    this.activate(id, window, cx);
                } else {
                    this.focus_editor(window, cx);
                }
                if errors.is_empty() {
                    this.status(Kind::Success, "Opened", None, cx);
                } else {
                    this.status(Kind::Error, "Couldn't open", Some(errors.join("\n")), cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn save(&mut self, id: TabId, save_as: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_tab_drag(true, window, cx);
        if self.rename.is_some() {
            self.confirm_rename(Some(save_as), window, cx);
            return;
        }
        if self.dialog_busy {
            return;
        }
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        if tab.busy() {
            return;
        }
        tab.editor
            .update(cx, |editor, cx| editor.finish_composition(window, cx));
        if !save_as && let Some(path) = &tab.path {
            self.write(id, path.clone(), window, cx);
            return;
        }
        let dir = tab
            .path
            .as_ref()
            .and_then(|path| path.parent())
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let suggested = if tab.path.is_some() || tab.draft_name.is_some() {
            tab.name()
        } else {
            format!("{}.md", tab.name())
        };
        self.dialog_busy = true;
        let path = cx.prompt_for_new_path(&dir, Some(&suggested));
        cx.spawn_in(window, async move |this, cx| {
            let result = path.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.dialog_busy = false;
                match result {
                    Ok(Ok(Some(mut path))) => {
                        if path.extension().is_none() {
                            path.set_extension("md");
                        }
                        this.write(id, path, window, cx);
                    }
                    Ok(Ok(None)) => {
                        this.closing = None;
                    }
                    Ok(Err(error)) => this.fail("Couldn't save", error.to_string(), cx),
                    Err(error) => this.fail("Couldn't save", error.to_string(), cx),
                }
                this.focus_editor(window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn write(&mut self, id: TabId, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let path = files::identity(&path);
        if self.tabs.iter().any(|tab| tab.id != id && tab.owns(&path)) {
            self.fail("Couldn't save", "That file is already open in another tab. Switch to it, or choose a different filename.".into(), cx);
            return;
        }
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        if tab.busy() {
            return;
        }
        let text = tab.editor.read(cx).markdown().to_string();
        let expected = (tab.path.as_ref() == Some(&path)).then(|| tab.saved.clone());
        tab.saving_to = Some(path.clone());
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        self.status(Kind::Progress, "Saving…", name, cx);
        let written = text.clone();
        let task = cx
            .background_executor()
            .spawn(async move { files::save(&path, &written, expected.as_deref()) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == id) else {
                    return;
                };
                tab.saving_to = None;
                match result {
                    Ok(path) => {
                        tab.editor.update(cx, |editor, cx| {
                            editor.set_document_path(Some(path.clone()), cx)
                        });
                        tab.path = Some(path);
                        tab.draft_name = None;
                        tab.saved = text;
                        let message = if tab.dirty(cx) {
                            "Saved · new edits remain"
                        } else {
                            "Saved"
                        };
                        let name = tab.name();
                        this.status(Kind::Success, message, Some(name), cx);
                        this.advance_close(window, cx);
                    }
                    Err(error) => {
                        let name = tab.name();
                        this.fail("Couldn't save", format!("{name}: {error:#}"), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn request_close(&mut self, target: CloseTarget, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_tab_drag(true, window, cx);
        self.cancel_rename(true, window, cx);
        self.cancel_switcher(window, cx);
        if self.dialog_busy || self.closing.is_some() {
            return;
        }
        for tab in self.tabs.iter().filter(|tab| match target {
            CloseTarget::Tab(id) => tab.id == id,
            CloseTarget::Window => true,
        }) {
            tab.editor
                .update(cx, |editor, cx| editor.finish_composition(window, cx));
        }
        if let CloseTarget::Tab(id) = target
            && !self.can_close_tab(id, cx)
        {
            return;
        }
        self.closing = Some(Closing {
            target,
            discarded: HashMap::new(),
        });
        self.advance_close(window, cx);
    }

    fn can_close_tab(&self, id: TabId, cx: &App) -> bool {
        self.tabs
            .iter()
            .find(|tab| tab.id == id)
            .is_some_and(|tab| self.tabs.len() > 1 || !tab.empty(cx))
    }

    fn advance_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog_busy {
            return;
        }
        let Some(closing) = &self.closing else { return };
        let affected = self
            .tabs
            .iter()
            .filter(|tab| match closing.target {
                CloseTarget::Tab(id) => tab.id == id,
                CloseTarget::Window => true,
            })
            .collect::<Vec<_>>();
        if affected.iter().any(|tab| tab.busy()) {
            return;
        }
        let dirty = affected.into_iter().find(|tab| {
            tab.dirty(cx)
                && closing.discarded.get(&tab.id).map(String::as_str)
                    != Some(tab.editor.read(cx).markdown())
        });
        if let Some(tab) = dirty {
            let id = tab.id;
            let text = tab.editor.read(cx).markdown().to_string();
            let name = tab.name();
            self.activate(id, window, cx);
            self.dialog_busy = true;
            let answer = window.prompt(
                PromptLevel::Warning,
                &format!("Save changes to {name}?"),
                Some("Your changes will be lost if you close this document."),
                &["Save", "Discard Changes", "Cancel"],
                cx,
            );
            cx.spawn_in(window, async move |this, cx| {
                let answer = answer.await.unwrap_or(2);
                let _ = this.update_in(cx, |this, window, cx| {
                    this.dialog_busy = false;
                    match answer {
                        0 => this.save(id, false, window, cx),
                        1 => {
                            if let Some(closing) = &mut this.closing {
                                closing.discarded.insert(id, text);
                            }
                            this.advance_close(window, cx);
                        }
                        _ => {
                            this.closing = None;
                            this.focus_editor(window, cx);
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
            return;
        }
        let target = self.closing.take().unwrap().target;
        match target {
            CloseTarget::Window => {
                window.remove_window();
                cx.quit();
            }
            CloseTarget::Tab(id) => {
                if let Some(index) = self.tabs.iter().position(|tab| tab.id == id) {
                    self.tabs.remove(index);
                    if self.tabs.is_empty() {
                        let id = self.next_id;
                        self.next_id += 1;
                        self.tabs.push(Self::make_tab(id, None, String::new(), cx));
                    }
                    if self.active == id {
                        self.active = self.tabs[index.min(self.tabs.len() - 1)].id;
                        self.activate(self.active, window, cx);
                    }
                    self.reveal_active_tab(window);
                    self.focus_editor(window, cx);
                    cx.notify();
                }
            }
        }
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

    fn fail(&mut self, title: &'static str, error: String, cx: &mut Context<Self>) {
        self.closing = None;
        self.status(Kind::Error, title, Some(error), cx);
        cx.notify();
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.tab_viewport_width != window.viewport_size().width {
            self.finish_tab_drag(true, window, cx);
            self.tab_viewport_width = window.viewport_size().width;
            self.reveal_active_tab(window);
        }
        self.prepare_tab_header(window, cx);
        let tab = self.active_tab();
        window.set_window_title(&format!(
            "{}{} — Type",
            tab.name(),
            if tab.dirty(cx) { " •" } else { "" }
        ));
        let words = tab.editor.read(cx).word_count();
        let finding = tab.editor.read(cx).is_find_open();
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
            .on_modifiers_changed(
                cx.listener(|this, event: &ModifiersChangedEvent, window, cx| {
                    if !event.modifiers.control {
                        this.commit_switcher(window, cx);
                    }
                }),
            )
            .on_key_down(cx.listener(|this, event, window, cx| {
                if !this.tab_drag_key_down(event, window, cx) {
                    this.switcher_key_down(event, window, cx);
                }
            }))
            .on_action(
                cx.listener(|this, _: &crate::Rename, w, cx| this.begin_rename(this.active, w, cx)),
            )
            .on_action(cx.listener(|this, _: &New, w, cx| this.new_tab(w, cx)))
            .on_action(cx.listener(|this, _: &Open, w, cx| this.open(w, cx)))
            .on_action(cx.listener(|this, _: &Save, w, cx| {
                this.cancel_switcher(w, cx);
                this.save(this.active, false, w, cx)
            }))
            .on_action(cx.listener(|this, _: &SaveAs, w, cx| {
                this.cancel_switcher(w, cx);
                this.save(this.active, true, w, cx)
            }))
            .on_action(cx.listener(|this, _: &Close, w, cx| {
                this.request_close(CloseTarget::Tab(this.active), w, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseWindow, w, cx| {
                this.request_close(CloseTarget::Window, w, cx)
            }))
            .on_action(
                cx.listener(|this, _: &Quit, w, cx| this.request_close(CloseTarget::Window, w, cx)),
            )
            .on_action(cx.listener(|this, _: &NextTab, w, cx| this.cycle(false, w, cx)))
            .on_action(cx.listener(|this, _: &PreviousTab, w, cx| this.cycle(true, w, cx)))
            .on_action(
                cx.listener(|this, _: &SwitchNextTab, w, cx| this.step_switcher(false, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SwitchPreviousTab, w, cx| this.step_switcher(true, w, cx)),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .debug_selector(|| "document".into())
                    .child(tab.editor.clone()),
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
                    .debug_selector(|| "window-header".into())
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h(px(appearance::TOP_GUTTER))
                    .bg(rgb(appearance::CHROME))
                    .border_b_1()
                    .border_color(rgb(appearance::CHROME_BORDER))
                    .window_control_area(WindowControlArea::Drag)
                    .on_mouse_down(MouseButton::Left, |_, window, cx| {
                        window.start_window_move();
                        cx.stop_propagation();
                    }),
            )
            .child(self.tab_bar(window, cx))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .top(px(if finding { 52. } else { 0. }))
                    .child(self.notice.clone()),
            )
            .children(self.render_switcher(window, cx))
    }
}

#[cfg(test)]
mod tests;
