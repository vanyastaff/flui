//! FLUI Reactivity - Reactive state management with hooks and signals
//!
//! This crate provides a production-ready reactive system inspired by React Hooks,
//! with full thread-safety and type-safe signal management.
//!
//! # Core Concepts
//!
//! ## Signals
//!
//! Reactive state holders with automatic change tracking:
//!
//! ```rust,ignore
//! use flui_reactivity::Signal;
//!
//! let count = Signal::new(0);
//! println!("Count: {}", count.get());
//! count.set(42);  // Triggers subscribed callbacks
//! ```
//!
//! ## Memo
//!
//! Memoized (cached) computations that update when dependencies change:
//!
//! ```rust,ignore
//! use flui_reactivity::{use_memo, DependencyId};
//!
//! let count_dep = DependencyId::new(count.id().0);
//! let doubled = use_memo(ctx, vec![count_dep], || count.get() * 2);
//! ```
//!
//! ## Effect
//!
//! Side effects with automatic cleanup:
//!
//! ```rust,ignore
//! use flui_reactivity::{use_effect, DependencyId};
//!
//! let count_dep = DependencyId::new(count.id().0);
//! use_effect(ctx, vec![count_dep], || {
//!     println!("Count changed to: {}", count.get());
//!     None // No cleanup
//! });
//! ```
//!
//! ## Resource
//!
//! Async data fetching (requires `async` feature):
//!
//! ```rust,ignore
//! use flui_reactivity::{use_resource, ResourceState};
//!
//! let user_resource = use_resource(ctx, vec![], || {
//!     Box::pin(async { fetch_user().await })
//! });
//!
//! match user_resource.state() {
//!     ResourceState::Loading => println!("Loading..."),
//!     ResourceState::Ready(user) => println!("User: {:?}", user),
//!     ResourceState::Error(err) => println!("Error: {}", err),
//!     _ => {}
//! }
//! ```
//!
//! # Thread Safety
//!
//! All signals are fully thread-safe:
//! - Signal values must be `Send`
//! - Callbacks must be `Send + Sync`
//! - Uses `Arc<Mutex<T>>` and `parking_lot::Mutex`
//! - Lock-free reads with DashMap

pub mod batch;
pub mod computed;
pub mod context;
pub mod context_provider;
pub mod error;
pub mod hooks;
pub mod owner;
pub mod runtime;
pub mod scheduler;
pub mod signal;
pub mod test_harness;
pub mod traits;

// Async support (optional, enabled via "async" feature)
#[cfg(feature = "async")]
pub mod r#async;

// Re-export core types
pub use batch::{batch, is_batching, BatchGuard};
pub use computed::{Computed, ComputedId};
pub use owner::{create_root, with_owner, Owner, OwnerId};
pub use runtime::{RuntimeConfig, SignalRuntime};
pub use scheduler::{EffectId, EffectPriority, EffectScheduler};
pub use signal::{Signal, SignalId};

// Re-export hooks from hooks module
pub use hooks::{
    use_callback, use_effect, use_effect_always, use_memo, use_memo_once, use_reducer, use_ref,
    Callback, CleanupFn, Dispatch, EffectFn, Reducer, Ref,
};

#[cfg(feature = "async")]
pub use hooks::{use_resource, Resource, ResourceState};

// Re-export Context API
pub use context_provider::{
    provide_context, remove_context, use_context, ContextId, ContextProvider, ContextScope,
};

// Re-export error types
pub use error::{HookError, ReactivityError, Result, ResultExt, RuntimeError, SignalError};

// Hook system infrastructure
pub use context::{ComponentId, HookContext, HookId, HookIndex};
pub use traits::EffectHook as EffectHookTrait;
pub use traits::{AsyncHook, DependencyId, Hook, ReactiveHook};

// Hook implementations
// TODO: Uncomment when implementations are ready
// pub use hooks::{
//     Effect, EffectHook, EffectState, Memo, MemoHook, MemoState, Resource, ResourceHook,
//     ResourceState,
// };

// TODO: Uncomment when test harness is implemented
// pub use test_harness::{HookTestHarness, MultiHookTestHarness};

// Re-export foundation types that hooks may need
// TODO: Uncomment when flui_foundation is available
// pub use flui_foundation::ElementId;

// ============================================================================
// PRELUDE
// ============================================================================

/// Commonly used types and traits for reactive programming.
///
/// ```rust,ignore
/// use flui_reactivity::prelude::*;
/// ```
pub mod prelude {
    // Signals
    pub use crate::{Owner, Signal, SignalId};

    // Computed
    pub use crate::{Computed, ComputedId};

    // Hooks
    pub use crate::{use_callback, use_effect, use_memo, use_reducer, use_ref, Callback, Ref};

    // Context API
    pub use crate::{provide_context, use_context, ContextProvider};

    // Batching
    pub use crate::{batch, BatchGuard};

    // Hook infrastructure
    pub use crate::{DependencyId, HookContext, HookId};

    // Error types
    pub use crate::{HookError, ReactivityError};
}
