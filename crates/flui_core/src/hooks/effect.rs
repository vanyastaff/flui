//! Effect hook implementation for side effects.
//!
//! Provides `use_effect` hook that runs side effects after rendering,
//! similar to React's useEffect.

use super::hook_trait::{Hook, EffectHook as EffectHookTrait, DependencyId};
use super::hook_context::with_hook_context; // Still used by Effect::run_if_needed() for dependency tracking
use crate::BuildContext;
use std::rc::Rc;
use std::cell::RefCell;
use std::marker::PhantomData;

/// Cleanup function for effects.
pub type CleanupFn = Box<dyn FnOnce()>;

/// Inner state for an effect.
struct EffectInner {
    effect_fn: Rc<dyn Fn() -> Option<CleanupFn>>,
    cleanup_fn: RefCell<Option<CleanupFn>>,
    dependencies: RefCell<Vec<DependencyId>>,
    prev_deps: RefCell<Option<Vec<DependencyId>>>,
    ran_once: RefCell<bool>,
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
    inner: Rc<EffectInner>,
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
    pub fn run_if_needed(&self) {
        // Track current dependencies
        with_hook_context(|ctx| {
            ctx.start_tracking();
        });

        // Check if we should run
        let deps = with_hook_context(|ctx| ctx.end_tracking());

        let should_run = {
            let prev_deps = self.inner.prev_deps.borrow();
            let ran_once = *self.inner.ran_once.borrow();

            !ran_once || match prev_deps.as_ref() {
                None => true,
                Some(prev) => prev != &deps,
            }
        };

        if should_run {
            // Run cleanup from previous effect
            if let Some(cleanup) = self.inner.cleanup_fn.borrow_mut().take() {
                cleanup();
            }

            // Run effect
            let cleanup = (self.inner.effect_fn)();

            // Store cleanup and dependencies
            *self.inner.cleanup_fn.borrow_mut() = cleanup;
            *self.inner.dependencies.borrow_mut() = deps.clone();
            *self.inner.prev_deps.borrow_mut() = Some(deps);
            *self.inner.ran_once.borrow_mut() = true;
        }
    }

    /// Run cleanup manually.
    pub fn cleanup(&self) {
        if let Some(cleanup) = self.inner.cleanup_fn.borrow_mut().take() {
            cleanup();
        }
    }
}

impl Drop for Effect {
    fn drop(&mut self) {
        // Run cleanup on drop
        if let Some(cleanup) = self.inner.cleanup_fn.borrow_mut().take() {
            cleanup();
        }
    }
}

/// Hook state for EffectHook.
pub struct EffectState {
    inner: Rc<EffectInner>,
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
    F: Fn() -> Option<CleanupFn> + Clone + 'static,
{
    type State = EffectState;
    type Input = Rc<F>;
    type Output = Effect;

    fn create(effect: Rc<F>) -> Self::State {
        EffectState {
            inner: Rc::new(EffectInner {
                effect_fn: effect as Rc<dyn Fn() -> Option<CleanupFn>>,
                cleanup_fn: RefCell::new(None),
                dependencies: RefCell::new(Vec::new()),
                prev_deps: RefCell::new(None),
                ran_once: RefCell::new(false),
            }),
        }
    }

    fn update(state: &mut Self::State, _effect: Rc<F>) -> Self::Output {
        Effect {
            inner: state.inner.clone(),
        }
    }

    fn cleanup(state: Self::State) {
        // Run cleanup function if present
        if let Some(cleanup) = state.inner.cleanup_fn.borrow_mut().take() {
            cleanup();
        }
    }
}

impl<F> EffectHookTrait for EffectHook<F>
where
    F: Fn() -> Option<CleanupFn> + Clone + 'static,
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
///     fn build(&self, ctx: &BuildContext) -> Widget {
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
    F: Fn() -> Option<CleanupFn> + Clone + 'static,
{
    ctx.with_hook_context_mut(|hook_ctx| {
        let eff = hook_ctx.use_hook::<EffectHook<F>>(Rc::new(effect));
        // Run effect if needed
        eff.run_if_needed();
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
///     fn build(&self, ctx: &BuildContext) -> Widget {
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
    F: Fn() + Clone + 'static,
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
