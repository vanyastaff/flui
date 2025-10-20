//! Foundation types for the Flui framework
//!
//! Core utilities and types used throughout Flui:
//! - Keys: Unique identifiers for widgets
//! - Change Notification: Observable pattern
//! - Diagnostics: Debug utilities
//! - Platform: Platform detection
//! - IDs: Element identifiers
//! - Lifecycle: Element lifecycle states

pub mod change_notifier;
pub mod diagnostics;
pub mod id;
pub mod key;
pub mod platform;
pub mod slot;
pub mod string_cache;

// Re-exports
pub use change_notifier::{
    ChangeNotifier, Listenable, ListenerCallback, ListenerId, MergedListenable, ValueNotifier,
};
pub use diagnostics::{
    DiagnosticLevel, Diagnosticable, DiagnosticsBuilder, DiagnosticsNode, DiagnosticsProperty,
    DiagnosticsTreeStyle,
};
pub use id::ElementId;
pub use key::{
    GlobalKey, GlobalObjectKey, IntKey, Key, KeyFactory, KeyId, LabeledGlobalKey, LocalKey,
    ObjectKey, StringKey, UniqueKey, ValueKey, WidgetKey,
};
pub use platform::{PlatformBrightness, TargetPlatform};
pub use slot::Slot;
pub use string_cache::{intern, resolve, InternedString};

/// Type alias for void callback functions
pub type VoidCallback = std::sync::Arc<dyn Fn() + Send + Sync>;
