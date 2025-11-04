//! Reactive scopes for automatic dependency tracking
//!
//! Scopes track which signals are accessed during execution,
//! enabling automatic reactivity.

use std::cell::RefCell;
use std::collections::HashSet;
use crate::signal::SignalId;

/// Unique identifier for a reactive scope
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(usize);

impl ScopeId {
    pub const fn new(id: usize) -> Self {
        Self(id)
    }

    pub const fn as_usize(self) -> usize {
        self.0
    }
}

/// A reactive scope that tracks signal dependencies
///
/// When signals are read (`.get()`) within a scope, they are automatically
/// registered as dependencies. When those signals change, the scope's
/// callback is executed.
pub struct ReactiveScope {
    id: ScopeId,
    /// Signals that this scope depends on
    dependencies: HashSet<SignalId>,
    /// Callback to execute when dependencies change
    callback: Option<Box<dyn FnMut()>>,
}

impl ReactiveScope {
    /// Create a new reactive scope
    pub fn new(id: ScopeId) -> Self {
        Self {
            id,
            dependencies: HashSet::new(),
            callback: None,
        }
    }

    /// Get the scope's ID
    pub const fn id(&self) -> ScopeId {
        self.id
    }

    /// Track a signal as a dependency
    pub fn track_signal(&mut self, signal_id: SignalId) {
        self.dependencies.insert(signal_id);
    }

    /// Get all dependencies
    pub fn dependencies(&self) -> &HashSet<SignalId> {
        &self.dependencies
    }

    /// Clear all dependencies
    pub fn clear_dependencies(&mut self) {
        self.dependencies.clear();
    }

    /// Set the callback for this scope
    pub fn set_callback<F>(&mut self, callback: F)
    where
        F: FnMut() + 'static,
    {
        self.callback = Some(Box::new(callback));
    }

    /// Execute the callback if one is set
    pub fn execute(&mut self) {
        if let Some(callback) = &mut self.callback {
            callback();
        }
    }
}

impl std::fmt::Debug for ReactiveScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReactiveScope")
            .field("id", &self.id)
            .field("dependencies", &self.dependencies)
            .field("has_callback", &self.callback.is_some())
            .finish()
    }
}

/// Scope stack for nested reactive contexts
struct ScopeStack {
    scopes: Vec<ReactiveScope>,
    next_id: usize,
}

impl ScopeStack {
    fn new() -> Self {
        Self {
            scopes: Vec::new(),
            next_id: 0,
        }
    }

    /// Push a new scope onto the stack
    fn push(&mut self) -> ScopeId {
        let id = ScopeId::new(self.next_id);
        self.next_id += 1;
        self.scopes.push(ReactiveScope::new(id));
        id
    }

    /// Pop the current scope
    fn pop(&mut self) -> Option<ReactiveScope> {
        self.scopes.pop()
    }

    /// Get the current (top) scope
    fn current(&mut self) -> Option<&mut ReactiveScope> {
        self.scopes.last_mut()
    }

    /// Check if there's an active scope
    fn has_scope(&self) -> bool {
        !self.scopes.is_empty()
    }
}

impl Default for ScopeStack {
    fn default() -> Self {
        Self::new()
    }
}

// Thread-local scope stack
thread_local! {
    static SCOPE_STACK: RefCell<ScopeStack> = RefCell::new(ScopeStack::new());
}

/// Create a new reactive scope and execute a function within it
///
/// This automatically tracks all signal accesses within the function.
///
/// # Example
///
/// ```rust,ignore
/// let count = Signal::new(0);
/// let double = create_scope(|| {
///     count.get() * 2  // count is automatically tracked
/// });
/// ```
pub fn create_scope<R>(f: impl FnOnce() -> R) -> (ScopeId, R, HashSet<SignalId>) {
    // Push new scope
    let scope_id = SCOPE_STACK.with(|stack| stack.borrow_mut().push());

    // Execute function (signals will register dependencies)
    let result = f();

    // Pop scope and get dependencies
    let scope = SCOPE_STACK
        .with(|stack| stack.borrow_mut().pop())
        .expect("Scope stack underflow");

    let dependencies = scope.dependencies().clone();

    (scope_id, result, dependencies)
}

/// Execute a function with access to the current scope (if any)
pub fn with_scope<R>(f: impl FnOnce(Option<&mut ReactiveScope>) -> R) -> R {
    SCOPE_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        let scope = stack.current();
        f(scope)
    })
}

/// Check if there's a currently active scope
pub fn has_active_scope() -> bool {
    SCOPE_STACK.with(|stack| stack.borrow().has_scope())
}

/// Clear all scopes (useful for cleanup)
pub fn clear_scopes() {
    SCOPE_STACK.with(|stack| {
        *stack.borrow_mut() = ScopeStack::new();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_tracking() {
        let mut scope = ReactiveScope::new(ScopeId::new(0));
        assert_eq!(scope.dependencies().len(), 0);

        scope.track_signal(SignalId::new(1));
        scope.track_signal(SignalId::new(2));
        assert_eq!(scope.dependencies().len(), 2);

        scope.clear_dependencies();
        assert_eq!(scope.dependencies().len(), 0);
    }

    #[test]
    fn test_create_scope() {
        clear_scopes();

        let (scope_id, result, deps) = create_scope(|| {
            // Manually track some signals
            with_scope(|scope| {
                if let Some(scope) = scope {
                    scope.track_signal(SignalId::new(1));
                    scope.track_signal(SignalId::new(2));
                }
            });
            42
        });

        assert_eq!(result, 42);
        assert_eq!(deps.len(), 2);
        assert!(deps.contains(&SignalId::new(1)));
        assert!(deps.contains(&SignalId::new(2)));
    }

    #[test]
    fn test_nested_scopes() {
        clear_scopes();

        create_scope(|| {
            with_scope(|scope| {
                if let Some(scope) = scope {
                    scope.track_signal(SignalId::new(1));
                }
            });

            create_scope(|| {
                with_scope(|scope| {
                    if let Some(scope) = scope {
                        scope.track_signal(SignalId::new(2));
                    }
                });
            });

            // Outer scope should still be active
            assert!(has_active_scope());
        });

        // All scopes should be popped
        assert!(!has_active_scope());
    }
}
