//! Element module - Core element types for FLUI
//!
//! This module provides the Element struct and related types for managing
//! the element tree in FLUI applications.

mod element;
mod element_base;
mod element_flags;
mod lifecycle;

pub use element::Element;
pub use element_base::ElementBase;
pub use element_flags::{AtomicElementFlags, ElementFlags};
pub use lifecycle::ElementLifecycle;
