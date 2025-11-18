//! Reducer hook for state management with actions.
//!
//! The `use_reducer` hook provides Redux-style state management with reducer functions.
//! It's useful for complex state logic that involves multiple sub-values or when the next state
//! depends on the previous one.

use crate::context::HookContext;
use crate::signal::Signal;
use crate::traits::Hook;
use std::marker::PhantomData;
use std::sync::Arc;

/// Dispatch function for sending actions to a reducer.
///
/// This is similar to Redux's dispatch function.
#[derive(Clone)]
pub struct Dispatch<A> {
    signal: Signal<ActionState<A>>,
}

impl<A> Dispatch<A>
where
    A: Send + 'static,
{
    /// Dispatch an action to the reducer.
    ///
    /// This will call the reducer function with the current state and the action,
    /// and update the state with the result.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// dispatch.send(Action::Increment);
    /// dispatch.send(Action::Add(5));
    /// ```
    pub fn send(&self, action: A) {
        self.signal.update_mut(|state| {
            state.pending_action = Some(action);
        });
    }
}

impl<A> std::fmt::Debug for Dispatch<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dispatch")
            .field("signal_id", &self.signal.id())
            .finish()
    }
}

/// Internal state for tracking pending actions.
#[derive(Clone)]
struct ActionState<A> {
    pending_action: Option<A>,
}

impl<A> ActionState<A> {
    fn new() -> Self {
        Self {
            pending_action: None,
        }
    }
}

/// Reducer function type.
///
/// Takes the current state and an action, returns the new state.
pub type Reducer<S, A> = Arc<dyn Fn(&S, A) -> S + Send + Sync>;

/// Hook state for ReducerHook.
pub struct ReducerHookState<S, A> {
    state_signal: Signal<S>,
    action_signal: Signal<ActionState<A>>,
    reducer: Reducer<S, A>,
}

impl<S, A> std::fmt::Debug for ReducerHookState<S, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReducerHookState")
            .field("state_signal", &self.state_signal.id())
            .field("action_signal", &self.action_signal.id())
            .finish_non_exhaustive()
    }
}

/// Reducer hook implementation.
///
/// Provides Redux-style state management with reducer functions.
pub struct ReducerHook<S, A>(PhantomData<(S, A)>);

impl<S, A> Hook for ReducerHook<S, A>
where
    S: Clone + Send + 'static,
    A: Clone + Send + 'static,
{
    type State = ReducerHookState<S, A>;
    type Input = (S, Reducer<S, A>);
    type Output = (Signal<S>, Dispatch<A>);

    fn create(input: Self::Input) -> Self::State {
        let (initial_state, reducer) = input;

        // Create signal for state
        let state_signal = Signal::new(initial_state);

        // Create signal for actions
        let action_signal = Signal::new(ActionState::<A>::new());

        ReducerHookState {
            state_signal,
            action_signal,
            reducer,
        }
    }

    fn update(state: &mut Self::State, _input: Self::Input) -> Self::Output {
        // Check if there's a pending action
        let pending_action = state.action_signal.get().pending_action.clone();

        if let Some(action) = pending_action {
            // Apply the reducer
            let current_state = state.state_signal.get();
            let new_state = (state.reducer)(&current_state, action);
            state.state_signal.set(new_state);

            // Clear the pending action
            state.action_signal.update_mut(|s| {
                s.pending_action = None;
            });
        }

        let dispatch = Dispatch {
            signal: state.action_signal.clone(), // Signal<T> derives Clone
        };

        (state.state_signal.clone(), dispatch) // Signal<T> derives Clone
    }

    fn cleanup(_state: Self::State) {
        // Signals are cleaned up automatically by SignalRuntime
    }
}

/// Create a reducer for state management.
///
/// Similar to Redux's useReducer. The reducer function takes the current state and an action,
/// and returns the new state.
///
/// # Example
///
/// ```rust,ignore
/// enum Action {
///     Increment,
///     Decrement,
///     Add(i32),
/// }
///
/// let reducer = Arc::new(|state: &i32, action: Action| match action {
///     Action::Increment => state + 1,
///     Action::Decrement => state - 1,
///     Action::Add(n) => state + n,
/// });
///
/// let (count, dispatch) = use_reducer(ctx, 0, reducer);
///
/// // Later...
/// dispatch.send(Action::Increment);
/// dispatch.send(Action::Add(5));
/// ```
pub fn use_reducer<S, A>(
    ctx: &mut HookContext,
    initial_state: S,
    reducer: Reducer<S, A>,
) -> (Signal<S>, Dispatch<A>)
where
    S: Clone + Send + 'static,
    A: Clone + Send + 'static,
{
    ctx.use_hook::<ReducerHook<S, A>>((initial_state, reducer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ComponentId;

    #[derive(Debug, Clone, PartialEq)]
    enum CounterAction {
        Increment,
        Decrement,
        Add(i32),
        Reset,
    }

    #[test]
    fn test_reducer_basic() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let reducer = Arc::new(|state: &i32, action: CounterAction| match action {
            CounterAction::Increment => state + 1,
            CounterAction::Decrement => state - 1,
            CounterAction::Add(n) => state + n,
            CounterAction::Reset => 0,
        });

        let (count, dispatch) = use_reducer(&mut ctx, 0, reducer);
        assert_eq!(count.get(), 0);

        // Dispatch increment
        dispatch.send(CounterAction::Increment);
        ctx.end_component();

        // Re-render
        ctx.begin_component(ComponentId(1));
        let reducer = Arc::new(|state: &i32, action: CounterAction| match action {
            CounterAction::Increment => state + 1,
            CounterAction::Decrement => state - 1,
            CounterAction::Add(n) => state + n,
            CounterAction::Reset => 0,
        });
        let (count, dispatch) = use_reducer(&mut ctx, 0, reducer);
        assert_eq!(count.get(), 1);

        // Dispatch add
        dispatch.send(CounterAction::Add(5));
        ctx.end_component();

        // Re-render
        ctx.begin_component(ComponentId(1));
        let reducer = Arc::new(|state: &i32, action: CounterAction| match action {
            CounterAction::Increment => state + 1,
            CounterAction::Decrement => state - 1,
            CounterAction::Add(n) => state + n,
            CounterAction::Reset => 0,
        });
        let (count, _) = use_reducer(&mut ctx, 0, reducer);
        assert_eq!(count.get(), 6);
    }

    #[test]
    fn test_reducer_with_struct_state() {
        #[derive(Debug, Clone, PartialEq)]
        struct State {
            count: i32,
            name: String,
        }

        #[derive(Debug, Clone)]
        enum Action {
            SetCount(i32),
            SetName(String),
        }

        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let initial = State {
            count: 0,
            name: "test".to_string(),
        };

        let reducer = Arc::new(|state: &State, action: Action| match action {
            Action::SetCount(n) => State {
                count: n,
                ..state.clone()
            },
            Action::SetName(name) => State {
                name,
                ..state.clone()
            },
        });

        let (state, dispatch) = use_reducer(&mut ctx, initial.clone(), reducer);
        assert_eq!(state.get().count, 0);
        assert_eq!(state.get().name, "test");

        dispatch.send(Action::SetCount(42));
        ctx.end_component();

        ctx.begin_component(ComponentId(1));
        let reducer = Arc::new(|state: &State, action: Action| match action {
            Action::SetCount(n) => State {
                count: n,
                ..state.clone()
            },
            Action::SetName(name) => State {
                name,
                ..state.clone()
            },
        });
        let (state, _) = use_reducer(&mut ctx, initial, reducer);
        assert_eq!(state.get().count, 42);
        assert_eq!(state.get().name, "test");
    }
}
