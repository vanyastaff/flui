use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::PresentationAddress;
use flui_runtime::owner::{
    Delivery, DispatchError, FrameDispatcher, OwnerEffects, OwnerHost, PresentationDispatcher,
    PublicationError,
};
use flui_runtime::ui_runtime::UiRuntime;
use flui_view::prelude::StatefulView;

use crate::owner_publication::{runtime, window};

#[derive(Clone, StatefulView)]
struct MotionProbe {
    observation: Rc<RefCell<Option<MotionObservation>>>,
    cancellation_failure: Option<&'static str>,
    closing_peers: Rc<RefCell<Vec<flui_animation::AnimationController>>>,
}

struct MotionObservation {
    registry: flui_animation::Vsync,
    controller: flui_animation::AnimationController,
    run: flui_animation::AnimationRunFuture,
    outcomes: Rc<RefCell<Vec<bool>>>,
}

struct MotionProbeState {
    owner: flui_animation::DrivenController,
    observation: Rc<RefCell<Option<MotionObservation>>>,
    cancellation_failure: Option<&'static str>,
    closing_peers: Rc<RefCell<Vec<flui_animation::AnimationController>>>,
}

impl flui_view::StatefulView for MotionProbe {
    type State = MotionProbeState;

    fn create_state(&self) -> Self::State {
        MotionProbeState {
            owner: flui_animation::AnimationController::builder(std::time::Duration::from_secs(1))
                .build_on(None),
            observation: Rc::clone(&self.observation),
            cancellation_failure: self.cancellation_failure,
            closing_peers: Rc::clone(&self.closing_peers),
        }
    }
}

impl flui_view::ViewState<MotionProbe> for MotionProbeState {
    fn init_state(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        let registry = flui_widgets::VsyncScope::maybe_of(ctx).expect("runtime motion scope");
        self.owner.rebind(Some(&registry)).expect("live registry");
        let run = self.owner.controller().forward().expect("bound owner runs");
        let outcomes = Rc::new(RefCell::new(Vec::new()));
        let recorded = Rc::clone(&outcomes);
        let reentrant = registry.clone();
        let failure = self.cancellation_failure;
        let closing_peers = Rc::clone(&self.closing_peers);
        run.when_complete_or_cancel(move |result| {
            recorded.borrow_mut().push(result.is_err());
            let peers = closing_peers.borrow().clone();
            for peer in peers {
                assert!(
                    matches!(
                        peer.forward(),
                        Err(flui_animation::AnimationError::Disposed)
                    ),
                    "every closing kernel refuses work before cancellation callouts"
                );
            }
            reentrant
                .tick_all(&flui_animation::MotionClock::new().frame(std::time::Duration::ZERO));
            let peer =
                flui_animation::AnimationController::builder(std::time::Duration::from_secs(1))
                    .build_on(Some(&reentrant));
            drop(peer);
            if let Some(failure) = failure {
                std::panic::panic_any(failure);
            }
        });
        self.observation.replace(Some(MotionObservation {
            registry,
            controller: self.owner.controller().clone(),
            run,
            outcomes,
        }));
    }

    fn build(&self, _: &MotionProbe, _: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
        flui_widgets::SizedBox::square(20.0)
    }
}

fn stopping_the_realm_mid_animation_cancels_every_run() {
    use flui_animation::{Animation, MotionClock};
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};
    use std::time::Duration;

    for (addressed, incoming, first_failure, next_failure) in [
        (false, false, None, None),
        (false, false, Some("first cancellation"), None),
        (false, false, None, Some("next cancellation")),
        (
            false,
            false,
            Some("first cancellation"),
            Some("next cancellation"),
        ),
        (false, true, None, None),
        (
            false,
            true,
            Some("first cancellation"),
            Some("next cancellation"),
        ),
        (true, false, None, None),
        (true, false, Some("first cancellation"), None),
    ] {
        let mut runtime = runtime();
        let primary = runtime.presentation_id();
        let second = runtime.assemble_presentation(window());
        let sibling = runtime.install_presentation(second);
        let first = Rc::new(RefCell::new(None));
        let next = Rc::new(RefCell::new(None));
        let closing_peers = Rc::new(RefCell::new(Vec::new()));
        for (id, observation, cancellation_failure) in [
            (primary, Rc::clone(&first), first_failure),
            (sibling, Rc::clone(&next), next_failure),
        ] {
            runtime
                .attach_root_widget_with_size_to(
                    id,
                    &MotionProbe {
                        observation,
                        cancellation_failure,
                        closing_peers: Rc::clone(&closing_peers),
                    },
                    100.0,
                    100.0,
                )
                .expect("mount motion owner");
        }
        runtime.enter(|runtime| {
            for id in [primary, sibling] {
                runtime.presentation_widgets_for_test(id).draw_frame();
            }
        });
        let mut first = first.take().expect("primary mounted");
        let mut next = next.take().expect("sibling mounted");
        closing_peers.borrow_mut().push(first.controller.clone());
        if !addressed {
            closing_peers.borrow_mut().push(next.controller.clone());
        }
        let mut clock = MotionClock::new();
        for time in [Duration::ZERO, Duration::from_millis(20)] {
            let tick = clock.frame(time);
            first.registry.tick_all(&tick);
            next.registry.tick_all(&tick);
        }
        assert!(first.controller.value() > 0.0 && first.controller.is_animating());
        assert!(next.controller.value() > 0.0 && next.controller.is_animating());
        let mut runtime = Some(runtime);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if incoming {
                let _unwinding_runtime = runtime.take();
                std::panic::panic_any("outer runtime shutdown");
            } else if addressed {
                assert!(
                    runtime
                        .as_mut()
                        .expect("resident runtime")
                        .close_presentation_entered(primary)
                );
            } else {
                drop(runtime.take());
            }
        }));
        let expected_failure = if incoming {
            Some("outer runtime shutdown")
        } else {
            first_failure.or(next_failure)
        };
        assert_eq!(result.is_err(), expected_failure.is_some());
        if let Err(failure) = result {
            assert_eq!(failure.downcast_ref::<&str>().copied(), expected_failure);
        }
        assert!(first.registry.is_empty(), "primary seat is withdrawn");
        assert!(matches!(
            Pin::new(&mut first.run).poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Err(_))
        ));
        assert_eq!(*first.outcomes.borrow(), [true], "one cancellation");
        if addressed {
            assert!(matches!(
                Pin::new(&mut next.run).poll(&mut Context::from_waker(Waker::noop())),
                Poll::Pending
            ));
            let before = next.controller.value();
            next.registry
                .tick_all(&clock.frame(Duration::from_millis(40)));
            assert!(next.controller.value() > before, "surviving run advances");
            closing_peers.borrow_mut().push(next.controller.clone());
            drop(runtime.take());
        }
        let outcome = Pin::new(&mut next.run).poll(&mut Context::from_waker(Waker::noop()));
        assert!(
            next.registry.is_empty(),
            "every stopped presentation withdraws motion: addressed={addressed}, incoming={incoming}, outcome={outcome:?}"
        );
        assert!(matches!(outcome, Poll::Ready(Err(_))));
        assert_eq!(
            *next.outcomes.borrow(),
            [true],
            "sibling cancellation arrives once"
        );
    }
}

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
        .post_frame_handle()
        .schedule(move |_| drop(capture))
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
                "stopping_the_realm_mid_animation_cancels_every_run",
                stopping_the_realm_mid_animation_cancels_every_run,
            ),
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
