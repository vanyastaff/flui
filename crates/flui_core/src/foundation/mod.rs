//! Foundation types for the Flui framework
//!
//! This module provides core utilities and types used throughout Flui:
//!
//! # Key Types
//!
//! - **Keys**: Unique identifiers for widgets ([`Key`], [`UniqueKey`], [`ValueKey`], [`GlobalKey`])
//! - **Change Notification**: Observable pattern ([`ChangeNotifier`], [`ValueNotifier`])
//! - **Diagnostics**: Debug utilities ([`DiagnosticsNode`], [`Diagnosticable`])
//! - **Platform**: Platform detection ([`TargetPlatform`], [`PlatformBrightness`])
//! - **IDs**: Element identifiers ([`ElementId`])
//! - **Slots**: Position tracking ([`Slot`])
//! - **String Cache**: Interned strings ([`InternedString`])
//!
//! # Examples
//!
//! ```rust
//! use flui_core::foundation::{ChangeNotifier, Listenable, ValueKey, TargetPlatform};
//! use std::sync::Arc;
//!
//! // Keys for widgets
//! let key = ValueKey::new("my_widget");
//!
//! // Change notifications
//! let mut notifier = ChangeNotifier::new();
//! notifier.add_listener(Arc::new(|| {
//!     println!("Changed!");
//! }));
//!
//! // Platform detection
//! let platform = TargetPlatform::current();
//! if platform.is_mobile() {
//!     println!("Running on mobile");
//! }
//! ```
//!
//! # Prelude
//!
//! For convenient imports, consider using the prelude:
//!
//! ```rust
//! use flui_core::foundation::prelude::*;
//! ```

pub mod change_notifier;
pub mod diagnostics;
pub mod id;
pub mod key;
pub mod platform;
pub mod slot;
pub mod string_cache;

// Prelude module for convenient imports
pub mod prelude {
    //! Prelude module for convenient imports
    //!
    //! This module re-exports the most commonly used types from the foundation module.
    //!
    //! # Example
    //!
    //! ```rust
    //! use flui_core::foundation::prelude::*;
    //!
    //! let key = ValueKey::new("widget");
    //! let mut notifier = ChangeNotifier::new();
    //! let platform = TargetPlatform::current();
    //! ```

    pub use super::change_notifier::{
        ChangeNotifier, Listenable, ListenerCallback, ListenerId, ValueNotifier,
    };
    pub use super::diagnostics::{Diagnosticable, DiagnosticLevel};
    pub use super::id::ElementId;
    pub use super::key::{GlobalKey, Key, KeyId, StringKey, UniqueKey, ValueKey, WidgetKey};
    pub use super::platform::{PlatformBrightness, TargetPlatform};
    pub use super::slot::Slot;
    pub use super::string_cache::InternedString;
}

// Re-exports - Change Notification
#[doc(inline)]
pub use change_notifier::{
    ChangeNotifier, Listenable, ListenerCallback, ListenerId, MergedListenable, ValueNotifier,
};

// Re-exports - Diagnostics
#[doc(inline)]
pub use diagnostics::{
    DiagnosticLevel, Diagnosticable, DiagnosticsBuilder, DiagnosticsNode, DiagnosticsProperty,
    DiagnosticsTreeStyle, ParseDiagnosticLevelError, ParseDiagnosticsTreeStyleError,
};

// Re-exports - IDs
#[doc(inline)]
pub use id::ElementId;

// Re-exports - Keys
#[doc(inline)]
pub use key::{
    GlobalKey, GlobalObjectKey, IntKey, Key, KeyId, LabeledGlobalKey, LocalKey, ObjectKey,
    StringKey, UniqueKey, ValueKey, WidgetKey,
};

// Re-exports - Platform
#[doc(inline)]
pub use platform::{
    ParseBrightnessError, ParsePlatformError, PlatformBrightness, TargetPlatform,
};

// Re-exports - Slot
#[doc(inline)]
pub use slot::{Slot, SlotConversionError};

// Re-exports - String Cache
pub use string_cache::{capacity, get, intern, is_empty, len, resolve, InternedString};

/// Type alias for callback functions without parameters or return values
///
/// This is used throughout Flui for event handlers, listeners, and other callbacks
/// that don't need to pass data.
///
/// # Examples
///
/// ```rust
/// use flui_core::foundation::VoidCallback;
/// use std::sync::Arc;
///
/// fn register_callback(callback: VoidCallback) {
///     callback();
/// }
///
/// let callback = Arc::new(|| {
///     println!("Called!");
/// });
///
/// register_callback(callback);
/// ```
///
/// # Note
///
/// This is identical to [`ListenerCallback`] from the change notification system.
/// Use [`ListenerCallback`] when working with [`ChangeNotifier`] and [`Listenable`],
/// and [`VoidCallback`] for general-purpose callbacks.
pub type VoidCallback = ListenerCallback;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_void_callback_is_listener_callback() {
        // VoidCallback and ListenerCallback are the same type
        fn takes_void(cb: VoidCallback) {
            cb();
        }

        fn takes_listener(cb: ListenerCallback) {
            cb();
        }

        let callback: VoidCallback = std::sync::Arc::new(|| {
            println!("test");
        });

        takes_void(callback.clone());
        takes_listener(callback);
    }

    #[test]
    fn test_prelude_imports() {
        use prelude::*;

        // Should be able to use types from prelude
        let _key = ValueKey::new("test");
        let _notifier = ChangeNotifier::new();
        let _platform = TargetPlatform::current();
        let _id = ElementId::new();
        let _slot = Slot::new(0);
    }
}