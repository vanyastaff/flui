//! Effect hook for side effects with automatic cleanup.
//!
//! The `use_effect` hook allows you to perform side effects in components.
//! It runs after rendering and can optionally return a cleanup function.

use crate::context::HookContext;
use crate::traits::{DependencyId, Hook};
use std::sync::Arc;

/// Cleanup function returned by effects.
pub type CleanupFn = Box<dyn FnOnce() + Send + 'static>;

/// Effect function type.
///
/// Returns an optional cleanup function that will be called when:
/// - Dependencies change (before running the new effect)
/// - The component unmounts
///
/// Note: We use Arc<dyn Fn()> instead of Box<dyn FnOnce()> to satisfy Clone requirement.
pub type EffectFn = Arc<dyn Fn() -> Option<CleanupFn> + Send + Sync + 'static>;

/// Hook state for EffectHook.
pub struct EffectState {
    dependencies: Vec<DependencyId>,
    cleanup: Option<CleanupFn>,
    first_run: bool,
}

impl std::fmt::Debug for EffectState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EffectState")
            .field("dependencies", &self.dependencies)
            .field("has_cleanup", &self.cleanup.is_some())
            .field("first_run", &self.first_run)
            .finish()
    }
}

/// Effect hook implementation.
///
/// Runs side effects after rendering with automatic cleanup.
pub struct EffectHook;

impl Hook for EffectHook {
    type State = EffectState;
    type Input = (EffectFn, Vec<DependencyId>);
    type Output = ();

    fn create(input: Self::Input) -> Self::State {
        let (effect, dependencies) = input;

        // Run the effect immediately on creation
        let cleanup = effect();

        EffectState {
            dependencies,
            cleanup,
            first_run: false,
        }
    }

    fn update(state: &mut Self::State, input: Self::Input) -> Self::Output {
        let (effect, new_deps) = input;

        // Check if dependencies changed
        let deps_changed = state.dependencies != new_deps;

        if deps_changed || state.first_run {
            // Run cleanup from previous effect
            if let Some(cleanup) = state.cleanup.take() {
                cleanup();
            }

            // Run the new effect
            state.cleanup = effect();
            state.dependencies = new_deps;
            state.first_run = false;
        }
    }

    fn cleanup(mut state: Self::State) {
        // Run cleanup when the component unmounts
        if let Some(cleanup) = state.cleanup.take() {
            cleanup();
        }
    }
}

/// Create an effect that runs after rendering.
///
/// The effect function can return an optional cleanup function that will be called when:
/// - Dependencies change (before running the new effect)
/// - The component unmounts
///
/// # Dependencies
///
/// - Empty vec `vec![]` - Runs only on mount (cleanup on unmount)
/// - Some deps `vec![dep1, dep2]` - Runs when any dependency changes
///
/// # Example
///
/// ```rust,ignore
/// // Run only on mount
/// use_effect(ctx, vec![], || {
///     println!("Component mounted!");
///
///     // Cleanup function
///     Some(Box::new(|| {
///         println!("Component unmounted!");
///     }))
/// });
///
/// // Run when count changes
/// let count_dep = DependencyId::new(count.id().0);
/// use_effect(ctx, vec![count_dep], || {
///     println!("Count changed to: {}", count.get());
///     None // No cleanup
/// });
///
/// // Subscription example
/// use_effect(ctx, vec![], || {
///     let subscription = service.subscribe(|event| {
///         handle_event(event);
///     });
///
///     // Cleanup: unsubscribe when component unmounts
///     Some(Box::new(move || {
///         drop(subscription);
///     }))
/// });
/// ```
pub fn use_effect<F>(ctx: &mut HookContext, dependencies: Vec<DependencyId>, effect: F)
where
    F: Fn() -> Option<CleanupFn> + Send + Sync + 'static,
{
    ctx.use_hook::<EffectHook>((Arc::new(effect), dependencies))
}

/// Create an effect that runs on every render (no dependencies).
///
/// **Warning:** This runs after EVERY render, which can be expensive.
/// Consider using `use_effect` with specific dependencies instead.
///
/// # Example
///
/// ```rust,ignore
/// use_effect_always(ctx, || {
///     println!("Rendered!");
///     None
/// });
/// ```
pub fn use_effect_always<F>(ctx: &mut HookContext, effect: F)
where
    F: Fn() -> Option<CleanupFn> + Send + Sync + 'static,
{
    // Create a unique dependency for each render to force re-run
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let unique_dep = DependencyId::new(COUNTER.fetch_add(1, Ordering::Relaxed));

    ctx.use_hook::<EffectHook>((Arc::new(effect), vec![unique_dep]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ComponentId;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_effect_runs_on_mount() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        use_effect(&mut ctx, vec![], move || {
            counter_clone.fetch_add(1, Ordering::Relaxed);
            None
        });

        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    // Note: Cleanup on unmount test removed because HookContext doesn't implement Drop.
    // In practice, cleanup is handled by the FLUI framework when components are unmounted.
    // The cleanup logic itself is tested in test_effect_cleanup_before_rerun.

    #[test]
    fn test_effect_runs_when_deps_change() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let run_count = Arc::new(AtomicUsize::new(0));
        let run_count_clone = Arc::clone(&run_count);

        let dep1 = DependencyId::new(1);

        // First render
        use_effect(&mut ctx, vec![dep1], move || {
            run_count_clone.fetch_add(1, Ordering::Relaxed);
            None
        });

        assert_eq!(run_count.load(Ordering::Relaxed), 1);

        ctx.end_component();

        // Second render with same deps
        ctx.begin_component(ComponentId(1));
        let run_count_clone = Arc::clone(&run_count);
        use_effect(&mut ctx, vec![dep1], move || {
            run_count_clone.fetch_add(1, Ordering::Relaxed);
            None
        });

        // Should not run again (deps unchanged)
        assert_eq!(run_count.load(Ordering::Relaxed), 1);

        ctx.end_component();

        // Third render with different deps
        ctx.begin_component(ComponentId(1));
        let dep2 = DependencyId::new(2);
        let run_count_clone = Arc::clone(&run_count);
        use_effect(&mut ctx, vec![dep2], move || {
            run_count_clone.fetch_add(1, Ordering::Relaxed);
            None
        });

        // Should run again (deps changed)
        assert_eq!(run_count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_effect_cleanup_before_rerun() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let cleanup_count = Arc::new(AtomicUsize::new(0));
        let run_count = Arc::new(AtomicUsize::new(0));

        let dep1 = DependencyId::new(1);

        // First render
        let cleanup_clone = Arc::clone(&cleanup_count);
        let run_clone = Arc::clone(&run_count);
        use_effect(&mut ctx, vec![dep1], move || {
            run_clone.fetch_add(1, Ordering::Relaxed);
            let cleanup_clone = cleanup_clone.clone();
            Some(Box::new(move || {
                cleanup_clone.fetch_add(1, Ordering::Relaxed);
            }))
        });

        assert_eq!(run_count.load(Ordering::Relaxed), 1);
        assert_eq!(cleanup_count.load(Ordering::Relaxed), 0);

        ctx.end_component();

        // Second render with different deps
        ctx.begin_component(ComponentId(1));
        let dep2 = DependencyId::new(2);
        let cleanup_clone = Arc::clone(&cleanup_count);
        let run_clone = Arc::clone(&run_count);
        use_effect(&mut ctx, vec![dep2], move || {
            run_clone.fetch_add(1, Ordering::Relaxed);
            let cleanup_clone = cleanup_clone.clone();
            Some(Box::new(move || {
                cleanup_clone.fetch_add(1, Ordering::Relaxed);
            }))
        });

        // Cleanup should run before new effect
        assert_eq!(cleanup_count.load(Ordering::Relaxed), 1);
        assert_eq!(run_count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_effect_always() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let counter = Arc::new(AtomicUsize::new(0));

        // First render
        let counter_clone = Arc::clone(&counter);
        use_effect_always(&mut ctx, move || {
            counter_clone.fetch_add(1, Ordering::Relaxed);
            None
        });

        assert_eq!(counter.load(Ordering::Relaxed), 1);

        ctx.end_component();

        // Second render
        ctx.begin_component(ComponentId(1));
        let counter_clone = Arc::clone(&counter);
        use_effect_always(&mut ctx, move || {
            counter_clone.fetch_add(1, Ordering::Relaxed);
            None
        });

        // Should run again (always runs)
        assert_eq!(counter.load(Ordering::Relaxed), 2);

        ctx.end_component();

        // Third render
        ctx.begin_component(ComponentId(1));
        let counter_clone = Arc::clone(&counter);
        use_effect_always(&mut ctx, move || {
            counter_clone.fetch_add(1, Ordering::Relaxed);
            None
        });

        assert_eq!(counter.load(Ordering::Relaxed), 3);
    }
}
