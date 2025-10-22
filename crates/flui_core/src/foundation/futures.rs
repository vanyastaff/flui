//! Future utilities for Flui
//!
//! This module provides Future implementations and utilities used throughout Flui.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

#[cfg(test)]
use futures::executor::block_on;

/// A Future that completes synchronously with a value
///
/// `SynchronousFuture` is a Future that is already completed when created.
/// It immediately returns `Poll::Ready` with its value on the first poll.
///
/// This is useful for APIs that return futures but sometimes have the result
/// immediately available without needing to await anything.
///
/// # Examples
///
/// ```rust
/// use flui_core::foundation::SynchronousFuture;
/// use futures::executor::block_on;
///
/// let future = SynchronousFuture::new(42);
/// let result = block_on(future);
/// assert_eq!(result, 42);
/// ```
///
/// # Use Cases
///
/// - Implementing async APIs that can sometimes complete immediately
/// - Returning cached values from async functions
/// - Testing async code without actual async operations
/// - Adapting sync code to async interfaces
///
/// # Performance
///
/// `SynchronousFuture` has zero overhead compared to returning a value directly.
/// The compiler can often optimize away the Future machinery entirely when it
/// detects the future completes immediately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SynchronousFuture<T> {
    value: Option<T>,
}

impl<T> SynchronousFuture<T> {
    /// Create a new `SynchronousFuture` with the given value
    ///
    /// The future will complete immediately with this value when polled.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::foundation::SynchronousFuture;
    /// use futures::executor::block_on;
    ///
    /// let future = SynchronousFuture::new(42);
    /// assert_eq!(block_on(future), 42);
    /// ```
    pub fn new(value: T) -> Self {
        Self { value: Some(value) }
    }

    /// Create a future that's immediately ready with a value
    ///
    /// This is an alias for `new()` that matches the naming convention
    /// of `std::future::ready()`. Prefer this method for better API consistency.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::foundation::SynchronousFuture;
    /// use futures::executor::block_on;
    ///
    /// let future = SynchronousFuture::ready(42);
    /// assert_eq!(block_on(future), 42);
    /// ```
    #[inline]
    pub const fn ready(value: T) -> Self {
        Self { value: Some(value) }
    }

    /// Get the value without awaiting
    ///
    /// This consumes the future and returns its value directly.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::foundation::SynchronousFuture;
    ///
    /// let future = SynchronousFuture::new(42);
    /// assert_eq!(future.into_inner(), 42);
    /// ```
    pub fn into_inner(mut self) -> T {
        self.value.take().expect("SynchronousFuture polled after completion")
    }

    /// Check if the future still has a value
    ///
    /// Returns `false` if the future has already been polled to completion.
    /// Since `SynchronousFuture` can only be polled once, this effectively
    /// tells you whether the future has been consumed.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::foundation::SynchronousFuture;
    ///
    /// let future = SynchronousFuture::new(42);
    /// assert!(future.is_ready());
    ///
    /// let value = future.into_inner();
    /// assert_eq!(value, 42);
    /// // Note: future is consumed after into_inner()
    /// ```
    pub fn is_ready(&self) -> bool {
        self.value.is_some()
    }

    /// Map the value inside the future
    ///
    /// Transforms the value using the provided function, returning a new
    /// `SynchronousFuture` with the transformed value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::foundation::SynchronousFuture;
    /// use futures::executor::block_on;
    ///
    /// let future = SynchronousFuture::new(21).map(|x| x * 2);
    /// assert_eq!(block_on(future), 42);
    /// ```
    ///
    /// ```rust
    /// use flui_core::foundation::SynchronousFuture;
    /// use futures::executor::block_on;
    ///
    /// let future = SynchronousFuture::new("hello").map(|s| s.to_uppercase());
    /// assert_eq!(block_on(future), "HELLO");
    /// ```
    pub fn map<U, F>(self, f: F) -> SynchronousFuture<U>
    where
        F: FnOnce(T) -> U,
    {
        SynchronousFuture::new(f(self.into_inner()))
    }
}

// Safe because SynchronousFuture doesn't implement Drop or have any special pinning requirements
impl<T> Unpin for SynchronousFuture<T> {}

impl<T> Future for SynchronousFuture<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(
            self.get_mut()
                .value
                .take()
                .expect("SynchronousFuture polled after completion"),
        )
    }
}

impl<T> From<T> for SynchronousFuture<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_synchronous_future_new() {
        let future = SynchronousFuture::new(42);
        assert_eq!(future.value, Some(42));
    }

    #[test]
    fn test_synchronous_future_into_inner() {
        let future = SynchronousFuture::new(42);
        assert_eq!(future.into_inner(), 42);
    }

    #[test]
    fn test_synchronous_future_from() {
        let future: SynchronousFuture<i32> = 42.into();
        assert_eq!(future.into_inner(), 42);
    }

    #[test]
    fn test_synchronous_future_await() {
        let future = SynchronousFuture::new(42);
        let result = block_on(future);
        assert_eq!(result, 42);
    }

    #[test]
    fn test_synchronous_future_string() {
        let future = SynchronousFuture::new("hello".to_string());
        let result = block_on(future);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_synchronous_future_option() {
        let future = SynchronousFuture::new(Some(42));
        let result = block_on(future);
        assert_eq!(result, Some(42));
    }

    #[test]
    fn test_synchronous_future_clone() {
        let future1 = SynchronousFuture::new(42);
        let future2 = future1.clone();

        assert_eq!(future1.into_inner(), 42);
        assert_eq!(future2.into_inner(), 42);
    }

    #[test]
    fn test_synchronous_future_debug() {
        let future = SynchronousFuture::new(42);
        let debug_str = format!("{:?}", future);
        assert!(debug_str.contains("SynchronousFuture"));
    }

    #[test]
    fn test_synchronous_future_eq() {
        let future1 = SynchronousFuture::new(42);
        let future2 = SynchronousFuture::new(42);
        let future3 = SynchronousFuture::new(43);

        assert_eq!(future1, future2);
        assert_ne!(future1, future3);
    }

    #[test]
    fn test_synchronous_future_chain() {
        async fn get_value() -> i32 {
            SynchronousFuture::new(21).await
        }

        async fn double_value(x: i32) -> i32 {
            SynchronousFuture::new(x * 2).await
        }

        let result = block_on(async {
            double_value(get_value().await).await
        });
        assert_eq!(result, 42);
    }

    #[test]
    fn test_synchronous_future_poll_direct() {
        use std::task::{Context, RawWaker, RawWakerVTable, Waker};

        // Create a no-op waker
        unsafe fn clone_waker(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        unsafe fn wake(_: *const ()) {}
        unsafe fn wake_by_ref(_: *const ()) {}
        unsafe fn drop_waker(_: *const ()) {}

        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone_waker, wake, wake_by_ref, drop_waker);

        let raw_waker = RawWaker::new(std::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw_waker) };
        let mut context = Context::from_waker(&waker);

        let mut future = SynchronousFuture::new(42);
        let pinned = Pin::new(&mut future);

        let poll_result = pinned.poll(&mut context);
        assert_eq!(poll_result, Poll::Ready(42));
    }

    #[test]
    fn test_synchronous_future_unit() {
        let future = SynchronousFuture::new(());
        block_on(future);
        // Just ensure it compiles and runs
    }

    #[test]
    fn test_synchronous_future_complex_type() {
        #[derive(Debug, Clone, PartialEq)]
        struct ComplexData {
            id: usize,
            name: String,
            values: Vec<i32>,
        }

        let data = ComplexData {
            id: 1,
            name: "test".to_string(),
            values: vec![1, 2, 3],
        };

        let future = SynchronousFuture::new(data.clone());
        let result = block_on(future);
        assert_eq!(result, data);
    }

    #[test]
    fn test_synchronous_future_ready() {
        let future = SynchronousFuture::ready(42);
        assert_eq!(block_on(future), 42);
    }

    #[test]
    fn test_synchronous_future_ready_const() {
        // Test that ready() is const
        const FUTURE: SynchronousFuture<i32> = SynchronousFuture::ready(42);
        assert_eq!(block_on(FUTURE), 42);
    }

    #[test]
    fn test_synchronous_future_is_ready() {
        let mut future = SynchronousFuture::new(42);
        assert!(future.is_ready());

        // After polling, value is taken
        use std::task::{Context, RawWaker, RawWakerVTable, Waker};
        unsafe fn clone_waker(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        unsafe fn wake(_: *const ()) {}
        unsafe fn wake_by_ref(_: *const ()) {}
        unsafe fn drop_waker(_: *const ()) {}
        static VTABLE: RawWakerVTable =
            RawWakerVTable::new(clone_waker, wake, wake_by_ref, drop_waker);

        let raw_waker = RawWaker::new(std::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw_waker) };
        let mut context = Context::from_waker(&waker);

        let pinned = Pin::new(&mut future);
        let _ = pinned.poll(&mut context);

        assert!(!future.is_ready());
    }

    #[test]
    fn test_synchronous_future_map() {
        let future = SynchronousFuture::new(21).map(|x| x * 2);
        assert_eq!(block_on(future), 42);
    }

    #[test]
    fn test_synchronous_future_map_string() {
        let future = SynchronousFuture::new("hello").map(|s| s.to_uppercase());
        assert_eq!(block_on(future), "HELLO");
    }

    #[test]
    fn test_synchronous_future_map_chain() {
        let future = SynchronousFuture::new(10)
            .map(|x| x * 2) // 20
            .map(|x| x + 2) // 22
            .map(|x| x * 2); // 44

        assert_eq!(block_on(future), 44);
    }

    #[test]
    fn test_synchronous_future_map_type_change() {
        let future = SynchronousFuture::new(42).map(|x| format!("Value: {}", x));
        assert_eq!(block_on(future), "Value: 42");
    }

    #[test]
    fn test_synchronous_future_ready_with_map() {
        let future = SynchronousFuture::ready(5).map(|x| x * x);
        assert_eq!(block_on(future), 25);
    }
}
