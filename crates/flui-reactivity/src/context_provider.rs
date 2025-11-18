//! Context API for dependency injection and global state.
//!
//! Inspired by React's Context API, provides a way to share values
//! between components without prop drilling.
//!
//! # Thread Safety
//!
//! **This Context API is fully thread-safe!**
//!
//! - Uses `OnceLock` for lazy global initialization
//! - Uses `RwLock` for concurrent read access
//! - All context values must be `Send + Sync + 'static`
//! - Can be safely used across threads
//!
//! Unlike React's Context which is single-threaded, this implementation
//! uses Rust's thread-safe primitives for true multi-threaded access.
//!
//! # Example
//!
//! ```rust,ignore
//! use std::thread;
//! use flui_reactivity::{provide_context, use_context};
//!
//! #[derive(Clone)]
//! struct Config {
//!     api_key: String,
//! }
//!
//! // Provide context in main thread
//! provide_context(Config {
//!     api_key: "secret".to_string(),
//! });
//!
//! // Access from another thread
//! let handle = thread::spawn(|| {
//!     let config = use_context::<Config>().unwrap();
//!     println!("API key: {}", config.api_key);
//! });
//!
//! handle.join().unwrap();
//! ```

use parking_lot::RwLock;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use tracing::{debug, trace};

/// Unique identifier for a context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContextId(TypeId);

impl ContextId {
    /// Create a context ID for a specific type.
    pub fn of<T: 'static>() -> Self {
        Self(TypeId::of::<T>())
    }
}

/// Thread-safe context value storage.
type ContextValue = Arc<dyn Any + Send + Sync>;

/// Global context store.
struct ContextStore {
    contexts: RwLock<HashMap<ContextId, ContextValue>>,
}

impl ContextStore {
    fn new() -> Self {
        Self {
            contexts: RwLock::new(HashMap::new()),
        }
    }

    fn get() -> &'static Self {
        use std::sync::OnceLock;
        static INSTANCE: OnceLock<ContextStore> = OnceLock::new();
        INSTANCE.get_or_init(Self::new)
    }

    fn provide<T: Send + Sync + 'static>(&self, value: T) {
        let context_id = ContextId::of::<T>();
        let value_arc = Arc::new(value);

        self.contexts.write().insert(context_id, value_arc);

        debug!(
            context_type = std::any::type_name::<T>(),
            "Context provided"
        );
    }

    fn consume<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        let context_id = ContextId::of::<T>();

        let contexts = self.contexts.read();
        let value = contexts.get(&context_id)?;

        let result = value.clone().downcast::<T>().ok();

        trace!(
            context_type = std::any::type_name::<T>(),
            found = result.is_some(),
            "Context consumed"
        );

        result
    }

    fn remove<T: Send + Sync + 'static>(&self) {
        let context_id = ContextId::of::<T>();
        self.contexts.write().remove(&context_id);

        debug!(context_type = std::any::type_name::<T>(), "Context removed");
    }
}

/// Context provider for dependency injection.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Clone)]
/// struct Theme {
///     primary_color: String,
///     font_size: u32,
/// }
///
/// // Provide context
/// provide_context(Theme {
///     primary_color: "blue".to_string(),
///     font_size: 16,
/// });
///
/// // Consume context
/// let theme = use_context::<Theme>().unwrap();
/// println!("Theme: {}", theme.primary_color);
/// ```
pub fn provide_context<T: Send + Sync + 'static>(value: T) {
    ContextStore::get().provide(value);
}

/// Consume a context value.
///
/// Returns None if the context hasn't been provided.
pub fn use_context<T: Send + Sync + 'static>() -> Option<Arc<T>> {
    ContextStore::get().consume()
}

/// Remove a context from the store.
pub fn remove_context<T: Send + Sync + 'static>() {
    ContextStore::get().remove::<T>();
}

/// Context provider with RAII cleanup.
///
/// Automatically removes the context when dropped.
///
/// # Example
///
/// ```rust,ignore
/// {
///     let _provider = ContextProvider::new(Theme::default());
///
///     // Theme is available here
///     let theme = use_context::<Theme>().unwrap();
/// } // Theme is removed when provider drops
/// ```
pub struct ContextProvider<T: Send + Sync + 'static> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Send + Sync + 'static> ContextProvider<T> {
    /// Create a new context provider.
    pub fn new(value: T) -> Self {
        provide_context(value);
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T: Send + Sync + 'static> Drop for ContextProvider<T> {
    fn drop(&mut self) {
        remove_context::<T>();
    }
}

impl<T: Send + Sync + 'static> fmt::Debug for ContextProvider<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContextProvider")
            .field("type", &std::any::type_name::<T>())
            .finish()
    }
}

/// Scoped context that combines multiple providers.
///
/// # Example
///
/// ```rust,ignore
/// let scope = ContextScope::new()
///     .with(Theme::default())
///     .with(AppConfig::default());
///
/// scope.run(|| {
///     let theme = use_context::<Theme>().unwrap();
///     let config = use_context::<AppConfig>().unwrap();
///     // Both contexts available here
/// });
/// // Both contexts removed after run()
/// ```
pub struct ContextScope {
    providers: Vec<Box<dyn Any>>,
}

impl ContextScope {
    /// Create a new empty scope.
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    /// Add a context to the scope.
    pub fn with<T: Send + Sync + 'static>(mut self, value: T) -> Self {
        let provider = ContextProvider::new(value);
        self.providers.push(Box::new(provider));
        self
    }

    /// Run a function with this scope active.
    pub fn run<F, R>(self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let result = f();
        // Providers dropped here, cleaning up all contexts
        drop(self.providers);
        result
    }
}

impl Default for ContextScope {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ContextScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContextScope")
            .field("provider_count", &self.providers.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct Theme {
        color: String,
    }

    #[derive(Debug, Clone, PartialEq)]
    struct Config {
        debug: bool,
    }

    #[test]
    fn test_provide_and_consume() {
        provide_context(Theme {
            color: "blue".to_string(),
        });

        let theme = use_context::<Theme>().unwrap();
        assert_eq!(theme.color, "blue");

        remove_context::<Theme>();
        assert!(use_context::<Theme>().is_none());
    }

    #[test]
    fn test_context_provider_raii() {
        {
            let _provider = ContextProvider::new(Theme {
                color: "red".to_string(),
            });

            let theme = use_context::<Theme>().unwrap();
            assert_eq!(theme.color, "red");
        }

        // Should be removed after drop
        assert!(use_context::<Theme>().is_none());
    }

    #[test]
    fn test_multiple_contexts() {
        provide_context(Theme {
            color: "green".to_string(),
        });
        provide_context(Config { debug: true });

        let theme = use_context::<Theme>().unwrap();
        let config = use_context::<Config>().unwrap();

        assert_eq!(theme.color, "green");
        assert_eq!(config.debug, true);

        remove_context::<Theme>();
        remove_context::<Config>();
    }

    #[test]
    fn test_context_scope() {
        let scope = ContextScope::new()
            .with(Theme {
                color: "purple".to_string(),
            })
            .with(Config { debug: false });

        scope.run(|| {
            let theme = use_context::<Theme>().unwrap();
            let config = use_context::<Config>().unwrap();

            assert_eq!(theme.color, "purple");
            assert_eq!(config.debug, false);
        });

        // Both should be cleaned up
        assert!(use_context::<Theme>().is_none());
        assert!(use_context::<Config>().is_none());
    }

    #[test]
    fn test_context_override() {
        provide_context(Theme {
            color: "blue".to_string(),
        });

        let theme1 = use_context::<Theme>().unwrap();
        assert_eq!(theme1.color, "blue");

        // Override with new value
        provide_context(Theme {
            color: "yellow".to_string(),
        });

        let theme2 = use_context::<Theme>().unwrap();
        assert_eq!(theme2.color, "yellow");

        remove_context::<Theme>();
    }

    #[test]
    fn test_multi_threaded_context() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;
        use std::thread;

        // Use unique type to avoid interference with other tests
        #[derive(Debug, Clone)]
        struct MultiThreadConfig {
            debug: bool,
        }

        // Provide context in main thread
        provide_context(MultiThreadConfig { debug: true });

        let counter = Arc::new(AtomicU32::new(0));

        // Spawn multiple threads that access the same context
        let handles: Vec<_> = (0..5)
            .map(|_| {
                let counter = counter.clone();
                thread::spawn(move || {
                    // Access context from different thread
                    if let Some(config) = use_context::<MultiThreadConfig>() {
                        if config.debug {
                            counter.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        // All threads should have accessed the context
        assert_eq!(counter.load(Ordering::SeqCst), 5);

        remove_context::<MultiThreadConfig>();
    }

    #[test]
    fn test_concurrent_context_reads() {
        use std::thread;
        use std::time::Duration;

        // Use unique type to avoid interference with other tests
        #[derive(Debug, Clone, PartialEq)]
        struct ConcurrentTheme {
            color: String,
        }

        provide_context(ConcurrentTheme {
            color: "concurrent".to_string(),
        });

        // Small delay to ensure context is set
        thread::sleep(Duration::from_millis(10));

        // Multiple threads reading simultaneously
        let handles: Vec<_> = (0..10)
            .map(|i| {
                thread::spawn(move || {
                    if let Some(theme) = use_context::<ConcurrentTheme>() {
                        assert_eq!(theme.color, "concurrent");
                        i
                    } else {
                        panic!("Context not found in thread {}", i);
                    }
                })
            })
            .collect();

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results.len(), 10);

        remove_context::<ConcurrentTheme>();
    }
}
