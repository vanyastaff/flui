//! Gesture recognizers
//!
//! Recognizers analyze pointer event streams and detect specific gestures.
//!
//! # Available Recognizers
//!
//! - [`TapGestureRecognizer`] - Single tap detection
//! - [`DoubleTapGestureRecognizer`] - Double tap detection
//! - [`LongPressGestureRecognizer`] - Long press detection
//! - [`DragGestureRecognizer`] - Drag/pan gesture detection
//! - [`ScaleGestureRecognizer`] - Pinch-to-zoom detection
//! - [`MultiTapGestureRecognizer`] - Multi-finger tap detection
//! - [`ForcePressGestureRecognizer`] - Force/pressure touch detection
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::prelude::*;
//!
//! let arena = GestureArena::new();
//! let recognizer = TapGestureRecognizer::new(arena)
//!     .with_on_tap(|details| {
//!         println!("Tapped at {:?}", details.global_position);
//!     });
//! ```

pub mod double_tap;
pub mod drag;
pub mod force_press;
pub mod long_press;
pub mod multi_tap;
pub mod recognizer;
pub mod scale;
pub mod tap;

pub use double_tap::DoubleTapGestureRecognizer;
pub use drag::DragGestureRecognizer;
pub use force_press::ForcePressGestureRecognizer;
pub use long_press::LongPressGestureRecognizer;
pub use multi_tap::MultiTapGestureRecognizer;
pub use recognizer::{constants, GestureRecognizer, GestureRecognizerState, GestureState};
pub use scale::ScaleGestureRecognizer;
pub use tap::TapGestureRecognizer;
