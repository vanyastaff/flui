//! Testing utilities for gesture and event handling
//!
//! This module provides utilities for testing gesture recognizers and event handling:
//!
//! - [`GestureRecorder`] - Record pointer event sequences
//! - [`GesturePlayer`] - Replay recorded gestures
//! - [`GestureBuilder`] - Pre-built gesture patterns (tap, drag, pinch, etc.)
//! - [`input`] - Builders for creating test events
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::testing::{GestureBuilder, GesturePlayer};
//!
//! // Create a tap gesture
//! let recording = GestureBuilder::tap(Offset::new(100.0, 100.0));
//!
//! // Replay it
//! let player = GesturePlayer::new(recording);
//! for event in player {
//!     recognizer.handle_event(&event);
//! }
//! ```

pub mod input;
mod recording;

pub use recording::{
    GestureBuilder, GesturePlayer, GestureRecorder, GestureRecording, RecordedEvent,
    RecordedEventType,
};

// Re-export input builders
pub use input::{KeyEventBuilder, ModifiersBuilder};
