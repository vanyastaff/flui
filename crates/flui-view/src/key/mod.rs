//! Key system for element identity and reparenting.
//!
//! This module provides:
//! - [`GlobalKey`] - Global keys for cross-tree element lookup
//! - [`ValueKey`] - Value-based keys for list reconciliation
//! - [`ObjectKey`] - Identity-based unique keys

mod global_key;

pub use global_key::{GlobalKey, GlobalKeyId, ObjectKey, ValueKey};
