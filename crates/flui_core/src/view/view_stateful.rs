//! Stateful view trait.
//!
//! For views with persistent mutable state and full lifecycle.

use crate::element::IntoElement;
use crate::view::{BuildContext, Stateful, View, ViewState};

/// Stateful view - views with persistent mutable state.
///
/// Similar to Flutter's `StatefulWidget + State`. Separates immutable
/// configuration (view) from mutable state that persists across rebuilds.
///
/// # Architecture
///
/// ```text
/// Counter (View)          CounterState (State)
/// ──────────────          ────────────────────
/// initial: i32            count: i32
/// Clone + Send            Send
/// Recreated on update     Persists across builds
/// ```
///
/// # Lifecycle
///
/// ```text
/// 1. create_state()              → State created
/// 2. init_state(&mut state)      → Element mounted
/// 3. build(&mut state)           → UI built
///    ↓ (repeat on setState/updates)
/// 4. did_update(&mut state)      → View config changed
/// 5. deactivate(&mut state)      → Element deactivated
/// 6. dispose(&mut state)         → Element destroyed
/// ```
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Clone)]
/// struct Counter {
///     initial: i32,
/// }
///
/// struct CounterState {
///     count: i32,
/// }
///
/// impl StatefulView<CounterState> for Counter {
///     fn create_state(&self) -> CounterState {
///         CounterState {
///             count: self.initial,
///         }
///     }
///
///     fn init_state(&mut self, state: &mut CounterState, _ctx: &BuildContext) {
///         println!("Counter mounted! Count: {}", state.count);
///     }
///
///     fn build(&mut self, state: &mut CounterState, ctx: &BuildContext) -> impl IntoElement {
///         Column::new()
///             .child(Text::new(format!("Count: {}", state.count)))
///             .child(Button::new("++").on_press(move || {
///                 state.count += 1;
///                 ctx.mark_dirty();
///             }))
///     }
///
///     fn dispose(&mut self, state: &mut CounterState, _ctx: &BuildContext) {
///         println!("Counter disposed! Final: {}", state.count);
///     }
/// }
/// ```
///
/// # When to Use
///
/// - Interactive widgets (buttons, forms, etc)
/// - User input handling
/// - Subscriptions (streams, timers)
/// - Complex lifecycle management
///
/// # Comparison to Flutter
///
/// | Flutter | FLUI |
/// |---------|------|
/// | `StatefulWidget` | View (implements `StatefulView`) |
/// | `State<T>` | State struct (separate) |
/// | `createState()` | `create_state()` |
/// | `initState()` | `init_state()` |
/// | `build()` | `build()` |
/// | `didUpdateWidget()` | `did_update()` |
/// | `deactivate()` | `deactivate()` |
/// | `dispose()` | `dispose()` |
pub trait StatefulView<S: ViewState>: Clone + Send + 'static {
    /// Create initial state.
    ///
    /// Called once when view is first mounted. Override to customize
    /// initial state from view props.
    ///
    /// # Default Implementation
    ///
    /// Uses `S::default()` if state implements `Default`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn create_state(&self) -> CounterState {
    ///     CounterState {
    ///         count: self.initial,  // From view props
    ///     }
    /// }
    /// ```
    fn create_state(&self) -> S
    where
        S: Default,
    {
        S::default()
    }

    /// Initialize state after mounting (optional).
    ///
    /// Called once after state is created and element is mounted to tree.
    ///
    /// Use for:
    /// - Setting up subscriptions
    /// - Creating timers/streams
    /// - Depending on providers
    /// - Any initialization requiring `BuildContext`
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn init_state(&mut self, state: &mut TimerState, ctx: &BuildContext) {
    ///     let ctx = ctx.clone();
    ///     state.timer = Some(Timer::periodic(Duration::from_secs(1), move || {
    ///         ctx.mark_dirty();
    ///     }));
    /// }
    /// ```
    fn init_state(&mut self, _state: &mut S, _ctx: &BuildContext) {}

    /// Build UI with state.
    ///
    /// Called on every rebuild. Can mutate state (triggers rebuild via `ctx.mark_dirty()`).
    ///
    /// # Parameters
    ///
    /// - `state`: Mutable reference to persistent state
    /// - `ctx`: Build context for tree queries and marking dirty
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn build(&mut self, state: &mut CounterState, ctx: &BuildContext) -> impl IntoElement {
    ///     Column::new()
    ///         .child(Text::new(format!("Count: {}", state.count)))
    ///         .child(Button::new("++").on_press(move || {
    ///             state.count += 1;
    ///             ctx.mark_dirty();  // Schedule rebuild
    ///         }))
    /// }
    /// ```
    fn build(&mut self, state: &mut S, ctx: &BuildContext) -> impl IntoElement;

    /// Called when dependencies change (optional).
    ///
    /// For views that depend on `InheritedWidget` or `Provider`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn did_change_dependencies(&mut self, state: &mut State, ctx: &BuildContext) {
    ///     let theme = ctx.depend_on::<Theme>();
    ///     state.color = theme.primary_color;
    /// }
    /// ```
    fn did_change_dependencies(&mut self, _state: &mut S, _ctx: &BuildContext) {}

    /// Called when view configuration updates (optional).
    ///
    /// View is cloned with new props from parent. Override to update
    /// state based on new configuration.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn did_update(&mut self, state: &mut State, ctx: &BuildContext) {
    ///     // View was updated with new `initial` value
    ///     if self.initial != state.count {
    ///         state.count = self.initial;
    ///     }
    /// }
    /// ```
    fn did_update(&mut self, _state: &mut S, _ctx: &BuildContext) {}

    /// Called when element is deactivated (optional).
    ///
    /// Element removed from tree but might be reinserted.
    fn deactivate(&mut self, _state: &mut S, _ctx: &BuildContext) {}

    /// Called when element is permanently removed (optional).
    ///
    /// Use for cleanup: cancel subscriptions, stop timers, free resources.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn dispose(&mut self, state: &mut TimerState, _ctx: &BuildContext) {
    ///     state.timer = None;  // Stop timer
    ///     state.subscription = None;  // Unsubscribe
    /// }
    /// ```
    fn dispose(&mut self, _state: &mut S, _ctx: &BuildContext) {}
}

/// Auto-implement `View<Stateful<S>>` for all `StatefulView<S>`.
///
/// This allows `StatefulView` to integrate with the internal protocol system.
impl<V, S> View<Stateful<S>> for V
where
    V: StatefulView<S>,
    S: ViewState,
{
    fn _build(&mut self, _ctx: &BuildContext) -> crate::element::Element {
        // Note: This is never called directly.
        // StatefulViewWrapper manages state and calls StatefulView::build().
        unreachable!("StatefulView::_build should not be called directly")
    }
}
