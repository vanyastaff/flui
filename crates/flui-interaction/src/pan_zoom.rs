//! Trackpad pan/zoom pointer events.
//!
//! Flutter 3.27+ exposes a single trackpad gesture source as three distinct
//! [`PointerPanZoomEvent`] variants — `Start`, `Update`, `End` — each carrying
//! the information its stage needs. The `Update` variant carries the running
//! pan offset, the per-event pan delta, the cumulative scale, and the
//! cumulative rotation in radians.
//!
//! Upstream `ui_events::PointerEvent::Gesture` is too coarse: its
//! [`ui_events::pointer::PointerGesture`] enum holds only `Pinch(f32)` and
//! `Rotate(f32)`, dropping the pan delta entirely and folding `Pinch` into
//! `scale` semantics. That collapser makes it impossible for
//! `PanGestureRecognizer` (which needs the pan delta) and a trackpad-aware
//! `ScaleGestureRecognizer` (which needs both pan and scale) to coexist
//! against the same event stream.
//!
//! This module introduces a Flutter-flavoured sum type that:
//!
//! - is consumed by the gesture recognizer layer (no W3C enum unpacking in
//!   recognizer code),
//! - carries the full Update payload (pan, pan delta, scale, rotation) so a
//!   recognizer can read what it actually needs,
//! - converts from upstream `ui_events::PointerEvent::Gesture` (or its
//!   underlying [`ui_events::pointer::PointerGesture`]) at the routing
//!   boundary, keeping the W3C enum un-touched downstream.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::pan_zoom::PointerPanZoomEvent;
//! use ui_events::pointer::{PointerEvent, PointerGesture, PointerGestureEvent, PointerInfo,
//!     PointerState, PointerButtons};
//!
//! let event = PointerPanZoomEvent::Update {
//!     pointer_id: 1,
//!     position: Offset::new(px(50.0), px(60.0)),
//!     pan: Offset::new(px(10.0), px(0.0)),
//!     pan_delta: Offset::new(px(2.0), px(0.0)),
//!     scale: 1.0,
//!     rotation: 0.0,
//!     timestamp_nanos: 1_000,
//!     device_kind: PointerDeviceKind::Trackpad,
//! };
//!
//! if let PointerPanZoomEvent::Update { pan_delta, .. } = event {
//!     // recognizer sees the running pan delta
//! }
//! ```
//!
//! Flutter reference:
//! <https://api.flutter.dev/flutter/gestures/PointerPanZoomEvent-class.html>

use flui_types::geometry::{Offset, Pixels};
use flui_types::gestures::PointerDeviceKind;
use ui_events::pointer::PointerEvent;

use crate::ids::PointerId;

/// Truncate a `f64` to `f32`.
///
/// Lossless for any screen-pixel coordinate: a `f32` mantissa rounds at
/// ~7 decimal digits and physical pointer positions are reported in
/// device pixels (≤ 2^23 ≈ 8M), so `f64 → f32` is exact in that range.
/// Used at the W3C→flui boundary where upstream carries `f64` physical
/// pixels and our `Offset<Pixels>` stores `f32`. Truncation can only
/// occur for synthetic values (test fixtures, NaN propagation handled
/// by [`f32::is_finite`] checks upstream).
#[inline]
fn px_f32(v: f64) -> Pixels {
    // f64 → f32 is intentionally lossy at extreme values; for pointer
    // coordinates the dynamic range fits in `f32` exactly. This is the
    // single canonical W3C→flui downcast site for pointer positions.
    Pixels(v as f32)
}

// ============================================================================
// PointerPanZoomEvent
// ============================================================================

/// A trackpad pan/zoom pointer event.
///
/// Sum type over three Flutter-aligned stages. The `Start` and `End` stages
/// only carry pointer identity, current position, and a wall-clock
/// timestamp; the `Update` stage additionally carries the cumulative pan
/// offset, the per-event pan delta, the cumulative scale (1.0 = no zoom),
/// and the cumulative rotation in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum PointerPanZoomEvent {
    /// Trackpad pan/zoom began on this pointer.
    ///
    /// Mirrors Flutter's `PointerPanZoomStartEvent`. Carries no
    /// pan/scale/rotation deltas — those are introduced in [`Self::Update`].
    Start {
        /// Stable pointer id (primary for the only pointer in a trackpad
        /// gesture).
        pointer_id: PointerId,
        /// Current pointer position in global coordinates.
        position: Offset<Pixels>,
        /// Wall-clock timestamp in nanoseconds. Monotonic relative to
        /// `PointerState::time` (u64 ns).
        timestamp_nanos: u64,
        /// Always `PointerDeviceKind::Trackpad`. Repeated on every
        /// variant so recognizers can read the device without matching
        /// on the upstream W3C enum.
        device_kind: PointerDeviceKind,
    },

    /// Trackpad pan/zoom update on this pointer.
    ///
    /// Mirrors Flutter's `PointerPanZoomUpdateEvent`. Carries the
    /// cumulative `pan` offset, the per-event `pan_delta`, the cumulative
    /// `scale` (1.0 = identity), and the cumulative `rotation` in radians.
    Update {
        /// Stable pointer id.
        pointer_id: PointerId,
        /// Current pointer position in global coordinates.
        position: Offset<Pixels>,
        /// Cumulative pan offset since the `Start`.
        pan: Offset<Pixels>,
        /// Pan offset change since the previous `Update` event.
        pan_delta: Offset<Pixels>,
        /// Cumulative scale factor since the `Start`. `1.0` = no zoom,
        /// `> 1.0` = zoomed in, `< 1.0` = zoomed out.
        scale: f64,
        /// Cumulative rotation in radians since the `Start`.
        rotation: f64,
        /// Wall-clock timestamp in nanoseconds.
        timestamp_nanos: u64,
        /// Always `PointerDeviceKind::Trackpad`.
        device_kind: PointerDeviceKind,
    },

    /// Trackpad pan/zoom ended on this pointer.
    ///
    /// Mirrors Flutter's `PointerPanZoomEndEvent`. Carries the final
    /// pointer position only — final pan/scale/rotation are zero by
    /// convention (the gesture is over).
    End {
        /// Stable pointer id.
        pointer_id: PointerId,
        /// Final pointer position in global coordinates.
        position: Offset<Pixels>,
        /// Wall-clock timestamp in nanoseconds.
        timestamp_nanos: u64,
        /// Always `PointerDeviceKind::Trackpad`.
        device_kind: PointerDeviceKind,
    },
}

impl PointerPanZoomEvent {
    /// Returns the pointer id for any variant.
    #[inline]
    #[must_use]
    pub const fn pointer_id(&self) -> PointerId {
        match *self {
            Self::Start { pointer_id, .. }
            | Self::Update { pointer_id, .. }
            | Self::End { pointer_id, .. } => pointer_id,
        }
    }

    /// Returns the current pointer position for any variant.
    #[inline]
    #[must_use]
    pub const fn position(&self) -> Offset<Pixels> {
        match *self {
            Self::Start { position, .. }
            | Self::Update { position, .. }
            | Self::End { position, .. } => position,
        }
    }

    /// Returns the wall-clock timestamp (nanoseconds) for any variant.
    #[inline]
    #[must_use]
    pub const fn timestamp_nanos(&self) -> u64 {
        match *self {
            Self::Start {
                timestamp_nanos, ..
            }
            | Self::Update {
                timestamp_nanos, ..
            }
            | Self::End {
                timestamp_nanos, ..
            } => timestamp_nanos,
        }
    }

    /// Returns the device kind. Always [`PointerDeviceKind::Trackpad`].
    #[inline]
    #[must_use]
    pub const fn device_kind(&self) -> PointerDeviceKind {
        match *self {
            Self::Start { device_kind, .. }
            | Self::Update { device_kind, .. }
            | Self::End { device_kind, .. } => device_kind,
        }
    }

    /// Returns `true` for [`Self::Start`].
    #[inline]
    pub const fn is_start(&self) -> bool {
        matches!(self, Self::Start { .. })
    }

    /// Returns `true` for [`Self::Update`].
    #[inline]
    pub const fn is_update(&self) -> bool {
        matches!(self, Self::Update { .. })
    }

    /// Returns `true` for [`Self::End`].
    #[inline]
    pub const fn is_end(&self) -> bool {
        matches!(self, Self::End { .. })
    }
}

// ============================================================================
// Conversion from upstream W3C event
// ============================================================================

/// Convert upstream [`PointerEvent::Gesture`] to a Flutter-flavoured
/// [`PointerPanZoomEvent`].
///
/// Returns `None` for any non-`Gesture` upstream event.
///
/// # Conversion rules
///
/// The upstream `ui_events::pointer::PointerGesture` carries only
/// `Pinch(f32)` and `Rotate(f32)` deltas. The pan delta is dropped at the
/// transport layer (no upstream field exists). To preserve recognizer
/// fidelity we synthesize a zero pan/pan_delta on the output — recognizers
/// that need a real pan delta should consume the upstream
/// `PointerScrollEvent` (trackpad two-finger scroll) or a richer transport
/// when one becomes available. This conversion is a *type-level*
/// un-collapse, not a magic source of pan data.
///
/// `Pinch` maps to `scale = 1.0 + pinch` (Flutter's `PointerPanZoomUpdateEvent.scale`
/// semantics). `Rotate` passes through as the cumulative rotation in
/// radians. The `Start` / `End` transition is signalled by the upstream
/// `PointerButtons` state (pressed vs released) which on most platforms
/// is *not* a reliable indicator for trackpad gestures — so the default
/// mapping emits [`PointerPanZoomEvent::Update`] for every gesture tick.
/// For boundary detection (real Start/End) use a higher-level binding
/// that tracks when the trackpad finger lands / lifts.
#[inline]
pub fn from_w3c_event(event: &PointerEvent) -> Option<PointerPanZoomEvent> {
    let PointerEvent::Gesture(gesture) = event else {
        return None;
    };
    Some(convert_gesture(gesture))
}

/// Convert a single upstream [`ui_events::pointer::PointerGestureEvent`]
/// into a [`PointerPanZoomEvent::Update`].
///
/// Use this when the caller has already pattern-matched on
/// `PointerEvent::Gesture` and wants a direct mapping. See
/// [`from_w3c_event`] for the upstream-event entry point and the
/// caveats around `Start` / `End` detection.
#[inline]
pub fn convert_gesture(event: &ui_events::pointer::PointerGestureEvent) -> PointerPanZoomEvent {
    use ui_events::pointer::PointerGesture;
    let state = &event.state;
    let position = Offset::new(px_f32(state.position.x), px_f32(state.position.y));
    // `ui_events::PointerId` wraps a `NonZeroU64`; the only fallible step is
    // the `new(u64)` constructor (rejects 0). Round-trip through the inner
    // value so we never call `new(0).expect(...)` on borrowed data.
    let pointer_id = event
        .pointer
        .pointer_id
        .and_then(|nz| crate::ids::PointerId::new(nz.get_inner().get()))
        .unwrap_or(crate::ids::PointerId::PRIMARY);
    let (scale, rotation) = match event.gesture {
        PointerGesture::Pinch(pinch) => (1.0_f64 + f64::from(pinch), 0.0_f64),
        PointerGesture::Rotate(rot) => (1.0_f64, f64::from(rot)),
    };
    PointerPanZoomEvent::Update {
        pointer_id,
        position,
        // Pan data is dropped at the transport layer (no upstream field).
        // Synthesise zero so the Update variant stays structurally
        // well-formed; recognizers reading `pan` / `pan_delta` from a
        // single Gesture event will see a zero delta (one-event signal,
        // not a real gesture stream). Use `PointerScrollEvent` for real
        // two-finger trackpad scroll deltas.
        pan: Offset::ZERO,
        pan_delta: Offset::ZERO,
        scale,
        rotation,
        timestamp_nanos: state.time,
        device_kind: PointerDeviceKind::Trackpad,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Modifiers;
    use ui_events::pointer::{
        ContactGeometry, PersistentDeviceId, PointerButtons, PointerGesture, PointerGestureEvent,
        PointerInfo, PointerOrientation, PointerState, PointerType,
    };

    /// Build a `PointerGestureEvent` for tests.
    fn make_gesture(
        pointer_id: u64,
        gesture: PointerGesture,
        x: f64,
        y: f64,
    ) -> PointerGestureEvent {
        PointerGestureEvent {
            pointer: PointerInfo {
                pointer_id: crate::ids::PointerId::new(pointer_id),
                pointer_type: PointerType::Touch,
                persistent_device_id: PersistentDeviceId::new(1),
            },
            gesture,
            state: PointerState {
                time: 1_000,
                position: dpi::PhysicalPosition::new(x, y),
                buttons: PointerButtons::new(),
                modifiers: Modifiers::empty(),
                count: 0,
                contact_geometry: ContactGeometry {
                    width: 1.0,
                    height: 1.0,
                },
                orientation: PointerOrientation::default(),
                pressure: 0.0,
                tangential_pressure: 0.0,
                scale_factor: 1.0,
            },
        }
    }

    #[test]
    fn pinch_maps_to_scale_minus_one() {
        // Pinch = 0.2 → scale = 1.2
        let gesture = make_gesture(1, PointerGesture::Pinch(0.2), 10.0, 20.0);
        let ev = convert_gesture(&gesture);
        match ev {
            PointerPanZoomEvent::Update {
                scale, rotation, ..
            } => {
                assert!((scale - 1.2).abs() < 1e-6, "scale={}", scale);
                assert_eq!(rotation, 0.0);
            }
            _ => panic!("expected Update, got {:?}", ev),
        }
    }

    #[test]
    fn pinch_negative_zooms_out() {
        // Pinch = -0.1 → scale = 0.9
        let gesture = make_gesture(1, PointerGesture::Pinch(-0.1), 0.0, 0.0);
        let ev = convert_gesture(&gesture);
        match ev {
            PointerPanZoomEvent::Update { scale, .. } => {
                assert!((scale - 0.9).abs() < 1e-6, "scale={}", scale);
            }
            _ => panic!("expected Update"),
        }
    }

    #[test]
    fn rotate_passes_through_in_radians() {
        // Rotate = π/4 → rotation = π/4
        let gesture = make_gesture(
            1,
            PointerGesture::Rotate(core::f32::consts::FRAC_PI_4),
            0.0,
            0.0,
        );
        let ev = convert_gesture(&gesture);
        match ev {
            PointerPanZoomEvent::Update {
                rotation, scale, ..
            } => {
                assert!((rotation - core::f64::consts::FRAC_PI_4).abs() < 1e-6);
                assert_eq!(scale, 1.0);
            }
            _ => panic!("expected Update"),
        }
    }

    #[test]
    fn pan_delta_is_zero_synthesised() {
        // No upstream pan data → zero pan/pan_delta in output.
        let gesture = make_gesture(1, PointerGesture::Pinch(0.0), 50.0, 60.0);
        let ev = convert_gesture(&gesture);
        match ev {
            PointerPanZoomEvent::Update { pan, pan_delta, .. } => {
                assert_eq!(pan, Offset::ZERO);
                assert_eq!(pan_delta, Offset::ZERO);
            }
            _ => panic!("expected Update"),
        }
    }

    #[test]
    fn position_propagates() {
        let gesture = make_gesture(1, PointerGesture::Pinch(0.0), 12.5, 34.5);
        let ev = convert_gesture(&gesture);
        let pos = ev.position();
        assert_eq!(pos.dx, Pixels(12.5));
        assert_eq!(pos.dy, Pixels(34.5));
    }

    #[test]
    fn pointer_id_propagates() {
        // NonZeroU64 id 7 → our PointerId (which is also NonZeroU64-backed).
        let gesture = make_gesture(7, PointerGesture::Pinch(0.0), 0.0, 0.0);
        let ev = convert_gesture(&gesture);
        assert_eq!(ev.pointer_id().get_inner().get(), 7);
    }

    #[test]
    fn timestamp_propagates() {
        let gesture = make_gesture(1, PointerGesture::Pinch(0.0), 0.0, 0.0);
        let ev = convert_gesture(&gesture);
        assert_eq!(ev.timestamp_nanos(), 1_000);
    }

    #[test]
    fn device_kind_is_trackpad() {
        // Whatever upstream PointerType is, we tag output as Trackpad —
        // we only convert from PointerEvent::Gesture, which by Flutter
        // convention originates from a trackpad.
        let gesture = make_gesture(1, PointerGesture::Pinch(0.0), 0.0, 0.0);
        let ev = convert_gesture(&gesture);
        assert_eq!(ev.device_kind(), PointerDeviceKind::Trackpad);
    }

    #[test]
    fn from_w3c_event_gesture_path() {
        let gesture = make_gesture(1, PointerGesture::Pinch(0.1), 5.0, 6.0);
        let up = PointerEvent::Gesture(gesture);
        let out = from_w3c_event(&up).expect("Gesture event converts");
        assert!(out.is_update());
    }

    #[test]
    fn from_w3c_event_rejects_non_gesture() {
        // PointerEvent::Down must NOT convert — only Gesture does.
        use ui_events::pointer::{PointerButton, PointerButtonEvent};
        let down = PointerEvent::Down(PointerButtonEvent {
            button: Some(PointerButton::Primary),
            pointer: PointerInfo {
                pointer_id: Some(crate::ids::PointerId::PRIMARY),
                pointer_type: PointerType::Mouse,
                persistent_device_id: None,
            },
            state: PointerState::default(),
        });
        assert!(from_w3c_event(&down).is_none());
    }

    #[test]
    fn missing_pointer_id_falls_back_to_primary() {
        // Upstream `pointer_id: None` → our PRIMARY sentinel.
        let mut gesture = make_gesture(1, PointerGesture::Pinch(0.0), 0.0, 0.0);
        gesture.pointer.pointer_id = None;
        let ev = convert_gesture(&gesture);
        assert!(ev.pointer_id().is_primary_pointer());
    }

    #[test]
    fn is_start_update_end_match() {
        let start = PointerPanZoomEvent::Start {
            pointer_id: PointerId::PRIMARY,
            position: Offset::ZERO,
            timestamp_nanos: 0,
            device_kind: PointerDeviceKind::Trackpad,
        };
        let update = PointerPanZoomEvent::Update {
            pointer_id: PointerId::PRIMARY,
            position: Offset::ZERO,
            pan: Offset::ZERO,
            pan_delta: Offset::ZERO,
            scale: 1.0,
            rotation: 0.0,
            timestamp_nanos: 0,
            device_kind: PointerDeviceKind::Trackpad,
        };
        let end = PointerPanZoomEvent::End {
            pointer_id: PointerId::PRIMARY,
            position: Offset::ZERO,
            timestamp_nanos: 0,
            device_kind: PointerDeviceKind::Trackpad,
        };
        assert!(start.is_start() && !start.is_update() && !start.is_end());
        assert!(!update.is_start() && update.is_update() && !update.is_end());
        assert!(!end.is_start() && !end.is_update() && end.is_end());
    }
}
