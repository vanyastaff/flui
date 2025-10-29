//! Typography types for text rendering and styling.
//!
//! This module provides comprehensive types for text styling, alignment,
//! decoration, metrics, and spans, inspired by Flutter's typography system.

pub mod font_family;
pub mod font_loader;
pub mod font_provider;
pub mod text_alignment;
pub mod text_decoration;
pub mod text_metrics;
pub mod text_spans;
pub mod text_style;

pub use font_family::*;
pub use font_loader::*;
pub use font_provider::*;
pub use text_alignment::*;
pub use text_decoration::*;
pub use text_metrics::*;
pub use text_spans::*;
pub use text_style::*;
