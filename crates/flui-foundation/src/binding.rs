//! Binding infrastructure for singleton services.
//!
//! This module provides the `BindingBase` trait which is the base for all
//! binding mixins that provide singleton services.
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's `BindingBase` class:
//!
//! ```dart
//! abstract class BindingBase {
//!   void initInstances() {
//!     // Initialize the binding singleton
//!   }
//!
//!   static T checkInstance<T extends BindingBase>(T? instance) {
//!     assert(instance != null, 'Binding not initialized');
//!     return instance!;
//!   }
//! }
//! ```
//!
//! # Implementing a Binding
//!
//! In Rust, we use traits instead of mixins. Each binding:
//!
//! 1. Implements `BindingBase`
//! 2. Has a static `OnceLock<Self>` for singleton storage
//! 3. Provides `instance()` and `ensure_initialized()` methods
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_foundation::BindingBase;
//! use std::sync::OnceLock;
//!
//! pub struct MyBinding {
//!     // binding fields
//! }
//!
//! impl BindingBase for MyBinding {
//!     fn init_instances(&mut self) {
//!         // Initialize singleton services
//!     }
//! }
//!
//! impl MyBinding {
//!     fn new() -> Self {
//!         let mut binding = Self { /* ... */ };
//!         binding.init_instances();
//!         binding
//!     }
//!
//!     pub fn instance() -> &'static Self {
//!         static INSTANCE: OnceLock<MyBinding> = OnceLock::new();
//!         INSTANCE.get_or_init(|| MyBinding::new())
//!     }
//!
//!     pub fn ensure_initialized() -> &'static Self {
//!         Self::instance()
//!     }
//! }
//! ```
//!
//! # Combined Bindings
//!
//! Flutter's `WidgetsFlutterBinding` combines multiple bindings using mixins.
//! In Rust, we use composition:
//!
//! ```rust,ignore
//! pub struct WidgetsFlutterBinding {
//!     // Each binding is a singleton, accessed via instance()
//! }
//!
//! impl WidgetsFlutterBinding {
//!     // Delegates to individual binding singletons
//!     pub fn gestures() -> &'static GestureBinding {
//!         GestureBinding::instance()
//!     }
//!
//!     pub fn widgets() -> &'static WidgetsBinding {
//!         WidgetsBinding::instance()
//!     }
//! }
//! ```

use std::sync::atomic::{AtomicBool, Ordering};

/// Base trait for binding mixins that provide singleton services.
///
/// The Flutter engine exposes some low-level services, but these are
/// typically not suitable for direct use. Bindings provide the glue
/// between these low-level APIs and the higher-level framework APIs.
///
/// # Contract
///
/// Implementations must:
/// 1. Call `init_instances()` exactly once during construction
/// 2. Store the singleton in a static `OnceLock`
/// 3. Provide `instance()` returning `&'static Self`
///
/// # Thread Safety
///
/// Bindings are guaranteed to be initialized only once, even in
/// multi-threaded environments, due to `OnceLock` semantics.
pub trait BindingBase: Sized + Send + Sync + 'static {
    /// Initialize the binding's instances.
    ///
    /// This is called exactly once when the binding is first created.
    /// Implementations should initialize all singleton services here.
    ///
    /// # Important
    ///
    /// If this binding depends on other bindings, ensure they are
    /// initialized first by calling their `ensure_initialized()`.
    fn init_instances(&mut self);

    /// Check if this binding type has been initialized.
    ///
    /// Returns `true` if `instance()` has been called at least once.
    fn is_initialized() -> bool
    where
        Self: HasInstance,
    {
        Self::INITIALIZED.load(Ordering::Acquire)
    }
}

/// Marker trait for bindings that have a singleton instance.
///
/// This trait provides the static storage for tracking initialization.
/// Each binding should implement this trait with its own static.
pub trait HasInstance: BindingBase {
    /// Static flag tracking initialization state.
    const INITIALIZED: &'static AtomicBool;

    /// Get the singleton instance.
    ///
    /// Creates the instance on first call, returns cached instance thereafter.
    fn instance() -> &'static Self;

    /// Ensure the binding is initialized and return the instance.
    ///
    /// This is the preferred way to access a binding when you're not sure
    /// if it has been initialized yet.
    fn ensure_initialized() -> &'static Self {
        Self::instance()
    }
}

/// Helper macro to implement singleton pattern for a binding.
///
/// # Example
///
/// ```rust,ignore
/// use flui_foundation::{BindingBase, impl_binding_singleton};
///
/// pub struct MyBinding { /* ... */ }
///
/// impl BindingBase for MyBinding {
///     fn init_instances(&mut self) {
///         // initialization
///     }
/// }
///
/// impl MyBinding {
///     fn new() -> Self {
///         let mut binding = Self { /* ... */ };
///         binding.init_instances();
///         binding
///     }
/// }
///
/// impl_binding_singleton!(MyBinding);
/// ```
#[macro_export]
macro_rules! impl_binding_singleton {
    ($binding:ty) => {
        impl $crate::HasInstance for $binding {
            const INITIALIZED: &'static std::sync::atomic::AtomicBool = {
                static INIT: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                &INIT
            };

            fn instance() -> &'static Self {
                static INSTANCE: std::sync::OnceLock<$binding> = std::sync::OnceLock::new();
                INSTANCE.get_or_init(|| {
                    Self::INITIALIZED.store(true, std::sync::atomic::Ordering::Release);
                    <$binding>::new()
                })
            }
        }
    };
}

/// Check that a binding instance exists.
///
/// This is the Rust equivalent of Flutter's `BindingBase.checkInstance()`.
///
/// # Panics
///
/// Panics if the binding has not been initialized.
///
/// # Example
///
/// ```rust,ignore
/// let binding = check_instance::<GestureBinding>();
/// ```
pub fn check_instance<B: HasInstance>() -> &'static B {
    if !B::is_initialized() {
        panic!(
            "Binding {} has not been initialized. \
             Call {0}::ensure_initialized() first.",
            std::any::type_name::<B>()
        );
    }
    B::instance()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestBinding {
        initialized: bool,
    }

    impl BindingBase for TestBinding {
        fn init_instances(&mut self) {
            self.initialized = true;
        }
    }

    impl TestBinding {
        fn new() -> Self {
            let mut binding = Self { initialized: false };
            binding.init_instances();
            binding
        }
    }

    impl_binding_singleton!(TestBinding);

    #[test]
    fn test_binding_singleton() {
        let binding1 = TestBinding::instance();
        let binding2 = TestBinding::instance();

        // Should be the same instance
        assert!(std::ptr::eq(binding1, binding2));

        // Should be initialized
        assert!(binding1.initialized);
        assert!(TestBinding::is_initialized());
    }

    #[test]
    fn test_ensure_initialized() {
        let binding = TestBinding::ensure_initialized();
        assert!(binding.initialized);
    }

    #[test]
    fn test_check_instance() {
        // Ensure initialized first
        let _ = TestBinding::instance();

        // Now check should work
        let binding = check_instance::<TestBinding>();
        assert!(binding.initialized);
    }
}
