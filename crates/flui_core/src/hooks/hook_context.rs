//! Hook context with thread-local storage and lifecycle management.

use super::hook_trait::{Hook, DependencyId};
use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;

/// Unique identifier for a component instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ComponentId(pub u64);

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

/// Storage for hook state.
#[derive(Debug)]
pub struct HookState {
    state: Box<dyn Any>,
    type_id: TypeId,
    #[allow(dead_code)] // TODO: Implement update tracking in future
    needs_update: bool,
}

impl HookState {
    /// Create a new hook state with the given value
    pub fn new<T: 'static>(state: T) -> Self {
        Self {
            state: Box::new(state),
            type_id: TypeId::of::<T>(),
            needs_update: false,
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
    pub fn begin_component(&mut self, id: ComponentId) {
        self.current_component = Some(id);
        self.current_hook_index = 0;
    }

    /// End component rendering
    pub fn end_component(&mut self) {
        self.current_component = None;
        self.current_hook_index = 0;
    }

    fn current_hook_id(&self) -> HookId {
        HookId {
            component: self.current_component.expect("No active component"),
            index: HookIndex(self.current_hook_index),
        }
    }

    /// Use a hook, creating or updating its state
    pub fn use_hook<H: Hook>(&mut self, input: H::Input) -> H::Output {
        use std::collections::hash_map::Entry;

        let hook_id = self.current_hook_id();
        self.current_hook_index += 1;

        match self.hooks.entry(hook_id) {
            Entry::Occupied(mut entry) => {
                // Hook already exists, update it
                let hook_state = entry.get_mut().get_mut::<H::State>()
                    .expect("Hook state type mismatch");
                H::update(hook_state, input)
            }
            Entry::Vacant(entry) => {
                // First call, create state then update
                let initial_state = H::create(input.clone());
                entry.insert(HookState::new(initial_state));

                let hook_state = self.hooks.get_mut(&hook_id).unwrap()
                    .get_mut::<H::State>().unwrap();

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
    pub fn cleanup_component(&mut self, component_id: ComponentId) {
        self.hooks.retain(|id, _state| {
            id.component != component_id
        });
    }
}

impl Default for HookContext {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    static HOOK_CONTEXT: RefCell<HookContext> = RefCell::new(HookContext::new());
}

/// Access the thread-local hook context
///
/// Provides mutable access to the hook context for the current thread.
/// Used internally by hook implementations to manage state.
pub fn with_hook_context<F, R>(f: F) -> R
where
    F: FnOnce(&mut HookContext) -> R,
{
    HOOK_CONTEXT.with(|ctx| f(&mut ctx.borrow_mut()))
}
