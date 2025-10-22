//! Core rendering infrastructure
//!
//! This module contains the foundational traits and types for the rendering layer.

pub mod box_protocol;
pub mod flags;
pub mod render_object;


pub use render_object::RenderObject;
pub use box_protocol::{RenderBox, RenderProxyBox};
pub use flags::RenderFlags;

