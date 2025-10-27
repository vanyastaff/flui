//! Foundation - Core types and utilities
//!
//! This module provides fundamental types used throughout FLUI.

pub mod change_notifier;
pub mod diagnostics;
pub mod key;
pub mod notification;
pub mod slot;






pub use key::{Key, KeyRef};
pub use diagnostics::{
    DiagnosticLevel,
    DiagnosticsTreeStyle,
    DiagnosticsProperty,
    DiagnosticsNode,
    Diagnosticable,
    DiagnosticsBuilder,
};
pub use change_notifier::{
    Listenable,
    ListenerId,
    ListenerCallback,
    ChangeNotifier,
    ValueNotifier,
    MergedListenable,
};
pub use slot::Slot;
pub use notification::{
    Notification,
    DynNotification,
    ScrollNotification,
    LayoutChangedNotification,
    SizeChangedNotification,
    KeepAliveNotification,
    FocusChangedNotification,
};






