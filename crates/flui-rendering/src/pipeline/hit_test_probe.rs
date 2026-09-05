//! The render tree's answer to "what is under this position right now".
//!
//! Pointer dispatch resolves a hit-test path once, at `PointerDown`, and
//! replays that cached route for every later `Move`/`Up` — so a gesture that
//! travels over something new never learns about it. Drag-target discovery
//! needs the opposite: a fresh test at the pointer's *current* global position
//! on every move, independent of where the drag went down.
//!
//! The capability is declared in `flui-interaction`, where realm identity and
//! thread affinity already live, and implemented here, where the tree is.

use flui_interaction::{HitTestProbe, HitTestResult, InteractionDispatchError};
use flui_types::{Offset, Pixels};

use super::{PipelineCell, WeakPipelineCell};

/// A [`HitTestProbe`] backed by a live [`PipelineCell`].
///
/// Installed on a realm's interaction lane at construction, so widget code
/// reaching `BuildContext::hit_test_handle()` tests against the same tree
/// pointer dispatch walks — not a parallel registry that would drift from it.
///
/// Holds the cell **weakly**. The lane outlives any one presentation, so a
/// strong clone here would keep a closed presentation's whole render tree —
/// and its dirty-request receiver — alive, turning handles that are supposed
/// to fail closed after a close into ones that quietly keep working.
///
/// Liveness is a **separate** signal from that weak reference, because the two
/// are different facts. `BuildContext::pipeline_owner()` hands out a strong
/// `PipelineCell`, and a widget that stores one keeps the tree's allocation
/// alive past its presentation's close; a probe treating "the allocation is
/// freed" as "the presentation closed" would go on answering from a detached
/// tree. Under `SharedRealm` the realm ticket stays valid too, since a sibling
/// presentation is still live, so nothing else would catch it. The presentation
/// therefore owns a token, and dropping the presentation drops it regardless of
/// who still holds the tree.
#[derive(Debug, Clone)]
pub struct PipelineHitTestProbe {
    pipeline: WeakPipelineCell,
    alive: std::rc::Weak<()>,
}

impl PipelineHitTestProbe {
    /// Probe `pipeline`'s render tree for as long as `alive` upgrades.
    ///
    /// `alive` is a weak handle on a token the PRESENTATION owns — see the
    /// type's docs for why the tree's own liveness is not enough.
    #[must_use]
    pub fn new(pipeline: &PipelineCell, alive: std::rc::Weak<()>) -> Self {
        Self {
            pipeline: pipeline.downgrade(),
            alive,
        }
    }
}

impl HitTestProbe for PipelineHitTestProbe {
    fn probe(
        &self,
        position: Offset<Pixels>,
        result: &mut HitTestResult,
    ) -> Result<(), InteractionDispatchError> {
        // `try_with`, not `with`: a frame phase holding the tree checked out
        // is a state this call can legitimately land in, and the shared borrow
        // `with` takes would panic against it. Reporting the tree busy is the
        // honest answer -- an empty path would read as "the drag is over
        // nothing" and make every target it was over fire a leave.
        // The presentation's own token first: a tree someone else is keeping
        // alive is still a tree its presentation has shut.
        if self.alive.upgrade().is_none() {
            return Err(InteractionDispatchError::OwnerGone);
        }
        let pipeline = self
            .pipeline
            .upgrade()
            .ok_or(InteractionDispatchError::OwnerGone)?;
        pipeline
            .try_with(|owner| owner.hit_test(position, result))
            .map(|_hit| ())
            .ok_or(InteractionDispatchError::TreeBusy)
    }
}

#[cfg(test)]
mod tests {
    use flui_interaction::{HitTestProbe, HitTestResult, InteractionDispatchError};
    use flui_types::{Offset, Pixels, Size, geometry::px};

    use super::{PipelineCell, PipelineHitTestProbe};
    use crate::{
        PipelineOwner,
        constraints::BoxConstraints,
        context::BoxLayoutContext,
        parent_data::BoxParentData,
        protocol::BoxProtocol,
        traits::{RenderBox, RenderObject},
    };
    use flui_foundation::RenderId;

    /// A leaf that claims every hit inside its own size — the default
    /// `RenderBox::hit_test`, spelled out so this test does not depend on
    /// which catalog types happen to be hit-testable.
    #[derive(Debug)]
    struct HittableLeaf {
        size: Size,
    }

    impl flui_foundation::Diagnosticable for HittableLeaf {}

    impl RenderBox for HittableLeaf {
        type Arity = flui_tree::Leaf;
        type ParentData = BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
        ) -> Size {
            ctx.constraints().constrain(self.size)
        }
    }

    /// A laid-out one-node tree, 20x20 at the origin.
    fn laid_out_cell() -> PipelineCell {
        let mut owner = PipelineOwner::new();
        let root = owner.insert(Box::new(HittableLeaf {
            size: Size::new(px(20.0), px(20.0)),
        }) as Box<dyn RenderObject<BoxProtocol>>);
        owner.set_root_id(Some(root));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(20.0), px(20.0)))));

        let cell = PipelineCell::new(owner);
        cell.with_mut(|o| {
            let (returned, _) = std::mem::take(o).run_frame();
            *o = returned;
        });
        cell
    }

    fn snapshot_at(probe: &PipelineHitTestProbe, x: f32, y: f32) -> Vec<RenderId> {
        let mut result = HitTestResult::new();
        probe
            .probe(Offset::new(Pixels(x), Pixels(y)), &mut result)
            .expect("tree is free");
        result.path().iter().map(|entry| entry.target).collect()
    }

    #[test]
    fn the_probe_answers_from_the_position_it_is_given() {
        let cell = laid_out_cell();
        let open = std::rc::Rc::new(());
        let probe = PipelineHitTestProbe::new(&cell, std::rc::Rc::downgrade(&open));

        assert!(
            !snapshot_at(&probe, 10.0, 10.0).is_empty(),
            "the middle of a laid-out 20x20 root must hit it"
        );
        assert!(
            snapshot_at(&probe, 100.0, 100.0).is_empty(),
            "a position outside every node must hit nothing -- if this also \
             reports hits, the probe is answering from position-independent \
             state rather than testing the position given"
        );
    }

    /// The probe must not keep the tree it reads alive.
    ///
    /// A realm's interaction lane outlives any one presentation, so a strong
    /// clone here would keep a closed presentation's whole render tree — and
    /// its dirty-request receiver — alive past the close. That is not a leak
    /// you would notice as a leak: it shows up as
    /// `RenderInvalidationHandle`s that are supposed to fail closed after a
    /// presentation shuts quietly continuing to work, which is exactly how
    /// flui-app's `dropped_presentations_surviving_pipeline_handles_fail_closed`
    /// caught the first draft of this.
    #[test]
    fn the_probe_does_not_keep_a_dropped_tree_alive() {
        let probe = {
            let cell = laid_out_cell();
            let open = std::rc::Rc::new(());
            let probe = PipelineHitTestProbe::new(&cell, std::rc::Rc::downgrade(&open));
            let mut warm = HitTestResult::new();
            probe
                .probe(Offset::new(Pixels(10.0), Pixels(10.0)), &mut warm)
                .expect("answers while the tree is alive");
            probe
        };

        let mut result = HitTestResult::new();
        assert_eq!(
            probe
                .probe(Offset::new(Pixels(10.0), Pixels(10.0)), &mut result)
                .unwrap_err(),
            InteractionDispatchError::OwnerGone,
            "once the last strong holder drops the tree, the probe must report \
             it gone -- neither answering from a tree it is itself keeping \
             alive, nor reporting an empty path"
        );
    }

    /// A retained tree does not keep a closed presentation answerable.
    ///
    /// `BuildContext::pipeline_owner()` hands out a STRONG `PipelineCell`, and
    /// a widget may legitimately store one. If that were the only liveness
    /// signal, such a widget would keep this probe's weak reference
    /// upgradeable after its presentation closed, and the handle would go on
    /// hit-testing a detached tree instead of reporting it gone — reachable
    /// today under `SharedRealm`, where the realm ticket stays valid because a
    /// sibling presentation is still live.
    ///
    /// So closure is signalled by a token the presentation owns, not by the
    /// tree's allocation being freed. The two are different facts.
    #[test]
    fn a_retained_tree_does_not_keep_a_closed_presentation_answerable() {
        let cell = laid_out_cell();
        let alive = std::rc::Rc::new(());
        let probe = PipelineHitTestProbe::new(&cell, std::rc::Rc::downgrade(&alive));

        let mut warm = HitTestResult::new();
        probe
            .probe(Offset::new(Pixels(10.0), Pixels(10.0)), &mut warm)
            .expect("answers while the presentation is open");

        // The presentation closes. `cell` is still held here, standing in for
        // the widget that retained `pipeline_owner()`.
        drop(alive);

        let mut result = HitTestResult::new();
        assert_eq!(
            probe
                .probe(Offset::new(Pixels(10.0), Pixels(10.0)), &mut result)
                .unwrap_err(),
            InteractionDispatchError::OwnerGone,
            "a closed presentation must report itself gone even while someone \
             still holds its tree alive -- answering from it is answering from \
             a tree the application has shut"
        );
    }

    #[test]
    fn a_checked_out_tree_is_reported_busy_rather_than_answered_empty() {
        let cell = laid_out_cell();
        let open = std::rc::Rc::new(());
        let probe = PipelineHitTestProbe::new(&cell, std::rc::Rc::downgrade(&open));

        // Hold the tree the way a frame phase does, then ask from inside.
        let verdict = cell.with_mut(|_owner| {
            let mut result = HitTestResult::new();
            probe.probe(Offset::new(Pixels(10.0), Pixels(10.0)), &mut result)
        });

        assert_eq!(
            verdict.unwrap_err(),
            InteractionDispatchError::TreeBusy,
            "a position that hits when the tree is free must report BUSY, not \
             an empty path, while a frame holds it -- an empty path there is a \
             lie the caller cannot detect, and would read as a drag over \
             nothing"
        );
    }
}
