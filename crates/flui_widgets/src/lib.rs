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
//! - Stack, Wrap (future)
//!
//! ## Visual Effects (`basic` module - in progress)
//! - **DecoratedBox**: Paints decoration before or after child
//!
//! ## Future Categories
//! - **Visual effects:** Opacity, Transform, ClipRRect
//! - **Flex children:** Expanded, Flexible, Spacer
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
pub mod visual_effects;
pub mod gestures;





// Re-exports for convenient top-level access
pub use basic::{Align, AspectRatio, Button, Center, Container, DecoratedBox, Padding, SizedBox};
pub use layout::{Column, Expanded, Flexible, IndexedStack, Positioned, Row, Stack};
pub use visual_effects::{ClipRRect, Opacity, Transform};
pub use gestures::GestureDetector;

// Re-export commonly used types
pub use flui_core::{BoxConstraints, BuildContext, Widget};
pub use flui_rendering::{DecorationPosition, FlexFit, RenderBox, RenderObject, StackFit};
pub use flui_types::styling::{BorderRadius, BoxDecoration, Radius};
pub use flui_types::{Alignment, Color, EdgeInsets, Matrix4, Offset, Size};

/// Prelude module for convenient imports
///
/// Import this module to get access to all commonly used widgets and types:
/// ```rust,ignore
/// use flui_widgets::prelude::*;
/// ```
pub mod prelude {
    // Re-export all widgets
    pub use crate::basic::{Align, AspectRatio, Button, Center, Container, DecoratedBox, Padding, SizedBox, Text};
    pub use crate::layout::{Column, Expanded, Flexible, IndexedStack, Positioned, Row, Stack};
    pub use crate::visual_effects::{ClipRRect, Opacity, Transform};
    pub use crate::gestures::GestureDetector;

    // Re-export core types
    pub use flui_core::{BoxConstraints, BuildContext, Widget};
    pub use flui_rendering::{FlexFit, StackFit};
    pub use flui_types::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize};
    pub use flui_types::styling::{BorderRadius, BoxDecoration, Radius};
    pub use flui_types::{Alignment, Color, EdgeInsets, Matrix4, Offset, Size};
}













