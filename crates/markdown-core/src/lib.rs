//! Native GPUI text-input foundation for Type.
pub mod appearance;
mod document;
mod editor;
mod text_input;
pub use document::Document;
pub use text_input::{TextInput, TextInputEvent};
pub fn init(cx: &mut gpui::App) {
    text_input::init(cx);
}
