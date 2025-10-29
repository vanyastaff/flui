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
//! # Widget Categories
//!
//! ## Basic Widgets (`basic` module)
//! - **Container**: Combines sizing, padding, decoration, and constraints
//! - **SizedBox**: A box with fixed dimensions
//! - **Padding**: Insets its child by padding
//! - **Center**: Centers its child
//! - **Align**: Aligns its child with flexible positioning
//!
//! ## Layout Widgets (`layout` module)
//! - **Row**: Horizontal flex layout
//! - **Column**: Vertical flex layout
//! - **Stack**: Positioned layout with z-ordering
//! - **IndexedStack**: Shows only one child at a time
//!
//! ## Visual Effects (`visual_effects` module)
//! - **Opacity**: Controls child opacity
//! - **Transform**: Applies matrix transformations
//! - **ClipRRect**: Clips child with rounded rectangle
//! - **DecoratedBox**: Paints decoration before or after child
//!
//! ## Interaction Widgets (`interaction` module)
//! - **IgnorePointer**: Makes widget transparent to pointer events
//! - **AbsorbPointer**: Blocks pointer events from passing through
//!
//! ## Future Categories
//! - **Scrolling:** ListView, GridView, ScrollView
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_widgets::prelude::*;
//!
//! // Create a centered container with padding
//! Container::builder()
//!     .width(200.0)
//!     .height(100.0)
//!     .padding(EdgeInsets::all(16.0))
//!     .color(Color::rgb(0, 0, 255))
//!     .build()
//! ```

#![warn(missing_docs)]

// Widget modules organized by category
pub mod basic;
pub mod layout;

// Temporarily disabled - will be fixed later
// pub mod visual_effects;
// pub mod gestures;
// pub mod interaction;

// Re-exports for convenient top-level access
// Essential widgets for Container and Flex layout
pub use basic::{Align, Center, Container, DecoratedBox, Padding, SizedBox};
pub use layout::{Column, Expanded, Flexible, Row};

// Temporarily disabled - will be fixed later
// pub use basic::{AspectRatio, Button, Text};
// pub use layout::{IndexedStack, Positioned, Stack};
// pub use visual_effects::{ClipRect, ClipRRect, Offstage, Opacity, Transform};
// pub use gestures::GestureDetector;
// pub use interaction::{AbsorbPointer, IgnorePointer, MouseRegion};

// Re-export commonly used types
pub use flui_core::{BoxedWidget, DynWidget, Widget};
pub use flui_rendering::DecorationPosition;
pub use flui_types::layout::{FlexFit, StackFit};
pub use flui_types::styling::{BorderRadius, BoxDecoration, Radius};
pub use flui_types::{Alignment, BoxConstraints, Color, EdgeInsets, Matrix4, Offset, Size};

/// Prelude module for convenient imports
///
/// Import this module to get access to all commonly used widgets and types:
/// ```rust,ignore
/// use flui_widgets::prelude::*;
/// ```
pub mod prelude {
    // Re-export essential widgets for Container and Flex layout
    pub use crate::basic::{Align, Center, Container, DecoratedBox, Padding, SizedBox};
    pub use crate::layout::{Column, Expanded, Flexible, Row};

    // Temporarily disabled
    // pub use crate::basic::{AspectRatio, Button, Text};
    // pub use crate::layout::{IndexedStack, Positioned, Stack};
    // pub use crate::visual_effects::{ClipRect, ClipRRect, Offstage, Opacity, Transform};
    // pub use crate::gestures::GestureDetector;
    // pub use crate::interaction::{AbsorbPointer, IgnorePointer, MouseRegion};

    // Re-export core types
    pub use flui_core::{BuildContext, Widget};
    pub use flui_rendering::{FlexFit, StackFit};
    pub use flui_types::styling::{BorderRadius, BoxDecoration, Radius};
    pub use flui_types::BoxConstraints;
    pub use flui_types::{Alignment, Color, EdgeInsets, Matrix4, Offset, Size};
    pub use flui_types::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize};
}
