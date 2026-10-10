use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::PresentationAddress;
use flui_runtime::owner::{
    Delivery, DispatchError, FrameDispatcher, OwnerEffects, OwnerHost, PreparedInstall,
    PublicationError, RuntimeDispatcher, RuntimeOperation,
};
use flui_runtime::ui_runtime::UiRuntime;
use flui_view::prelude::*;

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
        .post_frame_handle()
        .schedule(move |_| drop(capture))
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum TextServiceCase {
    Ready,
    Close,
    Replace,
    Drop,
    Panic,
    Unavailable,
}

#[derive(Clone, StatefulView)]
struct DeliveredAuthoredText {
    size: Rc<Cell<f64>>,
    rebuild: Rc<RefCell<Option<flui_view::RebuildHandle>>>,
}

struct DeliveredAuthoredTextState(Rc<RefCell<Option<flui_view::RebuildHandle>>>);

impl StatefulView for DeliveredAuthoredText {
    type State = DeliveredAuthoredTextState;

    fn create_state(&self) -> Self::State {
        DeliveredAuthoredTextState(Rc::clone(&self.rebuild))
    }
}

impl ViewState<DeliveredAuthoredText> for DeliveredAuthoredTextState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.0.borrow_mut() = Some(ctx.rebuild_handle());
    }

    fn build(&self, view: &DeliveredAuthoredText, _: &dyn BuildContext) -> impl IntoView {
        let text = if view.size.get() == 14.0 {
            "native sizing owner"
        } else {
            "replacement text"
        };
        flui_widgets::Text::new(text)
            .style(flui_painting::typography::TextStyle::default().with_font_size(view.size.get()))
    }
}

fn detached_text_service_returns_the_runtime_before_native_reentry(case: TextServiceCase) {
    struct Sink<'a> {
        scenes: &'a Cell<usize>,
        painted: &'a RefCell<Vec<(String, Vec<f64>)>>,
    }
    impl flui_runtime::sink::FrameSink for Sink<'_> {
        fn surface_size(&mut self) -> (u32, u32) {
            (400, 400)
        }
        fn submit(&mut self, scene: flui_layer::Scene) -> flui_runtime::sink::SubmitVerdict {
            let mut registry = flui_painting::glyphs::FontRegistry::with_owned_sources();
            let mut painted = Vec::new();
            for (_, node) in scene.tree().iter() {
                let flui_layer::Layer::Picture(picture) = node.layer() else {
                    continue;
                };
                for command in picture.picture() {
                    let flui_painting::DrawOp::Paragraph { paragraph, .. } = &command.op else {
                        continue;
                    };
                    let mut sizes = Vec::new();
                    for run in paragraph.runs() {
                        let key = registry.prepare_run(&run).expect("actual submitted font");
                        sizes.extend(
                            run.placed_glyphs(key, (0.0, 0.0), 1.0)
                                .map(|glyph| f64::from(glyph.key.size())),
                        );
                    }
                    painted.push((paragraph.text().to_owned(), sizes));
                }
            }
            self.painted.replace(painted);
            self.scenes.set(self.scenes.get() + 1);
            flui_runtime::sink::SubmitVerdict::Presented
        }
    }
    struct Effects {
        owner: OwnerHost,
        admission: RefCell<flui_painting::TextSizingAdmission>,
        scenes: Cell<usize>,
        requests: Cell<usize>,
        painted: RefCell<Vec<(String, Vec<f64>)>>,
        case: TextServiceCase,
        authored_size: Rc<Cell<f64>>,
        rebuild: Rc<RefCell<Option<flui_view::RebuildHandle>>>,
    }
    impl OwnerEffects for Effects {
        fn runtime_lifecycle(
            &self,
            _: flui_foundation::UiRuntimeId,
            _: flui_scheduler::AppLifecycleState,
        ) {
        }
        fn runtimes_stopped(&self, _: flui_runtime::owner::RecoveryState) {}
        fn retire_host(&self, _: &[PresentationAddress], _: flui_runtime::owner::RecoveryState) {}
        fn commit_install(
            &self,
            _: flui_runtime::owner::InstallToken,
            _: flui_runtime::owner::RecoveryState,
        ) -> Option<flui_runtime::owner::InstallInitialization> {
            panic!("this fixture publishes its owner before frame delivery");
        }
        fn finish_install(
            &self,
            _: PresentationAddress,
            _: flui_runtime::owner::InitializationOutcome,
            _: flui_runtime::owner::RecoveryState,
        ) {
        }
        fn cancel_install(
            &self,
            _: flui_runtime::owner::InstallToken,
            _: flui_runtime::owner::RecoveryState,
        ) {
        }
        fn resize_surface(
            &self,
            _: PresentationAddress,
            _: flui_foundation::geometry::Size<f64>,
            _: f64,
        ) {
        }
        fn frame(&self, _: PresentationAddress, runtime: &mut UiRuntime) {
            let _ = runtime.pump(
                &mut flui_runtime::pump::SampledClock(web_time::Instant::now()),
                &mut Sink {
                    scenes: &self.scenes,
                    painted: &self.painted,
                },
            );
        }
        fn retire_presentation(&self, _: PresentationAddress, _: Option<PresentationAddress>) {}
        fn after_turn(&self, _: flui_runtime::owner::RecoveryState) {}
        fn request_continuation(&self) -> bool {
            true
        }
        fn text_sizing(
            &self,
            frontier: flui_runtime::owner::TextSizingFrontier,
            _: flui_runtime::owner::RecoveryState,
        ) -> Option<flui_runtime::owner::TextSizingFrontier> {
            self.requests.set(self.requests.get() + 1);
            assert_eq!(self.scenes.get(), 0, "partial geometry was not submitted");
            let proposal = self
                .owner
                .prepare_presentation(frontier.address(), window())
                .expect("native service can assemble a sibling");
            let publication = self
                .owner
                .publication(proposal)
                .expect("runtime checkout returned before native service");
            drop(publication);
            if self.case == TextServiceCase::Close {
                self.owner
                    .presentation_dispatcher(frontier.address())
                    .expect("current native service target")
                    .close(self)
                    .expect("native reentry closes the exact presentation");
                assert_eq!(
                    frontier.settle(flui_runtime::ui_runtime::TextSizingSettlement::Ready, self),
                    Err(DispatchError::PresentationClosing),
                    "a late numeric reply cannot cross close admission"
                );
            } else {
                if self.requests.get() == 1 {
                    if self.case == TextServiceCase::Drop {
                        return None;
                    }
                    assert!(
                        self.case != TextServiceCase::Panic,
                        "native sizing service failed after receiving the frontier"
                    );
                    if self.case == TextServiceCase::Replace {
                        let size = Rc::clone(&self.authored_size);
                        let rebuild = Rc::clone(&self.rebuild);
                        assert_eq!(
                            self.owner
                                .presentation_dispatcher(frontier.address())
                                .expect("addressed authored replacement")
                                .test_callback(
                                    Box::new(move |_| {
                                        size.set(22.75);
                                        let handle = rebuild
                                            .borrow()
                                            .as_ref()
                                            .expect("mounted authored state's lifecycle handle")
                                            .clone();
                                        handle
                                            .schedule(flui_foundation::RebuildReason::StateChange);
                                    }),
                                    self
                                ),
                            Ok(Delivery::Queued),
                            "authored input is accepted before the old numeric settlement"
                        );
                    }
                }
                let answers: Vec<_> = frontier
                    .requests()
                    .iter()
                    .map(|request| {
                        (
                            *request,
                            flui_foundation::TextSize::new(request.size.value() * 1.5)
                                .expect("finite native answer"),
                        )
                    })
                    .collect();
                self.admission
                    .borrow_mut()
                    .admit(answers)
                    .expect("host admits into its sole numeric source");
                frontier
                    .settle(flui_runtime::ui_runtime::TextSizingSettlement::Ready, self)
                    .expect("same owner accepts the detached reply");
            }
            None
        }
    }
    struct DefaultEffects<'a>(&'a Effects);
    impl OwnerEffects for DefaultEffects<'_> {
        fn runtime_lifecycle(
            &self,
            id: flui_foundation::UiRuntimeId,
            state: flui_scheduler::AppLifecycleState,
        ) {
            self.0.runtime_lifecycle(id, state);
        }
        fn runtimes_stopped(&self, recovery: flui_runtime::owner::RecoveryState) {
            self.0.runtimes_stopped(recovery);
        }
        fn retire_host(
            &self,
            addresses: &[PresentationAddress],
            recovery: flui_runtime::owner::RecoveryState,
        ) {
            self.0.retire_host(addresses, recovery);
        }
        fn commit_install(
            &self,
            token: flui_runtime::owner::InstallToken,
            recovery: flui_runtime::owner::RecoveryState,
        ) -> Option<flui_runtime::owner::InstallInitialization> {
            self.0.commit_install(token, recovery)
        }
        fn finish_install(
            &self,
            address: PresentationAddress,
            outcome: flui_runtime::owner::InitializationOutcome,
            recovery: flui_runtime::owner::RecoveryState,
        ) {
            self.0.finish_install(address, outcome, recovery);
        }
        fn cancel_install(
            &self,
            token: flui_runtime::owner::InstallToken,
            recovery: flui_runtime::owner::RecoveryState,
        ) {
            self.0.cancel_install(token, recovery);
        }
        fn resize_surface(
            &self,
            address: PresentationAddress,
            size: flui_foundation::geometry::Size<f64>,
            scale: f64,
        ) {
            self.0.resize_surface(address, size, scale);
        }
        fn frame(&self, address: PresentationAddress, runtime: &mut UiRuntime) {
            self.0.frame(address, runtime);
        }
        fn retire_presentation(
            &self,
            address: PresentationAddress,
            surviving: Option<PresentationAddress>,
        ) {
            self.0.retire_presentation(address, surviving);
        }
        fn after_turn(&self, recovery: flui_runtime::owner::RecoveryState) {
            self.0.after_turn(recovery);
        }
        fn request_continuation(&self) -> bool {
            self.0.request_continuation()
        }
        // Deliberately inherit OwnerEffects::text_sizing's default refusal.
    }
    let owner = OwnerHost::new();
    let host = flui_testing::RecordingTextStoreHost::new();
    let focus = flui_interaction::routing::FocusNode::new();
    let authored_size = Rc::new(Cell::new(14.0));
    let rebuild = Rc::new(RefCell::new(None));
    let runtime = if case == TextServiceCase::Unavailable {
        UiRuntime::new(
            window().with_text_store_host(Some(host.clone())),
            1.0,
            flui_runtime::ui_runtime::RuntimeHostServices::new(
                std::sync::Arc::new(|| {}),
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                std::sync::Arc::new(flui_platform_api::InMemoryClipboard::new()),
                &flui_painting::FontCollection::new(),
                flui_scheduler::ClockSource::Platform,
            ),
        )
        .expect("runtime with real text-store host")
    } else {
        runtime()
    };
    if case == TextServiceCase::Unavailable {
        runtime
            .attach_root_widget_with_size(
                &flui_widgets::EditableText::new(
                    flui_widgets::TextEditingController::with_text("native sizing owner"),
                    Rc::clone(&focus),
                )
                .text_style(flui_painting::typography::TextStyle::default().with_font_size(14.0)),
                400.0,
                400.0,
            )
            .expect("mount real editable document");
    } else if case == TextServiceCase::Replace {
        runtime
            .attach_root_widget_with_size(
                &DeliveredAuthoredText {
                    size: Rc::clone(&authored_size),
                    rebuild: Rc::clone(&rebuild),
                },
                400.0,
                400.0,
            )
            .expect("mount retained authored state before native delivery");
    } else {
        runtime
            .attach_root_widget_with_size(
                &flui_widgets::Text::new("native sizing owner"),
                400.0,
                400.0,
            )
            .expect("mount actual text before native delivery");
    }
    let address = install(&owner, runtime);
    let (_, admission) = flui_painting::TextSizing::captured();
    let effects = Effects {
        owner: owner.clone(),
        admission: RefCell::new(admission),
        scenes: Cell::new(0),
        requests: Cell::new(0),
        painted: RefCell::new(Vec::new()),
        case,
        authored_size,
        rebuild,
    };
    let defaults = DefaultEffects(&effects);
    let dispatcher = owner.frame_dispatcher(address).expect("frame authority");
    if case == TextServiceCase::Unavailable {
        dispatcher
            .deliver(&defaults)
            .expect("commit original editable geometry");
        let _ = focus.request_focus();
        dispatcher
            .deliver(&defaults)
            .expect("attach the focused native text store");
        assert!(host.focused_store().is_some());
        effects.scenes.set(0);
        effects.painted.borrow_mut().clear();
    }
    let source = effects.admission.borrow().source();
    let delivery_effects: &dyn OwnerEffects = if case == TextServiceCase::Unavailable {
        &defaults
    } else {
        &effects
    };
    owner
        .presentation_dispatcher(address)
        .expect("captured policy authority")
        .install_captured_text_sizing(source.clone(), delivery_effects)
        .expect("install the native owner's read source");
    let delivered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        dispatcher.deliver(delivery_effects)
    }));
    if case == TextServiceCase::Panic {
        let failure = delivered.expect_err("native failure remains authoritative");
        assert_eq!(
            failure.downcast_ref::<&str>(),
            Some(&"native sizing service failed after receiving the frontier")
        );
    } else {
        delivered
            .expect("healthy detached effect")
            .expect("drive missing text");
    }
    assert_eq!(
        effects.requests.get(),
        usize::from(case != TextServiceCase::Unavailable)
    );
    assert_eq!(effects.scenes.get(), 0);
    if case == TextServiceCase::Unavailable {
        use flui_platform_api::text_store::{
            LockGrant, LockOutcome, LockTiming, TextStoreError, Utf16Offset, Utf16Range,
        };
        for _ in 0..3 {
            dispatcher
                .deliver(&defaults)
                .expect("unchanged unavailable source remains parked");
        }
        assert_eq!(effects.scenes.get(), 0);
        let store = host
            .focused_store()
            .expect("park retains document editing authority");
        assert_eq!(
            store.request_lock(
                LockGrant::read_write(|session| {
                    assert_eq!(
                        session.rect_for_range(Utf16Range::collapsed(Utf16Offset::new(0))),
                        Err(TextStoreError::NoLayout)
                    );
                    let end = session.document_len();
                    session
                        .replace(
                            Utf16Range::new(Utf16Offset::new(0), end)
                                .expect("the whole document is an ordered range"),
                            "edited document",
                        )
                        .expect("document edit can repair parked geometry");
                }),
                LockTiming::Sync
            ),
            Ok(LockOutcome::Granted)
        );
        dispatcher
            .deliver(&defaults)
            .expect("changed document remains unavailable without a producer");
        assert_eq!(effects.scenes.get(), 0);
        owner
            .presentation_dispatcher(address)
            .expect("exact native readiness target")
            .service_text_sizing_source(&source, &effects)
            .expect("actual native authority now accepts the frontier");
        dispatcher
            .deliver(&effects)
            .expect("edited document resumes after numeric admission");
        assert_eq!(effects.scenes.get(), 1);
        assert_eq!(
            effects.requests.get(),
            1,
            "default effects never called an invented native producer"
        );
        let painted = effects.painted.borrow();
        let (_, sizes) = painted
            .iter()
            .find(|(text, _)| text == "edited document")
            .expect("edited document reaches submitted geometry");
        assert_ne!(sizes.as_slice(), [] as [f64; 0]);
        assert!(sizes.iter().all(|size| (*size - 21.0).abs() < 0.001));
        return;
    }
    if case == TextServiceCase::Close {
        assert!(matches!(
            dispatcher.deliver(&effects),
            Err(DispatchError::UnknownPresentation | DispatchError::UnknownRuntime)
        ));
    } else {
        if matches!(case, TextServiceCase::Drop | TextServiceCase::Panic) {
            owner
                .presentation_dispatcher(address)
                .expect("retained native authority")
                .service_text_sizing_source(&source, &effects)
                .expect("explicit ready opportunity recovers the accepted tail");
        }
        dispatcher.deliver(&effects).expect("resume admitted text");
        if case == TextServiceCase::Replace {
            assert_eq!(
                effects.scenes.get(),
                0,
                "old receipt cannot submit obsolete authored geometry"
            );
            assert_eq!(
                effects.requests.get(),
                2,
                "replacement has its own authored frontier"
            );
            dispatcher
                .deliver(&effects)
                .expect("complete replacement text");
        }
        assert_eq!(effects.scenes.get(), 1);
        let painted = effects.painted.borrow();
        let (text, expected_size) = if case == TextServiceCase::Replace {
            ("replacement text", 34.125)
        } else {
            ("native sizing owner", 21.0)
        };
        let (_, sizes) = painted
            .iter()
            .find(|(actual, _)| actual == text)
            .expect("intended text was actually submitted");
        assert_ne!(sizes.as_slice(), [] as [f64; 0]);
        assert!(
            sizes
                .iter()
                .all(|size| (*size - expected_size).abs() < 0.001)
        );
        assert_eq!(
            effects.requests.get(),
            if matches!(
                case,
                TextServiceCase::Replace | TextServiceCase::Drop | TextServiceCase::Panic
            ) {
                2
            } else {
                1
            },
            "the accepted frontier settled once"
        );
    }
}

fn detached_text_service_can_publish_after_checkout() {
    detached_text_service_returns_the_runtime_before_native_reentry(TextServiceCase::Ready);
}

fn detached_text_service_reentry_cannot_settle_a_closed_presentation() {
    detached_text_service_returns_the_runtime_before_native_reentry(TextServiceCase::Close);
}

fn detached_text_service_cannot_commit_obsolete_authored_text() {
    detached_text_service_returns_the_runtime_before_native_reentry(TextServiceCase::Replace);
}

fn detached_text_service_recovers_dropped_and_panicking_receipts() {
    for case in [TextServiceCase::Drop, TextServiceCase::Panic] {
        detached_text_service_returns_the_runtime_before_native_reentry(case);
    }
}

fn default_text_service_parks_geometry_but_allows_document_repair() {
    detached_text_service_returns_the_runtime_before_native_reentry(TextServiceCase::Unavailable);
}

#[test]
fn owner_delivery_contract() {
    crate::table_test::run_table(
        "owner_delivery_contract",
        &[
            (
                "default_text_service_parks_geometry_but_allows_document_repair",
                default_text_service_parks_geometry_but_allows_document_repair as fn(),
            ),
            (
                "detached_text_service_cannot_commit_obsolete_authored_text",
                detached_text_service_cannot_commit_obsolete_authored_text as fn(),
            ),
            (
                "detached_text_service_recovers_dropped_and_panicking_receipts",
                detached_text_service_recovers_dropped_and_panicking_receipts as fn(),
            ),
            (
                "detached_text_service_can_publish_after_checkout",
                detached_text_service_can_publish_after_checkout as fn(),
            ),
            (
                "detached_text_service_reentry_cannot_settle_a_closed_presentation",
                detached_text_service_reentry_cannot_settle_a_closed_presentation as fn(),
            ),
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
