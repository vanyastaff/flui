//! Per-frame rendering state and command encoding
//!
//! Manages the frame lifecycle: begin frame, encode draw commands,
//! submit to GPU. Replaces inline frame logic from `WgpuPainter`.

pub mod state_stack;
pub mod submission;
pub mod encoder;
pub mod dispatch;
