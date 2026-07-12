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
/// 2. Store the singleton in owner-thread-local storage
/// 3. Provide `instance()` returning `&'static Self` for that owner thread
///
/// # Owner Runtime
///
/// ADR-0027 makes bindings owner-runtime objects. They may hold owner-local
/// callback registries and therefore are not required to be `Send + Sync`.
pub trait BindingBase: Sized + 'static {
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
    /// Creates the current owner thread's instance on first call and returns
    /// that cached owner-local instance thereafter.
    fn instance() -> &'static Self;

    /// Ensure the binding is initialized and return the instance.
    ///
    /// This is the preferred way to access a binding when you're not sure
    /// if it has been initialized yet.
    #[must_use]
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
                std::thread_local! {
                    static INSTANCE: std::cell::OnceCell<&'static $binding> =
                        const { std::cell::OnceCell::new() };
                }

                INSTANCE.with(|instance| {
                    let inst = instance.get_or_init(|| {
                        let leaked: &'static mut $binding = Box::leak(Box::new(<$binding>::new()));
                        leaked as &'static $binding
                    });
                    // INITIALIZED is flipped AFTER `<Self>::new()` returns.
                    // If construction panics, `OnceCell` remains empty and
                    // this store is not reached. The flag intentionally tracks
                    // process-wide "initialized at least once"; the singleton
                    // value itself is owner-thread-local under ADR-0027.
                    let _ = Self::INITIALIZED.compare_exchange(
                        false,
                        true,
                        std::sync::atomic::Ordering::Release,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    *inst
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
#[must_use]
pub fn check_instance<B: HasInstance>() -> &'static B {
    assert!(
        B::is_initialized(),
        "Binding {} has not been initialized. \
         Call {0}::ensure_initialized() first.",
        std::any::type_name::<B>()
    );
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

    // Audit I-3 regression: a panic inside `<Self>::new()` must NOT leave
    // `INITIALIZED == true`. Tests a separate panicking binding type so
    // the existing TestBinding singleton stays clean.
    struct PanicBinding {
        _never: (),
    }

    impl BindingBase for PanicBinding {
        fn init_instances(&mut self) {}
    }

    impl PanicBinding {
        fn new() -> Self {
            panic!("PanicBinding::new panics by design — regression test");
        }
    }

    impl_binding_singleton!(PanicBinding);

    // F17 — document `OnceLock::get_or_init` retry-after-panic semantics.
    //
    // The contract on `OnceLock::get_or_init` is: "If this function panics,
    // the cell is unchanged." That means a panic inside the init closure does
    // NOT poison the cell (unlike `std::sync::Once::call_once`, and unlike a
    // poisoned `Mutex`). A subsequent `get_or_init` therefore re-runs the
    // closure and can succeed. This test pins that behaviour so a future std
    // change (or a regression to a poisoning primitive) is caught here.
    #[test]
    fn instance_retries_after_panic() {
        use std::sync::OnceLock;
        use std::sync::atomic::AtomicU32;

        static CALL_COUNT: AtomicU32 = AtomicU32::new(0);

        struct RetryBinding;
        impl BindingBase for RetryBinding {
            fn init_instances(&mut self) {
                let count = CALL_COUNT.fetch_add(1, Ordering::Relaxed);
                // First init (count == 0) must fail; later inits succeed.
                assert_ne!(count, 0, "simulated first-init failure");
            }
        }

        static INSTANCE: OnceLock<RetryBinding> = OnceLock::new();

        // First call panics inside the closure; the cell stays empty.
        let result = std::panic::catch_unwind(|| {
            INSTANCE.get_or_init(|| {
                let mut b = RetryBinding;
                b.init_instances();
                b
            })
        });
        assert!(result.is_err(), "first init must panic");
        assert!(
            INSTANCE.get().is_none(),
            "OnceLock cell must stay empty after the init closure panics"
        );

        // Second call re-runs the closure and now succeeds (no poison state).
        let instance = INSTANCE.get_or_init(|| {
            let mut b = RetryBinding;
            b.init_instances();
            b
        });
        assert!(
            std::ptr::eq(instance, INSTANCE.get().unwrap()),
            "retried get_or_init must populate and return the stored value"
        );
    }

    // F17 triangulation — `instance()` is idempotent: two calls return the
    // very same `&'static` (pointer equality), confirming the singleton is
    // created exactly once.
    #[test]
    fn binding_instance_idempotent() {
        let a = TestBinding::instance();
        let b = TestBinding::instance();
        assert!(
            std::ptr::eq(a, b),
            "instance() must return the same singleton"
        );
    }

    #[test]
    fn init_panic_does_not_flip_initialized_flag() {
        // Sanity: not yet initialized.
        assert!(!PanicBinding::is_initialized());

        // Force `instance()` into the panicking init path. We catch the
        // panic so the rest of the test can run.
        let result = std::panic::catch_unwind(|| {
            let _ = PanicBinding::instance();
        });
        assert!(result.is_err(), "PanicBinding::new must propagate panic");

        // Post-condition: `INITIALIZED` MUST still be `false`. Pre-I-3
        // fix this assertion failed — the closure flipped `INITIALIZED`
        // to `true` BEFORE `new()` was called.
        assert!(
            !PanicBinding::is_initialized(),
            "init panic incorrectly flipped INITIALIZED to true \
             (regression of audit I-3)"
        );
    }
}
