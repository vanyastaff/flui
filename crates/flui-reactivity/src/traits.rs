//! Core hook trait definitions.
//!
//! Defines the base [`Hook`] trait and specialized hook traits for
//! different hook capabilities.

use std::future::Future;

/// Core trait that all hooks must implement.
///
/// A hook is a reusable piece of stateful logic that can be used in components.
/// Each hook has persistent state, takes input parameters, and produces output.
///
/// # Type Parameters
///
/// - `State` - Persistent state stored between hook calls
/// - `Input` - Parameters passed to the hook on each call
/// - `Output` - Value returned from the hook
///
/// # Example
///
/// ```rust,ignore
/// struct CounterHook;
///
/// impl Hook for CounterHook {
///     type State = i32;
///     type Input = ();
///     type Output = (i32, Box<dyn Fn()>);
///
///     fn create(_input: Self::Input) -> Self::State {
///         0
///     }
///
///     fn update(state: &mut Self::State, _input: Self::Input) -> Self::Output {
///         let count = *state;
///         let increment = Box::new(move || *state += 1);
///         (count, increment)
///     }
/// }
/// ```
pub trait Hook: 'static {
    /// Persistent state stored between hook calls.
    type State: 'static;

    /// Input parameters for the hook.
    /// Must be Clone to support hook lifecycle (create + update on first call).
    type Input: Clone + 'static;

    /// Output returned from the hook.
    type Output;

    /// Create initial state.
    ///
    /// Called once when the hook is first used.
    fn create(input: Self::Input) -> Self::State;

    /// Update hook with new input and get output.
    ///
    /// Called on every render after the first.
    fn update(state: &mut Self::State, input: Self::Input) -> Self::Output;

    /// Cleanup when component unmounts.
    ///
    /// Override this to perform cleanup (e.g., cancel subscriptions).
    /// Default implementation drops the state.
    fn cleanup(state: Self::State) {
        drop(state);
    }
}

/// Hook that tracks reactive dependencies.
///
/// Reactive hooks automatically track which signals they depend on
/// and re-run when those dependencies change.
pub trait ReactiveHook: Hook {
    /// Track dependencies accessed during execution.
    ///
    /// Returns a list of dependency IDs that this hook depends on.
    fn track_dependencies(&self) -> Vec<DependencyId>;
}

/// Hook that runs side effects.
///
/// Effect hooks run after rendering completes and can optionally
/// return a cleanup function.
pub trait EffectHook: Hook {
    /// Run side effect.
    ///
    /// Called after the component renders.
    fn run_effect(&mut self);

    /// Run cleanup before next effect or unmount.
    ///
    /// Called before the next effect runs or when the component unmounts.
    fn run_cleanup(&mut self);
}

/// Hook that manages async operations.
///
/// Async hooks start async operations and handle their results.
pub trait AsyncHook: Hook {
    /// Future type returned by async operation.
    type Future: Future<Output = Self::Output>;

    /// Start async operation.
    ///
    /// Returns a future that will be polled to completion.
    fn start_async(state: &mut Self::State, input: Self::Input) -> Self::Future;
}

/// Unique identifier for a reactive dependency.
///
/// Used to track which signals a hook depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DependencyId(pub u64);

impl DependencyId {
    /// Create a new dependency ID.
    #[inline]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get the inner ID value.
    #[inline]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl From<u64> for DependencyId {
    #[inline]
    fn from(id: u64) -> Self {
        Self(id)
    }
}

impl From<DependencyId> for u64 {
    #[inline]
    fn from(id: DependencyId) -> Self {
        id.0
    }
}

impl std::fmt::Display for DependencyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Dependency({})", self.0)
    }
}
