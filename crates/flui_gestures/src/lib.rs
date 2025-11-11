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
    DoubleTapGestureRecognizer, DragGestureRecognizer, GestureRecognizer,
    LongPressGestureRecognizer, MultiTapGestureRecognizer, ScaleGestureRecognizer,
    TapGestureRecognizer,
};

pub mod prelude {
    //! Commonly used types and traits
    #[allow(ambiguous_glob_reexports)]
    pub use crate::arena::*;
    #[allow(ambiguous_glob_reexports)]
    pub use crate::detector::*;
    pub use crate::recognizers::{
        double_tap::*, drag::*, long_press::*, multi_tap::*, scale::*, tap::*,
        DoubleTapGestureRecognizer, DragGestureRecognizer, LongPressGestureRecognizer,
        MultiTapGestureRecognizer, ScaleGestureRecognizer, TapGestureRecognizer,
    };
}
