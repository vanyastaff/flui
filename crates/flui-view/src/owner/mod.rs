//! Build phase management.
//!
//! This module provides:
//! - [`BuildOwner`] - Manages dirty elements and build scheduling

mod build_owner;

pub use build_owner::BuildOwner;
