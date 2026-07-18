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
//! flush handle installed (the bare, non-interactive `.offset(f32)` builder
//! path, and most unit tests) simply accumulates a dirty flag that is never
//! read — those positions have no external subscriber to notify.
//!
//! All mutation that a caller expects to observe synchronously — the
//! `ViewportOffset::jump_to`/`ScrollPosition::set_pixels` gesture path — still
//! notifies immediately, epsilon-guarded against no-op writes.

use std::fmt;
use std::sync::Arc;

use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use flui_scheduler::PostFrameHandle;
use parking_lot::Mutex;

use super::viewport_offset::{ScrollDirection, ViewportOffset};

/// The `ViewportOffset` fields today's `ScrollableViewportOffset` tracks —
/// pixel position plus the viewport/content extents layout reports.
struct State {
    pixels: f32,
    min_scroll_extent: f32,
    max_scroll_extent: f32,
    viewport_dimension: f32,
    /// How `apply_viewport_dimension` reconciles `pixels` across a dimension
    /// change. Lives inside `State` (not a separate field/mutex) so the
    /// policy read and the recompute it drives happen under the one lock
    /// acquisition `apply_viewport_dimension` already takes.
    dimension_policy: DimensionChangePolicy,
}

impl State {
    const fn zero() -> Self {
        Self {
            pixels: 0.0,
            min_scroll_extent: 0.0,
            max_scroll_extent: 0.0,
            viewport_dimension: 0.0,
            dimension_policy: DimensionChangePolicy::KeepPixels,
        }
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
    pub pixels: f32,
    /// The smallest in-range value for `pixels`.
    pub min_scroll_extent: f32,
    /// The largest in-range value for `pixels`.
    pub max_scroll_extent: f32,
    /// The viewport's length along the scroll axis.
    pub viewport_dimension: f32,
}

/// Controls how [`ScrollPosition::apply_viewport_dimension`] reconciles the
/// current pixel offset when the viewport's length along the scroll axis
/// changes.
///
/// # Flutter parity
///
/// Mirrors the page-preserving recompute `_PagePosition.applyViewportDimension`
/// performs in `widgets/page_view.dart` (tag `3.44.0`) so the same logical
/// page stays in view across a viewport resize. Ported here as a general
/// policy on the plain `ScrollPosition` (rather than bolted onto a future
/// `PageView`-only type) so any scrollable can opt in.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum DimensionChangePolicy {
    /// Keep the pixel offset unchanged across a dimension change. Today's
    /// only behavior, and the default.
    #[default]
    KeepPixels,
    /// Keep the fractional "page" — `pixels / max(1.0, viewport_dimension *
    /// viewport_fraction)` — unchanged, recomputing `pixels` for the new
    /// dimension.
    ///
    /// `viewport_fraction` is the fraction of the viewport one logical page
    /// occupies (`1.0` = one page per viewport, matching Flutter's
    /// `PageController.viewportFraction` default).
    KeepFractionalPage {
        /// Fraction of the viewport one logical page occupies.
        viewport_fraction: f32,
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
    state: Mutex<State>,
    /// `Listenable` sink — what `ScrollController::as_listenable()` and
    /// `AnimatedBuilder` subscribe to.
    notifier: ChangeNotifier,
    /// The `ViewportOffset` trait's own ptr-eq listener list, kept separate
    /// from `notifier` because it has a different identity contract
    /// (`Arc::ptr_eq` removal, not a `ListenerId`).
    offset_listeners: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>>,
    flush: Mutex<FlushState>,
}

impl Inner {
    /// The single fusion point that fires both notification sinks. Every
    /// mutation that must be observable — the gesture-driven `set_pixels`
    /// path and the coalesced post-frame flush — routes through here so
    /// there is exactly one place that decides who gets told.
    fn notify(&self) {
        self.notifier.notify_listeners();
        // Snapshot-then-fire: a listener that calls `add_listener`/
        // `remove_listener` on this same position must not deadlock on
        // `offset_listeners`. This list has a real, load-bearing consumer:
        // `RenderViewport`/`RenderShrinkWrappingViewport` (`flui-objects`)
        // register their render-side relayout listener here in `attach`
        // (and re-register it on `set_offset` while attached). That listener
        // only sends a cross-thread `RepaintHandle::mark_needs_layout()`
        // request — it never calls back into `notify()`/`add_listener`/
        // `remove_listener` synchronously — so unlike `ScrollableViewportOffset`,
        // this list still doesn't need `ScrollableViewportOffset`-style
        // pending-pass bookkeeping for a REENTRANT notify pass; the snapshot
        // above is only guarding the add/remove-during-iteration case.
        let listeners = self.offset_listeners.lock().clone();
        for listener in &listeners {
            listener();
        }
    }

    /// Mark the extents dirty and, if a flush handle is installed and no
    /// flush is already queued, schedule one coalesced post-frame callback
    /// that clears the flag and calls `notify()` exactly once.
    fn mark_metrics_dirty_and_maybe_schedule_flush(self: &Arc<Self>) {
        let mut flush = self.flush.lock();
        flush.metrics_dirty = true;
        if flush.flush_pending {
            return;
        }
        let Some(handle) = flush.flush_handle.clone() else {
            // No handle installed: the flag sits harmlessly. Bare
            // `.offset(f32)`-mode positions and most unit tests have no
            // external subscriber to notify, so there is nothing to flush.
            return;
        };
        flush.flush_pending = true;
        drop(flush);

        let inner = Arc::clone(self);
        handle.schedule(move |_timing| {
            let was_dirty = {
                let mut flush = inner.flush.lock();
                flush.flush_pending = false;
                std::mem::take(&mut flush.metrics_dirty)
            };
            if was_dirty {
                inner.notify();
            }
        });
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
    inner: Arc<Inner>,
}

impl fmt::Debug for ScrollPosition {
    // `try_lock` rather than `lock`: `Debug` is reachable from inside a
    // listener callback (e.g. a panic handler formatting state while
    // `notify()` is mid-iteration on another clone's stack), so this must
    // never block or recurse into the same mutex.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("ScrollPosition");
        match self.inner.state.try_lock() {
            Some(state) => {
                d.field("pixels", &state.pixels)
                    .field("min_scroll_extent", &state.min_scroll_extent)
                    .field("max_scroll_extent", &state.max_scroll_extent)
                    .field("viewport_dimension", &state.viewport_dimension)
                    .field("dimension_policy", &state.dimension_policy);
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
    pub fn new(initial_pixels: f32) -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    pixels: initial_pixels,
                    ..State::zero()
                }),
                notifier: ChangeNotifier::new(),
                offset_listeners: Mutex::new(Vec::new()),
                flush: Mutex::new(FlushState::new()),
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
    pub fn min_scroll_extent(&self) -> f32 {
        self.inner.state.lock().min_scroll_extent
    }

    /// The largest pixel value reachable without overscroll.
    #[must_use]
    pub fn max_scroll_extent(&self) -> f32 {
        self.inner.state.lock().max_scroll_extent
    }

    /// The viewport's length along the scroll axis, as last committed by
    /// `apply_viewport_dimension`.
    #[must_use]
    pub fn viewport_dimension(&self) -> f32 {
        self.inner.state.lock().viewport_dimension
    }

    /// Snapshots `pixels`, `min_scroll_extent`, `max_scroll_extent`, and
    /// `viewport_dimension` under a single lock acquisition. See
    /// [`ScrollPositionSnapshot`].
    #[must_use]
    pub fn extents_snapshot(&self) -> ScrollPositionSnapshot {
        let state = self.inner.state.lock();
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
        self.inner.state.lock().dimension_policy = policy;
    }

    /// Sets the scroll offset to `value`, unclamped, and notifies listeners
    /// if the value actually changed (epsilon-guarded — a same-value write
    /// does not re-notify). This is the gesture/programmatic write path;
    /// `ScrollController::jump_to` clamps to the current extents before
    /// calling this.
    pub fn set_pixels(&self, value: f32) {
        let changed = {
            let mut state = self.inner.state.lock();
            if (state.pixels - value).abs() > f32::EPSILON {
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
    /// never from `build`/`perform_layout` (port-check trigger #22).
    pub fn set_flush_handle(&self, handle: PostFrameHandle) {
        self.inner.flush.lock().flush_handle = Some(handle);
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
            let mut flush = self.inner.flush.lock();
            flush.metrics_dirty = false;
            flush.flush_pending = false;
        }
        self.notify();
    }

    /// Returns an `Arc<dyn Listenable>` pointing at the same shared state —
    /// the upcast `ScrollController::as_listenable()` hands to `AnimatedBuilder`.
    #[must_use]
    pub fn as_listenable(&self) -> Arc<dyn Listenable> {
        Arc::clone(&self.inner) as Arc<dyn Listenable>
    }

    /// Whether `self` and `other` are clones of the same underlying position
    /// (share one `Arc`-backed inner state), not merely equal by value. A
    /// widget reconciling an injected `ScrollPosition` against the one
    /// already installed on its render object uses this to decide whether a
    /// swap is actually a change — replacing a same-identity offset would
    /// discard layout-committed extents for no reason.
    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Whether `self` is the only live handle to its underlying position —
    /// no clone of it is held anywhere else.
    ///
    /// A composer that owns a private position (creates it itself and never
    /// hands out a clone) can use this to detect that the position currently
    /// installed somewhere is instead a *foreign* one — e.g. `Viewport`
    /// switching from an injected, externally-shared position (Position
    /// mode) back to its own private one (Pixels mode) uses this to decide
    /// whether it's safe to keep writing into the installed position or
    /// must swap in a fresh, privately-owned one first.
    #[must_use]
    pub fn is_uniquely_held(&self) -> bool {
        Arc::strong_count(&self.inner) == 1
    }
}

impl ViewportOffset for ScrollPosition {
    fn pixels(&self) -> f32 {
        self.inner.state.lock().pixels
    }

    fn has_pixels(&self) -> bool {
        true
    }

    fn apply_viewport_dimension(&mut self, viewport_dimension: f32) -> bool {
        let changed = {
            let mut state = self.inner.state.lock();
            if (state.viewport_dimension - viewport_dimension).abs() < f32::EPSILON {
                false
            } else {
                // Flutter parity: `_PagePosition.applyViewportDimension`
                // (`widgets/page_view.dart`, tag `3.44.0`) recomputes the
                // pixel offset from the page fraction *before* committing the
                // new dimension, so the same logical page stays in view
                // across a resize. Pure recompute under the lock already
                // held here — no notify, no scheduling; the dirty-flag +
                // coalesced flush below carries the observable change.
                if let DimensionChangePolicy::KeepFractionalPage { viewport_fraction } =
                    state.dimension_policy
                {
                    let old_dimension = state.viewport_dimension;
                    let old_page = state.pixels / (old_dimension * viewport_fraction).max(1.0);
                    state.pixels = old_page * (viewport_dimension * viewport_fraction).max(1.0);
                }
                state.viewport_dimension = viewport_dimension;
                true
            }
        };
        if changed {
            self.inner.mark_metrics_dirty_and_maybe_schedule_flush();
        }
        true
    }

    fn apply_content_dimensions(&mut self, min_scroll_extent: f32, max_scroll_extent: f32) -> bool {
        let (changed, accepted) = {
            let mut state = self.inner.state.lock();
            if (state.min_scroll_extent - min_scroll_extent).abs() < f32::EPSILON
                && (state.max_scroll_extent - max_scroll_extent).abs() < f32::EPSILON
            {
                (false, true)
            } else {
                state.min_scroll_extent = min_scroll_extent;
                state.max_scroll_extent = max_scroll_extent;
                let clamped = state.pixels.clamp(min_scroll_extent, max_scroll_extent);
                if (state.pixels - clamped).abs() > f32::EPSILON {
                    state.pixels = clamped;
                    (true, false)
                } else {
                    (true, true)
                }
            }
        };
        if changed {
            self.inner.mark_metrics_dirty_and_maybe_schedule_flush();
        }
        accepted
    }

    fn correct_by(&mut self, correction: f32) {
        // No notification: a layout-time correction must not fire
        // listeners (same contract as `ScrollableViewportOffset`).
        self.inner.state.lock().pixels += correction;
    }

    fn jump_to(&mut self, pixels: f32) {
        // Unclamped + epsilon-guarded — identical body to `set_pixels`, kept
        // as one call so there is a single source of truth for the guard.
        self.set_pixels(pixels);
    }

    fn animate_to(&mut self, to: f32, _duration_ms: u64) {
        // No animation support yet (v1 restriction, `ScrollController` docs);
        // synchronous jump is the documented fallback.
        self.jump_to(to);
    }

    fn user_scroll_direction(&self) -> ScrollDirection {
        // Direction tracking is out of scope for this type (see module docs
        // of the feature this shipped with); `Idle` is the same default
        // `ScrollableViewportOffset` starts from and never updates without a
        // setter either.
        ScrollDirection::Idle
    }

    fn allow_implicit_scrolling(&self) -> bool {
        true
    }

    fn add_listener(&self, listener: Arc<dyn Fn() + Send + Sync>) {
        self.inner.offset_listeners.lock().push(listener);
    }

    fn remove_listener(&self, listener: &Arc<dyn Fn() + Send + Sync>) {
        let mut listeners = self.inner.offset_listeners.lock();
        if let Some(pos) = listeners.iter().position(|l| Arc::ptr_eq(l, listener)) {
            listeners.remove(pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_scheduler::Scheduler;

    use super::*;

    #[test]
    fn clone_shares_state() {
        let position = ScrollPosition::zero();
        let clone = position.clone();
        position.set_pixels(42.0);
        assert_eq!(
            clone.pixels(),
            42.0,
            "a clone must observe a write made through the original"
        );
    }

    #[test]
    fn ptr_eq_distinguishes_clones_from_independent_positions() {
        let position = ScrollPosition::zero();
        let clone = position.clone();
        let other = ScrollPosition::zero();

        assert!(position.ptr_eq(&clone), "clones must share identity");
        assert!(
            !position.ptr_eq(&other),
            "two independently constructed positions must not share identity"
        );
    }

    #[test]
    fn is_uniquely_held_reflects_whether_a_clone_is_alive_elsewhere() {
        let position = ScrollPosition::zero();
        assert!(
            position.is_uniquely_held(),
            "a freshly created position with no clones must be uniquely held"
        );

        let clone = position.clone();
        assert!(
            !position.is_uniquely_held(),
            "a position with a live clone elsewhere must not be uniquely held"
        );

        drop(clone);
        assert!(
            position.is_uniquely_held(),
            "dropping the only other clone must restore unique-held status"
        );
    }

    #[test]
    fn epsilon_guard_skips_notify_on_no_op_set_pixels() {
        let position = ScrollPosition::new(10.0);
        let notified = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&notified);
        position.add_listener(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));

        position.set_pixels(10.0); // same value: no-op
        assert_eq!(
            notified.load(Ordering::SeqCst),
            0,
            "writing the same pixel value must not notify"
        );

        position.set_pixels(20.0); // real change
        assert_eq!(
            notified.load(Ordering::SeqCst),
            1,
            "writing a different pixel value must notify exactly once"
        );
    }

    #[test]
    fn apply_viewport_and_content_dimensions_do_not_notify_synchronously() {
        let mut position = ScrollPosition::zero();
        let notified = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&notified);
        position.add_listener(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));

        // No flush handle installed: this is the bare `.offset(f32)`/unit-test
        // path from the module docs — extents commit, nothing ever notifies.
        assert!(position.apply_viewport_dimension(300.0));
        assert!(position.apply_content_dimensions(0.0, 500.0));

        assert_eq!(
            notified.load(Ordering::SeqCst),
            0,
            "apply_viewport_dimension/apply_content_dimensions must never notify synchronously"
        );
        assert_eq!(position.viewport_dimension(), 300.0);
        assert_eq!(position.max_scroll_extent(), 500.0);
    }

    #[test]
    fn coalesces_multiple_apply_calls_into_one_flushed_notify() {
        let scheduler = Scheduler::new();
        let handle = PostFrameHandle::new(&scheduler);

        let mut position = ScrollPosition::zero();
        position.set_flush_handle(handle);

        let notified = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&notified);
        position.add_listener(Arc::new(move || {
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

        scheduler.execute_frame();

        assert_eq!(
            notified.load(Ordering::SeqCst),
            1,
            "three apply_* calls in one layout pass must coalesce into exactly one flushed notify"
        );
    }

    #[test]
    fn flush_now_consumes_a_pending_flush_so_the_later_frame_does_not_double_notify() {
        let scheduler = Scheduler::new();
        let handle = PostFrameHandle::new(&scheduler);

        let mut position = ScrollPosition::zero();
        position.set_flush_handle(handle);

        let notified = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&notified);
        position.add_listener(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));

        // Mirrors `ScrollController::update_dimensions`: apply_* marks dirty
        // and queues a flush, then the caller notifies synchronously via
        // `flush_now` (not `notify`) so that queued flush becomes a no-op.
        assert!(position.apply_viewport_dimension(300.0));
        assert!(position.apply_content_dimensions(0.0, 500.0));
        position.flush_now();

        assert_eq!(
            notified.load(Ordering::SeqCst),
            1,
            "flush_now must notify exactly once, synchronously"
        );

        // The flush queued by apply_content_dimensions above is still
        // sitting on the scheduler (PostFrameHandle::schedule cannot be
        // cancelled) — running the frame must NOT produce a second
        // notification now that flush_now already consumed the dirty state.
        scheduler.execute_frame();

        assert_eq!(
            notified.load(Ordering::SeqCst),
            1,
            "a flush queued before flush_now must not double-notify once the frame completes"
        );
    }

    #[test]
    fn debug_under_lock_reports_locked_placeholder_instead_of_blocking() {
        let position = ScrollPosition::new(5.0);
        let guard = position
            .inner
            .state
            .try_lock()
            .expect("uncontended lock must succeed");

        let debug = format!("{position:?}");
        assert!(
            debug.contains("<locked>"),
            "Debug must report the locked placeholder while the state mutex is held, got: {debug}"
        );

        drop(guard);
        let debug = format!("{position:?}");
        assert!(
            debug.contains("pixels: 5.0"),
            "Debug must report real field values once the lock is free, got: {debug}"
        );
    }

    // DimensionChangePolicy -----------------------------------------------

    #[test]
    fn keep_pixels_default_policy_leaves_pixels_unchanged_across_a_dimension_change() {
        let mut position = ScrollPosition::zero();
        assert!(position.apply_viewport_dimension(300.0));
        position.set_pixels(150.0);

        assert!(position.apply_viewport_dimension(600.0));
        assert_eq!(
            position.pixels(),
            150.0,
            "KeepPixels (the default) must not move the pixel offset when the \
             viewport dimension changes"
        );
    }

    #[test]
    fn keep_fractional_page_preserves_page_at_full_viewport_fraction() {
        let mut position = ScrollPosition::zero();
        assert!(position.apply_viewport_dimension(300.0));
        position.set_pixels(600.0); // page = 600 / (300 * 1.0) = 2.0
        position.set_dimension_policy(DimensionChangePolicy::KeepFractionalPage {
            viewport_fraction: 1.0,
        });

        assert!(position.apply_viewport_dimension(600.0));
        // page preserved at 2.0: new_pixels = 2.0 * (600 * 1.0) = 1200.0
        assert_eq!(
            position.pixels(),
            1200.0,
            "resizing 300 -> 600 at viewport_fraction 1.0 must preserve the \
             fractional page (2.0), not the raw pixel offset"
        );
    }

    #[test]
    fn keep_fractional_page_preserves_page_at_partial_viewport_fraction() {
        let mut position = ScrollPosition::zero();
        assert!(position.apply_viewport_dimension(300.0));
        // page = 720 / (300 * 0.8) = 720 / 240 = 3.0
        position.set_pixels(720.0);
        position.set_dimension_policy(DimensionChangePolicy::KeepFractionalPage {
            viewport_fraction: 0.8,
        });

        assert!(position.apply_viewport_dimension(600.0));
        // page preserved at 3.0: new_pixels = 3.0 * (600 * 0.8) = 1440.0
        assert_eq!(
            position.pixels(),
            1440.0,
            "resizing 300 -> 600 at viewport_fraction 0.8 must preserve the \
             fractional page (3.0), not the raw pixel offset"
        );
    }

    #[test]
    fn keep_fractional_page_guards_against_zero_dimension_without_nan_or_inf() {
        let mut position = ScrollPosition::zero();
        assert!(position.apply_viewport_dimension(300.0));
        position.set_pixels(150.0); // page = 150 / 300 = 0.5
        position.set_dimension_policy(DimensionChangePolicy::KeepFractionalPage {
            viewport_fraction: 1.0,
        });

        // Collapse to a zero dimension: the `max(1.0, old_dim * fraction)`
        // divisor guard must prevent a divide-by-zero, not produce NaN/inf.
        assert!(position.apply_viewport_dimension(0.0));
        assert!(
            position.pixels().is_finite(),
            "collapsing to a zero viewport dimension must not produce NaN/inf pixels"
        );
        assert_eq!(
            position.pixels(),
            0.5,
            "page 0.5 recomputed against the guarded divisor (max(1.0, 0.0)) \
             lands at pixels = 0.5 * 1.0"
        );

        // Recover from zero: the `max(1.0, new_dim * fraction)` multiplier
        // guard must not produce NaN/inf either.
        assert!(position.apply_viewport_dimension(600.0));
        assert!(
            position.pixels().is_finite(),
            "recovering from a zero viewport dimension must not produce NaN/inf pixels"
        );
        assert_eq!(
            position.pixels(),
            300.0,
            "page 0.5 recomputed against the guarded multiplier (max(1.0, 600.0)) \
             lands at pixels = 0.5 * 600.0"
        );
    }

    #[test]
    fn keep_fractional_page_recompute_does_not_notify_synchronously() {
        let mut position = ScrollPosition::zero();
        assert!(position.apply_viewport_dimension(300.0));
        position.set_pixels(600.0);
        position.set_dimension_policy(DimensionChangePolicy::KeepFractionalPage {
            viewport_fraction: 1.0,
        });

        let notified = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&notified);
        position.add_listener(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));

        assert!(position.apply_viewport_dimension(600.0));
        assert_eq!(
            notified.load(Ordering::SeqCst),
            0,
            "the KeepFractionalPage recompute is a pure in-lock write — same \
             no-synchronous-notify contract as the KeepPixels path"
        );
    }
}
