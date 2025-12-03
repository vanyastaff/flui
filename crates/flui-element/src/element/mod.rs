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
mod render_element;
mod view_element;

// Core element enum
pub use element::Element;

// Element variants
pub use render_element::{RenderElement, RenderObjectTrait};
pub use view_element::ViewElement;

// Base types
pub use element_base::ElementBase;
pub use element_flags::{AtomicElementFlags, ElementFlags};
pub use lifecycle::ElementLifecycle;
