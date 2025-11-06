//! Effect hook implementation for side effects.
//!
//! Provides `use_effect` hook that runs side effects after rendering,
//! similar to React's useEffect.

use super::hook_context::HookContext;
use super::hook_trait::{DependencyId, EffectHook as EffectHookTrait, Hook};
use crate::BuildContext;
use parking_lot::Mutex;
use std::marker::PhantomData;
use std::sync::Arc;

/// Cleanup function for effects.
pub type CleanupFn = Box<dyn FnOnce() + Send>;

/// Inner state for an effect.
struct EffectInner {
    effect_fn: Arc<dyn Fn() -> Option<CleanupFn> + Send + Sync>,
    cleanup_fn: Mutex<Option<CleanupFn>>,
    dependencies: Mutex<Vec<DependencyId>>,
    prev_deps: Mutex<Option<Vec<DependencyId>>>,
    ran_once: Mutex<bool>,
}

impl std::fmt::Debug for EffectInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EffectInner")
            .field("effect_fn", &"<function>")
            .field("cleanup_fn", &"<cleanup>")
            .field("dependencies", &self.dependencies)
            .field("prev_deps", &self.prev_deps)
            .field("ran_once", &self.ran_once)
            .finish()
    }
}

/// Effect wrapper that manages side effects.
pub struct Effect {
    inner: Arc<EffectInner>,
}

impl std::fmt::Debug for Effect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Effect")
            .field("inner", &self.inner)
            .finish()
    }
}

impl Effect {
    /// Run the effect if dependencies changed.
    pub fn run_if_needed(&self, ctx: &mut HookContext) {
        // Track current dependencies
        ctx.start_tracking();

        // Check if we should run
        let deps = ctx.end_tracking();

        let should_run = {
            let prev_deps = self.inner.prev_deps.lock();
            let ran_once = *self.inner.ran_once.lock();

            !ran_once
                || match prev_deps.as_ref() {
                    None => true,
                    Some(prev) => prev != &deps,
                }
        };

        if should_run {
            // Run cleanup from previous effect
            if let Some(cleanup) = self.inner.cleanup_fn.lock().take() {
                cleanup();
            }

            // Run effect
            let cleanup = (self.inner.effect_fn)();

            // Store cleanup and dependencies
            *self.inner.cleanup_fn.lock() = cleanup;
            *self.inner.dependencies.lock() = deps.clone();
            *self.inner.prev_deps.lock() = Some(deps);
            *self.inner.ran_once.lock() = true;
        }
    }

    /// Run cleanup manually.
    pub fn cleanup(&self) {
        if let Some(cleanup) = self.inner.cleanup_fn.lock().take() {
            cleanup();
        }
    }
}

impl Drop for Effect {
    fn drop(&mut self) {
        // Run cleanup on drop, with panic safety
        // Mutex::lock() doesn't fail in normal circumstances
        if let Some(cleanup) = self.inner.cleanup_fn.lock().take() {
            // Catch panics in cleanup to prevent double panic during unwinding
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                cleanup();
            }));
        }
    }
}

/// Hook state for EffectHook.
pub struct EffectState {
    inner: Arc<EffectInner>,
}

impl std::fmt::Debug for EffectState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EffectState")
            .field("inner", &self.inner)
            .finish()
    }
}

/// Effect hook implementation.
///
/// This hook runs side effects after rendering completes.
#[derive(Debug)]
pub struct EffectHook<F>(PhantomData<F>);

impl<F> Hook for EffectHook<F>
where
    F: Fn() -> Option<CleanupFn> + Clone + Send + Sync + 'static,
{
    type State = EffectState;
    type Input = Arc<F>;
    type Output = Effect;

    fn create(effect: Arc<F>) -> Self::State {
        EffectState {
            inner: Arc::new(EffectInner {
                effect_fn: effect as Arc<dyn Fn() -> Option<CleanupFn> + Send + Sync>,
                cleanup_fn: Mutex::new(None),
                dependencies: Mutex::new(Vec::new()),
                prev_deps: Mutex::new(None),
                ran_once: Mutex::new(false),
            }),
        }
    }

    fn update(state: &mut Self::State, _effect: Arc<F>) -> Self::Output {
        Effect {
            inner: Arc::clone(&state.inner),
        }
    }

    fn cleanup(state: Self::State) {
        // Run cleanup function if present
        if let Some(cleanup) = state.inner.cleanup_fn.lock().take() {
            cleanup();
        }
    }
}

impl<F> EffectHookTrait for EffectHook<F>
where
    F: Fn() -> Option<CleanupFn> + Clone + Send + Sync + 'static,
{
    fn run_effect(&mut self) {
        // Effect is run via Effect::run_if_needed()
    }

    fn run_cleanup(&mut self) {
        // Cleanup is run automatically
    }
}

/// Run a side effect after rendering.
///
/// The effect runs after every render where dependencies have changed.
/// If the effect returns a cleanup function, it will be called before
/// the next effect or when the component unmounts.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::hooks::{use_signal, use_effect};
///
/// struct Logger;
///
/// impl Component for Logger {
///     fn build(&self, ctx: &BuildContext) -> View {
///         let count = use_signal(ctx, 0);
///
///         use_effect(ctx, move || {
///             println!("Count changed: {}", count.get());
///
///             // Optional cleanup
///             Some(Box::new(|| {
///                 println!("Cleaning up...");
///             }))
///         });
///
///         Text::new(format!("Count: {}", count.get())).into()
///     }
/// }
/// ```
pub fn use_effect<F>(ctx: &BuildContext, effect: F) -> Effect
where
    F: Fn() -> Option<CleanupFn> + Clone + Send + Sync + 'static,
{
    ctx.with_hook_context_mut(|hook_ctx| {
        let eff = hook_ctx.use_hook::<EffectHook<F>>(Arc::new(effect));
        // Run effect if needed
        eff.run_if_needed(hook_ctx);
        eff
    })
}

/// Run a side effect without cleanup.
///
/// This is a convenience wrapper for `use_effect` when you don't need cleanup.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::hooks::{use_signal, use_effect_simple};
///
/// struct Logger;
///
/// impl Component for Logger {
///     fn build(&self, ctx: &BuildContext) -> View {
///         let count = use_signal(ctx, 0);
///
///         use_effect_simple(ctx, move || {
///             println!("Count: {}", count.get());
///         });
///
///         Text::new(format!("Count: {}", count.get())).into()
///     }
/// }
/// ```
pub fn use_effect_simple<F>(ctx: &BuildContext, effect: F) -> Effect
where
    F: Fn() + Clone + Send + Sync + 'static,
{
    use_effect(ctx, move || {
        effect();
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::hook_context::{ComponentId, HookContext};
    use crate::hooks::signal::SignalHook;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_effect_runs_once() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_clone = call_count.clone();

        let effect = ctx.use_hook::<EffectHook<_>>(move || {
            call_count_clone.fetch_add(1, Ordering::Relaxed);
            None
        });

        effect.run_if_needed();
        assert_eq!(call_count.load(Ordering::Relaxed), 1);

        // Second run should not execute (no dependency changes)
        effect.run_if_needed();
        assert_eq!(call_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_effect_with_cleanup() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let cleanup_count = Arc::new(AtomicUsize::new(0));
        let cleanup_count_clone = cleanup_count.clone();

        let effect = ctx.use_hook::<EffectHook<_>>(move || {
            let cleanup_count = cleanup_count_clone.clone();
            Some(Box::new(move || {
                cleanup_count.fetch_add(1, Ordering::Relaxed);
            }) as CleanupFn)
        });

        effect.run_if_needed();
        assert_eq!(cleanup_count.load(Ordering::Relaxed), 0);

        // Run cleanup manually
        effect.cleanup();
        assert_eq!(cleanup_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_effect_with_dependency() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);
        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_clone = call_count.clone();

        let effect = ctx.use_hook::<EffectHook<_>>(move || {
            let _value = signal.get();
            call_count_clone.fetch_add(1, Ordering::Relaxed);
            None
        });

        effect.run_if_needed();
        assert_eq!(call_count.load(Ordering::Relaxed), 1);

        // Change signal
        signal.set(10);

        // Effect should run again
        effect.run_if_needed();
        assert_eq!(call_count.load(Ordering::Relaxed), 2);
    }
}
