//! Typography types.
//!
//! This module contains types for text styling and rendering:
//! - [`FontSize`]: Type-safe font size in points
//! - [`FontWeight`]: Font weight (thin, normal, bold, etc.)
//! - [`FontFamily`]: Font family name
//! - [`LineHeight`]: Line height for text
//! - [`TextStyle`]: Complete text styling (font, size, color, decoration)
//! - Text alignment, direction, overflow, and selection types

pub mod font;
pub mod text;
pub mod text_span;
pub mod text_style;



// Re-export types for convenience
pub use font::{FontFamily, FontSize, FontWeight, LineHeight};
pub use text::*;
pub use text_span::{InlineSpan, TextSpan};
pub use text_style::{
    TextDecoration, TextDecorationStyle, TextStyle,
    text_style_to_egui, text_style_to_rich_text, default_egui_style,
};






