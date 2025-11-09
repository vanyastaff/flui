//! Hybrid style prelude (recommended)
//!
//! This style balances macros and builders for optimal ergonomics.
//! Macros for simple cases, builders for complex configuration.
//!
//! # Philosophy
//!
//! - Use macros for simple, repetitive patterns (text, spacing)
//! - Use builders for complex configuration
//! - Best of both worlds
//! - Pragmatic approach
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_widgets::style::hybrid::prelude::*;
//!
//! Scaffold::builder()
//!     .background_color(Color::WHITE)
//!     .body(
//!         column![
//!             text! { data: "Hello", size: 24.0 },
//!             sized_box! { height: 16.0 },
//!             Button::builder("Click me")
//!                 .color(Color::BLUE)
//!                 .on_pressed(|| println!("Clicked!"))
//!                 .build()
//!         ]
//!     )
//!     .build()
//! ```

/// Prelude for hybrid style (recommended default)
///
/// Import this to get both macros and builders, use whichever fits best.
pub mod prelude {
    // Re-export all widget types
    pub use crate::{
        Align, Baseline, Button, Card, Center, ClipOval, ClipRRect, ClipRect, ColoredBox, Column,
        Container, Divider, Expanded, Flex, Flexible, FractionallySizedBox, IndexedStack,
        IntrinsicHeight, IntrinsicWidth, ListBody, Material, Offstage, Opacity, OverflowBox,
        Padding, PhysicalModel, Positioned, PositionedDirectional, RepaintBoundary, RotatedBox,
        Row, Scaffold, ScrollController, SingleChildScrollView, SizedBox, SizedOverflowBox,
        Spacer, Stack, Text, Transform, Viewport, Visibility, Wrap,
    };

    // Re-export common types
    pub use flui_types::{
        layout::{
            Axis, CrossAxisAlignment, FlexFit, MainAxisAlignment, MainAxisSize, StackFit,
            WrapAlignment, WrapCrossAlignment,
        },
        styling::{BorderRadius, BoxDecoration, Radius}, painting::Clip,
        Alignment, BoxConstraints, Color, EdgeInsets, Matrix4, Offset, Size,
    };

    // Re-export View trait
    pub use flui_core::view::{AnyView, IntoElement, View};
    pub use flui_core::BuildContext;

    // Re-export commonly used macros
    // Guidelines: Macros for simple, repetitive patterns
    pub use crate::{column, row, scaffold, sized_box, text};

    // For complex widgets, use builders directly
    // This gives best of both worlds:
    // - Quick: text!("Hello")
    // - Complex: Button::builder("Click").color(Color::BLUE).build()
}
