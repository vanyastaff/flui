use std::{
    any::Any,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use flui_foundation::ViewKey;

use super::*;

const CLONE_PANIC: &str = "hostile key clone outside update region";

#[derive(Clone)]
struct ClonePanicsWhenArmedKey {
    identity: u64,
    armed: Arc<AtomicBool>,
}

impl ViewKey for ClonePanicsWhenArmedKey {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn key_eq(&self, other: &dyn ViewKey) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| self.identity == other.identity)
    }

    fn key_hash(&self) -> u64 {
        self.identity
    }

    fn clone_key(&self) -> Box<dyn ViewKey> {
        assert!(!self.armed.load(Ordering::SeqCst), "{CLONE_PANIC}");
        Box::new(self.clone())
    }

    fn debug_fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "ClonePanicsWhenArmedKey({})", self.identity)
    }

    fn is_global_key(&self) -> bool {
        true
    }
}

#[derive(Clone)]
struct ClonePanicView {
    key: ClonePanicsWhenArmedKey,
    update_calls: Arc<AtomicUsize>,
    activation_calls: Arc<AtomicUsize>,
}

struct ClonePanicState {
    update_calls: Arc<AtomicUsize>,
    activation_calls: Arc<AtomicUsize>,
}

impl crate::StatefulView for ClonePanicView {
    type State = ClonePanicState;

    fn create_state(&self) -> Self::State {
        ClonePanicState {
            update_calls: Arc::clone(&self.update_calls),
            activation_calls: Arc::clone(&self.activation_calls),
        }
    }
}

impl crate::ViewState<ClonePanicView> for ClonePanicState {
    fn build(
        &self,
        _view: &ClonePanicView,
        _ctx: &dyn crate::BuildContext,
    ) -> impl crate::IntoView {
        UnitRenderHost
    }

    fn activate(&mut self) {
        self.activation_calls.fetch_add(1, Ordering::SeqCst);
    }

    fn did_update_view(&mut self, _old_view: &ClonePanicView, _new_view: &ClonePanicView) {
        self.update_calls.fetch_add(1, Ordering::SeqCst);
    }
}

impl View for ClonePanicView {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::stateful(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

struct RetakeFixture {
    tree: ElementTree,
    owner: BuildOwner,
    pipeline: PipelineCell,
    donor: ElementId,
    destination: ElementId,
    candidate: ElementId,
    view: ClonePanicView,
}

fn retake_fixture(identity: u64) -> RetakeFixture {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let pipeline = PipelineCell::new(flui_rendering::pipeline::PipelineOwner::new());
    let root = tree.mount_root_with_pipeline_owner(
        &UnitRenderHost,
        Some(pipeline.clone()),
        &mut owner.element_owner_mut(),
    );
    let donor = tree.insert(&UnitRenderHost, root, 0, &mut owner.element_owner_mut());
    let destination = tree.insert(&UnitRenderHost, root, 1, &mut owner.element_owner_mut());
    tree.get_mut(root)
        .expect("root remains live")
        .set_child_ids(vec![donor, destination]);
    let view = ClonePanicView {
        key: ClonePanicsWhenArmedKey {
            identity,
            armed: Arc::new(AtomicBool::new(false)),
        },
        update_calls: Arc::new(AtomicUsize::new(0)),
        activation_calls: Arc::new(AtomicUsize::new(0)),
    };
    let candidate = tree.insert(&view, donor, 0, &mut owner.element_owner_mut());
    tree.get_mut(donor)
        .expect("donor remains live")
        .set_child_ids(vec![candidate]);
    owner.schedule_build_for(
        candidate,
        tree.get(candidate).expect("candidate remains live").depth(),
        crate::RebuildReason::InitialMount,
    );
    owner.build_scope(&mut tree);
    RetakeFixture {
        tree,
        owner,
        pipeline,
        donor,
        destination,
        candidate,
        view,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct RetakeTopology {
    donor_children: Vec<ElementId>,
    destination_children: Vec<ElementId>,
    candidate_parent: Option<ElementId>,
    candidate_lifecycle: crate::Lifecycle,
    is_inactive: bool,
    registered_candidate: Option<ElementId>,
    render_frontier: Vec<(ElementId, flui_foundation::RenderId)>,
    render_parents: Vec<Option<flui_foundation::RenderId>>,
}

fn retake_topology(fixture: &mut RetakeFixture) -> RetakeTopology {
    let render_frontier = collect_render_frontier(&fixture.tree, fixture.candidate)
        .expect("the candidate subtree remains resolvable");
    let render_parents = fixture.pipeline.with(|pipeline| {
        render_frontier
            .iter()
            .map(|&(_, render_id)| pipeline.render_tree().parent(render_id))
            .collect()
    });
    let candidate = fixture
        .tree
        .get(fixture.candidate)
        .expect("the candidate remains live");
    RetakeTopology {
        donor_children: fixture
            .tree
            .get(fixture.donor)
            .expect("the donor remains live")
            .child_ids()
            .to_vec(),
        destination_children: fixture
            .tree
            .get(fixture.destination)
            .expect("the destination remains live")
            .child_ids()
            .to_vec(),
        candidate_parent: candidate.parent(),
        candidate_lifecycle: candidate.element().lifecycle(),
        is_inactive: fixture
            .owner
            .element_owner_mut()
            .is_inactive(fixture.candidate),
        registered_candidate: fixture.owner.element_for_global_key(&fixture.view.key),
        render_frontier,
        render_parents,
    }
}

fn assert_clone_panic(payload: &(dyn Any + Send)) {
    assert_eq!(
        flui_foundation::panic::payload_text(payload),
        Some(CLONE_PANIC)
    );
}

fn assert_no_recovery(fixture: &mut RetakeFixture) {
    assert!(fixture.owner.take_recovered_panics().is_empty());
    assert!(fixture.tree.iter_nodes().all(|(_, node)| {
        node.element().view_type_id() != std::any::TypeId::of::<crate::ErrorView>()
    }));
}

#[test]
fn direct_update_key_clone_panic_propagates_before_user_update() {
    let mut fixture = retake_fixture(41);
    let update_calls = fixture.view.update_calls.load(Ordering::SeqCst);
    fixture.view.key.armed.store(true, Ordering::SeqCst);

    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = fixture.tree.update_or_substitute(
            fixture.candidate,
            &fixture.view,
            0,
            &mut fixture.owner.element_owner_mut(),
            "direct hostile key clone",
        );
    }))
    .expect_err("framework key cloning remains outside the update catch");

    assert_clone_panic(payload.as_ref());
    assert_no_recovery(&mut fixture);
    assert_eq!(
        fixture.view.update_calls.load(Ordering::SeqCst),
        update_calls
    );
    assert_eq!(
        fixture
            .tree
            .get(fixture.donor)
            .expect("donor remains live")
            .child_ids(),
        &[fixture.candidate]
    );
}

#[test]
fn inactive_retake_key_clone_panic_propagates_before_any_retake_mutation() {
    let mut fixture = retake_fixture(42);
    fixture
        .tree
        .remove(fixture.candidate, &mut fixture.owner.element_owner_mut());
    fixture
        .tree
        .get_mut(fixture.donor)
        .expect("donor remains live")
        .set_child_ids(Vec::new());
    let before = retake_topology(&mut fixture);
    let activation_calls = fixture.view.activation_calls.load(Ordering::SeqCst);
    let update_calls = fixture.view.update_calls.load(Ordering::SeqCst);
    fixture.view.key.armed.store(true, Ordering::SeqCst);

    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _reconcile_guard = fixture.tree.begin_reconcile(fixture.destination);
        let _ = fixture.tree.mount_or_substitute(
            &fixture.view,
            fixture.destination,
            0,
            &mut fixture.owner.element_owner_mut(),
            ProvisionalOrder::NONE,
            "inactive retake hostile key clone",
        );
    }))
    .expect_err("framework key cloning remains outside the retake update catch");

    assert_clone_panic(payload.as_ref());
    assert_no_recovery(&mut fixture);
    assert_eq!(retake_topology(&mut fixture), before);
    assert_eq!(
        fixture.view.activation_calls.load(Ordering::SeqCst),
        activation_calls
    );
    assert_eq!(
        fixture.view.update_calls.load(Ordering::SeqCst),
        update_calls
    );
}

#[test]
fn active_retake_key_clone_panic_propagates_before_any_retake_mutation() {
    let mut fixture = retake_fixture(43);
    let before = retake_topology(&mut fixture);
    let activation_calls = fixture.view.activation_calls.load(Ordering::SeqCst);
    let update_calls = fixture.view.update_calls.load(Ordering::SeqCst);
    fixture.view.key.armed.store(true, Ordering::SeqCst);

    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _reconcile_guard = fixture.tree.begin_reconcile(fixture.destination);
        let _ = fixture.tree.mount_or_substitute(
            &fixture.view,
            fixture.destination,
            0,
            &mut fixture.owner.element_owner_mut(),
            ProvisionalOrder::NONE,
            "active retake hostile key clone",
        );
    }))
    .expect_err("framework key cloning remains outside the retake update catch");

    assert_clone_panic(payload.as_ref());
    assert_no_recovery(&mut fixture);
    assert_eq!(retake_topology(&mut fixture), before);
    assert_eq!(
        fixture.view.activation_calls.load(Ordering::SeqCst),
        activation_calls
    );
    assert_eq!(
        fixture.view.update_calls.load(Ordering::SeqCst),
        update_calls
    );
}
