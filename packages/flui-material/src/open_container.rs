//! [`OpenContainer`] — the container transform: a closed surface (a card, a
//! list row, a button) that grows into a full page and shrinks back.
//!
//! # How it flies
//!
//! The closed surface is the *source*. Tapping it measures where it sits and
//! pushes a route whose [`PageTransitionBuilder`] (ADR-0221) morphs one surface
//! from the source's rect to the navigator's rect. The morph's frame is new, so
//! it can only be measured once it has been laid out: until then the morph
//! draws nothing and the source shows in place. On that first measurement the
//! source freezes its [`FlightSeat`] (it keeps its size, its child goes
//! offstage so its state survives, and nothing under it can be hit or
//! announced) and the morph takes over, at the source's exact rect whatever
//! the navigator's offset in the window:
//!
//! | Property | Closed → open |
//! |---|---|
//! | rect | linear in the eased progress |
//! | colour, elevation, corner radii | linear in the eased progress ([`BorderRadius::lerp`]) |
//! | closed content | laid out at its closed size, scaled to the surface width, faded out |
//! | open page | laid out at the navigator's size, scaled to the surface width, faded in |
//!
//! [`ContainerTransition`] picks the fade schedule. The default motion is
//! 300 ms along `FAST_OUT_SLOW_IN`, and the exit runs the same curve flipped.
//! Under reduced motion the route animation settles on its first frame, so the
//! page is simply there.
//!
//! # The source is measured again
//!
//! The source may move while the page covers it (a list that scrolled, a
//! layout that changed). The morph re-measures it on every tick of the route
//! animation, in the morph frame's own coordinates, so the exit lands on where the
//! source is now. The measurement reads the previous frame's layout and never
//! the frame being built.
//!
//! # The result
//!
//! The open page closes with [`CloseAction::close_with`], which pops the route
//! with a typed value. [`OpenContainer::on_closed`] receives it as
//! `Option<T>`; `None` means the page was dismissed without a value. The
//! callback runs once the exit morph has left the tree and the source is back
//! in its seat, never under the closing transform, so it may change or remove
//! the source freely. A page the navigator accepted reports its result even if
//! the container unmounted meanwhile.
//!
//! # Under a `Router`
//!
//! A navigator a [`Router`](flui_sdk::widgets::Router) drives owns its pages
//! and admits only popups, so there the open page is a [`PopupRoute`] instead
//! of a [`PageRoute`]. The morph is the same; the page below stays in the tree
//! while the open page covers it, as under any popup.
//!
//! # Focus and hit testing
//!
//! While the page is open the source is frozen offstage: it is not laid out at
//! its own size, painted, hit tested or announced, so it cannot compete with
//! the page. The closed copy the morph draws is decoration only: it ignores
//! pointers and is excluded from semantics. A second [`OpenAction::open`]
//! while the page is open does nothing, so a key press that still reaches the
//! hidden source cannot push a second page.
//!
//! # Semantics
//!
//! The closed container is a button that opens the page, named by
//! [`OpenContainer::semantic_label`].
//!
//! # Not modelled
//!
//! Morph shapes other than rounded rectangles, and a morph into a navigator
//! that is not a bounded rect (an unbounded page shows the page unmorphed).

use std::cell::{Cell, RefCell};
use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;
use std::task::{Poll, Waker};
use std::time::Duration;

use flui_sdk::animation::{AnimationStatus, ArcCurve, Curve, curves};
use flui_sdk::foundation::ListenerId;
use flui_sdk::geometry::Rect;
use flui_sdk::painting::{Alignment, BorderRadius, BorderRadiusExt, Clip, Color};
use flui_sdk::view::element::ElementKind;
use flui_sdk::view::prelude::*;
use flui_sdk::view::{AsyncDriver, BoxedView, RebuildHandle, RebuildReason, ViewExt};
use flui_sdk::widgets::__private::{AnchoredBox, FlightSeat, SeatClaim, SeatHold};
use flui_sdk::widgets::prelude::BoxFit;
use flui_sdk::widgets::{
    ExcludeSemantics, FittedBox, IgnorePointer, LayoutBuilder, NavigatorHandle, Offstage, Opacity,
    PageRoute, PageTransitionBuilder, PopupRoute, Positioned, RouteAnimation, Semantics,
    SemanticsRole, SizedBox, Stack,
};

use crate::ink_well::InkWell;
use crate::material::Material;
use crate::shape::MaterialShape;
use crate::theme::Theme;

/// The default motion's duration.
const DEFAULT_DURATION: Duration = Duration::from_millis(300);

/// The eased progress the morph admits. Overshooting curves (back, elastic)
/// stay well inside it; a value beyond it is clamped so the interpolated
/// geometry stays finite.
const MIN_PROGRESS: f64 = -1.0;
const MAX_PROGRESS: f64 = 2.0;

/// How the closed and the open content trade places while the surface grows.
///
/// Both schedules run on the eased progress, so they follow the curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ContainerTransition {
    /// A dissolve: the closed content fades out over the first 40 % while the
    /// open content fades in from 20 %, so the two overlap.
    #[default]
    Fade,
    /// A fade through the surface colour: the closed content is gone after the
    /// first 30 % and only then does the open content fade in.
    FadeThrough,
}

impl ContainerTransition {
    /// The closed content's opacity at eased progress `t`.
    fn closed_opacity(self, t: f64) -> f64 {
        let end = match self {
            Self::Fade => 0.4,
            Self::FadeThrough => 0.3,
        };
        1.0 - interval(t, 0.0, end)
    }

    /// The open content's opacity at eased progress `t`.
    fn open_opacity(self, t: f64) -> f64 {
        let start = match self {
            Self::Fade => 0.2,
            Self::FadeThrough => 0.3,
        };
        interval(t, start, 1.0)
    }
}

/// `t` mapped from `[start, end]` onto `[0, 1]`, clamped.
fn interval(t: f64, start: f64, end: f64) -> f64 {
    ((t - start) / (end - start)).clamp(0.0, 1.0)
}

/// Opens the page of the [`OpenContainer`] it was handed to.
///
/// A closed content builder receives one to open the page from anywhere in
/// the closed content, for a container that is not [`tappable`](OpenContainer::tappable).
#[derive(Clone)]
pub struct OpenAction {
    open: Rc<dyn Fn()>,
}

impl OpenAction {
    /// Open the page. Does nothing while it is already open, or when the
    /// container has no navigator above it or has not been laid out.
    pub fn open(&self) {
        (self.open)();
    }

    /// An action that opens nothing: what the closed copy inside the morph
    /// holds.
    fn inert() -> Self {
        Self {
            open: Rc::new(|| {}),
        }
    }
}

impl fmt::Debug for OpenAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OpenAction")
    }
}

/// Closes the open page of an [`OpenContainer`], optionally with a result.
///
/// It pops the navigator the page lives in, so it closes the container when
/// the page is that navigator's current route.
pub struct CloseAction<T> {
    navigator: Option<NavigatorHandle>,
    result: PhantomData<fn(T)>,
}

impl<T> Clone for CloseAction<T> {
    fn clone(&self) -> Self {
        Self {
            navigator: self.navigator.clone(),
            result: PhantomData,
        }
    }
}

impl<T> fmt::Debug for CloseAction<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloseAction")
            .field("navigator", &self.navigator.is_some())
            .finish()
    }
}

impl<T: Send + 'static> CloseAction<T> {
    /// The action for the page built at `ctx`.
    fn of(ctx: &dyn BuildContext) -> Self {
        Self {
            navigator: NavigatorHandle::maybe_of(ctx),
            result: PhantomData,
        }
    }

    /// Close the page without a value: [`OpenContainer::on_closed`] receives
    /// `None`. Returns whether a route was popped.
    pub fn close(&self) -> bool {
        self.navigator.as_ref().is_some_and(NavigatorHandle::pop)
    }

    /// Close the page with `result`. Returns whether a route was popped.
    pub fn close_with(&self, result: T) -> bool {
        self.navigator
            .as_ref()
            .is_some_and(|navigator| navigator.pop_with(result))
    }
}

type ClosedBuilder = Rc<dyn Fn(&dyn BuildContext, &OpenAction) -> BoxedView>;
type OpenBuilder<T> = Rc<dyn Fn(&dyn BuildContext, &CloseAction<T>) -> BoxedView>;
type ClosedCallback<T> = Rc<dyn Fn(Option<T>)>;

/// A surface that opens into a page and closes back into the surface.
///
/// ```
/// use flui_material::{ContainerTransition, OpenContainer};
/// use flui_sdk::view::prelude::*;
/// use flui_sdk::widgets::Text;
///
/// let card = OpenContainer::<bool>::new(
///     |_ctx, _open| Text::new("Open me"),
///     |_ctx, _close| Text::new("Opened"),
/// )
/// .transition(ContainerTransition::FadeThrough)
/// .on_closed(|saved| println!("closed with {saved:?}"));
/// # let _ = card;
/// ```
pub struct OpenContainer<T> {
    closed: ClosedBuilder,
    open: OpenBuilder<T>,
    closed_color: Option<Color>,
    open_color: Option<Color>,
    closed_elevation: f64,
    open_elevation: f64,
    closed_radius: BorderRadius,
    open_radius: BorderRadius,
    transition: ContainerTransition,
    duration: Duration,
    curve: ArcCurve,
    tappable: bool,
    semantic_label: Option<String>,
    on_closed: Option<ClosedCallback<T>>,
}

impl<T> Clone for OpenContainer<T> {
    fn clone(&self) -> Self {
        Self {
            closed: Rc::clone(&self.closed),
            open: Rc::clone(&self.open),
            closed_color: self.closed_color,
            open_color: self.open_color,
            closed_elevation: self.closed_elevation,
            open_elevation: self.open_elevation,
            closed_radius: self.closed_radius,
            open_radius: self.open_radius,
            transition: self.transition,
            duration: self.duration,
            curve: self.curve.clone(),
            tappable: self.tappable,
            semantic_label: self.semantic_label.clone(),
            on_closed: self.on_closed.clone(),
        }
    }
}

impl<T> fmt::Debug for OpenContainer<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenContainer")
            .field("transition", &self.transition)
            .field("duration", &self.duration)
            .field("tappable", &self.tappable)
            .finish_non_exhaustive()
    }
}

impl<T: Send + Clone + 'static> OpenContainer<T> {
    /// A container showing `closed` until it is opened, then `open` as a page.
    ///
    /// `closed` is built inside the container's surface and gets an
    /// [`OpenAction`]; `open` is built as the page and gets a [`CloseAction`]
    /// for the result type `T`.
    #[must_use]
    pub fn new<CV, OV>(
        closed: impl Fn(&dyn BuildContext, &OpenAction) -> CV + 'static,
        open: impl Fn(&dyn BuildContext, &CloseAction<T>) -> OV + 'static,
    ) -> Self
    where
        CV: IntoView,
        OV: IntoView,
    {
        Self {
            closed: Rc::new(move |ctx, action| closed(ctx, action).into_view().boxed()),
            open: Rc::new(move |ctx, action| open(ctx, action).into_view().boxed()),
            closed_color: None,
            open_color: None,
            closed_elevation: 1.0,
            open_elevation: 0.0,
            closed_radius: BorderRadius::all(flui_sdk::geometry::Radius::circular(12.0)),
            open_radius: BorderRadius::all(flui_sdk::geometry::Radius::ZERO),
            transition: ContainerTransition::default(),
            duration: DEFAULT_DURATION,
            curve: ArcCurve::new(curves::FAST_OUT_SLOW_IN),
            tappable: true,
            semantic_label: None,
            on_closed: None,
        }
    }

    /// The closed surface's colour. Defaults to the theme's
    /// `surface_container_low`.
    #[must_use]
    pub fn closed_color(mut self, color: Color) -> Self {
        self.closed_color = Some(color);
        self
    }

    /// The open page's surface colour. Defaults to the theme's `surface`.
    #[must_use]
    pub fn open_color(mut self, color: Color) -> Self {
        self.open_color = Some(color);
        self
    }

    /// The closed surface's elevation. Defaults to 1.
    #[must_use]
    pub fn closed_elevation(mut self, elevation: f64) -> Self {
        self.closed_elevation = elevation;
        self
    }

    /// The open page's elevation. Defaults to 0.
    #[must_use]
    pub fn open_elevation(mut self, elevation: f64) -> Self {
        self.open_elevation = elevation;
        self
    }

    /// The closed surface's corner radii. Defaults to 12.
    #[must_use]
    pub fn closed_radius(mut self, radius: BorderRadius) -> Self {
        self.closed_radius = radius;
        self
    }

    /// The open page's corner radii. Defaults to square.
    #[must_use]
    pub fn open_radius(mut self, radius: BorderRadius) -> Self {
        self.open_radius = radius;
        self
    }

    /// How the closed and open content trade places. Defaults to
    /// [`ContainerTransition::Fade`].
    #[must_use]
    pub fn transition(mut self, transition: ContainerTransition) -> Self {
        self.transition = transition;
        self
    }

    /// How long the morph runs, in each direction. Defaults to 300 ms.
    #[must_use]
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// The morph's easing; the exit runs it flipped. Defaults to
    /// `FAST_OUT_SLOW_IN`.
    #[must_use]
    pub fn curve(mut self, curve: impl Curve + Send + Sync + 'static) -> Self {
        self.curve = ArcCurve::new(curve);
        self
    }

    /// Whether tapping the surface opens the page. Defaults to `true`; with
    /// `false` the page opens only from an [`OpenAction`] the closed builder
    /// calls, and the surface is not announced as a button.
    #[must_use]
    pub fn tappable(mut self, tappable: bool) -> Self {
        self.tappable = tappable;
        self
    }

    /// The label assistive technology announces for the closed button.
    #[must_use]
    pub fn semantic_label(mut self, label: impl Into<String>) -> Self {
        self.semantic_label = Some(label.into());
        self
    }

    /// Called with the page's result once the route completes: the value a
    /// [`CloseAction::close_with`] passed, or `None`.
    #[must_use]
    pub fn on_closed(mut self, callback: impl Fn(Option<T>) + 'static) -> Self {
        self.on_closed = Some(Rc::new(callback));
        self
    }
}

/// What the morph needs from the container, fixed when the page opens.
#[derive(Clone)]
struct Look {
    closed_color: Color,
    open_color: Color,
    closed_elevation: f64,
    open_elevation: f64,
    closed_radius: BorderRadius,
    open_radius: BorderRadius,
    transition: ContainerTransition,
    curve: ArcCurve,
}

/// One open page: the source's seat, the rects the morph runs between, and
/// the bookkeeping that decides when the source freezes and when it comes back.
struct Flight {
    seat: Rc<FlightSeat>,
    claim: SeatClaim,
    /// Rebuilds the source when it freezes or is released.
    rebuild: Option<RebuildHandle>,
    look: Look,
    closed: ClosedBuilder,
    duration: Duration,
    /// The source's rect in the morph frame's own coordinates, measured again
    /// every tick; `None` until the frame has been laid out and measured. Until
    /// then the morph draws nothing and the source stays in place, so no frame
    /// shows it displaced. Mapped through the render root, so a transform
    /// above the navigator (a scale, a rotation) is not applied twice.
    closed_in_frame: Cell<Option<Rect>>,
    /// The source is frozen under this flight's claim.
    airborne: Cell<bool>,
    /// The page's morph is in the tree.
    mounted: Cell<bool>,
    /// The page's route has completed.
    completed: Cell<bool>,
    /// Whether a completion will ever be reported; without it the source
    /// returns as soon as the morph leaves the tree.
    tracks_completion: Cell<bool>,
    /// The flight has landed: the source is back. Shared with the container,
    /// which opens no second page while a flight is in the air.
    landed: Rc<Landing>,
}

impl Flight {
    /// Measure the source and the frame again. A measurement that is not
    /// available (not laid out, or a layout pass in progress) keeps the last
    /// good one. The first frame measurement is when the source takes off.
    fn measure(&self, frame: &FlightSeat) {
        if let Some(rect) = self.seat.bounds_in_seat(frame) {
            self.closed_in_frame.set(Some(rect));
            self.take_off();
        }
    }

    /// Freeze the source once the morph can draw it where it sits. Inert once
    /// airborne or landed.
    fn take_off(&self) {
        if self.airborne.get() || self.landed.is_landed() {
            return;
        }
        self.airborne.set(true);
        if self
            .seat
            .freeze(SeatHold::Claimed(self.claim.clone()), true)
            .is_some()
            && let Some(rebuild) = &self.rebuild
        {
            rebuild.schedule(RebuildReason::StateChange);
        }
    }

    /// The source goes back to its place when the page is both gone and
    /// completed, so a morph that leaves the tree while the route is still
    /// alive (a page covered with `maintain_state: false`) keeps it frozen.
    /// Landing wakes the result's delivery; it never runs user code itself,
    /// because it can be reached from the morph's `dispose`.
    fn settle(&self) {
        if self.landed.is_landed()
            || self.mounted.get()
            || (self.tracks_completion.get() && !self.completed.get())
        {
            return;
        }
        if self.seat.release_for(&self.claim, false)
            && let Some(rebuild) = &self.rebuild
        {
            rebuild.schedule(RebuildReason::StateChange);
        }
        self.landed.land();
    }

    fn set_mounted(&self, mounted: bool) {
        self.mounted.set(mounted);
        self.settle();
    }

    fn complete(&self) {
        self.completed.set(true);
        self.settle();
    }
}

/// A one-shot signal that a flight has landed, awaitable by the task that
/// delivers its result.
#[derive(Default)]
struct Landing {
    landed: Cell<bool>,
    waker: RefCell<Option<Waker>>,
}

impl Landing {
    fn is_landed(&self) -> bool {
        self.landed.get()
    }

    fn land(&self) {
        self.landed.set(true);
        let waker = self.waker.take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Resolves once [`land`](Self::land) has run.
    fn wait(self: Rc<Self>) -> impl Future<Output = ()> {
        std::future::poll_fn(move |cx| {
            if self.landed.get() {
                Poll::Ready(())
            } else {
                let previous = self.waker.replace(Some(cx.waker().clone()));
                drop(previous);
                Poll::Pending
            }
        })
    }
}

fn finite(rect: Rect) -> bool {
    [rect.left(), rect.top(), rect.width(), rect.height()]
        .iter()
        .all(|value| value.is_finite())
}

/// The state shared by a container's closures and its state object.
struct Shared<T> {
    seat: Rc<FlightSeat>,
    config: RefCell<OpenContainer<T>>,
    rebuild: RefCell<Option<RebuildHandle>>,
    driver: RefCell<Option<AsyncDriver>>,
    /// The landing of the latest flight; no second page opens before it.
    flight: RefCell<Option<Rc<Landing>>>,
}

impl<T: Send + Clone + 'static> Shared<T> {
    /// Push the page; the source freezes once the morph has measured its
    /// frame. Does nothing while a page is already in the air, or when the
    /// source has no committed layout to fly from.
    fn open(&self, navigator: &NavigatorHandle, look: &Look) {
        let in_flight = self
            .flight
            .borrow()
            .as_ref()
            .is_some_and(|landing| !landing.is_landed());
        if in_flight {
            return;
        }
        if self.seat.size().is_none() {
            return;
        }

        let config = self.config.borrow().clone();
        let landed = Rc::new(Landing::default());
        let previous = self.flight.replace(Some(Rc::clone(&landed)));
        drop(previous);
        let flight = Rc::new(Flight {
            seat: Rc::clone(&self.seat),
            claim: SeatClaim::default(),
            rebuild: self.rebuild.borrow().clone(),
            look: look.clone(),
            closed: Rc::clone(&config.closed),
            duration: config.duration,
            closed_in_frame: Cell::new(None),
            airborne: Cell::new(false),
            mounted: Cell::new(false),
            completed: Cell::new(false),
            tracks_completion: Cell::new(false),
            landed: Rc::clone(&landed),
        });

        let open = Rc::clone(&config.open);
        let page =
            move |ctx: &dyn BuildContext,
                  _primary: &RouteAnimation,
                  _secondary: &RouteAnimation| { open(ctx, &CloseAction::of(ctx)) };
        let transform = Rc::new(ContainerTransform {
            flight: Rc::clone(&flight),
        });
        let result = if navigator.admits_pages() {
            // The page below stays still: the source is measured on it, and a
            // covered transition (a zoom, a slide) would move it under the morph.
            navigator.push(
                PageRoute::<T>::new(page)
                    .transition_builder(transform)
                    .animates_route_below(false),
            )
        } else {
            // A Router owns this navigator's pages and admits only popups. A
            // popup does not coordinate with the page below, so it stays still.
            navigator.push(PopupRoute::<T>::new(page).transition_builder(transform))
        };

        let driver = self.driver.borrow().clone();
        let Some(driver) = driver else {
            tracing::warn!(
                "OpenContainer: no async driver on this tree; its result will not be delivered"
            );
            return;
        };
        flight.tracks_completion.set(true);
        let on_closed = config.on_closed.clone();
        let task_flight = Rc::clone(&flight);
        // The task owns its own token: the result of a page the navigator has
        // accepted is delivered even if the container has unmounted by then.
        // The route resolves its result as soon as the pop is accepted; the
        // value is held until the exit morph has left the tree and the source
        // is back, so `on_closed` never runs under the closing transform.
        let token_slot = Rc::new(RefCell::new(None));
        let task_slot = Rc::clone(&token_slot);
        let token = driver.spawn_local(Box::pin(async move {
            let value = result.await;
            task_flight.complete();
            let landing = Rc::clone(&task_flight.landed);
            drop(task_flight);
            landing.wait().await;
            if let Some(on_closed) = on_closed {
                on_closed(value);
            }
            drop(task_slot);
        }));
        if token.is_cancelled() {
            flight.tracks_completion.set(false);
        }
        *token_slot.borrow_mut() = Some(token);
    }
}

/// Persistent state behind [`OpenContainer`]: the source's seat.
pub struct OpenContainerState<T> {
    shared: Rc<Shared<T>>,
}

impl<T> fmt::Debug for OpenContainerState<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenContainerState")
            .field("seat", &self.shared.seat)
            .finish_non_exhaustive()
    }
}

impl<T: Send + Clone + 'static> View for OpenContainer<T> {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl<T: Send + Clone + 'static> StatefulView for OpenContainer<T> {
    type State = OpenContainerState<T>;

    fn create_state(&self) -> Self::State {
        OpenContainerState {
            shared: Rc::new(Shared {
                seat: Rc::new(FlightSeat::new()),
                config: RefCell::new(self.clone()),
                rebuild: RefCell::new(None),
                driver: RefCell::new(None),
                flight: RefCell::new(None),
            }),
        }
    }
}

impl<T: Send + Clone + 'static> ViewState<OpenContainer<T>> for OpenContainerState<T> {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.shared
            .seat
            .bind(ctx.pipeline_owner().map(|owner| owner.downgrade()));
        let previous = self.shared.rebuild.replace(Some(ctx.rebuild_handle()));
        drop(previous);
        let previous = self.shared.driver.replace(ctx.async_driver());
        drop(previous);
    }

    fn did_update_view(&mut self, _old: &OpenContainer<T>, new_view: &OpenContainer<T>) {
        let previous = self.shared.config.replace(new_view.clone());
        drop(previous);
    }

    fn build(&self, view: &OpenContainer<T>, ctx: &dyn BuildContext) -> impl IntoView {
        let scheme = Theme::of(ctx).color_scheme;
        let look = Look {
            closed_color: view.closed_color.unwrap_or(scheme.surface_container_low),
            open_color: view.open_color.unwrap_or(scheme.surface),
            closed_elevation: view.closed_elevation,
            open_elevation: view.open_elevation,
            closed_radius: view.closed_radius,
            open_radius: view.open_radius,
            transition: view.transition,
            curve: view.curve.clone(),
        };

        let navigator = NavigatorHandle::maybe_of(ctx);
        let shared = Rc::clone(&self.shared);
        let open_look = look.clone();
        let action = OpenAction {
            open: Rc::new(move || {
                if let Some(navigator) = &navigator {
                    shared.open(navigator, &open_look);
                }
            }),
        };

        let content = (view.closed)(ctx, &action);
        let mut well = InkWell::new(content).shape(MaterialShape::RoundedRect(look.closed_radius));
        let mut semantics = Semantics::new().container(true);
        if let Some(label) = &view.semantic_label {
            semantics = semantics.label(label.clone());
        }
        if view.tappable {
            let on_tap = action.clone();
            well = well.on_tap(move |_cx| on_tap.open());
            let on_click = action;
            semantics = semantics
                .role(SemanticsRole::Button)
                .on_click(move |_cx| on_click.open());
        }
        let surface = Material::new(look.closed_color)
            .elevation(look.closed_elevation)
            .shape(MaterialShape::RoundedRect(look.closed_radius))
            .clip_behavior(Clip::AntiAlias)
            .child(well);
        let closed: BoxedView = semantics.child(surface).into_view().boxed();
        self.shared.seat.build(&closed, None)
    }

    fn dispose(&mut self) {
        self.shared.seat.withdraw();
        let rebuild = self.shared.rebuild.take();
        let driver = self.shared.driver.take();
        drop((rebuild, driver));
    }
}

// ============================================================================
// The route's transition
// ============================================================================

/// The page transition of one open container: wraps the route's page in the
/// morph.
struct ContainerTransform {
    flight: Rc<Flight>,
}

impl PageTransitionBuilder for ContainerTransform {
    fn duration(&self) -> Duration {
        self.flight.duration
    }

    fn build(
        &self,
        _ctx: &dyn BuildContext,
        primary: &RouteAnimation,
        _secondary: &RouteAnimation,
        child: BoxedView,
    ) -> BoxedView {
        ContainerMorph {
            flight: Rc::clone(&self.flight),
            primary: Rc::clone(primary),
            page: child,
        }
        .boxed()
    }
}

/// The morph: one surface running from the source's rect to the frame's.
#[derive(Clone, StatefulView)]
struct ContainerMorph {
    flight: Rc<Flight>,
    primary: RouteAnimation,
    page: BoxedView,
}

impl fmt::Debug for ContainerMorph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContainerMorph").finish_non_exhaustive()
    }
}

struct MorphState {
    flight: Rc<Flight>,
    primary: RouteAnimation,
    /// The morph's own frame: where the navigator's page area sits.
    frame: Rc<FlightSeat>,
    listener: Option<ListenerId>,
}

impl fmt::Debug for MorphState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MorphState").finish_non_exhaustive()
    }
}

impl MorphState {
    /// Re-measure on every tick of the primary animation.
    fn listen(&mut self) {
        self.unlisten();
        let flight = Rc::clone(&self.flight);
        let frame = Rc::clone(&self.frame);
        self.listener = Some(
            self.primary
                .add_listener(Rc::new(move || flight.measure(&frame))),
        );
    }

    fn unlisten(&mut self) {
        if let Some(id) = self.listener.take() {
            self.primary.remove_listener(id);
        }
    }

    /// Measure the frame once the frame it was mounted in has laid it out,
    /// then redraw. A route whose animation is already settled when the morph
    /// mounts (a zero duration) never ticks again, so the tick listener alone
    /// would leave the morph offstage for good.
    fn measure_after_first_frame(&self, ctx: &dyn LifecycleContext) {
        let Some(post_frame) = ctx.post_frame_handle() else {
            return;
        };
        let flight = Rc::clone(&self.flight);
        let frame = Rc::clone(&self.frame);
        let rebuild = ctx.rebuild_handle();
        let scheduled = post_frame.schedule(move |_timing| {
            if !flight.mounted.get() || flight.closed_in_frame.get().is_some() {
                return;
            }
            flight.measure(&frame);
            if flight.closed_in_frame.get().is_some() {
                rebuild.schedule(RebuildReason::StateChange);
            }
        });
        if let Err(error) = scheduled {
            tracing::warn!(
                ?error,
                "OpenContainer: could not schedule the morph's first measurement"
            );
        }
    }
}

impl StatefulView for ContainerMorph {
    type State = MorphState;

    fn create_state(&self) -> Self::State {
        MorphState {
            flight: Rc::clone(&self.flight),
            primary: Rc::clone(&self.primary),
            frame: Rc::new(FlightSeat::new()),
            listener: None,
        }
    }
}

impl ViewState<ContainerMorph> for MorphState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.frame
            .bind(ctx.pipeline_owner().map(|owner| owner.downgrade()));
        self.listen();
        self.flight.set_mounted(true);
        self.measure_after_first_frame(ctx);
    }

    fn did_update_view(&mut self, _old: &ContainerMorph, new_view: &ContainerMorph) {
        if !Rc::ptr_eq(&self.primary, &new_view.primary) {
            self.primary = Rc::clone(&new_view.primary);
            self.listen();
        }
    }

    fn build(&self, view: &ContainerMorph, _ctx: &dyn BuildContext) -> impl IntoView {
        let flight = Rc::clone(&view.flight);
        let primary = Rc::clone(&view.primary);
        let page = view.page.clone();
        AnchoredBox::new(
            self.frame.anchor(),
            LayoutBuilder::new(move |ctx, constraints| {
                let size = constraints.biggest();
                if !size.width.is_finite() || !size.height.is_finite() {
                    return page.clone();
                }
                morph(
                    &flight,
                    &primary,
                    &page,
                    ctx,
                    Rect::from_ltwh(0.0, 0.0, size.width, size.height),
                )
            }),
        )
    }

    fn dispose(&mut self) {
        self.unlisten();
        self.flight.set_mounted(false);
    }
}

/// The eased progress of `primary`; the exit runs the curve flipped.
fn eased(primary: &RouteAnimation, curve: &ArcCurve) -> f64 {
    let raw = primary.value();
    if !raw.is_finite() {
        return 0.0;
    }
    match primary.status() {
        AnimationStatus::Reverse => 1.0 - curve.transform(1.0 - raw),
        _ => curve.transform(raw),
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// The morph at the current progress, inside a frame of `full`.
fn morph(
    flight: &Flight,
    primary: &RouteAnimation,
    page: &BoxedView,
    ctx: &dyn BuildContext,
    full: Rect,
) -> BoxedView {
    let look = &flight.look;
    // A curve may overshoot, but a finite value far outside the unit range
    // would carry the interpolated geometry to infinity: bound it to an
    // overshoot no real easing reaches.
    let t = eased(primary, &look.curve);
    let t = if t.is_finite() {
        t.clamp(MIN_PROGRESS, MAX_PROGRESS)
    } else {
        0.0
    };

    // Until the frame is measured the surface would land at the wrong place,
    // so the scene is kept offstage (same shape, nothing painted, hit or
    // announced) and the source, not yet frozen, still shows in its seat.
    let measured = flight.closed_in_frame.get();
    let closed = measured.unwrap_or(full);
    let rect = Rect::from_ltwh(
        lerp(closed.left(), full.left(), t),
        lerp(closed.top(), full.top(), t),
        lerp(closed.width(), full.width(), t).max(0.0),
        lerp(closed.height(), full.height(), t).max(0.0),
    );
    // Never publish non-finite geometry; the open rect is always valid.
    let rect = if finite(rect) { rect } else { full };

    let color = Color::lerp(look.closed_color, look.open_color, t.clamp(0.0, 1.0));
    let elevation = lerp(look.closed_elevation, look.open_elevation, t).max(0.0);
    let radius = BorderRadius::lerp(look.closed_radius, look.open_radius, t.clamp(0.0, 1.0));

    // Both layers are laid out at their own size and scaled to the surface's
    // width, so neither reflows while the surface grows.
    let closed_copy = (flight.closed)(ctx, &OpenAction::inert());
    let closed_layer = ExcludeSemantics::new().child(
        IgnorePointer::new().child(
            Opacity::new(look.transition.closed_opacity(t)).child(
                FittedBox::new()
                    .fit(BoxFit::FitWidth)
                    .alignment(Alignment::TOP_CENTER)
                    .child(SizedBox::new(closed.width(), closed.height()).child(closed_copy)),
            ),
        ),
    );
    let open_layer = Opacity::new(look.transition.open_opacity(t)).child(
        FittedBox::new()
            .fit(BoxFit::FitWidth)
            .alignment(Alignment::TOP_CENTER)
            .child(SizedBox::new(full.width(), full.height()).child(page.clone())),
    );

    let surface = Material::new(color)
        .elevation(elevation)
        .shape(MaterialShape::RoundedRect(radius))
        .clip_behavior(Clip::AntiAlias)
        .child(Stack::new(vec![
            Positioned::fill(closed_layer).into_view().boxed(),
            Positioned::fill(open_layer).into_view().boxed(),
        ]));

    Offstage::new()
        .offstage(measured.is_none())
        .child(Stack::new(vec![
            Positioned::new(surface)
                .left(rect.left())
                .top(rect.top())
                .width(rect.width())
                .height(rect.height())
                .into_view()
                .boxed(),
        ]))
        .into_view()
        .boxed()
}
