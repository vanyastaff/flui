//! [`RefreshIndicator`] — pull to refresh, a participant of the nested scroll
//! channel (ADR-0209).
//!
//! The indicator is not a second scroll path. It joins the chain of the
//! scrollables beneath it as a [`NestedScrollConnection`] and takes what a
//! scroll leaf leaves at its leading edge — the top of a vertical list, the
//! reading start of a horizontal one (its left, or its right under
//! right-to-left text) — while a contact drags it:
//!
//! - **Pull.** `post_scroll` takes the leaf's remainder toward its start
//!   when the step is [`ScrollCause::UserInput`]: a drag. A fling
//!   ([`ScrollCause::SideEffect`]) and platform momentum never pull, and a
//!   fling residual is declined. A wheel, a trackpad pan, keys and
//!   assistive steps are user input too, and a list at its top passes a
//!   wheel or trackpad packet up its chain, but none of them is a contact
//!   ([`ScrollStep::contact`]): only a dragging hand reveals the indicator. Taking the first
//!   delta latches the contact here (ADR-0209 §5): `pre_scroll` gives a
//!   reversing finger its pull back first, and the list moves again on the
//!   next contact. A leaf with bouncing physics keeps its own overscroll and
//!   leaves nothing to pull with.
//! - **Release.** `released` decides: a pull of at least
//!   [`threshold_px`](RefreshIndicator::threshold_px) performs the refresh
//!   command after the frame; a shorter one settles back. A cancelled
//!   contact, a contact the arena gave away and a preempted one settle back
//!   without refreshing, whatever their pull.
//! - **Refresh.** The command performs [`RefreshIntent`], which the
//!   indicator binds: it calls [`on_refresh`](RefreshIndicator::on_refresh)
//!   and runs the future it returns on the owner's async driver. Its
//!   completion is delivered when the driver polls it, at the start of a
//!   frame; the frame path itself never waits. While a refresh runs, a pull
//!   reveals nothing and every other request to refresh is refused.
//!
//! # Alternatives
//!
//! The pull is a path gesture, so the indicator takes its
//! [`GestureAlternatives`] in its constructor (ADR-0216): the first command
//! performs [`RefreshIntent`] — [`RefreshIntent::command`] builds it with
//! the platform's refresh keys — and assistive technology's custom action,
//! the keys and the visible control or menu perform it as the pull does,
//! through the same action.
//!
//! # Indicator
//!
//! A band at the leading edge slides in as the pull nears the threshold and
//! rests there while refreshing. What it shows comes from the nearest
//! [`RefreshIndicatorStyle`]: a builder from the [`RefreshIndicatorPhase`] (how
//! far the pull has gone, or refreshing) to a view. A design system installs one
//! for its apps (`flui-material`'s `MaterialApp` installs the Material circular
//! indicator); without one the band holds an [`ActivityIndicator`] (ADR-0235).

use std::cell::{Cell, RefCell};
use std::fmt;
use std::future::Future;
use std::rc::{Rc, Weak};
use std::time::Duration;

use flui_animation::{Animation, AnimationController, DrivenController, Vsync};
use flui_foundation::geometry::{Axis, Vec2};
use flui_foundation::panic::PanicRecovery;
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use flui_interaction::InputSequenceId;
use flui_painting::typography::TextDirection;
use flui_platform_api::TargetPlatform;
use flui_platform_api::keyboard::NamedKey;
use flui_view::prelude::StatefulView;
use flui_view::{
    AsyncDriver, BoxedTask, BoxedView, BuildContext, BuildContextExt, EventCx, InheritedView,
    IntoView, LifecycleContext, PostFrameHandle, RebuildHandle, RebuildReason, TaskToken, View,
    ViewExt, ViewState, WriterSource, impl_inherited_view,
};

use super::nested::{
    FlingStep, NestedScroll, NestedScrollConnection, ScrollCause, ScrollEndReason, ScrollStep,
};
use crate::animated::VsyncScope;
use crate::{
    Action, ActionOutcome, Actions, ActivityIndicator, Center, CommandHandle, Directionality,
    GestureAlternatives, GestureCommand, Intent, Positioned, SingleActivator, SizedBox, Stack,
};

/// Default pull, in logical pixels past the leading edge, that refreshes.
const DEFAULT_THRESHOLD_PX: f64 = 80.0;

/// The indicator's band across the content (logical pixels): 56 dp.
const INDICATOR_EXTENT_PX: f64 = 56.0;

/// How long a released or abandoned pull takes to settle back.
const SETTLE: Duration = Duration::from_millis(200);

// ---------------------------------------------------------------------------
// RefreshIntent
// ---------------------------------------------------------------------------

/// Refresh the content of the enclosing [`RefreshIndicator`].
///
/// The indicator binds it: its pull, and every alternative path of its
/// [`GestureAlternatives`] whose command carries this intent, start one
/// refresh, and none while a refresh runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RefreshIntent;

impl Intent for RefreshIntent {}

impl RefreshIntent {
    /// The refresh command, labelled `label` ("Refresh inbox"), with the
    /// running platform's refresh keys: F5, and Cmd+R on macOS and iOS or
    /// Ctrl+R elsewhere.
    #[must_use]
    pub fn command(label: impl Into<String>) -> GestureCommand {
        refresh_keys(TargetPlatform::current())
            .into_iter()
            .fold(GestureCommand::new(label, Self), GestureCommand::key)
    }
}

/// The refresh keys on `platform`: F5, and Cmd+R on macOS and iOS or Ctrl+R
/// elsewhere. The collision table checks them on every platform.
pub(crate) fn refresh_keys(platform: TargetPlatform) -> [SingleActivator; 2] {
    let chord = SingleActivator::character("r");
    let reload = match platform {
        TargetPlatform::MacOS | TargetPlatform::iOS => chord.meta(),
        _ => chord.control(),
    };
    [SingleActivator::named(NamedKey::F5), reload]
}

// ---------------------------------------------------------------------------
// RefreshController — what the indicator shows, for the caller to observe
// ---------------------------------------------------------------------------

struct RefreshControllerInner {
    pull_distance_px: Cell<f64>,
    refreshing: Cell<bool>,
    notifier: ChangeNotifier,
}

impl fmt::Debug for RefreshControllerInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RefreshControllerInner")
            .field("is_refreshing", &self.refreshing.get())
            .field("pull_distance_px", &self.pull_distance_px.get())
            .finish_non_exhaustive()
    }
}

impl Listenable for RefreshControllerInner {
    fn add_observer(&self, observer: flui_foundation::ListenerObserver) -> ListenerId {
        self.notifier.add_observer(observer)
    }

    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.notifier.remove_all_listeners();
    }
}

/// The phase and the pull of a [`RefreshIndicator`], for its caller to
/// observe.
///
/// Pass one to [`RefreshIndicator::controller`]; every clone shares the same
/// state on the owning UI thread. The indicator writes it and notifies its
/// [`listenable`](Self::listenable) on every change.
#[derive(Clone, Debug)]
pub struct RefreshController {
    inner: Rc<RefreshControllerInner>,
}

impl Default for RefreshController {
    fn default() -> Self {
        Self {
            inner: Rc::new(RefreshControllerInner {
                pull_distance_px: Cell::new(0.0),
                refreshing: Cell::new(false),
                notifier: ChangeNotifier::new(),
            }),
        }
    }
}

impl RefreshController {
    /// A controller of an idle indicator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a refresh runs: from the moment it started until the future
    /// [`on_refresh`](RefreshIndicator::on_refresh) returned completes, or
    /// the indicator goes away.
    #[must_use]
    pub fn is_refreshing(&self) -> bool {
        self.inner.refreshing.get()
    }

    /// How far the indicator is pulled past the leading edge, in logical
    /// pixels: the contact's travel while it pulls, then the settling
    /// distance back to `0.0`.
    #[must_use]
    pub fn pull_distance_px(&self) -> f64 {
        self.inner.pull_distance_px.get()
    }

    /// An `Rc<dyn Listenable>` sharing this controller's notifications.
    #[must_use]
    pub fn listenable(&self) -> Rc<dyn Listenable> {
        Rc::clone(&self.inner) as Rc<dyn Listenable>
    }

    fn shares_state_with(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    fn set_pull(&self, distance_px: f64, recovery: &mut PanicRecovery) {
        if self.inner.pull_distance_px.replace(distance_px) != distance_px {
            recovery.run_with(|scope| self.inner.notifier.notify_listeners_with_recovery(scope));
        }
    }

    fn set_refreshing(&self, refreshing: bool, recovery: &mut PanicRecovery) {
        if self.inner.refreshing.replace(refreshing) != refreshing {
            recovery.run_with(|scope| self.inner.notifier.notify_listeners_with_recovery(scope));
        }
    }
}

// ---------------------------------------------------------------------------
// RefreshIndicatorStyle — the ambient indicator
// ---------------------------------------------------------------------------

/// Where a pull-to-refresh gesture stands, for a [`RefreshIndicatorStyle`] builder.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum RefreshIndicatorPhase {
    /// The user is pulling past the leading edge.
    Pulling {
        /// The pull as a share of the threshold that triggers a refresh, in `[0, 1]`.
        progress: f64,
    },
    /// The refresh is running, until its future completes.
    Refreshing,
}

/// The indicator every [`RefreshIndicator`] below shows, built from its phase.
///
/// The builder returns the view to show for a phase, or `None` for nothing. It runs when the
/// phase changes and, while pulling, as the pull grows; the view it returns is laid out in the
/// indicator's band at the leading edge of the content, and builds in the refresh indicator's
/// own context, so it reads the theme there. The nearest style wins. Without one, the band
/// holds an [`ActivityIndicator`] (ADR-0235).
///
/// # Example
///
/// ```rust
/// use flui_widgets::prelude::*;
/// use flui_widgets::{ActivityIndicator, RefreshIndicatorPhase, RefreshIndicatorStyle};
/// use flui_view::ViewExt;
///
/// let app = RefreshIndicatorStyle::new(
///     |phase| match phase {
///         RefreshIndicatorPhase::Refreshing => Some(ActivityIndicator::new().boxed()),
///         _ => None,
///     },
///     SizedBox::shrink(),
/// );
/// # let _ = app;
/// ```
#[derive(Clone)]
pub struct RefreshIndicatorStyle {
    builder: Rc<RefreshIndicatorBuilder>,
    child: BoxedView,
}

/// A [`RefreshIndicatorStyle`]'s builder.
pub(crate) type RefreshIndicatorBuilder = dyn Fn(RefreshIndicatorPhase) -> Option<BoxedView>;

impl RefreshIndicatorStyle {
    /// A style over `child` whose indicator `builder` makes.
    #[must_use]
    pub fn new(
        builder: impl Fn(RefreshIndicatorPhase) -> Option<BoxedView> + 'static,
        child: impl IntoView,
    ) -> Self {
        Self {
            builder: Rc::new(builder),
            child: child.into_view().boxed(),
        }
    }
}

impl fmt::Debug for RefreshIndicatorStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RefreshIndicatorStyle")
            .finish_non_exhaustive()
    }
}

impl InheritedView for RefreshIndicatorStyle {
    type Data = Rc<RefreshIndicatorBuilder>;

    fn data(&self) -> &Self::Data {
        &self.builder
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        !Rc::ptr_eq(&self.builder, &old.builder)
    }
}

impl_inherited_view!(RefreshIndicatorStyle);

// ---------------------------------------------------------------------------
// RefreshIndicator
// ---------------------------------------------------------------------------

/// Starts a refresh: the future that completes it.
type RefreshCallback = Rc<dyn Fn(&mut EventCx<'_>) -> BoxedTask>;

/// Pull to refresh over the scrollables in its child (see the
/// `refresh_indicator` module documentation).
///
/// ```rust
/// use flui_widgets::{
///     Alternative, ControlSpec, GestureAlternatives, RefreshIndicator, RefreshIntent, Scrollable,
///     SizedBox, Text,
/// };
///
/// let alternatives = GestureAlternatives::new(Alternative::VisibleControl(ControlSpec::new(
///     RefreshIntent::command("Refresh inbox"),
///     Text::new("Refresh"),
/// )))
/// .expect("one labelled command");
/// let inbox = RefreshIndicator::new(
///     Scrollable::new().child(SizedBox::new(300.0, 1400.0)),
///     alternatives,
/// )
/// .on_refresh(|_cx| async { /* fetch the messages */ });
/// ```
#[derive(Clone, StatefulView)]
pub struct RefreshIndicator {
    child: BoxedView,
    alternatives: GestureAlternatives,
    controller: Option<RefreshController>,
    on_refresh: RefreshCallback,
    threshold_px: f64,
    axis: Axis,
}

impl fmt::Debug for RefreshIndicator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RefreshIndicator")
            .field("threshold_px", &self.threshold_px)
            .field("axis", &self.axis)
            .field("alternatives", &self.alternatives)
            .finish_non_exhaustive()
    }
}

impl RefreshIndicator {
    /// Pull to refresh over the scrollables in `child`, reached without the
    /// pull through `alternatives`, whose first command performs
    /// [`RefreshIntent`].
    pub fn new(child: impl IntoView, alternatives: GestureAlternatives) -> Self {
        Self {
            child: child.into_view().boxed(),
            alternatives,
            controller: None,
            on_refresh: Rc::new(|_cx| Box::pin(std::future::ready(()))),
            threshold_px: DEFAULT_THRESHOLD_PX,
            axis: Axis::Vertical,
        }
    }

    /// Start the refresh: called with the event's context when one starts,
    /// on any path. The refresh runs until the returned future completes.
    #[must_use]
    pub fn on_refresh<F>(mut self, on_refresh: impl Fn(&mut EventCx<'_>) -> F + 'static) -> Self
    where
        F: Future<Output = ()> + 'static,
    {
        self.on_refresh = Rc::new(move |cx| Box::pin(on_refresh(cx)));
        self
    }

    /// Observe the phase and the pull through `controller`.
    #[must_use]
    pub fn controller(mut self, controller: RefreshController) -> Self {
        self.controller = Some(controller);
        self
    }

    /// The pull past the leading edge, in logical pixels, that refreshes
    /// (default `80.0`). A value that is not finite and positive keeps the
    /// default.
    #[must_use]
    pub fn threshold_px(mut self, threshold: f64) -> Self {
        if threshold.is_finite() && threshold > 0.0 {
            self.threshold_px = threshold;
        } else {
            tracing::warn!(
                threshold,
                "RefreshIndicator: a threshold must be finite and positive"
            );
        }
        self
    }

    /// The axis it pulls along (default vertical): down from the top of a
    /// vertical scrollable, toward the reading end from the reading start
    /// of a horizontal one — right from its left edge, or left from its
    /// right edge under the ambient right-to-left [`Directionality`].
    #[must_use]
    pub fn axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }
}

// ---------------------------------------------------------------------------
// Refresher — the participant and the refresh it starts
// ---------------------------------------------------------------------------

/// What the indicator keeps behind its connection: the pull of the contact
/// that pulls, the refresh in flight and the capabilities that start and
/// finish one.
struct Refresher {
    /// This refresher, for the deferred command and the refresh task.
    this: Weak<Refresher>,
    controller: RefCell<RefreshController>,
    axis: Cell<Axis>,
    /// The ambient reading direction, which places a horizontal axis's
    /// start.
    text_direction: Cell<TextDirection>,
    threshold_px: Cell<f64>,
    on_refresh: RefCell<RefreshCallback>,
    /// The contact sequence pulling now, and its travel past the edge.
    pulling: Cell<Option<InputSequenceId>>,
    travel: Cell<f64>,
    /// The gesture's command, resolved by the latest build below the
    /// indicator's action.
    command: RefCell<Option<CommandHandle>>,
    writer: RefCell<Option<WriterSource>>,
    post_frame: RefCell<Option<PostFrameHandle>>,
    driver: RefCell<Option<AsyncDriver>>,
    /// The refresh in flight; dropping it cancels the future.
    task: RefCell<Option<TaskToken>>,
    /// The settling run: its value is the fraction of `settle_from` left.
    settle: AnimationController,
    settle_from: Cell<f64>,
    settle_bound: Cell<bool>,
    mounted: Cell<bool>,
}

impl Refresher {
    fn new(view: &RefreshIndicator, settle: AnimationController) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            controller: RefCell::new(view.controller.clone().unwrap_or_default()),
            axis: Cell::new(view.axis),
            text_direction: Cell::new(TextDirection::Ltr),
            threshold_px: Cell::new(view.threshold_px),
            on_refresh: RefCell::new(Rc::clone(&view.on_refresh)),
            pulling: Cell::new(None),
            travel: Cell::new(0.0),
            command: RefCell::new(None),
            writer: RefCell::new(None),
            post_frame: RefCell::new(None),
            driver: RefCell::new(None),
            task: RefCell::new(None),
            settle,
            settle_from: Cell::new(0.0),
            settle_bound: Cell::new(false),
            mounted: Cell::new(false),
        })
    }

    fn controller(&self) -> RefreshController {
        self.controller.borrow().clone()
    }

    /// Whether the pull axis starts at its physical far end: a horizontal
    /// axis under right-to-left text starts on the right.
    fn starts_at_far_end(&self) -> bool {
        self.axis.get() == Axis::Horizontal && self.text_direction.get().is_rtl()
    }

    /// `delta`'s component along the pull axis, growing away from its
    /// reading start: negative toward the start.
    fn along(&self, delta: Vec2) -> f64 {
        let physical = match self.axis.get() {
            Axis::Vertical => delta.y,
            Axis::Horizontal => delta.x,
        };
        if self.starts_at_far_end() {
            -physical
        } else {
            physical
        }
    }

    /// The physical vector of `value` along the pull axis, as [`along`]
    /// measures it.
    ///
    /// [`along`]: Self::along
    fn on_axis(&self, value: f64) -> Vec2 {
        let physical = if self.starts_at_far_end() {
            -value
        } else {
            value
        };
        match self.axis.get() {
            Axis::Vertical => Vec2::new(0.0, physical),
            Axis::Horizontal => Vec2::new(physical, 0.0),
        }
    }

    fn show_pull(&self, distance_px: f64) {
        let mut recovery = PanicRecovery::new();
        self.controller().set_pull(distance_px, &mut recovery);
        recovery.finish();
    }

    /// Let the shown pull settle back to zero from where it is.
    fn settle_back(&self) {
        let from = self.controller().pull_distance_px();
        let _ = self.settle.stop();
        if from == 0.0 {
            return;
        }
        self.settle_from.set(from);
        if self.settle_bound.get() {
            self.settle.set_value(1.0);
            if self
                .settle
                .animate_to_curved(0.0, Some(SETTLE), flui_animation::curves::EASE_OUT)
                .is_ok()
            {
                return;
            }
        }
        self.show_pull(0.0);
    }

    /// The settling run moved.
    fn settled_to(&self, fraction: f64) {
        if self.pulling.get().is_none() {
            self.show_pull(self.settle_from.get() * fraction);
        }
    }

    /// The pull of `sequence` ends: settle back, and refresh when `release`
    /// and the pull reached the threshold.
    fn end_pull(&self, sequence: InputSequenceId, release: bool) {
        if self.pulling.get() != Some(sequence) {
            return;
        }
        self.pulling.set(None);
        let travel = self.travel.replace(0.0);
        self.settle_back();
        if release && travel >= self.threshold_px.get() && !self.controller().is_refreshing() {
            self.perform_after_frame();
        }
    }

    /// Perform the gesture's command once the frame is over: outside the
    /// scroll chain, so the refresh may scroll the content it refreshes.
    fn perform_after_frame(&self) {
        let Some(post_frame) = self.post_frame.borrow().clone() else {
            tracing::warn!("RefreshIndicator: no post-frame lane; the pull does not refresh");
            return;
        };
        let refresher = self.this.clone();
        if let Err(error) = post_frame.schedule(move |_| {
            if let Some(refresher) = refresher.upgrade() {
                refresher.perform_command();
            }
        }) {
            tracing::warn!(?error, "RefreshIndicator: the post-frame lane is closed");
        }
    }

    fn perform_command(&self) {
        if !self.mounted.get() {
            return;
        }
        let command = self.command.borrow().clone();
        let writer = self.writer.borrow().clone();
        if let (Some(command), Some(writer)) = (command, writer) {
            writer.write(|cx| command.perform(cx));
        }
    }

    /// Start one refresh, unless one runs.
    fn start(&self, cx: &mut EventCx<'_>) -> ActionOutcome {
        let controller = self.controller();
        if !self.mounted.get() || controller.is_refreshing() {
            return ActionOutcome::NotPerformed;
        }
        let mut recovery = PanicRecovery::new();
        controller.set_refreshing(true, &mut recovery);
        let on_refresh = Rc::clone(&*self.on_refresh.borrow());
        let mut future = None;
        recovery.run(|| future = Some(on_refresh(cx)));
        let driver = self.driver.borrow().clone();
        match (future, driver) {
            (Some(future), Some(driver)) if self.mounted.get() => {
                // The task is the refresh: one runs at a time, and dropping
                // its token (unmount) cancels it before it can complete.
                let refresher = self.this.clone();
                let token = driver.spawn_local(Box::pin(async move {
                    future.await;
                    if let Some(refresher) = refresher.upgrade() {
                        refresher.complete();
                    }
                }));
                let outgoing = self.task.replace(Some(token));
                recovery.retire(outgoing);
            }
            (future, driver) => {
                if driver.is_none() {
                    tracing::warn!("RefreshIndicator: no async driver; the refresh ends at once");
                }
                recovery.retire(future);
                self.complete();
            }
        }
        recovery.finish();
        ActionOutcome::Performed
    }

    /// The running refresh completed.
    fn complete(&self) {
        let mut recovery = PanicRecovery::new();
        self.controller().set_refreshing(false, &mut recovery);
        recovery.finish();
    }

    /// The indicator goes away: cancel its refresh and show it idle.
    fn retire(&self) {
        self.mounted.set(false);
        self.pulling.set(None);
        self.travel.set(0.0);
        let _ = self.settle.stop();
        let task = self.task.take();
        let command = self.command.take();
        let controller = self.controller();
        let mut recovery = PanicRecovery::new();
        recovery.retire(task);
        recovery.retire(command);
        controller.set_refreshing(false, &mut recovery);
        controller.set_pull(0.0, &mut recovery);
        recovery.finish();
    }
}

impl NestedScrollConnection for Refresher {
    fn pre_scroll(&self, step: &ScrollStep) -> Vec2 {
        // A finger reversing its pull takes the pull back before anything
        // scrolls.
        if self.pulling.get() != Some(step.sequence) || step.cause != ScrollCause::UserInput {
            return Vec2::ZERO;
        }
        let back = self.along(step.available);
        let travel = self.travel.get();
        if back <= 0.0 || travel <= 0.0 {
            return Vec2::ZERO;
        }
        let taken = back.min(travel);
        self.travel.set(travel - taken);
        self.show_pull(travel - taken);
        self.on_axis(taken)
    }

    fn post_scroll(&self, step: &ScrollStep, _consumed_by_child: Vec2) -> Vec2 {
        // Only a contact pulls: a wheel or trackpad packet at the top
        // reaches the chain too, and pulls nothing.
        if step.cause != ScrollCause::UserInput || !step.contact || !self.mounted.get() {
            return Vec2::ZERO;
        }
        if self.controller().is_refreshing() {
            return Vec2::ZERO;
        }
        if self
            .pulling
            .get()
            .is_some_and(|pulling| pulling != step.sequence)
        {
            return Vec2::ZERO;
        }
        let toward_start = self.along(step.available);
        if toward_start >= 0.0 {
            return Vec2::ZERO;
        }
        if self.pulling.get().is_none() {
            // Catch the indicator where it is settling.
            let _ = self.settle.stop();
            self.travel.set(self.controller().pull_distance_px());
            self.pulling.set(Some(step.sequence));
        }
        let travel = self.travel.get() - toward_start;
        self.travel.set(travel);
        self.show_pull(travel);
        self.on_axis(toward_start)
    }

    fn released(&self, step: &FlingStep) {
        self.end_pull(step.sequence, true);
    }

    fn sequence_ended(&self, sequence: InputSequenceId, _reason: ScrollEndReason) {
        // Cancelled, given to another gesture or preempted before it was
        // released: the pull settles back and refreshes nothing.
        self.end_pull(sequence, false);
    }
}

/// The indicator's binding of [`RefreshIntent`]: every path that performs
/// it starts the refresh through the same refresher.
struct RefreshAction {
    refresher: Weak<Refresher>,
}

impl Action<RefreshIntent> for RefreshAction {
    fn invoke(&self, cx: &mut EventCx<'_>, _intent: &RefreshIntent) -> ActionOutcome {
        self.refresher
            .upgrade()
            .map_or(ActionOutcome::NotPerformed, |refresher| refresher.start(cx))
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Persistent state for [`RefreshIndicator`]: its participant, kept for as
/// long as it is mounted so the scrollables below keep their chain, and the
/// registry seat of its settling run.
pub struct RefreshIndicatorState {
    refresher: Rc<Refresher>,
    connection: Rc<dyn NestedScrollConnection>,
    settle: DrivenController,
    settle_listener: Option<ListenerId>,
    vsync: Option<Vsync>,
}

impl fmt::Debug for RefreshIndicatorState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RefreshIndicatorState")
            .field("controller", &self.refresher.controller())
            .field("settle_bound", &self.settle.is_bound())
            .finish_non_exhaustive()
    }
}

impl StatefulView for RefreshIndicator {
    type State = RefreshIndicatorState;

    fn create_state(&self) -> Self::State {
        let settle = AnimationController::builder(SETTLE)
            .debug_label("RefreshIndicator.settle")
            .build_on(None);
        let refresher = Refresher::new(self, settle.controller().clone());
        let connection: Rc<dyn NestedScrollConnection> = refresher.clone();
        RefreshIndicatorState {
            refresher,
            connection,
            settle,
            settle_listener: None,
            vsync: None,
        }
    }
}

impl RefreshIndicatorState {
    fn bind_vsync(&mut self, ctx: &dyn LifecycleContext) {
        let incoming = VsyncScope::maybe_of(ctx);
        if VsyncScope::same_registry(self.vsync.as_ref(), incoming.as_ref()) {
            return;
        }
        self.vsync = incoming;
        // A run on the old clock ends where it is.
        let _ = self.settle.controller().stop();
        let rebound = self.settle.rebind(self.vsync.as_ref());
        self.refresher.settle_bound.set(self.settle.is_bound());
        if let Err(error) = rebound {
            tracing::error!(%error, "RefreshIndicator lost its frame registry");
        }
    }

    /// Point the indicator at `incoming`: it takes over the phase and the
    /// pull, and the outgoing controller is left idle.
    fn replace_controller(&self, incoming: RefreshController) {
        let outgoing = self.refresher.controller.replace(incoming.clone());
        let mut recovery = PanicRecovery::new();
        incoming.set_refreshing(outgoing.is_refreshing(), &mut recovery);
        incoming.set_pull(outgoing.pull_distance_px(), &mut recovery);
        outgoing.set_refreshing(false, &mut recovery);
        outgoing.set_pull(0.0, &mut recovery);
        recovery.finish();
    }
}

impl ViewState<RefreshIndicator> for RefreshIndicatorState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.refresher.writer.borrow_mut() = Some(ctx.writer_source());
        self.refresher.mounted.set(true);
        let settle = self.settle.controller().clone();
        let weak = Rc::downgrade(&self.refresher);
        self.settle_listener = Some(self.settle.controller().add_listener(Rc::new(move || {
            if let Some(refresher) = weak.upgrade() {
                refresher.settled_to(settle.value());
            }
        })));
        self.did_change_dependencies(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        *self.refresher.post_frame.borrow_mut() = ctx.post_frame_handle();
        *self.refresher.driver.borrow_mut() = ctx.async_driver();
        self.refresher
            .text_direction
            .set(Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr));
        self.bind_vsync(ctx);
    }

    fn build(&self, view: &RefreshIndicator, _ctx: &dyn BuildContext) -> impl IntoView {
        Actions::new(RefreshBody {
            refresher: Rc::clone(&self.refresher),
            alternatives: view.alternatives.clone(),
            content: NestedScroll::new(Rc::clone(&self.connection), view.child.clone()).boxed(),
        })
        .action(RefreshAction {
            refresher: Rc::downgrade(&self.refresher),
        })
    }

    fn did_update_view(&mut self, _old_view: &RefreshIndicator, new_view: &RefreshIndicator) {
        let refresher = &self.refresher;
        refresher.axis.set(new_view.axis);
        refresher.threshold_px.set(new_view.threshold_px);
        let outgoing = refresher
            .on_refresh
            .replace(Rc::clone(&new_view.on_refresh));
        drop(outgoing);
        // Without a controller of its own, a rebuild keeps the one the
        // indicator has.
        if let Some(incoming) = &new_view.controller
            && !incoming.shares_state_with(&refresher.controller())
        {
            self.replace_controller(incoming.clone());
        }
    }

    fn dispose(&mut self) {
        self.refresher.retire();
        if let Some(id) = self.settle_listener.take() {
            self.settle.controller().remove_listener(id);
        }
        self.settle.dispose();
    }
}

// ---------------------------------------------------------------------------
// Body and overlay
// ---------------------------------------------------------------------------

/// The content inside the alternatives, below the indicator's action, where
/// the gesture resolves its command as the alternatives do.
#[derive(Clone, StatefulView)]
struct RefreshBody {
    refresher: Rc<Refresher>,
    alternatives: GestureAlternatives,
    content: BoxedView,
}

impl fmt::Debug for RefreshBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RefreshBody").finish_non_exhaustive()
    }
}

/// `RefreshBody` keeps nothing between builds; its state exists to hold the
/// view-to-slot write out of `build`.
struct RefreshBodyState;

impl StatefulView for RefreshBody {
    type State = RefreshBodyState;

    fn create_state(&self) -> Self::State {
        RefreshBodyState
    }
}

impl ViewState<RefreshBody> for RefreshBodyState {
    fn apply_view(&mut self, view: &RefreshBody, ctx: &dyn LifecycleContext) {
        if view
            .alternatives
            .commands()
            .next()
            .is_some_and(|command| !command.performs::<RefreshIntent>())
        {
            tracing::warn!(
                target: "flui::a11y",
                "RefreshIndicator: the first command of its alternatives does not perform \
                 RefreshIntent, so the pull performs it without refreshing; build it with \
                 RefreshIntent::command"
            );
        }
        let outgoing = view
            .refresher
            .command
            .replace(Some(view.alternatives.handle(ctx)));
        drop(outgoing);
    }

    fn build(&self, view: &RefreshBody, _ctx: &dyn BuildContext) -> impl IntoView {
        let overlay = RefreshOverlay {
            controller: view.refresher.controller(),
            axis: view.refresher.axis.get(),
            from_far_end: view.refresher.starts_at_far_end(),
            threshold_px: view.refresher.threshold_px.get(),
        };
        view.alternatives
            .clone()
            .child(Stack::new(vec![view.content.clone(), overlay.boxed()]))
    }
}

/// Shows the pull and the refresh; the only part of the indicator that
/// rebuilds while a contact pulls.
#[derive(Clone, StatefulView)]
struct RefreshOverlay {
    controller: RefreshController,
    axis: Axis,
    /// The pull axis starts at its physical far end (the right, under
    /// right-to-left text), where the band slides in from.
    from_far_end: bool,
    threshold_px: f64,
}

impl fmt::Debug for RefreshOverlay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RefreshOverlay")
            .field("controller", &self.controller)
            .finish_non_exhaustive()
    }
}

struct RefreshOverlayState {
    rebuild: Option<RebuildHandle>,
    subscription: Option<(RefreshController, ListenerId)>,
    initial: Option<RefreshController>,
}

impl StatefulView for RefreshOverlay {
    type State = RefreshOverlayState;

    fn create_state(&self) -> Self::State {
        RefreshOverlayState {
            rebuild: None,
            subscription: None,
            initial: Some(self.controller.clone()),
        }
    }
}

impl RefreshOverlayState {
    fn subscribe(&mut self, controller: &RefreshController) {
        self.unsubscribe();
        let Some(rebuild) = self.rebuild.clone() else {
            return;
        };
        let watched = controller.clone();
        let shown = Cell::new((watched.is_refreshing(), watched.pull_distance_px()));
        let id = controller.inner.add_listener(Rc::new(move || {
            let current = (watched.is_refreshing(), watched.pull_distance_px());
            if shown.replace(current) != current {
                rebuild.schedule(RebuildReason::StateChange);
            }
        }));
        self.subscription = Some((controller.clone(), id));
    }

    fn unsubscribe(&mut self) {
        if let Some((controller, id)) = self.subscription.take() {
            controller.inner.remove_listener(id);
        }
    }
}

impl ViewState<RefreshOverlay> for RefreshOverlayState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.rebuild = Some(ctx.rebuild_handle());
        if let Some(controller) = self.initial.take() {
            self.subscribe(&controller);
        }
    }

    fn build(&self, view: &RefreshOverlay, ctx: &dyn BuildContext) -> impl IntoView {
        let style = ctx.depend_on::<RefreshIndicatorStyle, _>(|style| Rc::clone(&style.builder));
        let refreshing = view.controller.is_refreshing();
        let pull = view.controller.pull_distance_px();
        if !refreshing && (pull.is_nan() || pull <= 0.0) {
            return SizedBox::shrink().boxed();
        }
        // The band slides in from beyond the leading edge as the pull nears
        // the threshold, and rests fully in while refreshing. A share that is
        // not in [0, 1] (a zero threshold) is complete past it.
        let progress = if refreshing {
            1.0
        } else {
            let share = pull / view.threshold_px;
            if share >= 0.0 { share.min(1.0) } else { 0.0 }
        };
        let phase = if refreshing {
            RefreshIndicatorPhase::Refreshing
        } else {
            RefreshIndicatorPhase::Pulling { progress }
        };
        let indicator = match style {
            Some(builder) => match builder(phase) {
                Some(indicator) => indicator,
                None => return SizedBox::shrink().boxed(),
            },
            None => ActivityIndicator::new().boxed(),
        };
        let lead = INDICATOR_EXTENT_PX * (progress - 1.0);
        let spinner = Center::new().child(indicator);
        match view.axis {
            Axis::Vertical => Positioned::new(spinner)
                .top(lead)
                .left(0.0)
                .right(0.0)
                .height(INDICATOR_EXTENT_PX)
                .boxed(),
            Axis::Horizontal if view.from_far_end => Positioned::new(spinner)
                .right(lead)
                .top(0.0)
                .bottom(0.0)
                .width(INDICATOR_EXTENT_PX)
                .boxed(),
            Axis::Horizontal => Positioned::new(spinner)
                .left(lead)
                .top(0.0)
                .bottom(0.0)
                .width(INDICATOR_EXTENT_PX)
                .boxed(),
        }
    }

    fn did_update_view(&mut self, _old_view: &RefreshOverlay, new_view: &RefreshOverlay) {
        if !self
            .subscription
            .as_ref()
            .is_some_and(|(controller, _)| controller.shares_state_with(&new_view.controller))
        {
            self.subscribe(&new_view.controller);
        }
    }

    fn dispose(&mut self) {
        self.unsubscribe();
    }
}
