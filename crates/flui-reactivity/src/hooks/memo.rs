//! Memo hook for memoized computations.
//!
//! The `use_memo` hook computes a value and caches it until dependencies change.
//! This is useful for expensive computations that don't need to run on every render.

use crate::context::HookContext;
use crate::traits::{DependencyId, Hook};
use std::sync::Arc;

/// Hook state for MemoHook.
pub struct MemoState<T> {
    value: T,
    dependencies: Vec<DependencyId>,
}

impl<T: std::fmt::Debug> std::fmt::Debug for MemoState<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoState")
            .field("value", &self.value)
            .field("dependencies", &self.dependencies)
            .finish()
    }
}

/// Memo hook implementation.
///
/// Computes and caches a value until dependencies change.
pub struct MemoHook<T>(std::marker::PhantomData<T>);

impl<T> Hook for MemoHook<T>
where
    T: Clone + Send + 'static,
{
    type State = MemoState<T>;
    type Input = (Arc<dyn Fn() -> T + Send + Sync>, Vec<DependencyId>);
    type Output = T;

    fn create(input: Self::Input) -> Self::State {
        let (compute, dependencies) = input;
        let value = compute();

        MemoState {
            value,
            dependencies,
        }
    }

    fn update(state: &mut Self::State, input: Self::Input) -> Self::Output {
        let (compute, new_deps) = input;

        // Check if dependencies changed
        let deps_changed = state.dependencies != new_deps;

        if deps_changed {
            // Recompute value
            let new_value = compute();
            state.value = new_value;
            state.dependencies = new_deps;
        }

        state.value.clone()
    }

    fn cleanup(_state: Self::State) {
        // No cleanup needed
    }
}

/// Create a memoized value that only recomputes when dependencies change.
///
/// This is useful for expensive computations that don't need to run on every render.
///
/// # Dependencies
///
/// - Empty vec `vec![]` - Computes only once (on mount)
/// - Some deps `vec![dep1, dep2]` - Recomputes when any dependency changes
///
/// # Example
///
/// ```rust,ignore
/// let count_dep = DependencyId::new(count.id().0);
/// let doubled = use_memo(ctx, vec![count_dep], || {
///     // This expensive computation only runs when count changes
///     println!("Computing doubled value...");
///     count.get() * 2
/// });
/// ```
///
/// # Without Dependencies
///
/// ```rust,ignore
/// // Compute only once on mount
/// let initial_time = use_memo(ctx, vec![], || {
///     std::time::SystemTime::now()
/// });
/// ```
pub fn use_memo<T, F>(ctx: &mut HookContext, dependencies: Vec<DependencyId>, compute: F) -> T
where
    T: Clone + Send + 'static,
    F: Fn() -> T + Send + Sync + 'static,
{
    ctx.use_hook::<MemoHook<T>>((Arc::new(compute), dependencies))
}

/// Create a memoized value with no dependencies (computed only once).
///
/// Equivalent to `use_memo(ctx, vec![], compute)`.
///
/// # Example
///
/// ```rust,ignore
/// let initial_time = use_memo_once(ctx, || {
///     std::time::SystemTime::now()
/// });
/// ```
pub fn use_memo_once<T, F>(ctx: &mut HookContext, compute: F) -> T
where
    T: Clone + Send + 'static,
    F: Fn() -> T + Send + Sync + 'static,
{
    ctx.use_hook::<MemoHook<T>>((Arc::new(compute), vec![]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ComponentId;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_memo_basic() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let compute_count = Arc::new(AtomicUsize::new(0));
        let compute_clone = Arc::clone(&compute_count);

        let dep = DependencyId::new(1);

        let value = use_memo(&mut ctx, vec![dep], move || {
            compute_clone.fetch_add(1, Ordering::Relaxed);
            42
        });

        assert_eq!(value, 42);
        assert_eq!(compute_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_memo_memoization() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let compute_count = Arc::new(AtomicUsize::new(0));
        let dep = DependencyId::new(1);

        // First render
        let compute_clone = Arc::clone(&compute_count);
        let value = use_memo(&mut ctx, vec![dep], move || {
            compute_clone.fetch_add(1, Ordering::Relaxed);
            42
        });

        assert_eq!(value, 42);
        assert_eq!(compute_count.load(Ordering::Relaxed), 1);

        ctx.end_component();

        // Second render with same deps
        ctx.begin_component(ComponentId(1));
        let compute_clone = Arc::clone(&compute_count);
        let value = use_memo(&mut ctx, vec![dep], move || {
            compute_clone.fetch_add(1, Ordering::Relaxed);
            100 // Different value, but shouldn't be computed
        });

        // Should return cached value (42), not recompute
        assert_eq!(value, 42);
        assert_eq!(compute_count.load(Ordering::Relaxed), 1); // Not incremented!
    }

    #[test]
    fn test_memo_recomputes_on_dep_change() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let compute_count = Arc::new(AtomicUsize::new(0));
        let dep1 = DependencyId::new(1);

        // First render
        let compute_clone = Arc::clone(&compute_count);
        let value = use_memo(&mut ctx, vec![dep1], move || {
            compute_clone.fetch_add(1, Ordering::Relaxed);
            42
        });

        assert_eq!(value, 42);
        assert_eq!(compute_count.load(Ordering::Relaxed), 1);

        ctx.end_component();

        // Second render with different deps
        ctx.begin_component(ComponentId(1));
        let dep2 = DependencyId::new(2);
        let compute_clone = Arc::clone(&compute_count);
        let value = use_memo(&mut ctx, vec![dep2], move || {
            compute_clone.fetch_add(1, Ordering::Relaxed);
            100
        });

        // Should recompute with new value
        assert_eq!(value, 100);
        assert_eq!(compute_count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_memo_once() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let compute_count = Arc::new(AtomicUsize::new(0));

        // First render
        let compute_clone = Arc::clone(&compute_count);
        let value = use_memo_once(&mut ctx, move || {
            compute_clone.fetch_add(1, Ordering::Relaxed);
            42
        });

        assert_eq!(value, 42);
        assert_eq!(compute_count.load(Ordering::Relaxed), 1);

        ctx.end_component();

        // Second render
        ctx.begin_component(ComponentId(1));
        let compute_clone = Arc::clone(&compute_count);
        let value = use_memo_once(&mut ctx, move || {
            compute_clone.fetch_add(1, Ordering::Relaxed);
            100
        });

        // Should still return 42 (computed only once)
        assert_eq!(value, 42);
        assert_eq!(compute_count.load(Ordering::Relaxed), 1); // Not incremented!
    }

    #[test]
    fn test_memo_with_struct() {
        #[derive(Debug, Clone, PartialEq)]
        struct Point {
            x: i32,
            y: i32,
        }

        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let dep = DependencyId::new(1);

        let point = use_memo(&mut ctx, vec![dep], || Point { x: 10, y: 20 });

        assert_eq!(point, Point { x: 10, y: 20 });
    }

    #[test]
    fn test_memo_multiple_dependencies() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let compute_count = Arc::new(AtomicUsize::new(0));
        let dep1 = DependencyId::new(1);
        let dep2 = DependencyId::new(2);

        // First render
        let compute_clone = Arc::clone(&compute_count);
        let value = use_memo(&mut ctx, vec![dep1, dep2], move || {
            compute_clone.fetch_add(1, Ordering::Relaxed);
            42
        });

        assert_eq!(value, 42);
        assert_eq!(compute_count.load(Ordering::Relaxed), 1);

        ctx.end_component();

        // Second render with same deps
        ctx.begin_component(ComponentId(1));
        let compute_clone = Arc::clone(&compute_count);
        let value = use_memo(&mut ctx, vec![dep1, dep2], move || {
            compute_clone.fetch_add(1, Ordering::Relaxed);
            100
        });

        // Should not recompute
        assert_eq!(value, 42);
        assert_eq!(compute_count.load(Ordering::Relaxed), 1);

        ctx.end_component();

        // Third render with one dep changed
        ctx.begin_component(ComponentId(1));
        let dep3 = DependencyId::new(3);
        let compute_clone = Arc::clone(&compute_count);
        let value = use_memo(&mut ctx, vec![dep1, dep3], move || {
            compute_clone.fetch_add(1, Ordering::Relaxed);
            100
        });

        // Should recompute (one dep changed)
        assert_eq!(value, 100);
        assert_eq!(compute_count.load(Ordering::Relaxed), 2);
    }
}
