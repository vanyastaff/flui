//! A debug overlay that draws the gesture arena's contests over the app.
//!
//! [`GestureInspectorOverlay`] wraps the application's root. While its
//! [`GestureOverlayController`] shows it, each contest of the window's arena is
//! drawn beside the point its contact went down: the contest's sequence and
//! outcome, then one line per member with its composition path, claim,
//! verdict and rejection reason, and the deadlines that elapsed for it. A
//! contest stays until it settles, then fades out over [`FADE`].
//!
//! Toggling the controller rebuilds only the overlay's own layer, never the
//! application below it. While the overlay is hidden, or not mounted, no
//! observer is registered on the arena, so it costs the input path nothing.
//!
//! ```no_run
//! use flui_devtools::gesture_overlay::{GestureInspectorOverlay, GestureOverlayController};
//! use flui_sdk::widgets::Text;
//!
//! let controller = GestureOverlayController::new(true);
//! let root = GestureInspectorOverlay::new(controller.clone(), Text::new("app"));
//! // ...attach `root`; later, from a key binding or a debug menu:
//! controller.toggle();
//! ```

use std::cell::{Cell, RefCell};
use std::collections::{HashSet, VecDeque};
use std::fmt;
use std::rc::{Rc, Weak};
use std::time::Duration;

use flui_sdk::foundation::geometry::Offset;
use flui_sdk::foundation::{ChangeNotifier, Listenable, ListenerId, RebuildReason};
use flui_sdk::interaction::{EventTime, GestureArena, PointerEventExt, PointerId};
use flui_sdk::painting::{Color, TextStyle};
use flui_sdk::rendering::HitTestBehavior;
use flui_sdk::view::prelude::*;
use flui_sdk::view::{PostFrameHandle, RebuildHandle};
use flui_sdk::widgets::{
    ColoredBox, Column, CrossAxisAlignment, GestureArenaScope, IgnorePointer, Listener,
    MainAxisSize, Padding, Positioned, SizedBox, Stack, StackFit, Text,
};

use crate::gesture_inspector::{
    ContestReport, DEFAULT_CONTEST_CAPACITY, GestureInspector, Verdict,
};

/// How long a settled contest stays on screen, fading out.
pub const FADE: Duration = Duration::from_millis(1500);

/// Where a contest is drawn relative to its contact's Down position.
const LABEL_OFFSET: f64 = 12.0;

/// Shows or hides every [`GestureInspectorOverlay`] built with it.
///
/// Clones share one switch. Changing it rebuilds only the overlays' layers.
#[derive(Clone)]
pub struct GestureOverlayController {
    inner: Rc<ControllerInner>,
}

struct ControllerInner {
    shown: Cell<bool>,
    observing: Cell<usize>,
    changed: ChangeNotifier,
}

impl GestureOverlayController {
    /// A controller that starts shown or hidden.
    #[must_use]
    pub fn new(shown: bool) -> Self {
        Self {
            inner: Rc::new(ControllerInner {
                shown: Cell::new(shown),
                observing: Cell::new(0),
                changed: ChangeNotifier::new(),
            }),
        }
    }

    /// Whether the overlays draw.
    #[must_use]
    pub fn is_shown(&self) -> bool {
        self.inner.shown.get()
    }

    /// Show or hide the overlays.
    pub fn set_shown(&self, shown: bool) {
        if self.inner.shown.replace(shown) != shown {
            self.inner.changed.notify_listeners();
        }
    }

    /// Flip between shown and hidden.
    pub fn toggle(&self) {
        self.set_shown(!self.is_shown());
    }

    /// Whether a mounted overlay currently observes its arena. False while
    /// hidden and after every overlay unmounted: then no observer is
    /// registered.
    #[must_use]
    pub fn is_observing(&self) -> bool {
        self.inner.observing.get() > 0
    }

    fn same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Default for GestureOverlayController {
    /// A hidden controller.
    fn default() -> Self {
        Self::new(false)
    }
}

impl fmt::Debug for GestureOverlayController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GestureOverlayController")
            .field("shown", &self.is_shown())
            .field("observing", &self.is_observing())
            .finish()
    }
}

/// Where each contact went down, in the overlay's coordinates, oldest first
/// like the inspector's contests.
type Positions = Rc<RefCell<VecDeque<(PointerId, Offset<f64>)>>>;

/// Draws the window's gesture contests over `child` while its controller
/// shows it. Mount it at the application's root, below the window's
/// `GestureArenaScope`.
#[derive(Clone, StatefulView)]
pub struct GestureInspectorOverlay {
    controller: GestureOverlayController,
    child: BoxedView,
}

impl GestureInspectorOverlay {
    /// `child`, with the contests drawn over it while `controller` shows them.
    #[must_use]
    pub fn new(controller: GestureOverlayController, child: impl IntoView) -> Self {
        Self {
            controller,
            child: child.into_view().boxed(),
        }
    }
}

impl fmt::Debug for GestureInspectorOverlay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GestureInspectorOverlay")
            .field("controller", &self.controller)
            .finish_non_exhaustive()
    }
}

/// [`GestureInspectorOverlay`]'s state: the contacts' Down positions, which
/// the layer reads.
pub struct GestureInspectorOverlayState {
    positions: Positions,
}

impl fmt::Debug for GestureInspectorOverlayState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GestureInspectorOverlayState")
            .finish_non_exhaustive()
    }
}

impl StatefulView for GestureInspectorOverlay {
    type State = GestureInspectorOverlayState;

    fn create_state(&self) -> Self::State {
        GestureInspectorOverlayState {
            positions: Rc::default(),
        }
    }
}

impl ViewState<GestureInspectorOverlay> for GestureInspectorOverlayState {
    fn build(&self, view: &GestureInspectorOverlay, _ctx: &dyn BuildContext) -> impl IntoView {
        let positions = Rc::clone(&self.positions);
        let controller = view.controller.clone();
        let app = Listener::new()
            .behavior(HitTestBehavior::Translucent)
            .on_pointer_down(move |_, dispatch| {
                // Nothing reads a position while the overlay is hidden, and
                // the layer prunes only while it builds shown.
                if !controller.is_shown() {
                    return;
                }
                if let (Some(pointer), Some(at)) =
                    (dispatch.local.pointer_id(), dispatch.local.position())
                {
                    let mut positions = positions.borrow_mut();
                    // Ordered like the inspector's contests, which keeps the
                    // newest `DEFAULT_CONTEST_CAPACITY`: the position dropped
                    // is the one whose contest the inspector dropped, and a
                    // burst of downs between builds cannot outgrow it.
                    positions.retain(|(known, _)| *known != pointer);
                    positions.push_back((pointer, at));
                    if positions.len() > DEFAULT_CONTEST_CAPACITY {
                        positions.pop_front();
                    }
                }
            })
            .child(view.child.clone());
        let layer = OverlayLayer {
            controller: view.controller.clone(),
            positions: Rc::clone(&self.positions),
        };
        // `Expand` hands the app the constraints the overlay received, so
        // wrapping the root never changes the app's layout.
        Stack::new(vec![app.boxed(), Positioned::fill(layer).boxed()]).fit(StackFit::Expand)
    }
}

/// The part of the overlay that rebuilds when the controller or the arena
/// changes, apart from the application.
#[derive(Clone, StatefulView)]
struct OverlayLayer {
    controller: GestureOverlayController,
    positions: Positions,
}

struct OverlayLayerState {
    /// The controller the layer was created with, until `init_state` moves
    /// it into `live`.
    controller: GestureOverlayController,
    live: Option<Rc<Live>>,
    listener: Option<ListenerId>,
}

/// What the observer, the controller's listener and the fade loop share.
struct Live {
    arena: GestureArena,
    controller: RefCell<GestureOverlayController>,
    rebuild: RebuildHandle,
    post_frame: Option<PostFrameHandle>,
    inspector: RefCell<Option<GestureInspector>>,
    frame_pending: Cell<bool>,
}

impl Live {
    /// Register or drop the observer to match `shown`. The outgoing
    /// inspector is dropped with no borrow held.
    fn sync(self: &Rc<Self>, shown: bool) {
        let attached = self.inspector.borrow().is_some();
        if shown && !attached {
            let weak = Rc::downgrade(self);
            let changed = Box::new(move || {
                if let Some(live) = weak.upgrade() {
                    live.rebuild.schedule(RebuildReason::StateChange);
                    live.keep_fading();
                }
            });
            match GestureInspector::observing(&self.arena, DEFAULT_CONTEST_CAPACITY, Some(changed))
            {
                Ok(inspector) => {
                    *self.inspector.borrow_mut() = Some(inspector);
                    let observing = &self.controller.borrow().inner.observing;
                    observing.set(observing.get() + 1);
                }
                Err(error) => {
                    tracing::warn!(%error, "the gesture overlay cannot observe its arena");
                }
            }
        } else if !shown && attached {
            let outgoing = self.inspector.borrow_mut().take();
            // Dropping the inspector detaches from the arena and may run
            // observer code, so no borrow of the controller is held across it.
            drop(outgoing);
            let controller = self.controller.borrow();
            let observing = &controller.inner.observing;
            observing.set(observing.get().saturating_sub(1));
        }
        self.rebuild.schedule(RebuildReason::StateChange);
    }

    /// The contests to draw now, with their opacity.
    fn visible(&self) -> Vec<(ContestReport, f64)> {
        let now = self.arena.now();
        let mut visible = Vec::new();
        if let Some(inspector) = self.inspector.borrow().as_ref() {
            inspector.visit_contests(|contest| {
                if let Some(opacity) = opacity(contest, now) {
                    visible.push((contest.clone(), opacity));
                }
            });
        }
        visible
    }

    /// The pointers of every contest the inspector still retains.
    fn retained_pointers(&self) -> HashSet<PointerId> {
        let mut pointers = HashSet::new();
        if let Some(inspector) = self.inspector.borrow().as_ref() {
            inspector.visit_contests(|contest| {
                pointers.insert(contest.pointer);
            });
        }
        pointers
    }

    /// While a settled contest is still fading, rebuild after every frame.
    fn keep_fading(self: &Rc<Self>) {
        let now = self.arena.now();
        let mut fading = false;
        if let Some(inspector) = self.inspector.borrow().as_ref() {
            inspector.visit_contests(|contest| {
                fading |= settled_at(contest).is_some() && opacity(contest, now).is_some();
            });
        }
        let Some(post_frame) = &self.post_frame else {
            return;
        };
        if !fading || self.frame_pending.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        let scheduled = post_frame.schedule(move |_| {
            if let Some(live) = weak.upgrade() {
                live.frame_pending.set(false);
                live.rebuild.schedule(RebuildReason::StateChange);
                live.keep_fading();
            }
        });
        if scheduled.is_err() {
            self.frame_pending.set(false);
        }
    }
}

/// When the contest settled: its last verdict once it has a winner or was
/// cancelled.
fn settled_at(contest: &ContestReport) -> Option<EventTime> {
    if contest.winner.is_none() && contest.cancelled.is_none() {
        return None;
    }
    contest
        .members
        .iter()
        .filter_map(|member| match member.verdict? {
            Verdict::Won { at } | Verdict::Rejected { at, .. } => Some(at),
        })
        .max()
}

/// The contest's opacity at `now`: whole while it is undecided, fading over
/// [`FADE`] once it settled, `None` once faded out.
fn opacity(contest: &ContestReport, now: EventTime) -> Option<f64> {
    if contest.winner.is_none() && contest.cancelled.is_none() {
        return Some(1.0);
    }
    let elapsed = now.saturating_duration_since(settled_at(contest)?);
    (elapsed < FADE).then(|| 1.0 - elapsed.as_secs_f64() / FADE.as_secs_f64())
}

impl fmt::Debug for OverlayLayerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OverlayLayerState")
            .field("live", &self.live.is_some())
            .finish_non_exhaustive()
    }
}

impl StatefulView for OverlayLayer {
    type State = OverlayLayerState;

    fn create_state(&self) -> Self::State {
        OverlayLayerState {
            controller: self.controller.clone(),
            live: None,
            listener: None,
        }
    }
}

impl OverlayLayerState {
    fn listen(&mut self, controller: &GestureOverlayController) {
        let Some(live) = &self.live else {
            return;
        };
        let weak: Weak<Live> = Rc::downgrade(live);
        let id = controller.inner.changed.add_listener(Rc::new(move || {
            if let Some(live) = weak.upgrade() {
                let shown = live.controller.borrow().is_shown();
                live.sync(shown);
            }
        }));
        self.listener = Some(id);
    }

    fn unlisten(&mut self) {
        if let (Some(id), Some(live)) = (self.listener.take(), &self.live) {
            live.controller.borrow().inner.changed.remove_listener(id);
        }
    }
}

impl ViewState<OverlayLayer> for OverlayLayerState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let controller = self.controller.clone();
        let live = Rc::new(Live {
            arena: GestureArenaScope::of(ctx),
            controller: RefCell::new(controller.clone()),
            rebuild: ctx.rebuild_handle(),
            post_frame: ctx.post_frame_handle(),
            inspector: RefCell::new(None),
            frame_pending: Cell::new(false),
        });
        self.live = Some(Rc::clone(&live));
        self.listen(&controller);
        live.sync(controller.is_shown());
    }

    fn did_update_view(&mut self, old_view: &OverlayLayer, new_view: &OverlayLayer) {
        if old_view.controller.same(&new_view.controller) {
            return;
        }
        let Some(live) = self.live.clone() else {
            return;
        };
        self.unlisten();
        live.sync(false);
        *live.controller.borrow_mut() = new_view.controller.clone();
        self.controller = new_view.controller.clone();
        self.listen(&new_view.controller);
        live.sync(new_view.controller.is_shown());
    }

    fn dispose(&mut self) {
        self.unlisten();
        if let Some(live) = self.live.take() {
            live.sync(false);
        }
    }

    fn build(&self, view: &OverlayLayer, _ctx: &dyn BuildContext) -> impl IntoView {
        let Some(live) = self.live.as_ref().filter(|_| view.controller.is_shown()) else {
            return SizedBox::shrink().boxed();
        };
        // A position outlives its contest only until the next build: the
        // inspector keeps a bounded number of contests, so the map is bounded
        // by it too.
        let retained = live.retained_pointers();
        let mut positions = view.positions.borrow_mut();
        positions.retain(|(pointer, _)| retained.contains(pointer));
        let labels: Vec<BoxedView> = live
            .visible()
            .into_iter()
            .enumerate()
            .map(|(index, (contest, opacity))| {
                let at = positions
                    .iter()
                    .find(|(pointer, _)| *pointer == contest.pointer)
                    .map_or_else(
                        || {
                            #[expect(
                                clippy::cast_precision_loss,
                                reason = "a stacking index for contests with no known position"
                            )]
                            let row = index as f64;
                            Offset::new(0.0, row * 64.0)
                        },
                        |(_, at)| *at,
                    );
                Positioned::new(label(&contest, opacity))
                    .left(at.dx + LABEL_OFFSET)
                    .top(at.dy + LABEL_OFFSET)
                    .boxed()
            })
            .collect();
        IgnorePointer::new().child(Stack::new(labels)).boxed()
    }
}

/// One contest as text lines on a dark panel, faded to `opacity`.
fn label(contest: &ContestReport, opacity: f64) -> impl IntoView {
    let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=255 above"
    )]
    let alpha = alpha as u8;
    let text = contest.to_string();
    let lines: Vec<BoxedView> = text
        .lines()
        .map(|line| {
            Text::new(line.trim_start())
                .style(TextStyle::default().with_color(Color::rgba(255, 255, 255, alpha)))
                .boxed()
        })
        .collect();
    ColoredBox::new(Color::rgba(0, 0, 0, alpha / 4 * 3)).child(
        Padding::all(4.0).child(
            Column::new(lines)
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .main_axis_size(MainAxisSize::Min),
        ),
    )
}
