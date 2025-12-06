//! Element module - Core element types for FLUI
//!
//! This module provides the Element enum and related types for managing
//! the element tree in FLUI applications.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      ElementBase                            │
//! │  (lifecycle, flags, parent/slot, depth - shared by all)     │
//! └─────────────────────────────────────────────────────────────┘
//!                            ↓
//!         ┌──────────────────┼──────────────────┐
//!         ↓                                     ↓
//! ┌─────────────────────┐             ┌─────────────────────┐
//! │    ViewElement      │             │   RenderElement     │
//! │ - view_object       │             │ - render_object     │
//! │ - view_mode         │             │ - render_state      │
//! │ - children          │             │ - children          │
//! └─────────────────────┘             └─────────────────────┘
//!         ↓                                     ↓
//!         └──────────────────┬──────────────────┘
//!                            ↓
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      enum Element                           │
//! │     View(ViewElement) | Render(RenderElement)               │
//! └─────────────────────────────────────────────────────────────┘
//! ```

mod element;
mod element_base;
mod element_flags;
mod lifecycle;

// Core element enum
pub use element::Element;

// ViewElement is now in flui-view
pub use flui_view::{AtomicViewFlags, ViewElement, ViewFlags, ViewLifecycle};

// RenderElement is now in flui_rendering
pub use flui_rendering::{RenderElement, RenderLifecycle, RenderObject};

// Base types (for backward compatibility)
pub use element_base::ElementBase;
pub use element_flags::{AtomicElementFlags, ElementFlags};
pub use lifecycle::ElementLifecycle;
