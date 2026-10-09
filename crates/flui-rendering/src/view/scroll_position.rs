//! `ScrollPosition` — a shared, cheaply-cloneable [`ViewportOffset`] that is
//! also a [`Listenable`], so a gesture handler and `RenderViewport` can share
//! one scroll state and a `ScrollController`/`AnimatedBuilder` can subscribe
//! to it directly.
//!
//! # Content-dimension feedback
//!
//! [`RenderViewport::perform_layout`](super::RenderAbstractViewport)-style
//! callers report committed extents through [`ViewportOffset::apply_viewport_dimension`]
//! and [`ViewportOffset::apply_content_dimensions`]. Those two methods write
//! straight into the shared state and schedule (at most) one coalesced
//! post-frame flush — they never notify synchronously, because firing
//! listeners from inside layout can re-enter `build` while a frame is still
//! running. The flush is installed by [`ScrollPosition::set_flush_handle`]
//! (typically from `ViewState::init_state`, per ADR-0021); a position with no
//! flush handle installed (the bare, non-interactive `.offset(f64)` builder
//! path, and most unit tests) simply accumulates a dirty flag that is never
//! read — those positions have no external subscriber to notify.
//!
//! All mutation that a caller expects to observe synchronously — the
//! `ViewportOffset::jump_to`/`ScrollPosition::set_pixels` gesture path — still
//! notifies immediately, epsilon-guarded against no-op writes.

use std::fmt;
use std::rc::Rc;

use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use flui_scheduler::PostFrameHandle;
use std::cell::RefCell;

use super::viewport_offset::{ScrollDirection, ViewportOffset};

/// The `ViewportOffset` fields today's `ScrollableViewportOffset` tracks —
/// pixel position plus the viewport/content extents layout reports.
#[derive(Debug, Clone, Copy)]
struct State {
    pixels: f64,
    min_scroll_extent: f64,
    max_scroll_extent: f64,
    viewport_dimension: f64,
    /// How `apply_viewport_dimension` reconciles `pixels` across a dimension
    /// change. Lives inside `State` (not a separate field/mutex) so the
    /// policy read and the recompute it drives happen under the one lock
    /// acquisition `apply_viewport_dimension` already takes.
    dimension_policy: DimensionChangePolicy,
    /// Whether `apply_viewport_dimension` has ever committed a real
    /// dimension. Distinguishes "never laid out" from "laid out at literal
    /// `0.0`". The very first
    /// call must not run the `KeepFractionalPage` recompute: there is no
    /// prior dimension to divide by, so treating `viewport_dimension: 0.0`'s
    /// initial value as a real "old dimension" would reinterpret whatever
    /// pixels were seeded before layout (e.g. `ScrollPosition::new`) as a
    /// page count instead of a pixel offset.
    has_applied_viewport_dimension: bool,
    /// The fractional page cached by `KeepFractionalPage` whenever a resize
    /// collapses the viewport to a `0.0` dimension. While the dimension is `0.0`, `pixels`
    /// itself is set to `0.0` (there is no viewport to derive a pixel offset
    /// against), so the page must live in a separate field — one a
    /// concurrent `apply_content_dimensions` clamp on `pixels` cannot
    /// corrupt — until the viewport resizes back to a real dimension.
    cached_page: Option<f64>,
}

impl State {
    // Compare the admitted representation, not mathematical float equality:
    // an unchanged NaN must not masquerade as input arriving during layout.
    fn same_input(&self, other: &Self) -> bool {
        let policy_matches = match (self.dimension_policy, other.dimension_policy) {
            (DimensionChangePolicy::KeepPixels, DimensionChangePolicy::KeepPixels) => true,
            (
                DimensionChangePolicy::KeepFractionalPage {
                    viewport_fraction: a,
                    initial_page: ap,
                },
                DimensionChangePolicy::KeepFractionalPage {
                    viewport_fraction: b,
                    initial_page: bp,
                },
            ) => a.to_bits() == b.to_bits() && ap.map(f64::to_bits) == bp.map(f64::to_bits),
            _ => false,
        };
        policy_matches
            && [
                self.pixels.to_bits(),
                self.min_scroll_extent.to_bits(),
                self.max_scroll_extent.to_bits(),
                self.viewport_dimension.to_bits(),
            ] == [
                other.pixels.to_bits(),
                other.min_scroll_extent.to_bits(),
                other.max_scroll_extent.to_bits(),
                other.viewport_dimension.to_bits(),
            ]
            && self.has_applied_viewport_dimension == other.has_applied_viewport_dimension
            && self.cached_page.map(f64::to_bits) == other.cached_page.map(f64::to_bits)
    }

    const fn zero() -> Self {
        Self {
            pixels: 0.0,
            min_scroll_extent: 0.0,
            max_scroll_extent: 0.0,
            viewport_dimension: 0.0,
            dimension_policy: DimensionChangePolicy::KeepPixels,
            has_applied_viewport_dimension: false,
            cached_page: None,
        }
    }
}

impl State {
    fn apply_viewport_dimension(&mut self, viewport_dimension: f64) -> bool {
        // The equality short-circuit only applies once a REAL prior
        // dimension is on record (`has_applied_viewport_dimension`).
        // A first-ever call is NEVER treated as a no-op — even when
        // it happens to carry `0.0` (a `PageView` mounted inside a
        // currently-zero-size ancestor). Comparing raw `f64` values alone
        // conflates "never established" with "established at literal
        // `0.0`": a first call of exactly `0.0` would match
        // `State::zero()`'s own default and silently short-circuit, so
        // `has_applied_viewport_dimension` would never flip true and
        // `KeepFractionalPage`'s first-establishment branch would never
        // run for that call.
        if self.has_applied_viewport_dimension
            && (self.viewport_dimension - viewport_dimension).abs() < f64::EPSILON
        {
            false
        } else {
            // Pure recompute, also used by detached layout proposals. The
            // accepting owner schedules metrics delivery after publication.
            if let DimensionChangePolicy::KeepFractionalPage {
                viewport_fraction,
                initial_page,
            } = self.dimension_policy
            {
                debug_assert!(
                    viewport_fraction > 0.0,
                    "BUG: DimensionChangePolicy::KeepFractionalPage.viewport_fraction \
                     must be > 0.0"
                );
                // The three-way branch on the *old* dimension:
                // never established (null) is handled entirely by this
                // `if`'s absence below (see the comment after it); `0.0`
                // (a collapsed viewport) reads `cached_page` instead of
                // re-deriving it from `pixels`, which by then reads 0.0
                // and which a concurrent `apply_content_dimensions` clamp
                // could otherwise have stomped; anything else recomputes
                // via the pixels/dimension ratio.
                if self.has_applied_viewport_dimension {
                    let old_dimension = self.viewport_dimension;
                    let page = if old_dimension == 0.0 {
                        self.cached_page.unwrap_or(0.0)
                    } else {
                        // Numerator clamp: an overscrolled `pixels` below
                        // `min_scroll_extent` (allowed by
                        // `BouncingScrollPhysics`) must not encode as a
                        // negative page.
                        let raw_page = self.pixels.max(0.0) / (old_dimension * viewport_fraction);
                        round_snap_page(raw_page)
                    };

                    if viewport_dimension == 0.0 {
                        // Collapsing to zero: cache the page instead of
                        // encoding it in `pixels` — `pixels` becomes
                        // `0.0`, matching `getPixelsFromPage` evaluated at
                        // a zero dimension, and the real state survives
                        // in `cached_page` where a concurrent extent
                        // clamp cannot reach it.
                        self.cached_page = Some(page);
                        self.pixels = 0.0;
                    } else {
                        self.cached_page = None;
                        self.pixels = page * (viewport_dimension * viewport_fraction);
                    }
                } else if let Some(start_page) = initial_page {
                    // First-ever dimension establishment WITH a
                    // controller-driven startup page: mirrors
                    // `_pageToUseOnStartup`. Same zero-dimension collapse
                    // handling as the steady-state branch above — a
                    // `PageView` that never gets a real layout pass must
                    // still not divide by zero.
                    if viewport_dimension == 0.0 {
                        self.cached_page = Some(start_page);
                        self.pixels = 0.0;
                    } else {
                        self.cached_page = None;
                        self.pixels = start_page * (viewport_dimension * viewport_fraction);
                    }
                }
                // else: first-ever dimension establishment with no
                // controller-driven startup page (a bare `ScrollPosition`
                // opted into `KeepFractionalPage` directly). Behaves like
                // `KeepPixels` for this one call — the seeded `pixels`
                // value (e.g. from `ScrollPosition::new`) is left
                // untouched rather than reinterpreted as a page count
                // against a never-established prior dimension.
            }
            self.has_applied_viewport_dimension = true;
            self.viewport_dimension = viewport_dimension;
            true
        }
    }

    fn apply_content_dimensions(
        &mut self,
        min_scroll_extent: f64,
        max_scroll_extent: f64,
    ) -> (bool, bool) {
        if (self.min_scroll_extent - min_scroll_extent).abs() < f64::EPSILON
            && (self.max_scroll_extent - max_scroll_extent).abs() < f64::EPSILON
        {
            (false, true)
        } else {
            self.min_scroll_extent = min_scroll_extent;
            self.max_scroll_extent = max_scroll_extent;
            let clamped = self.pixels.clamp(min_scroll_extent, max_scroll_extent);
            if (self.pixels - clamped).abs() > f64::EPSILON {
                self.pixels = clamped;
                (true, false)
            } else {
                (true, true)
            }
        }
    }
}

/// Detached scroll-position layout proposal, including fractional-page state.
/// It owns no callbacks; acceptance compares the original state before publication.
#[derive(Debug)]
pub struct ScrollPositionLayout {
    original: State,
    proposed: State,
}

impl super::viewport_offset::ViewportLayout for ScrollPositionLayout {
    fn pixels(&self) -> f64 {
        self.proposed.pixels
    }
    fn apply_viewport_dimension(&mut self, dimension: f64) -> bool {
        let pixels = self.proposed.pixels.to_bits();
        self.proposed.apply_viewport_dimension(dimension);
        self.proposed.pixels.to_bits() == pixels
    }
    fn apply_content_dimensions(&mut self, min: f64, max: f64) -> bool {
        self.proposed.apply_content_dimensions(min, max).1
    }
    fn correct_by(&mut self, correction: f64) {
        self.proposed.pixels += correction;
    }
}

/// Floating-point tolerance for snapping a recomputed fractional page back to
/// the nearest whole page when the value "should" have landed exactly on one.
///
/// A `pixels / (dimension * fraction)` division should exactly reconstruct an
/// integral page when the pixels were originally seeded from `page *
/// dimension * fraction`, but float rounding leaves residue. The tolerance is
/// chosen against the scale of a page count (small integers, typically single
/// digits to low hundreds) rather than absolute machine epsilon.
const PAGE_ROUND_EPSILON: f64 = 1e-4;

/// Snaps `page` to the nearest whole page when within [`PAGE_ROUND_EPSILON`]
/// of one; otherwise returns it unchanged.
fn round_snap_page(page: f64) -> f64 {
    let rounded = page.round();
    if (page - rounded).abs() < PAGE_ROUND_EPSILON {
        rounded
    } else {
        page
    }
}

/// A one-lock-acquisition snapshot of a [`ScrollPosition`]'s four extent
/// fields (`pixels`, `min_scroll_extent`, `max_scroll_extent`,
/// `viewport_dimension`).
///
/// Exists so a caller in a higher crate can build a physics-facing metrics
/// value (e.g. `flui_widgets::scroll::ScrollMetrics`) without four separate
/// mutex acquisitions — one per field — which could observe a torn read if
/// another thread mutated the position between calls.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollPositionSnapshot {
    /// Current scroll offset in logical pixels.
    pub pixels: f64,
    /// The smallest in-range value for `pixels`.
    pub min_scroll_extent: f64,
    /// The largest in-range value for `pixels`.
    pub max_scroll_extent: f64,
    /// The viewport's length along the scroll axis.
    pub viewport_dimension: f64,
}

/// Controls how [`ScrollPosition::apply_viewport_dimension`] reconciles the
/// current pixel offset when the viewport's length along the scroll axis
/// changes.
///
/// # Page-preserving policy
///
/// `KeepFractionalPage` keeps the same logical page in view across a
/// viewport resize. It is a general policy on the plain `ScrollPosition`
/// (rather than bolted onto a `PageView`-only type) so any scrollable can opt
/// in. It branches three ways on the *old* dimension: never established uses
/// the startup page; collapsed to `0.0` reads the cached page; anything else
/// recomputes pixels from the page. See
/// [`ScrollPosition::apply_viewport_dimension`] for how each branch maps,
/// and the deferred pieces called out below.
///
/// # Deferred pieces
///
/// - **First-ever establishment.** The startup `page` comes from the page
///   controller's initial page. `KeepFractionalPage`'s `initial_page`
///   field carries that seed: `Some(page)` (what `PageController`,
///   `flui-widgets`, sets) seeds the first
///   `apply_viewport_dimension` call; `None` (a bare `ScrollPosition` opting
///   into this policy directly, with no controller) leaves `pixels`
///   untouched on that one call — the same as `KeepPixels` — so a value
///   seeded via [`ScrollPosition::new`] is not reinterpreted as a page count
///   against a never-established prior dimension. Restoring the page from
///   saved state is not modeled.
/// - **`viewport_fraction > 1.0`.** Centering each page within a
///   wider-than-one-page viewport (an initial offset of
///   `max(0, viewportDimension * (viewportFraction - 1) / 2)` applied to both
///   conversions) is not modeled
///   here — only `viewport_fraction <= 1.0` (many small pages per viewport,
///   or exactly one) is exercised; a `viewport_fraction > 1.0` caller gets
///   the un-centered formula until a real multi-page-viewport use case
///   lands.
/// - **No public "current page" accessor on `ScrollPosition` itself.**
///   A page accessor must be safe to call at any time (including before
///   content dimensions exist): a *defensively* guarded `max(0.0,
///   clamp(pixels, min, max)) / max(1.0, viewportDimension *
///   viewportFraction)`, plus the cached page when collapsed.
///   `flui-widgets`' `PageController::page`
///   is that accessor — it reads [`ScrollPosition::cached_page`] first,
///   falling back to the guarded formula (`ScrollMetrics::page`) only when
///   the viewport isn't currently collapsed.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum DimensionChangePolicy {
    /// Keep the pixel offset unchanged across a dimension change. Today's
    /// only behavior, and the default.
    #[default]
    KeepPixels,
    /// Keep the fractional "page" — `pixels / (viewport_dimension *
    /// viewport_fraction)` — unchanged, recomputing `pixels` for the new
    /// dimension. See the enum-level docs for exactly how the old-dimension
    /// null/zero/established three-way branch is handled, and the
    /// deferred pieces.
    ///
    /// `viewport_fraction` is the fraction of the viewport one logical page
    /// occupies (`1.0` = one page per viewport). Must be `> 0.0` —
    /// `flui-widgets`' `PageController` asserts this at its own construction, but
    /// `set_dimension_policy`'s caller is responsible when constructing this
    /// policy directly (checked with a `debug_assert!` in the recompute).
    KeepFractionalPage {
        /// Fraction of the viewport one logical page occupies. Must be
        /// `> 0.0`.
        viewport_fraction: f64,
        /// The page to seed `pixels` from on the very first
        /// `apply_viewport_dimension` call. `None` leaves `pixels`
        /// untouched on that one call instead (a caller with no
        /// controller-driven startup page) — see the enum-level docs.
        initial_page: Option<f64>,
    },
}

/// Coalescing bookkeeping for the post-frame extent flush.
struct FlushState {
    /// Set whenever `apply_viewport_dimension`/`apply_content_dimensions`
    /// commits a real change; cleared by the flush right before it notifies.
    metrics_dirty: bool,
    /// Installed via `set_flush_handle`. `None` until a `ViewState` acquires
    /// one in `init_state`/`did_change_dependencies` — see the module doc.
    flush_handle: Option<PostFrameHandle>,
    /// Whether a flush callback is already queued on `flush_handle`'s
    /// scheduler, so repeated `apply_*` calls within one layout pass
    /// schedule exactly one callback, not one per call.
    flush_pending: bool,
}

impl FlushState {
    const fn new() -> Self {
        Self {
            metrics_dirty: false,
            flush_handle: None,
            flush_pending: false,
        }
    }
}

/// The heap-allocated state shared by every clone of a [`ScrollPosition`].
struct Inner {
    state: RefCell<State>,
    /// `Listenable` sink — what `ScrollController::as_listenable()` and
    /// `AnimatedBuilder` subscribe to.
    notifier: ChangeNotifier,
    /// The `ViewportOffset` trait's own ptr-eq listener list, kept separate
    /// from `notifier` because it has a different identity contract
    /// (`Arc::ptr_eq` removal, not a `ListenerId`).
    offset_listeners: RefCell<Vec<(flui_foundation::ListenerId, std::rc::Weak<dyn Fn()>)>>,
    offset_notifier: ChangeNotifier,
    flush: RefCell<FlushState>,
    /// Scroll-activity state — whether a user drag or ballistic fling is
    /// underway, and which way the user last scrolled. Kept apart from
    /// `state` (which is pixel geometry) and notified through its own
    /// sink: activity subscribers (a floating header's snap trigger) must
    /// not be woken by every pixel change, and pixel subscribers must not
    /// be woken by drag start/stop.
    activity: RefCell<ActivityState>,
    /// Notified on every [`ActivityState`] transition — the is-scrolling
    /// and user-scroll-direction changes fused into one
    /// sink because every known consumer (snap) wants both.
    activity_notifier: ChangeNotifier,
}

#[derive(Default)]
struct ActivityState {
    is_scrolling: bool,
    user_scroll_direction: super::ScrollDirection,
}

impl Inner {
    /// The single fusion point that fires both notification sinks. Every
    /// mutation that must be observable — the gesture-driven `set_pixels`
    /// path and the coalesced post-frame flush — routes through here so
    /// there is exactly one place that decides who gets told.
    fn notify(&self) {
        let mut recovery = flui_foundation::panic::PanicRecovery::new();
        self.notifier.notify_listeners_with_recovery(&mut recovery);
        // Accepted pixel geometry must reach render invalidation even when a
        // widget listener failed. Both channels share the first failure and
        // the existing notifier's live-membership and retirement rules.
        self.offset_notifier
            .notify_listeners_with_recovery(&mut recovery);
        recovery.finish();
    }

    /// Mark the extents dirty and, if a flush handle is installed and no
    /// flush is already queued, schedule one coalesced post-frame callback
    /// that clears the flag and calls `notify()` exactly once.
    fn mark_metrics_dirty_and_maybe_schedule_flush(self: &Rc<Self>) {
        let mut flush = self.flush.borrow_mut();
        flush.metrics_dirty = true;
        if flush.flush_pending {
            return;
        }
        let Some(handle) = flush.flush_handle.clone() else {
            // No handle installed: the flag sits harmlessly. Bare
            // `.offset(f64)`-mode positions and most unit tests have no
            // external subscriber to notify, so there is nothing to flush.
            return;
        };
        flush.flush_pending = true;
        drop(flush);

        let inner = Rc::clone(self);
        if handle
            .schedule(move |_timing| {
                let was_dirty = {
                    let mut flush = inner.flush.borrow_mut();
                    flush.flush_pending = false;
                    std::mem::take(&mut flush.metrics_dirty)
                };
                if was_dirty {
                    inner.notify();
                }
            })
            .is_err()
        {
            self.flush.borrow_mut().flush_pending = false;
        }
    }
}

impl Listenable for Inner {
    fn add_listener(&self, listener: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(listener)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.notifier.remove_all_listeners();
    }
}

/// A shared, cheaply-cloneable scroll position: a [`ViewportOffset`]
/// implementation (consumed by `RenderViewport`) that is also
/// [`Listenable`] (consumed by `ScrollController`/`AnimatedBuilder`).
///
/// All clones point at the same underlying state via an `Arc`, so a gesture
/// handler holding one clone and a `RenderViewport` holding another observe
/// each other's writes immediately — no separate push step is needed once
/// both sides share the same `ScrollPosition`.
///
/// See the module docs for the content-dimension feedback contract.
#[derive(Clone)]
pub struct ScrollPosition {
    inner: Rc<Inner>,
}

impl fmt::Debug for ScrollPosition {
    // `try_lock` rather than `lock`: `Debug` is reachable from inside a
    // listener callback (e.g. a panic handler formatting state while
    // `notify()` is mid-iteration on another clone's stack), so this must
    // never block or recurse into the same mutex.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("ScrollPosition");
        match self.inner.state.try_borrow().ok() {
            Some(state) => {
                d.field("pixels", &state.pixels)
                    .field("min_scroll_extent", &state.min_scroll_extent)
                    .field("max_scroll_extent", &state.max_scroll_extent)
                    .field("viewport_dimension", &state.viewport_dimension)
                    .field("dimension_policy", &state.dimension_policy)
                    .field(
                        "has_applied_viewport_dimension",
                        &state.has_applied_viewport_dimension,
                    )
                    .field("cached_page", &state.cached_page);
            }
            None => {
                d.field("state", &"<locked>");
            }
        }
        d.finish_non_exhaustive()
    }
}

impl Default for ScrollPosition {
    fn default() -> Self {
        Self::zero()
    }
}

impl ScrollPosition {
    /// Creates a new scroll position at pixel offset `initial_pixels`, with
    /// no known extents and no flush handle installed.
    #[must_use]
    pub fn new(initial_pixels: f64) -> Self {
        Self {
            inner: Rc::new(Inner {
                state: RefCell::new(State {
                    pixels: initial_pixels,
                    ..State::zero()
                }),
                notifier: ChangeNotifier::new(),
                offset_listeners: RefCell::new(Vec::new()),
                offset_notifier: ChangeNotifier::new(),
                flush: RefCell::new(FlushState::new()),
                activity: RefCell::new(ActivityState::default()),
                activity_notifier: ChangeNotifier::new(),
            }),
        }
    }

    /// Creates a scroll position at zero.
    #[must_use]
    pub fn zero() -> Self {
        Self::new(0.0)
    }

    /// The smallest pixel value reachable without overscroll.
    #[must_use]
    pub fn min_scroll_extent(&self) -> f64 {
        self.inner.state.borrow_mut().min_scroll_extent
    }

    /// The largest pixel value reachable without overscroll.
    #[must_use]
    pub fn max_scroll_extent(&self) -> f64 {
        self.inner.state.borrow_mut().max_scroll_extent
    }

    /// The viewport's length along the scroll axis, as last committed by
    /// `apply_viewport_dimension`.
    #[must_use]
    pub fn viewport_dimension(&self) -> f64 {
        self.inner.state.borrow_mut().viewport_dimension
    }

    /// Whether `apply_viewport_dimension` has ever committed a real
    /// dimension. The "has this position ever been laid out" signal (see
    /// [`DimensionChangePolicy`]'s docs for why dimension-application, not
    /// pixel-nullity, plays that role here); `PageController::page`/
    /// `jump_to_page` (`flui-widgets`) consult this to decide whether a page
    /// request can be resolved to real pixels yet or must instead update the
    /// pending startup page.
    #[must_use]
    pub fn has_applied_viewport_dimension(&self) -> bool {
        self.inner.state.borrow_mut().has_applied_viewport_dimension
    }

    /// The page a [`DimensionChangePolicy::KeepFractionalPage`] policy is
    /// currently caching for a collapsed (`viewport_dimension == 0.0`)
    /// viewport, if any.
    ///
    /// `PageController::page` (`flui-widgets`) reads this first, falling back
    /// to the guarded `ScrollMetrics::page` formula only when it is `None`.
    /// Always `None` while not collapsed: every branch of
    /// `apply_viewport_dimension` that establishes a real (non-zero)
    /// dimension clears it in the same lock acquisition.
    #[must_use]
    pub fn cached_page(&self) -> Option<f64> {
        self.inner.state.borrow_mut().cached_page
    }

    /// Snapshots `pixels`, `min_scroll_extent`, `max_scroll_extent`, and
    /// `viewport_dimension` under a single lock acquisition. See
    /// [`ScrollPositionSnapshot`].
    #[must_use]
    pub fn extents_snapshot(&self) -> ScrollPositionSnapshot {
        let state = self.inner.state.borrow_mut();
        ScrollPositionSnapshot {
            pixels: state.pixels,
            min_scroll_extent: state.min_scroll_extent,
            max_scroll_extent: state.max_scroll_extent,
            viewport_dimension: state.viewport_dimension,
        }
    }

    /// Sets how a future `apply_viewport_dimension` call reconciles `pixels`
    /// when the viewport's dimension changes. See [`DimensionChangePolicy`].
    pub fn set_dimension_policy(&self, policy: DimensionChangePolicy) {
        self.inner.state.borrow_mut().dimension_policy = policy;
    }

    /// Overwrites the page a [`DimensionChangePolicy::KeepFractionalPage`]
    /// policy is currently holding cached for a collapsed
    /// (`viewport_dimension == 0.0`) viewport, without waiting for a real
    /// dimension to recompute `pixels` against. Returns `true` if the
    /// overwrite took effect.
    ///
    /// A no-op (returns `false`) when the policy isn't `KeepFractionalPage`,
    /// the viewport isn't currently collapsed, or — the guard that keeps this
    /// method's name honest — there is no page *already* cached to overwrite:
    /// this method replaces an existing cache entry, it never establishes one
    /// from nothing. In practice a genuinely collapsed `KeepFractionalPage`
    /// position always has one already (every branch of
    /// `apply_viewport_dimension` that lands on `viewport_dimension == 0.0`
    /// sets `cached_page` in that same call), but the check makes that an
    /// enforced precondition rather than an implicit fact a future edit to
    /// those branches could quietly break.
    ///
    /// # Why it exists
    ///
    /// A `PageView` resize sequence can pass through a collapsed viewport more than once
    /// (e.g. hidden inside a currently-zero-size ancestor, then a page jump
    /// request arrives, then the ancestor grows back) — [`set_dimension_policy`](Self::set_dimension_policy)
    /// alone only affects the *next* `apply_viewport_dimension` call's
    /// never-established branch, which does not run again once
    /// `has_applied_viewport_dimension` is already `true`; the steady-state
    /// branch that runs instead reads the private `cached_page`, not the
    /// policy's `initial_page`. `PageController::jump_to_page`
    /// (`flui-widgets`) calls this specifically for that already-collapsed
    /// case.
    #[must_use]
    pub fn set_cached_page_while_collapsed(&self, page: f64) -> bool {
        let mut state = self.inner.state.borrow_mut();
        if !matches!(
            state.dimension_policy,
            DimensionChangePolicy::KeepFractionalPage { .. }
        ) {
            return false;
        }
        if !state.has_applied_viewport_dimension || state.viewport_dimension != 0.0 {
            return false;
        }
        if state.cached_page.is_none() {
            // Genuinely collapsed under `KeepFractionalPage` but nothing was
            // ever cached — e.g. the policy was switched to
            // `KeepFractionalPage` only AFTER the position had already
            // collapsed under a different policy (`KeepPixels` never
            // touches `cached_page`). Replacing, not establishing: no-op.
            return false;
        }
        state.cached_page = Some(page);
        true
    }

    /// Sets the scroll offset to `value`, unclamped, and notifies listeners
    /// if the value actually changed (epsilon-guarded — a same-value write
    /// does not re-notify). This is the gesture/programmatic write path;
    /// `ScrollController::jump_to` clamps to the current extents before
    /// calling this.
    pub fn set_pixels(&self, value: f64) {
        let changed = {
            let mut state = self.inner.state.borrow_mut();
            if (state.pixels - value).abs() > f64::EPSILON {
                state.pixels = value;
                true
            } else {
                false
            }
        };
        if changed {
            self.notify();
        }
    }

    /// Installs the post-frame capability that lets `apply_viewport_dimension`/
    /// `apply_content_dimensions` flush a coalesced notification after
    /// layout instead of firing synchronously mid-frame. Acquire `handle`
    /// from `ViewState::init_state`/`did_change_dependencies` (ADR-0021) —
    /// never from `build`/`perform_layout`.
    pub fn set_flush_handle(&self, handle: PostFrameHandle) {
        self.inner.flush.borrow_mut().flush_handle = Some(handle);
    }

    /// Fires every registered listener (both the `Listenable` notifier and
    /// the `ViewportOffset` ptr-eq list) exactly once.
    ///
    /// Does NOT consume any pending coalesced-flush state — a caller that
    /// might have just triggered `apply_viewport_dimension`/
    /// `apply_content_dimensions` (and so may have a flush already queued)
    /// must use [`flush_now`](Self::flush_now) instead, or risk a second,
    /// redundant notification once that flush runs. This method is for
    /// callers that know no flush is in flight — e.g. the gesture/
    /// `set_pixels` path, which never schedules one.
    pub fn notify(&self) {
        self.inner.notify();
    }

    // ========== Scroll activity ==========

    /// Whether a user drag or ballistic fling is currently underway.
    #[must_use]
    pub fn is_scrolling(&self) -> bool {
        self.inner.activity.borrow_mut().is_scrolling
    }

    /// Records that scrolling started or stopped. Activity listeners fire on
    /// the TRANSITION only — a same-value write is silent, so a caller may
    /// set unconditionally at each gesture edge without spamming subscribers.
    ///
    /// Stopping also resets [`Self::user_scroll_direction`] to `Idle`,
    /// since no drag or fling remains to have a direction.
    pub fn set_is_scrolling(&self, is_scrolling: bool) {
        let mut recovery = flui_foundation::panic::PanicRecovery::new();
        self.set_is_scrolling_with_recovery(is_scrolling, &mut recovery);
        recovery.finish();
    }

    /// Publish activity within an enclosing framework delivery's failure custody.
    #[doc(hidden)]
    pub fn set_is_scrolling_with_recovery(
        &self,
        is_scrolling: bool,
        recovery: &mut flui_foundation::panic::PanicRecovery,
    ) {
        let changed = {
            let mut activity = self.inner.activity.borrow_mut();
            let changed = activity.is_scrolling != is_scrolling;
            activity.is_scrolling = is_scrolling;
            if changed && !is_scrolling {
                activity.user_scroll_direction = super::ScrollDirection::Idle;
            }
            changed
        };
        // Lock released before listeners run — a subscriber reading the
        // activity back must not deadlock.
        if changed {
            self.inner
                .activity_notifier
                .notify_listeners_with_recovery(recovery);
        }
    }

    /// The direction of the user's current scroll, `Idle` when none is
    /// underway.
    #[must_use]
    pub fn user_scroll_direction(&self) -> super::ScrollDirection {
        self.inner.activity.borrow_mut().user_scroll_direction
    }

    /// Records the user's scroll direction. Listeners fire on change only.
    ///
    /// A non-`Idle` direction is only recordable while
    /// [`Self::is_scrolling`] — outside an active scroll the write is
    /// ignored, keeping the documented invariant ("`Idle` when none is
    /// underway") true by construction instead of by caller discipline.
    pub fn set_user_scroll_direction(&self, direction: super::ScrollDirection) {
        let changed = {
            let mut activity = self.inner.activity.borrow_mut();
            if !activity.is_scrolling && direction != super::ScrollDirection::Idle {
                return;
            }
            let changed = activity.user_scroll_direction != direction;
            activity.user_scroll_direction = direction;
            changed
        };
        if changed {
            self.inner.activity_notifier.notify_listeners();
        }
    }

    /// Subscribe to activity transitions (scrolling started/stopped, user
    /// direction changed). The pixel [`Listenable`] is deliberately a
    /// different sink — see the `activity` field's own doc.
    pub fn add_activity_listener(
        &self,
        listener: flui_foundation::ListenerCallback,
    ) -> flui_foundation::ListenerId {
        use flui_foundation::Listenable as _;
        self.inner.activity_notifier.add_listener(listener)
    }

    /// Remove an activity subscription.
    pub fn remove_activity_listener(&self, id: flui_foundation::ListenerId) {
        use flui_foundation::Listenable as _;
        self.inner.activity_notifier.remove_listener(id);
    }

    /// Notifies exactly once, synchronously — consuming any coalesced-flush
    /// state first so a post-frame flush already queued by a preceding
    /// `apply_viewport_dimension`/`apply_content_dimensions` call becomes a
    /// no-op instead of firing a second, redundant notification for the
    /// same mutation.
    ///
    /// `ScrollController::update_dimensions` calls this (not
    /// [`notify`](Self::notify)) after applying dimensions outside a frame
    /// phase, precisely because those two `apply_*` calls may have just
    /// marked the position dirty and scheduled a flush.
    pub fn flush_now(&self) {
        {
            let mut flush = self.inner.flush.borrow_mut();
            flush.metrics_dirty = false;
            flush.flush_pending = false;
        }
        self.notify();
    }

    /// Returns an `Arc<dyn Listenable>` pointing at the same shared state —
    /// the upcast `ScrollController::as_listenable()` hands to `AnimatedBuilder`.
    #[must_use]
    pub fn as_listenable(&self) -> Rc<dyn Listenable> {
        Rc::clone(&self.inner) as Rc<dyn Listenable>
    }

    /// Whether any listeners are currently registered on this position's
    /// notifier.
    ///
    /// Disposal-testing hook, mirroring `TransformationController::has_listeners`
    /// (`interaction/transformation_controller.rs`) — a caller that mounts a
    /// widget against this position (e.g. a `PageView`'s `on_page_changed`
    /// subscription) and unmounts it, or swaps it for a different one, can
    /// assert this returns `false` afterward, proving the subscription was
    /// actually removed rather than left dangling into a torn-down subtree
    /// or leaked past a controller swap.
    #[must_use]
    pub fn has_listeners(&self) -> bool {
        self.inner.notifier.has_listeners()
    }

    /// The number of listeners currently registered on this position's
    /// notifier. See [`has_listeners`](Self::has_listeners).
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.notifier.len()
    }

    /// Whether this position's notifier currently has no listeners. See
    /// [`len`](Self::len).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether `self` and `other` are clones of the same underlying position
    /// (share one `Arc`-backed inner state), not merely equal by value. A
    /// widget reconciling an injected `ScrollPosition` against the one
    /// already installed on its render object uses this to decide whether a
    /// swap is actually a change — replacing a same-identity offset would
    /// discard layout-committed extents for no reason.
    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    /// Whether `self` is the only live handle to its underlying position —
    /// no clone of it is held anywhere else.
    ///
    /// A composer that owns a private position (creates it itself and never
    /// hands out a clone) can use this to detect that the position currently
    /// installed somewhere is instead a *foreign* one — e.g. `Viewport`
    /// switching from an injected, externally-shared position (Position
    /// mode) back to its own private one (Fixed mode) uses this to decide
    /// whether it's safe to keep writing into the installed position or
    /// must swap in a fresh, privately-owned one first.
    #[must_use]
    pub fn is_uniquely_held(&self) -> bool {
        Rc::strong_count(&self.inner) == 1
    }
}

impl ViewportOffset for ScrollPosition {
    type Layout = ScrollPositionLayout;

    fn begin_layout(&self) -> Self::Layout {
        let original = *self.inner.state.borrow();
        ScrollPositionLayout {
            original,
            proposed: original,
        }
    }

    fn accept_layout(&mut self, layout: Self::Layout) -> crate::RenderResult<()> {
        let changed = {
            let mut state = self.inner.state.borrow_mut();
            if !state.same_input(&layout.original) {
                return Err(crate::RenderError::ViewportOffsetChanged);
            }
            let changed = !state.same_input(&layout.proposed);
            *state = layout.proposed;
            changed
        };
        if changed {
            self.inner.mark_metrics_dirty_and_maybe_schedule_flush();
        }
        Ok(())
    }

    fn pixels(&self) -> f64 {
        self.inner.state.borrow_mut().pixels
    }

    fn has_pixels(&self) -> bool {
        true
    }

    fn apply_viewport_dimension(&mut self, viewport_dimension: f64) -> bool {
        let (changed, accepted) = {
            let mut state = self.inner.state.borrow_mut();
            let pixels = state.pixels.to_bits();
            let changed = state.apply_viewport_dimension(viewport_dimension);
            (changed, state.pixels.to_bits() == pixels)
        };
        if changed {
            self.inner.mark_metrics_dirty_and_maybe_schedule_flush();
        }
        accepted
    }

    fn apply_content_dimensions(&mut self, min_scroll_extent: f64, max_scroll_extent: f64) -> bool {
        let (changed, accepted) = self
            .inner
            .state
            .borrow_mut()
            .apply_content_dimensions(min_scroll_extent, max_scroll_extent);
        if changed {
            self.inner.mark_metrics_dirty_and_maybe_schedule_flush();
        }
        accepted
    }

    fn correct_by(&mut self, correction: f64) {
        // No notification: a layout-time correction must not fire
        // listeners (same contract as `ScrollableViewportOffset`).
        self.inner.state.borrow_mut().pixels += correction;
    }

    fn jump_to(&mut self, pixels: f64) {
        // Unclamped + epsilon-guarded — identical body to `set_pixels`, kept
        // as one call so there is a single source of truth for the guard.
        self.set_pixels(pixels);
    }

    fn animate_to(&mut self, to: f64, _duration_ms: u64) {
        // No animation support yet (v1 restriction, `ScrollController` docs);
        // synchronous jump is the documented fallback.
        self.jump_to(to);
    }

    fn user_scroll_direction(&self) -> ScrollDirection {
        // The activity state IS the tracked direction — the render layer
        // (`RenderViewport::layout_child_sequence` reading this through
        // `dyn ViewportOffset`) must see the same answer the widget layer
        // sees, or direction-dependent sliver behavior (a floating header's
        // reveal) runs on a permanently-Idle constraint.
        Self::user_scroll_direction(self)
    }

    fn allow_implicit_scrolling(&self) -> bool {
        true
    }

    fn add_listener(&self, listener: Rc<dyn Fn()>) {
        let identity = Rc::downgrade(&listener);
        let id = self.inner.offset_notifier.add_listener(listener);
        self.inner
            .offset_listeners
            .borrow_mut()
            .push((id, identity));
    }

    fn remove_listener(&self, listener: &Rc<dyn Fn()>) {
        let removed = {
            let mut listeners = self.inner.offset_listeners.borrow_mut();
            listeners
                .iter()
                .position(|(_, identity)| identity.ptr_eq(&Rc::downgrade(listener)))
                .map(|index| listeners.remove(index).0)
        };
        if let Some(id) = removed {
            let callback = self.inner.offset_notifier.take_listener(id);
            let mut recovery = flui_foundation::panic::PanicRecovery::new();
            self.inner.notifier.inherit_failure(&mut recovery);
            self.inner.offset_notifier.inherit_failure(&mut recovery);
            recovery.retire(callback);
            recovery.finish();
        }
    }
}

#[cfg(test)]
mod tests {

    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_scheduler::{OwnerFrame, UpdateScheduler};

    use super::*;

    #[test]
    fn coalesces_multiple_apply_calls_into_one_flushed_notify() {
        let scheduler = UpdateScheduler::new();
        let handle = PostFrameHandle::new(&scheduler);

        let mut position = ScrollPosition::zero();
        position.set_flush_handle(handle);

        let notified = Rc::new(AtomicUsize::new(0));
        let counter = Rc::clone(&notified);
        position.add_listener(Rc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));

        // Three separate layout-time calls before any frame completes.
        assert!(position.apply_viewport_dimension(300.0));
        assert!(position.apply_content_dimensions(0.0, 500.0));
        assert!(position.apply_content_dimensions(0.0, 600.0));

        assert_eq!(
            notified.load(Ordering::SeqCst),
            0,
            "the flush must not fire before the frame completes"
        );

        scheduler.execute_frame(
            &OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame"),
        );

        assert_eq!(
            notified.load(Ordering::SeqCst),
            1,
            "three apply_* calls in one layout pass must coalesce into exactly one flushed notify"
        );
    }

    // DimensionChangePolicy -----------------------------------------------

    // `initial_page` (`_pageToUseOnStartup` wiring) -------------------------

    // set_cached_page_while_collapsed -------------------------------------
}
