//! Resource hook implementation for async operations.
//!
//! Provides `use_resource` hook for managing async data fetching.
//! Note: Requires an async runtime to be configured.

use super::hook_trait::{Hook};
use super::hook_context::with_hook_context;
use super::signal::{Signal, SignalHook};
use std::future::Future;
use std::marker::PhantomData;
use std::rc::Rc;

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
pub struct Resource<T, E = String> {
    /// Whether the resource is currently loading.
    pub loading: Signal<bool>,

    /// The loaded data, if available.
    pub data: Signal<Option<T>>,

    /// Error that occurred during loading, if any.
    pub error: Signal<Option<E>>,
}

impl<T: Clone + 'static, E: Clone + 'static> Resource<T, E> {
    /// Create a new resource in loading state.
    fn new() -> Self {
        Self {
            loading: with_hook_context(|ctx| {
                ctx.use_hook::<SignalHook<bool>>(true)
            }),
            data: with_hook_context(|ctx| {
                ctx.use_hook::<SignalHook<Option<T>>>(None)
            }),
            error: with_hook_context(|ctx| {
                ctx.use_hook::<SignalHook<Option<E>>>(None)
            }),
        }
    }

    /// Check if the resource is loading.
    pub fn is_loading(&self) -> bool {
        self.loading.get()
    }

    /// Check if the resource has data.
    pub fn has_data(&self) -> bool {
        self.data.get().is_some()
    }

    /// Check if the resource has an error.
    pub fn has_error(&self) -> bool {
        self.error.get().is_some()
    }

    /// Get the data if available.
    pub fn get_data(&self) -> Option<T> {
        self.data.get()
    }

    /// Get the error if available.
    pub fn get_error(&self) -> Option<E> {
        self.error.get()
    }

    /// Refetch the resource.
    ///
    /// TODO(2025-03): Implement refetch mechanism.
    pub fn refetch(&self) {
        self.loading.set(true);
        self.error.set(None);
        // Trigger refetch
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
    resource: Resource<T, E>,
}

/// Resource hook implementation.
///
/// This hook manages async data fetching with loading and error states.
///
/// TODO(2025-03): Implement actual async fetching.
/// Current implementation provides the structure but requires async runtime integration.
pub struct ResourceHook<T, E, F, Fut>(PhantomData<(T, E, F, Fut)>);

impl<T, E, F, Fut> Hook for ResourceHook<T, E, F, Fut>
where
    T: Clone + 'static,
    E: Clone + 'static,
    F: Fn() -> Fut + Clone + 'static,
    Fut: Future<Output = Result<T, E>> + 'static,
{
    type State = ResourceState<T, E>;
    type Input = Rc<F>;
    type Output = Resource<T, E>;

    fn create(_fetcher: Rc<F>) -> Self::State {
        let resource = Resource::new();

        // TODO(2025-03): Start async fetch
        // This requires integration with an async runtime (tokio, async-std, etc.)
        // For now, just create the resource structure

        ResourceState { resource }
    }

    fn update(state: &mut Self::State, _fetcher: Rc<F>) -> Self::Output {
        state.resource.clone()
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
///     fn build(&self, ctx: &mut BuildContext) -> Widget {
///         let user_id = self.user_id.clone();
///
///         let user = use_resource(move || async move {
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
pub fn use_resource<T, E, F, Fut>(fetcher: F) -> Resource<T, E>
where
    T: Clone + 'static,
    E: Clone + 'static,
    F: Fn() -> Fut + Clone + 'static,
    Fut: Future<Output = Result<T, E>> + 'static,
{
    with_hook_context(|ctx| {
        ctx.use_hook::<ResourceHook<T, E, F, Fut>>(Rc::new(fetcher))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::hook_context::{ComponentId, HookContext};

    #[test]
    fn test_resource_initial_state() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let resource = ctx.use_hook::<ResourceHook<i32, String, _, _>>(|| async {
            Ok(42)
        });

        // Initially loading with no data
        assert!(resource.is_loading());
        assert!(!resource.has_data());
        assert!(!resource.has_error());
    }

    #[test]
    fn test_resource_clone() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let resource1 = ctx.use_hook::<ResourceHook<i32, String, _, _>>(|| async {
            Ok(42)
        });

        let resource2 = resource1.clone();

        // Both should share the same signals
        resource1.loading.set(false);
        assert!(!resource2.is_loading());
    }
}
