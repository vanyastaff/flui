//! Box protocol render objects (2D cartesian layout).
//!
//! Box render objects use `BoxConstraints` and `Size` for layout,
//! providing 2D positioning within rectangular bounds.
//!
//! # Categories
//!
//! - [`basic`]: Simple single-child modifications (Padding, Align, etc.)
//! - [`effects`]: Visual effects and transformations (Opacity, Transform, Clip*, etc.)
//! - [`layout`]: Multi-child layout algorithms (Flex, Stack, Wrap)

pub mod basic;
pub mod effects;
pub mod layout;

// TODO: Add remaining categories
// pub mod animation;
// pub mod interaction;
// pub mod gestures;
// pub mod media;
// pub mod text;
// pub mod accessibility;
// pub mod platform;
// pub mod scroll;
// pub mod debug;
