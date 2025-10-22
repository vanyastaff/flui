//! Core Widget trait definitions
//!
//! This module defines the fundamental traits that make up the widget system:
//! - DynWidget: Object-safe base trait for heterogeneous collections
//! - Widget: Trait with associated types for zero-cost element creation
//! - StatelessWidget: Immutable widgets that build once
//! - StatefulWidget: Widgets with mutable state
//! - State: Mutable state object for StatefulWidget

use std::any::Any;
use std::fmt;

use downcast_rs::{impl_downcast, DowncastSync};

use crate::context::Context;
use crate::element::{DynElement, ComponentElement, Element};
use crate::widget::DynWidget;

/// Widget - Trait with associated types for zero-cost element creation
///
/// This trait extends `DynWidget` with associated types, enabling zero-cost
/// element creation when working with concrete widget types.
///
/// # Two-Trait Pattern
///
/// - **DynWidget** - Object-safe, for `Box<dyn DynWidget>` collections
/// - **Widget** (this trait) - Has associated types, for concrete types
///
/// All types implementing `Widget` automatically implement `DynWidget` via a blanket impl.
///
/// # Three Types of Widgets
///
/// 1. **StatelessWidget** - builds once, no mutable state
/// 2. **StatefulWidget** - creates a State object that persists
/// 3. **RenderObjectWidget** - directly controls layout and painting
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct MyWidget {
///     title: String,
/// }
///
/// impl Widget for MyWidget {
///     type Element = ComponentElement<Self>;
///
///     fn into_element(self) -> Self::Element {
///         ComponentElement::new(self)  // ✅ Zero-cost! No Box!
///     }
/// }
///
/// // DynWidget is automatically implemented via blanket impl
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be used as a Widget",
    label = "this type doesn't implement `Widget`",
    note = "consider implementing `Widget`, `StatelessWidget`, or `StatefulWidget` for `{Self}`"
)]
pub trait Widget: DynWidget + Sized + Clone {
    /// Associated element type
    ///
    /// This allows the compiler to know the exact element type at compile time,
    /// enabling zero-cost element creation.
    type Element: Element;

    /// Consume self and create element (zero-cost)
    ///
    /// This moves the widget into the element without boxing or dynamic dispatch.
    /// For trait objects, use `DynWidget::create_element()` instead.
    fn into_element(self) -> Self::Element;
}

/// Blanket implementation of DynWidget for all Widget types
///
/// This allows any `Widget` implementation to be used as `Box<dyn DynWidget>`.
/// The `create_element()` method clones the widget and calls `into_element()`.
impl<T: Widget> DynWidget for T {
    fn create_element(&self) -> Box<dyn DynElement> {
        // Clone self and convert to concrete element, then box it as DynElement
        Box::new(self.clone().into_element())
    }

    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    fn can_update(&self, other: &dyn DynWidget) -> bool {
        // Same type required
        if self.type_id() != other.type_id() {
            return false;
        }

        // Check keys
        match (self.key(), other.key()) {
            (Some(k1), Some(k2)) => k1.id() == k2.id(),
            (None, None) => true,
            _ => false,
        }
    }
}

/// StatelessWidget - immutable widget that builds once
///
/// Stateless widgets don't hold any mutable state - all configuration comes from
/// their fields which are immutable.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct Greeting {
///     name: String,
/// }
///
/// impl StatelessWidget for Greeting {
///     fn build(&self, _context: &Context) -> Box<dyn DynWidget> {
///         Box::new(Text::new(format!("Hello, {}!", self.name)))
///     }
/// }
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be used as a StatelessWidget",
    label = "this type doesn't implement `StatelessWidget`",
    note = "implement the `build(&self, context: &Context) -> Box<dyn DynWidget>` method for `{Self}`"
)]
pub trait StatelessWidget: fmt::Debug + Clone + Send + Sync + 'static {
    /// Build this widget's child widget tree
    ///
    /// Called when the widget is first built or when it needs to rebuild.
    /// Should return the root widget of the child tree.
    fn build(&self, context: &Context) -> Box<dyn DynWidget>;
}

/// Automatically implement Widget for all StatelessWidgets
impl<T: StatelessWidget> Widget for T {
    type Element = ComponentElement<T>;

    fn into_element(self) -> ComponentElement<T> {
        ComponentElement::new(self)
    }
}

// DynWidget is automatically implemented for all Widget types via the blanket impl above

/// StatefulWidget - widget with mutable state
///
/// The widget itself is immutable, but the State can be mutated.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct Counter {
///     initial_value: i32,
/// }
///
/// impl StatefulWidget for Counter {
///     type State = CounterState;
///
///     fn create_state(&self) -> Self::State {
///         CounterState {
///             count: self.initial_value,
///         }
///     }
/// }
///
/// #[derive(Debug)]
/// struct CounterState {
///     count: i32,
/// }
///
/// impl State for CounterState {
///     fn build(&mut self, _context: &Context) -> Box<dyn DynWidget> {
///         Box::new(Text::new(format!("Count: {}", self.count)))
///     }
/// }
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be used as a StatefulWidget",
    label = "this type doesn't implement `StatefulWidget`",
    note = "implement `create_state(&self) -> Self::State` and define an associated `State` type for `{Self}`"
)]
pub trait StatefulWidget: fmt::Debug + Clone + Send + Sync + 'static {
    /// Associated State type
    type State: State;

    /// Create the state object
    ///
    /// Called once when the element is first mounted.
    fn create_state(&self) -> Self::State;
}

// ============================================================================
// WHY NO BLANKET IMPL FOR StatefulWidget?
// ============================================================================
//
// We CANNOT add a blanket impl like:
//
//   impl<T: StatefulWidget> Widget for T { ... }
//
// Because it conflicts with:
//
//   impl<T: StatelessWidget> Widget for T { ... }
//
// Rust's coherence rules prevent overlapping blanket implementations, even though
// StatelessWidget and StatefulWidget are mutually exclusive in practice.
//
// ATTEMPTED SOLUTIONS THAT DON'T WORK:
//
// 1. ❌ Negative trait bounds: `T: !StatelessWidget`
//    - Unstable feature, not available in stable Rust
//    - RFC 586: https://github.com/rust-lang/rfcs/pull/586
//
// 2. ❌ Sealed marker traits
//    - Creates circular dependency (trait requires marker, marker requires trait)
//
// 3. ❌ Specialization
//    - Unstable feature, complex, and may never stabilize
//
// THE CORRECT SOLUTION: MACROS
//
// Macros are the idiomatic Rust solution for this pattern. They provide:
// ✅ Type safety at compile time
// ✅ Zero runtime cost
// ✅ Clear, explicit code
// ✅ Works in stable Rust
//
// One extra line per widget is a small price for safety!
//
// ============================================================================

/// Macro to implement Widget for StatefulWidget types
///
/// This macro generates the Widget implementation for a StatefulWidget type.
/// Use this for all StatefulWidget implementations.
///
/// # Why a macro?
///
/// We cannot use a blanket impl like `impl<T: StatefulWidget> Widget for T` because
/// it would conflict with the existing `impl<T: StatelessWidget> Widget for T`.
/// Rust's trait coherence rules don't allow overlapping blanket implementations,
/// even though StatelessWidget and StatefulWidget are mutually exclusive in practice.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct Counter {
///     initial: i32,
/// }
///
/// impl StatefulWidget for Counter {
///     type State = CounterState;
///     fn create_state(&self) -> Self::State {
///         CounterState { count: self.initial }
///     }
/// }
///
/// impl_widget_for_stateful!(Counter);
/// ```
#[macro_export]
macro_rules! impl_widget_for_stateful {
    ($widget_type:ty) => {
        impl $crate::Widget for $widget_type {
            type Element = $crate::StatefulElement<$widget_type>;

            fn into_element(self) -> Self::Element {
                $crate::StatefulElement::new(self)
            }
        }
    };
}

/// State - mutable state for StatefulWidget
///
/// The state object persists across rebuilds, while the widget is recreated.
///
/// The trait provides downcasting capabilities via the `downcast-rs` crate.
///
/// # Enhanced Lifecycle
///
/// 1. **init_state()** - Called once when state is created and element mounted
/// 2. **did_change_dependencies()** - Called after initState and when InheritedWidget dependencies change
/// 3. **build()** - Called to build the widget tree (can be called multiple times)
/// 4. **did_update_widget()** - Called when widget configuration changes
/// 5. **reassemble()** - Called during hot reload (development only)
/// 6. **deactivate()** - Called when element removed from tree (might be reinserted)
/// 7. **activate()** - Called when element reinserted after deactivate()
/// 8. **dispose()** - Called when state is permanently removed from tree
///
/// # Lifecycle State Tracking
///
/// The State object tracks its lifecycle through the `StateLifecycle` enum to
/// enforce correct ordering and prevent operations on unmounted state.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be used as a State",
    label = "this type doesn't implement `State`",
    note = "State types must implement `State` trait and provide a `build(&mut self, context: &Context) -> Box<dyn DynWidget>` method"
)]
pub trait State: DowncastSync + fmt::Debug {
    /// Build the widget tree
    ///
    /// Called whenever the state needs to rebuild. Should return the root widget
    /// of the child tree.
    fn build(&mut self, context: &Context) -> Box<dyn DynWidget>;

    /// Called when state is first created
    ///
    /// Use this for initialization that depends on being in the tree.
    /// Called once when the element is mounted.
    ///
    /// # Lifecycle Order
    /// 1. State created via create_state()
    /// 2. **init_state()** <- You are here
    /// 3. did_change_dependencies()
    /// 4. build()
    fn init_state(&mut self) {}

    /// Called when InheritedWidget dependencies change
    ///
    /// This is called:
    /// - Once after init_state() on first build
    /// - Whenever an InheritedWidget that this state depends on changes
    ///
    /// Use this to respond to changes in InheritedWidgets obtained via
    /// `Context::depend_on_inherited_widget()`.
    ///
    /// 
    ///
    /// This callback enables proper dependency tracking with InheritedWidgets.
    fn did_change_dependencies(&mut self) {}

    /// Called when widget configuration changes
    ///
    /// The old widget is passed for comparison. Use this to detect changes
    /// and update internal state if needed.
    ///
    /// # Example
    /// ```rust,ignore
    /// fn did_update_widget(&mut self, old_widget: &dyn Any) {
    ///     if let Some(old) = old_widget.downcast_ref::<MyWidget>() {
    ///         if old.config != self.widget().config {
    ///             // Handle config change
    ///         }
    ///     }
    /// }
    /// ```
    fn did_update_widget(&mut self, _old_widget: &dyn Any) {}

    /// Called during hot reload (development only)
    ///
    /// This gives the state a chance to reinitialize data that was prepared
    /// in the constructor or init_state(), as if the object was newly created.
    ///
    /// 
    ///
    /// Enables hot reload support for development workflows.
    fn reassemble(&mut self) {}

    /// Called when element is removed from tree
    ///
    /// The element may be reinserted into the tree at a different location.
    /// If you need to cleanup resources, wait for dispose() instead.
    ///
    /// After deactivate(), the element might be:
    /// - Reinserted (activate() will be called)
    /// - Permanently removed (dispose() will be called)
    ///
    /// 
    ///
    /// Supports element reparenting and GlobalKey scenarios.
    fn deactivate(&mut self) {}

    /// Called when element is reinserted into tree
    ///
    /// This is called when a deactivated element is reinserted into the tree
    /// at a new location (e.g., via GlobalKey reparenting).
    ///
    /// 
    ///
    /// Supports element reparenting and GlobalKey scenarios.
    fn activate(&mut self) {}

    /// Called when state is permanently removed from tree
    ///
    /// Use this for cleanup like canceling timers, unsubscribing from streams, etc.
    /// After dispose() is called, the state should never be used again.
    ///
    /// # Lifecycle Order
    /// 1. deactivate()
    /// 2. **dispose()** <- You are here
    /// 3. State is defunct and cannot be used
    fn dispose(&mut self) {}

    /// Check if state is mounted (managed by StatefulElement)
    ///
    /// Returns `true` if the state is currently in the tree and can call setState.
    /// Returns `false` if the state has not been mounted yet or has been disposed.
    ///
    /// # Naming Convention
    ///
    /// This method follows the Rust API Guidelines (C-QUESTION) by using the
    /// `is_*` prefix for boolean predicates.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if self.is_mounted() {
    ///     self.set_state(|| {
    ///         self.counter += 1;
    ///     });
    /// }
    /// ```
    #[must_use]
    fn is_mounted(&self) -> bool {
        true // Default for backward compatibility
    }

    /// Get lifecycle state (managed by framework)
    ///
    /// Returns the current lifecycle state of this State object.
    /// The default implementation returns `Ready` for backward compatibility.
    #[must_use]
    fn lifecycle(&self) -> crate::StateLifecycle {
        crate::StateLifecycle::Ready
    }
}

// Enable downcasting for State trait objects
impl_downcast!(sync State);