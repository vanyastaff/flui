//! Base widget types and traits
//!
//! This module provides the fundamental widget hierarchy, similar to Flutter:
//! - Object (Rust's default)
//! - DiagnosticableTree → Widget
//! - StatelessWidget (data-only widgets)
//! - StatefulWidget (widgets with mutable state)
//! - RenderObjectWidget (widgets that create render objects)
//!
//! # Widget Hierarchy
//!
//! ```text
//! Object (Rust default)
//!   └─ Widget (trait)
//!       ├─ StatelessWidget (no internal state)
//!       ├─ StatefulWidget (has internal state)
//!       └─ RenderObjectWidget (creates render objects)
//!           ├─ SingleChildRenderObjectWidget (one child)
//!           │   └─ Align, Padding, Transform, Container
//!           └─ MultiChildRenderObjectWidget (multiple children)
//!               └─ Row, Column, Stack, Flex
//! ```

use crate::types::core::{Rect, Size};
use std::any::Any;
use std::fmt::Debug;

/// Unique identifier for a widget instance.
///
/// Similar to Flutter's Key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WidgetKey(pub egui::Id);

impl WidgetKey {
    /// Create a new unique key.
    pub fn new() -> Self {
        Self(egui::Id::new(std::time::Instant::now()))
    }

    /// Create a key from a value.
    pub fn from_value(value: impl std::hash::Hash) -> Self {
        Self(egui::Id::new(value))
    }

    /// Create a key from a string.
    pub fn from_string(value: impl Into<String>) -> Self {
        Self(egui::Id::new(value.into()))
    }
}

impl Default for WidgetKey {
    fn default() -> Self {
        Self::new()
    }
}

impl From<egui::Id> for WidgetKey {
    fn from(id: egui::Id) -> Self {
        Self(id)
    }
}

impl From<WidgetKey> for egui::Id {
    fn from(key: WidgetKey) -> Self {
        key.0
    }
}

/// Base trait for all widgets.
///
/// Similar to Flutter's Widget class. In nebula-ui, this combines with egui::Widget.
pub trait NebulaWidget: Debug + 'static {
    /// Get the widget's key (if any).
    fn key(&self) -> Option<WidgetKey> {
        None
    }

    /// Get diagnostic information about this widget.
    fn diagnostics(&self) -> WidgetDiagnostics {
        WidgetDiagnostics {
            type_name: std::any::type_name::<Self>(),
            key: self.key(),
            properties: Vec::new(),
        }
    }

    /// Check if this widget can update another widget.
    ///
    /// Two widgets can update each other if they have the same type and key.
    fn can_update(&self, other: &dyn Any) -> bool {
        // In Rust, we use TypeId for type checking
        self.key().is_some() && other.type_id() == std::any::TypeId::of::<Self>()
    }
}

/// Diagnostic information about a widget.
///
/// Similar to Flutter's DiagnosticsNode.
#[derive(Debug, Clone)]
pub struct WidgetDiagnostics {
    /// The type name of the widget
    pub type_name: &'static str,

    /// The widget's key
    pub key: Option<WidgetKey>,

    /// Additional properties
    pub properties: Vec<DiagnosticProperty>,
}

/// A diagnostic property of a widget.
#[derive(Debug, Clone)]
pub struct DiagnosticProperty {
    /// Property name
    pub name: String,

    /// Property value as string
    pub value: String,
}

impl DiagnosticProperty {
    /// Create a new diagnostic property.
    pub fn new(name: impl Into<String>, value: impl std::fmt::Display) -> Self {
        Self {
            name: name.into(),
            value: value.to_string(),
        }
    }
}

/// Marker trait for stateless widgets.
///
/// Stateless widgets are immutable and describe part of the UI.
/// Similar to Flutter's StatelessWidget.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct MyStatelessWidget {
///     text: String,
/// }
///
/// impl NebulaWidget for MyStatelessWidget { }
/// impl StatelessWidget for MyStatelessWidget { }
///
/// impl egui::Widget for MyStatelessWidget {
///     fn ui(self, ui: &mut egui::Ui) -> egui::Response {
///         ui.label(&self.text)
///     }
/// }
/// ```
pub trait StatelessWidget: NebulaWidget + egui::Widget {}

/// Marker trait for stateful widgets.
///
/// Stateful widgets have mutable state that can change over time.
/// Similar to Flutter's StatefulWidget.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct MyStatefulWidget {
///     counter: i32,
/// }
///
/// impl NebulaWidget for MyStatefulWidget { }
/// impl StatefulWidget for MyStatefulWidget {
///     type State = i32;
///
///     fn state(&self) -> &Self::State {
///         &self.counter
///     }
///
///     fn state_mut(&mut self) -> &mut Self::State {
///         &mut self.counter
///     }
/// }
/// ```
pub trait StatefulWidget: NebulaWidget {
    /// The type of state this widget manages
    type State: Debug;

    /// Get immutable reference to the state
    fn state(&self) -> &Self::State;

    /// Get mutable reference to the state
    fn state_mut(&mut self) -> &mut Self::State;
}

/// Base trait for render object widgets.
///
/// Render object widgets are responsible for layout, painting, and hit testing.
/// Similar to Flutter's RenderObjectWidget.
pub trait RenderObjectWidget: NebulaWidget + egui::Widget {
    /// Get the constraints for this render object
    fn constraints(&self) -> Option<RenderConstraints> {
        None
    }

    /// Get the computed size after layout
    fn size(&self) -> Option<Size> {
        None
    }

    /// Get the bounding rectangle
    fn rect(&self) -> Option<Rect> {
        None
    }
}

/// Render constraints for layout.
///
/// Similar to Flutter's BoxConstraints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderConstraints {
    /// Minimum width
    pub min_width: f32,

    /// Maximum width
    pub max_width: f32,

    /// Minimum height
    pub min_height: f32,

    /// Maximum height
    pub max_height: f32,
}

impl RenderConstraints {
    /// Create tight constraints (fixed size).
    pub fn tight(size: Size) -> Self {
        Self {
            min_width: size.width,
            max_width: size.width,
            min_height: size.height,
            max_height: size.height,
        }
    }

    /// Create loose constraints (max size).
    pub fn loose(size: Size) -> Self {
        Self {
            min_width: 0.0,
            max_width: size.width,
            min_height: 0.0,
            max_height: size.height,
        }
    }

    /// Create unbounded constraints.
    pub fn unbounded() -> Self {
        Self {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }
    }

    /// Check if these constraints have a bounded width.
    pub fn has_bounded_width(&self) -> bool {
        self.max_width.is_finite()
    }

    /// Check if these constraints have a bounded height.
    pub fn has_bounded_height(&self) -> bool {
        self.max_height.is_finite()
    }

    /// Check if these constraints are tight (fixed size).
    pub fn is_tight(&self) -> bool {
        self.min_width == self.max_width && self.min_height == self.max_height
    }

    /// Constrain a size to fit within these constraints.
    pub fn constrain(&self, size: Size) -> Size {
        Size::new(
            size.width.clamp(self.min_width, self.max_width),
            size.height.clamp(self.min_height, self.max_height),
        )
    }
}

/// Trait for widgets with a single child.
///
/// Similar to Flutter's SingleChildRenderObjectWidget.
pub trait SingleChildWidget: RenderObjectWidget {
    /// Type of the child widget
    type Child: egui::Widget;

    /// Get the child widget
    fn child(&self) -> Option<&Self::Child>;
}

/// Trait for widgets with multiple children.
///
/// Similar to Flutter's MultiChildRenderObjectWidget.
pub trait MultiChildWidget: RenderObjectWidget {
    /// Type of the child widgets
    type Child: egui::Widget;

    /// Get the children widgets
    fn children(&self) -> &[Self::Child];

    /// Get the number of children
    fn child_count(&self) -> usize {
        self.children().len()
    }
}

/// Helper trait for building widgets with a fluent API.
///
/// This provides common builder methods like `with_key()`.
pub trait WidgetBuilder: Sized {
    /// Set the key for this widget.
    fn with_key(self, key: WidgetKey) -> Self;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_widget_key() {
        let key1 = WidgetKey::from_string("test");
        let key2 = WidgetKey::from_string("test");

        // Same string should produce same key
        assert_eq!(key1, key2);

        let key3 = WidgetKey::from_string("other");
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_render_constraints_tight() {
        let size = Size::new(100.0, 200.0);
        let constraints = RenderConstraints::tight(size);

        assert!(constraints.is_tight());
        assert_eq!(constraints.min_width, 100.0);
        assert_eq!(constraints.max_width, 100.0);
        assert_eq!(constraints.min_height, 200.0);
        assert_eq!(constraints.max_height, 200.0);
    }

    #[test]
    fn test_render_constraints_loose() {
        let size = Size::new(100.0, 200.0);
        let constraints = RenderConstraints::loose(size);

        assert!(!constraints.is_tight());
        assert_eq!(constraints.min_width, 0.0);
        assert_eq!(constraints.max_width, 100.0);
        assert_eq!(constraints.min_height, 0.0);
        assert_eq!(constraints.max_height, 200.0);
    }

    #[test]
    fn test_render_constraints_constrain() {
        let constraints = RenderConstraints {
            min_width: 50.0,
            max_width: 150.0,
            min_height: 50.0,
            max_height: 150.0,
        };

        // Too small - should clamp to min
        let size1 = constraints.constrain(Size::new(30.0, 30.0));
        assert_eq!(size1, Size::new(50.0, 50.0));

        // Too large - should clamp to max
        let size2 = constraints.constrain(Size::new(200.0, 200.0));
        assert_eq!(size2, Size::new(150.0, 150.0));

        // Just right - should stay the same
        let size3 = constraints.constrain(Size::new(100.0, 100.0));
        assert_eq!(size3, Size::new(100.0, 100.0));
    }

    #[test]
    fn test_render_constraints_bounded() {
        let bounded = RenderConstraints::loose(Size::new(100.0, 200.0));
        assert!(bounded.has_bounded_width());
        assert!(bounded.has_bounded_height());

        let unbounded = RenderConstraints::unbounded();
        assert!(!unbounded.has_bounded_width());
        assert!(!unbounded.has_bounded_height());
    }

    #[test]
    fn test_diagnostic_property() {
        let prop = DiagnosticProperty::new("width", 100.0);
        assert_eq!(prop.name, "width");
        assert_eq!(prop.value, "100");
    }
}
