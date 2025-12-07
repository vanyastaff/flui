//! Platform event conversions
//!
//! Converts platform-specific events (winit, Android, iOS, Web) to FLUI's unified event types.

pub mod keyboard;

pub use keyboard::{convert_key_event, convert_modifiers};
