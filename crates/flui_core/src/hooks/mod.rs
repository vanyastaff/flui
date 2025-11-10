//! Advanced hook system with trait-based architecture
//!
//! This module provides a production-ready hook system inspired by React Hooks
//! with additional type safety, thread-safety, and performance optimizations.
//!
//! # What are Hooks?
//!
//! Hooks are functions that let you "hook into" FLUI's state and lifecycle features
//! from within Views. They provide:
//!
//! - **State management** without GAT types
//! - **Side effects** with automatic cleanup
//! - **Memoization** for expensive computations
//! - **Async data** fetching with suspense
//!
//! Similar to:
//! - **React**: useState, useEffect, useMemo, etc.
//! - **Leptos**: create_signal, create_memo, create_effect
//! - **SolidJS**: createSignal, createMemo, createEffect
//!
//! # Architecture
//!
//! The hook system is built on several core abstractions:
//!
//! - [`Hook`] trait - Base trait all hooks implement
//! - [`HookContext`] - Manages hook state and lifecycle per component
//! - [`ComponentId`] & [`HookId`] - Unique identifiers for components and hooks
//! - [`SignalRuntime`] - Thread-local runtime for signal storage
//!
//! # Available Hooks
//!
//! | Hook | Purpose | Example |
//! |------|---------|---------|
//! | [`use_signal`] | Reactive state | `let count = use_signal(ctx, 0);` |
//! | [`use_memo`] | Memoized computation | `let doubled = use_memo(ctx, \|_\| count.get() * 2);` |
//! | [`use_effect`] | Side effects | `use_effect(ctx, \|\| { log("mounted"); });` |
//! | [`use_resource`] | Async data fetching | `let data = use_resource(ctx, fetch_user);` |
//!
//! # Thread Safety (CRITICAL)
//!
//! **FLUI hooks are fully thread-safe!** This is different from React:
//!
//! - **Signal values**: Must be `Send` (can be moved between threads)
//! - **Callbacks**: Must be `Send + Sync` (can be called from any thread)
//! - **Storage**: Uses `Arc<Mutex<T>>` instead of `Rc<RefCell<T>>`
//! - **Mutex**: Uses `parking_lot::Mutex` (2-3x faster than std, no poisoning)
//!
//! ```rust,ignore
//! // GOOD: i32 is Send
//! let count = use_signal(ctx, 0);
//!
//! // GOOD: String is Send
//! let name = use_signal(ctx, String::from("Alice"));
//!
//! // BAD: Rc is !Send
//! // let rc = use_signal(ctx, Rc::new(42));  // Compile error!
//!
//! // GOOD: Arc is Send
//! let arc = use_signal(ctx, Arc::new(42));
//! ```
//!
//! # Hook Rules (MUST FOLLOW)
//!
//! **Breaking these rules causes panics!** See `crates/flui_core/src/hooks/RULES.md` for details.
//!
//! 1. **Always call hooks in the same order** every render
//! 2. **Never call hooks conditionally** (no `if` around hooks)
//! 3. **Never call hooks in loops** with variable iterations
//! 4. **Only call hooks at top level** of View::build()
//! 5. **Clone signals before moving** into closures (now Copy in v0.7.0!)
//!
//! ```rust,ignore
//! // GOOD: Hooks always called in same order
//! fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!     let count = use_signal(ctx, 0);
//!     let doubled = use_memo(ctx, move |_| count.get() * 2);
//!     // ...
//! }
//!
//! // BAD: Conditional hook call
//! fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!     if self.show_counter {
//!         let count = use_signal(ctx, 0);  // ❌ PANIC!
//!     }
//! }
//! ```
//!
//! # Example: Counter with Hooks
//!
//! ```rust,ignore
//! use flui_core::{View, BuildContext, IntoElement};
//! use flui_core::hooks::{use_signal, use_effect};
//!
//! #[derive(Debug, Clone)]
//! struct Counter;
//!
//! impl View for Counter {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         // Reactive state (now Copy in v0.7.0!)
//!         let count = use_signal(ctx, 0);
//!
//!         // Side effect (runs when count changes)
//!         use_effect(ctx, move || {
//!             println!("Count changed: {}", count.get());
//!             None  // No cleanup
//!         });
//!
//!         // Build UI
//!         Column::new()
//!             .child(Text::new(format!("Count: {}", count.get())))
//!             .child(Button::new("Increment")
//!                 .on_click(move || count.update(|n| n + 1)))
//!     }
//! }
//! ```
//!
//! # Performance
//!
//! - **Signals are Copy** (8 bytes) - no allocation overhead
//! - **parking_lot::Mutex** is 2-3x faster than std::sync::Mutex
//! - **Thread-local storage** for SignalRuntime (no locks for reads)
//! - **Fine-grained reactivity** (only affected parts rebuild)

pub mod effect;
pub mod hook_context;
pub mod hook_trait;
pub mod memo;
pub mod resource;
pub mod signal;
pub mod signal_runtime;
pub mod test_harness;

pub use effect::{use_effect, use_effect_simple, CleanupFn, Effect, EffectHook as EffectHookImpl};
pub use hook_context::{ComponentId, HookContext, HookId, HookIndex};
pub use hook_trait::{AsyncHook, DependencyId, EffectHook, Hook, ReactiveHook};
pub use memo::{use_memo, Memo, MemoHook};
pub use resource::{use_resource, Resource, ResourceHook};
pub use signal::{use_signal, Signal, SignalHook, SignalId};
pub use signal_runtime::{SignalRuntime, SIGNAL_RUNTIME};
pub use test_harness::{HookTestHarness, MultiHookTestHarness};

// Future enhancement: Hook composition via ComposableHook trait
// Future enhancement: Compile-time hook rules enforcement using marker traits
