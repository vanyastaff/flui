//! Render object lifecycle management.
//!
//! This module provides efficient lifecycle state tracking for render objects,
//! following Flutter's approach where attachment is determined by owner presence.
//!
//! # Module Structure
//!
//! - [`core`]: Core types ([`DirtyFlags`], [`RelayoutBoundary`], [`RenderObjectFlags`])
//! - [`state`]: Unified render object state ([`RenderObjectState`])
//! - [`base`]: Base render object implementation ([`BaseRenderObject`])
//!
//! # Flutter Approach
//!
//! In Flutter, attachment is determined by `owner != null`:
//! ```dart
//! bool get attached => _owner != null;
//! ```
//!
//! Dirty state is tracked via separate boolean flags:
//! - `_needsLayout`
//! - `_needsPaint`
//! - `_needsCompositingBitsUpdate`
//! - etc.
//!
//! We pack these into `DirtyFlags` (1 byte) for memory efficiency.
//!
//! # Benefits
//!
//! - **Memory efficiency**: 2 bytes for flags vs 6-8+ bytes of booleans in Flutter
//! - **Flutter compatibility**: Same semantics as Flutter's render object lifecycle
//! - **Type safety**: Relayout boundary uses enum instead of nullable bool
//! - **Clear API**: Attachment via owner, dirty state via flags
//!
//! # Example
//!
//! ```
//! use flui_rendering::lifecycle::{DirtyFlags, RenderObjectFlags};
//!
//! let mut flags = RenderObjectFlags::new();
//! assert!(flags.needs_layout());  // New objects need layout
//! assert!(flags.needs_paint());   // New objects need paint
//!
//! flags.clear_needs_layout();
//! assert!(!flags.needs_layout());
//! ```

mod base;
mod core;
mod state;

// Re-export core types
pub use self::base::BaseRenderObject;
pub use self::core::{DirtyFlags, RelayoutBoundary, RenderObjectFlags};
pub use self::state::RenderObjectState;
