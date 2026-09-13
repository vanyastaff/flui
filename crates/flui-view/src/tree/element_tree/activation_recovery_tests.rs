//! Transaction-failure coverage for behavior-attributed activation recovery.

use std::any::TypeId;

use flui_foundation::panic::payload_text;

use super::*;
use crate::view::{
    FlutterError, clear_error_view_builder, isolate_error_view_builder_test, set_error_view_builder,
};

struct ResetErrorViewBuilder;

impl Drop for ResetErrorViewBuilder {
    fn drop(&mut self) {
        clear_error_view_builder();
    }
}

#[derive(Clone)]
struct RecoveryMountPanics;

impl RenderView for RecoveryMountPanics {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = flui_objects::RenderSizedBox;

    fn create_render_object(&self, _ctx: &crate::RenderObjectContext<'_>) -> Self::RenderObject {
        panic!("activation substitute mount panic");
    }

    fn update_render_object(
        &self,
        _ctx: &crate::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for RecoveryMountPanics {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::render_variable(self)
    }
}

fn recovery_mount_panics(_error: &FlutterError) -> Box<dyn View> {
    Box::new(RecoveryMountPanics)
}

fn recovery_factory_panics(_error: &FlutterError) -> Box<dyn View> {
    panic!("activation recovery factory panic");
}

fn active_retake_fixture() -> (
    ElementTree,
    BuildOwner,
    ActiveRetakePanicsOnActivate,
    ElementId,
    ElementId,
) {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let pipeline = PipelineCell::new(flui_rendering::pipeline::PipelineOwner::new());
    let root = tree.mount_root_with_pipeline_owner(
        &UnitRenderHost,
        Some(pipeline),
        &mut owner.element_owner_mut(),
    );
    let donor = tree.insert(&UnitRenderHost, root, 0, &mut owner.element_owner_mut());
    let destination = tree.insert(&UnitRenderHost, root, 1, &mut owner.element_owner_mut());
    tree.get_mut(root)
        .expect("root remains live")
        .set_child_ids(vec![donor, destination]);

    let armed = std::rc::Rc::new(std::cell::Cell::new(false));
    let keyed = ActiveRetakePanicsOnActivate {
        key: GlobalKey::new(),
        armed: std::rc::Rc::clone(&armed),
    };
    let candidate = tree.insert(&keyed, donor, 0, &mut owner.element_owner_mut());
    tree.get_mut(donor)
        .expect("donor remains live")
        .set_child_ids(vec![candidate]);
    let depth = tree.get(candidate).expect("candidate remains live").depth();
    owner.schedule_build_for(candidate, depth, crate::RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    armed.set(true);
    (tree, owner, keyed, destination, root)
}

fn seed_prior_record(owner: &mut BuildOwner, element: ElementId) {
    let payload: Box<dyn std::any::Any + Send> = Box::new("prior recovery");
    owner.element_owner_mut().record_hook_panic(
        Some(element),
        None,
        TypeId::of::<UnitRenderHost>(),
        LifecycleHook::Build,
        payload.as_ref(),
        "prior committed recovery",
    );
}

fn assert_failed_activation_diagnostic(
    owner: &mut BuildOwner,
    prior: ElementId,
    captured_log: &flui_testing::log_capture::CapturedLog,
) {
    assert_eq!(
        captured_log.count_containing("recovery transaction pending"),
        1
    );
    assert_eq!(
        captured_log.count_containing("lifecycle hook panicked; contained"),
        0,
        "an uncommitted recovery must not emit the committed-path trace: {captured_log}"
    );
    let recovered = owner.take_recovered_panics();
    assert_eq!(recovered.len(), 1, "only the prior record is committed");
    assert_eq!(recovered[0].element(), Some(prior));
    assert!(owner.lifecycle_panic_handoff.take().is_disarmed());
}

#[test]
fn activation_factory_panic_drops_staged_record_and_preserves_prior_record() {
    const TEST_NAME: &str = "tree::element_tree::tests::activation_recovery_tests::activation_factory_panic_drops_staged_record_and_preserves_prior_record";
    if isolate_error_view_builder_test(TEST_NAME) {
        return;
    }
    clear_error_view_builder();
    let _reset = ResetErrorViewBuilder;
    let (mut tree, mut owner, keyed, destination, prior) = active_retake_fixture();
    seed_prior_record(&mut owner, prior);
    set_error_view_builder(recovery_factory_panics);

    let (payload, captured_log) = flui_testing::log_capture::capture(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = tree.begin_reconcile(destination);
            let _ = tree.mount_or_substitute(
                &keyed,
                destination,
                0,
                &mut owner.element_owner_mut(),
                ProvisionalOrder::NONE,
                "active retake factory failure",
            );
        }))
        .expect_err("the configured recovery factory must remain fatal")
    });
    assert_eq!(
        payload_text(payload.as_ref()),
        Some("activation recovery factory panic")
    );
    assert_failed_activation_diagnostic(&mut owner, prior, &captured_log);
}

#[test]
fn activation_substitute_mount_panic_drops_staged_record_and_preserves_prior_record() {
    const TEST_NAME: &str = "tree::element_tree::tests::activation_recovery_tests::activation_substitute_mount_panic_drops_staged_record_and_preserves_prior_record";
    if isolate_error_view_builder_test(TEST_NAME) {
        return;
    }
    clear_error_view_builder();
    let _reset = ResetErrorViewBuilder;
    let (mut tree, mut owner, keyed, destination, prior) = active_retake_fixture();
    seed_prior_record(&mut owner, prior);
    set_error_view_builder(recovery_mount_panics);

    let (payload, captured_log) = flui_testing::log_capture::capture(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = tree.begin_reconcile(destination);
            let _ = tree.mount_or_substitute(
                &keyed,
                destination,
                0,
                &mut owner.element_owner_mut(),
                ProvisionalOrder::NONE,
                "active retake substitute failure",
            );
        }))
        .expect_err("the substitute mount panic must remain fatal")
    });
    assert_eq!(
        payload_text(payload.as_ref()),
        Some("activation substitute mount panic")
    );
    assert_failed_activation_diagnostic(&mut owner, prior, &captured_log);
}
