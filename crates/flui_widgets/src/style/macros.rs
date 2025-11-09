//! Macro-heavy style prelude
//!
//! This style uses macros for everything possible, creating the most
//! compact and declarative code. Perfect for prototyping and Flutter-like syntax.
//!
//! # Philosophy
//!
//! - Minimize boilerplate
//! - Maximum declarative syntax
//! - Flutter/SwiftUI-like feel
//! - Use macros for all structural widgets
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_widgets::style::macros::prelude::*;
//!
//! scaffold! {
//!     background_color: Color::WHITE,
//!     body: column![
//!         text! { data: "Hello", size: 24.0 },
//!         sized_box! { height: 16.0 },
//!         row![
//!             text! { data: "A" },
//!             text! { data: "B" }
//!         ]
//!     ]
//! }
//! ```

/// Prelude for macro-heavy style
///
/// Import this to get all macros and minimal builders.
pub mod prelude {
    // Re-export all widget types (for type annotations)
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

    // Import macros with explicit paths for clarity
    // This is the key: we DON'T use the struct types directly in macro style
    // Instead, everything goes through macros

    /// scaffold! macro - replaces Scaffold::builder()
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// scaffold! {
    ///     background_color: Color::WHITE,
    ///     body: my_widget
    /// }
    /// ```
    pub use crate::scaffold;

    /// column! macro - replaces Column::builder()
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// column![widget1, widget2, widget3]
    ///
    /// column! {
    ///     main_axis_alignment: MainAxisAlignment::Center;
    ///     [widget1, widget2]
    /// }
    /// ```
    pub use crate::column;

    /// row! macro - replaces Row::builder()
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// row![widget1, widget2, widget3]
    ///
    /// row! {
    ///     main_axis_alignment: MainAxisAlignment::SpaceEvenly;
    ///     [widget1, widget2]
    /// }
    /// ```
    pub use crate::row;

    /// text! macro - replaces Text::builder()
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// text!("Hello")
    ///
    /// text! {
    ///     data: "Hello",
    ///     size: 24.0,
    ///     color: Color::RED
    /// }
    /// ```
    pub use crate::text;

    /// sized_box! macro - replaces SizedBox::builder()
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// sized_box! { height: 16.0 }
    ///
    /// sized_box! {
    ///     width: 100.0,
    ///     height: 100.0,
    ///     child: my_widget
    /// }
    /// ```
    pub use crate::sized_box;

    // TODO: Add more macros as they are implemented
    // pub use crate::container;
    // pub use crate::padding;
    // pub use crate::center;
    // pub use crate::align;
    // pub use crate::stack;
}
