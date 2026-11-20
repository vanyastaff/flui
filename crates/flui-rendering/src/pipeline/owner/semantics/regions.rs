//! The identities of the semantics nodes that regions of a render node host, and the callbacks a
//! pass retains while it assembles them.

use flui_foundation::RenderId;
use flui_foundation::arena::FrameArena;
use flui_semantics::SemanticsConfiguration;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{error::SemanticsError, storage::RenderTree};

use super::{BuiltSemanticsNode, SemanticsPlan, graft::PlacementScratch};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum RegionPlacement {
    BeforeChildren,
    AfterChildren,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct RegionSource {
    pub(super) host: RenderId,
    pub(super) placement: RegionPlacement,
    pub(super) key: flui_semantics::SemanticsRegionKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum SemanticsSource {
    Render(RenderId),
    Region(RegionSource),
}

/// A text run's full identity (ADR-0204): an edit that changes any part gives
/// the run a new identity, so an old position never lands on other text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TextRunKey {
    field: SemanticsSource,
    ordinal: usize,
    start: usize,
    text: Box<str>,
}
/// The pipeline keeps the authority across accessibility disable/enable and
/// owner replacement. Only a successfully assembled scope retires missing keys.
#[derive(Debug, Default)]
pub(in crate::pipeline::owner) struct RegionIdentities {
    source: flui_semantics::SemanticsIdentitySource,
    pub(super) retained: FxHashMap<RegionSource, flui_semantics::SemanticsRegionId>,
    /// The announcement queue and announcer node identities, minted from
    /// `source` on first request and kept across owner replacement.
    announcements: Option<flui_semantics::Announcements>,
    /// The identity space ran out before the announcer nodes were minted.
    announcements_exhausted: bool,
    /// Each field's text runs and their identities, kept while the run is
    /// the same: the same node, place, start and text.
    text_runs: FxHashMap<TextRunKey, flui_semantics::SemanticsRegionId>,
    /// The identity space ran out before a field's runs were given theirs.
    text_runs_exhausted: bool,
    /// The render nodes the last assembly walk visited, kept between passes
    /// so a pass reuses the table instead of growing a new one.
    pub(super) visited: FxHashSet<RenderId>,
    /// A pass's marked nodes, its placement anchors and the placement walk's
    /// stack, kept between passes for the same reason.
    pub(super) marked: Vec<RenderId>,
    pub(super) placed: Vec<RenderId>,
    pub(super) placement_walk: PlacementScratch,
    /// The published nodes that are not contained (see
    /// [`PendingSemanticsNode::form`]), which a graft compares its anchor
    /// with. Its table is kept between passes, as the lists above are.
    pub(super) uncontained: FxHashSet<flui_foundation::SemanticsId>,
    /// Whether `uncontained` describes the published tree: false until a
    /// full assembly publishes it here.
    pub(super) containment_known: bool,
    /// Set by a test that measures a pass and so must not pay for
    /// [`check_placement_against_rebuild`].
    #[cfg(any(test, feature = "testing"))]
    skip_placement_check: bool,
}

pub(super) struct AssemblyRegions<'f> {
    /// Where the pass's scratch lists come from (ADR-0328): each is freed before the level
    /// that listed it returns.
    pub(super) arena: &'f FrameArena,
    /// Where the child lists of the nodes the pass builds come from: they outlive the level
    /// that built them, and in [`Self::arena`] would sit above every list freed after them.
    pub(super) built: &'f FrameArena,
    /// The render nodes this pass walked, in the table the previous pass grew:
    /// emptying it writes its control bytes, which costs less than regrowing.
    pub(super) visited: FxHashSet<RenderId>,
    pub(super) authored: FxHashSet<RegionSource>,
    pub(super) duplicate: Option<RegionSource>,
    pub(super) invalid_sort_key: Option<RenderId>,
    pub(super) callbacks: AssemblyCallbacks,
}

impl<'f> AssemblyRegions<'f> {
    /// Regions for a pass that takes its scratch lists from `arena`, builds its nodes' child
    /// lists in `built` and walks into the table of `visited`, emptied.
    pub(super) fn reusing(
        arena: &'f FrameArena,
        built: &'f FrameArena,
        mut visited: FxHashSet<RenderId>,
    ) -> Self {
        visited.clear();
        Self {
            arena,
            built,
            visited,
            authored: FxHashSet::default(),
            duplicate: None,
            invalid_sort_key: None,
            callbacks: AssemblyCallbacks::default(),
        }
    }
}

/// Pins executable ownership before folding can replace a receiver's handler
/// or discard a blocked, clipped or fallback contributor. Topology and metadata
/// remain ordinary temporary values; only the opaque captures need this custody.
#[derive(Default)]
pub(super) struct AssemblyCallbacks {
    handlers: Vec<flui_semantics::SemanticsActionHandler>,
    pub(super) rejected: bool,
}

impl AssemblyCallbacks {
    pub(super) fn retain(&mut self, config: &SemanticsConfiguration) {
        self.handlers
            .extend(config.registered_action_handlers().cloned());
    }
}

impl Drop for AssemblyCallbacks {
    fn drop(&mut self) {
        let mut recovery = flui_foundation::panic::PanicRecovery::new();
        if self.rejected {
            recovery.inherit_failure();
        }
        for handler in std::mem::take(&mut self.handlers) {
            recovery.retire(handler);
        }
        recovery.finish();
    }
}

impl RegionIdentities {
    /// Forgets which published nodes are contained, so no graft is planned
    /// until a full assembly has published the tree again.
    pub(in crate::pipeline::owner) fn forget_containment(&mut self) {
        self.containment_known = false;
        self.uncontained.clear();
    }

    /// Whether a pass that re-placed moved subtrees checks itself against a
    /// rebuild: in debug builds with the `testing` feature, unless a test
    /// turned it off.
    #[cfg(any(test, feature = "testing"))]
    pub(super) fn checks_placement(&self) -> bool {
        cfg!(debug_assertions) && !self.skip_placement_check
    }

    /// Turns the debug placement check on or off.
    #[cfg(any(test, feature = "testing"))]
    pub(in crate::pipeline::owner) fn set_checks_placement(&mut self, checks: bool) {
        self.skip_placement_check = !checks;
    }

    /// The presentation's announcement queue, minting the two announcer
    /// identities on first use. `None` once the identity space is exhausted,
    /// reported once.
    pub(in crate::pipeline::owner) fn announcements(
        &mut self,
    ) -> Option<flui_semantics::Announcements> {
        if self.announcements.is_none() && !self.announcements_exhausted {
            if let Ok(ids) = self.source.allocate(2) {
                self.announcements = Some(flui_semantics::Announcements::new(ids[0], ids[1]));
            } else {
                self.announcements_exhausted = true;
                tracing::error!(
                    "semantics identity space exhausted: this presentation cannot announce"
                );
            }
        }
        self.announcements.clone()
    }

    /// Gives the text runs of every field `plan` builds their identities:
    /// a run seen before keeps its own, a new one takes one from the source.
    /// When the source is exhausted the field keeps its runs without
    /// identities, so the owner publishes it without them and reports it.
    /// A field the plan rebuilt forgets the runs it no longer has.
    pub(super) fn assign_text_runs(&mut self, plan: &mut SemanticsPlan<'_>, tree: &RenderTree) {
        let mut stack: Vec<&mut BuiltSemanticsNode<'_>> = match plan {
            SemanticsPlan::Rebuild(root) => root.as_deref_mut().into_iter().collect(),
            SemanticsPlan::Graft(grafts) => grafts.iter_mut().map(|(_, built)| built).collect(),
        };
        let mut rebuilt = FxHashSet::default();
        let mut live = FxHashSet::default();
        while let Some(node) = stack.pop() {
            rebuilt.insert(node.source);
            if let Some(content) = node.config.text_content() {
                let keys: Vec<TextRunKey> = content
                    .runs()
                    .iter()
                    .enumerate()
                    .map(|(ordinal, run)| TextRunKey {
                        field: node.source,
                        ordinal,
                        start: run.byte_range().start,
                        text: run.text().into(),
                    })
                    .collect();
                let missing: Vec<&TextRunKey> = keys
                    .iter()
                    .filter(|key| !self.text_runs.contains_key(*key))
                    .collect();
                match self.source.allocate(missing.len()) {
                    Ok(ids) => {
                        for (key, id) in missing.into_iter().zip(ids) {
                            self.text_runs.insert(key.clone(), id);
                        }
                        let ids = keys.iter().map(|key| self.text_runs[key]).collect();
                        let content = content
                            .clone()
                            .with_run_identities(ids)
                            .expect("BUG: one identity is assigned per run");
                        node.config.set_text_content(content);
                    }
                    Err(_) if !self.text_runs_exhausted => {
                        self.text_runs_exhausted = true;
                        tracing::error!(
                            "semantics identity space exhausted: text fields publish without runs"
                        );
                    }
                    Err(_) => {}
                }
                live.extend(keys);
            }
            stack.extend(node.children.iter_mut());
        }
        let retained = &self.retained;
        let identities = &mut self.source;
        self.text_runs.retain(|key, id| {
            let keep = live.contains(key)
                || (!rebuilt.contains(&key.field)
                    && match key.field {
                        SemanticsSource::Render(id) => tree.contains(id),
                        SemanticsSource::Region(source) => retained.contains_key(&source),
                    });
            if !keep {
                let released = identities.release(*id);
                debug_assert!(released, "BUG: a kept text run holds its identity");
            }
            keep
        });
    }
    /// Spends the identity source.
    #[cfg(any(test, feature = "testing"))]
    pub(in crate::pipeline::owner) fn exhaust(&mut self) {
        self.source.exhaust();
    }

    /// The queue, if one was requested.
    pub(in crate::pipeline::owner) fn existing_announcements(
        &self,
    ) -> Option<&flui_semantics::Announcements> {
        self.announcements.as_ref()
    }

    pub(super) fn prepare(
        &mut self,
        regions: &AssemblyRegions<'_>,
        tree: &RenderTree,
    ) -> Result<(), SemanticsError> {
        if let Some(host) = regions.invalid_sort_key {
            return Err(SemanticsError::InvalidSortKey { host });
        }
        if let Some(source) = regions.duplicate {
            return Err(SemanticsError::DuplicateRegion {
                host: source.host,
                key: source.key.0,
            });
        }
        let missing: Vec<_> = regions
            .authored
            .iter()
            .copied()
            .filter(|source| !self.retained.contains_key(source))
            .collect();
        let ids = self
            .source
            .allocate(missing.len())
            .map_err(|_| SemanticsError::RegionIdentityExhausted)?;
        let identities = &mut self.source;
        self.retained.retain(|source, id| {
            let keep = tree.contains(source.host)
                && (!regions.visited.contains(&source.host) || regions.authored.contains(source));
            if !keep {
                let released = identities.release(*id);
                debug_assert!(released, "BUG: a retained region holds its identity");
            }
            keep
        });
        self.retained.extend(missing.into_iter().zip(ids));
        Ok(())
    }
}
