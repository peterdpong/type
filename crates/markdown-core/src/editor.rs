use gpui::actions;
pub(crate) mod caret;
actions!(markdown_core, [Undo, Redo, Copy, Cut, Paste, SelectAll]);
