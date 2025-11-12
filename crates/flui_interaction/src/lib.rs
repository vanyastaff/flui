//! Event routing and interaction handling for FLUI
//!
//! This crate provides the event handling infrastructure for FLUI:
//! - **EventRouter**: Routes pointer/keyboard events via hit testing
//! - **HitTest**: Determines which UI elements are under cursor/touch
//! - **FocusManager**: Manages keyboard focus (global singleton)
//!
//! # Architecture
//!
//! ```text
//! Platform (winit, Win32, etc.)
//!     ↓
//! PointerEvent/KeyEvent (flui_types)
//!     ↓
//! EventRouter (this crate)
//!     ├─ Hit Testing (spatial)
//!     └─ Focus Management (keyboard)
//!         ↓
//! Handlers (closures in Layers)
//!     ↓
//! GestureRecognizers (flui_gestures)
//!     ↓
//! User code (Signal::update, etc.)
//! ```
//!
//! # Example: Basic Event Routing
//!
//! ```rust,ignore
//! use flui_interaction::{EventRouter, HitTestable};
//! use flui_types::events::{Event, PointerEvent};
//!
//! let mut router = EventRouter::new();
//!
//! // Register a layer with hit testing
//! let layer = MyLayer { bounds: Rect::from_xywh(0.0, 0.0, 100.0, 100.0) };
//!
//! // Route pointer event
//! let event = PointerEvent::Down { position: Offset::new(50.0, 50.0), ... };
//! router.route_event(&mut layer, &Event::Pointer(event));
//! ```
//!
//! # Example: Keyboard Focus
//!
//! ```rust,ignore
//! use flui_interaction::FocusManager;
//!
//! let focus_id = FocusNodeId::new(1);
//!
//! // Request focus
//! FocusManager::global().request_focus(focus_id);
//!
//! // Check focus
//! if FocusManager::global().has_focus(focus_id) {
//!     println!("We have focus!");
//! }
//! ```
//!
//! # Separation from Rendering
//!
//! This crate is deliberately separate from `flui_engine` (rendering):
//! - ✅ Can test event logic without GPU
//! - ✅ Can use rendering without event handling (headless)
//! - ✅ Clear separation of concerns (SOLID principles)
//! - ✅ Smaller compile times and dependencies

pub mod event_router;
pub mod focus_manager;
pub mod hit_test;
pub mod input;

// Re-export main types
pub use event_router::EventRouter;
pub use focus_manager::{FocusManager, FocusNodeId};
pub use hit_test::{HitTestEntry, HitTestResult, HitTestable};

// Re-export common types from flui_types for convenience
pub use flui_types::events::{Event, KeyEvent, PointerEvent};
pub use flui_types::geometry::{Offset, Rect};