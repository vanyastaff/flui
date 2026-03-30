//! Integration tests for `flui_painting` crate.
//!
//! This module contains integration tests covering:
//! - Canvas composition API
//! - Transform API integration
//! - Thread safety guarantees
//! - Rich text examples
//! - Text layout pipeline (TextLayout -> TextPainter -> Canvas)

pub mod canvas_composition;
pub mod canvas_transform;
pub mod rich_text_example;
pub mod text_layout_pipeline;
pub mod thread_safety;
