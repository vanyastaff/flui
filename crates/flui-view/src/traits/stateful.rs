//! `StatefulView` - Views with internal mutable state
//!
//! `StatefulView` is for views that need to maintain state between rebuilds.

use flui_element::IntoElement;

use crate::state::ViewState;
use flui_element::BuildContext;

/// `StatefulView` - A view with internal mutable state
///
/// Use `StatefulView` when your view:
/// - Needs to maintain state that persists across rebuilds
/// - Has interactive behavior (buttons, forms, etc.)
/// - Needs to trigger rebuilds via `setState`
///
/// # Example
///
/// ```rust,ignore
/// struct Counter {
///     initial: i32,
/// }
///
/// struct CounterState {
///     count: i32,
/// }
///
/// impl StatefulView for Counter {
///     type State = CounterState;
///
///     fn create_state(&self) -> Self::State {
///         CounterState { count: self.initial }
///     }
///
///     fn build(&self, state: &mut Self::State, ctx: &BuildContext) -> impl IntoElement {
///         Column::new()
///             .child(Text::new(format!("Count: {}", state.count)))
///             .child(Button::new("+").on_press(|| {
///                 state.count += 1;
///                 ctx.mark_dirty();
///             }))
///     }
/// }
/// ```
///
/// # Lifecycle
///
/// 1. `create_state()` - Called once when element is first created
/// 2. `build()` - Called on each rebuild with current state
/// 3. State persists until element is unmounted
///
/// # Thread Safety
///
/// Both the view and state must be `Send + 'static`.
pub trait StatefulView: Send + Sync + 'static {
    /// The state type for this view
    type State: ViewState;

    /// Create initial state
    ///
    /// Called once when the element is first mounted.
    fn create_state(&self) -> Self::State;

    /// Build the view with current state
    ///
    /// Called during each rebuild. Modify state as needed,
    /// then return the child element(s).
    fn build(&self, state: &mut Self::State, ctx: &dyn BuildContext) -> impl IntoElement;

    /// Called when view configuration changes
    ///
    /// Override to update state when new view config is received.
    /// Default implementation does nothing.
    fn did_update_view(&self, _state: &mut Self::State, _old_view: &Self) {}
}
