//! Enabling and disabling semantics, the semantics owner and its update, route-change and
//! announcement outputs.

use crate::pipeline::phase::PipelinePhase;

use super::PipelineOwner;

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// Turns off, or back on, the debug check that a pass which re-placed
    /// moved semantics publishes what a whole-tree rebuild would. A test that
    /// counts what a pass costs turns it off; every other test keeps it.
    #[cfg(any(test, feature = "testing"))]
    pub fn set_semantics_placement_check(&mut self, checks: bool) {
        self.semantics_regions.set_checks_placement(checks);
    }

    /// Returns whether semantics are enabled.
    #[inline]
    pub fn semantics_enabled(&self) -> bool {
        self.semantics_enabled
    }

    /// Sets whether semantics are enabled.
    ///
    /// Lazily creates a [`SemanticsOwner`](flui_semantics::SemanticsOwner) on the
    /// `false` → `true` transition (unless one was already installed via
    /// [`Self::set_semantics_owner`] — e.g. a platform binding that needs
    /// its own update callback — in which case that owner is kept as-is).
    /// Disposes the owner on the `true` → `false` transition. Enablement is a
    /// single boolean flag that gates `run_semantics`.
    ///
    /// A freshly-created owner has an empty tree even though the render
    /// tree may already hold content built while semantics was off, so this
    /// also seeds the root as needing a semantics rebuild (mirrors
    /// `insert()`'s "new nodes need layout and paint" seeding) — without
    /// it, nothing would ever push the first `run_semantics` assembly pass.
    ///
    /// The transition is committed before any user code runs (ADR-0287): the
    /// flag, the owner, the root's seed, and on disable the owner's
    /// teardown. The visual-update wake runs after that, contained, even when
    /// the teardown panics; the first panic is resumed at the end.
    ///
    /// A lazily-created owner publishes through the callback installed via
    /// [`Self::set_semantics_update_callback`] — the composition root's wire
    /// to the platform accessibility bridge (flui-semantics ARCHITECTURE.md, semantics assembly). With none installed
    /// the owner falls back to the documented no-op placeholder: it still
    /// assembles a tree, inspectable via [`Self::semantics_owner`] or the render
    /// harness, but publishes nowhere.
    pub fn set_semantics_enabled(&mut self, enabled: bool) {
        let was_enabled = std::mem::replace(&mut self.semantics_enabled, enabled);
        let mut recovery = flui_foundation::panic::PanicRecovery::new();
        let wake = if enabled && !was_enabled {
            if self.semantics_owner.is_none() {
                let callback = self
                    .semantics_update_callback
                    .clone()
                    .unwrap_or_else(no_op_semantics_update_callback);
                let mut owner = flui_semantics::SemanticsOwner::new(callback);
                owner.set_device_pixel_ratio(self.device_pixel_ratio.get());
                owner.set_route_change_sink(self.semantics_route_change_sink.clone());
                if let Some(announcements) = self.semantics_regions.existing_announcements() {
                    owner.set_announcements(announcements.clone());
                }
                self.semantics_owner = Some(owner);
            }
            let seeded = self.root_id.is_some_and(|root| {
                self.render_tree.get(root).is_some_and(|node| {
                    self.scheduler
                        .commit_node_needing_semantics(root, usize::from(node.depth()))
                })
            });
            seeded.then(|| self.notifier.visual_update())
        } else if !enabled && was_enabled {
            if let Some(mut owner) = self.semantics_owner.take() {
                // Teardown drops the tree's action handlers and stops the
                // announcement listener: user code, after the flag and the
                // owner slot are already committed.
                recovery.run(|| owner.dispose());
                recovery.retire(owner);
            }
            None
        } else {
            return;
        };
        if let Some(wake) = wake {
            wake.notify_with(&mut recovery.scope());
        }
        recovery.finish();
    }

    /// Installs the platform-publish callback for this pipeline's semantics
    /// owner.
    ///
    /// The composition root calls this once it holds a platform
    /// accessibility bridge, so that every [`SemanticsOwner`]
    /// [`Self::set_semantics_enabled`] lazily creates — now or on any later
    /// re-enable — publishes to the platform instead of the no-op
    /// placeholder. An owner already alive when the callback arrives is
    /// swapped live: its next flush publishes the full rooted tree, so no
    /// state is lost to the ordering.
    ///
    /// [`SemanticsOwner`]: flui_semantics::SemanticsOwner
    pub fn set_semantics_update_callback(
        &mut self,
        callback: flui_semantics::SemanticsUpdateCallback,
    ) {
        if let Some(owner) = self.semantics_owner.as_mut() {
            owner.set_callback(callback.clone());
        }
        self.semantics_update_callback = Some(callback);
    }

    /// Installs the sink told each route change (ADR-0204 §5) on this
    /// pipeline's semantics owner, now and on each one it creates later.
    ///
    /// The composition root installs one where the platform bridge conveys
    /// route changes natively; `None` makes the owner announce each named
    /// route instead.
    pub fn set_semantics_route_change_sink(
        &mut self,
        sink: Option<flui_semantics::platform::RouteChangeSink>,
    ) {
        if let Some(owner) = self.semantics_owner.as_mut() {
            owner.set_route_change_sink(sink.clone());
        }
        self.semantics_route_change_sink = sink;
    }

    /// Schedules a self-contained full semantics publish, re-seeding the
    /// assembly pass so it actually happens.
    ///
    /// The reconnect path: assistive technology (re)activated, so whatever
    /// the adapter previously held is unknown — the owner forgets its
    /// published-state mirror ([`flui_semantics::SemanticsOwner::schedule_full_publish`])
    /// and the root is marked as needing semantics so the next
    /// `run_semantics` rebuilds and flushes even on an otherwise-idle
    /// frame. A no-op while no owner exists: the enable transition that
    /// creates one starts from an empty mirror, which is already a full
    /// first publish.
    pub fn request_semantics_full_publish(&mut self) {
        let Some(owner) = self.semantics_owner.as_mut() else {
            return;
        };
        owner.schedule_full_publish();
        if let Some(root_id) = self.root_id {
            self.mark_needs_semantics(root_id);
        }
    }

    /// The presentation's announcement queue (ADR-0204 §4), created on first
    /// request and installed in every semantics owner this pipeline creates,
    /// so messages and announcer identities survive accessibility being
    /// turned off and on. `None` once the synthetic identity space is
    /// exhausted.
    pub fn announcements(&mut self) -> Option<flui_semantics::Announcements> {
        let created = self.semantics_regions.existing_announcements().is_none();
        let announcements = self.semantics_regions.announcements()?;
        if created && let Some(owner) = self.semantics_owner.as_mut() {
            owner.set_announcements(announcements.clone());
        }
        Some(announcements)
    }

    /// Whether the semantics owner holds an announcement that the next
    /// frame's semantics phase publishes, though no node is dirty.
    #[must_use]
    pub fn has_pending_announcements(&self) -> bool {
        self.semantics_owner
            .as_ref()
            .is_some_and(flui_semantics::SemanticsOwner::has_undelivered_announcements)
    }

    /// Returns the semantics owner, if semantics is currently enabled.
    ///
    /// `None` until [`Self::set_semantics_enabled`]`(true)` lazily creates
    /// one; `None` again once the matching `false` transition disposes it.
    /// Read-only by design (no lock, no `&mut` escape —
    /// the owner's tree is written only by `Semantics::run_semantics`).
    #[inline]
    pub fn semantics_owner(&self) -> Option<&flui_semantics::SemanticsOwner> {
        self.semantics_owner.as_ref()
    }

    /// Takes the semantics diagnostics the frame driver has not yet reported,
    /// oldest first. They live on this pipeline, not on the semantics owner,
    /// so disabling semantics or replacing the owner does not drop them.
    #[must_use]
    pub fn take_semantics_diagnostics(&mut self) -> Vec<flui_semantics::SemanticsDiagnostic> {
        self.semantics_diagnostics.drain(..).collect()
    }

    /// Puts back semantics diagnostics taken and not reported, ahead of any
    /// queued since.
    pub fn restore_semantics_diagnostics(
        &mut self,
        unreported: Vec<flui_semantics::SemanticsDiagnostic>,
    ) {
        for finding in unreported.into_iter().rev() {
            self.semantics_diagnostics.push_front(finding);
        }
    }

    /// Takes every semantics diagnostic queued since the last call, reported
    /// or not, oldest first: a test harness fails the frame on any.
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn take_unchecked_semantics_diagnostics(
        &mut self,
    ) -> Vec<flui_semantics::SemanticsDiagnostic> {
        std::mem::take(&mut self.unchecked_semantics_diagnostics)
    }

    /// Spends the presentation's synthetic identity space, so a test sees
    /// what exhaustion publishes.
    #[cfg(any(test, feature = "testing"))]
    pub fn exhaust_semantics_identities(&mut self) {
        self.semantics_regions.exhaust();
    }

    /// Installs (or removes) the semantics owner directly, bypassing the
    /// lazy-creation path in [`Self::set_semantics_enabled`].
    ///
    /// The escape hatch for a platform binding that needs its own update
    /// callback (forwarding the translated [`flui_semantics::TreeUpdate`]s
    /// to a real OS accessibility bridge), or a test that
    /// wants to observe `flush()`'s callback invocations directly. Call
    /// this *before* `set_semantics_enabled(true)` — the enable path only
    /// lazily creates a no-op-callback owner when none is installed yet.
    ///
    /// An installed owner is given this pipeline's device pixel ratio, so it
    /// publishes physical bounds like a lazily created one.
    /// Trees containing synthetic regions are rebuilt under this pipeline's
    /// identity authority; identities allocated by a different source cannot be
    /// imported. Hand-built trees containing only render boundaries remain valid.
    pub fn set_semantics_owner(&mut self, mut owner: Option<flui_semantics::SemanticsOwner>) {
        if let Some(owner) = owner.as_mut() {
            if owner.tree().iter().any(|(_, node)| {
                node.accessibility_id().is_some() && node.source_render_id().is_none()
            }) {
                owner.clear();
            }
            owner.set_device_pixel_ratio(self.device_pixel_ratio.get());
        }
        // A tree assembled elsewhere has no containment known here, so the
        // next pass assembles it whole.
        self.semantics_regions.forget_containment();
        let retired = std::mem::replace(&mut self.semantics_owner, owner);
        if self.semantics_enabled()
            && let Some(root) = self.root_id
        {
            self.mark_needs_semantics(root);
        }
        drop(retired);
    }
}

/// The fallback semantics-update callback for a lazily-created
/// [`SemanticsOwner`](flui_semantics::SemanticsOwner) whose pipeline was
/// never handed a platform bridge.
///
/// A composition root with a real accessibility adapter installs its own
/// callback via [`PipelineOwner::set_semantics_update_callback`] before (or
/// after — the swap is live) enabling semantics. Without one — the render
/// harness, `flui-testing`, a headless embedder — swallowing updates here is
/// an explicit, documented placeholder (not a silent gap): the assembled
/// tree is still inspectable via [`PipelineOwner::semantics_owner`] or the
/// harness.
///
/// The callback receives an already-translated `accesskit::TreeUpdate`, so a
/// real bridge forwards it to the platform capability without touching
/// semantics types — which is what keeps `flui-native` (layer 2) free of a
/// dependency on `flui-semantics` (layer 3).
fn no_op_semantics_update_callback() -> flui_semantics::SemanticsUpdateCallback {
    std::rc::Rc::new(|_update: &flui_semantics::TreeUpdate| {})
}
