//! Builder pattern style prelude
//!
//! This style uses traditional builder patterns everywhere, avoiding macros.
//! Perfect for explicit, IDE-friendly code with full autocomplete.
//!
//! # Philosophy
//!
//! - Explicit is better than implicit
//! - Full IDE autocomplete support
//! - Traditional Rust patterns
//! - No "magic" macros
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_widgets::style::builder::prelude::*;
//!
//! Scaffold::builder()
//!     .background_color(Color::WHITE)
//!     .body(
//!         Column::builder()
//!             .child(
//!                 Text::builder()
//!                     .data("Hello")
//!                     .size(24.0)
//!                     .build()
//!             )
//!             .child(SizedBox::builder().height(16.0).build())
//!             .build()
//!     )
//!     .build()
//! ```

/// Prelude for builder pattern style
///
/// Import this to get all widgets as builders, no macros.
pub mod prelude {
    // Re-export all widget types (structs with ::builder() methods)
    pub use crate::{
        Align, Baseline, Button, Card, Center, ClipOval, ClipRRect, ClipRect, ColoredBox, Column,
        Container, Divider, Expanded, Flex, Flexible, FractionallySizedBox, IndexedStack,
        IntrinsicHeight, IntrinsicWidth, ListBody, Material, Offstage, Opacity, OverflowBox,
        Padding, PhysicalModel, Positioned, PositionedDirectional, RepaintBoundary, RotatedBox,
        Row, Scaffold, ScrollController, SingleChildScrollView, SizedBox, SizedOverflowBox, Spacer,
        Stack, Text, Transform, Viewport, Visibility, Wrap,
    };

    // Re-export common types
    pub use flui_types::{
        layout::{
            Axis, CrossAxisAlignment, FlexFit, MainAxisAlignment, MainAxisSize, StackFit,
            WrapAlignment, WrapCrossAlignment,
        },
        painting::Clip,
        styling::{BorderRadius, BoxDecoration, Radius},
        Alignment, BoxConstraints, Color, EdgeInsets, Matrix4, Offset, Size,
    };

    // Re-export View trait
    pub use flui_core::view::{IntoElement, View};
    pub use flui_core::BuildContext;

    // Explicitly NO macros - pure builder pattern
    // This style prioritizes:
    // - IDE autocomplete
    // - Explicit method calls
    // - Traditional Rust patterns
}
