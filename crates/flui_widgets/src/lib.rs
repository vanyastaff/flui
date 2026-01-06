//! Widget implementations for Flui framework
//!
//! This crate provides high-level widget implementations built on top of
//! the RenderObject layer (flui_rendering).
//!
//! # Architecture
//!
//! Widgets in Flui follow the three-tree pattern:
//!
//! ```text
//! Widget (immutable) → Element (mutable) → RenderObject (layout & paint)
//! ```
//!
//! # Available Widgets
//!
//! ## Basic Widgets (`basic` module)
//! - **Padding**: Insets its child by padding
//! - **Center**: Centers its child
//! - **SizedBox**: A box with fixed dimensions
//! - **ColoredBox**: Paints a colored rectangle
//!
//! ## Layout Widgets (`layout` module)
//! - **Row**: Horizontal flex layout
//! - **Column**: Vertical flex layout
//! - **Flex**: Base flex layout widget
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_widgets::prelude::*;
//!
//! // Create a centered colored box
//! Center::new().child(
//!     ColoredBox::red(100.0, 50.0)
//! )
//!
//! // Create a row of boxes
//! Row::new()
//!     .spacing(8.0)
//!     .children([
//!         ColoredBox::red(50.0, 50.0),
//!         ColoredBox::green(50.0, 50.0),
//!         ColoredBox::blue(50.0, 50.0),
//!     ])
//! ```

#![warn(missing_docs)]

// Active modules (using new RenderBox architecture)
pub mod animation;
pub mod basic;
pub mod layout;

// Re-export commonly used widgets
pub use animation::{FadeTransition, RotationTransition, ScaleTransition, SlideTransition};
pub use basic::{Center, ColoredBox, Padding, SizedBox};
pub use layout::{Column, CrossAxisAlignment, Flex, MainAxisAlignment, Row};

// ============================================================================
// DISABLED: Modules below use old flui_core/flui_objects architecture
// They will be migrated when their RenderObjects are implemented
// ============================================================================

// pub mod animation;
// pub mod error;
// pub mod gestures;
// pub mod interaction;
// pub mod scrolling;
// pub mod style;
// pub mod visual_effects;

/// Prelude module for convenient imports
///
/// Import this module to get access to all commonly used widgets and types:
/// ```rust,ignore
/// use flui_widgets::prelude::*;
/// ```
pub mod prelude {
    // Re-export animation widgets
    pub use crate::animation::{
        FadeTransition, RotationTransition, ScaleTransition, SlideTransition,
    };

    // Re-export all active widgets
    pub use crate::basic::{Center, ColoredBox, Padding, SizedBox};
    pub use crate::layout::{Column, CrossAxisAlignment, Flex, MainAxisAlignment, Row};

    // Re-export core types
    pub use flui_rendering::prelude::BoxConstraints;
    pub use flui_types::{EdgeInsets, Offset, Size};
}
