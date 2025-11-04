//! Test harness for hook testing.
//!
//! Provides utilities for testing hooks in isolation without a full component tree.

use super::hook_trait::Hook;
use super::hook_context::{HookContext, ComponentId};
use std::marker::PhantomData;

/// Test harness for testing hooks in isolation.
///
/// This allows you to test hook behavior without setting up a full component tree.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::hooks::{SignalHook, HookTestHarness};
///
/// #[test]
/// fn test_my_hook() {
///     let mut harness = HookTestHarness::<SignalHook<i32>>::new();
///
///     // First render
///     let signal = harness.call(0);
///     assert_eq!(signal.get(), 0);
///
///     // Update value
///     signal.set(42);
///
///     // Rerender
///     let signal = harness.rerender(0);
///     assert_eq!(signal.get(), 42);
/// }
/// ```
pub struct HookTestHarness<H: Hook> {
    context: HookContext,
    component_id: ComponentId,
    render_count: usize,
    _phantom: PhantomData<H>,
}

impl<H: Hook> HookTestHarness<H> {
    /// Create a new test harness for a hook.
    pub fn new() -> Self {
        let mut context = HookContext::new();
        let component_id = ComponentId(1);
        context.begin_component(component_id);

        Self {
            context,
            component_id,
            render_count: 0,
            _phantom: PhantomData,
        }
    }

    /// Call the hook with the given input.
    ///
    /// This simulates the first render of a component.
    pub fn call(&mut self, input: H::Input) -> H::Output {
        self.render_count += 1;
        self.context.use_hook::<H>(input)
    }

    /// Rerender the hook with new input.
    ///
    /// This simulates a subsequent render of the same component.
    pub fn rerender(&mut self, input: H::Input) -> H::Output {
        self.render_count += 1;
        self.context.end_component();
        self.context.begin_component(self.component_id);
        self.context.use_hook::<H>(input)
    }

    /// Get the number of times the hook has been rendered.
    pub fn render_count(&self) -> usize {
        self.render_count
    }

    /// Cleanup the component, running any cleanup hooks.
    pub fn cleanup(&mut self) {
        self.context.cleanup_component(self.component_id);
    }

    /// Get a reference to the hook context.
    ///
    /// This allows access to low-level context operations for advanced testing.
    pub fn context(&self) -> &HookContext {
        &self.context
    }

    /// Get a mutable reference to the hook context.
    ///
    /// This allows access to low-level context operations for advanced testing.
    pub fn context_mut(&mut self) -> &mut HookContext {
        &mut self.context
    }
}

impl<H: Hook> Default for HookTestHarness<H> {
    fn default() -> Self {
        Self::new()
    }
}

/// Multi-hook test harness for testing multiple hooks together.
///
/// This allows testing interactions between multiple hooks.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::hooks::{SignalHook, MemoHook, MultiHookTestHarness};
///
/// #[test]
/// fn test_signal_and_memo() {
///     let mut harness = MultiHookTestHarness::new();
///
///     harness.render(|ctx| {
///         let count = ctx.use_hook::<SignalHook<i32>>(0);
///         let doubled = ctx.use_hook::<MemoHook<i32, _>>(move || count.get() * 2);
///
///         assert_eq!(count.get(), 0);
///         assert_eq!(doubled.get(), 0);
///     });
/// }
/// ```
pub struct MultiHookTestHarness {
    context: HookContext,
    component_id: ComponentId,
    render_count: usize,
}

impl MultiHookTestHarness {
    /// Create a new multi-hook test harness.
    pub fn new() -> Self {
        let mut context = HookContext::new();
        let component_id = ComponentId(1);
        context.begin_component(component_id);

        Self {
            context,
            component_id,
            render_count: 0,
        }
    }

    /// Render using the provided function.
    ///
    /// The function receives a mutable reference to the hook context
    /// and can call multiple hooks.
    pub fn render<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut HookContext) -> R,
    {
        self.render_count += 1;
        f(&mut self.context)
    }

    /// Rerender using the provided function.
    ///
    /// This simulates a subsequent render of the same component.
    pub fn rerender<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut HookContext) -> R,
    {
        self.render_count += 1;
        self.context.end_component();
        self.context.begin_component(self.component_id);
        f(&mut self.context)
    }

    /// Get the number of renders.
    pub fn render_count(&self) -> usize {
        self.render_count
    }

    /// Cleanup the component.
    pub fn cleanup(&mut self) {
        self.context.cleanup_component(self.component_id);
    }

    /// Get a reference to the hook context.
    pub fn context(&self) -> &HookContext {
        &self.context
    }

    /// Get a mutable reference to the hook context.
    pub fn context_mut(&mut self) -> &mut HookContext {
        &mut self.context
    }
}

impl Default for MultiHookTestHarness {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::signal::SignalHook;
    use crate::hooks::memo::MemoHook;

    #[test]
    fn test_harness_basic() {
        let mut harness = HookTestHarness::<SignalHook<i32>>::new();

        let signal = harness.call(0);
        assert_eq!(signal.get(), 0);
        assert_eq!(harness.render_count(), 1);

        signal.set(42);

        let signal = harness.rerender(0);
        assert_eq!(signal.get(), 42);
        assert_eq!(harness.render_count(), 2);
    }

    #[test]
    fn test_multi_harness() {
        let mut harness = MultiHookTestHarness::new();

        harness.render(|ctx| {
            let count = ctx.use_hook::<SignalHook<i32>>(0);
            assert_eq!(count.get(), 0);

            count.set(10);
        });

        assert_eq!(harness.render_count(), 1);

        harness.rerender(|ctx| {
            let count = ctx.use_hook::<SignalHook<i32>>(0);
            assert_eq!(count.get(), 10);
        });

        assert_eq!(harness.render_count(), 2);
    }

    #[test]
    fn test_multi_hooks_together() {
        let mut harness = MultiHookTestHarness::new();

        harness.render(|ctx| {
            let count = ctx.use_hook::<SignalHook<i32>>(5);

            let doubled = ctx.use_hook::<MemoHook<i32, _>>(move || {
                count.get() * 2
            });

            assert_eq!(count.get(), 5);
            assert_eq!(doubled.get(), 10);

            count.set(10);
        });

        harness.rerender(|ctx| {
            let count = ctx.use_hook::<SignalHook<i32>>(5);

            let doubled = ctx.use_hook::<MemoHook<i32, _>>(move || {
                count.get() * 2
            });

            assert_eq!(count.get(), 10);
            assert_eq!(doubled.get(), 20);
        });
    }
}
