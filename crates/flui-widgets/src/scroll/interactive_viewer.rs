//! [`InteractiveViewer`] — pans, zooms and rotates its child through a
//! transformation matrix, as a leaf of the nested scroll channel.
//!
//! The contract: a single [`TransformationController`]-held [`Matrix4`] maps
//! the child's scene coordinates to viewport coordinates; gestures update that
//! matrix; `min_scale`/`max_scale` clamp the zoom level; `boundary_margin`
//! constrains how far the transformed viewport may drift from the child's own
//! rect (an all-infinite margin removes the boundary entirely). No update
//! publishes a matrix that is not finite and invertible.
//!
//! # A leaf of the nested scroll channel
//!
//! Panning is scrolling (ADR-0209). A contact's pan is a sequence of
//! [`ScrollCause::UserInput`] steps under the contact's
//! [`InputSequenceId`], dispatched through
//! the chain of [`NestedScrollConnection`](crate::NestedScrollConnection)s
//! and scrollables above the viewer: the viewer takes what its boundary
//! admits and the remainder goes up, so a photo at its edge (or not zoomed
//! at all) in a pager turns the page. The chain latches per axis to the
//! deepest participant that consumed the sequence's first motion; under the
//! default remainder rule an ancestor latched at the start keeps the axis for
//! the whole contact, and a viewer latched at the start keeps it even at its
//! edge until the next contact. In the arena the viewer is a leaf that never
//! yields to an enclosing scrollable (ADR-0208 §1).
//!
//! On release, each scrollable that holds an axis settles from where it
//! stands with the release velocity on that axis (a pager lands on a page);
//! the viewer's own inertia runs only on the axes it holds, as
//! [`ScrollCause::SideEffect`] steps of the same sequence. A cancelled
//! contact settles every holder from rest.
//!
//! Scale and rotation are not scrolling: a pinch scales about its focal
//! point and only its focal motion goes through the channel. A native
//! trackpad pinch ([`PanZoomEvent`]) transforms the viewer directly; its
//! translation is the focal point moving.
//!
//! # Snap-back
//!
//! A contact that holds an axis pulls the scene past its boundary there
//! through a rubber band — never a viewport further, less the further it
//! goes — and a pinch past a scale bound zooms on, never half an octave
//! past it. Released, the scene springs back inside its bounds; inertia the
//! boundary stops overshoots it and springs back. The spring is critically
//! damped, the snap-back is the releasing sequence's last motion, and a new
//! contact catches the scene where it shows. Nothing past the bounds is
//! handed to the chain: a contact that starts at the edge is the
//! ancestor's, not a stretch.
//!
//! # Other paths
//!
//! - **Wheel.** Under [`WheelScaleGate::AnyWheel`] a vertical tick zooms by
//!   `exp(-dy / scale_factor)` about the pointer. Under
//!   [`WheelScaleGate::CtrlWheel`] only Ctrl+wheel zooms; a plain wheel tick
//!   or a phased two-finger trackpad scroll pans, as a contactless sequence
//!   under the binding's latch — the viewer starts it only when it can move,
//!   so a viewer at its edge leaves the tick to an enclosing scrollable.
//! - **Double tap** performs the first command of the viewer's
//!   [`GestureAlternatives`] about the tapped point — [`ZoomIntent::In`] in
//!   the usual set — and, at the maximum scale, zooms back out to the minimum.
//! - **Pan controls.** [`PanIntent`] pans one step toward a side: the
//!   single-pointer path of a pan, for the alternatives' controls.
//! - **Keyboard.** A contact on the viewer focuses it. With focus in the
//!   viewer, the arrows pan one step as a
//!   keyboard sequence (an arrow that cannot move anything bubbles), and
//!   `+`/`=` and `-`, alone or with the platform's command key, perform
//!   [`ZoomIntent::In`] and [`ZoomIntent::Out`].
//! - **Assistive technology.** The viewer's node publishes its scale as a
//!   numeric range; increment and decrement zoom one step, setting the value
//!   zooms to it, and the four scroll actions pan.
//!
//! `on_interaction_start`/`on_interaction_update`/`on_interaction_end` report
//! the contact, wheel and trackpad gestures with FLUI's own detail types.
//! The update carries the applied scale multiplier (1.0 for a pure pan), a
//! focal point and the translation the gesture asked for. The end carries
//! separate focal and scale release velocities. Callbacks observe the
//! recognized gesture even when an option disables its transform.
//!
//! **`constrained: false`** (an unconstrained child escaping the viewport) is
//! not offered: the child is laid out under the constraints this widget
//! receives, exactly like [`Transform`], so the viewport rect and the child's
//! own rect are the same rectangle — see [`ViewerCore::geometry`].

#[cfg(doc)]
use crate::Transform;
#[cfg(doc)]
use crate::scroll::nested::ScrollCause;
#[cfg(doc)]
use flui_foundation::InputSequenceId;
#[cfg(doc)]
use flui_foundation::geometry::Matrix4;
#[cfg(doc)]
use flui_platform::input::pointer::PanZoomEvent;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_animation::AnimationController;
use flui_foundation::geometry::EdgeInsets;
use flui_interaction::focus::FocusNode;
use flui_objects::SubtreeAnchor;
use flui_painting::Alignment;
use flui_painting::paint::Clip;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{Child, IntoView, View};
mod alternatives;
mod boundary;
mod fling;
mod leaf;
mod semantics_node;
mod state;
mod stretch;
mod types;
mod viewer_core;
mod zoom_gestures;

pub use state::InteractiveViewerState;
#[cfg(test)]
pub(crate) use state::zoom_keys;
pub use types::{
    InteractionEndDetails, InteractionStartDetails, InteractionUpdateDetails, PanAxis, PanIntent,
    WheelScaleGate, ZoomIntent,
};

use fling::FocalFling;
use leaf::{GestureTracking, ViewerCallbacks, ViewerLeaf};
use stretch::{Overlay, Travel};
use types::{EndCallback, StartCallback, UpdateCallback};
use viewer_core::ViewerCore;

use crate::{GestureAlternatives, TransformationController};

/// One zoom step of a key, a double tap, a control or an assistive action:
/// a factor of two.
const ZOOM_STEP: f64 = 2.0;
/// An arrow key's pan: the framework's wheel line, at most
/// [`PAGE_FRACTION`] of the viewport.
const KEY_STEP: f64 = 53.0;
/// An assistive page scroll moves this fraction of the viewport.
const PAGE_FRACTION: f64 = 0.8;

/// What a mounted viewer reads at each event: the immutable options of the
/// latest configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ViewerConfig {
    boundary_margin: EdgeInsets,
    min_scale: f64,
    max_scale: f64,
    pan_enabled: bool,
    scale_enabled: bool,
    rotation_enabled: bool,
    double_tap_zoom: bool,
    wheel_scale_gate: WheelScaleGate,
    pan_axis: PanAxis,
    scale_factor: f64,
}

/// Pans, zooms and rotates `child` through a
/// [`TransformationController`]-held [`Matrix4`], as a leaf of the nested
/// scroll channel. See the module docs for each input path.
///
/// The constructor takes the viewer's [`GestureAlternatives`] (ADR-0216): a
/// pinch is a multipoint gesture, so zooming needs a single-pointer path.
/// Their commands name [`ZoomIntent`]s, which the viewer answers.
///
/// ```rust
/// use flui_widgets::{
///     Alternative, ColoredBox, ControlSpec, GestureAlternatives, GestureCommand,
///     InteractiveViewer, SizedBox, ZoomIntent,
/// };
/// use flui_painting::styling::Color;
///
/// let alternatives = GestureAlternatives::try_new(Alternative::VisibleControl(
///     ControlSpec::new(GestureCommand::new("Zoom in", ZoomIntent::In), SizedBox::new(16.0, 16.0))
///         .control(GestureCommand::new("Zoom out", ZoomIntent::Out), SizedBox::new(16.0, 16.0)),
/// ))
/// .expect("two distinct labels");
/// let viewer = InteractiveViewer::new(alternatives, ColoredBox::new(Color::rgb(20, 40, 60)))
///     .max_scale(8.0);
/// ```
#[derive(Clone)]
pub struct InteractiveViewer {
    alternatives: GestureAlternatives,
    controller: TransformationController,
    config: ViewerConfig,
    clip_behavior: Clip,
    alignment: Option<Alignment>,
    on_interaction_start: Option<StartCallback>,
    on_interaction_update: Option<UpdateCallback>,
    on_interaction_end: Option<EndCallback>,
    child: Child,
}

impl std::fmt::Debug for InteractiveViewer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InteractiveViewer")
            .field("controller", &self.controller)
            .field("alternatives", &self.alternatives)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl InteractiveViewer {
    /// A viewer that pans and zooms `child`, whose single-pointer, keyboard
    /// and assistive paths are `alternatives`, with default limits
    /// (`min_scale: 0.8`, `max_scale: 2.5`, zero boundary margin, pan, wheel
    /// scale and double tap zoom enabled).
    #[must_use]
    pub fn new(alternatives: GestureAlternatives, child: impl IntoView) -> Self {
        Self {
            alternatives,
            controller: TransformationController::new(),
            config: ViewerConfig {
                boundary_margin: EdgeInsets::ZERO,
                min_scale: 0.8,
                max_scale: 2.5,
                pan_enabled: true,
                scale_enabled: true,
                rotation_enabled: false,
                double_tap_zoom: true,
                wheel_scale_gate: WheelScaleGate::default(),
                pan_axis: PanAxis::Free,
                scale_factor: 200.0,
            },
            clip_behavior: Clip::HardEdge,
            alignment: None,
            on_interaction_start: None,
            on_interaction_update: None,
            on_interaction_end: None,
            child: Child::some(child.into_view()),
        }
    }

    /// Allow rotation around the gesture's focal point. Disabled by default.
    #[must_use]
    pub fn rotation_enabled(mut self, enabled: bool) -> Self {
        self.config.rotation_enabled = enabled;
        self
    }

    /// Share this transform through an external [`TransformationController`]
    /// instead of the one created internally. Multiple widgets can read (and
    /// drive) the same controller.
    #[must_use]
    pub fn controller(mut self, controller: TransformationController) -> Self {
        self.controller = controller;
        self
    }

    /// A margin for the visible boundaries of the child.
    ///
    /// Any transformation that would move the viewport outside of the
    /// boundary is clamped at the boundary. Pass `EdgeInsets::all(f64::
    /// INFINITY)` for no boundary at all.
    ///
    /// # Precondition
    ///
    /// Every edge must be finite, or every edge must be infinite — not a mix
    /// (checked with `debug_assert!`, so debug-only).
    #[must_use]
    pub fn boundary_margin(mut self, boundary_margin: EdgeInsets) -> Self {
        debug_assert!(
            (boundary_margin.left.is_infinite()
                && boundary_margin.top.is_infinite()
                && boundary_margin.right.is_infinite()
                && boundary_margin.bottom.is_infinite())
                || (boundary_margin.left.is_finite()
                    && boundary_margin.top.is_finite()
                    && boundary_margin.right.is_finite()
                    && boundary_margin.bottom.is_finite()),
            "InteractiveViewer::boundary_margin must be either fully finite or \
             fully infinite on all four edges, not a mix"
        );
        self.config.boundary_margin = boundary_margin;
        self
    }

    /// The minimum allowed scale. Must be finite and greater than zero, and
    /// no greater than [`max_scale`](Self::max_scale). Equal bounds fix the
    /// scale: the viewer then only pans.
    #[must_use]
    pub fn min_scale(mut self, min_scale: f64) -> Self {
        debug_assert!(
            min_scale > 0.0 && min_scale.is_finite(),
            "InteractiveViewer::min_scale must be finite and greater than zero"
        );
        self.config.min_scale = min_scale;
        self
    }

    /// The maximum allowed scale. Must be greater than zero, not NaN, and no
    /// less than [`min_scale`](Self::min_scale).
    #[must_use]
    pub fn max_scale(mut self, max_scale: f64) -> Self {
        debug_assert!(
            max_scale > 0.0 && !max_scale.is_nan(),
            "InteractiveViewer::max_scale must be greater than zero"
        );
        self.config.max_scale = max_scale;
        self
    }

    /// If `false`, the viewer does not pan: a drag's motion all goes to the
    /// enclosing scrollables. `on_interaction_*` callbacks still fire.
    #[must_use]
    pub fn pan_enabled(mut self, pan_enabled: bool) -> Self {
        self.config.pan_enabled = pan_enabled;
        self
    }

    /// If `false`, no gesture, key or assistive action scales the child.
    /// `on_interaction_*` callbacks still fire.
    #[must_use]
    pub fn scale_enabled(mut self, scale_enabled: bool) -> Self {
        self.config.scale_enabled = scale_enabled;
        self
    }

    /// Whether a double tap, or a trackpad's smart zoom, performs the first
    /// command of the alternatives (default `true`). A double tap holds a single tap on the content
    /// until the double-tap timeout passes; turn it off where the content's
    /// own taps must not wait.
    #[must_use]
    pub fn double_tap_zoom(mut self, enabled: bool) -> Self {
        self.config.double_tap_zoom = enabled;
        self
    }

    /// Gate wheel-driven zooming on the ctrl chord (default: every vertical
    /// tick zooms). See [`WheelScaleGate`].
    #[must_use]
    pub fn wheel_scale_gate(mut self, gate: WheelScaleGate) -> Self {
        self.config.wheel_scale_gate = gate;
        self
    }

    /// Restricts panning to one axis, or locks a free drag to whichever axis
    /// dominates it. Defaults to [`PanAxis::Free`]. Motion along an axis the
    /// viewer does not pan goes to the enclosing scrollables.
    #[must_use]
    pub fn pan_axis(mut self, pan_axis: PanAxis) -> Self {
        self.config.pan_axis = pan_axis;
        self
    }

    /// The divisor applied to a mouse-wheel scroll delta before it becomes an
    /// exponential scale change (`scale_change = exp(-scroll_dy /
    /// scale_factor)`). Larger values feel slower; smaller values feel
    /// faster. Defaults to `200.0`.
    ///
    /// Raw wheel detents and normalized line packets each use an authored
    /// zoom distance of 53 logical pixels per unit. System scroll line counts
    /// apply to wheel panning, independently of this zoom policy.
    #[must_use]
    pub fn scale_factor(mut self, scale_factor: f64) -> Self {
        self.config.scale_factor = scale_factor;
        self
    }

    /// How the child is clipped to this widget's bounds. Defaults to
    /// [`Clip::HardEdge`] — pass [`Clip::None`] to let a zoomed-in child
    /// paint outside its original area (it still won't receive gestures
    /// there).
    #[must_use]
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }

    /// The alignment of the child's transform pivot. See
    /// [`Transform::alignment`] for the exact contribution when combined
    /// with an origin — `InteractiveViewer` never sets an origin, so this is
    /// simply the pivot the transform matrix scales/rotates around.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Called when a pan, pinch, trackpad or wheel-scale interaction begins.
    #[must_use]
    pub fn on_interaction_start<R: EventOutcome>(
        mut self,
        callback: impl Fn(&mut EventContext<'_>, InteractionStartDetails) -> R + 'static,
    ) -> Self {
        self.on_interaction_start =
            Some(Rc::new(move |ctx, details| callback(ctx, details).report()));
        self
    }

    /// Called on every applied (or attempted, if disabled) gesture update.
    #[must_use]
    pub fn on_interaction_update<R: EventOutcome>(
        mut self,
        callback: impl Fn(&mut EventContext<'_>, InteractionUpdateDetails) -> R + 'static,
    ) -> Self {
        self.on_interaction_update =
            Some(Rc::new(move |ctx, details| callback(ctx, details).report()));
        self
    }

    /// Called when a gesture interaction ends.
    #[must_use]
    pub fn on_interaction_end<R: EventOutcome>(
        mut self,
        callback: impl Fn(&mut EventContext<'_>, InteractionEndDetails) -> R + 'static,
    ) -> Self {
        self.on_interaction_end =
            Some(Rc::new(move |ctx, details| callback(ctx, details).report()));
        self
    }
}

impl View for InteractiveViewer {
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for InteractiveViewer {
    type State = InteractiveViewerState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        let controller = AnimationController::builder(Duration::from_millis(1))
            .debug_label("InteractiveViewer.fling")
            .behavior(flui_animation::AnimationBehavior::Preserve)
            .unbounded()
            .build_on(None);
        let core = Rc::new(ViewerCore {
            controller: RefCell::new(self.controller.clone()),
            config: Cell::new(self.config),
            callbacks: RefCell::new(ViewerCallbacks::of(self)),
            anchor: SubtreeAnchor::new(),
            pipeline: RefCell::new(None),
            gesture: GestureTracking::default(),
            leaf: ViewerLeaf::default(),
            fling: FocalFling::new(controller.controller().clone()),
            focal_hint: Cell::new(None),
            command: RefCell::new(None),
            row_focus: RefCell::new(None),
            travel: Cell::new(Travel::default()),
            overlay: Cell::new(Overlay::default()),
            stretched: Cell::new(None),
        });
        let keys = FocusNode::with_debug_label("InteractiveViewer");
        let cancelled = Rc::downgrade(&core);
        let key_cancel = keys.register_on_key_cancel(Rc::new(
            move |press: &flui_platform::input::keyboard::KeyEvent| {
                if let Some(core) = cancelled.upgrade() {
                    core.leaf.key_released(press.code);
                }
            },
        ));
        InteractiveViewerState {
            core,
            keys,
            _key_cancel: key_cancel,
            post_frame: None,
            writer: ctx.writer_source(),
            controller,
            clock: None,
            wheel_preferences: None,
        }
    }
}
