//! Style prelude modules for different UI coding styles
//!
//! This module provides different "styles" or "dialects" for writing FLUI code.
//! Each style is optimized for different preferences and use cases.
//!
//! # Available Styles
//!
//! - **macros**: Maximum declarative style using macros everywhere
//! - **builder**: Traditional builder pattern style
//! - **hybrid**: Balanced mix of macros and builders (recommended)
//!
//! # Usage
//!
//! Choose your preferred style by importing its prelude:
//!
//! ```rust,ignore
//! // Macro-heavy style
//! use flui_widgets::style::macros::prelude::*;
//!
//! // Builder style
//! use flui_widgets::style::builder::prelude::*;
//!
//! // Hybrid style (default)
//! use flui_widgets::prelude::*;
//! ```
//!
//! # Feature Flags
//!
//! You can also use feature flags to set default style:
//!
//! ```toml
//! [dependencies]
//! flui_widgets = { version = "0.1", features = ["style-macros"] }
//! ```

pub mod builder;
pub mod hybrid;
pub mod macros;
