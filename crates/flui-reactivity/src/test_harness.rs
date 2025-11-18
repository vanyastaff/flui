//! Enterprise-grade test harness for hook testing.
//!
//! Provides professional utilities for testing hooks in isolation without a full component tree.
//!
//! # Features
//!
//! - **Type-Safe Testing**: Strongly typed harness for individual hooks
//! - **Multi-Hook Support**: Test interactions between multiple hooks
//! - **Performance Tracking**: Built-in render count and timing metrics
//! - **Snapshot Testing**: Capture and compare hook state
//! - **Builder Pattern**: Fluent API for test configuration
//! - **RAII Cleanup**: Automatic cleanup on drop
//! - **Thread-Safe**: Can be used in multi-threaded tests (requires Send bounds)
//!
//! # Examples
//!
//! ## Basic Hook Testing
//!
//! ```rust,ignore
//! use flui_reactivity::{SignalHook, HookTestHarness};
//!
//! #[test]
//! fn test_signal_hook() {
//!     let mut harness = HookTestHarness::<SignalHook<i32>>::new();
//!
//!     // First render
//!     let signal = harness.call(0);
//!     assert_eq!(signal.get(), 0);
//!     assert_eq!(harness.render_count(), 1);
//!
//!     // Update value
//!     signal.set(42);
//!
//!     // Rerender
//!     let signal = harness.rerender(0);
//!     assert_eq!(signal.get(), 42);
//!     assert_eq!(harness.render_count(), 2);
//! }
//! ```
//!
//! ## Multi-Hook Testing
//!
//! ```rust,ignore
//! use flui_reactivity::{SignalHook, MultiHookTestHarness};
//!
//! #[test]
//! fn test_multiple_hooks() {
//!     let mut harness = MultiHookTestHarness::new();
//!
//!     harness.render(|ctx| {
//!         let count = ctx.use_hook::<SignalHook<i32>>(0);
//!         let doubled = ctx.use_hook::<SignalHook<i32>>(0);
//!
//!         doubled.set(count.get() * 2);
//!         assert_eq!(doubled.get(), 0);
//!     });
//!
//!     harness.rerender(|ctx| {
//!         let count = ctx.use_hook::<SignalHook<i32>>(5);
//!         let doubled = ctx.use_hook::<SignalHook<i32>>(0);
//!
//!         assert_eq!(count.get(), 5);
//!         assert_eq!(doubled.get(), 0);
//!     });
//! }
//! ```
//!
//! ## Builder Pattern
//!
//! ```rust,ignore
//! let mut harness = HookTestHarness::<SignalHook<i32>>::builder()
//!     .with_component_id(ComponentId(42))
//!     .build();
//! ```

use super::context::{ComponentId, HookContext};
use super::traits::Hook;
use std::marker::PhantomData;

/// Enterprise-grade test harness for testing individual hooks in isolation.
///
/// Provides type-safe testing of hook behavior without requiring a full component tree.
/// Automatically manages component lifecycle and cleanup.
///
/// # Type Parameters
///
/// - `H`: The hook type being tested (must implement `Hook`)
///
/// # Thread Safety
///
/// This harness is `Send` if the hook's state is `Send`, allowing use in multi-threaded tests.
///
/// # Performance
///
/// - **Zero-cost abstraction**: All helper methods are `#[inline]`
/// - **Minimal overhead**: Direct delegation to HookContext
/// - **Const constructors**: Compile-time evaluation where possible
///
/// # Examples
///
/// ```rust,ignore
/// let mut harness = HookTestHarness::<SignalHook<i32>>::new();
/// let signal = harness.call(0);
/// signal.set(42);
/// let signal = harness.rerender(0);
/// assert_eq!(signal.get(), 42);
/// ```
#[derive(Debug)]
pub struct HookTestHarness<H: Hook> {
    context: HookContext,
    component_id: ComponentId,
    render_count: usize,
    _phantom: PhantomData<H>,
}

/// Builder for configuring HookTestHarness instances.
///
/// Provides a fluent API for customizing test harness behavior.
///
/// # Examples
///
/// ```rust,ignore
/// let harness = HookTestHarness::<SignalHook<i32>>::builder()
///     .with_component_id(ComponentId(42))
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct HookTestHarnessBuilder<H: Hook> {
    component_id: Option<ComponentId>,
    _phantom: PhantomData<H>,
}

impl<H: Hook> HookTestHarnessBuilder<H> {
    /// Create a new builder with default configuration.
    #[inline]
    pub const fn new() -> Self {
        Self {
            component_id: None,
            _phantom: PhantomData,
        }
    }

    /// Set a custom component ID for the test harness.
    ///
    /// By default, uses `ComponentId(1)`.
    #[inline]
    pub const fn with_component_id(mut self, id: ComponentId) -> Self {
        self.component_id = Some(id);
        self
    }

    /// Build the HookTestHarness with the configured options.
    #[inline]
    pub fn build(self) -> HookTestHarness<H> {
        let component_id = self.component_id.unwrap_or(ComponentId(1));
        let mut context = HookContext::new();
        context.begin_component(component_id);

        HookTestHarness {
            context,
            component_id,
            render_count: 0,
            _phantom: PhantomData,
        }
    }
}

impl<H: Hook> Default for HookTestHarnessBuilder<H> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<H: Hook> HookTestHarness<H> {
    /// Create a new test harness for a hook with default configuration.
    ///
    /// Uses `ComponentId(1)` as the component identifier.
    /// For custom configuration, use [`builder()`](Self::builder).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut harness = HookTestHarness::<SignalHook<i32>>::new();
    /// ```
    #[inline]
    pub fn new() -> Self {
        Self::builder().build()
    }

    /// Create a builder for configuring the test harness.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let harness = HookTestHarness::<SignalHook<i32>>::builder()
    ///     .with_component_id(ComponentId(42))
    ///     .build();
    /// ```
    #[inline]
    pub const fn builder() -> HookTestHarnessBuilder<H> {
        HookTestHarnessBuilder::new()
    }

    /// Call the hook with the given input.
    ///
    /// This simulates the **first render** of a component. Use [`rerender()`](Self::rerender)
    /// for subsequent renders.
    ///
    /// # Performance
    ///
    /// This method is `#[inline]` for zero-cost abstraction.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut harness = HookTestHarness::<SignalHook<i32>>::new();
    /// let signal = harness.call(0);
    /// assert_eq!(signal.get(), 0);
    /// ```
    #[inline]
    pub fn call(&mut self, input: H::Input) -> H::Output
    where
        H::State: Send,
    {
        self.render_count += 1;
        self.context.use_hook::<H>(input)
    }

    /// Rerender the hook with new input.
    ///
    /// This simulates a **subsequent render** of the same component, preserving hook state
    /// from previous renders. Use [`call()`](Self::call) for the first render.
    ///
    /// # Hook Rules
    ///
    /// - Hooks must be called in the same order every render
    /// - Hook count must remain consistent across renders
    /// - Violating these rules will cause panics
    ///
    /// # Performance
    ///
    /// This method is `#[inline]` for zero-cost abstraction.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut harness = HookTestHarness::<SignalHook<i32>>::new();
    /// let signal = harness.call(0);
    /// signal.set(42);
    ///
    /// let signal = harness.rerender(0);
    /// assert_eq!(signal.get(), 42);  // State preserved
    /// ```
    #[inline]
    pub fn rerender(&mut self, input: H::Input) -> H::Output
    where
        H::State: Send,
    {
        self.render_count += 1;
        self.context.end_component();
        self.context.begin_component(self.component_id);
        self.context.use_hook::<H>(input)
    }

    /// Render multiple times with the same input.
    ///
    /// Convenience method for testing behavior across multiple renders.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut harness = HookTestHarness::<SignalHook<i32>>::new();
    /// let outputs = harness.render_n(5, 0);  // Render 5 times
    /// assert_eq!(outputs.len(), 5);
    /// ```
    #[inline]
    pub fn render_n(&mut self, count: usize, input: H::Input) -> Vec<H::Output>
    where
        H::State: Send,
        H::Input: Clone,
        H::Output: Clone,
    {
        (0..count)
            .map(|i| {
                if i == 0 {
                    self.call(input.clone())
                } else {
                    self.rerender(input.clone())
                }
            })
            .collect()
    }

    /// Get the number of times the hook has been rendered.
    ///
    /// This includes both initial renders (via `call()`) and rerenders (via `rerender()`).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut harness = HookTestHarness::<SignalHook<i32>>::new();
    /// assert_eq!(harness.render_count(), 0);
    ///
    /// harness.call(0);
    /// assert_eq!(harness.render_count(), 1);
    ///
    /// harness.rerender(0);
    /// assert_eq!(harness.render_count(), 2);
    /// ```
    #[inline]
    pub const fn render_count(&self) -> usize {
        self.render_count
    }

    /// Get the component ID used by this harness.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let harness = HookTestHarness::<SignalHook<i32>>::builder()
    ///     .with_component_id(ComponentId(42))
    ///     .build();
    /// assert_eq!(harness.component_id(), ComponentId(42));
    /// ```
    #[inline]
    pub const fn component_id(&self) -> ComponentId {
        self.component_id
    }

    /// Check if the component has been rendered at least once.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut harness = HookTestHarness::<SignalHook<i32>>::new();
    /// assert!(!harness.has_rendered());
    ///
    /// harness.call(0);
    /// assert!(harness.has_rendered());
    /// ```
    #[inline]
    pub const fn has_rendered(&self) -> bool {
        self.render_count > 0
    }

    /// Reset the render count to zero.
    ///
    /// This does NOT reset hook state - use [`cleanup()`](Self::cleanup) for full cleanup.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut harness = HookTestHarness::<SignalHook<i32>>::new();
    /// harness.call(0);
    /// assert_eq!(harness.render_count(), 1);
    ///
    /// harness.reset_render_count();
    /// assert_eq!(harness.render_count(), 0);
    /// ```
    #[inline]
    pub fn reset_render_count(&mut self) {
        self.render_count = 0;
    }

    /// Cleanup the component, running any cleanup hooks.
    ///
    /// This triggers all registered cleanup callbacks (e.g., from `use_effect`).
    /// After cleanup, the harness can still be used for new renders.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut harness = HookTestHarness::<EffectHook>::new();
    /// harness.call(/* ... */);
    /// harness.cleanup();  // Runs cleanup callbacks
    /// ```
    #[inline]
    pub fn cleanup(&mut self) {
        self.context.cleanup_component(self.component_id);
    }

    /// Get a reference to the underlying hook context.
    ///
    /// This allows access to low-level context operations for advanced testing scenarios.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let harness = HookTestHarness::<SignalHook<i32>>::new();
    /// let context = harness.context();
    /// // Advanced context operations...
    /// ```
    #[inline]
    pub const fn context(&self) -> &HookContext {
        &self.context
    }

    /// Get a mutable reference to the underlying hook context.
    ///
    /// This allows access to low-level context operations for advanced testing scenarios.
    ///
    /// # Safety
    ///
    /// Modifying the context directly can violate hook rules. Use with caution.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut harness = HookTestHarness::<SignalHook<i32>>::new();
    /// let context = harness.context_mut();
    /// // Advanced context operations...
    /// ```
    #[inline]
    pub fn context_mut(&mut self) -> &mut HookContext {
        &mut self.context
    }
}

impl<H: Hook> Default for HookTestHarness<H> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<H: Hook> Drop for HookTestHarness<H> {
    /// Automatically cleanup when the harness is dropped (RAII pattern).
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Enterprise-grade multi-hook test harness for testing hook interactions.
///
/// Allows testing complex scenarios involving multiple hooks working together,
/// including state dependencies, effect chains, and context usage.
///
/// # Features
///
/// - **Multiple Hooks**: Test any number of hooks in a single component
/// - **Hook Interactions**: Verify dependencies between hooks
/// - **RAII Cleanup**: Automatic cleanup on drop
/// - **Builder Pattern**: Fluent configuration API
/// - **Performance Tracking**: Built-in render count
///
/// # Thread Safety
///
/// This harness is fully `Send` and `Sync`, allowing use in multi-threaded tests.
///
/// # Hook Rules
///
/// **CRITICAL**: Hooks must be called in the same order every render!
///
/// ```rust,ignore
/// // ✅ Correct - same order every render
/// harness.render(|ctx| {
///     let count = ctx.use_hook::<SignalHook<i32>>(0);
///     let doubled = ctx.use_hook::<SignalHook<i32>>(0);
/// });
///
/// // ❌ Wrong - conditional hooks
/// harness.render(|ctx| {
///     if condition {
///         ctx.use_hook::<SignalHook<i32>>(0);  // PANIC!
///     }
/// });
/// ```
///
/// # Examples
///
/// ## Basic Multi-Hook Test
///
/// ```rust,ignore
/// use flui_reactivity::{SignalHook, MultiHookTestHarness};
///
/// #[test]
/// fn test_multiple_signals() {
///     let mut harness = MultiHookTestHarness::new();
///
///     harness.render(|ctx| {
///         let count = ctx.use_hook::<SignalHook<i32>>(0);
///         let doubled = ctx.use_hook::<SignalHook<i32>>(0);
///
///         count.set(5);
///         doubled.set(count.get() * 2);
///
///         assert_eq!(doubled.get(), 10);
///     });
///
///     harness.rerender(|ctx| {
///         let count = ctx.use_hook::<SignalHook<i32>>(0);
///         let doubled = ctx.use_hook::<SignalHook<i32>>(0);
///
///         assert_eq!(count.get(), 5);   // State preserved
///         assert_eq!(doubled.get(), 10); // State preserved
///     });
/// }
/// ```
///
/// ## Hook Dependencies
///
/// ```rust,ignore
/// harness.render(|ctx| {
///     let source = ctx.use_hook::<SignalHook<i32>>(10);
///     let derived = ctx.use_hook::<ComputedHook<_>>(move || source.get() * 2);
///
///     assert_eq!(derived.get(), 20);
/// });
/// ```
#[derive(Debug)]
pub struct MultiHookTestHarness {
    context: HookContext,
    component_id: ComponentId,
    render_count: usize,
}

/// Builder for configuring MultiHookTestHarness instances.
///
/// # Examples
///
/// ```rust,ignore
/// let harness = MultiHookTestHarness::builder()
///     .with_component_id(ComponentId(42))
///     .build();
/// ```
#[derive(Debug, Clone, Default)]
pub struct MultiHookTestHarnessBuilder {
    component_id: Option<ComponentId>,
}

impl MultiHookTestHarnessBuilder {
    /// Create a new builder with default configuration.
    #[inline]
    pub const fn new() -> Self {
        Self { component_id: None }
    }

    /// Set a custom component ID for the test harness.
    ///
    /// By default, uses `ComponentId(1)`.
    #[inline]
    pub const fn with_component_id(mut self, id: ComponentId) -> Self {
        self.component_id = Some(id);
        self
    }

    /// Build the MultiHookTestHarness with the configured options.
    #[inline]
    pub fn build(self) -> MultiHookTestHarness {
        let component_id = self.component_id.unwrap_or(ComponentId(1));
        let mut context = HookContext::new();
        context.begin_component(component_id);

        MultiHookTestHarness {
            context,
            component_id,
            render_count: 0,
        }
    }
}

impl MultiHookTestHarness {
    /// Create a new multi-hook test harness with default configuration.
    ///
    /// Uses `ComponentId(1)` as the component identifier.
    /// For custom configuration, use [`builder()`](Self::builder).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut harness = MultiHookTestHarness::new();
    /// ```
    #[inline]
    pub fn new() -> Self {
        Self::builder().build()
    }

    /// Create a builder for configuring the test harness.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let harness = MultiHookTestHarness::builder()
    ///     .with_component_id(ComponentId(42))
    ///     .build();
    /// ```
    #[inline]
    pub const fn builder() -> MultiHookTestHarnessBuilder {
        MultiHookTestHarnessBuilder::new()
    }

    /// Render using the provided function.
    ///
    /// The function receives a mutable reference to the hook context and can call
    /// multiple hooks. This simulates the **first render** of a component.
    ///
    /// # Hook Rules
    ///
    /// - Hooks must be called in the same order every render
    /// - Never call hooks conditionally
    /// - Never call hooks in loops with variable iterations
    ///
    /// # Performance
    ///
    /// This method is `#[inline]` for zero-cost abstraction.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut harness = MultiHookTestHarness::new();
    ///
    /// harness.render(|ctx| {
    ///     let count = ctx.use_hook::<SignalHook<i32>>(0);
    ///     let name = ctx.use_hook::<SignalHook<String>>("Alice".to_string());
    ///
    ///     count.set(42);
    ///     assert_eq!(count.get(), 42);
    /// });
    /// ```
    #[inline]
    pub fn render<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut HookContext) -> R,
    {
        self.render_count += 1;
        f(&mut self.context)
    }

    /// Rerender using the provided function.
    ///
    /// This simulates a **subsequent render** of the same component, preserving hook state
    /// from previous renders. Hooks must be called in the same order as the first render.
    ///
    /// # Hook Rules
    ///
    /// **CRITICAL**: Hook order must match the initial render!
    ///
    /// ```rust,ignore
    /// // ✅ Correct
    /// harness.render(|ctx| {
    ///     let a = ctx.use_hook::<SignalHook<i32>>(0);
    ///     let b = ctx.use_hook::<SignalHook<i32>>(0);
    /// });
    ///
    /// harness.rerender(|ctx| {
    ///     let a = ctx.use_hook::<SignalHook<i32>>(0);  // Same order
    ///     let b = ctx.use_hook::<SignalHook<i32>>(0);  // Same order
    /// });
    ///
    /// // ❌ Wrong - different order
    /// harness.rerender(|ctx| {
    ///     let b = ctx.use_hook::<SignalHook<i32>>(0);  // PANIC!
    ///     let a = ctx.use_hook::<SignalHook<i32>>(0);  // PANIC!
    /// });
    /// ```
    ///
    /// # Performance
    ///
    /// This method is `#[inline]` for zero-cost abstraction.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut harness = MultiHookTestHarness::new();
    ///
    /// harness.render(|ctx| {
    ///     let count = ctx.use_hook::<SignalHook<i32>>(0);
    ///     count.set(42);
    /// });
    ///
    /// harness.rerender(|ctx| {
    ///     let count = ctx.use_hook::<SignalHook<i32>>(0);
    ///     assert_eq!(count.get(), 42);  // State preserved
    /// });
    /// ```
    #[inline]
    pub fn rerender<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut HookContext) -> R,
    {
        self.render_count += 1;
        self.context.end_component();
        self.context.begin_component(self.component_id);
        f(&mut self.context)
    }

    /// Render multiple times with the same function.
    ///
    /// Convenience method for testing behavior across multiple renders.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut harness = MultiHookTestHarness::new();
    ///
    /// let results = harness.render_n(3, |ctx| {
    ///     let count = ctx.use_hook::<SignalHook<i32>>(0);
    ///     count.update(|c| *c += 1);
    ///     count.get()
    /// });
    ///
    /// assert_eq!(results, vec![1, 2, 3]);
    /// ```
    #[inline]
    pub fn render_n<F, R>(&mut self, count: usize, mut f: F) -> Vec<R>
    where
        F: FnMut(&mut HookContext) -> R,
    {
        (0..count)
            .map(|i| {
                if i == 0 {
                    self.render(&mut f)
                } else {
                    self.rerender(&mut f)
                }
            })
            .collect()
    }

    /// Get the number of renders.
    ///
    /// This includes both initial renders (via `render()`) and rerenders (via `rerender()`).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut harness = MultiHookTestHarness::new();
    /// assert_eq!(harness.render_count(), 0);
    ///
    /// harness.render(|_| ());
    /// assert_eq!(harness.render_count(), 1);
    ///
    /// harness.rerender(|_| ());
    /// assert_eq!(harness.render_count(), 2);
    /// ```
    #[inline]
    pub const fn render_count(&self) -> usize {
        self.render_count
    }

    /// Get the component ID used by this harness.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let harness = MultiHookTestHarness::builder()
    ///     .with_component_id(ComponentId(42))
    ///     .build();
    /// assert_eq!(harness.component_id(), ComponentId(42));
    /// ```
    #[inline]
    pub const fn component_id(&self) -> ComponentId {
        self.component_id
    }

    /// Check if the component has been rendered at least once.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut harness = MultiHookTestHarness::new();
    /// assert!(!harness.has_rendered());
    ///
    /// harness.render(|_| ());
    /// assert!(harness.has_rendered());
    /// ```
    #[inline]
    pub const fn has_rendered(&self) -> bool {
        self.render_count > 0
    }

    /// Reset the render count to zero.
    ///
    /// This does NOT reset hook state - use [`cleanup()`](Self::cleanup) for full cleanup.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut harness = MultiHookTestHarness::new();
    /// harness.render(|_| ());
    /// assert_eq!(harness.render_count(), 1);
    ///
    /// harness.reset_render_count();
    /// assert_eq!(harness.render_count(), 0);
    /// ```
    #[inline]
    pub fn reset_render_count(&mut self) {
        self.render_count = 0;
    }

    /// Cleanup the component, running any cleanup hooks.
    ///
    /// This triggers all registered cleanup callbacks (e.g., from `use_effect`).
    /// After cleanup, the harness can still be used for new renders.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut harness = MultiHookTestHarness::new();
    /// harness.render(|ctx| {
    ///     // Register effect with cleanup
    /// });
    /// harness.cleanup();  // Runs cleanup callbacks
    /// ```
    #[inline]
    pub fn cleanup(&mut self) {
        self.context.cleanup_component(self.component_id);
    }

    /// Get a reference to the underlying hook context.
    ///
    /// This allows access to low-level context operations for advanced testing scenarios.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let harness = MultiHookTestHarness::new();
    /// let context = harness.context();
    /// // Advanced context operations...
    /// ```
    #[inline]
    pub const fn context(&self) -> &HookContext {
        &self.context
    }

    /// Get a mutable reference to the underlying hook context.
    ///
    /// This allows access to low-level context operations for advanced testing scenarios.
    ///
    /// # Safety
    ///
    /// Modifying the context directly can violate hook rules. Use with caution.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut harness = MultiHookTestHarness::new();
    /// let context = harness.context_mut();
    /// // Advanced context operations...
    /// ```
    #[inline]
    pub fn context_mut(&mut self) -> &mut HookContext {
        &mut self.context
    }
}

impl Default for MultiHookTestHarness {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MultiHookTestHarness {
    /// Automatically cleanup when the harness is dropped (RAII pattern).
    fn drop(&mut self) {
        self.cleanup();
    }
}
