//! Callback type definitions
//!
//! This module provides common callback type aliases similar to Flutter's foundation library.
//! These make APIs more readable and consistent.

/// Signature of callbacks that have no arguments and return no data.
///
/// Similar to Flutter's `VoidCallback`.
///
/// # Example
///
/// ```rust,ignore
/// fn on_pressed(callback: VoidCallback) {
///     callback();
/// }
/// ```
pub type VoidCallback = Box<dyn Fn() + Send + Sync>;

/// Signature of callbacks that return a Future with no data.
///
/// Similar to Flutter's `AsyncCallback`.
pub type AsyncCallback = Box<dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>;

/// Signature for callbacks that report that an underlying value has changed.
///
/// Similar to Flutter's `ValueChanged<T>`.
///
/// # Example
///
/// ```rust,ignore
/// fn on_text_changed(callback: ValueChanged<String>) {
///     callback("new text".to_string());
/// }
/// ```
pub type ValueChanged<T> = Box<dyn Fn(T) + Send + Sync>;

/// Signature for callbacks that are to report a value on demand.
///
/// Similar to Flutter's `ValueGetter<T>`.
///
/// # Example
///
/// ```rust,ignore
/// fn get_current_value(getter: ValueGetter<i32>) -> i32 {
///     getter()
/// }
/// ```
pub type ValueGetter<T> = Box<dyn Fn() -> T + Send + Sync>;

/// Signature for callbacks that receive a value.
///
/// Similar to Flutter's `ValueSetter<T>`.
///
/// # Example
///
/// ```rust,ignore
/// fn set_value(setter: ValueSetter<i32>, value: i32) {
///     setter(value);
/// }
/// ```
pub type ValueSetter<T> = Box<dyn Fn(T) + Send + Sync>;

/// Signature for callbacks that return a Future with a value.
///
/// Similar to Flutter's `AsyncValueGetter<T>`.
pub type AsyncValueGetter<T> = Box<dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>> + Send + Sync>;

/// Signature for callbacks that receive a value and return a Future.
///
/// Similar to Flutter's `AsyncValueSetter<T>`.
pub type AsyncValueSetter<T> = Box<dyn Fn(T) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>;

/// Helper to create void callbacks from closures.
pub fn void_callback<F>(f: F) -> VoidCallback
where
    F: Fn() + Send + Sync + 'static,
{
    Box::new(f)
}

/// Helper to create value changed callbacks from closures.
pub fn value_changed<T, F>(f: F) -> ValueChanged<T>
where
    F: Fn(T) + Send + Sync + 'static,
    T: 'static,
{
    Box::new(f)
}

/// Helper to create value getter callbacks from closures.
pub fn value_getter<T, F>(f: F) -> ValueGetter<T>
where
    F: Fn() -> T + Send + Sync + 'static,
    T: 'static,
{
    Box::new(f)
}

/// Helper to create value setter callbacks from closures.
pub fn value_setter<T, F>(f: F) -> ValueSetter<T>
where
    F: Fn(T) + Send + Sync + 'static,
    T: 'static,
{
    Box::new(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_void_callback() {
        let called = Arc::new(Mutex::new(false));
        let called_clone = Arc::clone(&called);

        let callback = void_callback(move || {
            *called_clone.lock().unwrap() = true;
        });

        assert!(!*called.lock().unwrap());
        callback();
        assert!(*called.lock().unwrap());
    }

    #[test]
    fn test_value_changed() {
        let value = Arc::new(Mutex::new(0));
        let value_clone = Arc::clone(&value);

        let callback = value_changed(move |new_value| {
            *value_clone.lock().unwrap() = new_value;
        });

        assert_eq!(*value.lock().unwrap(), 0);
        callback(42);
        assert_eq!(*value.lock().unwrap(), 42);
    }

    #[test]
    fn test_value_getter() {
        let callback = value_getter(|| 42);
        assert_eq!(callback(), 42);
    }

    #[test]
    fn test_value_setter() {
        let value = Arc::new(Mutex::new(0));
        let value_clone = Arc::clone(&value);

        let callback = value_setter(move |new_value| {
            *value_clone.lock().unwrap() = new_value;
        });

        callback(100);
        assert_eq!(*value.lock().unwrap(), 100);
    }

    #[test]
    fn test_value_changed_string() {
        let text = Arc::new(Mutex::new(String::new()));
        let text_clone = Arc::clone(&text);

        let callback = value_changed(move |new_text: String| {
            *text_clone.lock().unwrap() = new_text;
        });

        callback("Hello".to_string());
        assert_eq!(*text.lock().unwrap(), "Hello");
    }
}
