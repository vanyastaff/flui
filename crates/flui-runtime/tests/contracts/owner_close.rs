use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::PresentationAddress;
use flui_runtime::owner::{
    Delivery, DispatchError, FrameDispatcher, OwnerEffects, OwnerHost, PresentationDispatcher,
    PublicationError,
};
use flui_runtime::ui_runtime::UiRuntime;

use crate::owner_publication::{runtime, window};

fn close_fences_new_work_but_preserves_earlier_frames_and_the_surviving_runtime() {
    struct Effects {
        owner: OwnerHost,
        close: PresentationDispatcher,
        first: FrameDispatcher,
        sibling: FrameDispatcher,
        first_address: PresentationAddress,
        sibling_address: PresentationAddress,
        seeded: Cell<bool>,
        pending: RefCell<Option<flui_runtime::owner::PreparedInstall>>,
        trace: RefCell<Vec<&'static str>>,
    }
    impl OwnerEffects for Effects {
        fn finish_install(
            &self,
            _: flui_foundation::PresentationAddress,
            _: flui_runtime::owner::InitializationOutcome,
            _: flui_runtime::owner::RecoveryState,
        ) {
        }
        fn runtime_lifecycle(
            &self,
            _: flui_foundation::UiRuntimeId,
            _: flui_scheduler::AppLifecycleState,
        ) {
        }
        fn runtimes_stopped(&self, _: flui_runtime::owner::RecoveryState) {}
        fn frame(&self, address: PresentationAddress, runtime: &mut UiRuntime) {
            assert_eq!(runtime.id(), address.ui_runtime_id);
            self.trace
                .borrow_mut()
                .push(if address == self.first_address {
                    "first frame"
                } else {
                    "sibling frame"
                });
            if !self.seeded.replace(true) {
                assert_eq!(self.first.deliver(self), Ok(Delivery::Queued));
                assert_eq!(self.close.close(self), Ok(Delivery::Queued));
                assert_eq!(
                    self.first.deliver(self),
                    Err(DispatchError::PresentationClosing)
                );
                assert_eq!(
                    self.close.close(self),
                    Err(DispatchError::PresentationClosing)
                );
                let refused = self
                    .owner
                    .prepare_presentation(self.first_address, window())
                    .expect_err("closing authorizer");
                assert_eq!(refused.error, PublicationError::PresentationClosing);
                drop(refused);
            }
        }
        fn commit_install(
            &self,
            _: flui_runtime::owner::InstallToken,
            _: flui_runtime::owner::RecoveryState,
        ) -> Option<flui_runtime::owner::InstallInitialization> {
            panic!("no pending install");
        }
        fn retire_host(
            &self,
            _: &[flui_foundation::PresentationAddress],
            _: flui_runtime::owner::RecoveryState,
        ) {
        }
        fn cancel_install(
            &self,
            _: flui_runtime::owner::InstallToken,
            _: flui_runtime::owner::RecoveryState,
        ) {
            panic!("no pending install");
        }
        fn resize_surface(
            &self,
            _: flui_foundation::PresentationAddress,
            _: flui_foundation::geometry::Size<f64>,
            _: f64,
        ) {
        }
        fn retire_presentation(
            &self,
            address: PresentationAddress,
            surviving: Option<PresentationAddress>,
        ) {
            assert_eq!(address, self.first_address);
            assert_eq!(surviving, Some(self.sibling_address));
            assert_eq!(
                self.first.deliver(self),
                Err(DispatchError::UnknownPresentation)
            );
            assert_eq!(self.owner.runtime_count(), 1);
            self.trace.borrow_mut().push("native retirement");
            assert_eq!(self.sibling.deliver(self), Ok(Delivery::Queued));
        }
        fn after_turn(&self, _: flui_runtime::owner::RecoveryState) {
            if let Some(prepared) = self.pending.take() {
                let refused = self
                    .owner
                    .publication(prepared)
                    .expect_err("prepared sibling lost its authorizer at close admission");
                assert_eq!(refused.error, PublicationError::PresentationClosing);
            }
        }
        fn request_continuation(&self) -> bool {
            panic!("finite work fits the callback");
        }
    }
    let owner = OwnerHost::new();
    let first = owner
        .publication(owner.prepare_runtime(runtime()))
        .expect("runtime")
        .commit();
    let proposal = owner
        .prepare_presentation(first, window())
        .expect("sibling");
    let sibling = owner
        .publication(proposal)
        .expect("publish sibling")
        .commit();
    let effects = Effects {
        owner: owner.clone(),
        close: owner
            .presentation_dispatcher(first)
            .expect("close authority"),
        first: owner.frame_dispatcher(first).expect("first"),
        sibling: owner.frame_dispatcher(sibling).expect("sibling"),
        first_address: first,
        sibling_address: sibling,
        seeded: Cell::new(false),
        pending: RefCell::new(Some(
            owner
                .prepare_presentation(first, window())
                .expect("prepare before close admission"),
        )),
        trace: RefCell::new(Vec::new()),
    };
    effects.first.deliver(&effects).expect("first turn");
    assert_eq!(
        *effects.trace.borrow(),
        [
            "first frame",
            "first frame",
            "native retirement",
            "sibling frame"
        ]
    );
    owner
        .runtime_dispatcher(first.ui_runtime_id)
        .expect("runtime authority survives primary close");
    assert_eq!(
        owner
            .runtime_status(first.ui_runtime_id)
            .expect("surviving runtime")
            .primary,
        sibling
    );
    owner
        .prepare_presentation(sibling, window())
        .expect("surviving authorizer works");
}

fn native_close_failure_preserves_first_error_and_does_not_restore_the_runtime() {
    struct Capture;
    impl Drop for Capture {
        fn drop(&mut self) {
            panic!("callback retirement");
        }
    }
    struct Effects {
        owner: OwnerHost,
        address: PresentationAddress,
        retired: Rc<Cell<bool>>,
    }
    impl OwnerEffects for Effects {
        fn finish_install(
            &self,
            _: flui_foundation::PresentationAddress,
            _: flui_runtime::owner::InitializationOutcome,
            _: flui_runtime::owner::RecoveryState,
        ) {
        }
        fn runtime_lifecycle(
            &self,
            _: flui_foundation::UiRuntimeId,
            _: flui_scheduler::AppLifecycleState,
        ) {
        }
        fn runtimes_stopped(&self, _: flui_runtime::owner::RecoveryState) {}
        fn frame(&self, _: PresentationAddress, _: &mut UiRuntime) {
            panic!("no frame requested");
        }
        fn commit_install(
            &self,
            _: flui_runtime::owner::InstallToken,
            _: flui_runtime::owner::RecoveryState,
        ) -> Option<flui_runtime::owner::InstallInitialization> {
            panic!("no pending install");
        }
        fn retire_host(
            &self,
            _: &[flui_foundation::PresentationAddress],
            _: flui_runtime::owner::RecoveryState,
        ) {
        }
        fn cancel_install(
            &self,
            _: flui_runtime::owner::InstallToken,
            _: flui_runtime::owner::RecoveryState,
        ) {
            panic!("no pending install");
        }
        fn resize_surface(
            &self,
            _: flui_foundation::PresentationAddress,
            _: flui_foundation::geometry::Size<f64>,
            _: f64,
        ) {
        }
        fn retire_presentation(
            &self,
            address: PresentationAddress,
            surviving: Option<PresentationAddress>,
        ) {
            assert_eq!(address, self.address);
            assert_eq!(surviving, None);
            assert_eq!(self.owner.runtime_count(), 0);
            self.retired.set(true);
            panic!("native retirement");
        }
        fn after_turn(&self, _: flui_runtime::owner::RecoveryState) {
            assert!(self.retired.get());
        }
        fn request_continuation(&self) -> bool {
            panic!("no queued work");
        }
    }
    let owner = OwnerHost::new();
    let runtime = runtime();
    let capture = Capture;
    runtime
        .owner_frame()
        .local_post_frame_handle()
        .schedule_local(move |_| drop(capture))
        .expect("capture");
    let address = owner
        .publication(owner.prepare_runtime(runtime))
        .expect("runtime")
        .commit();
    let effects = Effects {
        owner: owner.clone(),
        address,
        retired: Rc::new(Cell::new(false)),
    };
    let closing = owner
        .presentation_dispatcher(address)
        .expect("close authority");
    let failure =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| closing.close(&effects)))
            .expect_err("first failure resumes");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"native retirement"));
    assert_eq!(owner.runtime_count(), 0);
    assert_eq!(closing.close(&effects), Err(DispatchError::UnknownRuntime));
    let next = owner
        .publication(owner.prepare_runtime(crate::owner_publication::runtime()))
        .expect("next install")
        .commit();
    assert_eq!(owner.runtime_count(), 1);
    owner
        .frame_dispatcher(next)
        .expect("next runtime is routable");
}

#[test]
fn owner_close_contract() {
    crate::table_test::run_table(
        "owner_close_contract",
        &[
            (
                "close_fences_new_work_but_preserves_earlier_frames_and_the_surviving_runtime",
                close_fences_new_work_but_preserves_earlier_frames_and_the_surviving_runtime
                    as fn(),
            ),
            (
                "native_close_failure_preserves_first_error_and_does_not_restore_the_runtime",
                native_close_failure_preserves_first_error_and_does_not_restore_the_runtime
                    as fn(),
            ),
        ],
    );
}
