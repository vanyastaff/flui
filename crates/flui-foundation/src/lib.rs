//! # FLUI Foundation
//!
//! This crate provides fundamental types, utilities, and abstractions used
//! throughout the FLUI UI framework ecosystem. It contains zero-dependency or
//! minimal-dependency building blocks that other FLUI crates depend on.
//!
//! ## Overview
//!
//! FLUI Foundation contains:
//! - **Tree IDs**: `ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId`
//!   for the 5-tree architecture
//! - **Keys**: `Key`, `ValueKey`, `UniqueKey` for widget identity
//!   (ObjectKey/GlobalKey in flui-view)
//! - **Change Notification**: Observable patterns for reactive UI updates
//! - **Callbacks**: `VoidCallback`, `ValueChanged`, and other callback type
//!   aliases
//! - **Listener IDs**: Stable identifiers for change-notification listeners
//! - **Diagnostics**: Debugging and introspection utilities
//! - **Error Handling**: Standardized error types and utilities
//! - **Notification System**: Base abstractions for event bubbling
//!
//! ## Design Principles
//!
//! 1. **Minimal Dependencies**: Only essential external crates
//! 2. **Zero-Cost Abstractions**: Performance-critical paths have no overhead
//! 3. **Thread Safety**: All types are designed to work in multi-threaded
//!    contexts
//! 4. **Composability**: Types work well together and with external code
//! 5. **Stability**: Strong backwards compatibility guarantees
//!
//! ## Quick Start
//!
//! ```rust
//! use std::sync::Arc;
//!
//! use flui_foundation::{ChangeNotifier, ElementId, Key, Listenable};
//!
//! // Create unique identifiers
//! let element_id = ElementId::new(1);
//! let key = Key::new();
//!
//! // Observable values for reactive UI
//! let mut notifier = ChangeNotifier::new();
//! let listener_id = notifier.add_listener(Arc::new(|| {
//!     // react to the change (e.g. mark a widget dirty)
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
//! use flui_foundation::{ElementId, Key};
//!
//! // Generational element identifier: packs slab index + generation into 8 bytes.
//! let element_id = ElementId::new(42); // 1-based: index() == 41
//! assert_eq!(element_id.index(), 41);
//! // Option<ElementId> has the same size (niche optimisation, generation >= 1)
//! assert_eq!(
//!     std::mem::size_of::<ElementId>(),
//!     std::mem::size_of::<Option<ElementId>>(),
//! );
//!
//! // Keys for element matching during rebuilds
//! let key1 = Key::new();
//! let key2 = Key::from_str("header");
//! assert_ne!(key1, key2);
//! ```
//!
//! ### Change Notification
//!
//! ```rust
//! use std::sync::Arc;
//!
//! use flui_foundation::{ChangeNotifier, Listenable, ValueNotifier};
//!
//! // Basic change notification
//! let mut notifier = ChangeNotifier::new();
//! let listener = notifier.add_listener(Arc::new(|| {
//!     // react to the change
//! }));
//!
//! // Value-holding notifier
//! let mut value_notifier = ValueNotifier::new(42);
//! let value_listener = value_notifier.add_listener(Arc::new(|| {
//!     // react to the value change
//! }));
//! value_notifier.set_value(100);
//! assert_eq!(*value_notifier.value(), 100);
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
//! let rendered = node.to_string();
//! assert!(rendered.contains("MyWidget"));
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
//! - `ElementId` uses a packed `NonZeroU64` (index + generation) for niche optimization
//! - Keys use atomic counters for O(1) generation
//! - Change notifiers use efficient listener storage
//! - Atomic flags provide lock-free state management

#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub,
    clippy::pedantic
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

// Core modules - fundamental types with minimal dependencies
pub mod binding;
pub mod callbacks;
pub mod consts;
pub mod id;
pub mod key;
pub mod wasm;

// Logging - cross-platform tracing backend (merged from flui-log in
// D-block PR-C-1 per docs/plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md U1).
pub mod log;

// Reactive programming - change notification and observables
pub mod notifier;

// Generic typed notification channel + unified listener registry
pub mod listener_registry;
pub mod notifier_generic;

// Diagnostics and debugging
pub mod debug;

// ============================================================================
// RE-EXPORTS
// ============================================================================

// Core types - IDs for all tree levels
// Binding infrastructure
pub use binding::{BindingBase, HasInstance, check_instance};
// Callbacks
pub use callbacks::{
    FallibleCallback, Predicate, ValueChanged, ValueGetter, ValueSetter, ValueTransformer,
    VoidCallback,
};
// Constants
pub use consts::{DEBUG_MODE, EPSILON, EPSILON_F32, IS_DESKTOP, IS_MOBILE, IS_WEB, RELEASE_MODE};
// Diagnostics
pub use debug::{
    DIAGNOSTICS_FORMAT_VERSION, DebugPaintConfig, DiagnosticLevel, Diagnosticable,
    DiagnosticsBuilder, DiagnosticsEnvelope, DiagnosticsNode, DiagnosticsProperty,
    DiagnosticsPropertyKind, DiagnosticsTreeStyle, DiagnosticsValue,
};
pub use id::{
    // Core tree IDs (5-tree architecture)
    ElementId,
    // Scheduler IDs (consumed by flui-scheduler)
    FrameCallbackId,
    FrameId,
    // Generic ID system
    Id,
    Identifier,
    LayerId,
    // Listener/Observer IDs
    ListenerId,
    Marker,
    ObserverId,
    RawId,
    RenderId,
    SemanticsId,
    TaskId,
    TickerId,
    // Minimal tree-generic bound (does not expose raw index; ElementId satisfies this)
    TreeId,
    ViewId,
    // Marker types module
    markers,
};
pub use key::{Key, KeyRef, Keyed, UniqueKey, ValueKey, ViewKey, WithKey};
// Change notification (Listenable pattern)
pub use notifier::{ChangeNotifier, Listenable, ListenerCallback, ValueListenable, ValueNotifier};
// Generic typed channel + unified listener registry
pub use listener_registry::{ListenerRegistry, ListenerSubscription};
pub use notifier_generic::{ArgCallback, Notifier};
// WASM compatibility
pub use wasm::WasmNotSendSync;

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
        // Constants
        DEBUG_MODE,
        // Diagnostics
        DiagnosticLevel,
        Diagnosticable,
        EPSILON,
        // IDs
        ElementId,
        // Callbacks
        FallibleCallback,
        // Keys
        HasInstance,
        IS_DESKTOP,
        IS_MOBILE,
        IS_WEB,
        // Generic ID system
        Id,
        Identifier,
        Key,
        KeyRef,
        Keyed,
        LayerId,
        Listenable,
        ListenerCallback,
        ListenerId,
        // Observer IDs
        ObserverId,
        Predicate,
        RELEASE_MODE,
        RenderId,
        SemanticsId,
        TreeId,
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
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_types() {
        // ElementId::new(n) is 1-based: new(1).index() == 0.
        let element_id = ElementId::new(1);
        assert_eq!(element_id.index(), 0);

        let _key = Key::new();

        let notifier = ChangeNotifier::new();
        let _listener = notifier.add_listener(std::sync::Arc::new(|| {}));
    }
}
