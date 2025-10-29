//! Foundation - Core types and utilities
//!
//! This module provides fundamental types used throughout FLUI.

pub mod change_notifier;
pub mod diagnostics;
pub mod key;
pub mod notification;
pub mod slot;

pub use change_notifier::{
    ChangeNotifier, Listenable, ListenerCallback, ListenerId, MergedListenable, ValueNotifier,
};
pub use diagnostics::{
    DiagnosticLevel, Diagnosticable, DiagnosticsBuilder, DiagnosticsNode, DiagnosticsProperty,
    DiagnosticsTreeStyle,
};
pub use key::{Key, KeyRef};
pub use notification::{
    DynNotification, FocusChangedNotification, KeepAliveNotification, LayoutChangedNotification,
    Notification, ScrollNotification, SizeChangedNotification,
};
pub use slot::Slot;
