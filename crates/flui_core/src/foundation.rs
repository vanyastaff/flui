//! Foundation re-export module
//!
//! This module provides backward compatibility by re-exporting foundation types
//! from the `flui-foundation` crate. All fundamental types have been moved to
//! the dedicated foundation crate for better modularity.
//!
//! ## Migration
//!
//! New code should import directly from `flui-foundation`:
//! ```rust
//! use flui_foundation::{ElementId, Key, Slot};
//! ```
//!
//! This module is maintained for backward compatibility:
//! ```rust
//! use flui_core::foundation::{ElementId, Key, Slot};
//! ```

// Re-export all foundation types for backward compatibility
pub use flui_foundation::*;

// Re-export UI notification types alongside foundation notification traits
pub use crate::notification::{
    FocusChangedNotification, KeepAliveNotification, LayoutChangedNotification, RouteChangeType,
    RouteChangedNotification, ScrollDirection, ScrollNotification, SizeChangedNotification,
};

// Type alias for backward compatibility
pub type CoreError = FoundationError;
