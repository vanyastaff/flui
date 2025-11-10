//! Gesture recognition and event handling for FLUI
//!
//! This crate provides a comprehensive gesture recognition system inspired by Flutter's
//! gesture system, with proper hit testing, event routing, and gesture conflict resolution.
//!
//! # Architecture
//!
//! ```text
//! PointerEvent → PointerRouter → GestureRecognizers → Callbacks
//!                      ↓
//!                 Hit Testing
//! ```
//!
//! # Modules
//!
//! - [`recognizers`] - Gesture recognizers (Tap, Drag, Scale, etc.)
//! - [`pointer_router`] - Event dispatch and routing
//! - [`detector`] - GestureDetector widget
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_gestures::prelude::*;
//!
//! GestureDetector::builder()
//!     .on_tap(|| println!("Tapped!"))
//!     .child(Container::new())
//!     .build()
//! ```

pub mod arena;
pub mod detector;
pub mod recognizers;

// Re-export key types at crate root for convenience
pub use arena::{GestureArena, GestureArenaMember, GestureDisposition, PointerId};
pub use detector::GestureDetector;
pub use recognizers::{
    DragGestureRecognizer, GestureRecognizer, LongPressGestureRecognizer,
    ScaleGestureRecognizer, TapGestureRecognizer,
};


pub mod prelude {
    //! Commonly used types and traits
    pub use crate::arena::*;
    pub use crate::detector::*;
    pub use crate::recognizers::{
        drag::*, long_press::*, scale::*, tap::*, DragGestureRecognizer,
        LongPressGestureRecognizer, ScaleGestureRecognizer, TapGestureRecognizer,
    };
}

