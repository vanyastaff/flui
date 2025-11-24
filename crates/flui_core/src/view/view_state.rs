//! View state trait for stateful views.
//!
//! Defines the interface for mutable state in `StatefulView<S>`.

use crate::view::BuildContext;

/// Mutable state for stateful views.
///
/// This trait is automatically satisfied by any type that is `Send + 'static`.
/// It serves as a marker and provides default lifecycle hooks.
///
/// # Purpose
///
/// ViewState represents the mutable, persistent part of a stateful view:
/// - Created once via `StatefulView::create_state()`
/// - Persists across rebuilds
/// - Passed as `&mut` to all lifecycle methods
///
/// # Lifecycle (called by StatefulView)
///
/// These are default implementations that can be overridden if needed,
/// but typically lifecycle is managed through `StatefulView` methods:
///
/// ```text
/// create_state()           → State created
/// init_state(&mut state)   → Element mounted
/// build(&mut state)        → UI built
/// did_update(&mut state)   → View config changed
/// deactivate(&mut state)   → Element deactivated
/// dispose(&mut state)      → Element destroyed
/// ```
///
/// # Example
///
/// ```rust,ignore
/// struct CounterState {
///     count: i32,
///     timer: Option<Timer>,
/// }
///
/// // ViewState is automatically implemented!
/// // Just need Send + 'static
/// ```
pub trait ViewState: Send + 'static {
    /// Called when dependencies change (optional).
    ///
    /// Override if state needs to react to InheritedWidget/Provider changes.
    fn did_change_dependencies(&mut self, _ctx: &BuildContext) {}
}

/// Blanket implementation for all Send + 'static types.
///
/// This makes any struct automatically a valid ViewState.
impl<T: Send + 'static> ViewState for T {
    // Uses default implementations
}
