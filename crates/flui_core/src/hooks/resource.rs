//! Resource hook implementation for async operations.
//!
//! Provides `use_resource` hook for managing async data fetching.
//! Note: Requires an async runtime to be configured.

use super::hook_trait::Hook;
use super::signal::{Signal, SignalHook};
use crate::BuildContext;
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;

/// Resource state tracking loading, data, and errors.
///
/// # Example
///
/// ```rust,ignore
/// let user_resource = use_resource(move || async {
///     fetch_user(user_id).await
/// });
///
/// match (user_resource.loading.get(), user_resource.data.get()) {
///     (true, _) => Text::new("Loading..."),
///     (false, Some(user)) => Text::new(user.name),
///     (false, None) => Text::new("Error loading user"),
/// }
/// ```
#[derive(Debug)]
pub struct Resource<T, E = String> {
    /// Whether the resource is currently loading.
    pub loading: Signal<bool>,

    /// The loaded data, if available.
    pub data: Signal<Option<T>>,

    /// Error that occurred during loading, if any.
    pub error: Signal<Option<E>>,
}

impl<T: Clone + Send + 'static, E: Clone + Send + 'static> Resource<T, E> {
    /// Create a new resource with pre-created signals.
    fn new(loading: Signal<bool>, data: Signal<Option<T>>, error: Signal<Option<E>>) -> Self {
        Self {
            loading,
            data,
            error,
        }
    }

    /// Check if the resource is loading.
    ///
    /// Note: This does not track the signal as a dependency.
    /// Use `loading.get(ctx)` if you need dependency tracking.
    pub fn is_loading(&self) -> bool {
        self.loading.get_untracked()
    }

    /// Check if the resource has data.
    ///
    /// Note: This does not track the signal as a dependency.
    /// Use `data.get(ctx)` if you need dependency tracking.
    pub fn has_data(&self) -> bool {
        self.data.get_untracked().is_some()
    }

    /// Check if the resource has an error.
    ///
    /// Note: This does not track the signal as a dependency.
    /// Use `error.get(ctx)` if you need dependency tracking.
    pub fn has_error(&self) -> bool {
        self.error.get_untracked().is_some()
    }

    /// Get the data if available.
    ///
    /// Note: This does not track the signal as a dependency.
    /// Use `data.get(ctx)` if you need dependency tracking.
    pub fn get_data(&self) -> Option<T> {
        self.data.get_untracked()
    }

    /// Get the error if available.
    ///
    /// Note: This does not track the signal as a dependency.
    /// Use `error.get(ctx)` if you need dependency tracking.
    pub fn get_error(&self) -> Option<E> {
        self.error.get_untracked()
    }

    /// Refetch the resource.
    ///
    /// Note: Refetch mechanism requires async runtime integration
    pub fn refetch(&self) {
        self.loading.set(true);
        self.error.set(None);
        // Future: Trigger actual async refetch when runtime is integrated
    }
}

impl<T: Clone + 'static, E: Clone + 'static> Clone for Resource<T, E> {
    fn clone(&self) -> Self {
        Self {
            loading: self.loading.clone(),
            data: self.data.clone(),
            error: self.error.clone(),
        }
    }
}

/// Hook state for ResourceHook.
pub struct ResourceState<T, E> {
    loading: Signal<bool>,
    data: Signal<Option<T>>,
    error: Signal<Option<E>>,
}

impl<T, E> std::fmt::Debug for ResourceState<T, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourceState")
            .field("loading", &"<Signal>")
            .field("data", &"<Signal>")
            .field("error", &"<Signal>")
            .finish()
    }
}

/// Resource hook implementation.
///
/// This hook manages async data fetching with loading and error states.
///
/// Note: Actual async fetching requires async runtime integration (tokio/async-std)
#[derive(Debug)]
pub struct ResourceHook<T, E, F, Fut>(PhantomData<(T, E, F, Fut)>);

impl<T, E, F, Fut> Hook for ResourceHook<T, E, F, Fut>
where
    T: Clone + Send + 'static,
    E: Clone + Send + 'static,
    F: Fn() -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<T, E>> + Send + 'static,
{
    type State = ResourceState<T, E>;
    type Input = (Arc<F>, Signal<bool>, Signal<Option<T>>, Signal<Option<E>>);
    type Output = Resource<T, E>;

    fn create(input: (Arc<F>, Signal<bool>, Signal<Option<T>>, Signal<Option<E>>)) -> Self::State {
        let (_fetcher, loading, data, error) = input;

        // Note: Async fetch startup requires async runtime (tokio/async-std) integration
        // Future: Spawn async task here when runtime is available

        ResourceState {
            loading,
            data,
            error,
        }
    }

    fn update(
        state: &mut Self::State,
        _input: (Arc<F>, Signal<bool>, Signal<Option<T>>, Signal<Option<E>>),
    ) -> Self::Output {
        Resource::new(
            state.loading.clone(),
            state.data.clone(),
            state.error.clone(),
        )
    }
}

// AsyncHook is intentionally not implemented for ResourceHook
// because it would create a type mismatch (Future<Output = Result<T, E>>
// vs Hook::Output = Resource<T, E>).
// Full async support will be added when integrating with an async runtime.

/// Create a resource for async data fetching.
///
/// The resource automatically manages loading, data, and error states.
///
/// # Note
///
/// This is a placeholder implementation. Full async support requires
/// integrating an async runtime.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::hooks::use_resource;
///
/// struct UserProfile {
///     user_id: String,
/// }
///
/// impl Component for UserProfile {
///     fn build(&self, ctx: &BuildContext) -> View {
///         let user_id = self.user_id.clone();
///
///         let user = use_resource(ctx, move || async move {
///             fetch_user(&user_id).await
///         });
///
///         if user.is_loading() {
///             return Text::new("Loading...").into();
///         }
///
///         if let Some(error) = user.get_error() {
///             return Text::new(format!("Error: {}", error)).into();
///         }
///
///         if let Some(data) = user.get_data() {
///             return Text::new(data.name).into();
///         }
///
///         Text::new("No data").into()
///     }
/// }
/// ```
pub fn use_resource<T, E, F, Fut>(ctx: &BuildContext, fetcher: F) -> Resource<T, E>
where
    T: Clone + Send + 'static,
    E: Clone + Send + 'static,
    F: Fn() -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<T, E>> + Send + 'static,
{
    // Create signals for the resource
    let loading = ctx.with_hook_context_mut(|hook_ctx| hook_ctx.use_hook::<SignalHook<bool>>(true));
    let data =
        ctx.with_hook_context_mut(|hook_ctx| hook_ctx.use_hook::<SignalHook<Option<T>>>(None));
    let error =
        ctx.with_hook_context_mut(|hook_ctx| hook_ctx.use_hook::<SignalHook<Option<E>>>(None));

    // Use the resource hook with the signals
    ctx.with_hook_context_mut(|hook_ctx| {
        hook_ctx.use_hook::<ResourceHook<T, E, F, Fut>>((Arc::new(fetcher), loading, data, error))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::hook_context::{ComponentId, HookContext};

    #[test]
    fn test_resource_initial_state() {
        use crate::hooks::signal::SignalHook;
        use std::sync::Arc;

        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        // Create signals manually
        let loading = ctx.use_hook::<SignalHook<bool>>(true);
        let data = ctx.use_hook::<SignalHook<Option<i32>>>(None);
        let error = ctx.use_hook::<SignalHook<Option<String>>>(None);

        let fetcher = || async { Ok(42) };
        let resource = ctx.use_hook::<ResourceHook<i32, String, _, _>>((
            Arc::new(fetcher),
            loading,
            data,
            error,
        ));

        // Initially loading with no data
        assert!(resource.is_loading());
        assert!(!resource.has_data());
        assert!(!resource.has_error());
    }

    #[test]
    fn test_resource_clone() {
        use crate::hooks::signal::SignalHook;
        use std::sync::Arc;

        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        // Create signals manually
        let loading = ctx.use_hook::<SignalHook<bool>>(true);
        let data = ctx.use_hook::<SignalHook<Option<i32>>>(None);
        let error = ctx.use_hook::<SignalHook<Option<String>>>(None);

        let fetcher = || async { Ok(42) };
        let resource1 = ctx.use_hook::<ResourceHook<i32, String, _, _>>((
            Arc::new(fetcher),
            loading,
            data,
            error,
        ));

        let resource2 = resource1.clone();

        // Both should share the same signals
        resource1.loading.set(false);
        assert!(!resource2.is_loading());
    }
}
