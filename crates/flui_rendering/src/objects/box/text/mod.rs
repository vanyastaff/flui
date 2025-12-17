//! Text rendering objects.
//!
//! This module provides render objects for text display:
//!
//! - [`RenderParagraph`]: Multi-line text with wrapping, overflow, and inline widgets
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderParagraph` | `RenderParagraph` |

mod paragraph;

pub use paragraph::*;
