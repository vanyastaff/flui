//! Text widgets — single-line display and editing over `flui-objects`'
//! `RenderParagraph`.

mod text;

pub mod controller;
pub mod editable_text;
pub mod text_field;

pub use controller::TextEditingController;
pub use editable_text::{EditableText, EditableTextState};
pub use text::Text;
pub use text_field::TextField;
