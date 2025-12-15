//! Render object lifecycle management.
//!
//! This module provides efficient lifecycle state tracking for render objects,
//! replacing Flutter's multiple boolean flags with compact, type-safe structures.
//!
//! # Module Structure
//!
//! - [`core`]: Core lifecycle types ([`RenderLifecycle`], [`DirtyFlags`], [`RenderState`])
//! - [`state`]: Unified render object state ([`RenderObjectState`])
//! - [`base`]: Base render object implementation ([`BaseRenderObject`])
//!
//! # Benefits over Flutter's approach
//!
//! - **Memory efficiency**: 2 bytes for lifecycle+flags vs 4-8+ bytes in Flutter
//! - **Type safety**: Compile-time state transition validation
//! - **Clarity**: Single source of truth for lifecycle state
//! - **Debug**: Clear state names in error messages
//!
//! # Example
//!
//! ```
//! use flui_rendering::lifecycle::{RenderLifecycle, DirtyFlags, RenderState};
//!
//! let mut state = RenderState::new();
//! assert!(state.lifecycle().is_detached());
//!
//! // After attach, needs layout
//! state.attach();
//! assert!(state.needs_layout());
//! ```

mod base;
mod core;
mod state;

// Re-export core types
pub use self::base::BaseRenderObject;
pub use self::core::{DirtyFlags, RenderLifecycle, RenderState};
pub use self::state::RenderObjectState;
