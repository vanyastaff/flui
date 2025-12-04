//! `StatefulView` - Views with internal mutable state
//!
//! `StatefulView` is for views that need to maintain state between rebuilds.
//! Mirrors Flutter's `StatefulWidget` + `State<T>` pattern.
//!
//! # State Management Patterns
//!
//! FLUI provides two approaches for state management:
//!
//! ## 1. Manual State (Flutter-like)
//!
//! Use `StatefulView` with manual `ctx.mark_dirty()` calls:
//!
//! ```rust,ignore
//! struct CounterState { count: i32 }
//!
//! impl StatefulView for Counter {
//!     type State = CounterState;
//!
//!     fn build(&self, state: &mut Self::State, ctx: &dyn BuildContext) -> impl IntoView {
//!         Button::new(format!("{}", state.count))
//!             .on_tap({
//!                 let ctx = ctx.clone();  // Clone context for closure
//!                 move || {
//!                     state.count += 1;
//!                     ctx.mark_dirty();  // Trigger rebuild
//!                 }
//!             })
//!     }
//! }
//! ```
//!
//! ## 2. Reactive Signals (Recommended)
//!
//! Use `Signal<T>` from `flui-reactivity` for automatic rebuilds:
//!
//! ```rust,ignore
//! use flui_reactivity::Signal;
//!
//! struct CounterState { count: Signal<i32> }
//!
//! impl StatefulView for Counter {
//!     type State = CounterState;
//!
//!     fn create_state(&self) -> Self::State {
//!         CounterState { count: Signal::new(0) }
//!     }
//!
//!     fn build(&self, state: &mut Self::State, ctx: &dyn BuildContext) -> impl IntoView {
//!         let count = state.count;  // Signal is Copy!
//!         Button::new(format!("{}", count.get()))
//!             .on_tap(move || count.update(|n| n + 1))  // Auto-rebuild!
//!     }
//! }
//! ```
//!
//! **Why Signals?**
//! - No manual `mark_dirty()` calls
//! - `Signal<T>` is `Copy` - no `.clone()` needed
//! - Automatic dependency tracking
//! - Fine-grained updates (only affected views rebuild)
//!
//! # Lifecycle
//!
//! The lifecycle of a `StatefulView` follows Flutter's `State` lifecycle:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     LIFECYCLE DIAGRAM                        │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                              │
//! │  ┌──────────────┐                                           │
//! │  │ create_state │ ← Called once when element is created     │
//! │  └──────┬───────┘                                           │
//! │         │                                                    │
//! │         ▼                                                    │
//! │  ┌──────────────┐                                           │
//! │  │  init_state  │ ← Initialize state, subscribe to streams  │
//! │  └──────┬───────┘                                           │
//! │         │                                                    │
//! │         ▼                                                    │
//! │  ┌─────────────────────────┐                                │
//! │  │ did_change_dependencies │ ← Called when dependencies     │
//! │  └──────┬──────────────────┘   change (InheritedWidget)     │
//! │         │                                                    │
//! │         ▼                                                    │
//! │  ┌──────────────┐                                           │
//! │  │    build     │◄──────────────────┐                       │
//! │  └──────┬───────┘                   │                       │
//! │         │                           │                       │
//! │         ▼                           │ (rebuild triggered    │
//! │  ┌──────────────────┐               │  by signal.set() or   │
//! │  │ did_update_view  │───────────────┘  mark_dirty())        │
//! │  └──────┬───────────┘                                       │
//! │         │                                                    │
//! │         ▼ (element unmounted)                               │
//! │  ┌──────────────┐                                           │
//! │  │   dispose    │ ← Clean up resources, cancel subscriptions│
//! │  └──────────────┘                                           │
//! │                                                              │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use crate::state::ViewState;
use crate::{BuildContext, IntoView};

/// `StatefulView` - A view with internal mutable state
///
/// Use `StatefulView` when your view:
/// - Needs to maintain state that persists across rebuilds
/// - Has interactive behavior (buttons, forms, etc.)
/// - Needs to trigger rebuilds via state changes
/// - Needs to manage resources (streams, controllers, etc.)
///
/// # Example (with Signals - Recommended)
///
/// ```rust,ignore
/// use flui_reactivity::Signal;
///
/// struct Counter {
///     initial: i32,
/// }
///
/// struct CounterState {
///     count: Signal<i32>,
/// }
///
/// impl StatefulView for Counter {
///     type State = CounterState;
///
///     fn create_state(&self) -> Self::State {
///         CounterState { count: Signal::new(self.initial) }
///     }
///
///     fn init_state(&self, state: &mut Self::State, ctx: &dyn BuildContext) {
///         tracing::debug!("Counter initialized with count: {}", state.count.get());
///     }
///
///     fn build(&self, state: &mut Self::State, ctx: &dyn BuildContext) -> impl IntoView {
///         let count = state.count;  // Signal is Copy!
///         Column::new()
///             .child(Text::new(format!("Count: {}", count.get())))
///             .child(Button::new("+").on_tap(move || {
///                 count.update(|n| n + 1);  // Auto-triggers rebuild!
///             }))
///     }
///
///     fn dispose(&self, state: &mut Self::State, ctx: &dyn BuildContext) {
///         tracing::debug!("Counter disposed");
///     }
/// }
/// ```
///
/// # Lifecycle Methods (in order)
///
/// 1. [`create_state`](Self::create_state) - Create initial state object
/// 2. [`init_state`](Self::init_state) - Initialize state (subscriptions, etc.)
/// 3. [`did_change_dependencies`](Self::did_change_dependencies) - Dependencies changed
/// 4. [`build`](Self::build) - Build the UI
/// 5. [`did_update_view`](Self::did_update_view) - View config changed (on rebuild)
/// 6. [`dispose`](Self::dispose) - Clean up resources (on unmount)
///
/// # Thread Safety
///
/// Both the view and state must be `Send + Sync + 'static`.
pub trait StatefulView: Send + Sync + 'static {
    /// The state type for this view.
    ///
    /// State persists across rebuilds and is owned by the element.
    type State: ViewState;

    /// Create initial state.
    ///
    /// Called once when the element is first created. This is where you
    /// create the state object with initial values.
    ///
    /// **Flutter equivalent:** `State<T> createState()`
    ///
    /// # Note
    ///
    /// At this point, the element is not yet mounted, so you cannot access
    /// `BuildContext` or inherited widgets. Use [`init_state`](Self::init_state)
    /// for initialization that requires context.
    fn create_state(&self) -> Self::State;

    /// Initialize state after element is mounted.
    ///
    /// Called once after [`create_state`](Self::create_state), when the element
    /// has been inserted into the tree. This is the place to:
    ///
    /// - Subscribe to streams or listeners
    /// - Start animations
    /// - Fetch initial data
    /// - Access inherited widgets (via `ctx.depend_on<T>()`)
    ///
    /// **Flutter equivalent:** `State.initState()`
    ///
    /// # Default
    ///
    /// Default implementation does nothing.
    #[allow(unused_variables)]
    fn init_state(&self, state: &mut Self::State, ctx: &dyn BuildContext) {
        // Default: no-op
    }

    /// Called when an inherited widget dependency changes.
    ///
    /// This method is called:
    /// - Immediately after [`init_state`](Self::init_state)
    /// - Whenever an `InheritedWidget` that this view depends on changes
    ///
    /// Use this to react to changes in inherited data (theme, locale, etc.).
    ///
    /// **Flutter equivalent:** `State.didChangeDependencies()`
    ///
    /// # Default
    ///
    /// Default implementation does nothing.
    #[allow(unused_variables)]
    fn did_change_dependencies(&self, state: &mut Self::State, ctx: &dyn BuildContext) {
        // Default: no-op
    }

    /// Build the view with current state.
    ///
    /// Called during each rebuild. Return the child view(s) that represent
    /// the current UI for this state.
    ///
    /// **Flutter equivalent:** `State.build(BuildContext)`
    ///
    /// # Guidelines
    ///
    /// - This method may be called frequently (every frame during animations)
    /// - Avoid side effects - use lifecycle methods for those
    /// - Keep builds fast and idempotent
    fn build(&self, state: &mut Self::State, ctx: &dyn BuildContext) -> impl IntoView;

    /// Called when the view configuration changes.
    ///
    /// Called when the parent rebuilds and provides a new view configuration.
    /// Use this to update state based on new view properties.
    ///
    /// **Flutter equivalent:** `State.didUpdateWidget(T oldWidget)`
    ///
    /// # Parameters
    ///
    /// - `state`: The current state (mutable)
    /// - `old_view`: The previous view configuration
    ///
    /// # Default
    ///
    /// Default implementation does nothing.
    #[allow(unused_variables)]
    fn did_update_view(&self, state: &mut Self::State, old_view: &Self) {
        // Default: no-op
    }

    /// Called when the element is permanently removed from the tree.
    ///
    /// This is the place to:
    /// - Cancel stream subscriptions
    /// - Dispose controllers
    /// - Release resources
    /// - Stop animations
    ///
    /// **Flutter equivalent:** `State.dispose()`
    ///
    /// # Important
    ///
    /// After `dispose` is called, the state should not be used again.
    /// Do not call `mark_dirty()` or access context after dispose.
    ///
    /// # Default
    ///
    /// Default implementation does nothing.
    #[allow(unused_variables)]
    fn dispose(&self, state: &mut Self::State, ctx: &dyn BuildContext) {
        // Default: no-op
    }

    /// Called when the element is temporarily removed from the tree.
    ///
    /// This happens when the element might be reinserted later (e.g., during
    /// a `GlobalKey` reparenting). Use this for temporary cleanup.
    ///
    /// **Flutter equivalent:** `State.deactivate()`
    ///
    /// # Default
    ///
    /// Default implementation does nothing.
    #[allow(unused_variables)]
    fn deactivate(&self, state: &mut Self::State, ctx: &dyn BuildContext) {
        // Default: no-op
    }

    /// Called when the element is reinserted after being deactivated.
    ///
    /// This is the opposite of [`deactivate`](Self::deactivate).
    ///
    /// **Flutter equivalent:** `State.activate()`
    ///
    /// # Default
    ///
    /// Default implementation does nothing.
    #[allow(unused_variables)]
    fn activate(&self, state: &mut Self::State, ctx: &dyn BuildContext) {
        // Default: no-op
    }
}
