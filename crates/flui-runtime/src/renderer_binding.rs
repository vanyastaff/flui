//! RenderingBinding - the glue between the render trees and the FLUI engine.
//!
//! It holds a presentation's root pipeline owner and its first-frame gate.
//!
//! # Architecture
//!
//! ```text
//! RenderingBinding
//!   ├── root_pipeline_owner   - Root of PipelineOwner tree
//!   └── first-frame gate      - defer_first_frame / allow_first_frame
//! ```
//!
//! # Usage
//!
//! For most applications, use `UiRuntime` instead ([`crate::ui_runtime`]),
//! which owns this binding plus widgets support per window. Use
//! `RenderingBinding` directly only when working with the rendering
//! layer without widgets.

use std::cell::Cell;

use flui_foundation::geometry::Offset;
use flui_rendering::{
    hit_testing::HitTestResult,
    pipeline::{PipelineCell, PipelineOwner, TextContextHandle},
};

/// Shared body for [`RenderingBinding::redirty_root_for_represent`]:
/// operates on the bare [`PipelineCell`] (rather than requiring a full
/// `RenderingBinding` reference) so a caller that only holds that
/// handle can reuse the identical logic instead of re-deriving it.
pub(crate) fn redirty_pipeline_root(pipeline_owner: &PipelineCell) {
    let render_invalidation_handle = pipeline_owner.with(|root_owner| {
        root_owner
            .root_id()
            .and_then(|root_id| root_owner.local_render_invalidation_handle(root_id))
    });
    if let Some(handle) = render_invalidation_handle
        && let Err(e) = handle.mark_needs_layout()
    {
        tracing::warn!(
            error = ?e,
            "redirty_root_for_represent: failed to re-mark the root dirty; \
             the withheld/re-enabled content may not present until \
             something else dirties the tree",
        );
    }
}

/// How the render tree answered [`RenderingBinding::hit_test`].
#[derive(Debug)]
#[must_use = "a busy tree or a contained panic is lost if the answer is dropped"]
#[expect(
    clippy::exhaustive_enums,
    reason = "a caller acts on each answer, and a `_` arm would lose a busy tree or a contained panic"
)]
pub enum ViewHitTest {
    /// The walk ran; the result holds its path.
    ///
    /// A render object whose hit test panicked counted as a miss, and the
    /// first such payload is here for the caller to resume once the event
    /// has been delivered to the path.
    Answered {
        /// The first contained render-object panic, if any.
        panic: Option<Box<dyn std::any::Any + Send>>,
    },
    /// The tree cannot answer now — its layout is withdrawn (before the
    /// first complete frame, or after a failed one) or a frame phase has it
    /// checked out. The result is untouched: an empty path here would claim
    /// that nothing is under the position.
    TreeBusy,
}

// ============================================================================
// RenderingBinding
// ============================================================================
/// Concrete binding for applications using the Rendering framework directly.
///
/// This is the glue that binds the framework to the FLUI engine.
/// For widget-based applications, use `UiRuntime` instead.
///
/// # Responsibilities
///
/// - Holding the root [`PipelineOwner`] tree
/// - Hit testing that tree for input and the agent
/// - The first-frame deferral gate
///
/// Whether semantics are enabled belongs to each presentation's
/// `SemanticsHost` (`crate::semantics_host`), which the platform's
/// accessibility bridge sets; the pipeline owner's own switch is
/// `PipelineOwner::set_semantics_enabled`.
///
/// # Thread Safety
///
/// Owner-thread-confined (`!Send + !Sync`): its state is `Cell`s,
/// and a `PipelineOwner` belongs to exactly one presentation on exactly one
/// thread (ADR-0323 §1).
///
/// This is enforced at compile time, not by convention -- pinned by
/// `assert_not_impl_any!(PipelineCell: Send, Sync)` and
/// `assert_not_impl_any!(PipelineOwner: Send, Sync)` in
/// `flui_rendering::pipeline::owner::cell`'s own tests. A clone taken from
/// this binding cannot cross a thread boundary. `trybuild_ui::ui_tests` pins
/// the diagnostic and a valid local caller; this example illustrates it:
///
/// ```compile_fail
/// use flui_runtime::renderer_binding::RenderingBinding;
///
/// let binding =
///     RenderingBinding::new(flui_rendering::TextContextHandle::standalone());
/// let pipeline = binding.root_pipeline_owner().clone();
/// std::thread::spawn(move || {
///     pipeline.with(|owner| owner.root_id());
/// });
/// ```
pub struct RenderingBinding {
    /// Root of the PipelineOwner tree (shared with the owning `UiRuntime`'s
    /// presentation).
    root_pipeline_owner: PipelineCell,

    /// Outstanding [`RenderingBinding::defer_first_frame`] calls.
    first_frame_deferred_count: Cell<u32>,

    /// Whether the first frame has been sent.
    first_frame_sent: Cell<bool>,
}

impl std::fmt::Debug for RenderingBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderingBinding")
            .field(
                "first_frame_deferred_count",
                &self.first_frame_deferred_count.get(),
            )
            .field("first_frame_sent", &self.first_frame_sent.get())
            .finish_non_exhaustive()
    }
}

impl RenderingBinding {
    /// Creates a rendering binding with its own [`PipelineOwner`], measuring
    /// text through `text`.
    pub fn new(text: TextContextHandle) -> Self {
        Self::new_with_pipeline(PipelineCell::new(PipelineOwner::new(text)))
    }

    /// Creates a rendering binding over `pipeline_owner`, the same
    /// [`PipelineCell`] the owning presentation's elements use, so one
    /// `PipelineOwner` serves both.
    pub fn new_with_pipeline(pipeline_owner: PipelineCell) -> Self {
        Self {
            root_pipeline_owner: pipeline_owner,
            first_frame_deferred_count: Cell::new(0),
            first_frame_sent: Cell::new(false),
        }
    }

    // ========================================================================
    // First Frame Deferral
    //
    // This is the ONE canonical implementation of the first-frame deferral
    // gate. Every consumer (`send_frames_to_engine` below and
    // `UiRuntime::defer_first_frame` / `allow_first_frame` /
    // `send_frames_to_engine` in `crates/flui-runtime/src/ui_runtime/`, which
    // the production `render_frame` path actually calls) forwards to this struct.
    //
    // # First-frame gate
    //
    // `send_frames_to_engine` is `first_frame_sent || deferred_count == 0`.
    // Layout/compositing-bits/paint always run; only the composite-to-engine
    // step is gated. The production split lives in `UiRuntime::
    // render_frame` (`crates/flui-runtime/src/ui_runtime/`): the
    // build/layout/paint pipeline always runs in `draw_frame_entered`, and
    // only the GPU `render_scene` (present) call is gated on
    // `send_frames_to_engine`. `run_frame` does not yet gate its own
    // semantics phase behind this flag — a documented, narrower scope for
    // this unit, not a silent gap.
    // ========================================================================

    /// Tell the framework to not send the first frames to the engine until
    /// there is a corresponding call to
    /// [`allow_first_frame`](Self::allow_first_frame).
    ///
    /// Call this to perform asynchronous initialization work before the first
    /// frame is rendered (which takes down the splash screen). The framework
    /// will still do all the work to produce frames, but those frames are never
    /// sent to the engine and will not appear on screen.
    ///
    /// Calling this has no effect after the first frame has been sent.
    ///
    /// # Panics
    ///
    /// Panics when the deferral count is exhausted, preserving all existing deferrals.
    pub fn defer_first_frame(&self) {
        let count = self
            .first_frame_deferred_count
            .get()
            .checked_add(1)
            .expect("first-frame deferral count exhausted");
        self.first_frame_deferred_count.set(count);
    }

    /// Called after [`defer_first_frame`](Self::defer_first_frame) to tell
    /// the framework that it is ok to send the first frame to the engine now.
    ///
    /// For best performance, this method should only be called while the
    /// scheduler phase is idle.
    ///
    /// This method may only be called once for each corresponding call
    /// to [`defer_first_frame`](Self::defer_first_frame).
    ///
    /// # Panics
    ///
    /// Panics if called without a matching prior
    /// [`defer_first_frame`](Self::defer_first_frame) call (i.e. the deferred
    /// count is already zero) — a caller-contract violation, mirroring the
    /// oracle's `assert(_firstFrameDeferredCount > 0)`.
    pub fn allow_first_frame(&self) {
        let count = self
            .first_frame_deferred_count
            .get()
            .checked_sub(1)
            .expect("allow_first_frame called without matching defer_first_frame");
        self.first_frame_deferred_count.set(count);

        // Schedule a warm-up frame even if the count is not down to zero
        // yet: removing one deferral may uncover a NEW one further down the
        // widget tree (a subtree that only registers its own
        // `defer_first_frame` once an outer one clears), so every
        // `allow_first_frame` call gets a pass, not just the last.
        //
        // The withheld frame(s) already ran build/layout/paint while
        // deferred (the pipeline work happens unconditionally — see the
        // module note above `defer_first_frame`); their dirty flags are
        // already clear by the time the deferral lifts. The oracle's
        // `compositeFrame()` re-submits the RETAINED layer tree regardless
        // of dirty state, so its `scheduleWarmUpFrame` always has something
        // to present. FLUI has no such retained-scene re-present — without
        // re-marking the root dirty, the warm-up frame would find nothing
        // to do and the withheld content would sit blank until some
        // UNRELATED input dirtied the tree. See
        // [`redirty_root_for_represent`](Self::redirty_root_for_represent).
        if !self.first_frame_sent.get() {
            self.redirty_root_for_represent();
        }
    }

    /// Re-dirty the root so the next frame actually produces content,
    /// without depending on some UNRELATED input dirtying the tree first.
    ///
    /// FLUI has no retained-scene layer to fall back on (see the
    /// first-frame-deferral module note above): a frame with nothing dirty
    /// produces `FramePaintOutcome::Idle`, not a repeat of the last
    /// `Scene`. Two callers hit this exact problem — [`allow_first_frame`]
    /// (a deferred frame becoming presentable) and the UI runtime's own
    /// scheduler's frames-disabled→enable re-enable edge (see `ADR-0035`
    /// and `emit_lifecycle_transition`'s doc in `runner.rs`), which has no
    /// retained scene to re-present either — so the shared logic lives
    /// here, once.
    ///
    /// Routed through [`PipelineOwner::local_render_invalidation_handle`].
    /// This binding belongs to the UI owner. The root's attachment retains
    /// the layout mark before waking, so external channel pressure cannot
    /// discard the request to present again.
    ///
    /// [`allow_first_frame`]: Self::allow_first_frame
    pub fn redirty_root_for_represent(&self) {
        redirty_pipeline_root(self.root_pipeline_owner());
    }

    /// Call this to pretend that no frames have been sent to the engine yet.
    ///
    /// This is useful for tests that want to call [`Self::defer_first_frame`] and
    /// [`Self::allow_first_frame`] since those methods only have an effect if no
    /// frames have been sent to the engine yet.
    pub fn reset_first_frame_sent(&self) {
        self.first_frame_sent.set(false);
    }

    /// Latch that the first frame has been sent to the engine.
    ///
    /// Mirrors the oracle's `_firstFrameSent = true`, set unconditionally
    /// once `send_frames_to_engine()` is found `true` during a frame —
    /// regardless of whether that particular frame painted anything.
    /// Idempotent: once latched, [`Self::send_frames_to_engine`] stays
    /// `true` forever (barring [`Self::reset_first_frame_sent`], a test-only
    /// escape hatch), so calling this again is a harmless no-op.
    pub fn mark_first_frame_sent(&self) {
        self.first_frame_sent.set(true);
    }
}

// ============================================================================
// Hit testing and the frame gate
// ============================================================================

impl RenderingBinding {
    /// Hit tests at `position` in this binding's render tree.
    ///
    /// `kind` is the pointer asking, whose minimum target expanded hit areas
    /// follow. The answer says whether the tree could be asked at all; see
    /// [`ViewHitTest`].
    pub fn hit_test(
        &self,
        result: &mut HitTestResult,
        position: Offset,
        kind: flui_interaction::events::PointerKind,
    ) -> ViewHitTest {
        // Hits route through the REAL render tree (the pipeline
        // owner's), not the per-view registry — the registry views
        // are bare metric holders with no children, so testing them
        // produced no targets. Leaf-first entries come back from the
        // owner's hit-test walk (RenderState.offset-aware).
        //
        // The position arrives in LOGICAL pixels already: every
        // platform converter normalizes at event build (Win32 divides raw
        // physical coordinates by the scale factor;
        // macOS NSPoint is logical natively). The render tree lives in
        // logical pixels too, so the position passes through unscaled
        // — dividing by the DPR here would shrink every hit a second
        // time on scaled displays.
        //
        // A render object's panic surfaces only after the walk completed the
        // path without it (`PipelineOwner::hit_test`); it is caught here so
        // the caller can deliver to that path first.
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: a render object's panic is held until the hit path is delivered"
        )]
        let walk = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.root_pipeline_owner
                .try_with_layout(|owner| owner.hit_test(position, kind, result))
        }));
        match walk {
            Ok(Some(_hit)) => ViewHitTest::Answered { panic: None },
            Ok(None) => ViewHitTest::TreeBusy,
            Err(payload) => ViewHitTest::Answered {
                panic: Some(payload),
            },
        }
    }

    /// The root pipeline owner of this binding's render tree.
    pub fn root_pipeline_owner(&self) -> &PipelineCell {
        &self.root_pipeline_owner
    }

    /// Whether frames should be sent to the engine.
    ///
    /// If false, the framework does all frame work but doesn't render; the
    /// first frame is deferred this way. Answers `true` once the first frame
    /// has been sent, or while no first-frame deferral is outstanding.
    pub fn send_frames_to_engine(&self) -> bool {
        self.first_frame_sent.get() || self.first_frame_deferred_count.get() == 0
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // `RenderingBinding` is no longer singleton-backed — each test
    // below constructs its own, independent instance instead of sharing a
    // process-wide one, so there is nothing left for a test lock to
    // serialize (the retired semantics-binding test lock guarded exactly
    // this shared-singleton hazard; see AGENTS.md's "Testing quirks").

    /// The gate withholds frames while any deferral is held, lifts when the
    /// last is released, and once a frame has been sent stays open: a later
    /// deferral has no effect. The production frame path consults it through
    /// `UiRuntime::render_frame`
    /// (`deferred_painted_frame_waits_for_the_later_present_to_commit`).
    fn the_gate_follows_its_deferrals_until_a_frame_is_sent() {
        let binding = RenderingBinding::new(TextContextHandle::standalone());
        assert!(binding.send_frames_to_engine(), "nothing is deferred");

        binding.defer_first_frame();
        binding.defer_first_frame();
        assert!(!binding.send_frames_to_engine());
        binding.allow_first_frame();
        assert!(!binding.send_frames_to_engine(), "one deferral remains");
        binding.allow_first_frame();
        assert!(binding.send_frames_to_engine(), "every deferral released");

        binding.mark_first_frame_sent();
        binding.defer_first_frame();
        assert!(
            binding.send_frames_to_engine(),
            "a deferral after the first frame was sent has no effect"
        );
    }

    fn an_exhausted_deferral_count_preserves_all_existing_deferrals() {
        let binding = RenderingBinding::new(TextContextHandle::standalone());
        // Exhausting the public counter would require billions of calls. Seed
        // this failure boundary, then exercise only the public gate operations.
        binding.first_frame_deferred_count.set(u32::MAX);
        for _ in 0..2 {
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    binding.defer_first_frame();
                }))
                .is_err()
            );
            assert!(!binding.send_frames_to_engine());
        }
        binding.allow_first_frame();
        assert!(
            !binding.send_frames_to_engine(),
            "other deferrals remain live"
        );
        binding.defer_first_frame();
        assert!(!binding.send_frames_to_engine());
    }

    #[test]
    fn the_first_frame_gate_counts_deferrals() {
        crate::table_test::run_table(
            "the_first_frame_gate_counts_deferrals",
            &[
                (
                    "the_gate_follows_its_deferrals_until_a_frame_is_sent",
                    the_gate_follows_its_deferrals_until_a_frame_is_sent,
                ),
                (
                    "an_exhausted_deferral_count_preserves_all_existing_deferrals",
                    an_exhausted_deferral_count_preserves_all_existing_deferrals,
                ),
            ],
        );
    }
}
