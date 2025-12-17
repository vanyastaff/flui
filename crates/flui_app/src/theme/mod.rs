//! Theme system for FLUI applications.
//!
//! Rust-way theme system - simple, type-safe, composable.
//!
//! # Design Philosophy
//!
//! Unlike Flutter's deeply nested ThemeData, we use:
//! - Flat, composable structs
//! - Builder pattern with sensible defaults
//! - Type-safe color tokens
//! - No runtime reflection
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_app::theme::{Theme, ThemeMode};
//!
//! // Use built-in theme
//! let theme = Theme::light();
//!
//! // Or customize
//! let theme = Theme::builder()
//!     .mode(ThemeMode::Dark)
//!     .primary(Color::from_hex("#6200EE"))
//!     .build();
//! ```

mod colors;
mod data;

pub use colors::{Color, ColorScheme};
pub use data::{Theme, ThemeBuilder, ThemeMode};
