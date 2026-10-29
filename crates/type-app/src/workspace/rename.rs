use super::{Kind, TabId, Workspace};
use crate::files;
use gpui::{prelude::*, *};
use markdown_core::{TextInput, TextInputEvent};

pub(super) struct Rename {
    pub id: TabId,
    pub input: Entity<TextInput>,
    _subscription: Subscription,
}

fn stem_end(name: &str) -> usize {
    name.rfind('.').filter(|&i| i > 0).unwrap_or(name.len())
}

impl Workspace {
    pub(super) fn begin_rename(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog_busy
            || self.closing.is_some()
            || self.rename.as_ref().is_some_and(|r| r.id == id)
        {
            return;
        }
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        if tab.busy() {
            return;
        }
        let name = if tab.path.is_none() && tab.draft_name.is_none() {
            format!("{}.md", tab.name())
        } else {
            tab.name()
        };
        self.activate(id, window, cx);
        self.active_tab()
            .editor
            .update(cx, |editor, cx| editor.deactivate(window, cx));
        let input = cx.new(|cx| {
            let mut input = TextInput::new("filename-input", "Filename", cx);
            input.compact();
            input.set_text(&name, 0..stem_end(&name), cx);
            input
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, input, event: &TextInputEvent, window, cx| {
                if !this
                    .rename
                    .as_ref()
                    .is_some_and(|r| r.input.entity_id() == input.entity_id())
                {
                    return;
                }
                match event {
                    TextInputEvent::Submit { .. } => this.confirm_rename(None, window, cx),
                    TextInputEvent::Cancel => this.cancel_rename(true, window, cx),
                    TextInputEvent::Blur => this.cancel_rename(false, window, cx),
                    _ => {}
                }
            },
        );
        window.focus(&input.focus_handle(cx));
        self.rename = Some(Rename {
            id,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(super) fn cancel_rename(
        &mut self,
        restore_focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.rename.take().is_some() {
            if restore_focus {
                self.focus_editor(window, cx);
            }
            cx.notify();
        }
    }

    pub(super) fn confirm_rename(
        &mut self,
        save_after: Option<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(rename) = &self.rename else {
            return;
        };
        if rename.input.read(cx).is_composing() {
            return;
        }
        let id = rename.id;
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        if tab.busy() || self.dialog_busy {
            return;
        }
        let current_name = tab.name();
        let extension = std::path::Path::new(&current_name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("md");
        let name = match files::filename(rename.input.read(cx).text(), extension) {
            Ok(name) => name,
            Err(error) => {
                self.fail("Couldn't rename", error.to_string(), cx);
                return;
            }
        };
        if name == current_name
            || (tab.path.is_none()
                && tab.draft_name.is_none()
                && name == format!("{current_name}.md"))
        {
            self.cancel_rename(true, window, cx);
            if let Some(save_as) = save_after {
                self.save(id, save_as, window, cx);
            }
            return;
        }
        let source = tab.path.clone();
        if let Some(source) = &source {
            let destination = files::identity(&source.with_file_name(&name));
            if self
                .tabs
                .iter()
                .any(|tab| tab.id != id && tab.owns(&destination))
            {
                self.fail(
                    "Couldn't rename",
                    "That filename belongs to another open tab. Choose another name.".into(),
                    cx,
                );
                return;
            }
        }
        self.cancel_rename(true, window, cx);
        let tab = self.tabs.iter_mut().find(|tab| tab.id == id).unwrap();
        let Some(source) = source else {
            tab.draft_name = Some(name);
            cx.notify();
            if let Some(save_as) = save_after {
                self.save(id, save_as, window, cx);
            }
            return;
        };
        tab.renaming_to = Some(source.with_file_name(&name));
        let revision = self.focus_revision;
        self.status(Kind::Progress, "Renaming…", Some(name.clone()), cx);
        let requested = name.clone();
        let task = cx
            .background_executor()
            .spawn(async move { files::rename(&source, &requested) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == id) else {
                    return;
                };
                tab.renaming_to = None;
                match result {
                    Ok(path) => {
                        tab.editor.update(cx, |editor, cx| {
                            editor.set_document_path(Some(path.clone()), cx)
                        });
                        tab.path = Some(path);
                        this.status(Kind::Success, "Renamed", Some(name), cx);
                        if let Some(save_as) = save_after {
                            this.save(id, save_as, window, cx);
                        }
                        this.advance_close(window, cx);
                    }
                    Err(error) => {
                        this.fail("Couldn't rename", format!("{error:#}"), cx);
                        if this.active == id
                            && this.focus_revision == revision
                            && this.rename.is_none()
                            && !this.dialog_busy
                        {
                            this.begin_rename(id, window, cx);
                            if let Some(rename) = &this.rename {
                                rename.input.update(cx, |input, cx| {
                                    input.set_text(&name, 0..stem_end(&name), cx)
                                });
                            }
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests;
