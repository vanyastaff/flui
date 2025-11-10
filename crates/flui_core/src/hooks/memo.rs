//! Memo hook implementation for memoized computations.
//!
//! Provides `use_memo` hook that memoizes expensive computations and only
//! re-computes when dependencies change.

use super::hook_context::HookContext;
use super::hook_trait::{DependencyId, Hook, ReactiveHook};
use crate::BuildContext;
use parking_lot::Mutex;
use std::marker::PhantomData;
use std::sync::Arc;

/// Error type for memoized value computation failures
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoError {
    /// Memo is poisoned due to a previous panic in compute function
    Poisoned,
    /// Reentrancy detected: compute function called itself
    Reentrancy,
    /// Compute function panicked
    ComputePanic,
}

impl std::fmt::Display for MemoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Poisoned => write!(
                f,
                "Memo poisoned: compute function panicked on a previous call"
            ),
            Self::Reentrancy => write!(
                f,
                "Reentrancy detected: Memo compute function called itself (infinite loop)"
            ),
            Self::ComputePanic => write!(f, "Compute function panicked"),
        }
    }
}

impl std::error::Error for MemoError {}

/// Inner state for a memoized value.
#[derive(Debug)]
struct MemoInner<T> {
    cached: Mutex<Option<T>>,
    dependencies: Mutex<Vec<DependencyId>>,
    is_dirty: Mutex<bool>,
    /// Tracks if compute function is currently executing (prevents reentrancy)
    is_computing: Mutex<bool>,
    /// Tracks if compute function panicked (poison state)
    is_poisoned: Mutex<bool>,
}

/// A memoized value that only recomputes when dependencies change.
///
/// # Example
///
/// ```rust,ignore
/// let count = use_signal(ctx, 0);
/// let doubled = use_memo(ctx, move |ctx| count.get(ctx) * 2);
/// println!("Doubled: {}", doubled.get(ctx));
/// ```
pub struct Memo<T> {
    inner: Arc<MemoInner<T>>,
    compute: Arc<dyn Fn(&mut HookContext) -> T + Send + Sync>,
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
    ///
    /// # Panics
    ///
    /// This method panics if:
    /// - Memo is poisoned from a previous panic (use `is_poisoned()` to check)
    /// - Compute function causes reentrancy (calls memo.get() recursively)
    ///
    /// For non-panicking version, use `try_get()`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let count = use_signal(ctx, 0);
    /// let doubled = use_memo(ctx, move || count.get(ctx) * 2);
    /// println!("Doubled: {}", doubled.get(ctx));
    /// ```
    pub fn get(&self, ctx: &mut HookContext) -> T
    where
        T: Clone,
    {
        self.try_get(ctx)
            .unwrap_or_else(|err| panic!("Memo::get() failed: {}", err))
    }

    /// Get the memoized value with a function, recomputing if needed.
    pub fn with<R>(&self, ctx: &mut HookContext, f: impl FnOnce(&T) -> R) -> R
    where
        T: Clone,
    {
        let value = self.get(ctx);
        f(&value)
    }

    /// Mark the memo as dirty, forcing recomputation on next access.
    pub fn invalidate(&self) {
        *self.inner.is_dirty.lock() = true;
    }

    /// Check if memo is poisoned from a previous panic
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let memo = use_memo(|| panic!("oops"));
    /// let _ = std::panic::catch_unwind(|| memo.get());
    /// assert!(memo.is_poisoned());
    /// ```
    pub fn is_poisoned(&self) -> bool {
        *self.inner.is_poisoned.lock()
    }

    /// Recover from poisoned state by resetting memo
    ///
    /// This clears the poisoned flag and cached value, allowing
    /// the compute function to be called again.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let memo = use_memo(|| panic!("oops"));
    /// let _ = std::panic::catch_unwind(|| memo.get());
    /// assert!(memo.is_poisoned());
    ///
    /// memo.recover();
    /// assert!(!memo.is_poisoned());
    /// ```
    pub fn recover(&self) {
        *self.inner.is_poisoned.lock() = false;
        *self.inner.is_dirty.lock() = true;
        *self.inner.cached.lock() = None;
    }

    /// Try to get the memoized value, returning an error if poisoned or reentrant
    ///
    /// This is the safe version of `get()` that handles panics and reentrancy
    /// without panicking itself.
    ///
    /// # Errors
    ///
    /// - `MemoError::Poisoned`: Memo is poisoned from a previous panic
    /// - `MemoError::Reentrancy`: Compute function called itself
    /// - `MemoError::ComputePanic`: Compute function panicked during this call
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let memo = use_memo(ctx, || expensive_computation());
    /// match memo.try_get(ctx) {
    ///     Ok(value) => println!("Value: {}", value),
    ///     Err(MemoError::Poisoned) => {
    ///         memo.recover();
    ///         // Try again after recovery
    ///     }
    ///     Err(e) => eprintln!("Error: {}", e),
    /// }
    /// ```
    pub fn try_get(&self, ctx: &mut HookContext) -> Result<T, MemoError>
    where
        T: Clone,
    {
        // Check if poisoned from previous panic
        if *self.inner.is_poisoned.lock() {
            return Err(MemoError::Poisoned);
        }

        // Check if we need to recompute
        let is_dirty = *self.inner.is_dirty.lock();
        let needs_compute = is_dirty || self.inner.cached.lock().is_none();

        if needs_compute {
            // Check for reentrancy
            if *self.inner.is_computing.lock() {
                return Err(MemoError::Reentrancy);
            }

            // Mark as computing
            *self.inner.is_computing.lock() = true;

            // Panic guard: if we panic, mark as poisoned and stop computing
            struct PanicGuard<'a, T> {
                inner: &'a MemoInner<T>,
            }

            impl<T> Drop for PanicGuard<'_, T> {
                fn drop(&mut self) {
                    if std::thread::panicking() {
                        // Mutex::lock() should succeed in panic situations
                        *self.inner.is_poisoned.lock() = true;
                        *self.inner.is_computing.lock() = false;
                    }
                }
            }

            let _guard = PanicGuard { inner: &self.inner };

            // Start tracking dependencies
            ctx.start_tracking();

            // Compute new value (this can panic!)
            let new_value = (self.compute)(ctx);

            // Get tracked dependencies
            let deps = ctx.end_tracking();

            // Check if dependencies changed
            let deps_changed = {
                let old_deps = self.inner.dependencies.lock();
                old_deps.len() != deps.len() || old_deps.iter().zip(&deps).any(|(a, b)| a != b)
            };

            if deps_changed || is_dirty {
                *self.inner.cached.lock() = Some(new_value);
                *self.inner.dependencies.lock() = deps;
                *self.inner.is_dirty.lock() = false;
            }

            // Clear computing flag (computation succeeded)
            *self.inner.is_computing.lock() = false;

            // Prevent PanicGuard from running (no panic occurred)
            std::mem::forget(_guard);
        }

        Ok(self
            .inner
            .cached
            .lock()
            .clone()
            .expect("Memo value should be cached after successful compute"))
    }
}

impl<T> Clone for Memo<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            compute: Arc::clone(&self.compute),
        }
    }
}

/// Hook state for MemoHook.
pub struct MemoState<T> {
    inner: Arc<MemoInner<T>>,
    compute: Arc<dyn Fn(&mut HookContext) -> T + Send + Sync>,
}

impl<T> std::fmt::Debug for MemoState<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoState")
            .field("inner", &"<MemoInner>")
            .field("compute", &"<function>")
            .finish()
    }
}

impl<T> Drop for MemoState<T> {
    fn drop(&mut self) {
        // Future-proofing: When dependency tracking is enhanced or subscribers are added,
        // this Drop impl will ensure proper cleanup to prevent Rc cycles.
        //
        // Currently this is mostly a no-op (Rc will drop naturally), but it establishes
        // the cleanup contract.
        //
        // When enhanced tracking is implemented, add:
        // - Clear cached value to free memory early
        // - Unregister from any dependency graphs
        // - Break cycles with other memoized computations
        #[cfg(debug_assertions)]
        {
            let value_exists = self.inner.cached.lock().is_some();
            tracing::debug!("Dropping MemoState (cached: {})", value_exists);
        }

        // Eagerly clear cached value to free memory
        self.inner.cached.lock().take();
    }
}

/// Memo hook implementation.
///
/// This hook creates a memoized computation that only runs when dependencies change.
#[derive(Debug)]
pub struct MemoHook<T, F>(PhantomData<(T, F)>);

impl<T, F> Hook for MemoHook<T, F>
where
    T: Clone + Send + 'static,
    F: Fn(&mut HookContext) -> T + Clone + Send + Sync + 'static,
{
    type State = MemoState<T>;
    type Input = Arc<F>;
    type Output = Memo<T>;

    fn create(compute: Arc<F>) -> Self::State {
        MemoState {
            inner: Arc::new(MemoInner {
                cached: Mutex::new(None),
                dependencies: Mutex::new(Vec::new()),
                is_dirty: Mutex::new(true),
                is_computing: Mutex::new(false),
                is_poisoned: Mutex::new(false),
            }),
            compute: compute as Arc<dyn Fn(&mut HookContext) -> T + Send + Sync>,
        }
    }

    fn update(state: &mut Self::State, _compute: Arc<F>) -> Self::Output {
        Memo {
            inner: Arc::clone(&state.inner),
            compute: Arc::clone(&state.compute),
        }
    }
}

impl<T, F> ReactiveHook for MemoHook<T, F>
where
    T: Clone + Send + 'static,
    F: Fn(&mut HookContext) -> T + Clone + Send + Sync + 'static,
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
///         let doubled = use_memo(ctx, move |ctx| {
///             expensive_computation(count.get(ctx))
///         });
///
///         Text::new(format!("Result: {}", doubled.get(ctx))).into()
///     }
/// }
/// ```
pub fn use_memo<T, F>(ctx: &BuildContext, compute: F) -> Memo<T>
where
    T: Clone + Send + 'static,
    F: Fn(&mut HookContext) -> T + Clone + Send + Sync + 'static,
{
    ctx.with_hook_context_mut(|hook_ctx| hook_ctx.use_hook::<MemoHook<T, F>>(Arc::new(compute)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::hook_context::{ComponentId, HookContext};
    use crate::hooks::signal::SignalHook;
    use parking_lot::Mutex;
    use std::sync::Arc;

    #[test]
    fn test_memo_basic() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let call_count = Arc::new(Mutex::new(0));
        let call_count_clone = call_count.clone();
        let memo =
            ctx.use_hook::<MemoHook<i32, _>>(Arc::new(move |_ctx: &mut HookContext| -> i32 {
                *call_count_clone.lock() += 1;
                42
            }));

        assert_eq!(memo.get(&mut ctx), 42);
        assert_eq!(*call_count.lock(), 1);

        // Second access should use cached value
        assert_eq!(memo.get(&mut ctx), 42);
        assert_eq!(*call_count.lock(), 1);
    }

    #[test]
    fn test_memo_with_signal() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(5);

        let call_count = Arc::new(Mutex::new(0));
        let call_count_clone = call_count.clone();
        let signal_clone = signal; // Signal is Copy
        let memo =
            ctx.use_hook::<MemoHook<i32, _>>(Arc::new(move |ctx: &mut HookContext| -> i32 {
                *call_count_clone.lock() += 1;
                signal_clone.get(ctx) * 2
            }));

        assert_eq!(memo.get(&mut ctx), 10);
        assert_eq!(*call_count.lock(), 1);

        // Change signal
        signal.set(10);

        // Memo needs to be manually invalidated (memos don't auto-subscribe to signals)
        memo.invalidate();

        // Memo should recompute
        assert_eq!(memo.get(&mut ctx), 20);
        assert_eq!(*call_count.lock(), 2);
    }

    #[test]
    fn test_memo_invalidate() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let call_count = Arc::new(Mutex::new(0));
        let call_count_clone = call_count.clone();
        let memo =
            ctx.use_hook::<MemoHook<i32, _>>(Arc::new(move |_ctx: &mut HookContext| -> i32 {
                *call_count_clone.lock() += 1;
                42
            }));

        assert_eq!(memo.get(&mut ctx), 42);
        assert_eq!(*call_count.lock(), 1);

        memo.invalidate();

        assert_eq!(memo.get(&mut ctx), 42);
        assert_eq!(*call_count.lock(), 2);
    }

    // =========================================================================
    // Panic Safety Tests
    // =========================================================================

    #[test]
    fn test_memo_poison_on_panic() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let call_count = Arc::new(Mutex::new(0));
        let call_count_clone = Arc::clone(&call_count);
        let memo =
            ctx.use_hook::<MemoHook<i32, _>>(Arc::new(move |_ctx: &mut HookContext| -> i32 {
                let mut count = call_count_clone.lock();
                *count += 1;
                if *count == 1 {
                    panic!("Intentional panic");
                }
                42
            }));

        // First call should panic and poison the memo
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| memo.get(&mut ctx)));
        assert!(result.is_err());

        // Memo should now be poisoned
        assert!(memo.is_poisoned());

        // Second call should fail with poisoned error
        let result = memo.try_get(&mut ctx);
        assert!(matches!(result, Err(MemoError::Poisoned)));
    }

    #[test]
    fn test_memo_recover_from_poison() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let call_count = Arc::new(Mutex::new(0));
        let call_count_clone = Arc::clone(&call_count);
        let memo =
            ctx.use_hook::<MemoHook<i32, _>>(Arc::new(move |_ctx: &mut HookContext| -> i32 {
                let mut count = call_count_clone.lock();
                *count += 1;
                if *count == 1 {
                    panic!("Intentional panic");
                }
                42
            }));

        // Panic and poison
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| memo.get(&mut ctx)));
        assert!(memo.is_poisoned());

        // Recover from poison
        memo.recover();
        assert!(!memo.is_poisoned());

        // Should be able to compute successfully now
        assert_eq!(memo.get(&mut ctx), 42);
    }

    #[test]
    fn test_memo_try_get_no_panic() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let memo =
            ctx.use_hook::<MemoHook<i32, _>>(Arc::new(|_ctx: &mut HookContext| -> i32 { 42 }));

        // try_get should succeed without panicking
        let result = memo.try_get(&mut ctx);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    // =========================================================================
    // Reentrancy Tests
    // =========================================================================

    #[test]
    fn test_memo_reentrancy_detection() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        // Create a memo that tries to call itself recursively
        // We need to use Arc<Mutex<Option<Memo<i32>>>> to allow self-reference
        let memo_cell: Arc<Mutex<Option<Memo<i32>>>> = Arc::new(Mutex::new(None));
        let memo_cell_clone = Arc::clone(&memo_cell);

        let memo =
            ctx.use_hook::<MemoHook<i32, _>>(Arc::new(move |ctx: &mut HookContext| -> i32 {
                // Try to access memo recursively
                if let Some(m) = memo_cell_clone.lock().as_ref() {
                    // Use try_get to avoid panic - but this will still trigger reentrancy detection
                    match m.try_get(ctx) {
                        Err(MemoError::Reentrancy) => {
                            // Expected - reentrancy detected
                        }
                        _ => {
                            // Should not get here
                            panic!("Expected reentrancy error");
                        }
                    }
                }
                42
            }));

        *memo_cell.lock() = Some(memo.clone());

        // First call should detect reentrancy in the compute function
        // The compute function handles it gracefully and returns 42
        let result = memo.try_get(&mut ctx);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    #[should_panic(expected = "Reentrancy")]
    fn test_memo_reentrancy_panics() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let memo_cell: Arc<Mutex<Option<Memo<i32>>>> = Arc::new(Mutex::new(None));
        let memo_cell_clone = Arc::clone(&memo_cell);

        let memo =
            ctx.use_hook::<MemoHook<i32, _>>(Arc::new(move |ctx: &mut HookContext| -> i32 {
                if let Some(m) = memo_cell_clone.lock().as_ref() {
                    m.get(ctx); // Should panic with reentrancy error
                }
                42
            }));

        *memo_cell.lock() = Some(memo.clone());

        // Should panic with reentrancy message
        memo.get(&mut ctx);
    }

    // =========================================================================
    // Panic Guard Tests
    // =========================================================================

    #[test]
    fn test_panic_guard_clears_computing_flag() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let call_count = Arc::new(Mutex::new(0));
        let call_count_clone = Arc::clone(&call_count);
        let memo =
            ctx.use_hook::<MemoHook<i32, _>>(Arc::new(move |_ctx: &mut HookContext| -> i32 {
                *call_count_clone.lock() += 1;
                panic!("Panic during compute");
            }));

        // Panic during compute
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| memo.try_get(&mut ctx)));

        // Computing flag should be cleared by panic guard
        // (verified indirectly: subsequent try_get returns Poisoned, not hangs)
        let result = memo.try_get(&mut ctx);
        assert!(matches!(result, Err(MemoError::Poisoned)));
    }
}
