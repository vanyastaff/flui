//! Common callback type aliases
//!
//! This module provides type aliases for commonly used callback patterns
//! in the FLUI framework, similar to Flutter's `basic_types.dart`.
//!
//! # Thread Safety
//!
//! All callback types are `Send + Sync` to support multi-threaded UI
//! frameworks. This is a key difference from Flutter's Dart-based callbacks.
//!
//! # Examples
//!
//! ```rust
//! use std::sync::{
//!     Arc,
//!     atomic::{AtomicI32, Ordering},
//! };
//!
//! use flui_foundation::{ValueChanged, ValueGetter, VoidCallback};
//!
//! let pressed = Arc::new(AtomicI32::new(0));
//! let pressed2 = Arc::clone(&pressed);
//! // Simple callback with no arguments
//! let on_pressed: VoidCallback = Arc::new(move || {
//!     pressed2.fetch_add(1, Ordering::Relaxed);
//! });
//!
//! let last = Arc::new(AtomicI32::new(0));
//! let last2 = Arc::clone(&last);
//! // Callback that receives a value
//! let on_changed: ValueChanged<i32> = Arc::new(move |value| {
//!     last2.store(value, Ordering::Relaxed);
//! });
//!
//! // Getter function
//! let get_count: ValueGetter<i32> = Arc::new(|| 42);
//!
//! on_pressed();
//! on_changed(7);
//! assert_eq!(pressed.load(Ordering::Relaxed), 1);
//! assert_eq!(last.load(Ordering::Relaxed), 7);
//! assert_eq!(get_count(), 42);
//! ```

use std::sync::Arc;

/// A callback with no arguments and no return value.
///
/// This is the most common callback type, used for event handlers
/// like button presses, tap events, etc.
///
/// # Examples
///
/// ```rust
/// use std::sync::{
///     Arc,
///     atomic::{AtomicBool, Ordering},
/// };
///
/// use flui_foundation::VoidCallback;
///
/// let invoked = Arc::new(AtomicBool::new(false));
/// let invoked2 = Arc::clone(&invoked);
/// let callback: VoidCallback = Arc::new(move || {
///     invoked2.store(true, Ordering::Relaxed);
/// });
/// callback();
/// assert!(invoked.load(Ordering::Relaxed));
/// ```
pub type VoidCallback = Arc<dyn Fn() + Send + Sync>;

/// A callback that receives a value of type `T`.
///
/// Used for change notifications where the new value is passed
/// to the callback.
///
/// # Examples
///
/// ```rust
/// use std::sync::{Arc, Mutex};
///
/// use flui_foundation::ValueChanged;
///
/// let seen = Arc::new(Mutex::new(String::new()));
/// let seen2 = Arc::clone(&seen);
/// let on_changed: ValueChanged<String> = Arc::new(move |value| {
///     *seen2.lock().unwrap() = value;
/// });
/// on_changed("Hello".to_string());
/// assert_eq!(*seen.lock().unwrap(), "Hello");
/// ```
pub type ValueChanged<T> = Arc<dyn Fn(T) + Send + Sync>;

/// A function that returns a value of type `T`.
///
/// Used for lazy evaluation or deferred value computation.
///
/// # Examples
///
/// ```rust
/// use std::sync::Arc;
///
/// use flui_foundation::ValueGetter;
///
/// let get_value: ValueGetter<i32> = Arc::new(|| 42);
/// assert_eq!(get_value(), 42);
/// ```
pub type ValueGetter<T> = Arc<dyn Fn() -> T + Send + Sync>;

/// A function that accepts a value of type `T`.
///
/// Used for setting values or consuming data.
///
/// # Examples
///
/// ```rust
/// use std::sync::{
///     Arc,
///     atomic::{AtomicI32, Ordering},
/// };
///
/// use flui_foundation::ValueSetter;
///
/// let counter = Arc::new(AtomicI32::new(0));
/// let counter_clone = counter.clone();
///
/// let set_value: ValueSetter<i32> = Arc::new(move |value| {
///     counter_clone.store(value, Ordering::SeqCst);
/// });
///
/// set_value(42);
/// assert_eq!(counter.load(Ordering::SeqCst), 42);
/// ```
pub type ValueSetter<T> = Arc<dyn Fn(T) + Send + Sync>;

/// A predicate function that tests a condition.
///
/// Returns `true` if the condition is satisfied.
///
/// # Examples
///
/// ```rust
/// use std::sync::Arc;
///
/// use flui_foundation::Predicate;
///
/// let is_positive: Predicate<i32> = Arc::new(|value| value > 0);
/// assert!(is_positive(5));
/// assert!(!is_positive(-1));
/// ```
pub type Predicate<T> = Arc<dyn Fn(T) -> bool + Send + Sync>;

/// A function that transforms a value from type `T` to type `R`.
///
/// # Examples
///
/// ```rust
/// use std::sync::Arc;
///
/// use flui_foundation::ValueTransformer;
///
/// let to_string: ValueTransformer<i32, String> = Arc::new(|value| format!("Value: {}", value));
/// assert_eq!(to_string(42), "Value: 42");
/// ```
pub type ValueTransformer<T, R> = Arc<dyn Fn(T) -> R + Send + Sync>;

/// A callback that can return a result indicating success or failure.
///
/// # Examples
///
/// ```rust
/// use std::sync::Arc;
///
/// use flui_foundation::FallibleCallback;
///
/// let validate: FallibleCallback<String> = Arc::new(|input| {
///     if input.is_empty() {
///         Err("Input cannot be empty".to_string())
///     } else {
///         Ok(())
///     }
/// });
///
/// assert!(validate("hello".to_string()).is_ok());
/// assert!(validate("".to_string()).is_err());
/// ```
pub type FallibleCallback<T> = Arc<dyn Fn(T) -> Result<(), String> + Send + Sync>;

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

    use super::*;

    #[test]
    fn test_void_callback() {
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        let callback: VoidCallback = Arc::new(move || {
            called_clone.store(true, Ordering::SeqCst);
        });

        assert!(!called.load(Ordering::SeqCst));
        callback();
        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_value_changed() {
        let value = Arc::new(AtomicI32::new(0));
        let value_clone = value.clone();

        let on_changed: ValueChanged<i32> = Arc::new(move |new_value| {
            value_clone.store(new_value, Ordering::SeqCst);
        });

        on_changed(42);
        assert_eq!(value.load(Ordering::SeqCst), 42);
    }

    #[test]
    fn test_value_getter() {
        let getter: ValueGetter<i32> = Arc::new(|| 42);
        assert_eq!(getter(), 42);
    }

    #[test]
    fn test_value_setter() {
        let value = Arc::new(AtomicI32::new(0));
        let value_clone = value.clone();

        let setter: ValueSetter<i32> = Arc::new(move |new_value| {
            value_clone.store(new_value, Ordering::SeqCst);
        });

        setter(100);
        assert_eq!(value.load(Ordering::SeqCst), 100);
    }

    #[test]
    fn test_predicate() {
        let is_even: Predicate<i32> = Arc::new(|value| value % 2 == 0);

        assert!(is_even(2));
        assert!(is_even(4));
        assert!(!is_even(3));
        assert!(!is_even(5));
    }

    #[test]
    fn test_value_transformer() {
        let double: ValueTransformer<i32, i32> = Arc::new(|value| value * 2);

        assert_eq!(double(5), 10);
        assert_eq!(double(21), 42);
    }

    #[test]
    fn test_fallible_callback() {
        let validate: FallibleCallback<i32> = Arc::new(|value| {
            if value > 0 {
                Ok(())
            } else {
                Err("Value must be positive".to_string())
            }
        });

        assert!(validate(5).is_ok());
        assert!(validate(-1).is_err());
    }

    #[test]
    fn test_callbacks_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<VoidCallback>();
        assert_send_sync::<ValueChanged<i32>>();
        assert_send_sync::<ValueGetter<i32>>();
        assert_send_sync::<ValueSetter<i32>>();
        assert_send_sync::<Predicate<i32>>();
        assert_send_sync::<ValueTransformer<i32, i32>>();
        assert_send_sync::<FallibleCallback<i32>>();
    }
}
