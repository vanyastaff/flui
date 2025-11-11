//! WidgetsFlutterBinding - Combined Flutter-style binding
//!
//! This is the main binding that combines all framework bindings:
//! - GestureBinding (events)
//! - SchedulerBinding (frame callbacks)
//! - RendererBinding (rendering pipeline)
//! - WidgetsBinding (widget tree)
//!
//! It provides a global singleton accessed via `ensure_initialized()`.

use super::{BindingBase, GestureBinding, RendererBinding, SchedulerBinding, WidgetsBinding};
use std::sync::{Arc, OnceLock};

/// Combined Flutter-style binding
///
/// # Architecture
///
/// ```text
/// WidgetsFlutterBinding (singleton)
///   ├─ GestureBinding (EventRouter)
///   ├─ SchedulerBinding (frame callbacks)
///   ├─ RendererBinding (PipelineOwner)
///   └─ WidgetsBinding (ElementTree)
/// ```
///
/// # Usage
///
/// ```rust,ignore
/// let binding = WidgetsFlutterBinding::ensure_initialized();
/// binding.widgets.attach_root_widget(MyApp::new());
/// ```
///
/// # Thread-Safety
///
/// The binding is thread-safe and can be accessed from any thread.
/// It uses OnceLock for lazy initialization with thread-safe guarantees.
pub struct WidgetsFlutterBinding {
    /// Gesture binding (event routing)
    pub gesture: GestureBinding,

    /// Scheduler binding (frame callbacks)
    pub scheduler: SchedulerBinding,

    /// Renderer binding (rendering pipeline)
    pub renderer: RendererBinding,

    /// Widgets binding (widget tree)
    pub widgets: WidgetsBinding,
}

impl WidgetsFlutterBinding {
    /// Ensure binding is initialized (idempotent)
    ///
    /// Returns the global singleton binding, initializing it if necessary.
    /// Subsequent calls return the same instance.
    ///
    /// # Thread-Safety
    ///
    /// This method is thread-safe. If multiple threads call it concurrently,
    /// only one thread will initialize the binding.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // First call initializes
    /// let binding1 = WidgetsFlutterBinding::ensure_initialized();
    ///
    /// // Second call returns same instance
    /// let binding2 = WidgetsFlutterBinding::ensure_initialized();
    ///
    /// assert!(Arc::ptr_eq(&binding1, &binding2));
    /// ```
    pub fn ensure_initialized() -> Arc<Self> {
        static INSTANCE: OnceLock<Arc<WidgetsFlutterBinding>> = OnceLock::new();

        INSTANCE
            .get_or_init(|| {
                tracing::info!("Initializing WidgetsFlutterBinding");

                let mut binding = Self {
                    gesture: GestureBinding::new(),
                    scheduler: SchedulerBinding::new(),
                    renderer: RendererBinding::new(),
                    widgets: WidgetsBinding::new(),
                };

                // Initialize all bindings
                binding.gesture.init();
                binding.scheduler.init();
                binding.renderer.init();
                binding.widgets.init();

                // Wire up frame callbacks
                binding.wire_up();

                tracing::info!("WidgetsFlutterBinding initialized");
                Arc::new(binding)
            })
            .clone()
    }

    /// Wire up bindings
    ///
    /// Connects the scheduler to widgets for automatic rebuilds.
    /// This is called once during initialization.
    fn wire_up(&self) {
        // Connect scheduler → widgets (build phase)
        // Every frame, rebuild dirty widgets
        // TODO: Implement proper build frame handling

        tracing::debug!("Bindings wired up");
    }

    /// Get instance if already initialized
    ///
    /// Returns None if `ensure_initialized()` has not been called yet.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(binding) = WidgetsFlutterBinding::instance() {
    ///     // Use binding...
    /// }
    /// ```
    pub fn instance() -> Option<Arc<Self>> {
        static INSTANCE: OnceLock<Arc<WidgetsFlutterBinding>> = OnceLock::new();
        INSTANCE.get().cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ensure_initialized() {
        let binding = WidgetsFlutterBinding::ensure_initialized();

        // Should have all components initialized
        assert_eq!(binding.scheduler.persistent_callback_count(), 0); // No callbacks yet (wire_up is stub)
    }

    #[test]
    fn test_singleton() {
        let binding1 = WidgetsFlutterBinding::ensure_initialized();
        let binding2 = WidgetsFlutterBinding::ensure_initialized();

        // Should be same instance (pointer equality)
        assert!(Arc::ptr_eq(&binding1, &binding2));
    }

    #[test]
    fn test_instance_before_init() {
        // This test is tricky because we can't reset the singleton
        // Just verify it returns Some after initialization
        let _ = WidgetsFlutterBinding::ensure_initialized();
        assert!(WidgetsFlutterBinding::instance().is_some());
    }
}
