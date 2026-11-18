//! Input for one FLUI presentation: pointer routing, keyboard focus, text
//! input, gesture recognition, drag and drop and haptics.
//!
//! The crate sits below the element tree and above `flui-platform`, and has
//! no GPU or host edge: everything here runs headless, which is how its tests
//! drive it.
//!
//! - The [gesture binding](binding::GestureBinding) takes a presentation's
//!   pointer stream, hit-tests each new contact through an
//!   [interaction lane](pointer::InteractionLane) and delivers the sequence
//!   along that route.
//! - The [focus manager](focus::FocusManager) owns a presentation's focus
//!   tree: [focus nodes](focus::FocusNode) attached under
//!   [scopes](focus::FocusScopeNode), with traversal order from a
//!   [traversal policy](focus::FocusTraversalPolicy).
//! - The [`gesture`] recognizers compete in a
//!   [gesture arena](gesture::GestureArena), which grants each pointer to one
//!   winner; a recognizer outside this crate implements the same traits as the
//!   built-in ones.
//!
//! Handlers and recognizers live on the owner thread behind `Rc`; what render
//! storage keeps of a route is data-only identity, resolved back to the
//! handlers when a sequence is delivered (ADR-0027).
//!
//! ```text
//! platform input (PointerEvent, KeyEvent)
//!   ├─ GestureBinding ── hit test ── InteractionLane route ── handlers
//!   │                                      └─ recognizers ── GestureArena
//!   └─ FocusManager ── focused path ── key handlers
//! ```
//!
//! # Example: Construct a recogniser and register a tap callback
//!
//! ```rust
//! use flui_interaction::gesture::{GestureArena, TapGestureRecognizer};
//!
//! // 1. The recogniser set lives behind a single shared `GestureArena`.
//! let arena = GestureArena::new();
//!
//! // 2. Configure callbacks before sharing the recognizer as an `Rc`.
//! let recognizer = TapGestureRecognizer::builder(arena)
//!     .on_tap(|details| {
//!         // The user callback fires only after the arena confirms
//!         // this recogniser won (`pending_up` deferral).
//!         let _pos = details.global_position;
//!     })
//!     .build();
//! // `recognizer` is now ready to receive pointer events via
//! // `flui_interaction::binding::GestureBinding` at runtime.
//! ```
//!
//! # Example: Keyboard Focus
//!
//! ```rust
//! use flui_interaction::{focus::{FocusManager, FocusNode}};
//!
//! let manager = FocusManager::new();
//! let node = FocusNode::new();
//! let attachment = manager.root_scope().attach_node(&node)?;
//!
//! // Request focus
//! let _ = node.request_focus();
//!
//! // Check focus
//! if node.has_primary_focus() {
//!     println!("We have focus!");
//! }
//! # Ok::<(), flui_interaction::focus::FocusTreeError>(())
//! ```
//!
//! # Example: Type-Safe IDs
//!
//! ```compile_fail
//! use flui_interaction::{ids::PointerId, focus::FocusNode};
//! fn process_pointer(id: PointerId) {}
//! process_pointer(FocusNode::new().id()); // A focus identity cannot name a pointer.
//! ```
//!
//! # Modules
//!
//! The modules form layers (ADR-0217); a module names only modules below it:
//!
//! 0. [`events`] - The owned input vocabulary; [`ids`] - Type-safe identifiers
//! 1. Close policy and panic containment (private)
//! 2. [`pointer`](mod@pointer) - Hit testing, routing, capture, hover tracking and resampling;
//!    [`focus`] - Keyboard focus and traversal; [`text_input`] - IME and clipboard
//! 3. [`gesture`] - The arena, the recognizers, their settings and details, and
//!    velocity estimation; [`drag`] - Drag-and-drop sessions
//! 4. [`binding`] - the [gesture binding](binding::GestureBinding), which composes them for one presentation
//! 5. [`haptics`] - A presentation's haptics, on the binding's clock and dispatch
//! 6. `testing` - Synthetic event builders (requires `testing` feature). Gesture
//!    scripting and replay live in `flui-testing`, which owns the virtual clock.

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
// Public items keep their contract documentation at the API boundary.
#![deny(missing_docs)]
// Id-keyed tables hash with `rustc_hash` (`clippy.toml`, `disallowed-types`).
#![warn(clippy::disallowed_types)]

// Layer 0: values and identities.
pub mod events;
pub mod ids;

// Layer 1: close policy and containment.
mod close;
mod containment;

// Layer 2: pointer delivery, keyboard focus, text input.
pub mod focus;
pub mod pointer;
pub mod text_input;

// Layer 3: gesture recognition.
pub mod gesture;

// Layer 3: drag-and-drop sessions, over pointer capture and hit testing.
pub mod drag;

// Layer 4: the per-presentation composition root.
pub mod binding;

// Layer 5: a presentation's haptics, on the binding's clock and dispatch.
pub mod haptics;

// Layer 6, above the binding: the runtime's terminal-cleanup seam, and test builders.
#[doc(hidden)]
pub mod __runtime;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

// ============================================================================
// Static Assertions: Send + Sync (data-plane only)
// ============================================================================

/// Compile-time assertions that identity/data types remain Send + Sync.
///
/// Gesture arenas, recognizers, and focus ownership are intentionally
/// owner-local under ADR-0027; executable gesture callbacks must not regain a
/// thread-safe bound through these assertions.
#[cfg(test)]
mod static_assertions {
    use crate::ids::{FocusNodeId, PointerId};
    use crate::pointer::{
        LocalPayloadTarget, PathClipTarget, PointerEventResampler, ScrollTarget, ShaderMaskTarget,
        SmartZoomTarget,
    };

    // Helper trait for static assertions
    #[expect(dead_code)]
    trait AssertSendSync: Send + Sync {}

    // IDs should be Send + Sync (they are Copy)
    impl AssertSendSync for PointerId {}
    impl AssertSendSync for FocusNodeId {}
    impl AssertSendSync for ScrollTarget {}
    impl AssertSendSync for SmartZoomTarget {}
    impl AssertSendSync for PathClipTarget {}
    impl AssertSendSync for ShaderMaskTarget {}
    impl AssertSendSync for LocalPayloadTarget {}

    impl AssertSendSync for PointerEventResampler {}
}
