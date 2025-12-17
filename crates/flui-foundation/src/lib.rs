//! # FLUI Foundation
//!
//! This crate provides fundamental types, utilities, and abstractions used throughout
//! the FLUI UI framework ecosystem. It contains zero-dependency or minimal-dependency
//! building blocks that other FLUI crates depend on.
//!
//! ## Overview
//!
//! FLUI Foundation contains:
//! - **Tree IDs**: `ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId` for the 5-tree architecture
//! - **Keys**: `Key`, `ValueKey`, `ObjectKey`, `UniqueKey`, `GlobalKey` for widget identity
//! - **Change Notification**: Observable patterns for reactive UI updates
//! - **Callbacks**: `VoidCallback`, `ValueChanged`, and other callback type aliases
//! - **Platform**: `TargetPlatform` for platform detection
//! - **Observer Lists**: Efficient observer/listener collections
//! - **Diagnostics**: Debugging and introspection utilities
//! - **Error Handling**: Standardized error types and utilities
//! - **Notification System**: Base abstractions for event bubbling
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
//! - `ElementId` uses `NonZeroUsize` for niche optimization
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
pub mod assert;
pub mod binding;
pub mod callbacks;
pub mod consts;
pub mod id;
pub mod key;
pub mod observer;
pub mod platform;
pub mod slot;

// Reactive programming - change notification and observables
pub mod notifier;

// Diagnostics and debugging
pub mod debug;
pub mod error;

// ============================================================================
// RE-EXPORTS
// ============================================================================

// Core types - IDs for all tree levels
pub use id::{
    ElementId, Identifier, LayerId, ListenerId, ObserverId, RenderId, SemanticsId, ViewId,
};
pub use key::{GlobalKey, Key, KeyRef, Keyed, ObjectKey, UniqueKey, ValueKey, ViewKey, WithKey};
pub use slot::Slot;

// Constants
pub use consts::{
    approx_equal, approx_equal_f32, is_near_zero, is_near_zero_f32, DEBUG_MODE, EPSILON,
    EPSILON_F32, IS_DESKTOP, IS_MOBILE, IS_WEB, RELEASE_MODE,
};

// Binding infrastructure
pub use binding::{check_instance, BindingBase, HasInstance};

// Assertions and error handling
pub use assert::FluiError;

// Callbacks
pub use callbacks::{
    FallibleCallback, Predicate, ValueChanged, ValueGetter, ValueSetter, ValueTransformer,
    VoidCallback,
};

// Platform
pub use platform::TargetPlatform;

// Observer lists
pub use observer::{HashedObserverList, ObserverList, SyncObserverList};

// Change notification (Listenable pattern)
pub use notifier::{
    ChangeNotifier, Listenable, ListenerCallback, MergedListenable, ValueListenable, ValueNotifier,
};

// Diagnostics
pub use debug::{
    DiagnosticLevel, Diagnosticable, DiagnosticsBuilder, DiagnosticsNode, DiagnosticsProperty,
    DiagnosticsTreeStyle,
};

// Error handling
pub use error::{FoundationError, Result};

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
        // Binding infrastructure
        BindingBase,
        // Change notification
        ChangeNotifier,
        // Diagnostics
        DiagnosticLevel,
        Diagnosticable,
        // IDs
        ElementId,
        // Callbacks
        FallibleCallback,
        // Assertions
        FluiError,
        // Keys
        GlobalKey,
        HasInstance,
        // Observer lists
        HashedObserverList,
        // Identifier trait
        Identifier,
        Key,
        KeyRef,
        Keyed,
        LayerId,
        Listenable,
        ListenerCallback,
        ListenerId,
        MergedListenable,
        ObjectKey,
        ObserverId,
        ObserverList,
        Predicate,
        RenderId,
        SemanticsId,
        // Slot
        Slot,
        SyncObserverList,
        // Platform
        TargetPlatform,
        UniqueKey,
        ValueChanged,
        ValueGetter,
        ValueKey,
        ValueListenable,
        ValueNotifier,
        ValueSetter,
        ValueTransformer,
        ViewId,
        ViewKey,
        VoidCallback,
        WithKey,
        // Constants
        DEBUG_MODE,
        EPSILON,
        IS_DESKTOP,
        IS_MOBILE,
        IS_WEB,
        RELEASE_MODE,
    };

    // Re-export assertion macros
    pub use crate::{
        debug_assert_finite, debug_assert_not_nan, debug_assert_range, debug_assert_valid,
        report_error, report_warning,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_types() {
        let element_id = ElementId::new(1);
        assert_eq!(element_id.get(), 1);

        let _key = Key::new();
        let slot = Slot::new(0);
        assert_eq!(slot.index(), 0);

        let notifier = ChangeNotifier::new();
        let _listener = notifier.add_listener(std::sync::Arc::new(|| {}));
    }
}
