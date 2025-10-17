//! Core foundation types
//!
//! This module contains foundational types and utilities similar to Flutter's foundation library.
//! These are fundamental building blocks used across the entire UI system.
//!
//! ## Observability
//! - [`Listenable`]: Interface for objects that can be listened to
//! - [`ChangeNotifier`]: Notifies listeners when changed
//! - [`ValueNotifier`]: ChangeNotifier that holds a single value
//!
//! ## Future additions
//! - Key types (widget keys)
//! - DiagnosticableTree (debugging support)
//! - BindingBase (platform bindings)
//! - Scheduler primitives

pub mod callbacks;
pub mod diagnostics;
pub mod key;
pub mod listenable;




// Re-exports
pub use callbacks::{
    VoidCallback, AsyncCallback,
    ValueChanged, ValueGetter, ValueSetter,
    AsyncValueGetter, AsyncValueSetter,
    void_callback, value_changed, value_getter, value_setter,
};

pub use diagnostics::{
    Diagnosticable, DiagnosticsNode, DiagnosticsProperty, DiagnosticsBuilder,
    DiagnosticLevel, DiagnosticsTreeStyle,
};

pub use key::{
    Key, LocalKey, KeyId,
    UniqueKey, ValueKey, StringKey, IntKey,
    KeyFactory, WidgetKey,
};

pub use listenable::{
    Listenable, ChangeNotifier, ValueNotifier,
    ListenerId, ListenerCallback,
};



