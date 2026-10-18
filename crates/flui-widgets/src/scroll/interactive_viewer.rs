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
//! [`InputSequenceId`](flui_interaction::InputSequenceId), dispatched through
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

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use flui_animation::{
    Animation, AnimationController, DrivenController, FrictionSimulation, Tolerance, Vsync,
};
use flui_foundation::geometry::{Axis, EdgeInsets, Matrix4, Offset, Point, Rect, Vec2};
use flui_foundation::panic::PanicRecovery;
use flui_foundation::{Listenable, ListenerId};
use flui_interaction::focus::{FocusNode, FocusNodeRegistration, KeyEventResult};
use flui_interaction::gesture::{
    GestureArena, GestureSettingsProvider, ScaleEndDetails, ScaleStartDetails, ScaleStartMode,
    ScaleUpdateDetails,
};
use flui_interaction::pointer::EventPropagation;
use flui_interaction::{GestureEndReason, InputSequenceId, RemainderHandoff, Velocity};
use flui_interaction::{PointerEvent, WheelPreferencesProvider};
use flui_objects::SubtreeAnchor;
use flui_painting::Alignment;
use flui_painting::paint::Clip;
use flui_platform_api::TargetPlatform;
use flui_platform_api::keyboard::{Code, ImeComposition, Key, KeyRepeat, KeyState, NamedKey};
use flui_platform_api::{
    keyboard::Modifiers,
    pointer::{PanZoomEvent, PanZoomPhase, PointerKind, ScrollEvent, ScrollPhase, ScrollUnit},
};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::pipeline::PipelineCell;
use flui_rendering::semantics::{NumericRange, ScrollUnit as SemanticsScrollUnit};
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{BoxedView, Child, IntoView, PostFrameHandle, View, ViewExt, ViewState};

mod stretch;

use stretch::{Overlay, Travel};

use super::WheelScrollDistances;
use super::nested::{
    ChainNode, NestedScrollScope, ScrollCause, ScrollDevice, ScrollEndReason, ScrollSequence,
};
use super::wheel_axis_delta;
use crate::anchored_box::AnchoredBox;
use crate::support::command_modifiers;
use crate::{
    Action, ActionOutcome, Actions, AnimatedBuilder, ClipRect, CommandHandle, Focus,
    GestureAlternatives, GestureArenaScope, GestureDetector, Intent, Listener, Semantics,
    Shortcuts, SingleActivator, Transform, TransformSlot, TransformationController, VsyncScope,
};

/// One zoom step of a key, a double tap, a control or an assistive action:
/// a factor of two.
const ZOOM_STEP: f64 = 2.0;
/// An arrow key's pan: the framework's wheel line, at most
/// [`PAGE_FRACTION`] of the viewport.
const KEY_STEP: f64 = 53.0;
/// An assistive page scroll moves this fraction of the viewport.
const PAGE_FRACTION: f64 = 0.8;

// ============================================================================
// PanAxis
// ============================================================================

/// Constrains which axis (or axes) [`InteractiveViewer`] pans along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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

// ============================================================================
// Interaction details
// ============================================================================

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

type StartCallback = Rc<dyn Fn(&mut EventCx<'_>, InteractionStartDetails)>;
type UpdateCallback = Rc<dyn Fn(&mut EventCx<'_>, InteractionUpdateDetails)>;
type EndCallback = Rc<dyn Fn(&mut EventCx<'_>, InteractionEndDetails)>;

// ============================================================================
// ZoomIntent
// ============================================================================

/// Zoom the enclosing [`InteractiveViewer`] one step: by a factor of two,
/// about the viewport's centre — or, when the viewer's double tap performs
/// it, about the tapped point.
///
/// The viewer answers it for everything inside it: the commands of its
/// [`GestureAlternatives`] name it, and its zoom keys and assistive
/// increment and decrement perform it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

// ============================================================================
// InteractiveViewer
// ============================================================================

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
pub enum WheelScaleGate {
    /// Every vertical wheel tick zooms.
    #[default]
    AnyWheel,
    /// Only ctrl+wheel zooms; a plain tick pans.
    CtrlWheel,
}

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
/// let alternatives = GestureAlternatives::new(Alternative::VisibleControl(
///     ControlSpec::new(GestureCommand::new("Zoom in", ZoomIntent::In), SizedBox::new(16.0, 16.0))
///         .control(GestureCommand::new("Zoom out", ZoomIntent::Out), SizedBox::new(16.0, 16.0)),
/// ))
/// .expect("two distinct labels");
/// let viewer = InteractiveViewer::new(alternatives)
///     .max_scale(8.0)
///     .child(ColoredBox::new(Color::rgb(20, 40, 60)));
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
    /// A viewer whose single-pointer, keyboard and assistive paths are
    /// `alternatives`, with default limits (`min_scale: 0.8`,
    /// `max_scale: 2.5`, zero boundary margin, pan, wheel scale and double
    /// tap zoom enabled).
    #[must_use]
    pub fn new(alternatives: GestureAlternatives) -> Self {
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
            child: Child::empty(),
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

    /// Whether a double tap performs the first command of the alternatives
    /// (default `true`). A double tap holds a single tap on the content
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
        callback: impl Fn(&mut EventCx<'_>, InteractionStartDetails) -> R + 'static,
    ) -> Self {
        self.on_interaction_start =
            Some(Rc::new(move |cx, details| callback(cx, details).report()));
        self
    }

    /// Called on every applied (or attempted, if disabled) gesture update.
    #[must_use]
    pub fn on_interaction_update<R: EventOutcome>(
        mut self,
        callback: impl Fn(&mut EventCx<'_>, InteractionUpdateDetails) -> R + 'static,
    ) -> Self {
        self.on_interaction_update =
            Some(Rc::new(move |cx, details| callback(cx, details).report()));
        self
    }

    /// Called when a gesture interaction ends.
    #[must_use]
    pub fn on_interaction_end<R: EventOutcome>(
        mut self,
        callback: impl Fn(&mut EventCx<'_>, InteractionEndDetails) -> R + 'static,
    ) -> Self {
        self.on_interaction_end = Some(Rc::new(move |cx, details| callback(cx, details).report()));
        self
    }

    /// The child to pan and zoom.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl View for InteractiveViewer {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for InteractiveViewer {
    type State = InteractiveViewerState;

    fn create_state(&self) -> Self::State {
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
            move |press: &flui_platform_api::keyboard::KeyEvent| {
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
            writer: None,
            controller,
            clock: None,
            wheel_preferences: None,
        }
    }
}

// ============================================================================
// State
// ============================================================================

/// The in-flight gesture's bookkeeping across the scale updates of one
/// interaction. It lives in the element's state, so a rebuild that swaps in
/// fresh closures mid-gesture continues the same gesture.
#[derive(Debug)]
struct GestureTracking {
    /// Viewport-local position at the most recent transform `on_start`.
    /// `None` between gestures.
    pan_start_local: Cell<Option<Offset<f64>>>,
    /// The axis a [`PanAxis::Aligned`] drag has locked to, established from
    /// the first non-zero movement of the gesture. `None` before that, and
    /// reset to `None` at the end of every gesture.
    current_axis: Cell<Option<Axis>>,
    scale: Cell<f64>,
    rotation: Cell<f64>,
    /// The gesture is a native trackpad pinch, not contacts: it transforms
    /// the viewer directly instead of through the channel.
    native: Cell<bool>,
}

impl Default for GestureTracking {
    fn default() -> Self {
        Self {
            pan_start_local: Cell::new(None),
            current_axis: Cell::new(None),
            scale: Cell::new(1.0),
            rotation: Cell::new(0.0),
            native: Cell::new(false),
        }
    }
}

impl GestureTracking {
    fn reset(&self) {
        self.pan_start_local.set(None);
        self.current_axis.set(None);
        self.scale.set(1.0);
        self.rotation.set(0.0);
    }
}

#[derive(Clone, Default)]
struct ViewerCallbacks {
    start: Option<StartCallback>,
    update: Option<UpdateCallback>,
    end: Option<EndCallback>,
}

impl ViewerCallbacks {
    fn of(view: &InteractiveViewer) -> Self {
        Self {
            start: view.on_interaction_start.clone(),
            update: view.on_interaction_update.clone(),
            end: view.on_interaction_end.clone(),
        }
    }
}

/// The viewer's place in the nested scroll channel: the chain above it, the
/// binding's sequence issuer, and the sequences it leads.
#[derive(Default)]
struct ViewerLeaf {
    parent: RefCell<Option<Rc<ChainNode>>>,
    arena: RefCell<Option<GestureArena>>,
    settings: RefCell<GestureSettingsProvider>,
    /// The device of the last contact that went down on the viewer.
    device: Cell<Option<ScrollDevice>>,
    /// The live contact's sequence: from its pan's start until it lifts.
    contact: RefCell<Option<Rc<ScrollSequence>>>,
    /// The latest contactless sequence: a wheel latch, or a key held down.
    contactless: RefCell<Option<Contactless>>,
}

struct Contactless {
    sequence: Rc<ScrollSequence>,
    /// The key whose press began it; its repeats continue it.
    key: Option<Code>,
}

impl ViewerLeaf {
    fn mint(&self) -> Option<InputSequenceId> {
        let arena = self.arena.borrow().clone()?;
        match arena.begin_contactless_sequence() {
            Ok(sequence) => Some(sequence),
            Err(error) => {
                tracing::debug!(%error, "a viewer pans without a sequence: identities exhausted");
                None
            }
        }
    }

    /// The sequence the binding is dispatching input of, or a fresh one.
    fn dispatched_or_mint(&self) -> Option<InputSequenceId> {
        let dispatched = self
            .arena
            .borrow()
            .as_ref()
            .and_then(GestureArena::dispatched_sequence);
        dispatched.or_else(|| self.mint())
    }

    fn remainder_rule(&self, device: ScrollDevice) -> RemainderHandoff {
        match device {
            ScrollDevice::Pointer { kind, .. } => self
                .settings
                .borrow()
                .snapshot_for(kind)
                .remainder_handoff(),
            _ => RemainderHandoff::NextSequence,
        }
    }

    /// Snapshot the chain above the viewer for `id` and tell it the
    /// sequence started.
    fn begin(
        &self,
        id: InputSequenceId,
        device: ScrollDevice,
        contact: bool,
        recovery: &mut PanicRecovery,
    ) -> Rc<ScrollSequence> {
        let parent = self.parent.borrow().clone();
        let sequence = ScrollSequence::above(
            parent.as_ref(),
            id,
            device,
            contact,
            self.remainder_rule(device),
        );
        sequence.start(recovery);
        sequence
    }

    fn end_contactless(&self, reason: ScrollEndReason, recovery: &mut PanicRecovery) {
        let outgoing = self.contactless.borrow_mut().take();
        if let Some(outgoing) = outgoing {
            outgoing.sequence.finish_input();
            outgoing.sequence.end(reason, recovery);
        }
    }

    /// A contact's pan started on the viewer: its sequence begins, and the
    /// contactless one it interrupts ends.
    fn begin_contact(&self) {
        let mut recovery = PanicRecovery::new();
        let outgoing = self.contact.borrow_mut().take();
        if let Some(outgoing) = outgoing {
            outgoing.cancel_contact();
            outgoing.end(ScrollEndReason::Cancelled, &mut recovery);
        }
        self.end_contactless(ScrollEndReason::Released, &mut recovery);
        if let Some(id) = self.dispatched_or_mint() {
            let device = self.device.get().unwrap_or(ScrollDevice::Pointer {
                kind: PointerKind::Touch,
                device: None,
            });
            let sequence = self.begin(id, device, true, &mut recovery);
            *self.contact.borrow_mut() = Some(sequence);
        }
        recovery.finish();
    }

    /// One delta of the live contact through the chain; without a sequence
    /// (identities exhausted) the viewer alone takes it.
    fn contact_step(&self, available: Vec2, consume: impl FnOnce(Vec2) -> Vec2) -> Vec2 {
        let sequence = self.contact.borrow().clone();
        let Some(sequence) = sequence.filter(|sequence| sequence.is_live()) else {
            return consume(available);
        };
        let mut recovery = PanicRecovery::new();
        let consumed =
            sequence.dispatch(0, ScrollCause::UserInput, available, consume, &mut recovery);
        recovery.finish();
        consumed
    }

    /// The axes the live contact's viewer holds.
    fn contact_holds(&self) -> [bool; 2] {
        self.contact
            .borrow()
            .as_ref()
            .map_or([false, false], |sequence| sequence.leaf_holds())
    }

    /// The contact lifted with `velocity` (physical channel axes): the
    /// release settles every scrollable the pan moved. Returns the sequence,
    /// for the viewer's own inertia, and the axes the viewer holds.
    fn release(&self, velocity: Vec2) -> Option<(Rc<ScrollSequence>, [bool; 2])> {
        let sequence = self.contact.borrow_mut().take()?;
        let mut recovery = PanicRecovery::new();
        sequence.release(velocity, &mut recovery);
        let held = sequence.leaf_holds();
        recovery.finish();
        Some((sequence, held))
    }

    /// The contact was cancelled: its end settles every scrollable the pan
    /// moved from rest.
    fn cancel(&self) {
        let Some(sequence) = self.contact.borrow_mut().take() else {
            return;
        };
        let mut recovery = PanicRecovery::new();
        sequence.cancel_contact();
        sequence.end(ScrollEndReason::Cancelled, &mut recovery);
        recovery.finish();
    }

    /// Whether `id` is the contactless sequence the viewer leads now.
    fn leads(&self, id: InputSequenceId) -> bool {
        self.contactless
            .borrow()
            .as_ref()
            .is_some_and(|current| current.sequence.id() == id && !current.sequence.is_ended())
    }

    /// One step of contactless sequence `id`: the one the viewer leads, or a
    /// new one that ends it.
    fn contactless_step(
        &self,
        id: InputSequenceId,
        device: ScrollDevice,
        key: Option<Code>,
        cause: ScrollCause,
        available: Vec2,
        consume: impl FnOnce(Vec2) -> Vec2,
    ) -> Vec2 {
        let mut recovery = PanicRecovery::new();
        let current = self
            .contactless
            .borrow()
            .as_ref()
            .filter(|current| current.sequence.id() == id && !current.sequence.is_ended())
            .map(|current| Rc::clone(&current.sequence));
        let sequence = if let Some(sequence) = current {
            sequence
        } else {
            self.end_contactless(ScrollEndReason::Released, &mut recovery);
            let sequence = self.begin(id, device, false, &mut recovery);
            *self.contactless.borrow_mut() = Some(Contactless {
                sequence: Rc::clone(&sequence),
                key,
            });
            sequence
        };
        let consumed = sequence.dispatch(0, cause, available, consume, &mut recovery);
        recovery.finish();
        consumed
    }

    /// The key sequence `code` held is continued by its auto-repeat.
    fn key_sequence(&self, code: Code) -> Option<InputSequenceId> {
        self.contactless
            .borrow()
            .as_ref()
            .filter(|current| current.key == Some(code) && !current.sequence.is_ended())
            .map(|current| current.sequence.id())
    }

    /// `code` was released: the sequence it held ends.
    fn key_released(&self, code: Code) {
        if self.key_sequence(code).is_some() {
            let mut recovery = PanicRecovery::new();
            self.end_contactless(ScrollEndReason::Released, &mut recovery);
            recovery.finish();
        }
    }

    /// A one-step sequence of its own: an assistive scroll action.
    fn one_shot(
        &self,
        device: ScrollDevice,
        available: Vec2,
        consume: impl FnOnce(Vec2) -> Vec2,
    ) -> Vec2 {
        let Some(id) = self.dispatched_or_mint() else {
            return consume(available);
        };
        let mut recovery = PanicRecovery::new();
        self.end_contactless(ScrollEndReason::Released, &mut recovery);
        let sequence = self.begin(id, device, false, &mut recovery);
        let consumed =
            sequence.dispatch(0, ScrollCause::UserInput, available, consume, &mut recovery);
        sequence.finish_input();
        sequence.end(ScrollEndReason::Released, &mut recovery);
        recovery.finish();
        consumed
    }

    /// The viewer went away: what it leads ends cancelled.
    fn retire(&self) {
        let mut recovery = PanicRecovery::new();
        if let Some(contact) = self.contact.borrow_mut().take() {
            contact.cancel_contact();
            contact.end(ScrollEndReason::Cancelled, &mut recovery);
        }
        self.end_contactless(ScrollEndReason::Cancelled, &mut recovery);
        recovery.finish();
    }
}

/// Everything a mounted viewer's callbacks share: the configuration, the
/// transform, the channel leaf and the inertia. It lives in the element's
/// state; every callback holds it, so a rebuild between two events of one
/// gesture continues that gesture under the new configuration.
struct ViewerCore {
    controller: RefCell<TransformationController>,
    config: Cell<ViewerConfig>,
    callbacks: RefCell<ViewerCallbacks>,
    /// Publishes the child's `RenderId` while mounted — see
    /// [`geometry`](Self::geometry).
    anchor: SubtreeAnchor,
    /// Acquired in lifecycle hooks, read only from event callbacks.
    pipeline: RefCell<Option<PipelineCell>>,
    gesture: GestureTracking,
    leaf: ViewerLeaf,
    fling: FocalFling,
    /// Where a double tap happened, for the zoom it performs.
    focal_hint: Cell<Option<Offset<f64>>>,
    /// Performs the first command of the alternatives, resolved below the
    /// viewer's own actions.
    command: RefCell<Option<CommandHandle>>,
    /// The focus node of the alternatives' row, which a contact on the
    /// viewer focuses so its keys work next.
    row_focus: RefCell<Option<Rc<FocusNode>>>,
    /// What the gesture pulled past the bounds, unresisted.
    travel: Cell<Travel>,
    /// What the viewer shows past the bounds.
    overlay: Cell<Overlay>,
    /// While an overlay shows: the in-bounds transform under it, and the
    /// value published with the overlay.
    stretched: Cell<Option<(Matrix4, Matrix4)>>,
}

impl ViewerCore {
    fn controller(&self) -> TransformationController {
        self.controller.borrow().clone()
    }

    fn callbacks(&self) -> ViewerCallbacks {
        self.callbacks.borrow().clone()
    }

    /// The viewport rect and the boundary rect, in scene coordinates, or
    /// `None` before the child is mounted and laid out.
    ///
    /// The child is laid out under this widget's own constraints, and
    /// everything between them (`Listener`, `GestureDetector`, `ClipRect`,
    /// `Transform`) adopts the child's size, so the viewport and the child's
    /// own unmargined rect (inflated by `boundary_margin` for the boundary)
    /// are the same rectangle and one anchor serves both.
    fn geometry(&self) -> Option<(Rect<f64>, Rect<f64>)> {
        let owner = self.pipeline.borrow().clone()?;
        let render_id = self.anchor.get()?;
        let size = owner.with(|owner| owner.box_size(render_id))?;
        let rect = Rect::from_origin_size(Point::new(0.0, 0.0), size);
        Some((rect, self.config.get().boundary_margin.inflate_rect(rect)))
    }

    fn scale(&self) -> f64 {
        uniform_scale(&self.controller().value())
    }

    /// `delta` limited to the axes the viewer pans now.
    fn lock(&self, delta: Offset<f64>) -> Offset<f64> {
        match self.config.get().pan_axis {
            PanAxis::Free => delta,
            PanAxis::Horizontal => align_to_axis(delta, Axis::Horizontal),
            PanAxis::Vertical => align_to_axis(delta, Axis::Vertical),
            PanAxis::Aligned => self
                .gesture
                .current_axis
                .get()
                .map_or(delta, |axis| align_to_axis(delta, axis)),
        }
    }

    /// The in-bounds transform: the published one without the overlay shown
    /// past the bounds. A value written from outside since replaces the
    /// overlay.
    fn base(&self) -> Matrix4 {
        let value = self.controller().value();
        match self.stretched.get() {
            Some((base, published)) if published == value => base,
            Some(_) => {
                self.travel.set(Travel::default());
                self.overlay.set(Overlay::default());
                self.stretched.set(None);
                value
            }
            None => value,
        }
    }

    /// Publish the in-bounds transform `base` with the overlay over it. An
    /// overlay that would not publish a finite, invertible transform is
    /// dropped.
    fn publish(&self, base: Matrix4) {
        let overlay = self.overlay.get();
        let shown = if overlay.is_rest() {
            None
        } else {
            overlay
                .matrix()
                .map(|overlay| overlay * base)
                .filter(|shown| shown.try_inverse().is_some())
        };
        let controller = self.controller();
        if let Some(shown) = shown {
            self.stretched.set(Some((base, shown)));
            controller.set_value(shown);
        } else {
            self.travel.set(Travel::default());
            self.overlay.set(Overlay::default());
            self.stretched.set(None);
            controller.set_value(base);
        }
    }

    /// Bring the scene back inside the bounds at once.
    fn collapse(&self) {
        if self.stretched.get().is_some() {
            let base = self.base();
            self.overlay.set(Overlay::default());
            self.publish(base);
        }
    }

    /// A path that is no gesture moves the scene: the motion stops and the
    /// scene is inside its bounds.
    fn interrupt(&self) {
        self.fling.stop(ScrollEndReason::Preempted);
        self.collapse();
    }

    /// A gesture starts over a snap-back: it continues from where the scene
    /// shows.
    fn catch_stretch(&self) {
        let overlay = self.overlay.get();
        if overlay.is_rest() {
            self.travel.set(Travel::default());
            return;
        }
        let Some((viewport, _)) = self.geometry() else {
            self.collapse();
            return;
        };
        self.travel
            .set(overlay.travel(viewport.width(), viewport.height()));
    }

    /// Show the travel through the rubber band, the scale part about `focal`.
    fn show_travel(&self, base: Matrix4, focal: Offset<f64>) {
        let Some((viewport, _)) = self.geometry() else {
            return;
        };
        self.overlay.set(Overlay::of(
            self.travel.get(),
            focal,
            viewport.width(),
            viewport.height(),
        ));
        self.publish(base);
    }

    /// Move the scene by `wanted` viewport pixels within the boundary and
    /// return how far it moved toward `wanted` on each axis. With `commit`
    /// false, only measure.
    fn translate(&self, wanted: Offset<f64>, commit: bool) -> Offset<f64> {
        if wanted == Offset::ZERO || !wanted.is_finite() {
            return Offset::ZERO;
        }
        let Some((viewport, boundary)) = self.geometry() else {
            return Offset::ZERO;
        };
        if !viewport.is_finite() || viewport.width() <= 0.0 || viewport.height() <= 0.0 {
            return Offset::ZERO;
        }
        let before = self.base();
        let Some(next) = contain_transform(
            Matrix4::translation(wanted.dx, wanted.dy, 0.0) * before,
            viewport,
            boundary,
        ) else {
            return Offset::ZERO;
        };
        let (bx, by, _) = before.translation_component();
        let (nx, ny, _) = next.translation_component();
        let moved = Offset::new(toward(nx - bx, wanted.dx), toward(ny - by, wanted.dy));
        if commit && next != before {
            self.publish(next);
        }
        moved
    }

    /// The viewer's part of a channel step `offer` (physical channel axes:
    /// the scene moves against it). On the axes `stretch` names — those the
    /// viewer holds for a contact — what the boundary stops is pulled past
    /// it through the rubber band and consumed whole.
    fn consume(&self, offer: Vec2, stretch: [bool; 2]) -> Vec2 {
        if !self.config.get().pan_enabled {
            return Vec2::ZERO;
        }
        let wanted = self.lock(Offset::new(-offer.x, -offer.y));
        if stretch == [false, false] {
            let moved = self.translate(wanted, true);
            return Vec2::new(-moved.dx, -moved.dy);
        }
        let mut travel = self.travel.get();
        let carried = |axis: usize, value: f64| if stretch[axis] { value } else { 0.0 };
        let total = Offset::new(
            wanted.dx + carried(0, travel.pan.dx),
            wanted.dy + carried(1, travel.pan.dy),
        );
        let moved = self.translate(total, true);
        let past = |axis: usize, total: f64, moved: f64| {
            let past = total - moved;
            if stretch[axis] && past.abs() > 1e-9 && past.is_finite() {
                past
            } else {
                0.0
            }
        };
        travel.pan = Offset::new(past(0, total.dx, moved.dx), past(1, total.dy, moved.dy));
        self.travel.set(travel);
        let focal = self.overlay.get().focal;
        self.show_travel(self.base(), focal);
        let consumed = Offset::new(
            if stretch[0] { wanted.dx } else { moved.dx },
            if stretch[1] { wanted.dy } else { moved.dy },
        );
        Vec2::new(-consumed.dx, -consumed.dy)
    }

    /// A pinch step: scale by `exp(log_ratio)` and rotate about `pivot`. The
    /// transform follows within the scale bounds; what a bound stops goes
    /// into the rubber band, shown about `pivot`.
    fn pinch(&self, pivot: Offset<f64>, log_ratio: f64, rotation: f64) {
        let config = self.config.get();
        if !log_ratio.is_finite() {
            return;
        }
        let Some((viewport, boundary)) = self.geometry() else {
            return;
        };
        let base = self.base();
        let mut travel = self.travel.get();
        let total = travel.log_scale + log_ratio;
        let Some(next) = gesture_transform(
            base,
            pivot,
            Offset::ZERO,
            total.exp(),
            rotation,
            config.min_scale,
            config.max_scale,
            viewport,
            boundary,
        ) else {
            return;
        };
        let past = total - (uniform_scale(&next) / uniform_scale(&base)).ln();
        travel.log_scale = if past.is_finite() && past.abs() > 1e-9 {
            past
        } else {
            0.0
        };
        self.travel.set(travel);
        self.show_travel(next, pivot);
    }

    /// One step of `intent` toward a side, as its own assistive sequence.
    fn pan_step(&self, intent: PanIntent) -> bool {
        let Some((viewport, _)) = self.geometry() else {
            return false;
        };
        let step = |dimension: f64| KEY_STEP.min((dimension * PAGE_FRACTION).max(0.0));
        let available = match intent {
            PanIntent::Left => Vec2::new(-step(viewport.width()), 0.0),
            PanIntent::Right => Vec2::new(step(viewport.width()), 0.0),
            PanIntent::Up => Vec2::new(0.0, -step(viewport.height())),
            PanIntent::Down => Vec2::new(0.0, step(viewport.height())),
        };
        self.interrupt();
        self.leaf
            .one_shot(ScrollDevice::Assistive, available, |offer| {
                self.consume(offer, [false, false])
            })
            != Vec2::ZERO
    }

    /// Zoom by `factor` about `focal` (the viewport's centre without one).
    /// Returns whether the transform changed.
    fn zoom_by(&self, factor: f64, focal: Option<Offset<f64>>) -> bool {
        let config = self.config.get();
        if !config.scale_enabled || !factor.is_finite() || factor <= 0.0 {
            return false;
        }
        let Some((viewport, boundary)) = self.geometry() else {
            return false;
        };
        let focal = focal.unwrap_or_else(|| {
            Offset::new(
                f64::midpoint(viewport.min.x, viewport.max.x),
                f64::midpoint(viewport.min.y, viewport.max.y),
            )
        });
        let before = self.base();
        let Some(next) = gesture_transform(
            before,
            focal,
            Offset::ZERO,
            factor,
            0.0,
            config.min_scale,
            config.max_scale,
            viewport,
            boundary,
        ) else {
            return false;
        };
        if next == before {
            return false;
        }
        self.publish(next);
        true
    }

    /// One zoom step for `intent`, about the double tap's point if one
    /// performed it.
    fn zoom_step(&self, intent: ZoomIntent) -> bool {
        let focal = self.focal_hint.take();
        self.interrupt();
        let factor = match intent {
            ZoomIntent::In => ZOOM_STEP,
            ZoomIntent::Out => ZOOM_STEP.recip(),
        };
        self.zoom_by(factor, focal)
    }

    /// Pan one assistive step toward `direction` (a physical channel unit
    /// vector), by an item or a page.
    fn assistive_pan(&self, direction: Vec2, unit: SemanticsScrollUnit) {
        let Some((viewport, _)) = self.geometry() else {
            return;
        };
        let distance = |dimension: f64| {
            let page = (dimension * PAGE_FRACTION).max(0.0);
            match unit {
                SemanticsScrollUnit::Page => page,
                _ => KEY_STEP.min(page),
            }
        };
        let available = Vec2::new(
            direction.x * distance(viewport.width()),
            direction.y * distance(viewport.height()),
        );
        self.interrupt();
        let _ = self
            .leaf
            .one_shot(ScrollDevice::Assistive, available, |offer| {
                self.consume(offer, [false, false])
            });
    }

    /// An arrow key: one step of the keyboard sequence its press began.
    fn key_pan(&self, event: &flui_platform_api::keyboard::KeyEvent) -> KeyEventResult {
        if event.state() == KeyState::Up {
            self.leaf.key_released(event.code);
            return KeyEventResult::Ignored;
        }
        if event.composition == ImeComposition::Active
            || !command_modifiers(event.modifiers).is_empty()
        {
            return KeyEventResult::Ignored;
        }
        let direction = match &event.key {
            Key::Named(NamedKey::ArrowLeft) => Vec2::new(-1.0, 0.0),
            Key::Named(NamedKey::ArrowRight) => Vec2::new(1.0, 0.0),
            Key::Named(NamedKey::ArrowUp) => Vec2::new(0.0, -1.0),
            Key::Named(NamedKey::ArrowDown) => Vec2::new(0.0, 1.0),
            _ => return KeyEventResult::Ignored,
        };
        let Some((viewport, _)) = self.geometry() else {
            return KeyEventResult::Ignored;
        };
        let step = |dimension: f64| KEY_STEP.min((dimension * PAGE_FRACTION).max(0.0));
        let available = Vec2::new(
            direction.x * step(viewport.width()),
            direction.y * step(viewport.height()),
        );
        let repeated = event.repeat() == KeyRepeat::AutoRepeat;
        let id = match self.leaf.key_sequence(event.code) {
            Some(id) if repeated => Some(id),
            _ => self.leaf.mint(),
        };
        self.interrupt();
        let consumed = match id {
            Some(id) => self.leaf.contactless_step(
                id,
                ScrollDevice::Keyboard,
                Some(event.code),
                ScrollCause::UserInput,
                available,
                |offer| self.consume(offer, [false, false]),
            ),
            None => self.consume(available, [false, false]),
        };
        if consumed == Vec2::ZERO {
            KeyEventResult::Ignored
        } else {
            KeyEventResult::Handled
        }
    }

    /// A plain wheel tick or a trackpad scroll under
    /// [`WheelScaleGate::CtrlWheel`]: a step of the binding's latch, which
    /// the viewer starts only when it can move.
    fn wheel_pan(&self, data: &ScrollEvent, wheel: Option<&WheelPreferencesProvider>) -> bool {
        if !self.config.get().pan_enabled {
            return false;
        }
        let Some((viewport, _)) = self.geometry() else {
            return false;
        };
        let preferences = wheel
            .map(WheelPreferencesProvider::snapshot)
            .unwrap_or_default();
        let distances = WheelScrollDistances::default();
        let available = Vec2::new(
            wheel_axis_delta(
                Axis::Horizontal,
                data,
                viewport.width(),
                &preferences,
                distances,
            ),
            wheel_axis_delta(
                Axis::Vertical,
                data,
                viewport.height(),
                &preferences,
                distances,
            ),
        );
        if available == Vec2::ZERO {
            return false;
        }
        let Some(id) = self.leaf.dispatched_or_mint() else {
            return false;
        };
        if !self.leaf.leads(id)
            && self.translate(self.lock(Offset::new(-available.x, -available.y)), false)
                == Offset::ZERO
        {
            return false;
        }
        self.interrupt();
        let cause = match data.phase {
            Some(
                ScrollPhase::MomentumBegan
                | ScrollPhase::MomentumChanged
                | ScrollPhase::MomentumEnded,
            ) => ScrollCause::Momentum,
            _ => ScrollCause::UserInput,
        };
        let device = ScrollDevice::Pointer {
            kind: data.pointer.kind,
            device: data.pointer.device,
        };
        let _ = self
            .leaf
            .contactless_step(id, device, None, cause, available, |offer| {
                self.consume(offer, [false, false])
            });
        true
    }
}

/// Answers [`ZoomIntent`] for everything inside the viewer.
struct ZoomAction(Rc<ViewerCore>);

impl Action<ZoomIntent> for ZoomAction {
    fn invoke(&self, _cx: &mut EventCx<'_>, intent: &ZoomIntent) -> ActionOutcome {
        if self.0.zoom_step(*intent) {
            ActionOutcome::Performed
        } else {
            ActionOutcome::NotPerformed
        }
    }
}

/// Answers [`PanIntent`] for everything inside the viewer.
struct PanAction(Rc<ViewerCore>);

impl Action<PanIntent> for PanAction {
    fn invoke(&self, _cx: &mut EventCx<'_>, intent: &PanIntent) -> ActionOutcome {
        if self.0.pan_step(*intent) {
            ActionOutcome::Performed
        } else {
            ActionOutcome::NotPerformed
        }
    }
}

/// Mounts the alternatives around the viewer's content, and resolves their
/// first command from below the viewer's own actions for the double tap.
#[derive(Clone)]
struct AlternativesMount {
    alternatives: GestureAlternatives,
    core: Rc<ViewerCore>,
    content: BoxedView,
}

impl std::fmt::Debug for AlternativesMount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlternativesMount")
            .field("alternatives", &self.alternatives)
            .finish_non_exhaustive()
    }
}

impl View for AlternativesMount {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for AlternativesMount {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let outgoing = self
            .core
            .command
            .replace(Some(self.alternatives.handle(ctx)));
        drop(outgoing);
        self.alternatives.clone().child(RowFocusProbe {
            core: Rc::clone(&self.core),
            child: self.content.clone(),
        })
    }
}

/// Records the focus node of the alternatives' row the viewer sits in.
#[derive(Clone)]
struct RowFocusProbe {
    core: Rc<ViewerCore>,
    child: BoxedView,
}

impl std::fmt::Debug for RowFocusProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RowFocusProbe").finish_non_exhaustive()
    }
}

impl View for RowFocusProbe {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for RowFocusProbe {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let outgoing = self.core.row_focus.replace(Focus::maybe_of(ctx));
        drop(outgoing);
        self.child.clone()
    }
}

/// What a run of the viewer's motion controller drives.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Motion {
    /// Inertia: a friction run of the focal velocity; its value is the
    /// distance travelled.
    Friction,
    /// The overlay shown at a release coming back; the run's value is the
    /// fraction of it left.
    Release(Overlay),
    /// A fling the boundary stopped at `velocity` (viewport pixels per
    /// second): the scene overshoots past the bound and comes back; the
    /// run's value is the overshoot in pixels.
    Overshoot(Offset<f64>),
}

/// The viewer's own motion: its inertia, its steps dispatched under the
/// sequence that released it, and its snap-backs.
#[derive(Debug)]
struct FocalFling {
    controller: AnimationController,
    run: RefCell<Option<Rc<FocalRun>>>,
    clock_bound: Cell<bool>,
    closed: Cell<bool>,
}

impl FocalFling {
    fn new(controller: AnimationController) -> Self {
        Self {
            controller,
            run: RefCell::new(None),
            clock_bound: Cell::new(false),
            closed: Cell::new(false),
        }
    }

    /// Stop the run; its sequence ends for `reason`.
    fn stop(&self, reason: ScrollEndReason) {
        let outgoing = self.run.borrow_mut().take();
        if let Some(run) = outgoing {
            run.finish(reason);
        }
        let _ = self.controller.stop();
    }

    /// Stop inertia, not a snap-back: a press that turns out a tap leaves
    /// the scene coming back inside its bounds.
    fn stop_friction(&self) {
        let coasting = self
            .run
            .borrow()
            .as_ref()
            .is_some_and(|run| run.motion == Motion::Friction);
        if coasting {
            self.stop(ScrollEndReason::Preempted);
        }
    }

    /// Whether the run can start; otherwise its sequence ends at once.
    fn admits(&self, sequence: Option<&Rc<ScrollSequence>>) -> bool {
        if !self.closed.get() && self.clock_bound.get() {
            return true;
        }
        if let Some(sequence) = sequence {
            let mut recovery = PanicRecovery::new();
            sequence.end(ScrollEndReason::Released, &mut recovery);
            recovery.finish();
        }
        false
    }

    /// Coast at `velocity` (viewport pixels per second) as `sequence`'s
    /// motion; a native pinch's run has no sequence. A run that cannot start
    /// ends the sequence at once. A boundary that stops the run turns it
    /// into an overshoot.
    fn start(
        &self,
        core: &Rc<ViewerCore>,
        velocity: Offset<f64>,
        sequence: Option<Rc<ScrollSequence>>,
        post_frame: Option<PostFrameHandle>,
    ) {
        self.stop(ScrollEndReason::Preempted);
        let speed = velocity.dx.hypot(velocity.dy);
        let geometry = core.geometry();
        let simulation = FrictionSimulation::new(0.1, 0.0, speed, Tolerance::DEFAULT);
        let (Some(post_frame), Some((viewport, boundary)), Ok(simulation), true) = (
            post_frame,
            geometry,
            simulation,
            speed.is_finite() && speed > 0.0,
        ) else {
            if let Some(sequence) = sequence {
                let mut recovery = PanicRecovery::new();
                sequence.end(ScrollEndReason::Released, &mut recovery);
                recovery.finish();
            }
            return;
        };
        if !self.admits(sequence.as_ref()) {
            return;
        }
        // Ten percent of the release speed remains after one second. The
        // library simulation supplies finite admission and its rest threshold.
        let direction = velocity / speed;
        let live = Rc::new(Cell::new(true));
        let blocked = Rc::new(Cell::new(None));
        let travelled = Rc::new(Cell::new(0.0_f64));
        let animation = self.controller.clone();
        let weak = Rc::downgrade(core);
        let (step_sequence, step_live, step_blocked) =
            (sequence.clone(), Rc::clone(&live), Rc::clone(&blocked));
        let listener = self.controller.add_listener(Rc::new(move || {
            if !step_live.get() {
                return;
            }
            let Some(core) = weak.upgrade() else {
                return;
            };
            let distance = animation.value();
            if !distance.is_finite() {
                return;
            }
            let step = direction * (distance - travelled.replace(distance));
            if step == Offset::ZERO {
                return;
            }
            let available = Vec2::new(-step.dx, -step.dy);
            let consume = |offer| core.consume(offer, [false, false]);
            let consumed = match &step_sequence {
                Some(sequence) => {
                    let mut recovery = PanicRecovery::new();
                    let consumed = sequence.dispatch(
                        0,
                        ScrollCause::SideEffect,
                        available,
                        consume,
                        &mut recovery,
                    );
                    recovery.finish();
                    consumed
                }
                None => consume(available),
            };
            // An axis the boundary stopped overshoots with what the run
            // still carried on it.
            let stopped =
                |offered: f64, taken: f64| offered != 0.0 && taken.abs() < offered.abs() - 1e-9;
            let speed = animation.velocity();
            let overshoot = Offset::new(
                if stopped(available.x, consumed.x) {
                    direction.dx * speed
                } else {
                    0.0
                },
                if stopped(available.y, consumed.y) {
                    direction.dy * speed
                } else {
                    0.0
                },
            );
            if overshoot != Offset::ZERO && overshoot.is_finite() {
                step_blocked.set(Some(overshoot));
                let _ = animation.stop();
            } else if consumed == Vec2::ZERO {
                let _ = animation.stop();
            }
        }));
        let run = Rc::new(FocalRun {
            listener,
            viewport,
            boundary,
            core: Rc::downgrade(core),
            post_frame,
            animation: self.controller.clone(),
            live,
            sequence,
            motion: Motion::Friction,
            blocked,
        });
        *self.run.borrow_mut() = Some(Rc::clone(&run));
        let _ = self.controller.animate_with(simulation);
        run.schedule();
    }

    /// Bring the scene back inside its bounds: the overlay a release left,
    /// or the overshoot of a fling the boundary stopped. Without a clock the
    /// scene is back at once. The run is `sequence`'s last.
    fn snap_back(
        &self,
        core: &Rc<ViewerCore>,
        motion: Motion,
        sequence: Option<Rc<ScrollSequence>>,
        post_frame: Option<PostFrameHandle>,
    ) {
        self.stop(ScrollEndReason::Preempted);
        let simulation = match motion {
            Motion::Release(_) => stretch::release_run(),
            Motion::Overshoot(velocity) => stretch::overshoot_run(velocity.dx.hypot(velocity.dy)),
            Motion::Friction => return,
        };
        let rest = || {
            core.overlay.set(Overlay::default());
            core.travel.set(Travel::default());
            core.publish(core.base());
        };
        let (Some(post_frame), Some((viewport, boundary)), Ok(simulation)) =
            (post_frame, core.geometry(), simulation)
        else {
            rest();
            if let Some(sequence) = sequence {
                let mut recovery = PanicRecovery::new();
                sequence.end(ScrollEndReason::Released, &mut recovery);
                recovery.finish();
            }
            return;
        };
        if !self.admits(sequence.as_ref()) {
            rest();
            return;
        }
        let live = Rc::new(Cell::new(true));
        let animation = self.controller.clone();
        let weak = Rc::downgrade(core);
        let step_live = Rc::clone(&live);
        let listener = self.controller.add_listener(Rc::new(move || {
            if !step_live.get() {
                return;
            }
            let Some(core) = weak.upgrade() else {
                return;
            };
            let value = animation.value();
            if !value.is_finite() {
                return;
            }
            let overlay = match motion {
                Motion::Release(overlay) => overlay.scaled(value.clamp(0.0, 1.0)),
                Motion::Overshoot(velocity) => {
                    let speed = velocity.dx.hypot(velocity.dy);
                    Overlay {
                        pan: velocity / speed * value.max(0.0),
                        ..Overlay::default()
                    }
                }
                Motion::Friction => return,
            };
            core.overlay.set(overlay);
            core.publish(core.base());
        }));
        let run = Rc::new(FocalRun {
            listener,
            viewport,
            boundary,
            core: Rc::downgrade(core),
            post_frame,
            animation: self.controller.clone(),
            live,
            sequence,
            motion,
            blocked: Rc::new(Cell::new(None)),
        });
        *self.run.borrow_mut() = Some(Rc::clone(&run));
        let _ = self.controller.animate_with(simulation);
        run.schedule();
    }

    fn close(&self) {
        self.closed.set(true);
        self.clock_bound.set(false);
        self.stop(ScrollEndReason::Cancelled);
    }
}

/// Completed layout validates the immutable limits before the next motion tick.
/// Publication remains on the Vsync listener, before build and paint.
struct FocalRun {
    listener: ListenerId,
    viewport: Rect<f64>,
    boundary: Rect<f64>,
    core: Weak<ViewerCore>,
    post_frame: PostFrameHandle,
    animation: AnimationController,
    live: Rc<Cell<bool>>,
    sequence: Option<Rc<ScrollSequence>>,
    motion: Motion,
    /// The velocity a boundary stopped a friction run with.
    blocked: Rc<Cell<Option<Offset<f64>>>>,
}

impl std::fmt::Debug for FocalRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocalRun")
            .field("live", &self.live.get())
            .field("motion", &self.motion)
            .finish_non_exhaustive()
    }
}

impl FocalRun {
    /// Detach the run once. Returns whether it was still live.
    fn detach(&self) -> bool {
        if !self.live.replace(false) {
            return false;
        }
        self.animation.remove_listener(self.listener);
        let _ = self.animation.stop();
        true
    }

    /// End the run once: detach it and end its sequence for `reason`. A
    /// snap-back that does not hand the scene to new input leaves it inside
    /// its bounds.
    fn finish(&self, reason: ScrollEndReason) {
        if !self.detach() {
            return;
        }
        if self.motion != Motion::Friction
            && reason != ScrollEndReason::Preempted
            && let Some(core) = self.core.upgrade()
        {
            core.overlay.set(Overlay::default());
            core.travel.set(Travel::default());
            core.publish(core.base());
        }
        if let Some(sequence) = &self.sequence {
            let mut recovery = PanicRecovery::new();
            sequence.end(reason, &mut recovery);
            recovery.finish();
        }
    }

    fn schedule(self: Rc<Self>) {
        let handle = self.post_frame.clone();
        let pending = Rc::downgrade(&self);
        if handle
            .schedule(move |_| {
                if let Some(run) = pending.upgrade() {
                    run.tick();
                }
            })
            .is_err()
        {
            self.finish(ScrollEndReason::Cancelled);
        }
    }

    fn tick(self: Rc<Self>) {
        if !self.live.get() {
            return;
        }
        let core = self.core.upgrade();
        let geometry = core.as_ref().and_then(|core| core.geometry());
        if geometry != Some((self.viewport, self.boundary)) {
            self.finish(ScrollEndReason::Cancelled);
            return;
        }
        if let Some(velocity) = self.blocked.take()
            && let Some(core) = core
        {
            // The boundary stopped the coast: the same sequence overshoots.
            self.detach();
            core.fling.snap_back(
                &core,
                Motion::Overshoot(velocity),
                self.sequence.clone(),
                Some(self.post_frame.clone()),
            );
            return;
        }
        if self.animation.is_animating() {
            self.schedule();
        } else {
            self.finish(ScrollEndReason::Released);
        }
    }
}

/// Persistent state for [`InteractiveViewer`].
pub struct InteractiveViewerState {
    core: Rc<ViewerCore>,
    /// The node the arrow keys reach this viewer through, from a focused row
    /// inside it.
    keys: Rc<FocusNode>,
    /// Ends the keyboard sequence a held arrow began when the key is
    /// cancelled instead of released here.
    _key_cancel: FocusNodeRegistration,
    writer: Option<WriterSource>,
    post_frame: Option<PostFrameHandle>,
    controller: DrivenController,
    /// Identity only: the driven controller owns the registry seat.
    clock: Option<Vsync>,
    wheel_preferences: Option<WheelPreferencesProvider>,
}

impl std::fmt::Debug for InteractiveViewerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InteractiveViewerState")
            .field("config", &self.core.config.get())
            .finish_non_exhaustive()
    }
}

impl ViewState<InteractiveViewer> for InteractiveViewerState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.core.pipeline.borrow_mut() = ctx.pipeline_owner();
        self.did_change_dependencies(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let incoming = ctx.pipeline_owner();
        let unchanged = match (&*self.core.pipeline.borrow(), &incoming) {
            (None, None) => true,
            (Some(current), Some(incoming)) => current.ptr_eq(incoming),
            _ => false,
        };
        if !unchanged {
            self.core.fling.stop(ScrollEndReason::Cancelled);
        }
        *self.core.pipeline.borrow_mut() = incoming;
        self.post_frame = ctx.post_frame_handle();
        self.writer = Some(ctx.writer_source());
        let leaf = &self.core.leaf;
        *leaf.parent.borrow_mut() = NestedScrollScope::parent_of(ctx);
        *leaf.arena.borrow_mut() = Some(GestureArenaScope::of(ctx));
        *leaf.settings.borrow_mut() = GestureArenaScope::settings_or_default_of(ctx);
        self.wheel_preferences = Some(GestureArenaScope::wheel_preferences_of(ctx));
        self.bind_vsync(ctx);
    }

    fn did_update_view(&mut self, old: &InteractiveViewer, view: &InteractiveViewer) {
        let core = &self.core;
        let incoming = view.config;
        let outgoing = core.config.replace(incoming);
        let callbacks = core.callbacks.replace(ViewerCallbacks::of(view));
        drop(callbacks);
        let controller = core.controller.replace(view.controller.clone());
        drop(controller);
        if !old.controller.ptr_eq(&view.controller)
            || outgoing.boundary_margin != incoming.boundary_margin
            || outgoing.pan_enabled != incoming.pan_enabled
            || outgoing.pan_axis != incoming.pan_axis
            || outgoing.min_scale != incoming.min_scale
            || outgoing.max_scale != incoming.max_scale
            || outgoing.rotation_enabled != incoming.rotation_enabled
            || old.alignment != view.alignment
        {
            core.fling.stop(ScrollEndReason::Cancelled);
        }
    }

    fn dispose(&mut self) {
        self.core.fling.close();
        self.core.leaf.retire();
        self.controller.dispose();
    }

    fn build(&self, view: &InteractiveViewer, _ctx: &dyn BuildContext) -> impl IntoView {
        let core = Rc::clone(&self.core);
        let content = self.content(view);
        let mount = AlternativesMount {
            alternatives: view.alternatives.clone(),
            core: Rc::clone(&core),
            content,
        };
        let key_core = Rc::clone(&core);
        let keys = Focus::new(mount)
            .focus_node(Rc::clone(&self.keys))
            .can_request_focus(false)
            .on_key_event(move |_cx, event| key_core.key_pan(event));
        let mut shortcuts = Shortcuts::new(keys);
        for (activator, intent) in zoom_keys(TargetPlatform::current()) {
            shortcuts = shortcuts.shortcut(activator, intent);
        }
        Actions::new(shortcuts)
            .action(ZoomAction(Rc::clone(&core)))
            .action(PanAction(core))
    }
}

/// The keys that zoom a focused viewer on `platform`: `+` (or `=`, its
/// unshifted key) and `-`, alone or with the platform's command key.
fn zoom_keys(platform: TargetPlatform) -> Vec<(SingleActivator, ZoomIntent)> {
    let apple = matches!(platform, TargetPlatform::MacOS | TargetPlatform::iOS);
    let command = |activator: SingleActivator| {
        if apple {
            activator.meta()
        } else {
            activator.control()
        }
    };
    let mut keys = Vec::new();
    for (character, intent) in [
        ("+", ZoomIntent::In),
        ("=", ZoomIntent::In),
        ("-", ZoomIntent::Out),
    ] {
        keys.push((SingleActivator::character_ignoring_shift(character), intent));
        keys.push((
            command(SingleActivator::character_ignoring_shift(character)),
            intent,
        ));
    }
    keys
}

impl InteractiveViewerState {
    /// The transformed, clipped and recognized child, rebuilt whenever the
    /// transform changes.
    #[expect(clippy::too_many_lines)] // one gesture-wiring closure; splitting fragments the capture set
    fn content(&self, view: &InteractiveViewer) -> BoxedView {
        let core = Rc::clone(&self.core);
        let post_frame = self.post_frame.clone();
        let writer = self
            .writer
            .clone()
            .expect("BUG: InteractiveViewer initialized before build");
        let wheel_preferences = self.wheel_preferences.clone();
        let clip_behavior = view.clip_behavior;
        let alignment = view.alignment;
        let child = view.child.clone();
        let listenable = core.controller().listenable();

        AnimatedBuilder::new(listenable, move || {
            let config = core.config.get();
            let matrix = core.controller().value();

            // -- Contacts and the native pinch (one transform slot) --------
            let start_core = Rc::clone(&core);
            let on_start = move |cx: &mut EventCx<'_>, details: ScaleStartDetails| {
                let core = &start_core;
                // A gesture that catches a snap-back continues from where the
                // scene shows.
                core.fling.stop(ScrollEndReason::Preempted);
                core.catch_stretch();
                core.gesture.reset();
                core.gesture
                    .pan_start_local
                    .set(Some(details.local_focal_point));
                let native = details.pointer_count == 0;
                core.gesture.native.set(native);
                if !native {
                    core.leaf.begin_contact();
                    // A contact gives the viewer the keys, as a drag gives a
                    // scrollable its own.
                    let row = core.row_focus.borrow().clone();
                    if let Some(row) = row {
                        let _ = row.request_focus();
                    }
                }
                if let Some(callback) = core.callbacks().start {
                    callback(
                        cx,
                        InteractionStartDetails {
                            focal_point: details.focal_point,
                            local_focal_point: details.local_focal_point,
                        },
                    );
                }
            };

            let update_core = Rc::clone(&core);
            let on_update = move |cx: &mut EventCx<'_>, details: ScaleUpdateDetails| {
                let core = &update_core;
                let config = core.config.get();
                let before = core.controller().value();
                let gesture = &core.gesture;
                // In logarithms: the ratio of two extreme cumulative factors overflows.
                let log_ratio = details.scale.ln() - gesture.scale.replace(details.scale).ln();
                let rotation = details.rotation - gesture.rotation.replace(details.rotation);
                let delta = details.focal_point_delta;
                if config.pan_axis == PanAxis::Aligned
                    && gesture.current_axis.get().is_none()
                    && let Some(start_local) = gesture.pan_start_local.get()
                {
                    let total = details.local_focal_point - start_local;
                    if total != Offset::ZERO {
                        gesture.current_axis.set(Some(dominant_axis(total)));
                    }
                }
                let locked = core.lock(delta);
                // Scale and rotate about the focal point where it was; the
                // focal motion follows as a pan.
                let log_ratio = if config.scale_enabled && log_ratio.is_finite() {
                    log_ratio
                } else {
                    0.0
                };
                let rotation = if config.rotation_enabled {
                    rotation
                } else {
                    0.0
                };
                if log_ratio != 0.0 || rotation != 0.0 {
                    core.pinch(details.local_focal_point - delta, log_ratio, rotation);
                }
                if gesture.native.get() {
                    if config.pan_enabled {
                        let _ = core.translate(locked, true);
                    }
                } else if delta != Offset::ZERO {
                    let _ = core
                        .leaf
                        .contact_step(Vec2::new(-delta.dx, -delta.dy), |offer| {
                            core.consume(offer, core.leaf.contact_holds())
                        });
                }
                if let Some(callback) = core.callbacks().update {
                    callback(
                        cx,
                        InteractionUpdateDetails {
                            focal_point: details.focal_point,
                            local_focal_point: details.local_focal_point,
                            scale: uniform_scale(&core.controller().value())
                                / uniform_scale(&before),
                            focal_point_delta: if config.pan_enabled {
                                locked
                            } else {
                                Offset::ZERO
                            },
                        },
                    );
                }
            };

            let end_core = Rc::clone(&core);
            let end_post_frame = post_frame.clone();
            let on_end = move |cx: &mut EventCx<'_>, details: ScaleEndDetails| {
                let core = &end_core;
                let config = core.config.get();
                let velocity = core.lock(details.focal_fling_velocity().pixels_per_second);
                core.gesture.reset();
                let native = core.gesture.native.replace(false);
                let overlay = core.overlay.get();
                let sequence = if native {
                    None
                } else {
                    core.leaf.release(Vec2::new(-velocity.dx, -velocity.dy))
                };
                if !overlay.is_rest() {
                    // Pulled past the bounds: the scene comes back, and no
                    // inertia runs from outside them.
                    core.fling.snap_back(
                        core,
                        Motion::Release(overlay),
                        sequence.map(|(sequence, _)| sequence),
                        end_post_frame.clone(),
                    );
                } else if native {
                    if config.pan_enabled {
                        core.fling
                            .start(core, velocity, None, end_post_frame.clone());
                    }
                } else if let Some((sequence, held)) = sequence {
                    let own = Offset::new(
                        if held[0] { velocity.dx } else { 0.0 },
                        if held[1] { velocity.dy } else { 0.0 },
                    );
                    let own = if config.pan_enabled {
                        own
                    } else {
                        Offset::ZERO
                    };
                    core.fling
                        .start(core, own, Some(sequence), end_post_frame.clone());
                }
                if let Some(callback) = core.callbacks().end {
                    callback(
                        cx,
                        InteractionEndDetails {
                            reason: GestureEndReason::Completed,
                            velocity: details.focal_velocity,
                            scale_velocity: details.velocity,
                        },
                    );
                }
            };

            let cancel_core = Rc::clone(&core);
            let cancel_post_frame = post_frame.clone();
            let on_cancel = move |cx: &mut EventCx<'_>| {
                let core = &cancel_core;
                core.fling.stop(ScrollEndReason::Cancelled);
                core.gesture.reset();
                core.gesture.native.set(false);
                core.leaf.cancel();
                let overlay = core.overlay.get();
                if !overlay.is_rest() {
                    core.fling.snap_back(
                        core,
                        Motion::Release(overlay),
                        None,
                        cancel_post_frame.clone(),
                    );
                }
                if let Some(callback) = core.callbacks().end {
                    callback(
                        cx,
                        InteractionEndDetails {
                            reason: GestureEndReason::Cancelled,
                            velocity: Velocity::ZERO,
                            scale_velocity: 0.0,
                        },
                    );
                }
            };

            // Reject an idle native claimant before it can mutate its actor.
            // Once admitted, GestureDetector owns the source through terminal.
            let admission_core = Rc::clone(&core);
            let native_admission = move |event: &PanZoomEvent, translation: Offset<f64>| {
                let core = &admission_core;
                let config = core.config.get();
                let PanZoomPhase::Update(transform) = event.phase else {
                    return false;
                };
                let Some((viewport, boundary)) = core.geometry() else {
                    return false;
                };
                // Translation is the focal point moving since Start
                // (ADR-0217 §4).
                let point = event.position.get();
                let delta = if config.pan_enabled {
                    match config.pan_axis {
                        PanAxis::Horizontal => align_to_axis(translation, Axis::Horizontal),
                        PanAxis::Vertical => align_to_axis(translation, Axis::Vertical),
                        PanAxis::Aligned if translation != Offset::ZERO => {
                            align_to_axis(translation, dominant_axis(translation))
                        }
                        _ => translation,
                    }
                } else {
                    Offset::ZERO
                };
                let focal = Offset::new(point.x, point.y);
                let before = core.base();
                gesture_transform(
                    before,
                    focal,
                    delta,
                    if config.scale_enabled {
                        transform.scale()
                    } else {
                        1.0
                    },
                    if config.rotation_enabled {
                        transform.rotation()
                    } else {
                        0.0
                    },
                    config.min_scale,
                    config.max_scale,
                    viewport,
                    boundary,
                )
                .is_some_and(|next| next != before)
            };

            // -- Wheel (Listener::on_scroll_claim) -------------------------
            //
            // The viewer goes through the arbitrated claim walk rather than
            // acting on the raw pointer signal: acting directly would make a
            // viewer nested in a scrollable both zoom and scroll on one wheel
            // tick. When the tick will actually zoom or pan, the viewer
            // claims it and the outer scrollable stays still.
            let wheel_core = Rc::clone(&core);
            let wheel_writer = writer.clone();
            let wheel_preferences = wheel_preferences.clone();
            let scroll_claim = move |data: &ScrollEvent| {
                wheel_writer.write(|cx| {
                    let core = &wheel_core;
                    if core.config.get().wheel_scale_gate == WheelScaleGate::CtrlWheel
                        && !data.modifiers.contains(Modifiers::CONTROL)
                    {
                        return if core.wheel_pan(data, wheel_preferences.as_ref()) {
                            EventPropagation::Stop
                        } else {
                            EventPropagation::Continue
                        };
                    }
                    wheel_zoom(core, cx, data)
                })
            };

            let mut transform = Transform::new(matrix);
            if let Some(inner_child) = child.clone().into_inner() {
                transform = transform.child(AnchoredBox::new(core.anchor.clone(), inner_child));
            }
            if let Some(alignment) = alignment {
                transform = transform.alignment(alignment);
            }
            let clipped = ClipRect::new()
                .clip_behavior(clip_behavior)
                .child(transform);

            let mut recognized = GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
                .transform(
                    TransformSlot::new()
                        .start_mode(ScaleStartMode::PanOrScale)
                        .native_admission(native_admission)
                        .on_start(on_start)
                        .on_update(on_update)
                        .on_end(on_end)
                        .on_cancel(on_cancel),
                );
            if config.double_tap_zoom {
                let hint_core = Rc::clone(&core);
                let tap_core = Rc::clone(&core);
                recognized = recognized
                    .on_double_tap_down(move |_cx, details| {
                        hint_core.focal_hint.set(Some(details.local_position));
                    })
                    .on_double_tap(move |cx| double_tap(&tap_core, cx));
            }
            let recognized = recognized.child(clipped);

            let down_core = Rc::clone(&core);
            let listener = Listener::new()
                .on_pointer_down(move |_, dispatch| {
                    if let PointerEvent::Down(press) = dispatch.local {
                        down_core.leaf.device.set(Some(ScrollDevice::Pointer {
                            kind: press.pointer.kind,
                            device: press.pointer.device,
                        }));
                    }
                    down_core.fling.stop_friction();
                })
                .on_scroll_claim(scroll_claim)
                .child(recognized);
            viewer_semantics(&core, &config, &matrix).child(listener)
        })
        .boxed()
    }

    fn bind_vsync(&mut self, ctx: &dyn LifecycleContext) {
        let incoming = VsyncScope::maybe_of(ctx);
        let unchanged = match (&self.clock, &incoming) {
            (None, None) => true,
            (Some(current), Some(incoming)) => current.is_same(incoming),
            _ => false,
        };
        if unchanged {
            return;
        }
        self.clock = incoming;
        let fling = &self.core.fling;
        fling.clock_bound.set(false);
        fling.stop(ScrollEndReason::Cancelled);
        let rebound = self.controller.rebind(self.clock.as_ref());
        fling.clock_bound.set(self.controller.is_bound());
        if let Err(error) = rebound {
            tracing::error!(%error, "InteractiveViewer lost its frame registry");
        }
    }
}

/// A double tap: the first command about the tapped point, or, at the
/// maximum scale, back out to the minimum.
fn double_tap(core: &ViewerCore, cx: &mut EventCx<'_>) {
    let config = core.config.get();
    let at_max = core.scale() >= config.max_scale * (1.0 - 1e-9);
    if at_max {
        let focal = core.focal_hint.take();
        core.interrupt();
        let _ = core.zoom_by(config.min_scale / core.scale(), focal);
        return;
    }
    let command = core.command.borrow().clone();
    if let Some(command) = command {
        let _ = command.perform(cx);
    }
    core.focal_hint.set(None);
}

/// The viewer's semantics node: its scale as a numeric range that
/// increment, decrement and set-value adjust, and the scroll actions that pan.
fn viewer_semantics(core: &Rc<ViewerCore>, config: &ViewerConfig, matrix: &Matrix4) -> Semantics {
    let scale = uniform_scale(matrix);
    let percent = if scale.is_finite() {
        (scale * 100.0).round()
    } else {
        0.0
    };
    let zoom = |intent: ZoomIntent| {
        let core = Rc::clone(core);
        move |_cx: &mut EventCx<'_>| {
            let _ = core.zoom_step(intent);
        }
    };
    let pan = |direction: Vec2| {
        let core = Rc::clone(core);
        move |_cx: &mut EventCx<'_>, unit: SemanticsScrollUnit| core.assistive_pan(direction, unit)
    };
    let mut semantics = Semantics::new()
        .value(format!("{percent}%"))
        .on_increment(zoom(ZoomIntent::In))
        .on_decrement(zoom(ZoomIntent::Out))
        .on_scroll_left(pan(Vec2::new(-1.0, 0.0)))
        .on_scroll_right(pan(Vec2::new(1.0, 0.0)))
        .on_scroll_up(pan(Vec2::new(0.0, -1.0)))
        .on_scroll_down(pan(Vec2::new(0.0, 1.0)));
    let (min, max) = (config.min_scale, config.max_scale);
    if min <= max
        && let Ok(range) = NumericRange::new(
            scale.clamp(min, max),
            min,
            max,
            (scale * (ZOOM_STEP - 1.0)).max(f64::MIN_POSITIVE),
        )
    {
        let set_core = Rc::clone(core);
        semantics = semantics
            .numeric_range(range)
            .on_set_numeric_value(move |_cx, target: f64| {
                let current = set_core.scale();
                set_core.interrupt();
                let _ = set_core.zoom_by(target / current, None);
            });
    }
    semantics
}

/// A wheel tick that zooms about the pointer.
fn wheel_zoom(core: &ViewerCore, cx: &mut EventCx<'_>, data: &ScrollEvent) -> EventPropagation {
    let config = core.config.get();
    let geometry = core.geometry();
    let pixels_per_unit = match data.delta.unit() {
        ScrollUnit::Pixels => 1.0,
        // Raw rotation and already normalized lines both retain the authored
        // zoom step. Scroll preferences govern translation, independently of
        // the zoom divisor.
        ScrollUnit::Detents | ScrollUnit::Lines => 53.0,
        ScrollUnit::Pages => {
            let Some((viewport, _)) = geometry else {
                return EventPropagation::Continue;
            };
            let height = viewport.height();
            if !height.is_finite() || height <= 0.0 {
                return EventPropagation::Continue;
            }
            height
        }
        _ => return EventPropagation::Continue,
    };
    let delta = data.delta.y() * pixels_per_unit;
    if delta == 0.0 || !delta.is_finite() {
        // Ignore horizontal-only wheel scroll.
        return EventPropagation::Continue;
    }

    let position = data.position.get();
    let position = Offset::new(position.x, position.y);
    let scale_change = (-delta / config.scale_factor).exp();
    if !scale_change.is_finite() || scale_change <= 0.0 {
        return EventPropagation::Continue;
    }
    core.interrupt();
    let callbacks = core.callbacks();
    if let Some(callback) = &callbacks.start {
        callback(
            cx,
            InteractionStartDetails {
                focal_point: position,
                local_focal_point: position,
            },
        );
    }

    let controller = core.controller();
    let value_before_zoom = controller.value();
    if config.scale_enabled
        && let Some((viewport, boundary)) = geometry
    {
        let scene_before = controller.to_scene(position);
        let scaled = clamp_scale(
            controller.value(),
            scale_change,
            config.min_scale,
            config.max_scale,
            viewport,
            boundary,
        );
        controller.set_value(scaled);

        // Keep the same scene point under the cursor before and after the
        // scale.
        let scene_after = controller.to_scene(position);
        let correction = Offset::new(
            scene_after.dx - scene_before.dx,
            scene_after.dy - scene_before.dy,
        );
        let translated = clamp_translation(controller.value(), correction, viewport, boundary);
        controller.set_value(translated);
    }

    if let Some(callback) = &callbacks.update {
        callback(
            cx,
            InteractionUpdateDetails {
                focal_point: position,
                local_focal_point: position,
                scale: scale_change,
                focal_point_delta: Offset::ZERO,
            },
        );
    }
    if let Some(callback) = &callbacks.end {
        callback(
            cx,
            InteractionEndDetails {
                reason: GestureEndReason::Completed,
                velocity: Velocity::ZERO,
                scale_velocity: 0.0,
            },
        );
    }
    // Claim only when the viewer actually zoomed — the same shape as the
    // scrollable's can-move predicate. Scaling disabled, or a zoom the
    // boundary/min/max clamp collapsed to a no-op (e.g. zoom-out at identity
    // with a zero boundary margin), leaves the tick to an enclosing
    // scrollable; the interaction callbacks above still observed it even
    // then.
    if controller.value().m == value_before_zoom.m {
        EventPropagation::Continue
    } else {
        EventPropagation::Stop
    }
}

/// `moved` limited to `[0, wanted]` (or `[wanted, 0]`): what a boundary
/// correction adds beyond or against a step is not the step's.
fn toward(moved: f64, wanted: f64) -> f64 {
    if !moved.is_finite() || wanted == 0.0 {
        0.0
    } else if wanted > 0.0 {
        moved.clamp(0.0, wanted)
    } else {
        moved.clamp(wanted, 0.0)
    }
}

// ============================================================================
// Matrix math — boundary-clamped translate/scale
// ============================================================================

/// Compose in viewport space, then keep the same scene point under the moving
/// focal point. Publish only an invertible finite matrix inside the boundary.
#[expect(clippy::too_many_arguments)]
fn gesture_transform(
    matrix: Matrix4,
    focal: Offset<f64>,
    delta: Offset<f64>,
    scale: f64,
    rotation: f64,
    min_scale: f64,
    max_scale: f64,
    viewport: Rect<f64>,
    boundary: Rect<f64>,
) -> Option<Matrix4> {
    if !focal.is_finite()
        || !delta.is_finite()
        || scale.is_nan()
        || scale < 0.0
        || !rotation.is_finite()
        || !viewport.is_finite()
        || viewport.width() <= 0.0
        || viewport.height() <= 0.0
    {
        return None;
    }
    if delta == Offset::ZERO && scale == 1.0 && rotation == 0.0 {
        return contain_transform(matrix, viewport, boundary);
    }
    let previous_focal = focal - delta;
    if !previous_focal.is_finite() {
        return None;
    }
    let inverse = matrix.try_inverse()?;
    let pivot = inverse.transform_point(previous_focal.dx, previous_focal.dy);
    if !pivot.0.is_finite() || !pivot.1.is_finite() {
        return None;
    }
    let current_scale = uniform_scale(&matrix);
    if !current_scale.is_finite() || current_scale <= 0.0 {
        return None;
    }
    let scaled = clamp_scale(matrix, scale, min_scale, max_scale, viewport, boundary);
    let mut next = Matrix4::rotation_z(rotation) * scaled;
    let mapped = next.transform_point(pivot.0, pivot.1);
    let correction = focal - Offset::new(mapped.0, mapped.1);
    if !correction.is_finite() {
        return None;
    }
    next = Matrix4::translation(correction.dx, correction.dy, 0.0) * next;
    contain_transform(next, viewport, boundary)
}

fn contain_transform(
    mut matrix: Matrix4,
    viewport: Rect<f64>,
    boundary: Rect<f64>,
) -> Option<Matrix4> {
    if !matrix
        .to_col_major_array()
        .iter()
        .all(|value| value.is_finite())
    {
        return None;
    }
    let values = matrix.to_col_major_array();
    if boundary.is_finite()
        && values[3] == 0.0
        && values[7] == 0.0
        && values[11] == 0.0
        && values[15] == 1.0
    {
        // Separate translation from the viewport's shape. Subtracting a huge
        // excess from a huge translation loses the small boundary coordinate.
        let (x, y, z) = matrix.translation_component();
        let mut linear = matrix;
        linear.set_translation(0.0, 0.0, 0.0);
        let inverse = linear.try_inverse()?;
        let shape = inverse.transform_rect(&viewport);
        if !shape.is_finite()
            || shape.width() > boundary.width() + EXCESS_EPSILON
            || shape.height() > boundary.height() + EXCESS_EPSILON
        {
            return None;
        }
        let shift = inverse.transform_point(-x, -y);
        if !shift.0.is_finite() || !shift.1.is_finite() {
            return None;
        }
        let lower = Offset::new(boundary.min.x - shape.min.x, boundary.min.y - shape.min.y);
        let upper = Offset::new(boundary.max.x - shape.max.x, boundary.max.y - shape.max.y);
        if !lower.is_finite() || !upper.is_finite() {
            return None;
        }
        let clamped = Offset::new(
            shift.0.clamp(lower.dx, upper.dx.max(lower.dx)),
            shift.1.clamp(lower.dy, upper.dy.max(lower.dy)),
        );
        if clamped.dx != shift.0 || clamped.dy != shift.1 {
            let translated = linear.transform_point(-clamped.dx, -clamped.dy);
            if !translated.0.is_finite() || !translated.1.is_finite() {
                return None;
            }
            matrix.set_translation(translated.0, translated.1, z);
        }
    }
    let inverse = matrix.try_inverse()?;
    let scene_viewport = inverse.transform_rect(&viewport);
    if !scene_viewport.is_finite() {
        return None;
    }
    if !boundary.is_finite() {
        return (![
            boundary.min.x,
            boundary.min.y,
            boundary.max.x,
            boundary.max.y,
        ]
        .iter()
        .any(|value| value.is_nan()))
        .then_some(matrix);
    }
    // A rotated viewport is a quad; its bounding rectangle fits this
    // axis-aligned scene boundary exactly when all four corners fit. If its
    // extent cannot fit at the admitted scale, reject rather than invent zoom.
    if scene_viewport.width() > boundary.width() + EXCESS_EPSILON
        || scene_viewport.height() > boundary.height() + EXCESS_EPSILON
    {
        return None;
    }
    let excess = rect_excess(boundary, scene_viewport);
    if !excess.is_finite() {
        return None;
    }
    if !excess_is_negligible(excess) {
        matrix *= Matrix4::translation(-excess.dx, -excess.dy, 0.0);
    }
    let result = matrix.try_inverse()?.transform_rect(&viewport);
    (matrix
        .to_col_major_array()
        .iter()
        .all(|value| value.is_finite())
        && result.is_finite()
        && excess_is_negligible(rect_excess(boundary, result)))
    .then_some(matrix)
}

/// Uniform scale of a translated/rotated matrix: length of its x basis vector.
fn uniform_scale(matrix: &Matrix4) -> f64 {
    let m = matrix.to_col_major_array();
    m[0].hypot(m[1])
}

/// Transforms `viewport`'s four corners by the inverse of `matrix` and
/// returns their axis-aligned bounding box — the viewport's rect in scene
/// coordinates after the child has been transformed by `matrix`. Falls back
/// to `viewport` unchanged if `matrix` is singular (should not happen for a
/// translation + uniform-scale matrix with a non-zero scale).
fn transform_viewport(matrix: Matrix4, viewport: Rect<f64>) -> Rect<f64> {
    match matrix.try_inverse() {
        Some(inverse) => inverse.transform_rect(&viewport),
        None => viewport,
    }
}

/// How far `[view_min, view_max]` lies outside `[bound_min, bound_max]` along
/// one axis, signed so that adding it to the viewport's position moves it
/// back inside the boundary. Zero when already inside (inclusive).
///
/// The caller supplies the transformed viewport's bounding rectangle, so the
/// interval comparison also covers rotation. If the viewport is wider than
/// the boundary, the edge with the larger-magnitude excess wins.
fn axis_excess(view_min: f64, view_max: f64, bound_min: f64, bound_max: f64) -> f64 {
    let excess_min = if view_min < bound_min {
        bound_min - view_min
    } else {
        0.0
    };
    let excess_max = if view_max > bound_max {
        bound_max - view_max
    } else {
        0.0
    };
    if excess_min.abs() >= excess_max.abs() {
        excess_min
    } else {
        excess_max
    }
}

fn rect_excess(boundary: Rect<f64>, viewport: Rect<f64>) -> Offset<f64> {
    Offset::new(
        axis_excess(
            viewport.min.x,
            viewport.max.x,
            boundary.min.x,
            boundary.max.x,
        ),
        axis_excess(
            viewport.min.y,
            viewport.max.y,
            boundary.min.y,
            boundary.max.y,
        ),
    )
}

/// Floating-point tolerance for the "did this transform round-trip produce
/// zero excess" checks in [`clamp_translation`].
///
/// The viewport's inverse-then-transform round trip leaves residue that
/// *should* be exactly zero but isn't once the matrix carries a non-unit (and
/// non-power-of-two) scale. This snaps anything within `EXCESS_EPSILON` of
/// zero back to exactly zero. Chosen against the scale of one gesture's excess (tens to
/// thousands of pixels) rather than absolute machine epsilon — comfortably
/// larger than the ~1e-4 residue a `scale * (a - b)` round trip leaves at
/// these magnitudes, comfortably smaller than any excess a real boundary
/// hit produces.
const EXCESS_EPSILON: f64 = 1e-3;

/// Whether a single excess component is within [`EXCESS_EPSILON`] of zero.
fn is_negligible(component: f64) -> bool {
    component.abs() < EXCESS_EPSILON
}

/// Whether `excess` is within [`EXCESS_EPSILON`] of `Offset::ZERO` on both
/// axes — the round-trip-tolerant replacement for `excess == Offset::ZERO`.
fn excess_is_negligible(excess: Offset<f64>) -> bool {
    is_negligible(excess.dx) && is_negligible(excess.dy)
}

/// Locks a `PanAxis::Aligned` drag to whichever axis dominates `delta`.
/// `delta` must be non-zero (callers only invoke this on real movement).
fn dominant_axis(delta: Offset<f64>) -> Axis {
    if delta.dx.abs() > delta.dy.abs() {
        Axis::Horizontal
    } else {
        Axis::Vertical
    }
}

/// Zeroes out the off-axis component of `delta`.
fn align_to_axis(delta: Offset<f64>, axis: Axis) -> Offset<f64> {
    match axis {
        Axis::Horizontal => Offset::new(delta.dx, 0.0),
        Axis::Vertical => Offset::new(0.0, delta.dy),
    }
}

/// Applies `translation` (in scene units) to `matrix`, clamped so the
/// transformed viewport stays within `boundary` when `boundary` is finite.
///
/// Composed via
/// `matrix * Matrix4::translation(..)` (post-multiply — the translation
/// happens in the matrix's own local/scene space before the rest of the
/// transform is applied). `flui_foundation::geometry`'s
/// own `Matrix4::translate` mutator has the *opposite* (pre-multiply,
/// global-space) convention and must not be used here.
fn clamp_translation(
    matrix: Matrix4,
    translation: Offset<f64>,
    viewport: Rect<f64>,
    boundary: Rect<f64>,
) -> Matrix4 {
    if translation == Offset::ZERO {
        return matrix;
    }
    let next = matrix * Matrix4::translation(translation.dx, translation.dy, 0.0);

    if !boundary.is_finite() {
        return next;
    }

    let next_viewport = transform_viewport(next, viewport);
    let excess = rect_excess(boundary, next_viewport);
    if excess_is_negligible(excess) {
        return next;
    }

    let (next_tx, next_ty, next_tz) = next.translation_component();
    let current_scale = uniform_scale(&matrix);
    let corrected_tx = next_tx - excess.dx * current_scale;
    let corrected_ty = next_ty - excess.dy * current_scale;
    let mut corrected = matrix;
    corrected.set_translation(corrected_tx, corrected_ty, next_tz);

    let corrected_viewport = transform_viewport(corrected, viewport);
    let corrected_excess = rect_excess(boundary, corrected_viewport);
    if excess_is_negligible(corrected_excess) {
        return corrected;
    }

    if !is_negligible(corrected_excess.dx) && !is_negligible(corrected_excess.dy) {
        // Neither axis fits at all (the viewport is larger than the
        // boundary in both directions): no translation.
        return matrix;
    }

    let unidirectional_tx = if is_negligible(corrected_excess.dx) {
        corrected_tx
    } else {
        0.0
    };
    let unidirectional_ty = if is_negligible(corrected_excess.dy) {
        corrected_ty
    } else {
        0.0
    };
    let mut result = matrix;
    result.set_translation(unidirectional_tx, unidirectional_ty, next_tz);
    result
}

/// Applies `scale` (a multiplicative change) to `matrix`, clamped so the
/// resulting overall scale stays within `[min_scale, max_scale]` and never
/// shrinks the child so much it can't cover `boundary` from `viewport`.
///
/// Composed via
/// `matrix * Matrix4::scaling(..)` — see [`clamp_translation`]'s doc for why
/// the mutating `Matrix4::scale` method is the wrong tool here.
fn clamp_scale(
    matrix: Matrix4,
    scale: f64,
    min_scale: f64,
    max_scale: f64,
    viewport: Rect<f64>,
    boundary: Rect<f64>,
) -> Matrix4 {
    if scale == 1.0 {
        return matrix;
    }
    // Debug-only: this does not replace `clamp_double`'s non-panicking
    // behavior below; it only surfaces the misconfiguration in debug builds.
    debug_assert!(
        max_scale >= min_scale,
        "InteractiveViewer: max_scale ({max_scale}) must be >= min_scale ({min_scale})"
    );
    let current_scale = uniform_scale(&matrix);
    // Finite / infinite (unbounded boundary) is naturally 0.0 here — no
    // separate infinite-boundary branch needed.
    let boundary_floor =
        (viewport.width() / boundary.width()).max(viewport.height() / boundary.height());
    let total_scale = (current_scale * scale).max(boundary_floor);
    let clamped_total = clamp_double(total_scale, min_scale, max_scale);
    let applied = clamped_total / current_scale;
    // The plane alone: depth keeps its unit scale, so the transform stays the
    // two-dimensional affine one the compositor draws (ADR-0102).
    matrix * Matrix4::scaling(applied, applied, 1.0)
}

/// Unlike `f64::clamp`
/// (which panics — in every build profile, not just debug — whenever `min >
/// max`), this never panics: a misconfigured `min_scale > max_scale` falls
/// through (the assertion above is debug-only) instead of crashing a release build over a caller error that
/// should have been caught in testing.
fn clamp_double(x: f64, min: f64, max: f64) -> f64 {
    if x < min {
        min
    } else if x > max {
        max
    } else {
        x
    }
}
