//! Trackpad pinch and rotation components folded into one gesture.
//!
//! A two-finger pan is not a component: it is a phased scroll
//! ([`ScrollPhases`](super::ScrollPhases)), on every backend (ADR-0217 §4).

use flui_platform::pointer::{PanZoomPhase, PanZoomTransform};
use smallvec::SmallVec;

/// One component of a trackpad gesture. The platform starts and ends each on
/// its own; the gesture lasts while any is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GestureComponent {
    /// Pinch.
    Scale,
    /// Two-finger rotation.
    Rotation,
    /// The platform's outer gesture session (AppKit `beginGesture`/`endGesture`):
    /// its end retires every component.
    Session,
}

impl GestureComponent {
    const fn bit(self) -> u8 {
        match self {
            Self::Scale => 1,
            Self::Rotation => 2,
            Self::Session => 4,
        }
    }
}

/// The phase of one gesture component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GesturePhase {
    /// The component began; the reading's delta already applies.
    Began,
    /// The component changed.
    Changed,
    /// The component ended; the reading's delta still applies.
    Ended,
    /// The platform cancelled the whole gesture.
    Cancelled,
}

/// The change one reading makes to the cumulative transform.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum GestureDelta {
    /// Multiply the scale by this factor (`1.0` is no change).
    Scale(f64),
    /// Add this many radians, clockwise positive.
    Rotation(f64),
    /// No change.
    None,
}

/// The cumulative transform of one trackpad gesture and the components still
/// active in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanZoomAccumulator {
    components: u8,
    transform: PanZoomTransform,
}

impl Default for PanZoomAccumulator {
    fn default() -> Self {
        Self {
            components: 0,
            transform: PanZoomTransform::IDENTITY,
        }
    }
}

impl PanZoomAccumulator {
    /// Whether any component is active.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.components != 0
    }

    /// The transform accumulated so far.
    #[must_use]
    pub const fn transform(&self) -> PanZoomTransform {
        self.transform
    }

    /// Apply one component reading and return the phases it publishes, in
    /// order: `Start` when the first component begins, `Update` when the
    /// transform changed or `focal_moved` (the focal point the reading
    /// carries differs from the previous one), `End` when the last active
    /// component ends.
    ///
    /// A repeated `Began`, or a reading of a component that is not active, is
    /// ignored. A delta whose result is not a valid transform (overflowing
    /// scale, non-finite rotation) keeps the previous transform.
    pub fn apply(
        &mut self,
        component: GestureComponent,
        phase: GesturePhase,
        delta: GestureDelta,
        focal_moved: bool,
    ) -> SmallVec<[PanZoomPhase; 3]> {
        let mut phases = SmallVec::new();
        let bit = component.bit();
        if phase == GesturePhase::Cancelled {
            if self.is_active() {
                *self = Self::default();
                phases.push(PanZoomPhase::Cancelled);
            }
            return phases;
        }
        let mut started = false;
        if phase == GesturePhase::Began {
            if self.components & bit != 0 {
                return phases;
            }
            if !self.is_active() {
                *self = Self::default();
                phases.push(PanZoomPhase::Start);
                started = true;
            }
            self.components |= bit;
        }
        // The session's end retires every component, even one whose own end
        // never arrived, and even when the session's begin was not seen.
        if component == GestureComponent::Session
            && phase == GesturePhase::Ended
            && self.is_active()
        {
            self.components |= bit;
        }
        if self.components & bit == 0 {
            return phases;
        }
        let changed = self
            .applied(delta)
            .filter(|transform| *transform != self.transform);
        if let Some(transform) = changed {
            self.transform = transform;
        }
        // Translation during a pinch is the focal point moving: an update
        // carries it even when scale and rotation stand still.
        if changed.is_some() || (focal_moved && !started) {
            phases.push(PanZoomPhase::Update(self.transform));
        }
        if phase == GesturePhase::Ended {
            self.components = if component == GestureComponent::Session {
                0
            } else {
                self.components & !bit
            };
            if !self.is_active() {
                phases.push(PanZoomPhase::End);
            }
        }
        phases
    }

    /// End the gesture as cancelled; `None` when none is active.
    pub fn cancel(&mut self) -> Option<PanZoomPhase> {
        self.is_active().then(|| {
            *self = Self::default();
            PanZoomPhase::Cancelled
        })
    }

    fn applied(&self, delta: GestureDelta) -> Option<PanZoomTransform> {
        let previous = self.transform;
        let (scale, rotation) = match delta {
            GestureDelta::Scale(factor) => (previous.scale() * factor, previous.rotation()),
            GestureDelta::Rotation(radians) => (previous.scale(), previous.rotation() + radians),
            GestureDelta::None => return None,
        };
        PanZoomTransform::try_new(scale, rotation).ok()
    }
}
