//! Ref hook for mutable references without triggering re-renders.
//!
//! The `use_ref` hook provides a way to store mutable values that persist across renders
//! without causing re-renders when the value changes. This is similar to React's useRef.

use crate::context::HookContext;
use crate::traits::Hook;
use parking_lot::Mutex;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

/// A mutable reference wrapper that doesn't trigger re-renders.
///
/// Unlike signals, updating a Ref does NOT cause components to re-render.
/// This is useful for:
/// - Storing DOM/widget references
/// - Keeping mutable values that don't affect rendering
/// - Caching expensive computation results
/// - Storing previous values for comparison
///
/// # Thread Safety
///
/// Ref is fully thread-safe using `Arc<Mutex<T>>` with parking_lot for performance.
#[derive(Clone)]
pub struct Ref<T> {
    inner: Arc<Mutex<T>>,
}

impl<T> Ref<T> {
    /// Create a new ref with an initial value.
    pub fn new(value: T) -> Self {
        Self {
            inner: Arc::new(Mutex::new(value)),
        }
    }

    /// Get a mutable reference to the value.
    ///
    /// This locks the mutex and returns a guard that derefs to &mut T.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let counter = use_ref(ctx, 0);
    /// *counter.get_mut() += 1;
    /// ```
    pub fn get_mut(&self) -> impl DerefMut<Target = T> + '_ {
        self.inner.lock()
    }

    /// Get a reference to the value.
    ///
    /// This locks the mutex and returns a guard that derefs to &T.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let counter = use_ref(ctx, 0);
    /// println!("Value: {}", *counter.get());
    /// ```
    pub fn get(&self) -> impl Deref<Target = T> + '_ {
        self.inner.lock()
    }

    /// Set the value to a new value.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let counter = use_ref(ctx, 0);
    /// counter.set(42);
    /// ```
    pub fn set(&self, value: T) {
        *self.inner.lock() = value;
    }

    /// Update the value using a function.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let counter = use_ref(ctx, 0);
    /// counter.update(|n| *n += 1);
    /// ```
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        f(&mut *self.inner.lock());
    }

    /// Get the current value (requires T: Clone).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let counter = use_ref(ctx, 0);
    /// let value = counter.current();
    /// ```
    pub fn current(&self) -> T
    where
        T: Clone,
    {
        self.inner.lock().clone()
    }

    /// Execute a function with a reference to the value.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let counter = use_ref(ctx, 0);
    /// let doubled = counter.with(|n| n * 2);
    /// ```
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&*self.inner.lock())
    }

    /// Execute a function with a mutable reference to the value.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let counter = use_ref(ctx, 0);
    /// counter.with_mut(|n| *n += 1);
    /// ```
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        f(&mut *self.inner.lock())
    }
}

impl<T> std::fmt::Debug for Ref<T>
where
    T: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ref")
            .field("value", &*self.inner.lock())
            .finish()
    }
}

/// Hook state for RefHook.
#[derive(Clone, Debug)]
pub struct RefState<T> {
    reference: Ref<T>,
}

/// Ref hook implementation.
///
/// Creates a mutable reference that persists across renders without triggering re-renders.
pub struct RefHook<T>(std::marker::PhantomData<T>);

impl<T> Hook for RefHook<T>
where
    T: Clone + Send + 'static,
{
    type State = RefState<T>;
    type Input = T;
    type Output = Ref<T>;

    fn create(input: Self::Input) -> Self::State {
        RefState {
            reference: Ref::new(input),
        }
    }

    fn update(state: &mut Self::State, _input: Self::Input) -> Self::Output {
        // Return the existing ref (ignoring new input)
        // This matches React's behavior where useRef keeps the same ref
        state.reference.clone()
    }

    fn cleanup(_state: Self::State) {
        // No cleanup needed - Arc will be dropped automatically
    }
}

/// Create a mutable reference that persists across renders.
///
/// Unlike signals, updating a ref does NOT trigger re-renders.
/// The initial value is only used on the first render.
///
/// # Example
///
/// ```rust,ignore
/// // Store a counter that doesn't trigger re-renders
/// let render_count = use_ref(ctx, 0);
/// render_count.update(|n| *n += 1);
///
/// // Store previous value for comparison
/// let prev_value = use_ref(ctx, 0);
/// let current = count.get();
/// if current != prev_value.current() {
///     println!("Value changed!");
///     prev_value.set(current);
/// }
///
/// // Store a widget reference
/// let button_ref = use_ref(ctx, None::<WidgetId>);
/// ```
pub fn use_ref<T>(ctx: &mut HookContext, initial: T) -> Ref<T>
where
    T: Clone + Send + 'static,
{
    ctx.use_hook::<RefHook<T>>(initial)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ComponentId;

    #[test]
    fn test_ref_basic() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let counter = use_ref(&mut ctx, 0);
        assert_eq!(*counter.get(), 0);

        *counter.get_mut() += 1;
        assert_eq!(*counter.get(), 1);

        counter.set(42);
        assert_eq!(*counter.get(), 42);
    }

    #[test]
    fn test_ref_persists_across_renders() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        // First render
        let counter = use_ref(&mut ctx, 0);
        counter.set(42);
        assert_eq!(counter.current(), 42);

        ctx.end_component();

        // Second render
        ctx.begin_component(ComponentId(1));
        let counter = use_ref(&mut ctx, 0); // Initial value ignored
        assert_eq!(counter.current(), 42); // Value persists!
    }

    #[test]
    fn test_ref_update() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let counter = use_ref(&mut ctx, 0);
        counter.update(|n| *n += 1);
        assert_eq!(counter.current(), 1);

        counter.update(|n| *n *= 2);
        assert_eq!(counter.current(), 2);
    }

    #[test]
    fn test_ref_with() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let counter = use_ref(&mut ctx, 10);
        let doubled = counter.with(|n| n * 2);
        assert_eq!(doubled, 20);
        assert_eq!(counter.current(), 10); // Original unchanged
    }

    #[test]
    fn test_ref_with_mut() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let counter = use_ref(&mut ctx, 10);
        counter.with_mut(|n| *n *= 2);
        assert_eq!(counter.current(), 20);
    }

    #[test]
    fn test_ref_clone() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let ref1 = use_ref(&mut ctx, 0);
        let ref2 = ref1.clone();

        ref1.set(42);
        assert_eq!(ref2.current(), 42); // Both point to same value
    }

    #[test]
    fn test_ref_with_struct() {
        #[derive(Debug, Clone, PartialEq)]
        struct Point {
            x: i32,
            y: i32,
        }

        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let point = use_ref(&mut ctx, Point { x: 0, y: 0 });

        point.update(|p| {
            p.x = 10;
            p.y = 20;
        });

        assert_eq!(point.current(), Point { x: 10, y: 20 });
    }
}
