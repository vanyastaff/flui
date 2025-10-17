//! Core UI components and controllers for Nebula
//!
//! This crate provides fundamental UI building blocks that are used across
//! all Nebula UI crates. It includes animation controllers, validation,
//! focus management, and common widgets.

#![warn(missing_docs)]

pub mod controllers;
pub mod core;
pub mod painters;
pub mod rendering;
pub mod theme;
pub mod types;
pub mod widgets;

// Re-export commonly used types
pub use controllers::{
    animation::{AnimationController, AnimationCurve, AnimationState},
    change_tracker::{ChangeEvent, ChangeTracker, Snapshot},
    focus::FocusController,
    input::{InputController, InputMode},
    theme_controller::{ThemeController, ThemeMode, ThemeTransition, ThemeBuilder},
    validation::{ValidationController, ValidationDisplayMode, ValidationState},
    visibility::{HideMode, VisibilityController},
};

pub use theme::{ColorPalette, Theme};

// Re-export core foundation types
pub use core::{
    // Observability
    Listenable, ChangeNotifier, ValueNotifier,
    // Keys
    Key, LocalKey, UniqueKey, ValueKey, StringKey, IntKey, KeyFactory,
    // Callbacks
    VoidCallback, ValueChanged, ValueGetter, ValueSetter,
    // Diagnostics
    Diagnosticable, DiagnosticsNode, DiagnosticsProperty, DiagnosticsBuilder,
};

// Re-export commonly used types - Core
pub use types::core::{
    Color, Offset, Point, Rect, Size, Scale, Transform, Matrix4,
    Duration, Opacity, Rotation, Vector2, Vector3,
    Circle, Arc, Bounds, Path, Range1D, Range2D,
};

// Re-export commonly used types - Layout
pub use types::layout::{
    Alignment, EdgeInsets, BoxConstraints, Padding, Margin,
};

// Re-export commonly used types - Styling
pub use types::styling::{
    BoxDecoration, Border, BorderRadius, BorderSide, Radius,
    BoxShadow, Shadow, BlurStyle, Clip,
    Gradient, LinearGradient, RadialGradient,
    BlendMode, StrokeCap, StrokeJoin, StrokeStyle,
};

// Re-export commonly used types - Interaction
pub use types::interaction::{
    Curve, Curves,
};

// Re-export base widget system
pub use widgets::{
    NebulaWidget, StatelessWidget, StatefulWidget, RenderObjectWidget,
    SingleChildWidget, MultiChildWidget, WidgetBuilder,
    WidgetKey, RenderConstraints,
};

// Re-export commonly used widgets
pub use widgets::primitives::{Container, Text};

// Re-export painters
pub use painters::{
    DecorationPainter, TransformPainter, BorderPainter, ShadowPainter,
};

// Re-export rendering features
pub use rendering::{
    AccessibilityFeatures, AccessibilityPreferences,
    SemanticsNode, SemanticsData, SemanticsAction, SemanticsFlag,
    TextSelection, TextSelectionHandleType,
    MouseTracker, MouseCursor,
};

// Re-export egui essentials for convenience
pub use egui::Widget;

/// Prelude module for convenient imports
///
/// This module re-exports the most commonly used types and widgets.
/// Use `use nebula_ui::prelude::*;` to import everything you need.
pub mod prelude {
    // Controllers
    pub use crate::controllers::{
        animation::{AnimationController, AnimationCurve},
        change_tracker::ChangeTracker,
        focus::FocusController,
        input::{InputController, InputMode},
        theme_controller::{ThemeController, ThemeMode, ThemeTransition, ThemeBuilder},
        validation::{ValidationController, ValidationState},
        visibility::{VisibilityController, HideMode},
    };

    // Theme
    pub use crate::theme::Theme;

    // All types (core, layout, styling)
    pub use crate::types::prelude::*;

    // Widgets
    pub use crate::widgets::primitives::{Container, Text};

    // Painters
    pub use crate::painters::{
        DecorationPainter, TransformPainter,
    };

    // egui essentials - Widget trait needed for .ui() method
    pub use egui::Widget;
}