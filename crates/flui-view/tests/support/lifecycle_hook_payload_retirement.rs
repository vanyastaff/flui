//! Swallowed removal-hook payloads cannot restart unwinding after containment.
use super::dense_reconcile_containment::{DenseHealthyLeaf, DenseRow};
use flui_foundation::ViewKey;
use flui_rendering::{
    pipeline::{PipelineCell, PipelineOwner},
    protocol::BoxProtocol,
};
use flui_view::{
    BuildContext, BuildOwner, ElementTree, GlobalKey, IntoView, LifecycleHook, RebuildReason,
    RenderView, StatefulView, View, ViewExt, ViewState,
};
use std::{
    any::TypeId,
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tracing_subscriber::{Layer, layer::Context, prelude::*};

struct Bomb(Arc<AtomicUsize>);
impl Drop for Bomb {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("opaque removal hook payload destroyed");
    }
}
fn fail(drops: &Arc<AtomicUsize>) -> ! {
    std::panic::panic_any((Bomb(Arc::clone(drops)), Bomb(Arc::clone(drops))));
}
#[derive(Clone, Copy)]
enum Hook {
    Dispose,
    Deactivate,
    RenderUnmount,
}
#[derive(Clone)]
struct Observed {
    hook: Hook,
    armed: Rc<Cell<bool>>,
    calls: Rc<Cell<usize>>,
    disposed: Rc<Cell<usize>>,
    drops: Arc<AtomicUsize>,
}
impl Observed {
    fn invoke(&self) {
        if self.armed.replace(false) {
            self.calls.set(self.calls.get() + 1);
            fail(&self.drops);
        }
    }
}
#[derive(Clone)]
struct StateSubject {
    key: Option<GlobalKey<HookState>>,
    observed: Observed,
}
struct HookState(Observed);
impl StatefulView for StateSubject {
    type State = HookState;
    fn create_state(&self) -> HookState {
        HookState(self.observed.clone())
    }
}
impl ViewState<StateSubject> for HookState {
    fn build(&self, _: &StateSubject, _: &dyn BuildContext) -> impl IntoView {
        DenseHealthyLeaf { marker: 3 }
    }
    fn dispose(&mut self) {
        self.0.disposed.set(self.0.disposed.get() + 1);
        if matches!(self.0.hook, Hook::Dispose) {
            self.0.invoke();
        }
    }
    fn deactivate(&mut self) {
        if matches!(self.0.hook, Hook::Deactivate) {
            self.0.invoke();
        }
    }
}
impl View for StateSubject {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
    fn key(&self) -> Option<&dyn ViewKey> {
        self.key.as_ref().map(|key| key as &dyn ViewKey)
    }
}
#[derive(Clone)]
struct RenderSubject(Observed);
impl RenderView for RenderSubject {
    type Protocol = BoxProtocol;
    type RenderObject = flui_objects::RenderSizedBox;
    fn create_render_object(&self, _: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        flui_objects::RenderSizedBox::shrink()
    }
    fn update_render_object(
        &self,
        _: &flui_view::RenderObjectContext<'_>,
        _: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
    fn did_unmount_render_object(
        &self,
        _: &flui_view::RenderObjectContext<'_>,
        _: &mut Self::RenderObject,
    ) {
        self.0.invoke();
    }
}
impl View for RenderSubject {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}
struct FailedTelemetry {
    calls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}
impl<S: tracing::Subscriber> Layer<S> for FailedTelemetry {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        if *event.metadata().level() == tracing::Level::ERROR
            && event.metadata().target() == "flui_view::owner::element_owner"
        {
            self.calls.fetch_add(1, Ordering::SeqCst);
            fail(&self.drops);
        }
    }
}
struct Drain<'a> {
    tree: &'a mut ElementTree,
    owner: &'a mut BuildOwner,
}
impl Drop for Drain<'_> {
    fn drop(&mut self) {
        self.owner.build_scope(self.tree);
    }
}

pub(crate) fn dispatch_child(kind: &str) {
    let hook = if kind.starts_with("dispose") {
        Hook::Dispose
    } else if kind.starts_with("deactivate") {
        Hook::Deactivate
    } else {
        Hook::RenderUnmount
    };
    let telemetry = kind.ends_with("report");
    let incoming = kind.ends_with("incoming");
    let original_drops = Arc::new(AtomicUsize::new(0));
    let report_drops = Arc::new(AtomicUsize::new(0));
    let report_calls = Arc::new(AtomicUsize::new(0));
    let observed = Observed {
        hook,
        armed: Rc::new(Cell::new(true)),
        calls: Rc::new(Cell::new(0)),
        disposed: Rc::new(Cell::new(0)),
        drops: Arc::clone(&original_drops),
    };
    let key = matches!(hook, Hook::Deactivate).then(GlobalKey::<HookState>::new);
    let subject = if matches!(hook, Hook::RenderUnmount) {
        RenderSubject(observed.clone()).boxed()
    } else {
        StateSubject {
            key: key.clone(),
            observed: observed.clone(),
        }
        .boxed()
    };
    let row = DenseRow {
        children: vec![
            DenseHealthyLeaf { marker: 1 }.boxed(),
            subject,
            DenseHealthyLeaf { marker: 2 }.boxed(),
        ],
    };
    let replacement = DenseRow {
        children: vec![
            DenseHealthyLeaf { marker: 1 }.boxed(),
            DenseHealthyLeaf { marker: 2 }.boxed(),
        ],
    };
    let pipeline = PipelineCell::new(PipelineOwner::new(
        flui_rendering::TextContextHandle::standalone(),
    ));
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let parent = tree.mount_root_with_pipeline_owner(
        &row,
        Some(pipeline.clone()),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(parent, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    let before = tree
        .get(parent)
        .expect("initial parent")
        .child_ids()
        .to_vec();
    let subject_id = before[1];
    let render_id = tree.get(subject_id).expect("subject").element().render_id();
    tree.update(parent, &replacement, &mut owner.element_owner_mut());
    owner.schedule_build_for(parent, 0, RebuildReason::ParentUpdate);
    let subscriber = tracing_subscriber::registry().with(telemetry.then(|| FailedTelemetry {
        calls: Arc::clone(&report_calls),
        drops: Arc::clone(&report_drops),
    }));
    let result = tracing::subscriber::with_default(subscriber, || {
        catch_unwind(AssertUnwindSafe(|| {
            if incoming {
                let _drain = Drain {
                    tree: &mut tree,
                    owner: &mut owner,
                };
                panic!("incoming lifecycle retirement failure");
            }
            owner.build_scope(&mut tree);
        }))
    });
    if incoming {
        let payload = result.expect_err("incoming failure remains authoritative");
        assert_eq!(
            flui_foundation::panic::payload_text(payload.as_ref()),
            Some("incoming lifecycle retirement failure")
        );
    } else {
        assert!(result.is_ok(), "removal-hook containment returns");
    }
    assert_eq!(observed.calls.get(), 1);
    assert_eq!(original_drops.load(Ordering::SeqCst), 0);
    assert_eq!(report_drops.load(Ordering::SeqCst), 0);
    assert_eq!(report_calls.load(Ordering::SeqCst), usize::from(telemetry));
    assert_eq!(
        tree.get(parent).expect("surviving parent").child_ids(),
        &[before[0], before[2]]
    );
    if matches!(hook, Hook::Deactivate) {
        assert_eq!(
            tree.get(subject_id)
                .expect("parked subject")
                .element()
                .lifecycle(),
            flui_view::element::Lifecycle::Inactive
        );
        assert!(owner.has_inactive_elements());
        assert_eq!(
            owner.element_for_global_key(key.as_ref().expect("key")),
            Some(subject_id)
        );
    } else {
        assert!(!tree.contains(subject_id));
        if let Some(render_id) = render_id {
            assert!(!pipeline.with(|pipeline| pipeline.render_tree().contains(render_id)));
        }
    }
    let records = owner.take_recovered_panics();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].hook,
        match hook {
            Hook::Dispose => LifecycleHook::Dispose,
            Hook::Deactivate => LifecycleHook::Deactivate,
            Hook::RenderUnmount => LifecycleHook::UnmountRenderObject,
        }
    );
    assert_eq!(
        records[0].view_type_id,
        if matches!(hook, Hook::RenderUnmount) {
            TypeId::of::<RenderSubject>()
        } else {
            TypeId::of::<StateSubject>()
        }
    );
    assert_eq!(records[0].payload_text, None);
    assert!(
        matches!(records[0].at, flui_view::RecoveredAt::Element { element, .. } if element == subject_id)
    );
    owner.finalize_tree(&mut tree);
    assert!(!tree.contains(subject_id));
    if let Some(key) = key {
        assert_eq!(owner.element_for_global_key(&key), None);
    }
    if !matches!(hook, Hook::RenderUnmount) {
        assert_eq!(observed.disposed.get(), 1);
    }
    // Recreate through the same producer, then remove normally without another failure.
    tree.update(parent, &row, &mut owner.element_owner_mut());
    owner.schedule_build_for(parent, 0, RebuildReason::ParentUpdate);
    owner.build_scope(&mut tree);
    let next_id = tree.get(parent).expect("next parent").child_ids()[1];
    assert_ne!(next_id, subject_id);
    tree.update(parent, &replacement, &mut owner.element_owner_mut());
    owner.schedule_build_for(parent, 0, RebuildReason::ParentUpdate);
    owner.build_scope(&mut tree);
    owner.finalize_tree(&mut tree);
    assert!(!tree.contains(next_id));
    assert!(owner.take_recovered_panics().is_empty());
    assert_eq!(observed.calls.get(), 1);
    if !matches!(hook, Hook::RenderUnmount) {
        assert_eq!(observed.disposed.get(), 2);
    }
}
fn run(kind: &str) {
    super::child_payload_recovery::run_child("FLUI_REMOVAL_HOOK_PAYLOAD_CHILD", kind);
}
pub(crate) fn dispose_payload_is_retained_after_containment() {
    run("dispose");
}
pub(crate) fn deactivate_payload_is_retained_after_containment() {
    run("deactivate");
}
pub(crate) fn render_unmount_payload_is_retained_after_containment() {
    run("render");
}
pub(crate) fn dispose_and_reporting_payloads_cannot_compete() {
    run("dispose_report");
}
pub(crate) fn deactivate_and_reporting_payloads_cannot_compete() {
    run("deactivate_report");
}
pub(crate) fn render_unmount_and_reporting_payloads_cannot_compete() {
    run("render_report");
}
pub(crate) fn dispose_payload_cannot_replace_incoming_unwind() {
    run("dispose_incoming");
}
pub(crate) fn deactivate_payload_cannot_replace_incoming_unwind() {
    run("deactivate_incoming");
}
pub(crate) fn render_unmount_payload_cannot_replace_incoming_unwind() {
    run("render_incoming");
}
