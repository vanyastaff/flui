//! [`CollapsingHeader`] — a header above scrolling content that the
//! content's own scroll collapses and expands, as a participant of the
//! nested scroll channel (ADR-0209).
//!
//! The header sits above its body, never over it: the body is laid out in
//! what the header leaves, so the header cannot obscure a row of the body,
//! and keyboard or assistive focus on a row is revealed by the body's own
//! scrollable. What the header can hide is its own content, clipped as it
//! collapses; focus moving onto that content expands it.
//!
//! # Consumption
//!
//! The header is a [`NestedScrollConnection`] above every scrollable in its
//! body. A scroll toward the content's end collapses it and one toward the
//! start expands it, each either before the content moves
//! ([`HeaderOrder::BeforeContent`], in `pre_scroll`) or with what the content
//! leaves at its edge ([`HeaderOrder::AfterContent`], in `post_scroll`, under
//! the channel's latch). It takes user input, platform momentum and fling
//! steps, never a programmatic one: a reveal or a controller jump moves the
//! content alone. A fling residual the content leaves at its edge carries on
//! into the header where the header takes that direction after the content.
//! With a [`SnapPolicy`](super::SnapPolicy), the header settles once every sequence that moved it
//! has ended.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{Animation, AnimationController, DrivenController};
use flui_foundation::geometry::{Rect, Vec2};
use flui_foundation::{ChangeNotifier, Listenable, ListenerId};
use flui_interaction::InputSequenceId;
use flui_interaction::focus::FocusNode;
use flui_painting::Alignment;
use flui_rendering::pipeline::WeakPipelineCell;
use flui_view::prelude::StatefulView;
use flui_view::{BoxedView, BuildContext, IntoView, LifecycleContext, ViewExt, ViewState};

use super::nested::{
    FlingHandoff, FlingStep, NestedScroll, NestedScrollConnection, ScrollCause, ScrollEndReason,
    ScrollStep,
};
use super::scroll_owner::{SharedSnapPolicy, settle_spring, snap_target};
use super::nested::presentation_device_pixel_ratio;
use super::scroll_physics::rest_slop;
use super::{ClampingScrollPhysics, ScrollMetrics, ScrollPhysics};
use crate::interaction::focus_reveal::{FocusRevealScope, RevealTarget, SharedRevealTarget};
use crate::{
    AnimatedBuilder, ClipRect, Column, Expanded, Focus, OverflowBox, Semantics, SemanticsRole,
    SizedBox, VsyncScope,
};

/// Why [`HeaderExtents::new`] refused a pair of extents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum HeaderExtentsError {
    /// An extent is NaN or infinite.
    #[error("header extents must be finite")]
    NonFinite,
    /// An extent is below zero.
    #[error("header extents must not be negative")]
    Negative,
    /// An extent exceeds [`HeaderExtents::MAX`].
    #[error("header extents must not exceed {max} logical pixels", max = HeaderExtents::MAX)]
    TooLarge,
    /// The collapsed extent exceeds the expanded one.
    #[error("the collapsed extent exceeds the expanded one")]
    Inverted,
}

/// How tall a [`CollapsingHeader`] is when fully collapsed and when fully
/// expanded, in logical pixels.
///
/// A collapsed extent above zero pins that much of the header on screen; at
/// zero the header leaves entirely. Equal extents make a header that never
/// moves and takes no scroll.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeaderExtents {
    collapsed: f64,
    expanded: f64,
}

impl HeaderExtents {
    /// The largest extent admitted: far past any display, and small enough
    /// that every span and velocity derived from it stays finite.
    pub const MAX: f64 = 1.0e6;

    /// A header `collapsed` tall at its smallest and `expanded` at its
    /// largest.
    ///
    /// # Errors
    ///
    /// [`HeaderExtentsError`] for a NaN, infinite, negative or too large
    /// extent, or a collapsed extent above the expanded one.
    pub fn new(collapsed: f64, expanded: f64) -> Result<Self, HeaderExtentsError> {
        if !collapsed.is_finite() || !expanded.is_finite() {
            return Err(HeaderExtentsError::NonFinite);
        }
        if collapsed < 0.0 || expanded < 0.0 {
            return Err(HeaderExtentsError::Negative);
        }
        if collapsed > Self::MAX || expanded > Self::MAX {
            return Err(HeaderExtentsError::TooLarge);
        }
        if collapsed > expanded {
            return Err(HeaderExtentsError::Inverted);
        }
        Ok(Self {
            collapsed,
            expanded,
        })
    }

    /// How far the header collapses.
    fn range(self) -> f64 {
        self.expanded - self.collapsed
    }
}

/// When a [`CollapsingHeader`] takes a scroll, relative to the content
/// beneath it.
///
/// Exhaustive: the header either goes first or takes what the content
/// leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeaderOrder {
    /// Before the content moves: the content waits until the header is
    /// fully collapsed (or expanded).
    BeforeContent,
    /// With what the content leaves at its edge: the header moves once the
    /// content reaches its end (or start), within the channel's latch.
    AfterContent,
}

/// A header above scrolling content that the content's scroll collapses and
/// expands.
///
/// The header is laid out at its expanded extent and clipped to its current
/// one, so its top stays put while its bottom slides under the body; the body
/// fills the rest. By default a scroll toward the content's end collapses the
/// header before the content moves, and a scroll toward the start expands it
/// only once the content is back at its start. [`expand`](Self::expand) with
/// [`HeaderOrder::BeforeContent`] brings it back on any scroll toward the
/// start instead.
///
/// The header publishes a labelled banner landmark whose scroll actions
/// expand it (up) and collapse it (down), and it expands when keyboard or
/// assistive focus moves onto content it hides. A [`VsyncScope`] above it animates its settles;
/// without one it jumps to where a settle would rest.
///
/// ```rust
/// use flui_widgets::{CollapsingHeader, HeaderExtents, Scrollable, SizedBox};
///
/// let extents = HeaderExtents::new(56.0, 200.0).expect("ordered, finite extents");
/// let view = CollapsingHeader::new(
///     extents,
///     SizedBox::new(400.0, 200.0),
///     Scrollable::new().child(SizedBox::new(400.0, 2000.0)),
/// )
/// .semantic_label("Inbox");
/// ```
#[derive(Clone, StatefulView)]
pub struct CollapsingHeader {
    extents: HeaderExtents,
    collapse: HeaderOrder,
    expand: HeaderOrder,
    snap: Option<SharedSnapPolicy>,
    semantic_label: Option<String>,
    header: BoxedView,
    body: BoxedView,
}

impl std::fmt::Debug for CollapsingHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CollapsingHeader")
            .field("extents", &self.extents)
            .field("collapse", &self.collapse)
            .field("expand", &self.expand)
            .field("snap", &self.snap.is_some())
            .finish_non_exhaustive()
    }
}

impl CollapsingHeader {
    /// `header` of `extents` above `body`, whose scrollables collapse it.
    pub fn new(extents: HeaderExtents, header: impl IntoView, body: impl IntoView) -> Self {
        Self {
            extents,
            collapse: HeaderOrder::BeforeContent,
            expand: HeaderOrder::AfterContent,
            snap: None,
            semantic_label: None,
            header: header.into_view().boxed(),
            body: body.into_view().boxed(),
        }
    }

    /// When a scroll toward the content's end collapses the header.
    /// Defaults to [`HeaderOrder::BeforeContent`].
    #[must_use]
    pub fn collapse(mut self, order: HeaderOrder) -> Self {
        self.collapse = order;
        self
    }

    /// When a scroll toward the content's start expands the header.
    /// Defaults to [`HeaderOrder::AfterContent`].
    #[must_use]
    pub fn expand(mut self, order: HeaderOrder) -> Self {
        self.expand = order;
        self
    }

    /// Settle the header through `policy` once the scrolls that moved it end.
    ///
    /// The policy sees the header's collapse as a scroll from `0` (expanded)
    /// to the extents' difference (collapsed) over a viewport of that
    /// length, so [`PageSnap`](super::PageSnap) of a
    /// [`ViewportFraction::FULL`](super::ViewportFraction::FULL) rests it fully
    /// expanded or fully collapsed, and the release's speed picks the side.
    #[must_use]
    pub fn snap_policy(mut self, policy: SharedSnapPolicy) -> Self {
        self.snap = Some(policy);
        self
    }

    /// The name assistive technology reads for the header's banner.
    #[must_use]
    pub fn semantic_label(mut self, label: impl Into<String>) -> Self {
        self.semantic_label = Some(label.into());
        self
    }
}

/// What the header's scroll consumption reads, copied from the view.
#[derive(Clone, Copy)]
struct Config {
    extents: HeaderExtents,
    collapse: HeaderOrder,
    expand: HeaderOrder,
}

/// The header's collapse and the participant that moves it, shared by the
/// widget state, the chain and the header's builder on the owner thread.
struct HeaderModel {
    config: Cell<Config>,
    snap: RefCell<Option<SharedSnapPolicy>>,
    /// How far the header is collapsed: `0` expanded, the extents'
    /// difference collapsed.
    offset: Cell<f64>,
    /// Sequences that started here and have not ended.
    live: Cell<u32>,
    /// User motion moved the header since it last came to rest.
    moved: Cell<bool>,
    /// The latest release and its velocity along the collapse.
    release: Cell<Option<(InputSequenceId, f64)>>,
    /// The settle runner; its value is the offset while it runs.
    settle: AnimationController,
    /// A frame clock drives the settle runner.
    clocked: Cell<bool>,
    pipeline: RefCell<Option<WeakPipelineCell>>,
    notifier: ChangeNotifier,
}

impl HeaderModel {
    fn new(config: Config, snap: Option<SharedSnapPolicy>, settle: AnimationController) -> Self {
        Self {
            config: Cell::new(config),
            snap: RefCell::new(snap),
            offset: Cell::new(0.0),
            live: Cell::new(0),
            moved: Cell::new(false),
            release: Cell::new(None),
            settle,
            clocked: Cell::new(false),
            pipeline: RefCell::new(None),
            notifier: ChangeNotifier::new(),
        }
    }

    fn range(&self) -> f64 {
        self.config.get().extents.range()
    }

    /// The header's current extent.
    fn extent(&self) -> f64 {
        let extents = self.config.get().extents;
        (extents.expanded - self.offset.get()).clamp(extents.collapsed, extents.expanded)
    }

    fn device_pixel_ratio(&self) -> f64 {
        presentation_device_pixel_ratio(self.pipeline.borrow().as_ref())
    }

    /// Within half a device pixel of `target`.
    fn rests_on(&self, target: f64) -> bool {
        (self.offset.get() - target).abs() <= rest_slop(self.device_pixel_ratio())
    }

    fn set_offset(&self, offset: f64) {
        let offset = if offset.is_finite() {
            offset.clamp(0.0, self.range())
        } else {
            self.offset.get()
        };
        if offset != self.offset.get() {
            self.offset.set(offset);
            self.notifier.notify_listeners();
        }
    }

    /// Take what `order` allows of the step's vertical motion: collapse
    /// toward the content's end (positive), expand toward its start
    /// (negative).
    fn take(&self, step: &ScrollStep, order: HeaderOrder) -> Vec2 {
        if matches!(step.cause, ScrollCause::UserInput | ScrollCause::Momentum) {
            // New user input takes the header from a settle in progress.
            let _ = self.settle.stop();
        }
        let config = self.config.get();
        let delta = step.available.y;
        let offset = self.offset.get();
        let taken = if delta > 0.0 && config.collapse == order {
            delta.min(self.range() - offset).max(0.0)
        } else if delta < 0.0 && config.expand == order {
            delta.max(-offset).min(0.0)
        } else {
            0.0
        };
        if taken == 0.0 || !taken.is_finite() {
            return Vec2::ZERO;
        }
        self.moved.set(true);
        self.set_offset(offset + taken);
        Vec2::new(0.0, taken)
    }

    /// Settle on `target` from where the header is, at `velocity` along the
    /// collapse; a jump without a frame clock.
    fn settle_to(&self, target: f64, velocity: f64) {
        self.moved.set(false);
        let target = target.clamp(0.0, self.range());
        if self.rests_on(target) {
            self.set_offset(target);
            return;
        }
        if !self.clocked.get() {
            self.set_offset(target);
            return;
        }
        let metrics = self.metrics();
        match settle_spring(&metrics, target, velocity) {
            Some(spring) => self.run(spring),
            None => self.set_offset(target),
        }
    }

    /// Carry a fling residual of `velocity` on: through the snap policy, or
    /// coasting under clamping friction to where it stops.
    fn coast(&self, velocity: f64) {
        let snap = self.snap.borrow().clone();
        if let Some(snap) = snap {
            let target = snap_target(snap.as_ref(), &self.metrics(), velocity);
            self.settle_to(target, velocity);
            return;
        }
        self.moved.set(false);
        if !self.clocked.get() {
            self.set_offset(if velocity > 0.0 { self.range() } else { 0.0 });
            return;
        }
        let simulation =
            ClampingScrollPhysics::new().create_ballistic_simulation(&self.metrics(), velocity);
        if let Some(simulation) = simulation {
            self.run(simulation);
        }
    }

    fn run(&self, simulation: impl flui_animation::Simulation + 'static) {
        self.settle.set_value(self.offset.get());
        if self.settle.animate_with(simulation).is_err() {
            tracing::warn!("CollapsingHeader could not start its settle");
        }
    }

    fn metrics(&self) -> ScrollMetrics {
        let range = self.range();
        ScrollMetrics::new(self.offset.get(), 0.0, range, range)
            .with_device_pixel_ratio(self.device_pixel_ratio())
    }

    /// Every sequence that moved the header is over: settle through the
    /// snap policy, at the speed of the release that let go of it.
    fn rest(&self, sequence: InputSequenceId, reason: ScrollEndReason) {
        if self.live.get() > 0 || !self.moved.get() || reason == ScrollEndReason::Preempted {
            return;
        }
        let snap = self.snap.borrow().clone();
        let Some(snap) = snap else {
            self.moved.set(false);
            return;
        };
        let velocity = self
            .release
            .get()
            .filter(|(id, _)| *id == sequence)
            .map_or(0.0, |(_, velocity)| velocity);
        let target = snap_target(snap.as_ref(), &self.metrics(), velocity);
        self.settle_to(target, velocity);
    }
}

impl Listenable for HeaderModel {
    fn add_observer(&self, observer: flui_foundation::ListenerObserver) -> ListenerId {
        self.notifier.add_observer(observer)
    }

    fn add_listener(&self, callback: flui_foundation::ListenerCallback) -> ListenerId {
        self.notifier.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.notifier.remove_all_listeners();
    }
}

impl NestedScrollConnection for HeaderModel {
    fn sequence_started(&self, _sequence: InputSequenceId) {
        self.live.set(self.live.get().saturating_add(1));
    }

    fn pre_scroll(&self, step: &ScrollStep) -> Vec2 {
        self.take(step, HeaderOrder::BeforeContent)
    }

    fn post_scroll(&self, step: &ScrollStep, _consumed_by_child: Vec2) -> Vec2 {
        self.take(step, HeaderOrder::AfterContent)
    }

    fn released(&self, step: &FlingStep) {
        let velocity = step.available.pixels_per_second.dy;
        self.release
            .set(velocity.is_finite().then_some((step.sequence, velocity)));
    }

    fn post_fling(&self, step: &FlingStep) -> FlingHandoff {
        let velocity = step.available.pixels_per_second.dy;
        let config = self.config.get();
        let offset = self.offset.get();
        let carries = if velocity > 0.0 {
            config.collapse == HeaderOrder::AfterContent && offset < self.range()
        } else if velocity < 0.0 {
            config.expand == HeaderOrder::AfterContent && offset > 0.0
        } else {
            false
        };
        if !carries || !velocity.is_finite() {
            return FlingHandoff::Declined;
        }
        self.coast(velocity);
        FlingHandoff::Accepted
    }

    fn sequence_ended(&self, sequence: InputSequenceId, reason: ScrollEndReason) {
        self.live.set(self.live.get().saturating_sub(1));
        self.rest(sequence, reason);
    }
}

/// The header as the reveal scope of the focus inside it: content it clips
/// expands it.
struct HeaderReveal {
    model: Rc<HeaderModel>,
    /// The clipped header box's geometry, from the `Focus` around it.
    header: Rc<FocusNode>,
    parent: RefCell<Option<SharedRevealTarget>>,
}

impl RevealTarget for HeaderReveal {
    fn reveal(&self, rect: Rect<f64>) {
        let visible = self.header.rect();
        let mut shown = rect;
        if !visible.contains_rect(&rect) {
            self.model.settle_to(0.0, 0.0);
            let expanded = Rect::from_ltwh(
                visible.left(),
                visible.top(),
                visible.width(),
                self.model.config.get().extents.expanded,
            );
            shown = rect.intersect(&expanded).unwrap_or(expanded);
        }
        let parent = self.parent.borrow().clone();
        if let Some(parent) = parent {
            parent.reveal(shown);
        }
    }
}

/// Persistent state for [`CollapsingHeader`]: the collapse, its settle
/// runner and the header's reveal scope.
pub struct CollapsingHeaderState {
    model: Rc<HeaderModel>,
    settle: DrivenController,
    settle_listener: Option<ListenerId>,
    reveal: Rc<HeaderReveal>,
}

impl std::fmt::Debug for CollapsingHeaderState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CollapsingHeaderState")
            .field("offset", &self.model.offset.get())
            .finish_non_exhaustive()
    }
}

impl StatefulView for CollapsingHeader {
    type State = CollapsingHeaderState;

    fn create_state(&self) -> Self::State {
        let settle = AnimationController::builder(Duration::from_millis(1))
            .debug_label("CollapsingHeader.settle")
            .behavior(flui_animation::AnimationBehavior::Preserve)
            .unbounded()
            .build_on(None);
        let model = Rc::new(HeaderModel::new(
            Config {
                extents: self.extents,
                collapse: self.collapse,
                expand: self.expand,
            },
            self.snap.clone(),
            settle.controller().clone(),
        ));
        CollapsingHeaderState {
            reveal: Rc::new(HeaderReveal {
                model: Rc::clone(&model),
                header: FocusNode::with_debug_label("CollapsingHeader"),
                parent: RefCell::new(None),
            }),
            model,
            settle,
            settle_listener: None,
        }
    }
}

impl CollapsingHeaderState {
    fn acquire(&mut self, ctx: &dyn LifecycleContext) {
        if let Err(error) = self.settle.rebind(VsyncScope::maybe_of(ctx).as_ref()) {
            tracing::error!(%error, "CollapsingHeader lost its frame registry");
        }
        self.model.clocked.set(self.settle.is_bound());
        *self.model.pipeline.borrow_mut() = ctx.pipeline_owner().map(|cell| cell.downgrade());
    }
}

impl ViewState<CollapsingHeader> for CollapsingHeaderState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let model = Rc::downgrade(&self.model);
        let settle = self.settle.controller().clone();
        self.settle_listener = Some(settle.add_listener(Rc::new(move || {
            if let Some(model) = model.upgrade() {
                model.set_offset(model.settle.value());
            }
        })));
        self.acquire(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.acquire(ctx);
    }

    fn did_update_view(&mut self, _old_view: &CollapsingHeader, new_view: &CollapsingHeader) {
        let model = &self.model;
        let old = model.config.get();
        model.config.set(Config {
            extents: new_view.extents,
            collapse: new_view.collapse,
            expand: new_view.expand,
        });
        model.snap.replace(new_view.snap.clone());
        if old.extents != new_view.extents {
            // Keep how far through its collapse the header is.
            let fraction = if old.extents.range() > 0.0 {
                model.offset.get() / old.extents.range()
            } else {
                0.0
            };
            let _ = model.settle.stop();
            model.offset.set(fraction * new_view.extents.range());
            model.notifier.notify_listeners();
        }
    }

    fn build(&self, view: &CollapsingHeader, ctx: &dyn BuildContext) -> impl IntoView {
        self.reveal.parent.replace(FocusRevealScope::of(ctx));
        let model = Rc::clone(&self.model);
        let reveal = Rc::clone(&self.reveal);
        let header = view.header.clone();
        let label = view.semantic_label.clone();
        let header_box =
            AnimatedBuilder::new(Rc::clone(&self.model) as Rc<dyn Listenable>, move || {
                header_box(&model, &reveal, &header, label.as_deref())
            });
        let connection = Rc::clone(&self.model) as Rc<dyn NestedScrollConnection>;
        Column::new(vec![
            header_box.boxed(),
            Expanded::new(NestedScroll::new(connection, view.body.clone())).boxed(),
        ])
    }

    fn dispose(&mut self) {
        if let Some(id) = self.settle_listener.take() {
            self.settle.controller().remove_listener(id);
        }
        let _ = self.settle.controller().stop();
        self.reveal.parent.borrow_mut().take();
    }
}

/// The header at its current extent: its content laid out at the expanded
/// extent from the top and clipped below, inside the banner landmark and the
/// reveal scope of the focus in it.
fn header_box(
    model: &Rc<HeaderModel>,
    reveal: &Rc<HeaderReveal>,
    header: &BoxedView,
    label: Option<&str>,
) -> BoxedView {
    let extents = model.config.get().extents;
    let content = SizedBox::new(f64::INFINITY, model.extent()).child(
        ClipRect::new().child(
            OverflowBox::new()
                .with_alignment(Alignment::TOP_CENTER)
                .with_min_height(extents.expanded)
                .with_max_height(extents.expanded)
                .child(header.clone()),
        ),
    );
    let target: SharedRevealTarget = Rc::clone(reveal) as SharedRevealTarget;
    let scoped = Focus::new(FocusRevealScope::new(target, content))
        .focus_node(Rc::clone(&reveal.header))
        .can_request_focus(false)
        .skip_traversal(true)
        .include_semantics(false);
    let mut semantics = Semantics::new().container(true).role(SemanticsRole::Banner);
    if let Some(label) = label {
        semantics = semantics.label(label.to_owned());
    }
    if extents.range() > 0.0 {
        // Assistive technology moves the header the way a scroll would: up
        // brings it back, down sends it away. A banner has no expanded state
        // (ARIA admits `aria-expanded` on controls only).
        if !model.rests_on(0.0) {
            let expand = Rc::clone(model);
            semantics = semantics.on_scroll_up(move |_cx, _unit| expand.settle_to(0.0, 0.0));
        }
        if !model.rests_on(model.range()) {
            let collapse = Rc::clone(model);
            semantics = semantics
                .on_scroll_down(move |_cx, _unit| collapse.settle_to(collapse.range(), 0.0));
        }
        let into_view = Rc::clone(model);
        semantics = semantics.on_scroll_into_view(move |_cx, request| {
            // A request for the banner itself, or for content it clips.
            let clipped = request
                .is_none_or(|reveal| !reveal.viewport_rect.contains_rect(&reveal.target_rect));
            if clipped {
                into_view.settle_to(0.0, 0.0);
            }
        });
    }
    semantics.child(scoped).boxed()
}
