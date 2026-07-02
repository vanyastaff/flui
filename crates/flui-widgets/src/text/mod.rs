//! Text widgets — single-line display and editing over `flui-objects`'
//! `RenderParagraph`.

mod rich_text;
mod text;

pub mod controller;
pub mod editable_text;
pub mod text_field;

pub use controller::TextEditingController;
pub use editable_text::{EditableText, EditableTextState};
pub use rich_text::RichText;
pub use text::Text;
pub use text_field::TextField;
