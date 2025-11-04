//! Fine-grained reactive primitives for FLUI
//!
//! This crate provides Signal-based reactivity inspired by Leptos and SolidJS,
//! enabling fine-grained UI updates without virtual DOM diffing.
//!
//! # Core Concepts
//!
//! - **Signal<T>**: A reactive value that automatically tracks dependencies
//! - **SignalRuntime**: Thread-local storage for signal values
//! - **Reactive Scope**: Automatic dependency tracking during widget build
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_reactive::Signal;
//!
//! let count = Signal::new(0);
//!
//! // Reading a signal (tracks dependency in reactive scope)
//! let value = count.get();
//!
//! // Writing to a signal (notifies dependents)
//! count.set(10);
//! count.update(|v| *v += 1);
//! ```

pub mod runtime;
pub mod signal;
pub mod scope;

pub use runtime::{SignalRuntime, with_runtime};
pub use signal::{Signal, SignalId};
pub use scope::{ScopeId, ReactiveScope, with_scope, create_scope};
