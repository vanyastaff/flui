//! Advanced hook system with trait-based architecture.
//!
//! This module provides a production-ready hook system inspired by React Hooks
//! with additional type safety and performance optimizations.
//!
//! # Architecture
//!
//! The hook system is built on several core abstractions:
//!
//! - [`Hook`] trait - Base trait all hooks implement
//! - [`HookContext`] - Manages hook state and lifecycle
//! - [`ComponentId`] & [`HookId`] - Unique identifiers for components and hooks
//!
//! # Available Hooks
//!
//! - [`use_signal`] - Reactive state (like useState)
//! - [`use_memo`] - Memoized computations
//! - [`use_effect`] - Side effects with cleanup
//! - [`use_resource`] - Async data fetching
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_core::hooks::{use_signal, use_effect};
//!
//! struct Counter;
//!
//! impl Component for Counter {
//!     fn build(&self, ctx: &mut BuildContext) -> Widget {
//!         let count = use_signal(0);
//!
//!         use_effect(move || {
//!             println!("Count: {}", count.get());
//!         });
//!
//!         Button::new("Increment")
//!             .on_press(move || count.update(|n| n + 1))
//!             .into()
//!     }
//! }
//! ```

pub mod hook_trait;
pub mod hook_context;
pub mod signal;
pub mod memo;
pub mod effect;
pub mod resource;
pub mod test_harness;

pub use hook_trait::{Hook, ReactiveHook, EffectHook, AsyncHook, DependencyId};
pub use hook_context::{HookContext, ComponentId, HookId, HookIndex, with_hook_context};
pub use signal::{use_signal, Signal, SignalHook, SignalId};
pub use memo::{use_memo, Memo, MemoHook};
pub use effect::{use_effect, use_effect_simple, Effect, EffectHook as EffectHookImpl, CleanupFn};
pub use resource::{use_resource, Resource, ResourceHook};
pub use test_harness::{HookTestHarness, MultiHookTestHarness};

// TODO(2025-03): Add hook composition support.
// Allow composing hooks together with ComposableHook trait.

// TODO(2025-03): Add compile-time hook rules enforcement.
// Use marker traits like ComponentHook to enforce hook rules at compile time.
