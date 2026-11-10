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
//! # One writer
//!
//! Outside layout a position moves only through its one
//! [`ScrollOffsetWriter`], which [`ScrollPosition::with_writer`] returns
//! beside it and which names the [`OffsetWriteCause`] of every write. A
//! [`ScrollPosition::fixed`] position has no writer at all. A clone of the
//! position reads and subscribes; it cannot scroll. The owner of the writer
//! (in `flui-widgets`, the `ScrollController` that made the position) hands
//! out requests and routes each through the nested scroll channel
//! (ADR-0209 §3). A write notifies synchronously, epsilon-guarded against
//! no-op writes, after its state is committed.

use std::fmt;
use std::rc::Rc;

use flui_foundation::panic::commit;
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback};
use flui_scheduler::PostFrameHandle;
use std::cell::RefCell;

use super::viewport_offset::{OffsetListeners, ScrollDirection, ViewportOffset};

/// The `ViewportOffset` fields a position tracks: the pixel offset plus the
/// viewport and content extents layout reports.
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
    /// pixels were seeded before layout (e.g. `ScrollPosition::with_writer`) as a
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
                // value (e.g. from `ScrollPosition::with_writer`) is left
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
        // Unordered or NaN extents bound no offset: they are refused, and the
        // committed range stays.
        let ordered = min_scroll_extent <= max_scroll_extent;
        if !ordered {
            tracing::warn!(
                min_scroll_extent,
                max_scroll_extent,
                "content extents that are unordered or NaN were refused"
            );
            return (false, true);
        }
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
///   seeded via [`ScrollPosition::with_writer`] is not reinterpreted as a page count
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
    /// `Listenable` sink — what `ScrollController::listenable()` and
    /// `AnimatedBuilder` subscribe to.
    notifier: ChangeNotifier,
    /// The `ViewportOffset` trait's own listeners, kept apart from `notifier`
    /// because they are removed by `Rc` identity, not by `ListenerId`.
    offset_listeners: OffsetListeners,
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
    /// mutation that must be observable — a [`ScrollOffsetWriter`] write and
    /// the coalesced post-frame flush — routes through here so there is
    /// exactly one place that decides who gets told.
    fn notify(&self) {
        let mut recovery = flui_foundation::panic::PanicRecovery::new();
        self.notify_with_recovery(&mut recovery.scope());
        recovery.finish();
    }

    fn notify_with_recovery(&self, recovery: &mut flui_foundation::panic::RecoveryScope<'_>) {
        self.notifier.notify_listeners_with_recovery(recovery);
        // Accepted pixel geometry must reach render invalidation even when a
        // widget listener failed. Both channels share the first failure and
        // the existing notifier's live-membership and retirement rules.
        self.offset_listeners
            .notifier()
            .notify_listeners_with_recovery(recovery);
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
    fn listen(&self, listener: ListenerCallback) -> flui_foundation::Subscription {
        self.notifier.listen(listener)
    }
    fn listen_with_recovery(
        &self,
        relay: flui_foundation::ListenerObserver,
    ) -> flui_foundation::Subscription {
        self.notifier.listen_with_recovery(relay)
    }
}

/// A shared, cheaply-cloneable scroll position: a [`ViewportOffset`]
/// implementation (consumed by `RenderViewport`) that is also
/// [`Listenable`] (consumed by `ScrollController`/`AnimatedBuilder`).
///
/// All clones point at the same underlying state via an `Rc`, so a
/// `RenderViewport` holding one clone observes the writes of the position's
/// [`ScrollOffsetWriter`] immediately — no separate push step is needed. A
/// clone reads and subscribes; only the writer scrolls.
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
    /// A [fixed](Self::fixed) position at zero.
    fn default() -> Self {
        Self::fixed(0.0)
    }
}

impl ScrollPosition {
    fn seeded(initial_pixels: f64) -> Self {
        Self {
            inner: Rc::new(Inner {
                state: RefCell::new(State {
                    pixels: initial_pixels,
                    ..State::zero()
                }),
                notifier: ChangeNotifier::new(),
                offset_listeners: OffsetListeners::default(),
                flush: RefCell::new(FlushState::new()),
                activity: RefCell::new(ActivityState::default()),
                activity_notifier: ChangeNotifier::new(),
            }),
        }
    }

    /// A position at `pixels` that nothing writes: only layout moves it,
    /// clamping it into the extents it reports and applying corrections.
    /// The offset of a viewport that does not scroll, or of a test.
    #[must_use]
    pub fn fixed(pixels: f64) -> Self {
        Self::seeded(pixels)
    }

    /// A position at `initial_pixels`, with no known extents and no flush
    /// handle installed, and its one [`ScrollOffsetWriter`]. Whoever holds
    /// the writer owns the position's motion.
    #[must_use]
    pub fn with_writer(initial_pixels: f64) -> (Self, ScrollOffsetWriter) {
        let position = Self::seeded(initial_pixels);
        let writer = ScrollOffsetWriter {
            inner: Rc::clone(&position.inner),
        };
        (position, writer)
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

    /// Installs the post-frame capability that lets `apply_viewport_dimension`/
    /// `apply_content_dimensions` flush a coalesced notification after
    /// layout instead of firing synchronously mid-frame. Acquire `handle`
    /// from `ViewState::init_state`/`did_change_dependencies` (ADR-0021) —
    /// never from `build`/`perform_layout`.
    pub fn set_flush_handle(&self, handle: PostFrameHandle) {
        self.inner.flush.borrow_mut().flush_handle = Some(handle);
    }

    // ========== Scroll activity ==========

    /// Whether a user drag or ballistic fling is currently underway.
    #[must_use]
    pub fn is_scrolling(&self) -> bool {
        self.inner.activity.borrow().is_scrolling
    }

    /// The direction of the user's current scroll, `Idle` when none is
    /// underway.
    #[must_use]
    pub fn user_scroll_direction(&self) -> super::ScrollDirection {
        self.inner.activity.borrow().user_scroll_direction
    }

    /// Subscribe to activity transitions (scrolling started/stopped, user
    /// direction changed). The pixel [`Listenable`] is deliberately a
    /// different sink — see the `activity` field's own doc.
    ///
    /// The subscription detaches on drop; `detach` for an early exit.
    pub fn add_activity_listener(
        &self,
        listener: flui_foundation::ListenerCallback,
    ) -> flui_foundation::Subscription {
        self.inner.activity_notifier.listen(listener)
    }

    /// Returns an `Rc<dyn Listenable>` sharing this position's notification channel.
    /// Each call retains the same channel identity without allocating a new notifier.
    #[must_use]
    pub fn listenable(&self) -> Rc<dyn Listenable> {
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
    /// (share one `Rc`-backed inner state), not merely equal by value. A
    /// widget reconciling an injected `ScrollPosition` against the one
    /// already installed on its render object uses this to decide whether a
    /// swap is actually a change — replacing a same-identity offset would
    /// discard layout-committed extents for no reason.
    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

/// Why a [`ScrollOffsetWriter`] moved its position.
///
/// The owner of the writer knows what drove each write, and says so: the
/// cause travels with the write into its diagnostics. The offset an owner
/// starts from is no write; it is the seed [`ScrollPosition::with_writer`]
/// takes. A layout-time correction or clamp is no write either: layout
/// applies it through [`ViewportOffset`] and notifies nobody.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OffsetWriteCause {
    /// A step of the user's own input: a drag, a wheel or trackpad packet,
    /// a key, an assistive scroll action.
    UserInput,
    /// Inertia the platform delivers after the user let go (trackpad
    /// momentum).
    Momentum,
    /// Motion the framework drives after a release or for the user: a
    /// fling, a ballistic or snap settle, a spring back into range, a drop
    /// autoscroll.
    Ballistic,
    /// An application request: a controller jump, a frame of its
    /// animation, a reveal, a resting place an owner picks when the motion
    /// that moved it is gone.
    Programmatic,
}

/// The one writer of a [`ScrollPosition`] made by
/// [`ScrollPosition::with_writer`].
///
/// It is not `Clone`, so a position has at most one, and not `Send`, as the
/// position it writes is not. Every pixel write names its
/// [`OffsetWriteCause`]. Each write commits the position's state before any
/// listener runs, so a listener reads the new value back and may write
/// again; a listener that panics does not stop the others, and the first
/// panic resumes once all of them ran (ADR-0287).
///
/// The methods take `&self`: the owner that holds the writer shares it
/// among its own handles, and a write that re-enters from a listener must
/// find no borrow of the writer held.
///
/// ```compile_fail
/// fn clone_it(writer: flui_rendering::view::ScrollOffsetWriter) {
///     let _second: flui_rendering::view::ScrollOffsetWriter = Clone::clone(&writer);
/// }
/// ```
///
/// ```compile_fail
/// fn send<T: Send>(_: T) {}
/// let (_position, writer) = flui_rendering::view::ScrollPosition::with_writer(0.0);
/// send(writer);
/// ```
///
/// A clone of the position cannot scroll:
///
/// ```compile_fail
/// let position = flui_rendering::view::ScrollPosition::fixed(0.0);
/// position.set_pixels(10.0);
/// ```
pub struct ScrollOffsetWriter {
    inner: Rc<Inner>,
}

impl fmt::Debug for ScrollOffsetWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pixels = self.inner.state.try_borrow().ok().map(|state| state.pixels);
        f.debug_struct("ScrollOffsetWriter")
            .field("pixels", &pixels)
            .finish_non_exhaustive()
    }
}

impl ScrollOffsetWriter {
    /// Move the offset to `pixels`, unclamped, for `cause`, and notify both
    /// listener channels when it changed (epsilon-guarded: a same-value
    /// write, or a value that is not finite, changes and notifies nothing).
    ///
    /// # Panics
    ///
    /// Resumes the first panic of a listener, after every listener ran and
    /// with the new offset committed.
    pub fn set_pixels(&self, pixels: f64, cause: OffsetWriteCause) {
        // Every scroll step writes here, and a step allocates nothing, so the
        // write commits by hand rather than through an `Outbox`: the borrow
        // closes, with the new offset in place, before any listener runs,
        // and nothing in it can panic.
        {
            let mut state = self.inner.state.borrow_mut();
            if !pixels.is_finite() || (state.pixels - pixels).abs() <= f64::EPSILON {
                return;
            }
            state.pixels = pixels;
        }
        self.inner.notify();
        // After the listeners: a subscriber is user code, and the state it
        // reads back is committed.
        tracing::trace!(pixels, ?cause, "scroll offset written");
    }

    /// Notify both listener channels once, now, whether or not anything
    /// changed: the owner changed what its listeners act on without moving
    /// the offset (new extents it applied itself, a request it queued). A
    /// coalesced post-frame flush already queued for the same change is
    /// consumed, so it does not notify a second time.
    ///
    /// # Panics
    ///
    /// Resumes the first panic of a listener, after every listener ran.
    pub fn notify(&self) {
        {
            let mut flush = self.inner.flush.borrow_mut();
            flush.metrics_dirty = false;
            flush.flush_pending = false;
        }
        self.inner.notify();
    }

    /// Record that scrolling started or stopped. Activity listeners hear the
    /// transition only: a same-value write is silent, so an owner may set
    /// it at each gesture edge. Stopping resets the user's scroll direction
    /// to `Idle`, since no drag or fling remains to have one.
    ///
    /// # Panics
    ///
    /// Resumes the first panic of an activity listener, after every one ran.
    pub fn set_is_scrolling(&self, is_scrolling: bool) {
        let inner = &self.inner;
        commit(&inner.activity, |activity, outbox| {
            if activity.is_scrolling == is_scrolling {
                return;
            }
            activity.is_scrolling = is_scrolling;
            if !is_scrolling {
                activity.user_scroll_direction = super::ScrollDirection::Idle;
            }
            let inner = Rc::clone(inner);
            outbox.call(move || inner.activity_notifier.notify_listeners());
        });
    }

    /// Record the direction of the user's scroll. Activity listeners hear a
    /// change only. A direction other than `Idle` is recorded only while
    /// scrolling is underway, so "`Idle` when none is underway" holds by
    /// construction.
    ///
    /// # Panics
    ///
    /// Resumes the first panic of an activity listener, after every one ran.
    pub fn set_user_scroll_direction(&self, direction: super::ScrollDirection) {
        let inner = &self.inner;
        commit(&inner.activity, |activity, outbox| {
            if !activity.is_scrolling && direction != super::ScrollDirection::Idle {
                return;
            }
            if activity.user_scroll_direction == direction {
                return;
            }
            activity.user_scroll_direction = direction;
            let inner = Rc::clone(inner);
            outbox.call(move || inner.activity_notifier.notify_listeners());
        });
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
        // listeners (ADR-0051: anchor correction is not a scroll).
        self.inner.state.borrow_mut().pixels += correction;
    }

    fn user_scroll_direction(&self) -> ScrollDirection {
        // The activity state IS the tracked direction — the render layer
        // (`RenderViewport::layout_child_sequence` reading this through
        // `dyn ViewportOffset`) must see the same answer the widget layer
        // sees, or direction-dependent sliver behavior (a floating header's
        // reveal) runs on a permanently-Idle constraint.
        Self::user_scroll_direction(self)
    }

    fn add_listener(&self, listener: Rc<dyn Fn()>) {
        self.inner.offset_listeners.add(listener);
    }

    fn remove_listener(&self, listener: &Rc<dyn Fn()>) {
        if let Some(subscription) = self.inner.offset_listeners.take(listener) {
            let mut recovery = flui_foundation::panic::PanicRecovery::new();
            self.inner.notifier.inherit_failure(&mut recovery.scope());
            self.inner
                .offset_listeners
                .notifier()
                .inherit_failure(&mut recovery.scope());
            recovery.retire(subscription);
            recovery.finish();
        }
    }
}

#[cfg(test)]
mod tests {

    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_scheduler::{OwnerFrame, UpdateScheduler};
    // Lower-crate test drives a frame through the composition-root seam
    // (ADR-0081 §4) until the raw-owner suites mount under the UI runtime's
    // root scopes (ADR-0083 6b).
    use flui_scheduler::__runtime::DriveFrame as _;

    use super::*;

    #[test]
    fn coalesces_multiple_apply_calls_into_one_flushed_notify() {
        let scheduler = UpdateScheduler::new();
        let handle = PostFrameHandle::new(&scheduler);

        let mut position = ScrollPosition::fixed(0.0);
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

        OwnerFrame::try_new(&scheduler)
            .expect("the scheduler has no live owner frame")
            .drive_frame(
                flui_scheduler::Instant::now(),
                flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
                || {},
                || {},
            )
            .expect("live owner frame");

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
