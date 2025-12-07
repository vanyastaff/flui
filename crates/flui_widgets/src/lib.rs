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

// Temporarily disable modules with compilation errors for counter demo
// pub mod animation;
pub mod basic;
// pub mod error;
// pub mod gestures;
// pub mod interaction;
// pub mod layout;
// pub mod scrolling;
// pub mod style;
// pub mod visual_effects;

// Re-export commonly used widgets for convenient access
pub use basic::Text;
// pub use layout::{Column, Row};

// Temporarily disabled exports until widgets are fixed
// pub use basic::{
//     Align, AspectRatio, Builder, Button, Card, Center, ColoredBox, ConstrainedBox, Container,
//     CustomPaint, DecoratedBox, Divider, Empty, FittedBox, LayoutBuilder, LimitedBox, Padding,
//     SafeArea, SizedBox, VerticalDivider,
// };
// pub use gestures::GestureDetector;
// pub use interaction::{AbsorbPointer, IgnorePointer, MouseRegion};
// pub use layout::{
//     Baseline, Expanded, Flex, Flexible, FractionallySizedBox, IndexedStack,
//     IntrinsicHeight, IntrinsicWidth, ListBody, OverflowBox, Positioned, PositionedDirectional,
//     RotatedBox, Scaffold, ScrollController, SingleChildScrollView, SizedOverflowBox, Spacer,
//     Stack, Wrap,
// };

// Re-export commonly used types
// pub use flui_rendering::objects::DecorationPosition;
// pub use flui_types::layout::{FlexFit, StackFit};
// pub use flui_types::styling::{BorderRadius, BoxDecoration, Radius};
// pub use flui_types::{Alignment, BoxConstraints, Color, EdgeInsets, Matrix4, Offset, Size};

/// Prelude module for convenient imports
///
/// Import this module to get access to all commonly used widgets and types:
/// ```rust,ignore
/// use flui_widgets::prelude::*;
/// ```
pub mod prelude {
    // Temporarily disabled - only Text widget is enabled for counter demo
    // // Re-export essential widgets for Container and Flex layout
    // pub use crate::basic::{
    //     Align, AspectRatio, Builder, Button, Card, Center, ColoredBox, ConstrainedBox, Container,
    //     CustomPaint, DecoratedBox, Divider, Empty, FittedBox, LayoutBuilder, LimitedBox, Padding,
    //     SafeArea, SizedBox, Text, VerticalDivider,
    // };
    pub use crate::basic::Text;

    // pub use crate::gestures::GestureDetector;
    // pub use crate::interaction::{AbsorbPointer, IgnorePointer, MouseRegion};
    // pub use crate::layout::{
    //     Baseline, Column, Expanded, Flex, Flexible, FractionallySizedBox, IndexedStack,
    //     IntrinsicHeight, IntrinsicWidth, ListBody, OverflowBox, Positioned, PositionedDirectional,
    //     RotatedBox, Row, Scaffold, ScrollController, SingleChildScrollView, SizedOverflowBox,
    //     Spacer, Stack, Wrap,
    // };

    // Re-export core types
    // pub use flui_core::BuildContext;  // Not needed for IntoElement pattern
    pub use flui_types::layout::{
        CrossAxisAlignment, FlexFit, MainAxisAlignment, MainAxisSize, StackFit,
    };
    pub use flui_types::styling::{BorderRadius, BoxDecoration, Radius};
    pub use flui_types::BoxConstraints;
    pub use flui_types::{Alignment, Color, EdgeInsets, Matrix4, Offset, Size};
}
