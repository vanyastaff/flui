//! Resource hook for async data fetching.
//!
//! The `use_resource` hook manages async data fetching with loading/error states.
//! It automatically tracks the state of the async operation and updates when dependencies change.

use crate::context::HookContext;
use crate::signal::Signal;
use crate::traits::{DependencyId, Hook};
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

/// Resource state representing the current state of an async operation.
#[derive(Debug, Clone, PartialEq)]
pub enum ResourceState<T, E> {
    /// Initial state before the fetch starts
    Idle,
    /// Currently loading
    Loading,
    /// Successfully loaded with data
    Ready(T),
    /// Failed with error
    Error(E),
}

impl<T, E> ResourceState<T, E> {
    /// Returns true if the resource is loading.
    pub fn is_loading(&self) -> bool {
        matches!(self, ResourceState::Loading)
    }

    /// Returns true if the resource is ready.
    pub fn is_ready(&self) -> bool {
        matches!(self, ResourceState::Ready(_))
    }

    /// Returns true if the resource has an error.
    pub fn is_error(&self) -> bool {
        matches!(self, ResourceState::Error(_))
    }

    /// Returns true if the resource is idle.
    pub fn is_idle(&self) -> bool {
        matches!(self, ResourceState::Idle)
    }

    /// Get the data if ready, otherwise None.
    pub fn data(&self) -> Option<&T> {
        match self {
            ResourceState::Ready(data) => Some(data),
            _ => None,
        }
    }

    /// Get the error if failed, otherwise None.
    pub fn error(&self) -> Option<&E> {
        match self {
            ResourceState::Error(err) => Some(err),
            _ => None,
        }
    }

    /// Map the data value if ready.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> ResourceState<U, E> {
        match self {
            ResourceState::Idle => ResourceState::Idle,
            ResourceState::Loading => ResourceState::Loading,
            ResourceState::Ready(data) => ResourceState::Ready(f(data)),
            ResourceState::Error(err) => ResourceState::Error(err),
        }
    }

    /// Map the error value if failed.
    pub fn map_err<F>(self, f: impl FnOnce(E) -> F) -> ResourceState<T, F> {
        match self {
            ResourceState::Idle => ResourceState::Idle,
            ResourceState::Loading => ResourceState::Loading,
            ResourceState::Ready(data) => ResourceState::Ready(data),
            ResourceState::Error(err) => ResourceState::Error(f(err)),
        }
    }
}

impl<T: Clone, E: Clone> Default for ResourceState<T, E> {
    fn default() -> Self {
        ResourceState::Idle
    }
}

/// Future type for resource fetching.
pub type ResourceFuture<T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send>>;

/// Fetcher function type.
pub type Fetcher<T, E> = Arc<dyn Fn() -> ResourceFuture<T, E> + Send + Sync>;

/// Resource handle for managing async data.
#[derive(Clone)]
pub struct Resource<T, E> {
    state_signal: Signal<ResourceState<T, E>>,
    fetcher: Fetcher<T, E>,
}

impl<T, E> Resource<T, E>
where
    T: Clone + Send + 'static,
    E: Clone + Send + 'static,
{
    /// Get the current state of the resource.
    pub fn state(&self) -> ResourceState<T, E> {
        self.state_signal.get()
    }

    /// Refetch the resource.
    ///
    /// This sets the state to Loading and starts a new fetch.
    pub fn refetch(&self) {
        self.state_signal.set(ResourceState::Loading);

        let state_signal = self.state_signal.clone();
        let fetcher = Arc::clone(&self.fetcher);

        // Spawn the async task
        #[cfg(feature = "async")]
        {
            use any_spawner::Executor;
            Executor::spawn(async move {
                let result = (fetcher)().await;
                match result {
                    Ok(data) => state_signal.set(ResourceState::Ready(data)),
                    Err(err) => state_signal.set(ResourceState::Error(err)),
                }
            });
        }

        #[cfg(not(feature = "async"))]
        {
            // Without async feature, we can't spawn tasks
            // This is a compile-time error with a helpful message
            compile_error!(
                "use_resource requires the 'async' feature. Enable it in Cargo.toml:\n\
                 flui-reactivity = { version = \"0.1\", features = [\"async\"] }"
            );
        }
    }

    /// Check if the resource is loading.
    pub fn is_loading(&self) -> bool {
        self.state().is_loading()
    }

    /// Check if the resource is ready.
    pub fn is_ready(&self) -> bool {
        self.state().is_ready()
    }

    /// Check if the resource has an error.
    pub fn is_error(&self) -> bool {
        self.state().is_error()
    }

    /// Get the data if ready.
    pub fn data(&self) -> Option<T> {
        self.state().data().cloned()
    }

    /// Get the error if failed.
    pub fn error(&self) -> Option<E> {
        self.state().error().cloned()
    }
}

impl<T, E> std::fmt::Debug for Resource<T, E>
where
    T: std::fmt::Debug + Clone + Send + 'static,
    E: std::fmt::Debug + Clone + Send + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resource")
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

/// Hook state for ResourceHook.
pub struct ResourceState_Internal<T, E> {
    resource: Resource<T, E>,
    dependencies: Vec<DependencyId>,
}

impl<T, E> std::fmt::Debug for ResourceState_Internal<T, E>
where
    T: std::fmt::Debug + Clone + Send + 'static,
    E: std::fmt::Debug + Clone + Send + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourceState")
            .field("resource", &self.resource)
            .field("dependencies", &self.dependencies)
            .finish()
    }
}

/// Resource hook implementation.
pub struct ResourceHook<T, E>(PhantomData<(T, E)>);

impl<T, E> Hook for ResourceHook<T, E>
where
    T: Clone + Send + 'static,
    E: Clone + Send + 'static,
{
    type State = ResourceState_Internal<T, E>;
    type Input = (Fetcher<T, E>, Vec<DependencyId>);
    type Output = Resource<T, E>;

    fn create(input: Self::Input) -> Self::State {
        let (fetcher, dependencies) = input;

        let state_signal = Signal::new(ResourceState::Idle);

        let resource = Resource {
            state_signal,
            fetcher,
        };

        // Start initial fetch
        resource.refetch();

        ResourceState_Internal {
            resource,
            dependencies,
        }
    }

    fn update(state: &mut Self::State, input: Self::Input) -> Self::Output {
        let (fetcher, new_deps) = input;

        // Check if dependencies changed
        let deps_changed = state.dependencies != new_deps;

        if deps_changed {
            // Update fetcher and dependencies
            state.resource.fetcher = fetcher;
            state.dependencies = new_deps;

            // Refetch with new dependencies
            state.resource.refetch();
        }

        state.resource.clone()
    }

    fn cleanup(_state: Self::State) {
        // Signal cleanup is automatic
    }
}

/// Create a resource for async data fetching.
///
/// The fetcher function is called immediately and whenever dependencies change.
/// Returns a `Resource` handle that tracks the loading/ready/error state.
///
/// **Note:** Requires the `async` feature to be enabled.
///
/// # Example
///
/// ```rust,ignore
/// use flui_reactivity::{use_resource, ResourceState};
///
/// let user_id_dep = DependencyId::new(user_id.id().0);
/// let user_resource = use_resource(ctx, vec![user_id_dep], || {
///     let id = user_id.get();
///     Box::pin(async move {
///         fetch_user(id).await
///     })
/// });
///
/// match user_resource.state() {
///     ResourceState::Loading => {
///         Text::new("Loading...")
///     }
///     ResourceState::Ready(user) => {
///         Text::new(format!("Hello, {}!", user.name))
///     }
///     ResourceState::Error(err) => {
///         Text::new(format!("Error: {}", err))
///     }
///     ResourceState::Idle => {
///         Text::new("Idle")
///     }
/// }
/// ```
///
/// # Refetching
///
/// ```rust,ignore
/// Button::new("Refresh")
///     .on_tap(move || {
///         user_resource.refetch();
///     })
/// ```
#[cfg(feature = "async")]
pub fn use_resource<T, E, F>(
    ctx: &mut HookContext,
    dependencies: Vec<DependencyId>,
    fetcher: F,
) -> Resource<T, E>
where
    T: Clone + Send + 'static,
    E: Clone + Send + 'static,
    F: Fn() -> ResourceFuture<T, E> + Send + Sync + 'static,
{
    ctx.use_hook::<ResourceHook<T, E>>((Arc::new(fetcher), dependencies))
}

#[cfg(test)]
#[cfg(feature = "async")]
mod tests {
    use super::*;
    use crate::context::ComponentId;

    #[tokio::test]
    async fn test_resource_basic() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let resource = use_resource(&mut ctx, vec![], || {
            Box::pin(async { Ok::<i32, String>(42) })
        });

        // Initial state should be Loading
        assert!(resource.is_loading());

        // Wait a bit for the fetch to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Should now be Ready
        assert!(resource.is_ready());
        assert_eq!(resource.data(), Some(42));
    }

    #[tokio::test]
    async fn test_resource_error() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let resource = use_resource(&mut ctx, vec![], || {
            Box::pin(async { Err::<i32, String>("Error!".to_string()) })
        });

        // Initial state should be Loading
        assert!(resource.is_loading());

        // Wait for fetch to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Should now have Error
        assert!(resource.is_error());
        assert_eq!(resource.error(), Some("Error!".to_string()));
    }

    #[tokio::test]
    async fn test_resource_refetch() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        let resource = use_resource(&mut ctx, vec![], move || {
            let count = counter_clone.fetch_add(1, Ordering::Relaxed);
            Box::pin(async move { Ok::<usize, String>(count) })
        });

        // Wait for initial fetch
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        assert_eq!(resource.data(), Some(0));

        // Refetch
        resource.refetch();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        assert_eq!(resource.data(), Some(1));

        // Refetch again
        resource.refetch();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        assert_eq!(resource.data(), Some(2));
    }

    #[test]
    fn test_resource_state_methods() {
        let idle: ResourceState<i32, String> = ResourceState::Idle;
        assert!(idle.is_idle());
        assert!(!idle.is_loading());
        assert!(!idle.is_ready());
        assert!(!idle.is_error());

        let loading: ResourceState<i32, String> = ResourceState::Loading;
        assert!(loading.is_loading());
        assert!(!loading.is_idle());

        let ready: ResourceState<i32, String> = ResourceState::Ready(42);
        assert!(ready.is_ready());
        assert_eq!(ready.data(), Some(&42));

        let error: ResourceState<i32, String> = ResourceState::Error("Error".to_string());
        assert!(error.is_error());
        assert_eq!(error.error(), Some(&"Error".to_string()));
    }

    #[test]
    fn test_resource_state_map() {
        let ready: ResourceState<i32, String> = ResourceState::Ready(42);
        let doubled = ready.map(|x| x * 2);
        assert_eq!(doubled.data(), Some(&84));

        let error: ResourceState<i32, String> = ResourceState::Error("Error".to_string());
        let mapped_err = error.map_err(|e| e.len());
        assert_eq!(mapped_err.error(), Some(&5));
    }
}
