//! Memo hook implementation for memoized computations.
//!
//! Provides `use_memo` hook that memoizes expensive computations and only
//! re-computes when dependencies change.

use super::hook_trait::{Hook, ReactiveHook, DependencyId};
use super::hook_context::with_hook_context; // Still used by Memo::get() for dependency tracking
use crate::BuildContext;
use std::cell::RefCell;
use std::rc::Rc;
use std::marker::PhantomData;

/// Inner state for a memoized value.
#[derive(Debug)]
struct MemoInner<T> {
    cached: RefCell<Option<T>>,
    dependencies: RefCell<Vec<DependencyId>>,
    is_dirty: RefCell<bool>,
}

/// A memoized value that only recomputes when dependencies change.
///
/// # Example
///
/// ```rust,ignore
/// let count = use_signal(0);
/// let doubled = use_memo(move || count.get() * 2);
/// println!("Doubled: {}", doubled.get());
/// ```
pub struct Memo<T> {
    inner: Rc<MemoInner<T>>,
    compute: Rc<dyn Fn() -> T>,
}

impl<T> std::fmt::Debug for Memo<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Memo")
            .field("inner", &"<MemoInner>")
            .field("compute", &"<function>")
            .finish()
    }
}

impl<T> Memo<T> {
    /// Get the memoized value, recomputing if dependencies changed.
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        // Check if we need to recompute
        let is_dirty = *self.inner.is_dirty.borrow();

        if is_dirty || self.inner.cached.borrow().is_none() {
            // Start tracking dependencies
            with_hook_context(|ctx| {
                ctx.start_tracking();
            });

            // Compute new value
            let new_value = (self.compute)();

            // Get tracked dependencies
            let deps = with_hook_context(|ctx| ctx.end_tracking());

            // Check if dependencies changed
            let deps_changed = {
                let old_deps = self.inner.dependencies.borrow();
                old_deps.len() != deps.len() ||
                    old_deps.iter().zip(&deps).any(|(a, b)| a != b)
            };

            if deps_changed || is_dirty {
                *self.inner.cached.borrow_mut() = Some(new_value);
                *self.inner.dependencies.borrow_mut() = deps;
                *self.inner.is_dirty.borrow_mut() = false;
            }
        }

        self.inner.cached.borrow().clone().expect("Memo value should be cached")
    }

    /// Get the memoized value with a function, recomputing if needed.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R
    where
        T: Clone,
    {
        let value = self.get();
        f(&value)
    }

    /// Mark the memo as dirty, forcing recomputation on next access.
    pub fn invalidate(&self) {
        *self.inner.is_dirty.borrow_mut() = true;
    }
}

impl<T> Clone for Memo<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            compute: self.compute.clone(),
        }
    }
}

/// Hook state for MemoHook.
pub struct MemoState<T> {
    inner: Rc<MemoInner<T>>,
    compute: Rc<dyn Fn() -> T>,
}

impl<T> std::fmt::Debug for MemoState<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoState")
            .field("inner", &"<MemoInner>")
            .field("compute", &"<function>")
            .finish()
    }
}

/// Memo hook implementation.
///
/// This hook creates a memoized computation that only runs when dependencies change.
#[derive(Debug)]
pub struct MemoHook<T, F>(PhantomData<(T, F)>);

impl<T, F> Hook for MemoHook<T, F>
where
    T: Clone + 'static,
    F: Fn() -> T + Clone + 'static,
{
    type State = MemoState<T>;
    type Input = Rc<F>;
    type Output = Memo<T>;

    fn create(compute: Rc<F>) -> Self::State {
        MemoState {
            inner: Rc::new(MemoInner {
                cached: RefCell::new(None),
                dependencies: RefCell::new(Vec::new()),
                is_dirty: RefCell::new(true),
            }),
            compute: compute as Rc<dyn Fn() -> T>,
        }
    }

    fn update(state: &mut Self::State, _compute: Rc<F>) -> Self::Output {
        Memo {
            inner: state.inner.clone(),
            compute: state.compute.clone(),
        }
    }
}

impl<T, F> ReactiveHook for MemoHook<T, F>
where
    T: Clone + 'static,
    F: Fn() -> T + Clone + 'static,
{
    fn track_dependencies(&self) -> Vec<DependencyId> {
        // Dependencies are tracked during computation
        vec![]
    }
}

/// Create a memoized computation.
///
/// The computation is only re-run when its dependencies change.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::hooks::{use_signal, use_memo};
///
/// struct ExpensiveComponent;
///
/// impl Component for ExpensiveComponent {
///     fn build(&self, ctx: &BuildContext) -> View {
///         let count = use_signal(ctx, 0);
///
///         // This expensive computation only runs when count changes
///         let doubled = use_memo(ctx, move || {
///             expensive_computation(count.get())
///         });
///
///         Text::new(format!("Result: {}", doubled.get())).into()
///     }
/// }
/// ```
pub fn use_memo<T, F>(ctx: &BuildContext, compute: F) -> Memo<T>
where
    T: Clone + 'static,
    F: Fn() -> T + Clone + 'static,
{
    ctx.with_hook_context_mut(|hook_ctx| {
        hook_ctx.use_hook::<MemoHook<T, F>>(Rc::new(compute))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::hook_context::{ComponentId, HookContext};
    use crate::hooks::signal::{use_signal, SignalHook};

    #[test]
    fn test_memo_basic() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let mut call_count = 0;
        let memo = ctx.use_hook::<MemoHook<i32, _>>(|| {
            call_count += 1;
            42
        });

        assert_eq!(memo.get(), 42);
        assert_eq!(call_count, 1);

        // Second access should use cached value
        assert_eq!(memo.get(), 42);
        assert_eq!(call_count, 1);
    }

    #[test]
    fn test_memo_with_signal() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(5);

        let mut call_count = 0;
        let memo = ctx.use_hook::<MemoHook<i32, _>>(move || {
            call_count += 1;
            signal.get() * 2
        });

        assert_eq!(memo.get(), 10);
        assert_eq!(call_count, 1);

        // Change signal
        signal.set(10);

        // Memo should recompute
        assert_eq!(memo.get(), 20);
        assert_eq!(call_count, 2);
    }

    #[test]
    fn test_memo_invalidate() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let mut call_count = 0;
        let memo = ctx.use_hook::<MemoHook<i32, _>>(|| {
            call_count += 1;
            42
        });

        assert_eq!(memo.get(), 42);
        assert_eq!(call_count, 1);

        memo.invalidate();

        assert_eq!(memo.get(), 42);
        assert_eq!(call_count, 2);
    }
}
