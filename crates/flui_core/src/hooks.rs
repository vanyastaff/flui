//! Hook adapters integrating flui-reactivity with UI tree.
//!
//! This module provides wrapper functions that connect the standalone
//! `flui-reactivity` crate with FLUI's element tree and rebuild system.
//!
//! # Architecture
//!
//! ```text
//! flui-reactivity (pure reactivity)
//!        ↑
//! flui_core/hooks.rs (adapter layer)
//!        ↑
//! View::build() (user code)
//! ```
//!
//! The adapter layer:
//! - Re-exports core types from `flui-reactivity`
//! - Provides `use_*` functions that integrate with `BuildContext`
//! - Subscribes to signals to trigger UI rebuilds
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_core::hooks::{use_signal, use_memo, use_effect};
//!
//! impl View for Counter {
//!     fn build(&self, ctx: &BuildContext) -> impl IntoElement {
//!         let count = use_signal(ctx, 0);
//!
//!         use_effect(ctx, move || {
//!             println!("Count: {}", count.get());
//!             None
//!         });
//!
//!         // ... build UI
//!     }
//! }
//! ```

use crate::view::BuildContext;

// =============================================================================
// Re-exports from flui-reactivity
// =============================================================================

// Core types
pub use flui_reactivity::{
    // Batching
    batch,
    create_root,
    is_batching,
    with_owner,
    BatchGuard,
    // Computed
    Computed,
    ComputedId,
    // Scheduler
    EffectId,
    EffectPriority,
    EffectScheduler,
    // Owner
    Owner,
    OwnerId,
    RuntimeConfig,
    // Signals
    Signal,
    SignalId,
    // Runtime
    SignalRuntime,
};

// Hook infrastructure
pub use flui_reactivity::{
    AsyncHook, ComponentId, DependencyId, EffectHookTrait, Hook, HookContext, HookId, HookIndex,
    ReactiveHook,
};

// Hook types (for advanced usage)
pub use flui_reactivity::{Callback, CleanupFn, Dispatch, EffectFn, Reducer, Ref};

// Errors
pub use flui_reactivity::{
    HookError, ReactivityError, Result as ReactivityResult, ResultExt, RuntimeError, SignalError,
};

// Context API (global, not scoped)
pub use flui_reactivity::{
    provide_context, remove_context, use_context, ContextId, ContextProvider, ContextScope,
};

// Resource (async)
#[cfg(feature = "async")]
pub use flui_reactivity::{Resource, ResourceState};

// =============================================================================
// Adapter functions - integrate reactivity with UI tree
// =============================================================================

/// Create a reactive signal integrated with UI rebuild system.
///
/// When the signal value changes, the current element is automatically
/// scheduled for rebuild.
///
/// # Example
///
/// ```rust,ignore
/// let count = use_signal(ctx, 0);
/// count.set(42); // Triggers rebuild
/// count.update(|n| n + 1); // Triggers rebuild
/// ```
///
/// # Thread Safety
///
/// Signal values must be `Send`. The signal itself is `Copy` (8 bytes).
pub fn use_signal<T>(ctx: &BuildContext, initial: T) -> Signal<T>
where
    T: Clone + Send + 'static,
{
    let signal = Signal::new(initial);

    // Subscribe to trigger rebuilds when signal changes
    let element_id = ctx.element_id();
    let rebuild_queue = ctx.rebuild_queue().clone();

    // Ignore subscription errors (e.g., too many subscribers)
    let _ = signal.subscribe(move || {
        tracing::info!(element_id = ?element_id, "Signal subscription triggered - pushing to rebuild queue");
        rebuild_queue.push(element_id, 0);
    });

    signal
}

/// Create a memoized computation integrated with UI rebuild system.
///
/// The computation is cached and only re-runs when dependencies change.
/// When the result changes, the element is scheduled for rebuild.
///
/// # Example
///
/// ```rust,ignore
/// let count = use_signal(ctx, 0);
/// let doubled = use_memo(ctx, move || count.get() * 2);
/// ```
pub fn use_memo<T, F>(ctx: &BuildContext, f: F) -> Computed<T>
where
    T: Clone + Send + 'static,
    F: FnMut() -> T + Send + 'static,
{
    let computed = Computed::new(f);

    // Subscribe to trigger rebuilds when computed value changes
    let element_id = ctx.element_id();
    let rebuild_queue = ctx.rebuild_queue().clone();

    let _ = computed.subscribe(move || {
        rebuild_queue.push(element_id, 0);
    });

    computed
}

/// Run a side effect after rendering.
///
/// The effect function can return an optional cleanup function that runs:
/// - Before the effect re-runs (when dependencies change)
/// - When the component unmounts
///
/// # Example
///
/// ```rust,ignore
/// // Run once on mount
/// use_effect(ctx, || {
///     println!("Mounted!");
///     Some(Box::new(|| println!("Unmounted!")))
/// });
///
/// // Run when signal changes
/// let count = use_signal(ctx, 0);
/// use_effect(ctx, move || {
///     println!("Count: {}", count.get());
///     None
/// });
/// ```
pub fn use_effect<F>(ctx: &BuildContext, effect: F)
where
    F: Fn() -> Option<flui_reactivity::CleanupFn> + Send + Sync + 'static,
{
    ctx.with_hook_context_mut(|hook_ctx| {
        flui_reactivity::use_effect(hook_ctx, vec![], effect);
    });
}

/// Run a side effect with explicit dependencies.
///
/// The effect only re-runs when one of the dependencies changes.
///
/// # Example
///
/// ```rust,ignore
/// let count = use_signal(ctx, 0);
/// let dep = DependencyId::new(count.id().0);
///
/// use_effect_with_deps(ctx, vec![dep], move || {
///     println!("Count changed: {}", count.get());
///     None
/// });
/// ```
pub fn use_effect_with_deps<F>(ctx: &BuildContext, deps: Vec<DependencyId>, effect: F)
where
    F: Fn() -> Option<flui_reactivity::CleanupFn> + Send + Sync + 'static,
{
    ctx.with_hook_context_mut(|hook_ctx| {
        flui_reactivity::use_effect(hook_ctx, deps, effect);
    });
}

/// Create a memoized callback.
///
/// The callback is cached and only recreated when dependencies change.
/// Useful for passing callbacks to child components without triggering
/// unnecessary rebuilds.
///
/// # Example
///
/// ```rust,ignore
/// let on_click = use_callback(ctx, vec![], || {
///     println!("Clicked!");
/// });
/// ```
pub fn use_callback<F>(
    ctx: &BuildContext,
    deps: Vec<DependencyId>,
    callback: F,
) -> flui_reactivity::Callback<F>
where
    F: Clone + Send + Sync + 'static,
{
    ctx.with_hook_context_mut(|hook_ctx| flui_reactivity::use_callback(hook_ctx, deps, callback))
}

/// Create a mutable reference that persists across renders.
///
/// Unlike signals, changing a ref does NOT trigger a rebuild.
/// Useful for storing values that shouldn't cause re-renders.
///
/// # Example
///
/// ```rust,ignore
/// let render_count = use_ref(ctx, 0);
/// render_count.set(render_count.get() + 1);
/// ```
pub fn use_ref<T>(ctx: &BuildContext, initial: T) -> flui_reactivity::Ref<T>
where
    T: Clone + Send + 'static,
{
    ctx.with_hook_context_mut(|hook_ctx| flui_reactivity::use_ref(hook_ctx, initial))
}

/// Redux-style state management with reducer.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Clone)]
/// enum Action { Increment, Decrement }
///
/// let reducer = Arc::new(|state: &i32, action: Action| match action {
///     Action::Increment => state + 1,
///     Action::Decrement => state - 1,
/// });
///
/// let (count, dispatch) = use_reducer(ctx, 0, reducer);
/// dispatch.send(Action::Increment);
/// ```
#[allow(clippy::type_complexity)]
pub fn use_reducer<S, A>(
    ctx: &BuildContext,
    initial: S,
    reducer: std::sync::Arc<dyn Fn(&S, A) -> S + Send + Sync>,
) -> (Signal<S>, flui_reactivity::Dispatch<A>)
where
    S: Clone + Send + 'static,
    A: Clone + Send + 'static,
{
    ctx.with_hook_context_mut(|hook_ctx| flui_reactivity::use_reducer(hook_ctx, initial, reducer))
}

/// Load async data with automatic state management.
///
/// Returns a `Resource` that tracks loading/ready/error states.
///
/// # Example
///
/// ```rust,ignore
/// let user = use_resource(ctx, vec![], || {
///     Box::pin(async { fetch_user().await })
/// });
///
/// match user.state() {
///     ResourceState::Loading => Text::new("Loading..."),
///     ResourceState::Ready(user) => Text::new(&user.name),
///     ResourceState::Error(e) => Text::new(&format!("Error: {}", e)),
///     ResourceState::Idle => Text::new(""),
/// }
/// ```
#[cfg(feature = "async")]
pub fn use_resource<T, F, Fut>(
    ctx: &BuildContext,
    deps: Vec<DependencyId>,
    fetcher: F,
) -> flui_reactivity::Resource<T>
where
    T: Clone + Send + 'static,
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = T> + Send + 'static,
{
    ctx.with_hook_context_mut(|hook_ctx| {
        flui_reactivity::use_resource(hook_ctx, deps, move || Box::pin(fetcher()))
    })
}

// =============================================================================
// Test utilities
// =============================================================================

/// Test harness for hooks (re-export).
pub use flui_reactivity::test_harness::HookTestHarness;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::ElementTree;
    use crate::ElementId;
    use parking_lot::RwLock;
    use std::sync::Arc;

    fn create_test_context() -> BuildContext {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let element_id = ElementId::new(1);
        BuildContext::new(tree, element_id)
    }

    #[test]
    fn test_use_signal_basic() {
        let ctx = create_test_context();
        let signal = use_signal(&ctx, 42);
        assert_eq!(signal.get(), 42);

        signal.set(100);
        assert_eq!(signal.get(), 100);
    }

    #[test]
    fn test_use_memo_basic() {
        let ctx = create_test_context();
        let signal = use_signal(&ctx, 5);
        let doubled = use_memo(&ctx, move || signal.get() * 2);

        assert_eq!(doubled.get(), 10);

        signal.set(10);
        assert_eq!(doubled.get(), 20);
    }

    #[test]
    fn test_signal_is_copy() {
        let ctx = create_test_context();
        let signal = use_signal(&ctx, 0);

        // Signal is Copy - no clone needed
        let signal2 = signal;
        signal.set(42);
        assert_eq!(signal2.get(), 42);
    }

    #[test]
    fn test_batch_updates() {
        let ctx = create_test_context();
        let signal = use_signal(&ctx, 0);

        batch(|| {
            signal.set(1);
            signal.set(2);
            signal.set(3);
        });

        assert_eq!(signal.get(), 3);
    }
}
