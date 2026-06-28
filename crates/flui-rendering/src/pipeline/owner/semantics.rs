//! Semantics phase implementation for `PipelineOwner<Semantics>`.

use crate::pipeline::{
    phase::{Idle, Semantics},
    scheduler::PhaseKind,
};

use super::{PipelineOwner, rebind_phase};

// ============================================================================
// Semantics phase: run_semantics
// ============================================================================

impl PipelineOwner<Semantics> {
    /// Completes the frame and returns to [`Idle`].
    #[must_use]
    pub fn finish(self) -> PipelineOwner<Idle> {
        rebind_phase(self)
    }

    /// Updates semantics for all dirty render objects.
    ///
    /// This is phase 4 of the rendering pipeline. During semantics:
    /// - Accessibility information is gathered
    /// - Semantics tree is updated
    ///
    /// Nodes are sorted by depth (shallow first) for top-down traversal.
    /// The geometries of children depend on ancestors' transforms and clips,
    /// so parents must be processed first. This matches Flutter's
    /// `flushSemantics`.
    pub fn run_semantics(&mut self) -> crate::error::RenderResult<()> {
        if !self.semantics_enabled() {
            return Ok(());
        }

        tracing::debug!(
            "run_semantics: {} nodes",
            self.scheduler.semantics_queue_len()
        );

        self.scheduler.enter_phase(PhaseKind::Semantics);

        // PR #109 review feedback: pre-fix this path used
        // `std::mem::take(&mut self.dirty.needs_semantics)` to drain in
        // one step. Take leaves an empty `Vec::new()` (capacity 0)
        // behind, so every subsequent semantics-enabled frame's first
        // push re-allocates. Switch to an in-place sort + iterate +
        // clear pattern that preserves the Vec's backing capacity
        // across frames (idiom: *Programming Rust* 2nd ed §11 "Owned
        // vs Borrowed", retain the allocation by retaining ownership).
        // The Flutter-parity `where !object._needsLayout` filter the
        // pre-cycle comment promised was never implemented; that gap
        // lands when the real semantics-config build is wired (R-1
        // follow-up).

        // Sort shallow-first matching Flutter's flushSemantics. Roots
        // dispatch before their descendants so a parent's config is
        // assembled before children fold into it.
        self.scheduler.sort_semantics_shallow_first();

        // Cycle 4 R-1: pre-cycle the path panicked with
        // `unimplemented!()` once any node was queued — a Constitution
        // Principle 6 violation in a hot-path callable from
        // `RendererBinding::draw_frame` on every frame as soon as
        // semantics_enabled() flipped true.
        //
        // Post-cycle: walk the dirty list, emit a `tracing::warn!`
        // per node carrying the missing-integration hint, and return
        // `Ok(())`. The framework no longer aborts on semantics flips;
        // when the full `SemanticsOwner` integration lands, swap the
        // warn for the real config-build + owner-register call.
        //
        // Aggregate into a count rather than emitting one warn per node
        // (avoids O(n) log spam when semantics is enabled on a large tree).
        // `nodes_needing_semantics()` is a shared slice accessor — disjoint
        // from the mutable `self.render_tree` field under Rust 2024 capture.
        let pending_count = self
            .scheduler
            .nodes_needing_semantics()
            .iter()
            .filter(|d| self.render_tree.contains(d.id))
            .count();
        if pending_count > 0 {
            tracing::warn!(
                count = pending_count,
                "run_semantics: full SemanticsOwner integration pending; \
                 semantics config build for {pending_count} node(s) is a no-op until \
                 RenderObject → SemanticsConfiguration plumbing lands"
            );
        }
        // `clear()` retains the Vec's allocated capacity; next frame's
        // pushes amortise into the existing buffer.
        self.scheduler.clear_semantics_queue();

        // exit_phase clears debug_doing_semantics AND drains mid-semantics
        // marks so semantics marks made during this iteration's
        // `debug_doing_semantics = true` window aren't stranded. Drained
        // entries land on dirty.needs_semantics for the NEXT run_semantics.
        let _ = self.scheduler.exit_phase(PhaseKind::Semantics);

        Ok(())
    }
}
