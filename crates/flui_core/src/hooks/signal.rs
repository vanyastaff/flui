//! Signal hook implementation for reactive state.
//!
//! Provides `use_signal` hook that creates reactive state similar to React's useState.
//! When a signal changes, all components that depend on it are automatically re-rendered.

use super::hook_trait::{Hook, DependencyId};
use super::hook_context::with_hook_context;
use std::cell::RefCell;
use std::rc::Rc;
use std::marker::PhantomData;

/// Unique identifier for a signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SignalId(u64);

impl SignalId {
    /// Create a new signal ID.
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for SignalId {
    fn default() -> Self {
        Self::new()
    }
}

/// Inner signal state shared between Signal instances.
#[derive(Debug)]
struct SignalInner<T> {
    value: Rc<RefCell<T>>,
    id: SignalId,
}

/// A reactive signal that can be read and updated.
///
/// When a signal is updated, it automatically tracks dependencies and
/// notifies dependent components to re-render.
///
/// # Example
///
/// ```rust,ignore
/// let count = use_signal(0);
/// println!("Count: {}", count.get());
/// count.set(42);
/// count.update(|n| n + 1);
/// ```
#[derive(Debug)]
pub struct Signal<T> {
    inner: Rc<SignalInner<T>>,
}

impl<T> Signal<T> {
    /// Get the current value of the signal.
    ///
    /// This tracks the signal as a dependency.
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        // Track dependency
        with_hook_context(|ctx| {
            ctx.track_dependency(DependencyId::new(self.inner.id.0));
        });

        self.inner.value.borrow().clone()
    }

    /// Get a reference to the current value without cloning.
    ///
    /// This tracks the signal as a dependency.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        // Track dependency
        with_hook_context(|ctx| {
            ctx.track_dependency(DependencyId::new(self.inner.id.0));
        });

        f(&*self.inner.value.borrow())
    }

    /// Set the signal to a new value.
    ///
    /// This will trigger re-renders of dependent components.
    pub fn set(&self, value: T) {
        *self.inner.value.borrow_mut() = value;
        // TODO(2025-03): Notify subscribers
    }

    /// Update the signal using a function.
    ///
    /// This is useful for updates that depend on the current value.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// count.update(|n| n + 1);
    /// ```
    pub fn update(&self, f: impl FnOnce(T) -> T)
    where
        T: Clone,
    {
        let old_value = self.inner.value.borrow().clone();
        let new_value = f(old_value);
        *self.inner.value.borrow_mut() = new_value;
        // TODO(2025-03): Notify subscribers
    }

    /// Update the signal by mutating it in place.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// count.update_mut(|n| *n += 1);
    /// ```
    pub fn update_mut(&self, f: impl FnOnce(&mut T)) {
        f(&mut *self.inner.value.borrow_mut());
        // TODO(2025-03): Notify subscribers
    }

    /// Get the signal ID.
    pub fn id(&self) -> SignalId {
        self.inner.id
    }
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

/// Hook state for SignalHook.
#[derive(Debug)]
pub struct SignalState<T> {
    value: Rc<RefCell<T>>,
    id: SignalId,
}

/// Signal hook implementation.
///
/// This hook creates a reactive signal that can be read and updated.
#[derive(Debug)]
pub struct SignalHook<T>(PhantomData<T>);

impl<T: Clone + 'static> Hook for SignalHook<T> {
    type State = SignalState<T>;
    type Input = T;
    type Output = Signal<T>;

    fn create(initial: T) -> Self::State {
        SignalState {
            value: Rc::new(RefCell::new(initial)),
            id: SignalId::new(),
        }
    }

    fn update(state: &mut Self::State, _input: T) -> Self::Output {
        Signal {
            inner: Rc::new(SignalInner {
                value: state.value.clone(),
                id: state.id,
            }),
        }
    }
}

/// Create a reactive signal.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::hooks::use_signal;
///
/// struct Counter;
///
/// impl Component for Counter {
///     fn build(&self, ctx: &mut BuildContext) -> Widget {
///         let count = use_signal(0);
///
///         Button::new("Increment")
///             .on_press(move || count.update(|n| n + 1))
///             .into()
///     }
/// }
/// ```
pub fn use_signal<T: Clone + 'static>(initial: T) -> Signal<T> {
    with_hook_context(|ctx| {
        ctx.use_hook::<SignalHook<T>>(initial)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::hook_context::{ComponentId, HookContext};

    #[test]
    fn test_signal_get_set() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);
        assert_eq!(signal.get(), 0);

        signal.set(42);
        assert_eq!(signal.get(), 42);
    }

    #[test]
    fn test_signal_update() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);
        signal.update(|n| n + 1);
        assert_eq!(signal.get(), 1);

        signal.update(|n| n * 2);
        assert_eq!(signal.get(), 2);
    }

    #[test]
    fn test_signal_clone() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal1 = ctx.use_hook::<SignalHook<i32>>(0);
        let signal2 = signal1.clone();

        signal1.set(42);
        assert_eq!(signal2.get(), 42);
    }
}
