//! Text widgets — single-line display and editing over `flui-objects`'
//! `RenderParagraph`.

mod animated_default_text_style;
mod default_text_style;
mod linked_paragraph;
mod rich_text;
mod text;

pub mod controller;
pub mod editable_text;
pub mod text_field;
mod text_store;

pub use animated_default_text_style::AnimatedDefaultTextStyle;
pub use controller::TextEditingController;
pub use default_text_style::DefaultTextStyle;
pub use editable_text::{EditableText, EditableTextState, SubmitCallback};
pub use rich_text::RichText;
pub use text::Text;
pub use text_field::{RawTextField, RawTextFieldState};
