//! Foundation layer for Flui framework
//!
//! This crate provides core utilities and types that are used throughout
//! the Flui framework, including keys, change notification, and diagnostics.
//!
//! # Architecture
//!
//! This is the lowest-level crate in Flui, similar to Flutter's foundation library.
//! It provides:
//!
//! - **Keys**: Unique identifiers for widgets (ValueKey, UniqueKey, GlobalKey)
//! - **Change Notification**: Observable pattern (Listenable, ChangeNotifier, ValueNotifier)
//! - **Diagnostics**: Debug and diagnostic utilities
//! - **Callbacks**: Common callback type aliases

#![warn(missing_docs)]
pub mod change_notifier;
pub mod diagnostics;
pub mod key;
pub mod platform;

// Re-exports
pub use change_notifier::{
    ChangeNotifier, Listenable, ListenerCallback, ListenerId, MergedListenable, ValueNotifier,
};
pub use diagnostics::{
    DiagnosticLevel, Diagnosticable, DiagnosticsBuilder, DiagnosticsNode, DiagnosticsProperty,
    DiagnosticsTreeStyle,
};
pub use key::{
    IntKey, Key, KeyFactory, KeyId, LocalKey, StringKey, UniqueKey, ValueKey, WidgetKey,
};
pub use platform::{PlatformBrightness, TargetPlatform};

/// Type alias for void callback functions
pub type VoidCallback = std::sync::Arc<dyn Fn() + Send + Sync>;

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::change_notifier::{ChangeNotifier, Listenable, ValueNotifier};
    pub use crate::diagnostics::{Diagnosticable, DiagnosticsNode};
    pub use crate::key::{Key, KeyFactory, UniqueKey, ValueKey};
    pub use crate::platform::TargetPlatform;
    pub use crate::VoidCallback;
}
