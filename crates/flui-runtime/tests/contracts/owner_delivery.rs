use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::PresentationAddress;
use flui_runtime::owner::{
    Delivery, DispatchError, FrameDispatcher, OwnerEffects, OwnerHost, PreparedInstall,
    PublicationError, RuntimeDispatcher, RuntimeOperation,
};
use flui_runtime::ui_runtime::UiRuntime;

use crate::owner_publication::{runtime, window};

fn install(owner: &OwnerHost, runtime: UiRuntime) -> PresentationAddress {
    owner
        .publication(owner.prepare_runtime(runtime))
        .expect("publish runtime")
        .commit()
}

fn shared_preparation_survives_checkout_and_publishes_after_return() {
    struct Effects {
        owner: OwnerHost,
        pending: RefCell<Option<PreparedInstall>>,
        published: Cell<Option<PresentationAddress>>,
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
            let prepared = self
                .owner
                .prepare_presentation(address, window())
                .expect("assembly capabilities survive checkout");
            let refused = self
                .owner
                .publication(prepared)
                .expect_err("lease is active");
            assert_eq!(refused.error, PublicationError::Busy);
            self.pending.replace(Some(refused.prepared));
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
            _: flui_foundation::PresentationAddress,
            _: Option<flui_foundation::PresentationAddress>,
        ) {
        }
        fn after_turn(&self, _: flui_runtime::owner::RecoveryState) {
            let pending = self.pending.take().expect("complete proposal retained");
            let address = self
                .owner
                .publication(pending)
                .expect("lease returned")
                .commit();
            self.published.set(Some(address));
        }

        fn request_continuation(&self) -> bool {
            panic!("no work remains");
        }
    }
    let owner = OwnerHost::new();
    let primary = install(&owner, runtime());
    let effects = Effects {
        owner: owner.clone(),
        pending: RefCell::new(None),
        published: Cell::new(None),
    };
    owner
        .frame_dispatcher(primary)
        .expect("frame authority")
        .deliver(&effects)
        .expect("drive");
    let sibling = effects.published.get().expect("sibling published");
    assert_eq!(primary.ui_runtime_id, sibling.ui_runtime_id);
    assert_ne!(primary.presentation_id, sibling.presentation_id);
    owner
        .frame_dispatcher(sibling)
        .expect("sibling is routable");
}

fn shutdown_withdraws_admission_before_leased_runtime_retires() {
    struct Capture(Rc<Cell<usize>>);
    impl Drop for Capture {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    struct Effects {
        owner: OwnerHost,
        dispatcher: FrameDispatcher,
        retired: Rc<Cell<usize>>,
        completed: Cell<bool>,
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
            let phase = self
                .owner
                .phase()
                .expect("active phase")
                .expect("leased runtime");
            self.owner.shutdown(self);
            assert_eq!(self.owner.runtime_count(), 0);
            assert_eq!(self.owner.runtime_ids().expect("closed membership"), []);
            assert_eq!(
                self.owner.phase(),
                Ok(Some(phase)),
                "retiring frame remains visible to the platform fence"
            );
            assert_eq!(self.owner.next_wake(), Err(DispatchError::Busy));
            assert_eq!(self.dispatcher.deliver(self), Err(DispatchError::Closed));
            assert_eq!(
                self.retired.get(),
                0,
                "frame still owns the checked-out runtime"
            );
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
            _: flui_foundation::PresentationAddress,
            _: Option<flui_foundation::PresentationAddress>,
        ) {
        }
        fn after_turn(&self, _: flui_runtime::owner::RecoveryState) {
            assert_eq!(self.retired.get(), 1, "lease retired before completion");
            self.completed.set(true);
        }
        fn request_continuation(&self) -> bool {
            panic!("shutdown leaves no deliverable work");
        }
    }
    let owner = OwnerHost::new();
    let runtime = runtime();
    let retired = Rc::new(Cell::new(0));
    let capture = Capture(Rc::clone(&retired));
    runtime
        .owner_frame()
        .local_post_frame_handle()
        .schedule_local(move |_| drop(capture))
        .expect("capture admitted");
    let address = install(&owner, runtime);
    let dispatcher = owner.frame_dispatcher(address).expect("frame authority");
    let effects = Effects {
        owner: owner.clone(),
        dispatcher: dispatcher.clone(),
        retired,
        completed: Cell::new(false),
    };
    assert_eq!(dispatcher.deliver(&effects), Ok(Delivery::Driven));
    assert!(effects.completed.get());
    assert_eq!(owner.runtime_count(), 0);
    assert_eq!(owner.phase(), Ok(None));
    assert_eq!(dispatcher.deliver(&effects), Err(DispatchError::Closed));
}

fn failed_frame_preserves_cross_runtime_fifo_for_the_next_opportunity() {
    struct Effects {
        first: FrameDispatcher,
        second: FrameDispatcher,
        delivered: RefCell<Vec<PresentationAddress>>,
        requests: Cell<usize>,
        fail: Cell<bool>,
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
            assert_eq!(address.ui_runtime_id, runtime.id());
            self.delivered.borrow_mut().push(address);
            if self.fail.replace(false) {
                assert_eq!(self.second.deliver(self), Ok(Delivery::Queued));
                assert_eq!(self.first.deliver(self), Ok(Delivery::Queued));
                panic!("frame failed");
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
            _: flui_foundation::PresentationAddress,
            _: Option<flui_foundation::PresentationAddress>,
        ) {
        }
        fn after_turn(&self, _: flui_runtime::owner::RecoveryState) {}
        fn request_continuation(&self) -> bool {
            self.requests.set(self.requests.get() + 1);
            false
        }
    }
    let owner = OwnerHost::new();
    let first = install(&owner, runtime());
    let second = install(&owner, runtime());
    let effects = Effects {
        first: owner.frame_dispatcher(first).expect("first"),
        second: owner.frame_dispatcher(second).expect("second"),
        delivered: RefCell::new(Vec::new()),
        requests: Cell::new(0),
        fail: Cell::new(true),
    };
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        effects.first.deliver(&effects).expect("frame admitted");
    }))
    .expect_err("frame panic propagates");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"frame failed"));
    assert_eq!(*effects.delivered.borrow(), [first]);
    assert_eq!(effects.requests.get(), 1);
    owner.continue_work(&effects);
    assert_eq!(*effects.delivered.borrow(), [first, second, first]);
    owner.continue_work(&effects);
    assert_eq!(*effects.delivered.borrow(), [first, second, first]);
    assert_eq!(effects.requests.get(), 1);
}

fn bounded_opportunities_preserve_every_frame_when_waking_fails() {
    struct Effects {
        owner: OwnerHost,
        first: FrameDispatcher,
        second: FrameDispatcher,
        delivered: RefCell<Vec<PresentationAddress>>,
        seeded: Cell<bool>,
        requests: Cell<usize>,
        wake: Result<bool, &'static str>,
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
        fn frame(&self, address: PresentationAddress, _: &mut UiRuntime) {
            self.delivered.borrow_mut().push(address);
            if !self.seeded.replace(true) {
                for index in 1..80 {
                    let target = if index % 2 == 0 {
                        &self.first
                    } else {
                        &self.second
                    };
                    assert_eq!(target.deliver(self), Ok(Delivery::Queued));
                }
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
            _: flui_foundation::PresentationAddress,
            _: Option<flui_foundation::PresentationAddress>,
        ) {
        }
        fn after_turn(&self, _: flui_runtime::owner::RecoveryState) {}
        fn request_continuation(&self) -> bool {
            self.requests.set(self.requests.get() + 1);
            self.owner.continue_work(self);
            match self.wake {
                Ok(posted) => posted,
                Err(message) => std::panic::panic_any(message),
            }
        }
    }
    for wake in [Ok(true), Ok(false), Err("wake failed")] {
        let owner = OwnerHost::new();
        let first = install(&owner, runtime());
        let second = install(&owner, runtime());
        let effects = Effects {
            owner: owner.clone(),
            first: owner.frame_dispatcher(first).expect("first"),
            second: owner.frame_dispatcher(second).expect("second"),
            delivered: RefCell::new(Vec::new()),
            seeded: Cell::new(false),
            requests: Cell::new(0),
            wake,
        };
        for expected_count in [32, 64, 80] {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if expected_count == 32 {
                    effects.first.deliver(&effects).expect("initial frame");
                } else {
                    owner.continue_work(&effects);
                }
            }));
            if wake.is_err() && expected_count != 80 {
                assert_eq!(
                    result
                        .expect_err("wake panic propagates")
                        .downcast_ref::<&str>(),
                    Some(&"wake failed")
                );
            } else {
                result.expect("opportunity completes");
            }
            let expected: Vec<_> = (0..expected_count)
                .map(|index| if index % 2 == 0 { first } else { second })
                .collect();
            assert_eq!(*effects.delivered.borrow(), expected);
        }
        assert_eq!(effects.requests.get(), 2);
        owner.continue_work(&effects);
        assert_eq!(effects.delivered.borrow().len(), 80);
        assert_eq!(effects.requests.get(), 2);
    }
}

fn nested_native_callbacks_share_one_delivery_budget() {
    struct Effects {
        delivered: RefCell<Vec<PresentationAddress>>,
        requests: Cell<usize>,
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
        fn frame(&self, address: PresentationAddress, _: &mut UiRuntime) {
            self.delivered.borrow_mut().push(address);
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
            _: flui_foundation::PresentationAddress,
            _: Option<flui_foundation::PresentationAddress>,
        ) {
        }
        fn after_turn(&self, _: flui_runtime::owner::RecoveryState) {}
        fn request_continuation(&self) -> bool {
            self.requests.set(self.requests.get() + 1);
            true
        }
    }
    let owner = OwnerHost::new();
    let first = install(&owner, runtime());
    let second = install(&owner, runtime());
    let first_driver = owner.frame_dispatcher(first).expect("first");
    let second_driver = owner.frame_dispatcher(second).expect("second");
    let effects = Effects {
        delivered: RefCell::new(Vec::new()),
        requests: Cell::new(0),
    };
    {
        let outer = owner.begin_callback(&effects).expect("native callback");
        assert!(!outer.resumes_carried_work());
        for _ in 0..20 {
            assert_eq!(first_driver.deliver(&effects), Ok(Delivery::Driven));
        }
        {
            let nested = owner.begin_callback(&effects).expect("nested callback");
            assert!(!nested.resumes_carried_work());
            for index in 0..20 {
                let expected = if index < 12 {
                    Delivery::Driven
                } else {
                    Delivery::Queued
                };
                assert_eq!(second_driver.deliver(&effects), Ok(expected));
            }
        }
        assert_eq!(effects.delivered.borrow().len(), 32);
        assert_eq!(
            effects.requests.get(),
            0,
            "post only after the physical callback"
        );
    }
    assert_eq!(effects.requests.get(), 1);
    {
        let continuation = owner
            .begin_callback(&effects)
            .expect("continuation callback");
        assert!(continuation.resumes_carried_work());
    }
    let expected: Vec<_> = std::iter::repeat_n(first, 20)
        .chain(std::iter::repeat_n(second, 20))
        .collect();
    assert_eq!(*effects.delivered.borrow(), expected);
    assert_eq!(effects.requests.get(), 1);
}

fn runtime_background_work_shares_frame_order_without_an_extra_frame() {
    struct Effects {
        frame: FrameDispatcher,
        background: RuntimeDispatcher,
        trace: Rc<RefCell<Vec<&'static str>>>,
        seeded: Cell<bool>,
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
            self.trace.borrow_mut().push("frame");
            if !self.seeded.replace(true) {
                assert_eq!(
                    self.background.deliver(RuntimeOperation::Background, self),
                    Ok(Delivery::Queued)
                );
                assert_eq!(self.frame.deliver(self), Ok(Delivery::Queued));
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
            _: flui_foundation::PresentationAddress,
            _: Option<flui_foundation::PresentationAddress>,
        ) {
        }
        fn after_turn(&self, _: flui_runtime::owner::RecoveryState) {}
        fn request_continuation(&self) -> bool {
            panic!("three operations fit one callback");
        }
    }
    let owner = OwnerHost::new();
    let first = install(&owner, runtime());
    let background = runtime();
    let trace = Rc::new(RefCell::new(Vec::new()));
    let task_trace = Rc::clone(&trace);
    let _task = background
        .owner_frame()
        .async_driver()
        .spawn_local(Box::pin(async move {
            task_trace.borrow_mut().push("background");
        }));
    let second = install(&owner, background);
    let effects = Effects {
        frame: owner.frame_dispatcher(first).expect("frame target"),
        background: owner
            .runtime_dispatcher(second.ui_runtime_id)
            .expect("runtime authority"),
        trace,
        seeded: Cell::new(false),
    };
    effects.frame.deliver(&effects).expect("drive shared FIFO");
    assert_eq!(*effects.trace.borrow(), ["frame", "background", "frame"]);
    owner.shutdown(&effects);
    assert_eq!(
        effects
            .background
            .deliver(RuntimeOperation::Background, &effects),
        Err(DispatchError::Closed)
    );
}

#[test]
fn owner_delivery_contract() {
    crate::table_test::run_table(
        "owner_delivery_contract",
        &[
            (
                "runtime_background_work_shares_frame_order_without_an_extra_frame",
                runtime_background_work_shares_frame_order_without_an_extra_frame as fn(),
            ),
            (
                "nested_native_callbacks_share_one_delivery_budget",
                nested_native_callbacks_share_one_delivery_budget as fn(),
            ),
            (
                "bounded_opportunities_preserve_every_frame_when_waking_fails",
                bounded_opportunities_preserve_every_frame_when_waking_fails as fn(),
            ),
            (
                "shared_preparation_survives_checkout_and_publishes_after_return",
                shared_preparation_survives_checkout_and_publishes_after_return as fn(),
            ),
            (
                "shutdown_withdraws_admission_before_leased_runtime_retires",
                shutdown_withdraws_admission_before_leased_runtime_retires as fn(),
            ),
            (
                "failed_frame_preserves_cross_runtime_fifo_for_the_next_opportunity",
                failed_frame_preserves_cross_runtime_fifo_for_the_next_opportunity as fn(),
            ),
        ],
    );
}
