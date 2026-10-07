//! Trackpad pan/zoom pointer events.
//!
//! A single trackpad gesture source is exposed as three distinct
//! [`PointerPanZoomEvent`] variants — `Start`, `Update`, `End` — each carrying
//! the information its stage needs. The `Update` variant carries the running
//! pan offset, the per-event pan delta, the per-event scale factor, and the
//! per-event rotation in radians.
//!
//! Upstream `ui_events::PointerEvent::Gesture` is too coarse: its
//! [`ui_events::pointer::PointerGesture`] enum holds only `Pinch(f64)` and
//! `Rotate(f64)`, dropping the pan delta entirely and folding `Pinch` into
//! `scale` semantics. That collapser makes it impossible for
//! `PanGestureRecognizer` (which needs the pan delta) and a trackpad-aware
//! `ScaleGestureRecognizer` (which needs both pan and scale) to coexist
//! against the same event stream.
//!
//! This module introduces a sum type that:
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
//!     position: Offset::new(50.0, 60.0),
//!     pan: Offset::new(10.0, 0.0),
//!     pan_delta: Offset::new(2.0, 0.0),
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

use crate::PointerDeviceKind;
use flui_foundation::geometry::Offset;
use ui_events::pointer::PointerEvent;

use crate::ids::PointerId;

/// Truncate a `f64` to `f64`.
///
/// Lossless for any screen-pixel coordinate: a `f64` mantissa rounds at
/// ~7 decimal digits and physical pointer positions are reported in
/// device pixels (≤ 2^23 ≈ 8M), so `f64 → f64` is exact in that range.
/// Used at the W3C→flui boundary where upstream carries `f64` physical
/// pixels and our `Offset` stores `f64`. Truncation can only
/// occur for synthetic values (test fixtures, NaN propagation handled
/// by [`f64::is_finite`] checks upstream).
#[inline]
fn px_f32(v: f64) -> f64 {
    // f64 → f64 is intentionally lossy at extreme values; for pointer
    // coordinates the dynamic range fits in `f64` exactly. This is the
    // single canonical W3C→flui downcast site for pointer positions.
    v
}

// ============================================================================
// PointerPanZoomEvent
// ============================================================================

/// A trackpad pan/zoom pointer event.
///
/// Sum type over three stages. The `Start` and `End` stages
/// only carry pointer identity, current position, and a wall-clock
/// timestamp; the `Update` stage additionally carries the cumulative pan
/// offset, the per-event pan delta, the per-event scale factor (1.0 = no zoom),
/// and the per-event rotation in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum PointerPanZoomEvent {
    /// Trackpad pan/zoom began on this pointer.
    ///
    /// Carries no
    /// pan/scale/rotation deltas — those are introduced in [`Self::Update`].
    Start {
        /// Stable pointer id (primary for the only pointer in a trackpad
        /// gesture).
        pointer_id: PointerId,
        /// Current pointer position in global coordinates.
        position: Offset<f64>,
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
    /// Carries the
    /// cumulative `pan` offset, the per-event `pan_delta`, and the per-event
    /// `scale` factor (1.0 = identity) and `rotation` in radians — each tick's
    /// change, which a consumer multiplies / adds into its own transform.
    Update {
        /// Stable pointer id.
        pointer_id: PointerId,
        /// Current pointer position in global coordinates.
        position: Offset<f64>,
        /// Cumulative pan offset since the `Start`.
        pan: Offset<f64>,
        /// Pan offset change since the previous `Update` event.
        pan_delta: Offset<f64>,
        /// Scale factor of this tick relative to the previous one. `1.0` =
        /// no change, `> 1.0` = zooming in, `< 1.0` = zooming out. Always
        /// finite and positive.
        scale: f64,
        /// Rotation of this tick in radians relative to the previous one.
        /// Always finite.
        rotation: f64,
        /// Wall-clock timestamp in nanoseconds.
        timestamp_nanos: u64,
        /// Always `PointerDeviceKind::Trackpad`.
        device_kind: PointerDeviceKind,
    },

    /// Trackpad pan/zoom ended on this pointer.
    ///
    /// Carries the final
    /// pointer position only — final pan/scale/rotation are zero by
    /// convention (the gesture is over).
    End {
        /// Stable pointer id.
        pointer_id: PointerId,
        /// Final pointer position in global coordinates.
        position: Offset<f64>,
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
    pub const fn position(&self) -> Offset<f64> {
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

/// Convert upstream [`PointerEvent::Gesture`] to a
/// [`PointerPanZoomEvent`].
///
/// Returns `None` for any non-`Gesture` upstream event.
///
/// # Conversion rules
///
/// The upstream `ui_events::pointer::PointerGesture` carries only
/// `Pinch(f64)` and `Rotate(f64)` deltas. The pan delta is dropped at the
/// transport layer (no upstream field exists). To preserve recognizer
/// fidelity we synthesize a zero pan/pan_delta on the output — recognizers
/// that need a real pan delta should consume the upstream
/// `PointerScrollEvent` (trackpad two-finger scroll) or a richer transport
/// when one becomes available. This conversion is a *type-level*
/// un-collapse, not a magic source of pan data.
///
/// `Pinch` maps to the per-tick `scale = 1.0 + pinch`. `Rotate` passes
/// through as the per-tick rotation in radians. A tick that is not a usable
/// zoom factor (non-finite, or `pinch <= -1`) or a non-finite rotation
/// becomes the identity (`scale = 1.0`, `rotation = 0.0`). The `Start` / `End` transition is signalled by the upstream
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
    // Both are per-tick deltas upstream. A tick that cannot be a zoom factor
    // (NaN, infinite, or `pinch <= -1`, which would scale to zero or flip
    // the content) or a non-finite rotation is published as "no change"
    // rather than poisoning every consumer's accumulated transform.
    let (scale, rotation) = match event.gesture {
        PointerGesture::Pinch(pinch) => {
            let scale = 1.0_f64 + f64::from(pinch);
            let scale = if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            };
            (scale, 0.0_f64)
        }
        PointerGesture::Rotate(rot) => {
            let rot = f64::from(rot);
            (1.0_f64, if rot.is_finite() { rot } else { 0.0 })
        }
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
