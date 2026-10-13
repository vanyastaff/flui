//! The phase grammar every backend's scrolling shares (ADR-0217 §4).
//!
//! A two-finger trackpad pan is a scroll with [`ScrollPhase`]s on every OS,
//! and platform momentum is `ScrollPhase::Momentum*`, whatever the native
//! event family (AppKit `scrollWheel:` phases, winit `MouseWheel` and
//! `PanGesture` touch phases). Backends report the native phases as plain
//! [`ScrollReading`]s; [`ScrollPhases`] turns them into the owned phase.

use flui_platform_api::pointer::ScrollPhase;

use super::{GesturePhase, NativeDevice, NativeSurface};

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
