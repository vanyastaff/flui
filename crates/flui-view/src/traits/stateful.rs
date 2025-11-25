//! Stateful view trait.
//!
//! For views with persistent mutable state and full lifecycle.

use crate::into_element::IntoElement;
use crate::state::ViewState;

// ============================================================================
// STATEFUL VIEW TRAIT
// ============================================================================

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
/// #[derive(Default)]
/// struct CounterState {
///     count: i32,
/// }
///
/// impl StatefulView for Counter {
///     type State = CounterState;
///     type Output = Column;
///
///     fn create_state(&self) -> CounterState {
///         CounterState { count: self.initial }
///     }
///
///     fn build(&self, state: &mut CounterState) -> Self::Output {
///         Column::new()
///             .child(Text::new(format!("Count: {}", state.count)))
///             .child(Button::new("++").on_press(|| state.count += 1))
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
pub trait StatefulView: Clone + Send + 'static {
    /// State type for this view.
    type State: ViewState;

    /// Output type from build.
    type Output: IntoElement;

    /// Create initial state.
    ///
    /// Called once when view is first mounted. Override to customize
    /// initial state from view props.
    fn create_state(&self) -> Self::State;

    /// Initialize state after mounting (optional).
    ///
    /// Called once after state is created and element is mounted to tree.
    ///
    /// Use for:
    /// - Setting up subscriptions
    /// - Creating timers/streams
    /// - Any initialization requiring mounted context
    #[allow(unused_variables)]
    fn init_state(&self, state: &mut Self::State) {}

    /// Build UI with state.
    ///
    /// Called on every rebuild. Returns the view output.
    ///
    /// # Parameters
    ///
    /// - `state`: Mutable reference to persistent state
    fn build(&self, state: &mut Self::State) -> Self::Output;

    /// Called when view configuration updates (optional).
    ///
    /// View is cloned with new props from parent. Override to update
    /// state based on new configuration.
    #[allow(unused_variables)]
    fn did_update(&self, state: &mut Self::State) {}

    /// Called when element is deactivated (optional).
    ///
    /// Element removed from tree but might be reinserted.
    #[allow(unused_variables)]
    fn deactivate(&self, state: &mut Self::State) {}

    /// Called when element is permanently removed (optional).
    ///
    /// Use for cleanup: cancel subscriptions, stop timers, free resources.
    #[allow(unused_variables)]
    fn dispose(&self, state: &mut Self::State) {}
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct TestCounter {
        initial: i32,
    }

    #[derive(Default)]
    struct TestCounterState {
        count: i32,
    }

    impl StatefulView for TestCounter {
        type State = TestCounterState;
        type Output = ();

        fn create_state(&self) -> TestCounterState {
            TestCounterState {
                count: self.initial,
            }
        }

        fn build(&self, state: &mut TestCounterState) -> Self::Output {
            let _ = state.count;
        }
    }

    #[test]
    fn test_stateful_view_create_state() {
        let view = TestCounter { initial: 42 };
        let state = view.create_state();
        assert_eq!(state.count, 42);
    }

    #[test]
    fn test_stateful_view_build() {
        let view = TestCounter { initial: 0 };
        let mut state = view.create_state();
        view.build(&mut state);
    }

    #[test]
    fn test_stateful_view_lifecycle() {
        let view = TestCounter { initial: 0 };
        let mut state = view.create_state();

        view.init_state(&mut state);
        view.build(&mut state);
        view.did_update(&mut state);
        view.deactivate(&mut state);
        view.dispose(&mut state);
    }
}
