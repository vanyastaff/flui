//! Renderer, vsync and frame clock; frame wake; first-frame deferral and frame accounting.

use super::UiRuntime;
use crate::presentation::PresentationState;
use crate::renderer_binding::RenderingBinding;
use flui_animation::Vsync;
use flui_foundation::PresentationId;
use flui_platform_api::HapticFeedback;
use flui_rendering::binding::RendererBinding as _;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

impl UiRuntime {
    // ========================================================================
    // Renderer, vsync, frame clock (moved from the retired `AppBinding`)
    // ========================================================================

    /// The render tree, layout/paint pipeline coordination, and semantics
    /// fan-out for this UI runtime's single presentation.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "production frame/input paths read self.renderer directly; \
                      this accessor exists for tests and future external callers"
        )
    )]
    pub(crate) fn renderer(&self) -> &RenderingBinding {
        self.presentations.primary().renderer()
    }

    /// A clone of the PRIMARY presentation's own controller registry for
    /// implicit animations.
    ///
    /// `Vsync` shares its owner-local registry through `Rc`. App
    /// code constructs a `VsyncScope` from this clone so every
    /// implicitly-animated widget below registers its controller here. The
    /// production frame driver (`Self::draw_frame_entered`) ticks EVERY
    /// presentation's own registry once per frame (before that
    /// presentation's build phase) and keeps the frame loop alive until the
    /// last running controller, on any presentation, completes.
    ///
    /// Moved from this UI runtime's own `vsync_slot` to per-presentation storage
    /// (each surface paces its own animations independently)
    /// — this forwarder to `self.presentations.primary()` keeps every
    /// existing single-presentation caller and test unchanged; a genuine
    /// multi-presentation caller reads
    /// `self.presentations.get(id).vsync()` directly instead.
    #[must_use]
    pub fn vsync(&self) -> Vsync {
        self.presentations.primary().vsync()
    }

    /// Replace the PRIMARY presentation's registry with a pre-existing
    /// shared `Vsync`.
    ///
    /// Use when a `VsyncScope` was built before this presentation's registry
    /// was acquired (the scope needs the handle to pass to descendants, and
    /// this presentation must drive that same registry). Call before any
    /// controller is registered so no registration is stranded on the
    /// discarded registry. Never mount a second `VsyncScope` at the root
    /// with a *different* registry — this presentation ticks its own while
    /// descendants register into the other, leaving them frozen.
    #[expect(
        dead_code,
        reason = "no production caller yet, and no test exercises the \
                  custom-registry substitution path -- an app-author escape \
                  hatch that has no wiring point: the ui_runtime is internal to its host"
    )]
    pub(crate) fn set_vsync(&self, vsync: Vsync) {
        self.presentations.primary().set_vsync(vsync);
    }

    /// The raw frame time, relative to this UI runtime's `start`, that every
    /// presentation's [`MotionClock`](flui_animation::MotionClock) maps to
    /// its animation tick.
    ///
    /// Checked in order:
    ///
    /// 1. the test override (`set_now_secs_for_test`, compiled only for
    ///    tests and `test-support`), for deterministic stepping through a
    ///    bare `draw_frame`/`render_frame`. An override that is not a valid
    ///    duration (NaN, ±∞, negative) reads as raw time zero, which every
    ///    clock past its first frame treats as stale and holds;
    /// 2. the timestamp of the frame [`Self::pump`] is running — the one
    ///    instant its [`FrameClockSource`](crate::pump::FrameClockSource)
    ///    returned, so `Vsync` controllers advance on the frame's clock, as
    ///    every ticker sees the frame's timestamp;
    /// 3. the UI runtime's clock source now, for a frame driven outside a pump
    ///    (the wall clock for a host's `ClockSource::Platform`).
    pub(super) fn raw_frame_time(&self) -> Duration {
        #[cfg(any(test, feature = "test-support"))]
        {
            let bits = self.now_secs_override.load(Ordering::Relaxed);
            if bits != 0 {
                return Duration::try_from_secs_f64(f64::from_bits(bits)).unwrap_or_default();
            }
        }
        match self.frame_time.get() {
            Some(frame_time) => frame_time.saturating_duration_since(self.start),
            None => flui_foundation::MonotonicClock::now(&self.clock)
                .saturating_duration_since(self.start),
        }
    }

    /// Inject a deterministic virtual `now_secs` for test frames: overrides
    /// the wall-clock read `raw_frame_time` otherwise takes, so a test can drive
    /// the Vsync tick and frame accounting from values it controls instead
    /// of racing real elapsed time. `0.0` is stored as a sentinel-adjusted
    /// nonzero bit pattern so `raw_frame_time`'s `bits != 0` check (its "no
    /// override installed" test) cannot mistake an explicit zero override
    /// for an absent one.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_now_secs_for_test(&self, secs: f64) {
        let bits = secs.to_bits();
        let stored = if bits == 0 { 1u64 } else { bits };
        self.now_secs_override.store(stored, Ordering::Relaxed);
    }

    /// Clear the test clock override, reverting to wall-clock time.
    #[cfg(test)]
    #[expect(
        dead_code,
        reason = "no test in this file's migrated set needs to revert to \
                  wall-clock mid-test; kept as the counterpart to \
                  set_now_secs_for_test for whichever future test needs to \
                  revert mid-run instead of dropping the whole ui_runtime"
    )]
    pub(crate) fn clear_now_secs_for_test(&self) {
        self.now_secs_override.store(0, Ordering::Relaxed);
    }

    // ========================================================================
    // Frame wake (moved from the retired `AppBinding`)
    // ========================================================================

    /// Request a redraw (flag only — does not poke the platform window).
    /// See [`Self::needs_redraw`]'s field doc for why this and
    /// [`Self::wake_frame`] share one underlying atomic with `AppRuntime`.
    ///
    /// Every current caller (`attach_root_widget*`, and every
    /// [`Self::handle_input_addressed`] arm not yet ported to it) is still a
    /// primary-only operation, so this marks `self.presentations.primary()`'s
    /// own pump wake bit (ADR-0043 §3) specifically — see
    /// `Self::request_redraw_for` for the addressed counterpart
    /// [`Self::handle_input_addressed`] actually uses.
    pub fn request_redraw(&self) {
        self.request_redraw_for(self.presentations.primary());
    }

    /// [`Self::request_redraw`], addressed to exactly `presentation` rather
    /// than always the primary (issue #555's addressed-routing slice): still
    /// sets the UI runtime-wide coalesced flag (needed either way — it is what
    /// wakes an idle event loop at all) but marks the ADDRESSED
    /// presentation's own pump wake bit, never a sibling's, so an
    /// idle-and-quiet sibling presentation is not woken by a redraw request
    /// that was never its own (the platform-window half of this same
    /// requirement is each presentation's own `on_need_visual_update`
    /// wiring, `presentation.rs`).
    pub(super) fn request_redraw_for(&self, presentation: &PresentationState) {
        self.needs_redraw.store(true, Ordering::Relaxed);
        presentation.mark_redraw_pending();
        // The flags alone are inert while the event loop is parked: nothing
        // converts them into a frame until SOMETHING pokes the platform.
        // An input-driven redraw request (a drag's scroll write) was
        // exactly that case — the loop went straight back to waiting after
        // the pointer event, the flags sat unread, and a live drag froze
        // the screen while the position advanced invisibly. The wake sets
        // `needs_redraw` (idempotent with the store above) AND requests a
        // platform redraw, which winit coalesces per frame.
        // Use the ui_runtime's shared owner-turn delivery ledger. If the platform
        // hook unwinds before posting the redraw, the dirty flags above stay
        // durable and the next ingress or completed command-drain boundary
        // retries the unacknowledged wake generation.
        self.sender_prototype.wake_owner();
    }

    /// [`Self::request_redraw_for`]'s pipeline-dirtying counterpart,
    /// addressed to exactly `presentation` rather than always the primary.
    /// Forces `presentation` to repaint on its next pump segment even
    /// though nothing in its widget tree itself changed — for a
    /// renderer-side reason the build/layout/paint pipeline has no way to
    /// observe on its own. Every caller resolves `presentation` from the
    /// failure or recovery event it alone owns:
    ///
    /// - [`Self::mark_primary_needs_full_repaint`] (below), for the
    ///   pre-frame GPU device-recovery path (`runner.rs`'s
    ///   `render_frame_with_device_recovery`), where the recovered device's
    ///   backing store was invalidated by the loss — `Renderer::recover`
    ///   already primes a full repaint (`damage_tracker.mark_full_repaint`)
    ///   for whatever scene reaches it next, but nothing reaches it at all
    ///   unless something forces one. That caller is still single-renderer/
    ///   single-presentation shaped (see its own TODO(#559)), so `primary()`
    ///   is the only presentation it has to address.
    /// - [`Self::render_frame`]'s own `retry_needed` arm (issue
    ///   #637), for a submit that failed with `SurfaceLost`, `DeviceLost`,
    ///   or `SurfaceValidation` mid-frame: the frame that just failed had
    ///   already consumed the pipeline's dirty state producing the scene it
    ///   tried to send, so the eventual retry needs its own fresh reason to
    ///   redo the work, same as the pre-frame case above — addressed to
    ///   that call's own `producer`, NEVER `primary()` unconditionally
    ///   (that method's own doc, at the `producer` binding, explains why
    ///   the two can differ).
    /// - The per-presentation unwind boundary, when a panic escapes after a
    ///   successful pipeline in [`SegmentPhase::Tail`](crate::frame_failure::SegmentPhase::Tail) or
    ///   [`SegmentPhase::Scene`](crate::frame_failure::SegmentPhase::Scene). That catch still holds the exact failed
    ///   presentation, so it re-dirties it immediately instead of asking
    ///   pump-wide producer selection to infer attribution later.
    ///
    /// [`Self::request_redraw_for`] ALONE is necessary but not sufficient
    /// for either: it opens `draw_frame_entered`'s per-presentation segment
    /// gate (`PresentationState::mark_redraw_pending`, read by
    /// `take_redraw_pending`), which is REQUIRED for the segment to run at
    /// all — but an otherwise-unchanged tree, even with that gate open,
    /// still produces `FramePaintOutcome::Idle` and never reaches
    /// `render_scene`, because `PipelineOwner` tracks its own dirty state
    /// independently of the clock's demand mechanism (confirmed by a
    /// probe, not assumed: `should_run_segment()` correctly read `true`
    /// with `request_redraw_for` alone, and the pipeline still produced
    /// `Idle`). Marking the root render object dirty is what gives the
    /// pipeline actual work to redo.
    ///
    /// [`PipelineOwner::mark_needs_paint`](flui_rendering::pipeline::PipelineOwner::mark_needs_paint), deliberately NOT `mark_needs_
    /// layout`: layout has not changed across either caller's failure —
    /// a device loss invalidates only the renderer's backing store, and a
    /// submit failure (`SurfaceLost`/`DeviceLost`/`SurfaceValidation`) is
    /// likewise a renderer/surface-side rejection of an already-correct
    /// scene, never a report that the widget tree's geometry is wrong — so
    /// only the PAINTED OUTPUT needs to be resubmitted in either case.
    /// `mark_needs_paint` is sufficient because paint is already a
    /// full-tree descent every frame in this codebase (see `docs/` notes on
    /// that shape) — marking just the root non-Idle is enough for the
    /// descent to cover everything beneath it; forcing a full RELAYOUT of
    /// the whole tree for a purely renderer-side rejection would be
    /// strictly heavier than either fix needs. Verified, not assumed:
    /// swapping this one call site is what the SurfaceLost retry test's
    /// by-hand stand-in (`ui_runtime/`'s
    /// `surface_lost_retry_preserves_the_original_input_epoch_for_the_presented_frame`)
    /// also uses `mark_needs_layout` for, but that stand-in predates this
    /// method and was never revisited against the lighter mark — the
    /// device-recovery caller was the first real (non-test) one, and it
    /// takes the lighter one.
    ///
    /// Deliberately NOT extended to a structured pipeline error or to a
    /// boundary panic in Build, Finalize, or Pipeline: those paths have not
    /// successfully consumed the whole pipeline into a scene. The
    /// post-pipeline Tail/Scene distinction above is what makes repainting a
    /// correct retry premise rather than an indiscriminate fallback.
    ///
    /// Reached on every target: [`Self::render_frame`]'s `retry_needed` arm
    /// calls it.
    pub(super) fn mark_needs_full_repaint_for(&self, presentation: &PresentationState) {
        self.request_redraw_for(presentation);
        presentation
            .renderer()
            .root_pipeline_owner()
            .with_mut(|owner| {
                if let Some(root_id) = owner.root_id() {
                    owner.mark_needs_paint(root_id);
                }
            });
    }

    /// `Self::mark_needs_full_repaint_for`, addressed to
    /// `self.presentations.primary()` — the shape its one caller
    /// (`runner.rs`'s `render_frame_with_device_recovery`) needs: that
    /// function runs BEFORE `render_frame`, driven purely by
    /// `renderer.is_device_lost()`, with no per-pump `producer` of its own
    /// to resolve which presentation actually owns the lost device.
    // TODO(#559): marks `self.presentations.primary()` unconditionally --
    // a ui_runtime with a non-primary presentation hosting the device that just
    // recovered would mark the WRONG presentation's tree. Latent today
    // only because a secondary window carries no widget content yet
    // (`open_secondary_window`'s own doc); #559's addressing slice is
    // where this needs to become presentation-addressed, matching how
    // `render_frame_with_device_recovery` itself is still single-renderer/
    // single-presentation shaped. Its caller is the host's device recovery
    // (`flui-app`'s `render_frame_with_device_recovery`, desktop and Android);
    // web's own recovery pokes `wake_handle()` directly instead.
    pub fn mark_primary_needs_full_repaint(&self) {
        self.mark_needs_full_repaint_for(self.presentations.primary());
    }

    pub(crate) fn surface_restored(&self, id: flui_foundation::PresentationId) {
        if let Some(presentation) = self.presentations.get(id) {
            self.mark_needs_full_repaint_for(presentation);
        }
    }

    /// Whether a redraw is needed.
    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw.load(Ordering::Relaxed)
    }

    /// Mark the frame as rendered, clearing the redraw flag.
    pub fn mark_rendered(&self) {
        self.needs_redraw.store(false, Ordering::Relaxed);
    }

    /// Wake the platform event loop so the next frame is rendered — sets
    /// `needs_redraw` AND pokes the installed window.
    ///
    /// Deadlock-safety: this only ever touches the `Send + Sync` state
    /// captured in the UI runtime's `wake` (an `Arc<AtomicBool>` plus an `Arc<Mutex<Option<Arc<dyn
    /// PlatformWindow>>>>` on `AppRuntime` — see `FrameWakeHandle` in
    /// `runtime.rs`), never this UI runtime's own `widgets`/`renderer`/gesture
    /// locks, so it is safe to call from inside a build/layout/paint
    /// callback without risking a lock-ordering cycle against those.
    pub fn wake_frame(&self) {
        (self.wake)();
    }

    /// A cloned, `'static` handle to this UI runtime's wake capability — for a
    /// caller that must move a wake past this UI runtime's own borrow (e.g. an
    /// `async move` block spawned from inside a frame callback, which
    /// outlives the synchronous `&UiRuntime` the callback was given).
    /// `wake_frame()` above is for every same-scope caller instead. The web
    /// runner's GPU device-recovery `spawn_local` is its caller.
    #[must_use]
    pub fn wake_handle(&self) -> Arc<dyn Fn() + Send + Sync> {
        Arc::clone(&self.wake)
    }

    // ========================================================================
    // First-frame deferral and frame accounting (moved from the retired
    // `AppBinding`; the deferral gate itself is re-homed from
    // `RenderingBinding`'s own counter onto the primary presentation's
    // `FrameClock` — see `render_frame`'s own doc for the submit-gate
    // check that is the actual production consumer now)
    // ========================================================================

    /// Defer this UI runtime's primary presentation from sending a produced
    /// frame to the engine until a matching [`Self::allow_first_frame`].
    /// Does NOT stop that presentation's segment from running — see
    /// [`FrameClock::defer`](flui_scheduler::FrameClock::defer)'s own doc.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no production caller yet -- an app-author async-init \
                      deferral has no wiring point: the ui_runtime is internal to \
                      its host; the retired AppBinding had the same gap"
        )
    )]
    pub(crate) fn defer_first_frame(&self) {
        self.presentations.primary().clock().defer();
    }

    /// Undo one [`Self::defer_first_frame`]. See
    /// [`FrameClock::lift`](flui_scheduler::FrameClock::lift).
    ///
    /// A deferred pump's segment already ran and produced a real (withheld)
    /// scene — see `FrameClock`'s module doc — so
    /// `poll` already cleared that demand; there is nothing retained for a
    /// bare `lift()` alone to produce from. There is no retained-scene
    /// layer to fall back on and re-composite (see
    /// `RenderingBinding::redirty_root_for_represent`'s own doc for
    /// the identical problem that method exists to solve), so this method
    /// re-dirties the root itself whenever it actually lifts an active
    /// deferral — the withheld work does not sit blank until some UNRELATED
    /// input dirties the tree.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no production caller yet -- see defer_first_frame's doc"
        )
    )]
    pub(crate) fn allow_first_frame(&self) {
        let clock = self.presentations.primary().clock();
        let was_deferred = clock.is_deferred();
        clock.lift();
        if was_deferred {
            crate::renderer_binding::redirty_pipeline_root(
                self.presentations
                    .primary()
                    .renderer()
                    .root_pipeline_owner(),
            );
        }
    }

    /// Records a compositor/platform-delivered frame-request signal — a
    /// native `RedrawRequested` the platform actually paces (Wayland's
    /// per-surface frame callbacks; see `docs/adr/ADR-0044-driver-loop-hybrid.md`'s
    /// per-platform table for which backends genuinely deliver one) —
    /// against the primary presentation's
    /// [`FrameClock`](flui_scheduler::FrameClock). See
    /// [`FrameClock::record_compositor_tick`](flui_scheduler::FrameClock::record_compositor_tick)
    /// for what this feeds — pacing-feedback bookkeeping ONLY; this marks
    /// no demand of its own (see that method's own doc for why that was
    /// tried and reverted). This remains primary-addressed because the
    /// canonical backend frame callback is not presentation-addressed yet;
    /// a UI runtime may already hold N resident presentations, but secondary
    ///
    /// Only the desktop runner's frame callback calls it: Android, iOS and
    /// the web runner keep the wake-channel driver (ADR-0044's per-platform
    /// table).
    pub fn record_compositor_tick(&self, now: web_time::Instant) {
        self.presentations
            .primary()
            .clock()
            .record_compositor_tick(now);
    }

    /// Turn this presentation's performance overlay on or off. See the
    /// retired `AppBinding::set_performance_overlay`'s doc.
    pub fn set_performance_overlay(&self, enabled: bool) {
        self.presentations
            .primary()
            .set_performance_overlay(enabled);
    }

    /// Perform haptic feedback on this presentation's window. See the
    /// retired `AppBinding::perform_haptic_feedback`'s doc.
    #[expect(
        dead_code,
        reason = "no production caller yet, and this specific forwarding \
                  wrapper is untested (the haptics tests exercise \
                  PresentationState::perform_haptic_feedback directly)"
    )]
    pub(crate) fn perform_haptic_feedback(&self, feedback: HapticFeedback) {
        self.presentations
            .primary()
            .perform_haptic_feedback(feedback);
    }

    /// Apply a new device pixel ratio to presentation `id`'s render pipeline,
    /// semantics owner and inherited window data. Mounted consumers observe
    /// the new ratio on their next build; logical window size is unchanged.
    ///
    /// Each window has its own scale (monitors differ), so the ratio reaches
    /// only the presentation whose window reported it; its siblings keep
    /// theirs. Returns `false`, changing nothing, for a non-positive or
    /// non-finite ratio, or when `id` names no
    /// presentation this UI runtime hosts — for example, when its close was
    /// delivered before the scale change.
    pub fn set_device_pixel_ratio_for(&self, id: PresentationId, device_pixel_ratio: f64) -> bool {
        if !self.set_pipeline_device_pixel_ratio_for(id, device_pixel_ratio) {
            return false;
        }
        if let Some(source) = self.media_query_for(id) {
            source.update(|data| data.device_pixel_ratio = device_pixel_ratio);
        }
        true
    }

    /// The native resize batch updates render scale before publishing size,
    /// scale and appearance together. Keep this partial operation internal so
    /// external hosts cannot leave the inherited ratio behind the renderer.
    pub(crate) fn set_pipeline_device_pixel_ratio_for(
        &self,
        id: PresentationId,
        device_pixel_ratio: f64,
    ) -> bool {
        if !device_pixel_ratio.is_finite() || device_pixel_ratio <= 0.0 {
            return false;
        }
        let Some(presentation) = self.presentations.get(id) else {
            return false;
        };
        presentation
            .renderer()
            .root_pipeline_owner()
            .with_mut(|owner| owner.set_device_pixel_ratio(device_pixel_ratio));
        true
    }

    /// Check if there is pending work in ANY presentation this UI runtime hosts:
    /// a pending build, pending gesture motion/deadlines, or a dirty render
    /// node. The runner's wake gate (`needs_redraw() || has_pending_work()`)
    /// reads this every frame. Production may host multiple presentations,
    /// although secondary windows are contentless today, so the union must
    /// remain presentation-wide rather than assuming the primary is the
    /// UI runtime's only source of pending work.
    pub fn has_pending_work(&self) -> bool {
        self.presentations.iter().any(|presentation| {
            presentation.has_pending_work()
                || presentation.gestures().has_pending_motion()
                || presentation.gestures().has_pending_deadlines()
        })
    }

    /// The earliest wall-clock instant ANY presentation this UI runtime hosts
    /// needs the platform loop to wake at, if any — the min over every
    /// presentation's own armed gesture-arena
    /// deadline (`GestureBinding::next_deadline`, e.g. a long-press hold or
    /// a double-tap give-up). This is genuinely INDEPENDENT of every
    /// presentation's `FrameClock` state: a gated presentation's pending
    /// input-correctness deadline (per this issue's own rule — arena
    /// deadlines are input correctness, not frame production) still needs
    /// the loop to wake and re-drive a pump so `tick_deadlines` can resolve
    /// it, even though that pump will render nothing for the gated
    /// presentation itself.
    #[must_use]
    pub fn next_wake(&self) -> Option<web_time::Instant> {
        self.presentations
            .iter()
            .filter_map(|presentation| presentation.gestures().next_deadline())
            .min()
    }
}
