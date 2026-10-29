//! A Markdown editing surface built directly on GPUI.
pub mod appearance;
mod autoformat;
mod code_block;
mod document;
mod editor;
mod extensions;
mod inline;
mod lists;
mod projection;
mod text_input;
mod typing;
pub use text_input::{TextInput, TextInputEvent};

pub use document::Document;
pub use editor::{
    Bold, BulletList, Copy, Cut, Find, FindNext, FindPrevious, Heading1, Heading2, Heading3,
    Heading4, Heading5, Heading6, InlineCode, Italic, OrderedList, Paste, Quote, Redo, SelectAll,
    TaskList, ToggleTask, Undo,
};
pub use editor::{EditorEvent, Format, MarkdownEditor, MarkdownPreview, init};

#[cfg(test)]
mod extensions_tests;
