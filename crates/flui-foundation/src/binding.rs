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
                static INSTANCE: std::sync::OnceLock<$binding> = std::sync::OnceLock::new();
                let inst = INSTANCE.get_or_init(<$binding>::new);
                // INITIALIZED is flipped AFTER `<Self>::new()` returns
                // (audit I-3 fix). Pre-fix, the store fired *inside* the
                // `get_or_init` closure before `new()` returned, so an
                // init-time panic would leave `INITIALIZED == true` while
                // the `OnceLock` stayed empty (per `OnceLock::get_or_init`
                // contract: "If this function panics, the cell is
                // unchanged"). A subsequent `is_initialized() → true` →
                // `instance()` caller would then either re-panic or, on
                // contention, observe incoherent state.
                //
                // The flip happens only on the successful path *after*
                // `OnceLock` has accepted the value. The steady state is a
                // single atomic CAS (false → true) that is a no-op on
                // every call past the first (F4): on an already-initialized
                // instance the `false` expectation fails and the CAS
                // returns `Err` without performing the `Release` store, so
                // the per-call cost on the hot path is one load + a failed
                // CAS rather than an unconditional `Release` write to a
                // shared cache line. The `let _ =` discards the result —
                // both the first-call `Ok(false)` and steady-state
                // `Err(true)` are expected and benign.
                let _ = Self::INITIALIZED.compare_exchange(
                    false,
                    true,
                    std::sync::atomic::Ordering::Release,
                    std::sync::atomic::Ordering::Relaxed,
                );
                inst
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
