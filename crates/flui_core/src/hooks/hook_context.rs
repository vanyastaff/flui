//! Hook context with thread-local storage and lifecycle management.

use super::hook_trait::{DependencyId, Hook};
use std::any::{Any, TypeId};
use std::collections::HashMap;

/// Unique identifier for a component instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ComponentId(pub u64);

/// RAII guard that automatically ends component rendering on drop.
///
/// This ensures that `end_component()` is called even if rendering panics,
/// preventing stale component state in the HookContext.
#[derive(Debug)]
pub struct ComponentGuard<'a> {
    context: &'a mut HookContext,
    #[allow(dead_code)]
    component_id: ComponentId,
}

impl<'a> ComponentGuard<'a> {
    /// Begin rendering a component with automatic cleanup.
    ///
    /// Returns a guard that will automatically call `end_component()` when dropped.
    pub fn new(context: &'a mut HookContext, component_id: ComponentId) -> Self {
        context.begin_component(component_id);
        Self {
            context,
            component_id,
        }
    }
}

impl Drop for ComponentGuard<'_> {
    fn drop(&mut self) {
        self.context.end_component();

        #[cfg(debug_assertions)]
        debug_assert_eq!(
            self.context.current_component, None,
            "Component guard dropped but current_component is not None"
        );
    }
}

/// Index of hook call within a component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HookIndex(pub usize);

/// Unique identifier for a hook instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HookId {
    /// Component that owns this hook
    pub component: ComponentId,
    /// Index of this hook within the component
    pub index: HookIndex,
}

/// Type alias for cleanup function (reduces type complexity)
type CleanupFn = Box<dyn FnOnce(Box<dyn Any>) + Send>;

/// Storage for hook state.
///
/// Contains type-erased hook state with explicit cleanup support.
/// Thread-safe: Send + Sync for multi-threaded UI.
pub struct HookState {
    state: Box<dyn Any + Send>,
    type_id: TypeId,
    #[allow(dead_code)] // TODO: Implement update tracking in future
    needs_update: bool,
    /// Explicit cleanup function called on unmount (Send + Sync for thread-safety)
    cleanup_fn: Option<CleanupFn>,
}

impl std::fmt::Debug for HookState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookState")
            .field("type_id", &self.type_id)
            .field("needs_update", &self.needs_update)
            .field("has_cleanup", &self.cleanup_fn.is_some())
            .finish()
    }
}

impl HookState {
    /// Create a new hook state with the given value
    ///
    /// Uses Hook::cleanup() for explicit cleanup on unmount.
    /// Thread-safe: requires H::State to be Send.
    pub fn new<H: Hook>(state: H::State) -> Self
    where
        H::State: Send,
    {
        Self {
            state: Box::new(state),
            type_id: TypeId::of::<H::State>(),
            needs_update: false,
            cleanup_fn: Some(Box::new(|state_any| {
                // Downcast and call Hook::cleanup()
                if let Ok(state) = state_any.downcast::<H::State>() {
                    H::cleanup(*state);
                }
            })),
        }
    }

    /// Get a mutable reference to the hook state if the type matches
    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        if self.type_id == TypeId::of::<T>() {
            self.state.downcast_mut()
        } else {
            None
        }
    }

    /// Explicitly run cleanup for this hook state
    ///
    /// This calls Hook::cleanup() if available, otherwise just drops the state.
    pub fn cleanup(mut self) {
        if let Some(cleanup) = self.cleanup_fn.take() {
            cleanup(self.state);
        }
        // If no cleanup_fn, state drops automatically
    }
}

/// Hook context manages all hook state and lifecycle.
#[derive(Debug)]
pub struct HookContext {
    current_component: Option<ComponentId>,
    current_hook_index: usize,
    hooks: HashMap<HookId, HookState>,
    effect_queue: Vec<HookId>,
    #[allow(dead_code)] // TODO: Implement cleanup in future
    cleanup_queue: Vec<HookId>,
    current_dependencies: Vec<DependencyId>,
    is_tracking: bool,
}

impl HookContext {
    /// Create a new hook context
    pub fn new() -> Self {
        Self {
            current_component: None,
            current_hook_index: 0,
            hooks: HashMap::new(),
            effect_queue: Vec::new(),
            cleanup_queue: Vec::new(),
            current_dependencies: Vec::new(),
            is_tracking: false,
        }
    }

    /// Begin rendering a component, resetting hook index
    ///
    /// # ⚠️ Prefer Using ComponentGuard
    ///
    /// For panic-safe component rendering, use [`ComponentGuard::new()`] instead.
    /// It automatically calls `end_component()` even if rendering panics.
    ///
    /// ```rust,ignore
    /// // ✅ CORRECT: Use guard for automatic cleanup
    /// let _guard = ComponentGuard::new(&mut hook_ctx, component_id);
    /// // ... render component with hooks ...
    /// // Guard automatically calls end_component() on drop
    ///
    /// // ❌ AVOID: Manual begin/end can leak state on panic
    /// hook_ctx.begin_component(component_id);
    /// // ... render component ...
    /// hook_ctx.end_component();  // Never called if panic!
    /// ```
    pub fn begin_component(&mut self, id: ComponentId) {
        self.current_component = Some(id);
        self.current_hook_index = 0;
    }

    /// End component rendering
    ///
    /// # ⚠️ Prefer Using ComponentGuard
    ///
    /// For panic-safe component rendering, use [`ComponentGuard::new()`] instead.
    /// See [`begin_component()`](HookContext::begin_component) for details.
    pub fn end_component(&mut self) {
        self.current_component = None;
        self.current_hook_index = 0;
    }

    fn current_hook_id(&self) -> HookId {
        let component = self.current_component.unwrap_or_else(|| {
            tracing::error!(
                hook_index = self.current_hook_index,
                "Hook called outside component render! Hooks must be called during component rendering.\n\
                 Common causes:\n\
                 1. Hook called in async callback\n\
                 2. Hook called outside component function\n\
                 3. Hook called after component render completed"
            );
            panic!(
                "Hook called outside component render at index {}. \
                 Hooks must only be called during component rendering.",
                self.current_hook_index
            );
        });

        HookId {
            component,
            index: HookIndex(self.current_hook_index),
        }
    }

    /// Use a hook, creating or updating its state
    /// Thread-safe: requires H::State to be Send for multi-threaded UI
    pub fn use_hook<H: Hook>(&mut self, input: H::Input) -> H::Output
    where
        H::State: Send,
    {
        use std::collections::hash_map::Entry;

        let hook_id = self.current_hook_id();
        self.current_hook_index += 1;

        match self.hooks.entry(hook_id) {
            Entry::Occupied(mut entry) => {
                // Hook already exists, update it
                let hook_state = entry.get_mut().get_mut::<H::State>().unwrap_or_else(|| {
                    tracing::error!(
                        component_id = ?hook_id.component,
                        hook_index = hook_id.index.0,
                        expected_type = std::any::type_name::<H::State>(),
                        "Hook state type mismatch! This is a CRITICAL bug.\n\
                         Common causes:\n\
                         1. Hook calls are conditional (if/else with different hooks)\n\
                         2. Hook calls are reordered between renders\n\
                         3. Different hook type used at same index\n\
                         4. Loop with variable number of hook calls\n\
                         \n\
                         Rules of Hooks:\n\
                         - Always call hooks in the same order\n\
                         - Never call hooks conditionally\n\
                         - Never call hooks in loops with variable iterations\n\
                         \n\
                         See hooks/RULES.md for detailed explanation and examples."
                    );
                    panic!(
                        "\n\
                            ╔════════════════════════════════════════════════════════════════╗\n\
                            ║          HOOK ORDERING VIOLATION DETECTED                      ║\n\
                            ╚════════════════════════════════════════════════════════════════╝\n\
                            \n\
                            Hook state type mismatch at:\n\
                            • Component: {:?}\n\
                            • Hook index: {}\n\
                            • Expected type: {}\n\
                            \n\
                            WHY THIS HAPPENED:\n\
                            You're calling hooks in a different order between renders.\n\
                            The hook system identifies hooks by their position (0, 1, 2...),\n\
                            so changing the order breaks the state tracking.\n\
                            \n\
                            COMMON CAUSES:\n\
                            ❌ Conditional hooks:     if x {{ use_signal(...) }}\n\
                            ❌ Early returns:         if !ready {{ return }}; use_signal(...)\n\
                            ❌ Variable loops:        for item in list {{ use_signal(...) }}\n\
                            ❌ Different code paths:  match {{ A => hook1(), B => hook2() }}\n\
                            \n\
                            HOW TO FIX:\n\
                            ✅ Call ALL hooks at the TOP LEVEL of your component's build() method\n\
                            ✅ Make sure EVERY render calls the SAME hooks in the SAME order\n\
                            ✅ Make VALUES conditional, not hook CALLS:\n\
                               let x = use_signal(ctx, 0);\n\
                               if condition {{ x.set(10); }}  // ← Correct\n\
                            \n\
                            📚 For detailed rules and examples, see:\n\
                            crates/flui_core/src/hooks/RULES.md\n\
                            \n\
                            ",
                        hook_id.component,
                        hook_id.index.0,
                        std::any::type_name::<H::State>()
                    );
                });
                H::update(hook_state, input)
            }
            Entry::Vacant(entry) => {
                // First call, create state then update
                let initial_state = H::create(input.clone());
                entry.insert(HookState::new::<H>(initial_state));

                let hook_state = self
                    .hooks
                    .get_mut(&hook_id)
                    .expect("BUG: Hook just inserted but not found")
                    .get_mut::<H::State>()
                    .expect("BUG: Hook state type mismatch on fresh insert");

                H::update(hook_state, input)
            }
        }
    }

    /// Track a dependency during reactive tracking
    pub fn track_dependency(&mut self, dep: DependencyId) {
        if self.is_tracking {
            self.current_dependencies.push(dep);
        }
    }

    /// Start tracking dependencies
    pub fn start_tracking(&mut self) {
        self.is_tracking = true;
        self.current_dependencies.clear();
    }

    /// End tracking and return collected dependencies
    pub fn end_tracking(&mut self) -> Vec<DependencyId> {
        self.is_tracking = false;
        std::mem::take(&mut self.current_dependencies)
    }

    /// Schedule an effect to run after rendering
    pub fn schedule_effect(&mut self, hook_id: HookId) {
        if !self.effect_queue.contains(&hook_id) {
            self.effect_queue.push(hook_id);
        }
    }

    /// Flush all pending effects
    pub fn flush_effects(&mut self) {
        for _hook_id in std::mem::take(&mut self.effect_queue) {
            // TODO(2025-03): Run pending effects
        }
    }

    /// Clean up all hooks for a component
    ///
    /// This method ensures proper cleanup by:
    /// 1. Identifying all hooks belonging to the component
    /// 2. **Explicitly calling Hook::cleanup()** for each hook
    /// 3. Removing the hooks from the map
    ///
    /// # Explicit Cleanup Guarantee
    ///
    /// Unlike the previous implementation that relied solely on Drop,
    /// this version **explicitly invokes Hook::cleanup()** for every hook.
    /// This ensures:
    /// - Effect cleanup functions are always called
    /// - Resources are freed in a controlled manner
    /// - Cleanup happens even if Drop is bypassed
    /// - Better guarantees for async cleanup (when implemented)
    ///
    /// # Memory Safety
    ///
    /// After Hook::cleanup() is called, the hook state is removed from the map.
    /// Drop implementations still run as a fallback, but explicit cleanup
    /// takes precedence.
    pub fn cleanup_component(&mut self, component_id: ComponentId) {
        // Collect hooks to clean up
        let hooks_to_cleanup: Vec<_> = self
            .hooks
            .iter()
            .filter(|(id, _)| id.component == component_id)
            .map(|(id, _)| *id)
            .collect();

        #[cfg(debug_assertions)]
        if !hooks_to_cleanup.is_empty() {
            tracing::debug!(
                "Cleaning up {} hooks for component {:?}",
                hooks_to_cleanup.len(),
                component_id
            );
        }

        // Explicitly cleanup each hook before removal
        for hook_id in hooks_to_cleanup {
            if let Some(hook_state) = self.hooks.remove(&hook_id) {
                // Explicitly call Hook::cleanup() via HookState::cleanup()
                hook_state.cleanup();
            }
        }
    }
}

impl Default for HookContext {
    fn default() -> Self {
        Self::new()
    }
}

// REMOVED: Thread-local global state (Issue #17)
//
// Previously, HookContext was stored in thread-local storage, which:
// - Prevented running hook tests in parallel
// - Made it impossible to have multiple independent UI trees
// - Made debugging harder due to hidden global state
// - Violated dependency injection principles
//
// Now, HookContext is passed explicitly through the API, providing:
// ✅ Explicit dependencies (no magic globals)
// ✅ Isolated test contexts
// ✅ Multiple independent apps
// ✅ Clear ownership and lifecycle
//
// Migration: Replace `with_hook_context(|ctx| ...)` with explicit context parameters.
// See HOOK_REFACTORING.md for full migration guide.
