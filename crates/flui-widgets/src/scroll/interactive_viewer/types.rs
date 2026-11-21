//! The values [`InteractiveViewer`] is configured and
//! observed with: the pan axis, the wheel gate, the interaction details its
//! callbacks receive, and the zoom and pan intents.

#[cfg(doc)]
use super::InteractiveViewer;
#[cfg(doc)]
use crate::GestureAlternatives;
use std::rc::Rc;

use flui_foundation::geometry::Offset;
use flui_interaction::gesture::{GestureEndReason, Velocity};
use flui_view::prelude::*;

use crate::Intent;

/// Constrains which axis (or axes) [`InteractiveViewer`] pans along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PanAxis {
    /// Panning is allowed only along the horizontal axis.
    Horizontal,
    /// Panning is allowed only along the vertical axis.
    Vertical,
    /// Panning is allowed along the horizontal and vertical axes, but never
    /// diagonally — the drag's dominant axis (established on the first
    /// non-zero movement of the gesture) locks for the rest of the gesture.
    Aligned,
    /// Panning is allowed freely in any direction.
    #[default]
    Free,
}

/// Details passed to `on_interaction_start`. See the module docs for why this
/// is FLUI's own shape rather than a `ScaleStartDetails` port.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteractionStartDetails {
    /// The interaction's focal point, in the coordinates of the widget that
    /// contains `InteractiveViewer`.
    pub focal_point: Offset<f64>,
    /// The interaction's focal point, in the coordinates of
    /// `InteractiveViewer` itself (viewport-local).
    pub local_focal_point: Offset<f64>,
}

/// Details passed to `on_interaction_update`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteractionUpdateDetails {
    /// The interaction's current focal point, in the coordinates of the
    /// widget that contains `InteractiveViewer`.
    pub focal_point: Offset<f64>,
    /// The interaction's current focal point, in `InteractiveViewer`'s own
    /// (viewport-local) coordinates.
    pub local_focal_point: Offset<f64>,
    /// The multiplicative scale change applied by this update. `1.0` for a
    /// pure pan update (no scale change).
    pub scale: f64,
    /// The translation this update asked for, in viewport pixels, after the
    /// pan axis; zero for a pure wheel-scale update and while panning is
    /// disabled. What the boundary admitted is the controller's change.
    pub focal_point_delta: Offset<f64>,
}

/// Details passed to `on_interaction_end`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteractionEndDetails {
    /// Normal completion or an interrupted pointer gesture.
    pub reason: GestureEndReason,
    /// The gesture's release velocity. [`Velocity::ZERO`] for a discrete
    /// wheel-scale interaction (there is no release to measure).
    pub velocity: Velocity,
    /// Measured scale change in scale units per second. Zero for cancellation
    /// and a discrete wheel step, which supply no scale release history.
    pub scale_velocity: f64,
}

pub(super) type StartCallback = Rc<dyn Fn(&mut EventContext<'_>, InteractionStartDetails)>;
pub(super) type UpdateCallback = Rc<dyn Fn(&mut EventContext<'_>, InteractionUpdateDetails)>;
pub(super) type EndCallback = Rc<dyn Fn(&mut EventContext<'_>, InteractionEndDetails)>;

/// Zoom the enclosing [`InteractiveViewer`] one step: by a factor of two,
/// about the viewport's centre — or, when the viewer's double tap performs
/// it, about the tapped point.
///
/// The viewer answers it for everything inside it: the commands of its
/// [`GestureAlternatives`] name it, and its zoom keys and assistive
/// increment and decrement perform it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(clippy::exhaustive_enums, reason = "a zoom goes in or out")]
pub enum ZoomIntent {
    /// Zoom in: the scene grows.
    In,
    /// Zoom out: the scene shrinks.
    Out,
}

impl Intent for ZoomIntent {}

/// Pan the enclosing [`InteractiveViewer`] one step toward a side, showing
/// what lies that way: the single-pointer path of a pan, for the viewer's
/// [`GestureAlternatives`] controls.
///
/// A step is the framework's wheel line (53 logical pixels), at most four
/// fifths of the viewport, within the boundary; it goes through the nested
/// scroll channel like the pan it stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(clippy::exhaustive_enums, reason = "the four directions of a plane")]
pub enum PanIntent {
    /// Show what lies to the left.
    Left,
    /// Show what lies to the right.
    Right,
    /// Show what lies above.
    Up,
    /// Show what lies below.
    Down,
}

impl Intent for PanIntent {}

/// When the wheel is allowed to drive scroll-to-scale.
///
/// The desktop contract "wheel scrolls, ctrl+wheel zooms" is only
/// composable when the viewer restricts itself to the chord. Under
/// [`CtrlWheel`], a plain tick pans the viewer through the nested scroll
/// channel, or reaches the enclosing scrollable when the viewer cannot move.
/// [`AnyWheel`] zooms on every vertical tick and is the default.
///
/// [`CtrlWheel`]: WheelScaleGate::CtrlWheel
/// [`AnyWheel`]: WheelScaleGate::AnyWheel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum WheelScaleGate {
    /// Every vertical wheel tick zooms.
    #[default]
    AnyWheel,
    /// Only ctrl+wheel zooms; a plain tick pans.
    CtrlWheel,
}
