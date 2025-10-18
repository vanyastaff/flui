//! Gesture detail types
//!
//! This module provides types that describe gesture events and their details,
//! such as tap, drag, scale, and long press gestures.

pub mod details;
pub mod pointer;
pub mod velocity;

pub use details::{
    DragDownDetails, DragEndDetails, DragStartDetails, DragUpdateDetails, ForcePressDetails,
    LongPressDownDetails, LongPressEndDetails, LongPressMoveUpdateDetails, LongPressStartDetails,
    ScaleEndDetails, ScaleStartDetails, ScaleUpdateDetails, TapDownDetails, TapUpDetails,
};
pub use pointer::{OffsetPair, PointerData, PointerDeviceKind};
pub use velocity::{Velocity, VelocityEstimate};
