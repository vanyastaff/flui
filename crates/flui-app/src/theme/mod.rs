//! Application-level theme system for FLUI applications.
//!
//! `AppTheme` is the app-framework's pre-tree configuration object. It is
//! distinct from `flui_widgets::Theme`, which is the in-tree inherited widget
//! that provides `ThemeData` to descendants at runtime.
//!
//! # Design Philosophy
//!
//! - Flat, composable structs
//! - Builder pattern with sensible defaults
//! - Type-safe color tokens via [`ColorScheme`]
//! - No runtime reflection
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_app::theme::{AppTheme, ThemeMode};
//!
//! // Use built-in theme
//! let theme = AppTheme::light();
//!
//! // Or customize
//! let theme = AppTheme::builder()
//!     .mode(ThemeMode::Dark)
//!     .build();
//! ```

mod colors;
mod data;

pub use colors::ColorScheme;
pub use data::{AppTheme, AppThemeBuilder, ThemeMode};
