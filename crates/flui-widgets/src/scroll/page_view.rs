//! [`PageView`] — a scrollable list that works page by page, plus its
//! [`PageController`].
//!
//! # Composition
//!
//! `PageView` composes a [`Scrollable`] with a [`PageSnap`] policy and a
//! [`PageController`], `viewport_builder`-ing a [`Viewport`] over a single
//! [`SliverFillViewport`]: one sliver whose children each fill a
//! `viewport_fraction`-sized page, over a plain eager child list (see
//! [`PageView`]'s own docs). A drag along the axis, a press on a moving
//! pager and an assistive scroll all settle on a page through the policy.
//!
//! # Alternatives
//!
//! A swipe is a path: the constructor takes the pager's
//! [`GestureAlternatives`] (ADR-0216). Commands that name [`PageIntent`] are
//! answered by the pager, by turning one page; a pager whose owner keeps the
//! selection (a tab view) names an intent of its own and answers it above the
//! pager. The alternatives make the pager a row of focus traversal with one
//! custom action per command for assistive technology, the commands' keys
//! while focus is in the row, and a path for one pointer, so a finger, a
//! mouse, a dwell or a switch device turns a page without a swipe: a control
//! per command, or the companion widget (page dots, a tab bar, Previous and
//! Next buttons) that already draws them.
//!
//! # Keys
//!
//! While the pager holds the focus ([`PageView::autofocus`], or a drag on
//! it), an arrow along the axis turns one page (following the
//! reading direction on a horizontal axis), PageDown/PageUp and
//! Space/Shift+Space turn one page in reading order on either axis, and
//! Home/End go to the first and last page. A key that cannot move the pager
//! bubbles to the enclosing scrollable.
//!
//! # Deferred / not modelled (v1)
//!
//! - **Eager children.** `SliverFillViewport` (`flui-widgets`) has no lazy
//!   child delegate yet — every page attaches up front, with no on-demand
//!   construction.
//! - **`on_page_changed` is listener-based and runs after the frame**, not
//!   inside a scroll-notification listener — there is no scroll-notification
//!   bubbling yet. The controller's listener records a change when
//!   `round(page)` moves; the callback runs on the local post-frame lane with
//!   the `EventCx` its writes need, one frame after the change, every recorded
//!   page in order (ADR-0086; mapping decision 37 in
//!   `crates/flui-widgets/ARCHITECTURE.md`).
//! - **Non-snapping mode, `reverse`, end padding, implicit scrolling, page
//!   storage restoration, and `viewport_fraction > 1.0` centering** are not
//!   modeled — [`PageSnap`] is always applied (page snapping is the
//!   only supported mode) and [`DimensionChangePolicy::KeepFractionalPage`]
//!   already documents the `viewport_fraction > 1.0` gap it inherits.
//!   [`PageView::cache_extent`]'s default keeps no neighbouring pages laid out,
//!   the same as with implicit scrolling off — see that method's docs.
//! - **`PageController::animate_to_page`/`next_page`/`previous_page`**
//!   (ADR-0037) delegate to [`ScrollController::animate_to`] — see that
//!   type's module docs for the "no `Future`" limitation this inherits, and
//!   [`PageController::next_page`]/[`PageController::previous_page`]'s own
//!   docs for end-of-range behavior.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use flui_animation::{SharedCurve, curves};
use flui_foundation::geometry::Axis;
use flui_foundation::{ChangeNotifier, Listenable, ListenerId};
use flui_interaction::PointerKindSet;
use flui_rendering::view::{
    CacheExtentStyle, DimensionChangePolicy, ScrollPosition, ViewportOffset,
};
use flui_view::prelude::StatefulView;
use flui_view::seq::ViewSeq;
use flui_view::{
    BoxedView, BuildContext, EventCx, EventOutcome, IntoView, LifecycleContext, PostFrameHandle,
    RebuildHandle, RebuildReason, ViewExt, ViewState, WriterSource,
};

use crate::localization::{
    BoxedWidgetsLocalizations, DefaultWidgetsLocalizations, WidgetsLocalizations,
    axis_direction_from_axis_reverse_and_directionality,
};
use crate::scroll::scroll_owner::SharedSnapPolicy;
use crate::scroll::{
    PageSnap, ScrollController, ScrollMetrics, Scrollable, SliverFillViewport, Viewport,
    ViewportFraction,
};
use crate::support::{ValueCallback, value_callback};
use crate::{Action, ActionOutcome, Actions, GestureAlternatives, Intent, MediaQuery};

// ============================================================================
// PageController
// ============================================================================

/// Controls which page is visible in a [`PageView`].
///
/// `initial_page` and `viewport_fraction` are fixed at construction rather than
/// mutable builder fields; retargeting either afterwards is not supported.
#[derive(Clone, Debug)]
pub struct PageController {
    scroll: ScrollController,
    initial_page: usize,
    viewport_fraction: ViewportFraction,
}

impl Default for PageController {
    fn default() -> Self {
        Self::new()
    }
}

impl PageController {
    /// A controller starting at page `0`, each page as long as the viewport.
    #[must_use]
    pub fn new() -> Self {
        Self::with_params(0, ViewportFraction::FULL)
    }

    /// A controller starting at `initial_page`, with pages occupying
    /// `viewport_fraction` of the viewport.
    #[must_use]
    pub fn with_params(initial_page: usize, viewport_fraction: ViewportFraction) -> Self {
        let scroll = ScrollController::new();
        scroll
            .position()
            .set_dimension_policy(DimensionChangePolicy::KeepFractionalPage {
                viewport_fraction: viewport_fraction.get(),
                initial_page: Some(initial_page as f64),
            });
        Self {
            scroll,
            initial_page,
            viewport_fraction,
        }
    }

    /// The page shown when the controlled [`PageView`] is first laid out.
    #[must_use]
    pub fn initial_page(&self) -> usize {
        self.initial_page
    }

    /// The fraction of the viewport each page occupies.
    #[must_use]
    pub fn viewport_fraction(&self) -> f64 {
        self.viewport_fraction.get()
    }

    /// The current fractional page, or `None` before the controlled
    /// [`PageView`] has completed its first layout.
    ///
    /// Consults [`ScrollPosition::cached_page`] first (the collapsed-viewport
    /// case — a page tracked while the viewport reads `0.0`, which
    /// `pixels / viewport_dimension` could never recover), falling back to
    /// the guarded [`ScrollMetrics::page`] formula — not the internal
    /// recompute `apply_viewport_dimension` drives — only when not collapsed.
    /// A `ScrollPosition` always "has pixels", so
    /// [`ScrollPosition::has_applied_viewport_dimension`] is the "not yet
    /// answerable" signal.
    #[must_use]
    pub fn page(&self) -> Option<f64> {
        let position = self.scroll.position();
        if !position.has_applied_viewport_dimension() {
            return None;
        }
        if let Some(cached) = position.cached_page() {
            return Some(cached);
        }
        let metrics = ScrollMetrics::from(&position);
        Some(metrics.page(self.viewport_fraction()))
    }

    /// Jumps to `page` without animation.
    ///
    /// Three cases:
    /// - **Currently collapsed** (a real dimension was established at least
    ///   once, but the viewport currently reads `0.0`) — overwrites the
    ///   cached page directly, so a page jump requested while temporarily
    ///   hidden takes effect the moment the viewport regains a real dimension.
    /// - **Never established** — updates the pending startup page, same as
    ///   before any layout has run.
    /// - **Real, established dimension** — jumps directly, unclamped, without
    ///   checking whether the new value is in range.
    pub fn jump_to_page(&self, page: usize) {
        let page_f = page as f64;
        let mut position = self.scroll.position();
        if position.set_cached_page_while_collapsed(page_f) {
            return;
        }
        if position.has_applied_viewport_dimension() {
            let metrics = ScrollMetrics::from(&position);
            let pixels = metrics.pixels_from_page(self.viewport_fraction(), page_f);
            position.jump_to(pixels);
        } else {
            position.set_dimension_policy(DimensionChangePolicy::KeepFractionalPage {
                viewport_fraction: self.viewport_fraction(),
                initial_page: Some(page_f),
            });
        }
    }

    /// Animates to `page` over `duration`, easing through `curve` — page →
    /// pixels via the guarded [`ScrollMetrics::pixels_from_page`] formula,
    /// then delegates to [`ScrollController::animate_to`].
    ///
    /// The same three cases as [`jump_to_page`](Self::jump_to_page): a page requested while the
    /// viewport is collapsed just overwrites the cached page (there is no
    /// viewport for an animation to visibly run in), one requested before any
    /// layout has committed a real dimension updates the pending startup
    /// page, and only a real, established dimension actually starts a run.
    ///
    /// The target page is **not** bounds-checked against a page count —
    /// `PageController` has no visibility into how many children the
    /// `PageView` holds, and the page-to-pixels conversion never clamps
    /// `page` either. [`ScrollController::animate_to`]'s own clamp to
    /// `[min_scroll_extent, max_scroll_extent]` is what stops the run at the
    /// last/first real page instead of overshooting past it — see
    /// [`next_page`](Self::next_page)/[`previous_page`](Self::previous_page)'s
    /// docs for the resulting end-of-range behavior.
    pub fn animate_to_page(&self, page: usize, duration: Duration, curve: SharedCurve) {
        let page_f = page as f64;
        let position = self.scroll.position();
        if position.set_cached_page_while_collapsed(page_f) {
            return;
        }
        if position.has_applied_viewport_dimension() {
            let metrics = ScrollMetrics::from(&position);
            let pixels = metrics.pixels_from_page(self.viewport_fraction(), page_f);
            let _ = self.scroll.animate_to(pixels, duration, curve);
        } else {
            position.set_dimension_policy(DimensionChangePolicy::KeepFractionalPage {
                viewport_fraction: self.viewport_fraction(),
                initial_page: Some(page_f),
            });
        }
    }

    /// Animates forward to the next whole page from the current fractional
    /// page, rounded (`page().round() + 1`). A no-op before the first layout
    /// — there is no current [`page`](Self::page) to round from yet.
    ///
    /// The requested page is NOT clamped to a known last page:
    /// `PageController` doesn't track a page count. Past the last real page,
    /// [`animate_to_page`](Self::animate_to_page)'s delegated
    /// [`ScrollController::animate_to`] clamps the resulting pixel target to
    /// `max_scroll_extent`, so the run visibly stops AT the last page instead
    /// of scrolling past it.
    pub fn next_page(&self, duration: Duration, curve: SharedCurve) {
        let Some(page) = self.page() else { return };
        self.animate_to_page((page.round() + 1.0).max(0.0) as usize, duration, curve);
    }

    /// Animates backward to the previous whole page (`page().round() - 1`),
    /// saturating at page `0` rather than underflowing — the page index is
    /// `usize`; the saturated pixel target is the same as a negative page
    /// would give, since a negative page produces a negative pixel offset that
    /// [`animate_to_page`](Self::animate_to_page)'s delegated `animate_to`
    /// clamps to `min_scroll_extent` regardless. A no-op before the first
    /// layout.
    pub fn previous_page(&self, duration: Duration, curve: SharedCurve) {
        let Some(page) = self.page() else { return };
        self.animate_to_page((page.round() - 1.0).max(0.0) as usize, duration, curve);
    }

    /// The shared [`ScrollPosition`] backing this controller.
    #[must_use]
    pub fn position(&self) -> ScrollPosition {
        self.scroll.position()
    }

    /// The underlying [`ScrollController`], for wiring into a [`Scrollable`].
    #[must_use]
    pub fn scroll_controller(&self) -> ScrollController {
        self.scroll.clone()
    }

    /// An `Rc<dyn Listenable>` sharing this controller's position channel.
    #[must_use]
    pub fn listenable(&self) -> std::rc::Rc<dyn Listenable> {
        self.scroll.listenable()
    }
}

// ============================================================================
// PageIntent
// ============================================================================

/// How long a [`PageIntent`]'s page turn takes.
const TURN: Duration = Duration::from_millis(300);

/// Turn the enclosing [`PageView`] one page, in page order (the reading
/// order on a horizontal axis).
///
/// The pager answers it for everything inside it: the commands of its
/// [`GestureAlternatives`] name it. A turn animates over 300 ms, and lands at
/// once under reduced motion. At the first or the last page the turn past it
/// is not performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PageIntent {
    /// Show the page after the current one.
    Next,
    /// Show the page before the current one.
    Previous,
}

impl Intent for PageIntent {}

/// Answers [`PageIntent`] for everything inside a [`PageView`] of `pages`
/// pages.
struct PageAction {
    controller: PageController,
    pages: usize,
    /// Under reduced motion a turn lands at once.
    reduced_motion: bool,
    /// Notified when a turn becomes possible or stops being, which the
    /// controller's position changes without a rebuild.
    turns_change: ChangeNotifier,
}

/// The page `intent` turns to from the one `controller` shows among `pages`,
/// if there is one.
fn turn_target(controller: &PageController, pages: usize, intent: PageIntent) -> Option<usize> {
    let current = controller.page()?.round();
    if !current.is_finite() {
        return None;
    }
    let current = current.max(0.0) as usize;
    match intent {
        PageIntent::Next => current.checked_add(1).filter(|page| *page < pages),
        PageIntent::Previous => current.checked_sub(1),
    }
}

/// Whether a turn to the previous page, and to the next, is possible.
fn possible_turns(controller: &PageController, pages: usize) -> (bool, bool) {
    (
        turn_target(controller, pages, PageIntent::Previous).is_some(),
        turn_target(controller, pages, PageIntent::Next).is_some(),
    )
}

impl PageAction {
    /// The page `intent` turns to from the one showing, if there is one.
    fn target(&self, intent: PageIntent) -> Option<usize> {
        turn_target(&self.controller, self.pages, intent)
    }
}

impl Action<PageIntent> for PageAction {
    /// Past either end the intent is not enabled, so a key there bubbles on
    /// and assistive technology is not offered the turn.
    fn is_enabled(&self, intent: &PageIntent) -> bool {
        self.target(*intent).is_some()
    }

    fn enablement_changes(&self) -> Option<ChangeNotifier> {
        Some(self.turns_change.clone())
    }

    fn invoke(&self, _cx: &mut EventCx<'_>, intent: &PageIntent) -> ActionOutcome {
        let Some(page) = self.target(*intent) else {
            return ActionOutcome::NotPerformed;
        };
        if self.reduced_motion {
            self.controller.jump_to_page(page);
        } else {
            self.controller
                .animate_to_page(page, TURN, SharedCurve::new(curves::EASE_IN_OUT));
        }
        ActionOutcome::Performed
    }
}

// ============================================================================
// PageView (configuration)
// ============================================================================

/// A callback fired when the displayed page changes: the dispatch's
/// `EventCx` and the new page (ADR-0086).
type OnPageChanged = ValueCallback<usize>;

/// A scrollable list that works page by page.
///
/// Each child fills [`PageController::viewport_fraction`] of the viewport
/// along [`PageView::scroll_direction`]. `PageView` is pure composition over
/// [`Scrollable`] with a [`PageSnap`] policy + [`Viewport`] +
/// [`SliverFillViewport`] — no new render objects. A horizontal
/// `scroll_direction` resolves its `AxisDirection` from the ambient
/// [`Directionality`](crate::Directionality) (`RightToLeft` under an RTL
/// ancestor); the vertical axis never consults it.
///
/// See the module docs for the v1 limits (eager children, listener-based
/// `on_page_changed` delivered after the frame, no non-snapping mode, `reverse`
/// or end padding).
///
/// ```rust
/// use flui_widgets::{
///     Alternative, ControlSpec, GestureAlternatives, GestureCommand, PageIntent, PageView,
///     SizedBox, Text,
/// };
///
/// let alternatives = GestureAlternatives::new(Alternative::VisibleControl(
///     ControlSpec::new(
///         GestureCommand::new("Previous page", PageIntent::Previous),
///         Text::new("<"),
///     )
///     .control(GestureCommand::new("Next page", PageIntent::Next), Text::new(">")),
/// ))
/// .expect("two distinct labels");
/// let pager = PageView::new(
///     vec![SizedBox::expand(), SizedBox::expand(), SizedBox::expand()],
///     alternatives,
/// );
/// ```
#[derive(Clone, StatefulView)]
pub struct PageView {
    /// `None` when the caller never called [`PageView::controller`] — in
    /// that case [`PageViewState`] owns a default [`PageController`] created
    /// once in `create_state` and kept across rebuilds (see
    /// [`PageView::controller`]'s docs for why this can't just default-clone
    /// a fresh one on every build).
    controller: Option<PageController>,
    scroll_direction: Axis,
    on_page_changed: Option<OnPageChanged>,
    cache_extent: Option<(f64, CacheExtentStyle)>,
    allowed_devices: PointerKindSet,
    autofocus: bool,
    children: Vec<BoxedView>,
    alternatives: GestureAlternatives,
}

impl PageView {
    /// A horizontally-scrolling page view over `children`, its pages turned
    /// without a swipe through `alternatives`, whose commands name
    /// [`PageIntent`] or an intent the surroundings answer. By default [`PageView::cache_extent`] keeps no
    /// neighbouring pages laid out.
    pub fn new(children: impl ViewSeq, alternatives: GestureAlternatives) -> Self {
        Self {
            alternatives,
            controller: None,
            scroll_direction: Axis::Horizontal,
            on_page_changed: None,
            cache_extent: Some((0.0, CacheExtentStyle::Viewport)),
            allowed_devices: PointerKindSet::all(),
            autofocus: false,
            children: super::sliver_list::wrap_in_repaint_boundaries(children.into_boxed_vec()),
        }
    }

    /// Attach a [`PageController`] (position + page navigation). Multiple
    /// clones of the same controller share state.
    ///
    /// Omitting this entirely (the default) is NOT the same as calling it
    /// with a fresh [`PageController::new`] on every rebuild: `PageViewState`
    /// creates its own default controller exactly once (`create_state`) and
    /// keeps it — and the current page it's tracking — alive across rebuilds
    /// that don't pass an explicit controller. The default is only
    /// re-evaluated when the supplied controller itself changes, never
    /// unconditionally on every `build`.
    #[must_use]
    pub fn controller(mut self, controller: PageController) -> Self {
        self.controller = Some(controller);
        self
    }

    /// The scroll axis (default [`Axis::Horizontal`]).
    #[must_use]
    pub fn scroll_direction(mut self, axis: Axis) -> Self {
        self.scroll_direction = axis;
        self
    }

    /// Called whenever the page in the center of the viewport changes —
    /// when `round(page)` differs from the last reported page.
    ///
    /// The callback receives an `EventCx` and may write signals. It runs
    /// after the frame that next rebuilds this page view, never inside a
    /// build, with every recorded page delivered in order. A change observed
    /// during build or input (a drag, `jump_to_page`) is delivered after that
    /// same frame; one observed during layout (a viewport resize) is
    /// delivered after the following frame. See the module docs for why the
    /// report is not synchronous.
    #[must_use]
    pub fn on_page_changed<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, usize) -> R + 'static,
        R: EventOutcome,
    {
        self.on_page_changed = Some(value_callback(callback));
        self
    }

    /// Admit only drags and presses of these pointer kinds (default: all).
    /// See [`Scrollable::allowed_devices`].
    #[must_use]
    pub fn allowed_devices(mut self, devices: PointerKindSet) -> Self {
        self.allowed_devices = devices;
        self
    }

    /// Take the keyboard focus when mounted, so the page keys work without a
    /// click first (default `false`). See the module docs for the keys.
    #[must_use]
    pub fn autofocus(mut self, autofocus: bool) -> Self {
        self.autofocus = autofocus;
        self
    }

    /// Set how far beyond the visible page(s) to keep neighboring pages laid
    /// out and painted ([`Viewport::cache_extent`] passthrough).
    ///
    /// Defaults to `(0.0, CacheExtentStyle::Viewport)`: no neighbouring pages
    /// (implicit scrolling is not modelled). `Viewport`'s own render-object
    /// default (250px, `Pixel` style — `RenderViewport`'s general-purpose
    /// default, unrelated to `PageView`) would otherwise silently keep
    /// neighboring pages laid out and painted.
    #[must_use]
    pub fn cache_extent(mut self, cache_extent: f64, style: CacheExtentStyle) -> Self {
        self.cache_extent = Some((cache_extent, style));
        self
    }
}

impl std::fmt::Debug for PageView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageView")
            .field("controller", &self.controller)
            .field("scroll_direction", &self.scroll_direction)
            .field("has_on_page_changed", &self.on_page_changed.is_some())
            .field("cache_extent", &self.cache_extent)
            .field("allowed_devices", &self.allowed_devices)
            .field("children", &self.children.len())
            .field("alternatives", &self.alternatives)
            .finish_non_exhaustive()
    }
}

// ============================================================================
// State
// ============================================================================

/// Persistent state for [`PageView`].
///
/// Owns the default [`PageController`] when the view config carries none
/// (kept alive, current-page-and-all, across every rebuild that doesn't pass
/// an explicit one — see [`PageView::controller`]), and the `round(page)`-
/// change listener registered on the controller's position in
/// [`init_state`](ViewState::init_state) / re-registered on a controller swap
/// in [`did_update_view`](ViewState::did_update_view), removed in
/// [`dispose`](ViewState::dispose).
pub struct PageViewState {
    controller: PageController,
    /// The page policy for `controller`'s fraction. One `Arc` for the life of
    /// the controller: the scroll owner compares policies by identity, so a
    /// fresh one per build would replace the owner (and end its run) on every
    /// rebuild.
    snap: SharedSnapPolicy,
    /// Shared, mutable slot for the current callback. `did_update_view`
    /// writes it on every rebuild; delivery reads it when the post-frame
    /// callback runs, so a page recorded before a rebuild reaches the
    /// callback that rebuild installed.
    on_page_changed: Rc<RefCell<Option<OnPageChanged>>>,
    /// Whether a callback is installed. Read by the controller's
    /// listener, which shares only this flag and not the callback slot, so a page view
    /// nobody listens to records nothing and schedules no rebuild.
    has_callback: Rc<Cell<bool>>,
    /// Pages the listener recorded and `build` has not yet handed to the
    /// post-frame lane, oldest first. One entry per change, so FIFO order
    /// and multiplicity survive a frame that observes several.
    pending_pages: Rc<RefCell<VecDeque<usize>>>,
    /// Minted in `init_state`; the listener schedules the draining rebuild
    /// through it.
    rebuild: Option<RebuildHandle>,
    /// Minted in `init_state`; recorded pages are delivered through it.
    post_frame: Option<PostFrameHandle>,
    /// Owner-local delivery target. Queued post-frame callbacks hold only a
    /// [`Weak`] to it, so the lane cannot keep this state's callback or
    /// writer alive past teardown.
    delivery: Option<Rc<PageChangeDelivery>>,
    last_reported_page: Rc<Cell<i64>>,
    /// The listenable the listener is registered on, alongside its id.
    /// Stored together (not looked up fresh from `self.controller` at
    /// removal time) because a controller SWAP changes which listenable
    /// `self.controller.listenable()` resolves to — each notifier keeps
    /// its own `ListenerId` counter, so removing by id from a DIFFERENT
    /// notifier than the one that issued it can collide with an unrelated
    /// listener there (silently removing the wrong one and leaking the
    /// real one). Same shape `AnimatedBehavior::on_view_updated`
    /// (`crates/flui-view/src/element/behavior.rs`) uses for a `Listenable`
    /// swap.
    page_listener: Option<(std::rc::Rc<dyn Listenable>, ListenerId)>,
    /// What the page action tells the semantics layer when a turn becomes
    /// possible or stops being: its enablement follows the controller's
    /// position, which changes with no rebuild.
    turns_change: ChangeNotifier,
    /// How many pages the last build showed, and which turns the listener
    /// last found possible, so it notifies only when that pair flips.
    page_count: Rc<Cell<usize>>,
    last_turns: Rc<Cell<(bool, bool)>>,
}

impl std::fmt::Debug for PageViewState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageViewState")
            .field("controller", &self.controller)
            .field("has_on_page_changed", &self.has_callback.get())
            .field("last_reported_page", &self.last_reported_page.get())
            .finish_non_exhaustive()
    }
}

/// The owner-local resources that deliver one recorded page change.
///
/// The state owns the only strong reference; a post-frame callback holds a
/// [`Weak`] one. `finalize_tree` drops an unmounted state before the frame's
/// post-frame lane runs, so a page view unmounted after `build` queued a page
/// fails the upgrade: it delivers nothing, and its callback's captures are
/// released with the state rather than by the lane.
struct PageChangeDelivery {
    callback: Rc<RefCell<Option<OnPageChanged>>>,
    writer: WriterSource,
}

impl PageChangeDelivery {
    fn deliver(&self, page: usize) {
        // Cloned out: the borrow ends before the callback runs, so a callback
        // that rebuilds this page view cannot collide with it.
        let callback = self.callback.borrow().clone();
        if let Some(callback) = callback {
            self.writer.write(|cx| callback(cx, page));
        }
    }
}

impl PageViewState {
    /// Hand every page the listener recorded since the last `build` to the
    /// local post-frame lane, one lane entry per page. Called at the top of
    /// `build`, which is where the recording listener's rebuild lands.
    ///
    /// One entry per page, not one for the batch: a callback that panics
    /// takes only its own entry with it, and the pages after it still run
    /// in order. A context with no local post-frame lane, or a closed one,
    /// has no moment after the frame to offer; running the callback here
    /// would run it inside `build`, where its writes are refused, so the
    /// pages are dropped with a warning instead.
    fn drain_page_changes(&self) {
        let mut pending = std::mem::take(&mut *self.pending_pages.borrow_mut());
        if pending.is_empty() {
            return;
        }
        let Some(handle) = self.post_frame.as_ref() else {
            tracing::warn!(
                count = pending.len(),
                "PageView: dropping page changes — the context has no local post-frame \
                 lane, and running on_page_changed now would run it inside build"
            );
            return;
        };
        let delivery = self
            .delivery
            .as_ref()
            .expect("BUG: init_state creates the delivery target before the first build");
        while let Some(page) = pending.pop_front() {
            let delivery: Weak<PageChangeDelivery> = Rc::downgrade(delivery);
            if let Err(error) = handle.schedule(move |_timing| {
                if let Some(delivery) = delivery.upgrade() {
                    delivery.deliver(page);
                }
            }) {
                tracing::warn!(
                    ?error,
                    dropped = pending.len() + 1,
                    "PageView: dropping page changes — the owning lane is gone"
                );
                break;
            }
        }
    }

    /// Subscribes to `self.controller`'s current listenable, tracking
    /// `round(page)` changes. A change is recorded, and a rebuild scheduled
    /// to deliver it; the listener never runs user code. Stores the
    /// listenable alongside the returned id so [`dispose`](ViewState::dispose)
    /// and a later controller swap in
    /// [`did_update_view`](ViewState::did_update_view) remove from the exact
    /// listenable this registered on.
    fn register_page_listener(&mut self) {
        let position = self.controller.position();
        let viewport_fraction = self.controller.viewport_fraction();
        let last_reported = Rc::clone(&self.last_reported_page);
        let has_callback = Rc::clone(&self.has_callback);
        let controller = self.controller.clone();
        let page_count = Rc::clone(&self.page_count);
        let last_turns = Rc::clone(&self.last_turns);
        let turns_change = self.turns_change.clone();
        let pending = Rc::clone(&self.pending_pages);
        let rebuild = self
            .rebuild
            .clone()
            .expect("BUG: init_state mints the rebuild handle before registering the listener");

        let listenable = self.controller.listenable();
        let listener_id = listenable.add_listener(std::rc::Rc::new(move || {
            if !position.has_applied_viewport_dimension() {
                return;
            }
            // Before anything else: the turns an action offers follow the
            // position whether or not a page was crossed or anyone listens.
            let turns = possible_turns(&controller, page_count.get());
            if last_turns.replace(turns) != turns {
                turns_change.notify_listeners();
            }
            let metrics = ScrollMetrics::from(&position);
            // The guarded `page` formula clamps its numerator to >= 0.0
            // before dividing, so `round()` never yields a negative value —
            // the `.max(0.0)` here is belt-and-suspenders against a
            // negative-zero rounding artifact, not a real overflow guard.
            let current_page = metrics.page(viewport_fraction).round().max(0.0) as i64;
            if current_page != last_reported.get() {
                last_reported.set(current_page);
                if has_callback.get() {
                    pending.borrow_mut().push_back(current_page as usize);
                }
                // The page label assistive technology reads follows the page.
                rebuild.schedule(RebuildReason::StateChange);
            }
        }));
        self.page_listener = Some((listenable, listener_id));
    }
}

/// The page policy for pages of `controller`'s fraction of the viewport.
fn page_snap(controller: &PageController) -> SharedSnapPolicy {
    Arc::new(PageSnap::new(controller.viewport_fraction))
}

impl StatefulView for PageView {
    type State = PageViewState;

    fn create_state(&self) -> Self::State {
        let controller = self.controller.clone().unwrap_or_default();
        PageViewState {
            last_reported_page: Rc::new(Cell::new(controller.initial_page() as i64)),
            snap: page_snap(&controller),
            controller,
            on_page_changed: Rc::new(RefCell::new(self.on_page_changed.clone())),
            has_callback: Rc::new(Cell::new(self.on_page_changed.is_some())),
            pending_pages: Rc::new(RefCell::new(VecDeque::new())),
            rebuild: None,
            post_frame: None,
            delivery: None,
            page_listener: None,
            turns_change: ChangeNotifier::new(),
            page_count: Rc::new(Cell::new(self.children.len())),
            last_turns: Rc::new(Cell::new((false, false))),
        }
    }
}

impl ViewState<PageView> for PageViewState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.delivery = Some(Rc::new(PageChangeDelivery {
            callback: Rc::clone(&self.on_page_changed),
            writer: ctx.writer_source(),
        }));
        self.rebuild = Some(ctx.rebuild_handle());
        self.post_frame = ctx.post_frame_handle();
        // Last: the listener captures the rebuild handle minted above.
        self.register_page_listener();
    }

    fn build(&self, view: &PageView, ctx: &dyn BuildContext) -> impl IntoView {
        self.drain_page_changes();
        let controller = self.controller.clone();
        let viewport_fraction = controller.viewport_fraction();
        let scroll_direction = view.scroll_direction;
        // `reverse` isn't modeled on `PageView` yet (see the module docs'
        // deferred-divergences list), so it's always `false` here — the
        // resolution still consults ambient `Directionality` for a
        // horizontal `scroll_direction`.
        let axis_direction =
            axis_direction_from_axis_reverse_and_directionality(ctx, scroll_direction, false);
        let children = view.children.clone();
        let cache_extent = view.cache_extent;
        let page_count = view.children.len();
        self.page_count.set(page_count);
        let mut scrollable = Scrollable::new()
            .controller(controller.scroll_controller())
            .snap_policy(Arc::clone(&self.snap))
            .allowed_devices(view.allowed_devices)
            .autofocus(view.autofocus)
            .scroll_direction(scroll_direction);
        if page_count > 0 {
            let page = usize::try_from(self.last_reported_page.get())
                .unwrap_or(0)
                .min(page_count - 1);
            let label = match BoxedWidgetsLocalizations::maybe_of(ctx) {
                Some(localizations) => localizations.page_label(page + 1, page_count),
                None => DefaultWidgetsLocalizations.page_label(page + 1, page_count),
            };
            scrollable = scrollable.semantic_label(label);
        }
        let scrollable = scrollable
            // The inner `Viewport` below is built with this SAME
            // `axis_direction`. Handing it to `Scrollable` explicitly (rather
            // than relying on `Scrollable`'s own ambient-`Directionality`
            // fallback, which currently resolves to the identical value here
            // — same helper, same `ctx`-visible `Directionality` ancestor,
            // same `reverse: false`, verified by deleting this line: every
            // test still passes) keeps this composition correct BY
            // CONSTRUCTION rather than by that coincidence: `PageView`'s own
            // module docs list `reverse` as a deferred feature, and once
            // added, this explicit value (not the fallback's hardcoded
            // `reverse: false`) is what must reach `Scrollable`'s gesture
            // code. `Scrollable::axis_direction`'s docs cover the general
            // case: any `.viewport_builder` composing content whose
            // `AxisDirection` doesn't reduce to "ambient `Directionality` +
            // this `scroll_direction` + `reverse: false`" needs this to stay
            // correct.
            .axis_direction(axis_direction)
            .viewport_builder(Rc::new(move |position: ScrollPosition| {
                let sliver = SliverFillViewport::new(viewport_fraction, children.clone());
                let mut viewport = Viewport::new((sliver,))
                    .axis_direction(axis_direction)
                    .position(position);
                if let Some((extent, style)) = cache_extent {
                    viewport = viewport.cache_extent(extent, style);
                }
                viewport.boxed()
            }));
        Actions::new(view.alternatives.clone().child(scrollable))
            .action(PageAction {
                controller,
                pages: page_count,
                turns_change: self.turns_change.clone(),
                reduced_motion: MediaQuery::motion_of(ctx)
                    == Some(flui_animation::MotionPolicy::Reduce),
            })
            .boxed()
    }

    fn did_update_view(&mut self, _old_view: &PageView, new_view: &PageView) {
        // The callback lives behind the shared slot delivery reads when it
        // runs (see `on_page_changed`'s docs) — no listener re-registration
        // needed for a callback-only change, including a `None` -> `Some`
        // transition. Pages already recorded, from this controller or one
        // swapped out below, are still delivered: they happened.
        self.on_page_changed
            .borrow_mut()
            .clone_from(&new_view.on_page_changed);
        self.has_callback.set(new_view.on_page_changed.is_some());

        // No explicit controller in this build: keep the state-owned
        // default (and its live subscription) across the rebuild — see
        // `PageView::controller`'s docs for why this must NOT unconditionally
        // adopt a fresh default every build.
        let Some(new_controller) = &new_view.controller else {
            return;
        };

        // Detect an actual controller SWAP by listenable identity — mirrors
        // `AnimatedBehavior::on_view_updated`'s `Rc::ptr_eq` guard
        // (`crates/flui-view/src/element/behavior.rs`): a rebuild that hands
        // back the SAME controller (by far the common case when one IS
        // explicitly supplied) must not tear down and rebuild the
        // subscription.
        let old_listenable = self.controller.listenable();
        let new_listenable = new_controller.listenable();
        let controller_swapped = !std::rc::Rc::ptr_eq(&old_listenable, &new_listenable);

        self.controller = new_controller.clone();

        if controller_swapped {
            self.snap = page_snap(&self.controller);
            // Remove from the OLD listenable specifically — never from
            // whatever `self.controller` resolves to AFTER this assignment
            // (a different notifier, whose own `ListenerId` counter could
            // collide with this one and remove an unrelated listener while
            // leaking this one).
            if let Some((listenable, id)) = self.page_listener.take() {
                listenable.remove_listener(id);
            }
            self.register_page_listener();
        }
    }

    fn dispose(&mut self) {
        if let Some((listenable, id)) = self.page_listener.take() {
            listenable.remove_listener(id);
        }
    }
}
