//! Cached document blur for the outline background.
#[cfg(target_os = "macos")]
mod native;

use crate::editor::MarkdownEditor;
use gpui::{Bounds, Context, Pixels, RenderImage, Task, Window};
use std::{ops::Range, sync::Arc};

#[derive(Clone, PartialEq)]
struct Key {
    layout: u64,
    selection: Range<usize>,
    bounds: Bounds<Pixels>,
}

#[derive(Default)]
pub(super) struct Backdrop {
    key: Option<Key>,
    image: Option<(Bounds<Pixels>, Arc<RenderImage>)>,
    retired: Vec<Arc<RenderImage>>,
    task: Option<Task<()>>,
}

impl Backdrop {
    #[cfg(test)]
    pub(super) fn image_id(&self) -> Option<gpui::ImageId> {
        self.image.as_ref().map(|(_, image)| image.id)
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.image.is_none() && self.task.is_none() && self.retired.is_empty()
    }

    pub fn invalidate(&mut self) {
        self.task = None;
        self.key = None;
        if let Some((_, image)) = self.image.take() {
            self.retired.push(image);
        }
    }

    pub fn collect(&mut self, window: &mut Window) {
        for image in self.retired.drain(..) {
            let _ = window.drop_image(image);
        }
    }

    pub fn clear(&mut self, window: &mut Window) {
        self.invalidate();
        self.collect(window);
    }
}

pub(super) fn prepare(
    editor: &mut MarkdownEditor,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut Context<MarkdownEditor>,
) -> Option<Arc<RenderImage>> {
    editor.outline.backdrop.collect(window);
    #[cfg(target_os = "macos")]
    {
        let key = Key {
            layout: editor.layout_revision,
            selection: editor.document.selection(),
            bounds,
        };
        if editor.outline.backdrop.key.as_ref() != Some(&key) {
            let snapshot = native::Snapshot::capture(editor, bounds);
            editor.outline.backdrop.key = Some(key.clone());
            let task = cx
                .background_executor()
                .spawn(async move { native::render(snapshot) });
            editor.outline.backdrop.task = Some(cx.spawn(async move |this, cx| {
                let result = task.await;
                let _ = this.update(cx, |editor, cx| {
                    let backdrop = &mut editor.outline.backdrop;
                    if backdrop.key.as_ref() != Some(&key) {
                        return;
                    }
                    if let Some(image) = result {
                        if let Some((_, old)) = backdrop.image.replace((bounds, Arc::new(image))) {
                            backdrop.retired.push(old);
                        }
                        cx.notify();
                    }
                });
            }));
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = cx;
    editor
        .outline
        .backdrop
        .image
        .as_ref()
        .filter(|(region, _)| *region == bounds)
        .map(|(_, image)| image.clone())
}
