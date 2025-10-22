//! Binding base trait for application lifecycle management
//!
//! This module provides the `BindingBase` trait, which serves as the foundation for
//! all Flutter-style bindings in Flui. Bindings manage the lifecycle and initialization
//! of different subsystems (rendering, widgets, gestures, etc.).
//!
//! # Overview
//!
//! In Flutter, bindings are singleton objects that initialize and manage specific
//! subsystems. The `BindingBase` trait defines the common interface all bindings share.
//!
//! # Example
//!
//! ```rust
//! use flui_core::foundation::BindingBase;
//!
//! struct MyBinding {
//!     initialized: bool,
//! }
//!
//! impl BindingBase for MyBinding {
//!     fn init_instances(&mut self) {
//!         self.initialized = true;
//!         println!("MyBinding initialized");
//!     }
//!
//!     fn init_service_extensions(&mut self) {
//!         println!("Service extensions initialized");
//!     }
//!
//!     fn locked(&self) -> bool {
//!         false // Not locked by default
//!     }
//!
//!     fn unlock_events(&mut self) {
//!         println!("Events unlocked");
//!     }
//! }
//! ```

use std::time::Duration;

/// Unique identifier for a frame callback
///
/// Returned by `add_persistent_frame_callback` and used to cancel callbacks
/// with `cancel_frame_callback`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameCallbackId(pub u64);

/// Base trait for all binding classes
///
/// Bindings are singleton objects that manage the lifecycle of different subsystems
/// in the Flui framework. Each binding is responsible for:
///
/// - Initializing its subsystem (`init_instances`)
/// - Setting up service extensions for debugging (`init_service_extensions`)
/// - Managing event locking for initialization (`locked`, `unlock_events`)
/// - Performing periodic tasks (`perform_reass_embly` for hot reload)
///
/// # Lifecycle
///
/// The typical binding lifecycle is:
///
/// 1. `init_instances()` - Initialize the binding and its dependencies
/// 2. `init_service_extensions()` - Setup debugging/profiling hooks
/// 3. `unlock_events()` - Allow events to flow (after initialization complete)
/// 4. `perform_reassembly()` - Handle hot reload events (development only)
///
/// # Implementation Notes
///
/// - Bindings are typically singletons accessed via static methods
/// - Multiple bindings can be composed together (mixin pattern)
/// - Event locking prevents events from being processed during initialization
pub trait BindingBase {
    /// Initialize the binding instance
    ///
    /// This is called when the binding is first created. Subclasses should
    /// initialize their subsystems and dependencies here.
    ///
    /// # Default Implementation
    ///
    /// The default implementation does nothing. Override this to provide
    /// initialization logic.
    fn init_instances(&mut self) {
        // Default: no-op
    }

    /// Initialize service extensions for debugging
    ///
    /// Service extensions allow external tools (like DevTools) to inspect
    /// and control the application at runtime. Bindings can register their
    /// own service extensions here.
    ///
    /// # Default Implementation
    ///
    /// The default implementation does nothing. Override this to register
    /// service extensions.
    fn init_service_extensions(&mut self) {
        // Default: no-op
    }

    /// Whether events are currently locked
    ///
    /// When locked, the binding will not process events. This is typically used
    /// during initialization to prevent events from being handled before the
    /// system is fully ready.
    ///
    /// # Returns
    ///
    /// `true` if events are locked, `false` if they can be processed.
    ///
    /// # Default Implementation
    ///
    /// Returns `false` (unlocked).
    fn locked(&self) -> bool {
        false
    }

    /// Unlock event processing
    ///
    /// Called when initialization is complete and the binding is ready to
    /// process events. After this is called, `locked()` should return `false`.
    ///
    /// # Default Implementation
    ///
    /// The default implementation does nothing. Override this if your binding
    /// implements event locking.
    fn unlock_events(&mut self) {
        // Default: no-op
    }

    /// Perform a reassembly (hot reload)
    ///
    /// This is called during development when the application code is reloaded.
    /// Bindings should refresh their state to reflect the new code.
    ///
    /// # Returns
    ///
    /// A future that completes when reassembly is done. The default implementation
    /// returns a future that completes immediately.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::foundation::BindingBase;
    ///
    /// struct MyBinding;
    ///
    /// impl BindingBase for MyBinding {
    ///     fn perform_reassembly(&mut self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    ///         // Clear caches, rebuild state, etc.
    ///         Box::pin(async {
    ///             println!("Reassembling...");
    ///         })
    ///     }
    /// }
    /// ```
    fn perform_reassembly(
        &mut self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        Box::pin(async {})
    }

    /// Register a callback to be called when the binding is reassembled
    ///
    /// This is useful for components that need to refresh their state during hot reload.
    ///
    /// # Default Implementation
    ///
    /// The default implementation does nothing. Override this to support reassembly callbacks.
    fn register_reassemble_callback(&mut self, _callback: Box<dyn Fn() + Send + Sync>) {
        // Default: no-op
    }

    // ============================================================================
    // Frame Callbacks
    // ============================================================================

    /// Add a callback to be executed after the current frame completes
    ///
    /// Post-frame callbacks are executed after the current frame has been rendered.
    /// This is useful for operations that should happen after rendering, such as:
    ///
    /// - Navigation after build completes
    /// - Focus changes after layout
    /// - Measurements after paint
    /// - Triggering additional builds
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_core::foundation::BindingBase;
    ///
    /// // In a widget's init_state:
    /// binding.add_post_frame_callback(Box::new(|| {
    ///     // Navigate after the first frame is rendered
    ///     Navigator::push(context, LoginRoute::new());
    /// }));
    /// ```
    ///
    /// # Default Implementation
    ///
    /// The default implementation does nothing. Override this in SchedulerBinding
    /// or other bindings that manage frame scheduling.
    fn add_post_frame_callback(&mut self, _callback: Box<dyn FnOnce() + Send>) {
        // Default: no-op (override in SchedulerBinding)
    }

    /// Execute all pending post-frame callbacks
    ///
    /// Called by the framework after each frame completes. Bindings that support
    /// post-frame callbacks should override this method to execute queued callbacks.
    ///
    /// # Default Implementation
    ///
    /// The default implementation does nothing.
    fn flush_post_frame_callbacks(&mut self) {
        // Default: no-op
    }

    /// Schedule a callback to be called every frame
    ///
    /// Persistent frame callbacks are called on every frame until explicitly cancelled.
    /// The callback receives the elapsed time since the app started.
    ///
    /// This is useful for:
    /// - Animations that need to update every frame
    /// - Continuous monitoring or polling
    /// - Game loops
    ///
    /// # Returns
    ///
    /// A `FrameCallbackId` that can be used to cancel the callback later.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_core::foundation::{BindingBase, FrameCallbackId};
    /// use std::time::Duration;
    ///
    /// // In an AnimationController:
    /// let callback_id = binding.add_persistent_frame_callback(Box::new(|elapsed: Duration| {
    ///     println!("Frame at {:?}", elapsed);
    ///     // Update animation state
    /// }));
    ///
    /// // Later, when animation completes:
    /// binding.cancel_frame_callback(callback_id);
    /// ```
    ///
    /// # Default Implementation
    ///
    /// Returns a dummy ID (0). Override this in SchedulerBinding.
    fn add_persistent_frame_callback(
        &mut self,
        _callback: Box<dyn Fn(Duration) + Send>,
    ) -> FrameCallbackId {
        FrameCallbackId(0) // Default: no-op
    }

    /// Cancel a persistent frame callback
    ///
    /// Removes a callback that was previously registered with
    /// `add_persistent_frame_callback`. After this call, the callback
    /// will no longer be invoked on future frames.
    ///
    /// # Arguments
    ///
    /// * `id` - The ID returned by `add_persistent_frame_callback`
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let id = binding.add_persistent_frame_callback(Box::new(|_| {
    ///     // Animation frame
    /// }));
    ///
    /// // Stop the animation
    /// binding.cancel_frame_callback(id);
    /// ```
    ///
    /// # Default Implementation
    ///
    /// Does nothing. Override this in SchedulerBinding.
    fn cancel_frame_callback(&mut self, _id: FrameCallbackId) {
        // Default: no-op
    }

    /// Schedule a one-time callback for the next frame
    ///
    /// Similar to `add_post_frame_callback`, but called before the frame is built
    /// rather than after it completes. This is useful for scheduling rebuilds or
    /// other operations that need to happen in the next frame.
    ///
    /// The callback receives the elapsed time since the app started.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_core::foundation::BindingBase;
    ///
    /// binding.schedule_frame_callback(Box::new(|elapsed| {
    ///     println!("Next frame starting at {:?}", elapsed);
    ///     // Trigger rebuild
    /// }));
    /// ```
    ///
    /// # Default Implementation
    ///
    /// Does nothing. Override this in SchedulerBinding.
    fn schedule_frame_callback(&mut self, _callback: Box<dyn FnOnce(Duration) + Send>) {
        // Default: no-op
    }

    /// Request that a new frame be scheduled
    ///
    /// This ensures that the framework will process frame callbacks and
    /// rebuild/repaint as needed. Call this when you need to trigger a new
    /// frame, for example after changing state that affects rendering.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_core::foundation::BindingBase;
    ///
    /// // After changing animation state:
    /// self.value = new_value;
    /// binding.schedule_frame(); // Ensure we get a new frame to render the change
    /// ```
    ///
    /// # Default Implementation
    ///
    /// Does nothing. Override this in SchedulerBinding.
    fn schedule_frame(&mut self) {
        // Default: no-op (override in SchedulerBinding)
    }
}

// ============================================================================
// Example Implementation - SchedulerBinding-like
// ============================================================================

/// Example binding that manages event locking
///
/// This demonstrates how to implement the event locking pattern used by
/// Flutter's SchedulerBinding.
#[derive(Debug)]
pub struct ExampleBinding {
    locked: bool,
    initialized: bool,
}

impl ExampleBinding {
    /// Create a new example binding
    pub fn new() -> Self {
        Self {
            locked: true, // Start locked
            initialized: false,
        }
    }
}

impl Default for ExampleBinding {
    fn default() -> Self {
        Self::new()
    }
}

impl BindingBase for ExampleBinding {
    fn init_instances(&mut self) {
        self.initialized = true;
    }

    fn init_service_extensions(&mut self) {
        // In a real implementation, register debug extensions here
    }

    fn locked(&self) -> bool {
        self.locked
    }

    fn unlock_events(&mut self) {
        self.locked = false;
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_example_binding_lifecycle() {
        let mut binding = ExampleBinding::new();

        // Should start locked and uninitialized
        assert!(binding.locked());
        assert!(!binding.initialized);

        // Initialize
        binding.init_instances();
        assert!(binding.initialized);
        assert!(binding.locked()); // Still locked

        // Unlock
        binding.unlock_events();
        assert!(!binding.locked());
    }

    #[test]
    fn test_binding_service_extensions() {
        let mut binding = ExampleBinding::new();

        // Should not panic
        binding.init_service_extensions();
    }

    #[test]
    fn test_binding_reassembly() {
        let mut binding = ExampleBinding::new();

        // Should return a future that completes immediately
        let future = binding.perform_reassembly();
        // Can't easily test async in sync test, but at least verify it compiles
        drop(future);
    }

    #[test]
    fn test_binding_register_reassemble_callback() {
        let mut binding = ExampleBinding::new();

        // Default implementation should not panic
        binding.register_reassemble_callback(Box::new(|| {
            println!("Reassembled!");
        }));
    }

    // Test custom binding implementation
    struct CustomBinding {
        init_called: bool,
        service_extensions_called: bool,
        unlocked: bool,
    }

    impl CustomBinding {
        fn new() -> Self {
            Self {
                init_called: false,
                service_extensions_called: false,
                unlocked: false,
            }
        }
    }

    impl BindingBase for CustomBinding {
        fn init_instances(&mut self) {
            self.init_called = true;
        }

        fn init_service_extensions(&mut self) {
            self.service_extensions_called = true;
        }

        fn locked(&self) -> bool {
            !self.unlocked
        }

        fn unlock_events(&mut self) {
            self.unlocked = true;
        }
    }

    #[test]
    fn test_custom_binding() {
        let mut binding = CustomBinding::new();

        assert!(!binding.init_called);
        assert!(!binding.service_extensions_called);
        assert!(binding.locked());

        binding.init_instances();
        assert!(binding.init_called);

        binding.init_service_extensions();
        assert!(binding.service_extensions_called);

        binding.unlock_events();
        assert!(!binding.locked());
    }

    #[test]
    fn test_default_implementations() {
        struct MinimalBinding;

        impl BindingBase for MinimalBinding {}

        let mut binding = MinimalBinding;

        // All default implementations should not panic
        binding.init_instances();
        binding.init_service_extensions();
        assert!(!binding.locked());
        binding.unlock_events();
        binding.register_reassemble_callback(Box::new(|| {}));

        // Frame callbacks should also not panic
        binding.add_post_frame_callback(Box::new(|| {}));
        binding.flush_post_frame_callbacks();

        let id = binding.add_persistent_frame_callback(Box::new(|_| {}));
        assert_eq!(id, FrameCallbackId(0)); // Default returns 0

        binding.cancel_frame_callback(id);
        binding.schedule_frame_callback(Box::new(|_| {}));
        binding.schedule_frame();
    }

    #[test]
    fn test_frame_callback_id() {
        let id1 = FrameCallbackId(1);
        let id2 = FrameCallbackId(1);
        let id3 = FrameCallbackId(2);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);

        // Test Debug
        let debug_str = format!("{:?}", id1);
        assert!(debug_str.contains("FrameCallbackId"));

        // Test Hash (used in HashSet/HashMap)
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(id1);
        assert!(set.contains(&id2)); // Same ID
        assert!(!set.contains(&id3)); // Different ID
    }

    #[test]
    fn test_binding_with_frame_callbacks() {
        use std::sync::Arc;
        use parking_lot::Mutex;

        struct FrameBinding {
            post_frame_callbacks: Arc<Mutex<Vec<Box<dyn FnOnce() + Send>>>>,
            persistent_callbacks: Arc<Mutex<Vec<(FrameCallbackId, Box<dyn Fn(Duration) + Send>)>>>,
            next_id: Arc<Mutex<u64>>,
        }

        impl FrameBinding {
            fn new() -> Self {
                Self {
                    post_frame_callbacks: Arc::new(Mutex::new(Vec::new())),
                    persistent_callbacks: Arc::new(Mutex::new(Vec::new())),
                    next_id: Arc::new(Mutex::new(1)),
                }
            }
        }

        impl BindingBase for FrameBinding {
            fn add_post_frame_callback(&mut self, callback: Box<dyn FnOnce() + Send>) {
                self.post_frame_callbacks.lock().push(callback);
            }

            fn flush_post_frame_callbacks(&mut self) {
                let callbacks = std::mem::take(&mut *self.post_frame_callbacks.lock());
                for callback in callbacks {
                    callback();
                }
            }

            fn add_persistent_frame_callback(
                &mut self,
                callback: Box<dyn Fn(Duration) + Send>,
            ) -> FrameCallbackId {
                let id = {
                    let mut next_id = self.next_id.lock();
                    let id = FrameCallbackId(*next_id);
                    *next_id += 1;
                    id
                };
                self.persistent_callbacks.lock().push((id, callback));
                id
            }

            fn cancel_frame_callback(&mut self, id: FrameCallbackId) {
                self.persistent_callbacks.lock().retain(|(cb_id, _)| *cb_id != id);
            }
        }

        let mut binding = FrameBinding::new();

        // Test post-frame callbacks
        let called = Arc::new(Mutex::new(false));
        let called_clone = called.clone();
        binding.add_post_frame_callback(Box::new(move || {
            *called_clone.lock() = true;
        }));

        assert!(!*called.lock());
        binding.flush_post_frame_callbacks();
        assert!(*called.lock());

        // Test persistent callbacks
        let counter = Arc::new(Mutex::new(0));
        let counter_clone = counter.clone();
        let id = binding.add_persistent_frame_callback(Box::new(move |_elapsed| {
            *counter_clone.lock() += 1;
        }));

        // Simulate frame ticks
        for (_, callback) in binding.persistent_callbacks.lock().iter() {
            callback(Duration::from_secs(1));
        }
        assert_eq!(*counter.lock(), 1);

        for (_, callback) in binding.persistent_callbacks.lock().iter() {
            callback(Duration::from_secs(2));
        }
        assert_eq!(*counter.lock(), 2);

        // Cancel callback
        binding.cancel_frame_callback(id);
        assert_eq!(binding.persistent_callbacks.lock().len(), 0);
    }
}
