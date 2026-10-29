//! A render-tree snapshot taken through its `PipelineCell` answers `SnapshotError::Busy` when
//! asked from inside a frame or from inside its own walk, and a description that reaches back
//! for the owner is recorded on its node rather than ending the snapshot.

use std::cell::RefCell;

use flui_foundation::geometry::{Matrix4, Size};
use flui_foundation::inspect::{INSPECT_FAILED, SnapshotError, SnapshotQuery};
use flui_rendering::{
    TextContextHandle,
    pipeline::{PipelineCell, PipelineOwner, WeakPipelineCell},
    protocol::{BoxProtocol, ProtocolGeometry},
    testing::{RenderTester, box_node},
    traits::{HitTestOutcome, RenderObject},
};

thread_local! {
    /// The cell holding the tree the probe sits in.
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static CELL: RefCell<Option<WeakPipelineCell>> = const { RefCell::new(None) };
    /// What each nested snapshot answered.
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static ANSWERS: RefCell<Vec<Option<SnapshotError>>> = const { RefCell::new(Vec::new()) };
}

/// What the probe's description does with its pipeline.
#[derive(Debug, Clone, Copy)]
enum Reach {
    /// Asks for a snapshot of its own tree and keeps the answer.
    Snapshot,
    /// Checks the owner out for writing.
    Write,
}

/// A 10×10 leaf whose description reaches back into its pipeline.
#[derive(Debug)]
struct Reaching(Reach);

impl flui_foundation::Diagnosticable for Reaching {
    fn debug_fill_properties(&self, _properties: &mut flui_foundation::DiagnosticsBuilder) {
        let cell = CELL
            .with_borrow(|cell| cell.as_ref().and_then(WeakPipelineCell::upgrade))
            .expect("the test keeps its cell alive");
        match self.0 {
            Reach::Snapshot => {
                let answer = cell.inspect_render(SnapshotQuery::default()).err();
                ANSWERS.with_borrow_mut(|answers| answers.push(answer));
            }
            Reach::Write => cell.with_mut(|_| ()),
        }
    }
}

impl RenderObject<BoxProtocol> for Reaching {
    fn perform_layout_raw(
        &mut self,
        _ctx: &mut <BoxProtocol as flui_rendering::protocol::Protocol>::LayoutCtxErased<'_>,
    ) -> flui_rendering::error::RenderResult<ProtocolGeometry<BoxProtocol>> {
        Ok(Size::new(10.0, 10.0))
    }

    fn paint_raw(
        &self,
        _recorder: &mut flui_rendering::context::FragmentRecorder,
        _child_count: usize,
        _size: Size,
    ) {
    }

    fn hit_test_raw(
        &self,
        _position: flui_rendering::protocol::ProtocolPosition<BoxProtocol>,
        _child_count: usize,
        _size: Size,
        _hit_child: &mut dyn FnMut(
            usize,
            Option<flui_rendering::protocol::ProtocolPosition<BoxProtocol>>,
            Option<Matrix4>,
        ) -> bool,
    ) -> HitTestOutcome {
        HitTestOutcome::miss()
    }
}

/// A laid-out tree of one probe, moved into a cell the probe can reach.
fn cell_with(reach: Reach) -> PipelineCell {
    let mut run = RenderTester::mount(box_node(Reaching(reach)))
        .with_constraints(flui_rendering::constraints::BoxConstraints::new(
            0.0, 100.0, 0.0, 100.0,
        ))
        .run_frame();
    let owner = std::mem::replace(
        run.owner_mut(),
        PipelineOwner::new(TextContextHandle::standalone()),
    );
    let cell = PipelineCell::new(owner);
    CELL.with_borrow_mut(|slot| *slot = Some(cell.downgrade()));
    ANSWERS.with_borrow_mut(Vec::clear);
    cell
}

/// A snapshot asked for while a frame holds the owner, or from a description inside the
/// snapshot's own walk, answers that the tree is busy; the outer snapshot completes, and the
/// next one is taken.
///
/// Fails without the walk flag: the nested snapshot walks the tree again from inside its own
/// walk, without end.
#[test]
fn a_render_snapshot_asked_for_from_a_frame_or_its_own_walk_answers_busy() {
    let cell = cell_with(Reach::Snapshot);
    assert_eq!(
        cell.with_mut(|_| cell.inspect_render(SnapshotQuery::default()).err()),
        Some(SnapshotError::Busy("checked out by a frame"))
    );

    let root = cell
        .inspect_render(SnapshotQuery::default())
        .expect("asked from outside, the snapshot is taken");
    assert_eq!(root.fact(INSPECT_FAILED), None);
    assert_eq!(
        ANSWERS.with_borrow_mut(std::mem::take),
        [Some(SnapshotError::Busy("inspecting"))]
    );
    assert!(
        cell.inspect_render(SnapshotQuery::default()).is_ok(),
        "the walk flag ends with the snapshot"
    );
}

/// A description that checks the owner out during a snapshot has the refusal recorded on its
/// node, and the snapshot is taken.
///
/// Fails without the containment: the refusal escapes and no snapshot is taken.
#[test]
fn a_description_that_checks_the_owner_out_is_recorded_on_its_node() {
    let cell = cell_with(Reach::Write);
    let root = cell
        .inspect_render(SnapshotQuery::default())
        .expect("the snapshot is taken");
    let failure = root
        .get_fact::<&str>(INSPECT_FAILED)
        .expect("the refusal is recorded");
    assert!(failure.contains("with_mut called reentrantly"), "{failure}");
}
