//! The phase grammar every backend's scrolling shares (ADR-0217 §4).
//!
//! A two-finger trackpad pan is a scroll with [`ScrollPhase`]s on every OS,
//! and platform momentum is `ScrollPhase::Momentum*`, whatever the native
//! event family (AppKit `scrollWheel:` phases, winit `MouseWheel` and
//! `PanGesture` touch phases). Backends report the native phases as plain
//! [`ScrollReading`]s; [`ScrollPhases`] turns them into the owned phase.

use flui_platform_api::EventTime;
use flui_platform_api::keyboard::Modifiers;
use flui_platform_api::pointer::{
    PointerEvent, ScrollDelta, ScrollEvent, ScrollPhase, ScrollPrecision,
};

use super::{ContactRegistry, GesturePhase, NativeContact, NativeDevice, NativeSurface};
use crate::traits::PlatformInput;

/// A scroll event's native phases, as plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScrollReading {
    /// The fingers' gesture phase, when the event is part of one.
    pub fingers: Option<GesturePhase>,
    /// The platform's momentum phase, when the event is momentum.
    pub momentum: Option<GesturePhase>,
    /// Whether the delta is precise (pixels) rather than notched.
    pub precise: bool,
}

impl ScrollReading {
    /// A reading in the fingers' `phase`.
    #[must_use]
    pub const fn fingers(phase: GesturePhase, precise: bool) -> Self {
        Self {
            fingers: Some(phase),
            momentum: None,
            precise,
        }
    }

    /// A momentum reading in `phase`.
    #[must_use]
    pub const fn momentum(phase: GesturePhase) -> Self {
        Self {
            fingers: None,
            momentum: Some(phase),
            precise: true,
        }
    }
}

/// The finger scroll gestures open per surface and device.
#[derive(Debug, Clone, Default)]
pub struct ScrollPhases {
    open: Vec<(NativeSurface, Option<NativeDevice>)>,
}

impl ScrollPhases {
    /// The owned phase of `reading` from `device` on `surface`, committing
    /// whether its finger gesture is open.
    ///
    /// Momentum wins over a finger phase reported with it. A `Changed`
    /// outside an open gesture is a phased step only when precise: a notched
    /// wheel that reports "moved" with no begin is a wheel tick, with no
    /// phase.
    pub fn phase(
        &mut self,
        surface: NativeSurface,
        device: Option<NativeDevice>,
        reading: ScrollReading,
    ) -> Option<ScrollPhase> {
        if let Some(momentum) = reading.momentum {
            return Some(match momentum {
                GesturePhase::Began => ScrollPhase::MomentumBegan,
                GesturePhase::Changed => ScrollPhase::MomentumChanged,
                GesturePhase::Ended | GesturePhase::Cancelled => ScrollPhase::MomentumEnded,
            });
        }
        let key = (surface, device);
        let open = self.open.iter().position(|entry| *entry == key);
        match reading.fingers? {
            GesturePhase::Began => {
                if open.is_none() {
                    self.open.push(key);
                }
                Some(ScrollPhase::Began)
            }
            GesturePhase::Changed => {
                (open.is_some() || reading.precise).then_some(ScrollPhase::Changed)
            }
            GesturePhase::Ended => {
                if let Some(index) = open {
                    self.open.swap_remove(index);
                }
                Some(ScrollPhase::Ended)
            }
            GesturePhase::Cancelled => {
                if let Some(index) = open {
                    self.open.swap_remove(index);
                }
                Some(ScrollPhase::Cancelled)
            }
        }
    }

    /// Forget the gestures open on `surface` (the window closed or lost its
    /// input).
    pub fn cancel_surface(&mut self, surface: NativeSurface) {
        self.open.retain(|(open, _)| *open != surface);
    }

    /// Forget the gestures of a removed `device`.
    pub fn remove_device(&mut self, device: NativeDevice) {
        self.open.retain(|(_, open)| *open != Some(device));
    }
}

/// One scroll step under an open contact, as plain data: a wheel turn or a
/// trackpad pan from where that contact's cursor rests, its phase already
/// resolved by [`ScrollPhases`] for a phased one.
///
/// [`ContactRegistry::scroll`](super::ContactRegistry::scroll) turns it into
/// the scroll, so a native recording holds the step as one row and replays
/// it on any host.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContactScroll {
    /// The surface.
    pub surface: NativeSurface,
    /// The device, when the platform names one.
    pub device: Option<NativeDevice>,
    /// The contact whose position the step scrolls from.
    pub contact: NativeContact,
    /// The owned phase; `None` for an unphased wheel step.
    pub phase: Option<ScrollPhase>,
    /// How far, in the owned sign (positive scrolls down and right).
    pub delta: ScrollDelta,
    /// Whether the delta is precise.
    pub precision: ScrollPrecision,
    /// When, already on the event timeline.
    pub time: EventTime,
    /// The modifiers held.
    pub modifiers: Modifiers,
}

impl ContactRegistry {
    /// The scroll `reading` delivers from its contact's last position, or
    /// `None` when that contact is not open or has no position.
    #[must_use]
    pub fn scroll(&self, reading: &ContactScroll) -> Option<PlatformInput> {
        journal!(self.journal, super::capture::Captured::Scroll(reading));
        let info = self.pointer(reading.surface, reading.device, reading.contact)?;
        let (position, _) = self.position(reading.surface, reading.device, reading.contact)?;
        let mut scroll = ScrollEvent::new(info, reading.time, position, reading.delta)
            .with_precision(reading.precision)
            .with_modifiers(reading.modifiers);
        if let Some(phase) = reading.phase {
            scroll = scroll.with_phase(phase);
        }
        Some(PlatformInput::Pointer(PointerEvent::Scroll(scroll)))
    }
}
