//! Application-level theme system for FLUI applications.
//!
//! `AppTheme` is the app-framework's pre-tree configuration object. It is
//! distinct from `flui_material::Theme`, which is the in-tree inherited
//! widget that provides `flui_material::ThemeData` to descendants at
//! runtime.
//!
//! **Status: parked, unwired.** Nothing in this crate reads `AppTheme` or
//! `AppColorScheme` yet — no runner, binding, or widget consumes them. This
//! is a deliberately-kept candidate surface pending the design-system
//! integration work (`ADR-0028`,
//! docs/adr/ADR-0028-design-system-decoupling-contract.md), not dead code
//! left behind by accident.
//!
//! # Design Philosophy
//!
//! - Flat, composable structs
//! - Builder pattern with sensible defaults
//! - Type-safe color tokens via [`AppColorScheme`]
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

pub use colors::AppColorScheme;
pub use data::{AppTheme, AppThemeBuilder, ThemeMode};
