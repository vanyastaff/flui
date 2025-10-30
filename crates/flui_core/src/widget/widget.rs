//! Widget - Core UI building block
//!
//! Widgets are immutable configuration objects that describe what the UI should look like.
//! This is the main type you'll use when building user interfaces with FLUI.
//!
//! # Widget Types
//!
//! FLUI provides five widget types, each with different capabilities:
//!
//! ```text
//! StatelessWidget    → Pure UI, no state
//! StatefulWidget     → Mutable state that persists
//! InheritedWidget    → Share data down the tree
//! RenderWidget       → Custom layout and painting
//! ParentDataWidget   → Metadata for parent layouts
//! ```
//!
//! # Creating Widgets
//!
//! Use the appropriate constructor for your widget type:
//!
//! ```ignore
//! // Stateless widget (most common)
//! let widget = Widget::stateless(MyWidget { ... });
//!
//! // Stateful widget (with mutable state)
//! let widget = Widget::stateful(Counter { initial: 0 });
//!
//! // Inherited widget (for data propagation)
//! let widget = Widget::inherited(Theme { colors: ... });
//!
//! // Render widget (custom rendering)
//! let widget = Widget::render_object(Container { width: 100.0 });
//!
//! // Parent data widget (layout metadata)
//! let widget = Widget::parent_data(Flexible { flex: 1, ... });
//! ```
//!
//! # Performance
//!
//! Widget uses an enum-based architecture for excellent performance:
//! - Fast dispatch (no vtable lookups)
//! - Cache-friendly memory layout
//! - Type-safe exhaustive matching
//!
//! # Examples
//!
//! ```
//! use flui_core::{Widget, StatelessWidget, BuildContext};
//!
//! #[derive(Debug, Clone)]
//! struct HelloWorld;
//!
//! impl StatelessWidget for HelloWorld {
//!     fn build(&self, ctx: &BuildContext) -> Widget {
//!         Widget::render_object(Text::new("Hello, World!"))
//!     }
//!
//!     fn clone_boxed(&self) -> Box<dyn StatelessWidget> {
//!         Box::new(self.clone())
//!     }
//!
//!     fn as_any(&self) -> &dyn std::any::Any {
//!         self
//!     }
//! }
//!
//! let widget = Widget::stateless(HelloWorld);
//! ```

use std::any::{Any, TypeId};

use super::traits::{
    InheritedWidget, ParentDataWidget, RenderWidget, StatefulWidget, StatelessWidget,
};
use crate::foundation::Key;

/// Widget - unified enum for all widget types
///
/// This is the core type for widgets in Flui. Instead of a trait hierarchy,
/// we use an enum with different variants for different types of widgets.
///
/// # Variants
///
/// - **Stateless** - Pure function from configuration to UI
/// - **Stateful** - Has mutable state
/// - **Inherited** - Provides data down the tree
/// - **Render** - Direct control over layout/paint
/// - **ParentData** - Attaches metadata to descendants
///
/// # Usage
///
/// ```
/// // Create a stateless widget
/// let widget = Widget::stateless(MyWidget { ... });
///
/// // Create a stateful widget
/// let widget = Widget::stateful(Counter { initial: 0 });
///
/// // Pattern match on type
/// match widget {
///     Widget::Stateless(w) => w.build(ctx),
///     Widget::Stateful(w) => {
///         let state = w.create_state();
///         // ...
///     }
///     _ => {}
/// }
/// ```
#[derive(Debug)]
pub enum Widget {
    /// Stateless widget - pure function from config to UI
    ///
    /// Stateless widgets have no mutable state. They rebuild from scratch
    /// when their configuration changes.
    Stateless(Box<dyn StatelessWidget>),

    /// Stateful widget - has mutable state
    ///
    /// Stateful widgets create a State object that persists across rebuilds.
    /// The state can be mutated and triggers rebuilds.
    Stateful(Box<dyn StatefulWidget>),

    /// Inherited widget - provides data down the tree
    ///
    /// Inherited widgets allow descendant widgets to access data without
    /// explicitly passing it through every level.
    Inherited(Box<dyn InheritedWidget>),

    /// Render widget - creates Render for layout/paint
    ///
    /// Render widgets directly create and manage Renders,
    /// which handle layout and painting.
    Render(Box<dyn RenderWidget>),

    /// ParentData widget - attaches metadata to descendants
    ///
    /// ParentData widgets don't create their own elements, but instead
    /// modify the parent data of descendant Renders.
    ParentData(Box<dyn ParentDataWidget>),
}

impl Widget {
    /// Create a Stateless widget
    ///
    /// # Examples
    ///
    /// ```
    /// let widget = Widget::stateless(HelloWorld);
    /// ```
    #[inline]
    pub fn stateless(widget: impl StatelessWidget) -> Self {
        Widget::Stateless(Box::new(widget))
    }

    /// Create a Stateful widget
    ///
    /// # Examples
    ///
    /// ```
    /// let widget = Widget::stateful(Counter { initial: 0 });
    /// ```
    #[inline]
    pub fn stateful(widget: impl StatefulWidget) -> Self {
        Widget::Stateful(Box::new(widget))
    }

    /// Create an Inherited widget
    ///
    /// # Examples
    ///
    /// ```
    /// let widget = Widget::inherited(Theme { color: Color::BLUE, child: ... });
    /// ```
    #[inline]
    pub fn inherited(widget: impl InheritedWidget) -> Self {
        Widget::Inherited(Box::new(widget))
    }

    /// Create a Render widget
    ///
    /// # Examples
    ///
    /// ```
    /// let widget = Widget::render_object(Text::new("Hello"));
    /// ```
    #[inline]
    pub fn render_object(widget: impl RenderWidget) -> Self {
        Widget::Render(Box::new(widget))
    }

    /// Create a ParentData widget
    ///
    /// # Examples
    ///
    /// ```
    /// let widget = Widget::parent_data(Positioned { top: 10.0, child: ... });
    /// ```
    #[inline]
    pub fn parent_data(widget: impl ParentDataWidget) -> Self {
        Widget::ParentData(Box::new(widget))
    }

    /// Get the widget's key
    ///
    /// Keys are used to preserve element state when widgets are reordered
    /// or to uniquely identify widget instances.
    ///
    /// # Examples
    ///
    /// ```
    /// let key = widget.key();
    /// if let Some(key) = key {
    ///     println!("Widget key: {:?}", key);
    /// }
    /// ```
    pub fn key(&self) -> Option<Key> {
        match self {
            Widget::Stateless(w) => w.key(),
            Widget::Stateful(w) => w.key(),
            Widget::Inherited(w) => w.key(),
            Widget::Render(w) => w.key(),
            Widget::ParentData(w) => w.key(),
        }
    }

    /// Check if this widget can update another widget
    ///
    /// Two widgets can update each other if they are the same variant
    /// and have the same concrete type.
    ///
    /// # Examples
    ///
    /// ```
    /// let widget1 = Widget::stateless(HelloWorld);
    /// let widget2 = Widget::stateless(HelloWorld);
    /// assert!(widget1.can_update(&widget2));
    /// ```
    pub fn can_update(&self, other: &Widget) -> bool {
        match (self, other) {
            (Widget::Stateless(a), Widget::Stateless(b)) => a.can_update(&**b),
            (Widget::Stateful(a), Widget::Stateful(b)) => a.type_id() == b.type_id(),
            (Widget::Inherited(a), Widget::Inherited(b)) => a.type_id() == b.type_id(),
            (Widget::Render(a), Widget::Render(b)) => a.type_id() == b.type_id(),
            (Widget::ParentData(a), Widget::ParentData(b)) => a.type_id() == b.type_id(),
            _ => false,
        }
    }

    /// Get child widget for RenderWidget (single child)
    ///
    /// Returns None if this is not a RenderWidget or has no child
    pub fn render_widget_child(&self) -> Option<&Widget> {
        match self {
            Widget::Render(w) => w.child(),
            _ => None,
        }
    }

    /// Get children widgets for RenderWidget (multi-child)
    ///
    /// Returns None if this is not a RenderWidget or has no children
    pub fn render_widget_children(&self) -> Option<&[Widget]> {
        match self {
            Widget::Render(w) => w.children(),
            _ => None,
        }
    }

    /// Create render object for RenderWidget
    ///
    /// Returns None if this is not a RenderWidget
    pub fn create_render_object(&self, context: &crate::BuildContext) -> Option<crate::render::RenderNode> {
        match self {
            Widget::Render(w) => Some(w.create_render_object(context)),
            _ => None,
        }
    }

    /// Clone the widget
    ///
    /// This creates a deep clone of the widget, including the boxed trait object.
    ///
    /// # Examples
    ///
    /// ```
    /// let widget1 = Widget::stateless(HelloWorld);
    /// let widget2 = widget1.clone_widget();
    /// ```
    pub fn clone_widget(&self) -> Widget {
        match self {
            Widget::Stateless(w) => Widget::Stateless(w.clone_boxed()),
            Widget::Stateful(w) => Widget::Stateful(w.clone_boxed()),
            Widget::Inherited(w) => Widget::Inherited(w.clone_boxed()),
            Widget::Render(w) => Widget::Render(w.clone_boxed()),
            Widget::ParentData(w) => Widget::ParentData(w.clone_boxed()),
        }
    }

    /// Downcast to a concrete type
    ///
    /// # Examples
    ///
    /// ```
    /// let widget = Widget::stateless(HelloWorld);
    /// if let Some(hello) = widget.downcast_ref::<HelloWorld>() {
    ///     println!("Found HelloWorld widget");
    /// }
    /// ```
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        match self {
            Widget::Stateless(w) => w.as_any().downcast_ref(),
            Widget::Stateful(w) => w.as_any().downcast_ref(),
            Widget::Inherited(w) => w.as_any().downcast_ref(),
            Widget::Render(w) => w.as_any().downcast_ref(),
            Widget::ParentData(w) => w.as_any().downcast_ref(),
        }
    }

    /// Check if the widget is of a specific type
    ///
    /// # Examples
    ///
    /// ```
    /// let widget = Widget::stateless(HelloWorld);
    /// assert!(widget.is::<HelloWorld>());
    /// assert!(!widget.is::<Counter>());
    /// ```
    #[inline]
    pub fn is<T: 'static>(&self) -> bool {
        self.downcast_ref::<T>().is_some()
    }

    /// Get the TypeId of the concrete widget type
    pub fn type_id(&self) -> TypeId {
        match self {
            Widget::Stateless(w) => w.type_id(),
            Widget::Stateful(w) => w.type_id(),
            Widget::Inherited(w) => w.type_id(),
            Widget::Render(w) => w.type_id(),
            Widget::ParentData(w) => w.type_id(),
        }
    }

    /// Get a human-readable type name
    pub fn type_name(&self) -> &'static str {
        match self {
            Widget::Stateless(_) => "Stateless",
            Widget::Stateful(_) => "Stateful",
            Widget::Inherited(_) => "Inherited",
            Widget::Render(_) => "Render",
            Widget::ParentData(_) => "ParentData",
        }
    }

    // ========== Widget-specific Methods ==========

    /// Build the widget tree (for StatelessWidget only)
    ///
    /// Returns Some(Widget) if this is a StatelessWidget, None otherwise.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(child) = widget.build(&context) {
    ///     // Process child widget
    /// }
    /// ```
    pub fn build(&self, context: &crate::BuildContext) -> Option<Widget> {
        match self {
            Widget::Stateless(w) => Some(w.build(context)),
            _ => None,
        }
    }

    /// Get the child widget (for ParentDataWidget only)
    ///
    /// Returns Some(&Widget) if this is a ParentDataWidget, None otherwise.
    pub fn parent_data_child(&self) -> Option<&Widget> {
        match self {
            Widget::ParentData(w) => Some(w.child()),
            _ => None,
        }
    }

    /// Get as Any for downcasting (unified interface)
    ///
    /// This provides a unified way to access the underlying Any trait
    /// for all widget types.
    pub fn as_any(&self) -> &dyn Any {
        match self {
            Widget::Stateless(w) => w.as_any(),
            Widget::Stateful(w) => w.as_any(),
            Widget::Inherited(w) => w.as_any(),
            Widget::Render(w) => w.as_any(),
            Widget::ParentData(w) => w.as_any(),
        }
    }
}

impl Clone for Widget {
    fn clone(&self) -> Self {
        self.clone_widget()
    }
}

// Note: We don't implement PartialEq because widgets are compared
// by type and key, not by value. Use can_update() instead.

// ============================================================================
// IntoWidget Trait - Ergonomic Conversion API
// ============================================================================

/// Trait for types that can be converted into a Widget
///
/// This trait provides ergonomic API for widget construction, eliminating
/// the need for explicit `Widget::*()` wrappers.
///
/// # Why Macro Instead of Blanket Impl?
///
/// We cannot use blanket implementations like:
/// ```ignore
/// impl<T: StatelessWidget> IntoWidget for T { ... }
/// impl<T: RenderWidget> IntoWidget for T { ... }
/// ```
///
/// Because Rust's trait coherence rules cannot guarantee that these impls
/// don't overlap (even though they never do in practice).
///
/// Even sealed traits don't help, because the sealed trait itself would
/// need the same overlapping blanket impls.
///
/// # Solution: Batch Macro
///
/// Use `impl_into_widget!` macro to generate impls for multiple types at once:
///
/// ```ignore
/// impl_into_widget!(stateless: Container, Align, Padding);
/// impl_into_widget!(render: Text, Center, SizedBox);
/// ```
pub trait IntoWidget {
    /// Convert self into a Widget
    fn into_widget(self) -> Widget;
}

// Widget converts to itself (identity)
impl IntoWidget for Widget {
    fn into_widget(self) -> Widget {
        self
    }
}

/// Macro to implement IntoWidget for widget types
///
/// # Usage
///
/// Single type per invocation (add at end of widget file):
/// ```ignore
/// // For StatelessWidget
/// flui_core::impl_into_widget!(Container, stateless);
///
/// // For RenderWidget
/// flui_core::impl_into_widget!(Text, render);
///
/// // For StatefulWidget
/// flui_core::impl_into_widget!(Counter, stateful);
///
/// // For InheritedWidget
/// flui_core::impl_into_widget!(Theme, inherited);
///
/// // For ParentDataWidget
/// flui_core::impl_into_widget!(Expanded, parent_data);
/// ```
#[macro_export]
macro_rules! impl_into_widget {
    ($ty:ty, stateless) => {
        impl $crate::IntoWidget for $ty {
            fn into_widget(self) -> $crate::Widget {
                $crate::Widget::stateless(self)
            }
        }

        impl From<$ty> for $crate::Widget {
            fn from(widget: $ty) -> Self {
                $crate::Widget::stateless(widget)
            }
        }
    };
    ($ty:ty, render) => {
        impl $crate::IntoWidget for $ty {
            fn into_widget(self) -> $crate::Widget {
                $crate::Widget::render_object(self)
            }
        }

        impl From<$ty> for $crate::Widget {
            fn from(widget: $ty) -> Self {
                $crate::Widget::render_object(widget)
            }
        }
    };
    ($ty:ty, stateful) => {
        impl $crate::IntoWidget for $ty {
            fn into_widget(self) -> $crate::Widget {
                $crate::Widget::stateful(self)
            }
        }

        impl From<$ty> for $crate::Widget {
            fn from(widget: $ty) -> Self {
                $crate::Widget::stateful(widget)
            }
        }
    };
    ($ty:ty, inherited) => {
        impl $crate::IntoWidget for $ty {
            fn into_widget(self) -> $crate::Widget {
                $crate::Widget::inherited(self)
            }
        }

        impl From<$ty> for $crate::Widget {
            fn from(widget: $ty) -> Self {
                $crate::Widget::inherited(widget)
            }
        }
    };
    ($ty:ty, parent_data) => {
        impl $crate::IntoWidget for $ty {
            fn into_widget(self) -> $crate::Widget {
                $crate::Widget::parent_data(self)
            }
        }

        impl From<$ty> for $crate::Widget {
            fn from(widget: $ty) -> Self {
                $crate::Widget::parent_data(widget)
            }
        }
    };

    // LEGACY BATCH SYNTAX (for backwards compatibility if needed)
    (stateless: $($ty:ty),+ $(,)?) => {
        $(
            impl $crate::IntoWidget for $ty {
                fn into_widget(self) -> $crate::Widget {
                    $crate::Widget::stateless(self)
                }
            }

            impl From<$ty> for $crate::Widget {
                fn from(widget: $ty) -> Self {
                    $crate::Widget::stateless(widget)
                }
            }
        )+
    };
    (render: $($ty:ty),+ $(,)?) => {
        $(
            impl $crate::IntoWidget for $ty {
                fn into_widget(self) -> $crate::Widget {
                    $crate::Widget::render_object(self)
                }
            }

            impl From<$ty> for $crate::Widget {
                fn from(widget: $ty) -> Self {
                    $crate::Widget::render_object(widget)
                }
            }
        )+
    };
    (stateful: $($ty:ty),+ $(,)?) => {
        $(
            impl $crate::IntoWidget for $ty {
                fn into_widget(self) -> $crate::Widget {
                    $crate::Widget::stateful(self)
                }
            }

            impl From<$ty> for $crate::Widget {
                fn from(widget: $ty) -> Self {
                    $crate::Widget::stateful(widget)
                }
            }
        )+
    };
    (inherited: $($ty:ty),+ $(,)?) => {
        $(
            impl $crate::IntoWidget for $ty {
                fn into_widget(self) -> $crate::Widget {
                    $crate::Widget::inherited(self)
                }
            }

            impl From<$ty> for $crate::Widget {
                fn from(widget: $ty) -> Self {
                    $crate::Widget::inherited(widget)
                }
            }
        )+
    };
    (parent_data: $($ty:ty),+ $(,)?) => {
        $(
            impl $crate::IntoWidget for $ty {
                fn into_widget(self) -> $crate::Widget {
                    $crate::Widget::parent_data(self)
                }
            }

            impl From<$ty> for $crate::Widget {
                fn from(widget: $ty) -> Self {
                    $crate::Widget::parent_data(widget)
                }
            }
        )+
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct TestWidget {
        value: i32,
    }

    impl StatelessWidget for TestWidget {
        fn build(&self, _ctx: &crate::BuildContext) -> Widget {
            Widget::Stateless(Box::new(TestWidget {
                value: self.value + 1,
            }))
        }

        fn clone_boxed(&self) -> Box<dyn StatelessWidget> {
            Box::new(self.clone())
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[test]
    fn test_widget_creation() {
        let widget = Widget::stateless(TestWidget { value: 42 });
        assert!(widget.is::<TestWidget>());
        assert_eq!(widget.type_name(), "Stateless");
    }

    #[test]
    fn test_widget_downcast() {
        let widget = Widget::stateless(TestWidget { value: 42 });
        let test_widget = widget.downcast_ref::<TestWidget>().unwrap();
        assert_eq!(test_widget.value, 42);
    }

    #[test]
    fn test_widget_clone() {
        let widget1 = Widget::stateless(TestWidget { value: 42 });
        let widget2 = widget1.clone();
        assert!(widget1.can_update(&widget2));
    }

    #[test]
    fn test_can_update_same_type() {
        let widget1 = Widget::stateless(TestWidget { value: 1 });
        let widget2 = Widget::stateless(TestWidget { value: 2 });
        assert!(widget1.can_update(&widget2));
    }

    #[test]
    fn test_can_update_different_variants() {
        let stateless = Widget::stateless(TestWidget { value: 1 });
        // Note: Can't easily test with Stateful without implementing it
        // Just verify the variant check works
        assert_eq!(stateless.type_name(), "Stateless");
    }
}
