//! Gesture recognizers
//!
//! Recognizers analyze pointer event streams and detect specific gestures.
//!
//! # Architecture
//!
//! ```text
//! GestureArenaMember (trait)
//!     │
//!     └── GestureRecognizer (trait) - base with add_pointer, handle_event
//!             │
//!             └── Concrete Recognizers
//!                 ├── TapGestureRecognizer
//!                 ├── LongPressGestureRecognizer
//!                 ├── DoubleTapGestureRecognizer
//!                 ├── DragGestureRecognizer
//!                 ├── ScaleGestureRecognizer
//!                 └── ...
//! ```
//!
//! Note: The canonical Flutter trait hierarchy
//! `GestureRecognizer ← OneSequenceGestureRecognizer ← PrimaryPointerGestureRecognizer`
//! is re-introduced as proper traits in U13 of the input-frame-loop-repair
//! plan; the zero-consumer scaffolds previously living here were deleted in U3.
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

// Concrete recognizers
pub mod double_tap;
pub mod drag;
pub mod force_press;
pub mod long_press;
pub mod multi_tap;
pub mod one_sequence;
pub mod primary_pointer;
pub mod recognizer;
pub mod scale;
pub mod tap;

// Re-export concrete recognizers
pub use double_tap::DoubleTapGestureRecognizer;
pub use drag::DragGestureRecognizer;
pub use force_press::ForcePressGestureRecognizer;
pub use long_press::LongPressGestureRecognizer;
pub use multi_tap::MultiTapGestureRecognizer;
pub use one_sequence::OneSequenceGestureRecognizer;
pub use primary_pointer::PrimaryPointerGestureRecognizer;
pub use recognizer::{GestureRecognizer, GestureRecognizerState, RecognizerBase, constants};
pub use scale::ScaleGestureRecognizer;
pub use tap::TapGestureRecognizer;
