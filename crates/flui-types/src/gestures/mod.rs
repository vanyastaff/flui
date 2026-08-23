//! Gesture detail types
//!
//! This module provides the recognizer-independent gesture vocabulary:
//! pointer/velocity primitives plus the tap, long-press move/end, and
//! force-press detail payloads. Drag, scale, and long-press down/start
//! details are defined by their recognizers in `flui-interaction` — see
//! the [`details`] module docs for why.

pub mod details;
pub mod pointer;
pub mod velocity;

pub use details::{
    ForcePressDetails, LongPressEndDetails, LongPressMoveUpdateDetails, TapDownDetails,
    TapUpDetails,
};
pub use pointer::{OffsetPair, PointerData, PointerDeviceKind};
pub use velocity::{Velocity, VelocityEstimate};
