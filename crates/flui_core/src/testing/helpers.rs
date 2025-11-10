//! Test helper utilities
//!
//! Common utilities for writing tests

use crate::hooks::hook_context::{ComponentId, HookContext};

/// Create a test HookContext with a component ID set
///
/// # Example
///
/// ```rust,ignore
/// let mut ctx = test_hook_context();
/// let signal = ctx.use_hook::<SignalHook<i32>>(0);
/// ```
pub fn test_hook_context() -> HookContext {
    let mut ctx = HookContext::new();
    ctx.begin_component(ComponentId(1));
    ctx
}

/// Create a test HookContext with a custom component ID
pub fn test_hook_context_with_id(id: usize) -> HookContext {
    let mut ctx = HookContext::new();
    ctx.begin_component(ComponentId(id as u64));
    ctx
}
