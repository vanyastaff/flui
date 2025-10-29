//! Widget Traits - Object-safe traits for enum Widget
//!
//! This module defines the traits used by the Widget enum variants.
//! All traits are object-safe to enable dynamic dispatch.
//!
//! # Auto-Implementations
//!
//! All widget traits automatically implement `clone_boxed()` and `as_any()`
//! via blanket implementations. Users only need to implement `Clone` on their
//! widget types.

use std::any::{Any, TypeId};
use std::fmt;

use crate::foundation::Key;
use crate::BuildContext;
use crate::render::RenderNode;

use super::widget::Widget;

// ============================================================================
// Auto-Clone Helper Traits
// ============================================================================

/// Helper trait for auto-implementing clone_boxed on StatelessWidget
///
/// This trait is automatically implemented for all `Clone` types.
/// Users never need to implement this manually.
trait CloneStatelessWidget: fmt::Debug + Send + Sync + 'static {
    fn clone_box_stateless(&self) -> Box<dyn StatelessWidget>;
}

/// Blanket impl: All Clone StatelessWidgets get clone_boxed for free!
impl<T> CloneStatelessWidget for T
where
    T: StatelessWidget + Clone,
{
    fn clone_box_stateless(&self) -> Box<dyn StatelessWidget> {
        Box::new(self.clone())
    }
}

/// Helper trait for auto-implementing clone_boxed on StatefulWidget
trait CloneStatefulWidget: fmt::Debug + Send + Sync + 'static {
    fn clone_box_stateful(&self) -> Box<dyn StatefulWidget>;
}

/// Blanket impl for StatefulWidget
impl<T> CloneStatefulWidget for T
where
    T: StatefulWidget + Clone,
{
    fn clone_box_stateful(&self) -> Box<dyn StatefulWidget> {
        Box::new(self.clone())
    }
}

/// Helper trait for auto-implementing clone_boxed on InheritedWidget
trait CloneInheritedWidget: fmt::Debug + Send + Sync + 'static {
    fn clone_box_inherited(&self) -> Box<dyn InheritedWidget>;
}

/// Blanket impl for InheritedWidget
impl<T> CloneInheritedWidget for T
where
    T: InheritedWidget + Clone,
{
    fn clone_box_inherited(&self) -> Box<dyn InheritedWidget> {
        Box::new(self.clone())
    }
}

/// Helper trait for auto-implementing clone_boxed on RenderWidget
trait CloneRenderWidget: fmt::Debug + Send + Sync + 'static {
    fn clone_box_render(&self) -> Box<dyn RenderWidget>;
}

/// Blanket impl for RenderWidget
impl<T> CloneRenderWidget for T
where
    T: RenderWidget + Clone,
{
    fn clone_box_render(&self) -> Box<dyn RenderWidget> {
        Box::new(self.clone())
    }
}

/// Helper trait for auto-implementing clone_boxed on ParentDataWidget
trait CloneParentDataWidget: fmt::Debug + Send + Sync + 'static {
    fn clone_box_parent_data(&self) -> Box<dyn ParentDataWidget>;
}

/// Blanket impl for ParentDataWidget
impl<T> CloneParentDataWidget for T
where
    T: ParentDataWidget + Clone,
{
    fn clone_box_parent_data(&self) -> Box<dyn ParentDataWidget> {
        Box::new(self.clone())
    }
}

// ============================================================================
// Auto-AsAny Helper Traits
// ============================================================================

/// Helper trait for auto-implementing as_any on all widget types.
///
/// This trait is automatically implemented for all 'static types.
/// Users never need to implement this manually.
trait AsAnyWidget: fmt::Debug + Send + Sync + 'static {
    fn as_any_widget(&self) -> &dyn Any;
}

/// Blanket impl: All 'static types get as_any() for free!
impl<T> AsAnyWidget for T
where
    T: fmt::Debug + Send + Sync + 'static,
{
    fn as_any_widget(&self) -> &dyn Any {
        self
    }
}

// ============================================================================
// StatelessWidget Trait
// ============================================================================

/// StatelessWidget - widget without mutable state
///
/// Stateless widgets are pure functions from configuration to UI.
/// They rebuild from scratch whenever their configuration changes.
///
/// # When to Use
///
/// - Widget has no mutable state
/// - Widget is a pure function of its configuration
/// - Widget rebuilds completely on each update
///
/// # Object Safety
///
/// This trait is object-safe, allowing `Box<dyn StatelessWidget>`.
/// Required methods:
/// - No `Clone` bound (use `clone_boxed` instead)
/// - `as_any()` for downcasting
///
/// # Examples
///
/// ```
/// use flui_core::{StatelessWidget, BuildContext, Widget};
///
/// #[derive(Debug, Clone)]
/// struct Greeting {
///     name: String,
/// }
///
/// impl StatelessWidget for Greeting {
///     fn build(&self, context: &BuildContext) -> Widget {
///         Widget::render_object(Text::new(format!("Hello, {}!", self.name)))
///     }
///
///     // clone_boxed() and as_any() are auto-implemented!
/// }
/// ```
pub trait StatelessWidget: CloneStatelessWidget + AsAnyWidget {
    /// Build the widget tree
    ///
    /// This method is called whenever the widget needs to rebuild.
    /// It should return a Widget based on the current configuration.
    ///
    /// # Parameters
    ///
    /// - `context` - BuildContext for accessing inherited widgets
    ///
    /// # Returns
    ///
    /// A Widget representing the UI
    ///
    /// # Performance
    ///
    /// This method should be fast - it's called on every rebuild.
    /// Avoid expensive operations like:
    /// - Network requests
    /// - Heavy computations
    /// - Large allocations
    ///
    /// If you need expensive initialization, use StatefulWidget instead.
    ///
    /// # Purity
    ///
    /// This method should be pure - same inputs should produce same output.
    /// Don't:
    /// - Modify external state
    /// - Use random numbers (without seed)
    /// - Use current time (unless rebuilding on time change)
    fn build(&self, context: &BuildContext) -> Widget;

    /// Optional widget key for identity tracking
    ///
    /// Keys are used to preserve element state when widgets are reordered
    /// or when you need to uniquely identify a widget instance.
    fn key(&self) -> Option<Key> {
        None
    }

    /// Clone into a boxed trait object
    ///
    /// **Auto-implemented!** You don't need to implement this manually.
    /// Just make your widget `Clone` and this method is provided automatically.
    ///
    /// # How it Works
    ///
    /// This uses a blanket implementation via the `CloneStatelessWidget` helper trait.
    /// All you need is:
    /// ```ignore
    /// #[derive(Debug, Clone)]
    /// struct MyWidget { /* ... */ }
    ///
    /// impl StatelessWidget for MyWidget {
    ///     fn build(&self, ctx: &BuildContext) -> Widget { /* ... */ }
    ///     fn as_any(&self) -> &dyn Any { self }
    ///     // clone_boxed is automatic!
    /// }
    /// ```
    fn clone_boxed(&self) -> Box<dyn StatelessWidget> {
        self.clone_box_stateless()
    }

    /// Check if this widget can update another widget
    ///
    /// Two widgets can update each other if they have the same concrete type.
    /// Override this if you need custom update logic.
    fn can_update(&self, other: &dyn StatelessWidget) -> bool {
        self.type_id() == other.type_id()
    }

    /// Downcast support - get &dyn Any reference
    ///
    /// **Auto-implemented!** You don't need to implement this manually.
    ///
    /// This method is automatically provided via the `AsAnyWidget` helper trait.
    fn as_any(&self) -> &dyn Any {
        self.as_any_widget()
    }

    /// Get TypeId for type checking
    ///
    /// Default implementation uses as_any().
    fn type_id(&self) -> TypeId {
        self.as_any().type_id()
    }
}

// ============================================================================
// StatefulWidget Trait
// ============================================================================

/// StatefulWidget - widget with mutable state
///
/// Stateful widgets create a State object that persists across rebuilds.
/// The widget itself is immutable, but the State can be mutated.
///
/// # Architecture
///
/// ```text
/// StatefulWidget (immutable config)
///   ↓
/// create_state() → State object (mutable)
///   ↓
/// State::build() → Widget tree
/// ```
///
/// # When to Use
///
/// - Widget needs mutable state
/// - State persists across rebuilds
/// - Need lifecycle callbacks (initState, dispose, etc.)
///
/// # Examples
///
/// ```
/// use flui_core::{StatefulWidget, State, BuildContext, Widget};
///
/// #[derive(Debug, Clone)]
/// struct Counter {
///     initial: i32,
/// }
///
/// #[derive(Debug)]
/// struct CounterState {
///     count: i32,
/// }
///
/// impl StatefulWidget for Counter {
///     fn create_state(&self) -> Box<dyn State> {
///         Box::new(CounterState { count: self.initial })
///     }
///
///     fn clone_boxed(&self) -> Box<dyn StatefulWidget> {
///         Box::new(self.clone())
///     }
///
///     fn as_any(&self) -> &dyn std::any::Any {
///         self
///     }
/// }
///
/// impl State for CounterState {
///     fn build(&mut self, ctx: &BuildContext) -> Widget {
///         Widget::render_object(Text::new(format!("Count: {}", self.count)))
///     }
///
///     fn as_any(&self) -> &dyn std::any::Any {
///         self
///     }
///
///     fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
///         self
///     }
/// }
/// ```
pub trait StatefulWidget: CloneStatefulWidget + AsAnyWidget {
    /// Create the initial state
    ///
    /// This is called once when the widget is first created.
    /// Returns a boxed State object that will persist across rebuilds.
    fn create_state(&self) -> Box<dyn State>;

    /// Optional widget key
    fn key(&self) -> Option<Key> {
        None
    }

    /// Clone into a boxed trait object
    ///
    /// **Auto-implemented!** Just derive `Clone` on your widget.
    fn clone_boxed(&self) -> Box<dyn StatefulWidget> {
        self.clone_box_stateful()
    }

    /// Downcast support
    ///
    /// **Auto-implemented!** You don't need to implement this manually.
    fn as_any(&self) -> &dyn Any {
        self.as_any_widget()
    }

    /// Get TypeId
    fn type_id(&self) -> TypeId {
        self.as_any().type_id()
    }
}

// ============================================================================
// State Trait
// ============================================================================

/// State - mutable state for StatefulWidget
///
/// State objects persist across widget rebuilds and can be mutated.
/// They have lifecycle callbacks for initialization and cleanup.
///
/// # Lifecycle
///
/// ```text
/// create_state() → init_state() → build()
///                       ↓
///                  (user interaction)
///                       ↓
///                  set_state() → build()
///                       ↓
///                  did_update_widget() → build()
///                       ↓
///                   dispose()
/// ```
///
/// # Examples
///
/// ```
/// use flui_core::{State, BuildContext, Widget};
///
/// #[derive(Debug)]
/// struct MyState {
///     counter: i32,
/// }
///
/// impl State for MyState {
///     fn build(&mut self, ctx: &BuildContext) -> Widget {
///         Widget::render_object(Text::new(format!("{}", self.counter)))
///     }
///
///     fn init_state(&mut self, ctx: &BuildContext) {
///         println!("State initialized");
///     }
///
///     fn dispose(&mut self) {
///         println!("State disposed");
///     }
///
///     fn as_any(&self) -> &dyn std::any::Any {
///         self
///     }
///
///     fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
///         self
///     }
/// }
/// ```
pub trait State: fmt::Debug + Send + Sync + 'static {
    /// Build the widget tree with access to mutable state
    ///
    /// This method has access to `&mut self`, allowing it to read
    /// and modify the state.
    fn build(&mut self, context: &BuildContext) -> Widget;

    /// Called when the State is first created
    ///
    /// Use this for initialization that requires access to BuildContext.
    fn init_state(&mut self, _context: &BuildContext) {}

    /// Called when the widget configuration changes
    ///
    /// This is called after the widget is updated with a new configuration.
    /// The old widget is provided for comparison.
    fn did_update_widget(
        &mut self,
        _old_widget: &dyn StatefulWidget,
        _context: &BuildContext,
    ) {
    }

    /// Called when the State is permanently removed
    ///
    /// Use this for cleanup: canceling timers, closing streams, etc.
    fn dispose(&mut self) {}

    /// Mark the state as dirty and schedule a rebuild
    ///
    /// Call this after you modify the state and want to trigger a rebuild.
    /// This is typically called manually after modifying state fields.
    ///
    /// # Examples
    ///
    /// ```
    /// fn increment(&mut self) {
    ///     self.counter += 1;
    ///     self.mark_needs_build();
    /// }
    /// ```
    fn mark_needs_build(&mut self) {
        // FIXME: Mark element as dirty for rebuild
        // Default implementation does nothing
    }

    /// Downcast support - immutable reference
    fn as_any(&self) -> &dyn Any;

    /// Downcast support - mutable reference
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Get TypeId
    fn type_id(&self) -> TypeId {
        self.as_any().type_id()
    }
}

// ============================================================================
// InheritedWidget Trait
// ============================================================================

/// InheritedWidget - provides data down the widget tree
///
/// Inherited widgets allow descendant widgets to access data without
/// explicitly passing it through every level.
///
/// # How It Works
///
/// ```text
/// Theme (InheritedWidget)
///   ↓
/// Container
///   ↓
/// Button ← Can access Theme via BuildContext
/// ```
///
/// # Examples
///
/// ```
/// use flui_core::{InheritedWidget, BuildContext, Widget};
///
/// #[derive(Debug, Clone)]
/// struct Theme {
///     primary_color: Color,
///     child: Widget,
/// }
///
/// impl InheritedWidget for Theme {
///     fn child(&self) -> &Widget {
///         &self.child
///     }
///
///     fn update_should_notify(&self, old: &dyn InheritedWidget) -> bool {
///         if let Some(old_theme) = old.as_any().downcast_ref::<Theme>() {
///             self.primary_color != old_theme.primary_color
///         } else {
///             true
///         }
///     }
///
///     fn clone_boxed(&self) -> Box<dyn InheritedWidget> {
///         Box::new(self.clone())
///     }
///
///     fn as_any(&self) -> &dyn std::any::Any {
///         self
///     }
/// }
///
/// // Helper method for access
/// impl Theme {
///     pub fn of(ctx: &BuildContext) -> Color {
///         ctx.depend_on_inherited_widget::<Theme>()
///             .map(|theme| theme.primary_color)
///             .unwrap_or(Color::BLACK)
///     }
/// }
/// ```
pub trait InheritedWidget: CloneInheritedWidget + AsAnyWidget {
    /// Get the child widget
    fn child(&self) -> &Widget;

    /// Check if dependents should be notified of changes
    ///
    /// This is called when the widget updates. If it returns true,
    /// all widgets that depend on this InheritedWidget will rebuild.
    ///
    /// # Parameters
    ///
    /// - `old` - The previous version of this widget
    ///
    /// # Returns
    ///
    /// `true` if dependents should rebuild, `false` otherwise
    fn update_should_notify(&self, old: &dyn InheritedWidget) -> bool;

    /// Optional widget key
    fn key(&self) -> Option<Key> {
        None
    }

    /// Clone into a boxed trait object (auto-implemented)
    fn clone_boxed(&self) -> Box<dyn InheritedWidget> {
        self.clone_box_inherited()
    }

    /// Downcast support
    ///
    /// **Auto-implemented!** You don't need to implement this manually.
    fn as_any(&self) -> &dyn Any {
        self.as_any_widget()
    }

    /// Get TypeId
    fn type_id(&self) -> TypeId {
        self.as_any().type_id()
    }
}

// ============================================================================
// RenderWidget Trait
// ============================================================================

/// RenderWidget - creates and manages Render
///
/// Render widgets directly create Renders, which handle
/// layout and painting.
///
/// # Architecture
///
/// ```text
/// RenderWidget → Render
///                        ↓
///                     layout()
///                        ↓
///                     paint()
/// ```
pub trait RenderWidget: CloneRenderWidget + AsAnyWidget {
    /// Create a new RenderNode
    ///
    /// This is called once when the widget is first inserted into the tree.
    /// Return a RenderNode (enum) wrapping your render implementation.
    fn create_render_object(&self, context: &BuildContext) -> RenderNode;

    /// Update an existing RenderNode
    ///
    /// This is called when the widget configuration changes.
    /// Update the RenderNode to reflect the new configuration.
    fn update_render_object(&self, context: &BuildContext, render_object: &mut RenderNode);

    /// Get children for MultiChildRenderWidget
    ///
    /// Returns None for leaf widgets and SingleChild widgets.
    fn children(&self) -> Option<&[Widget]> {
        None
    }

    /// Get child for SingleChildRenderWidget
    ///
    /// Returns None for leaf widgets and MultiChild widgets.
    fn child(&self) -> Option<&Widget> {
        None
    }

    /// Optional widget key
    fn key(&self) -> Option<Key> {
        None
    }

    /// Clone into a boxed trait object (auto-implemented)
    fn clone_boxed(&self) -> Box<dyn RenderWidget> {
        self.clone_box_render()
    }

    /// Downcast support
    ///
    /// **Auto-implemented!** You don't need to implement this manually.
    fn as_any(&self) -> &dyn Any {
        self.as_any_widget()
    }

    /// Get TypeId
    fn type_id(&self) -> TypeId {
        self.as_any().type_id()
    }
}

// ============================================================================
// ParentDataWidget Trait
// ============================================================================

/// ParentDataWidget - attaches metadata to descendant Renders
///
/// ParentData widgets don't create their own elements. Instead, they
/// modify the parent data of descendant Renders.
///
/// # Examples
///
/// - Positioned (in Stack)
/// - Flexible (in Row/Column)
/// - TableCell (in Table)
pub trait ParentDataWidget: CloneParentDataWidget + AsAnyWidget {
    /// Get the child widget
    fn child(&self) -> &Widget;

    /// Apply parent data to a Render
    ///
    /// This is called to configure the parent data on descendant Renders.
    fn apply_parent_data(&self, render_object: &mut RenderNode);

    /// Optional widget key
    fn key(&self) -> Option<Key> {
        None
    }

    /// Clone into a boxed trait object (auto-implemented)
    fn clone_boxed(&self) -> Box<dyn ParentDataWidget> {
        self.clone_box_parent_data()
    }

    /// Downcast support
    ///
    /// **Auto-implemented!** You don't need to implement this manually.
    fn as_any(&self) -> &dyn Any {
        self.as_any_widget()
    }

    /// Get TypeId
    fn type_id(&self) -> TypeId {
        self.as_any().type_id()
    }
}
