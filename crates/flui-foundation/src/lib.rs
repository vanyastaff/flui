//! # FLUI Foundation
//!
//! This crate provides fundamental types, utilities, and abstractions used throughout
//! the FLUI UI framework ecosystem. It contains zero-dependency or minimal-dependency
//! building blocks that other FLUI crates depend on.
//!
//! ## Overview
//!
//! FLUI Foundation contains:
//! - **Core Types**: ElementId, Key, Slot for element identification and positioning
//! - **Change Notification**: Observable patterns for reactive UI updates
//! - **Diagnostics**: Debugging and introspection utilities
//! - **Error Handling**: Standardized error types and utilities
//! - **Atomic Utilities**: Lock-free operations for performance-critical code
//! - **Notification System**: Base abstractions for event bubbling (traits only)
//!
//! ## Design Principles
//!
//! 1. **Minimal Dependencies**: Only essential external crates
//! 2. **Zero-Cost Abstractions**: Performance-critical paths have no overhead
//! 3. **Thread Safety**: All types are designed to work in multi-threaded contexts
//! 4. **Composability**: Types work well together and with external code
//! 5. **Stability**: Strong backwards compatibility guarantees
//!
//! ## Quick Start
//!
//! ```rust
//! use flui_foundation::{ElementId, Key, ChangeNotifier, Listenable};
//! use std::sync::Arc;
//!
//! // Create unique identifiers
//! let element_id = ElementId::new(1);
//! let key = Key::new();
//!
//! // Observable values for reactive UI
//! let mut notifier = ChangeNotifier::new();
//! let listener_id = notifier.add_listener(Arc::new(|| {
//!     println!("Value changed!");
//! }));
//!
//! // Notify listeners of changes
//! notifier.notify_listeners();
//! ```
//!
//! ## Feature Flags
//!
//! - `serde`: Enables serialization support for foundation types
//! - `async`: Enables async utilities and notification patterns
//! - `full`: Enables all optional features
//!
//! ## Architecture
//!
//! Foundation types are designed to be:
//! - **Lightweight**: Minimal memory footprint
//! - **Fast**: Optimized for common operations
//! - **Safe**: Extensive use of type system for correctness
//! - **Debuggable**: Rich debugging and diagnostic information
//!
//! ## Examples
//!
//! ### Element Identification
//!
//! ```rust
//! use flui_foundation::{ElementId, Key, Slot};
//!
//! // Unique element identifier with O(1) operations
//! let element_id = ElementId::new(42);
//! assert_eq!(element_id.get(), 42);
//!
//! // Keys for element matching during rebuilds
//! let key1 = Key::new();
//! let key2 = Key::from_str("header");
//! assert_ne!(key1, key2);
//!
//! // Child slot positioning
//! let slot = Slot::new(0);
//! assert_eq!(slot.index(), 0);
//! ```
//!
//! ### Change Notification
//!
//! ```rust
//! use flui_foundation::{ChangeNotifier, ValueNotifier, Listenable};
//! use std::sync::Arc;
//!
//! // Basic change notification
//! let mut notifier = ChangeNotifier::new();
//! let listener = notifier.add_listener(Arc::new(|| println!("Changed!")));
//!
//! // Value-holding notifier
//! let mut value_notifier = ValueNotifier::new(42);
//! let value_listener = value_notifier.add_listener(Arc::new(|| {
//!     println!("Value changed!");
//! }));
//! value_notifier.set_value(100);
//! ```
//!
//! ### Diagnostics
//!
//! ```rust
//! use flui_foundation::{DiagnosticsNode, DiagnosticsProperty, DiagnosticsTreeStyle};
//!
//! let node = DiagnosticsNode::new("MyWidget")
//!     .with_property(DiagnosticsProperty::new("width", 100.0))
//!     .with_property(DiagnosticsProperty::new("height", 200.0));
//!
//! println!("{}", node.to_string());
//! ```
//!
//! ## Thread Safety
//!
//! All foundation types are designed for multi-threaded use:
//! - `ElementId`: `Send + Sync` (copy type)
//! - `Key`: `Send + Sync` (copy type)
//! - `ChangeNotifier`: `Send + Sync` with internal synchronization
//! - `AtomicElementFlags`: Lock-free atomic operations
//!
//! ## Performance
//!
//! Foundation types are optimized for common UI patterns:
//! - ElementId uses `NonZeroUsize` for niche optimization
//! - Keys use atomic counters for O(1) generation
//! - Change notifiers use efficient listener storage
//! - Atomic flags provide lock-free state management

#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

// Core modules - fundamental types with minimal dependencies
pub mod element_id;
pub mod key;
pub mod slot;
pub mod view_mode;

// Reactive programming - change notification and observables
pub mod change_notifier;

// Diagnostics and debugging
pub mod diagnostics;
pub mod error;

// Atomic utilities for lock-free operations
pub mod atomic_flags;

// Notification system - base abstractions for event bubbling
pub mod notification;

// Async utilities (feature gated)
#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub mod async_utils;

// Serialization support (feature gated)
#[cfg(feature = "serde")]
#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
pub mod serde_support;

// ============================================================================
// RE-EXPORTS
// ============================================================================

// Core types
pub use element_id::ElementId;
pub use key::{Key, KeyRef};
pub use slot::Slot;
pub use view_mode::ViewMode;

// Change notification
pub use change_notifier::{
    ChangeNotifier, Listenable, ListenerCallback, ListenerId, MergedListenable, ValueNotifier,
};

// Notification system (base abstractions only)
pub use notification::{DynNotification, Notification};

// Diagnostics
pub use diagnostics::{
    DiagnosticLevel, Diagnosticable, DiagnosticsBuilder, DiagnosticsNode, DiagnosticsProperty,
    DiagnosticsTreeStyle,
};

// Error handling
pub use error::{FoundationError, Result};

// Atomic utilities
pub use atomic_flags::{AtomicElementFlags, ElementFlags};

// Async utilities (feature gated)
#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub use async_utils::{AsyncChangeNotifier, AsyncValueNotifier};

// ============================================================================
// PRELUDE
// ============================================================================

/// The foundation prelude - commonly used types and traits.
///
/// This module contains the most commonly used items from flui-foundation
/// to make them easy to import:
///
/// ```rust
/// use flui_foundation::prelude::*;
/// ```
pub mod prelude {
    pub use crate::{
        AtomicElementFlags, ChangeNotifier, DiagnosticLevel, Diagnosticable, DynNotification,
        ElementFlags, ElementId, Key, KeyRef, Listenable, ListenerCallback, ListenerId,
        Notification, Slot, ValueNotifier,
    };

    #[cfg(feature = "async")]
    #[cfg_attr(docsrs, doc(cfg(feature = "async")))]
    pub use crate::{AsyncChangeNotifier, AsyncValueNotifier};
}

// ============================================================================
// VERSION INFO
// ============================================================================

/// The version of the flui-foundation crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The version components (major, minor, patch).
pub const VERSION_TUPLE: (u32, u32, u32) = (
    0, // major
    1, // minor
    0, // patch
);

// ============================================================================
// COMPILE-TIME FEATURE DETECTION
// ============================================================================

/// Returns true if the `serde` feature is enabled.
pub const fn has_serde_support() -> bool {
    cfg!(feature = "serde")
}

/// Returns true if the `async` feature is enabled.
pub const fn has_async_support() -> bool {
    cfg!(feature = "async")
}

/// Returns a string describing the enabled features.
pub fn feature_summary() -> &'static str {
    match (has_serde_support(), has_async_support()) {
        (true, true) => "serde + async",
        (true, false) => "serde",
        (false, true) => "async",
        (false, false) => "minimal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_info() {
        assert_eq!(VERSION, "0.1.0");
        assert_eq!(VERSION_TUPLE, (0, 1, 0));
    }

    #[test]
    fn test_feature_detection() {
        // These tests verify feature detection works
        let _has_serde = has_serde_support();
        let _has_async = has_async_support();
        let _summary = feature_summary();
    }

    #[test]
    fn test_basic_types() {
        // Test that basic types work
        let element_id = ElementId::new(1);
        assert_eq!(element_id.get(), 1);

        let key = Key::new();
        let slot = Slot::new(0);
        assert_eq!(slot.index(), 0);

        let mut notifier = ChangeNotifier::new();
        let _listener = notifier.add_listener(std::sync::Arc::new(|| {}));
    }
}
