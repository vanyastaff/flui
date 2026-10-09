use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::PresentationAddress;
use flui_interaction::{events::Code, testing::input::KeyEventBuilder};
use flui_platform_api::{PlatformInput, WindowExecutionState};
use flui_runtime::owner::{
    Delivery, InputOutcome, OwnerEffects, OwnerHost, PresentationDispatcher,
};
use flui_runtime::ui_runtime::UiRuntime;

fn key(code: Code) -> PlatformInput {
    PlatformInput::Keyboard(KeyEventBuilder::new(code).build())
}

struct Effects {
    input: PresentationDispatcher,
    trace: Rc<RefCell<Vec<&'static str>>>,
    nested_input: Cell<bool>,
    wakes: Cell<usize>,
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
        if self.nested_input.replace(false) {
            assert_eq!(
                self.input.input(key(Code::F4), self),
                Ok(InputOutcome::Queued)
            );
            assert_eq!(
                *self.trace.borrow(),
                ["frame"],
                "input did not reenter the runtime"
            );
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
        self.trace.borrow_mut().push("resize");
    }
    fn retire_presentation(
        &self,
        _: flui_foundation::PresentationAddress,
        _: Option<flui_foundation::PresentationAddress>,
    ) {
    }
    fn after_turn(&self, _: flui_runtime::owner::RecoveryState) {}
    fn request_continuation(&self) -> bool {
        self.wakes.set(self.wakes.get() + 1);
        true
    }
}

fn fixture(handled: bool) -> (OwnerHost, PresentationAddress, Effects) {
    let owner = OwnerHost::new();
    let runtime = crate::owner_publication::runtime();
    runtime.synchronize_window_snapshot(
        runtime.presentation_id(),
        WindowExecutionState::Running,
        true,
        true,
    );
    runtime.update_host_lifecycle(flui_scheduler::AppLifecycleState::Resumed);
    let trace = Rc::new(RefCell::new(Vec::new()));
    let key_trace = Rc::clone(&trace);
    runtime
        .focus_manager()
        .add_global_key_handler(Rc::new(move |event| {
            key_trace.borrow_mut().push(if event.code == Code::F5 {
                "fresh key"
            } else {
                "queued key"
            });
            if handled {
                flui_interaction::KeyEventResult::Handled
            } else {
                flui_interaction::KeyEventResult::Ignored
            }
        }));
    let address = owner
        .publication(owner.prepare_runtime(runtime))
        .expect("publish")
        .commit();
    let effects = Effects {
        input: owner
            .presentation_dispatcher(address)
            .expect("input authority"),
        trace,
        nested_input: Cell::new(false),
        wakes: Cell::new(0),
    };
    (owner, address, effects)
}

fn keyboard_preserves_accepted_order_and_shares_the_callback_budget() {
    for handled in [false, true] {
        let (owner, address, effects) = fixture(handled);
        assert_eq!(
            effects.input.input(key(Code::F5), &effects),
            Ok(if handled {
                InputOutcome::Handled
            } else {
                InputOutcome::Unhandled
            })
        );
        effects.trace.borrow_mut().clear();
        let frame = owner.frame_dispatcher(address).expect("frame authority");
        {
            let _callback = owner.begin_callback(&effects).expect("native callback");
            for index in 0..80 {
                assert_eq!(
                    frame.deliver(&effects),
                    Ok(if index < 32 {
                        Delivery::Driven
                    } else {
                        Delivery::Queued
                    })
                );
            }
            assert_eq!(
                effects.input.input(key(Code::F4), &effects),
                Ok(InputOutcome::Queued)
            );
            assert_eq!(effects.trace.borrow().len(), 32);
        }
        assert_eq!(effects.wakes.get(), 1);
        let reply = effects
            .input
            .input(key(Code::F5), &effects)
            .expect("fresh input");
        assert_eq!(reply, InputOutcome::Queued);
        let mut expected = vec!["frame"; 64];
        assert_eq!(*effects.trace.borrow(), expected);
        assert_eq!(effects.wakes.get(), 2);
        owner.continue_work(&effects);
        expected.extend(std::iter::repeat_n("frame", 16));
        expected.push("queued key");
        expected.push("fresh key");
        assert_eq!(*effects.trace.borrow(), expected);
        assert_eq!(effects.wakes.get(), 2);
    }
}

fn queued_resize_and_input_remain_ordered_across_native_callbacks() {
    let (owner, address, effects) = fixture(true);
    let frame = owner.frame_dispatcher(address).expect("frame");
    {
        let _callback = owner.begin_callback(&effects).expect("callback");
        for _ in 0..32 {
            frame.deliver(&effects).expect("frame");
        }
        for width in [810.0, 820.0] {
            effects
                .input
                .observe(
                    flui_runtime::owner::WindowObservation::Metrics {
                        size: flui_foundation::geometry::Size::new(width, 600.0),
                        scale_factor: 1.0,
                    },
                    &effects,
                )
                .expect("metrics");
            if width == 810.0 {
                assert_eq!(
                    effects.input.input(key(Code::F4), &effects),
                    Ok(InputOutcome::Queued)
                );
            }
        }
    }
    effects.trace.borrow_mut().clear();
    assert_eq!(
        effects.input.input(key(Code::F5), &effects),
        Ok(InputOutcome::Handled)
    );
    assert_eq!(
        *effects.trace.borrow(),
        ["resize", "queued key", "resize", "fresh key"]
    );
}

fn non_keyboard_input_reports_deferred_delivery_when_carried_work_uses_the_budget() {
    let (owner, address, effects) = fixture(true);
    let frame = owner.frame_dispatcher(address).expect("frame");
    {
        let _callback = owner.begin_callback(&effects).expect("callback");
        for _ in 0..80 {
            frame.deliver(&effects).expect("frame");
        }
    }
    assert_eq!(
        effects.input.input(
            PlatformInput::Ime(flui_platform_api::ImeEvent::Enabled),
            &effects
        ),
        Ok(InputOutcome::Queued)
    );
    owner.continue_work(&effects);
    assert_eq!(
        effects.input.input(
            PlatformInput::Ime(flui_platform_api::ImeEvent::Disabled),
            &effects
        ),
        Ok(InputOutcome::Handled)
    );
}

fn reentrant_keyboard_is_queued_until_the_frame_returns() {
    let (owner, address, effects) = fixture(false);
    effects.nested_input.set(true);
    owner
        .frame_dispatcher(address)
        .expect("frame authority")
        .deliver(&effects)
        .expect("frame");
    assert_eq!(*effects.trace.borrow(), ["frame", "queued key"]);
    assert_eq!(effects.wakes.get(), 0);
}

fn pending_metrics_do_not_cross_ordered_operations() {
    use flui_foundation::geometry::Size;
    use flui_runtime::owner::{RuntimeOperation, WindowObservation};

    for barrier in [
        "input",
        "frame",
        "focus",
        "visibility",
        "execution",
        "snapshot",
        "hover",
        "lifecycle",
        "other window",
    ] {
        let (owner, address, effects) = fixture(true);
        let other = owner
            .publication(owner.prepare_runtime(crate::owner_publication::runtime()))
            .expect("second runtime")
            .commit();
        let frame = owner.frame_dispatcher(address).expect("frame");
        {
            let _callback = owner.begin_callback(&effects).expect("native callback");
            for _ in 0..32 {
                frame.deliver(&effects).expect("consume callback budget");
            }
            let metrics = |width| WindowObservation::Metrics {
                size: Size::new(width, 600.0),
                scale_factor: 1.0,
            };
            effects
                .input
                .observe(metrics(800.0), &effects)
                .expect("older metrics");
            effects
                .input
                .observe(metrics(810.0), &effects)
                .expect("replace pending metrics");
            match barrier {
                "input" => {
                    effects
                        .input
                        .input(key(Code::F4), &effects)
                        .expect("input barrier");
                }
                "frame" => {
                    frame.deliver(&effects).expect("frame barrier");
                }
                "lifecycle" => {
                    owner
                        .runtime_dispatcher(address.ui_runtime_id)
                        .expect("runtime")
                        .deliver(
                            RuntimeOperation::Lifecycle(flui_scheduler::AppLifecycleState::Resumed),
                            &effects,
                        )
                        .expect("lifecycle barrier");
                }
                "other window" => {
                    owner
                        .presentation_dispatcher(other)
                        .expect("other window")
                        .observe(
                            WindowObservation::Brightness(flui_platform_api::Brightness::Dark),
                            &effects,
                        )
                        .expect("other window barrier");
                }
                event => {
                    let observation = match event {
                        "focus" => WindowObservation::Focus(false),
                        "visibility" => WindowObservation::Visibility(false),
                        "execution" => {
                            WindowObservation::Execution(WindowExecutionState::Suspended)
                        }
                        "snapshot" => WindowObservation::Snapshot {
                            execution: WindowExecutionState::Running,
                            focused: true,
                            visible: true,
                        },
                        "hover" => WindowObservation::Hover(false),
                        _ => unreachable!("named observation"),
                    };
                    effects
                        .input
                        .observe(observation, &effects)
                        .expect("observation barrier");
                }
            }
            effects
                .input
                .observe(metrics(820.0), &effects)
                .expect("later metrics");
            effects
                .input
                .observe(metrics(830.0), &effects)
                .expect("replace later metrics");
        }
        effects.trace.borrow_mut().clear();
        owner.continue_work(&effects);
        let expected: &[&str] = match barrier {
            "input" => &["resize", "queued key", "resize"],
            "frame" => &["resize", "frame", "resize"],
            _ => &["resize", "resize"],
        };
        assert_eq!(*effects.trace.borrow(), expected, "{barrier}");
        owner.shutdown(&effects);
    }
}

fn window_observations_update_only_the_target_runtime_lifecycle() {
    use flui_runtime::owner::WindowObservation;
    use flui_scheduler::AppLifecycleState::{Hidden, Inactive, Paused, Resumed};
    use std::sync::{Arc, Mutex};

    let owner = OwnerHost::new();
    let mut installed = Vec::new();
    for _ in 0..2 {
        let runtime = crate::owner_publication::runtime();
        runtime.synchronize_window_snapshot(
            runtime.presentation_id(),
            WindowExecutionState::Running,
            true,
            true,
        );
        runtime.update_host_lifecycle(Resumed);
        let observed = Arc::new(Mutex::new(Vec::new()));
        let callback_observed = Arc::clone(&observed);
        runtime
            .scheduler()
            .add_lifecycle_state_listener(std::rc::Rc::new(move |state| {
                callback_observed
                    .lock()
                    .expect("observation lock")
                    .push(state);
            }));
        let address = owner
            .publication(owner.prepare_runtime(runtime))
            .expect("publish")
            .commit();
        installed.push((
            owner
                .presentation_dispatcher(address)
                .expect("presentation"),
            observed,
        ));
    }
    let effects = Effects {
        input: installed[0].0.clone(),
        trace: Rc::new(RefCell::new(Vec::new())),
        nested_input: Cell::new(false),
        wakes: Cell::new(0),
    };
    for observation in [
        WindowObservation::Focus(false),
        WindowObservation::Visibility(false),
        WindowObservation::Execution(WindowExecutionState::Suspended),
        WindowObservation::Snapshot {
            execution: WindowExecutionState::Running,
            focused: true,
            visible: true,
        },
    ] {
        assert_eq!(
            installed[0].0.observe(observation, &effects),
            Ok(Delivery::Driven)
        );
    }
    assert_eq!(
        *installed[0].1.lock().expect("observations"),
        [Inactive, Hidden, Paused, Resumed]
    );
    assert!(
        installed[1]
            .1
            .lock()
            .expect("other runtime observations")
            .is_empty()
    );
    assert!(
        effects.trace.borrow().is_empty(),
        "observations do not fabricate frames"
    );
}

#[test]
fn owner_input_contract() {
    crate::table_test::run_table(
        "owner_input_contract",
        &[
            (
                "pending_metrics_do_not_cross_ordered_operations",
                pending_metrics_do_not_cross_ordered_operations as fn(),
            ),
            (
                "window_observations_update_only_the_target_runtime_lifecycle",
                window_observations_update_only_the_target_runtime_lifecycle as fn(),
            ),
            (
                "keyboard_preserves_accepted_order_and_shares_the_callback_budget",
                keyboard_preserves_accepted_order_and_shares_the_callback_budget as fn(),
            ),
            (
                "queued_resize_and_input_remain_ordered_across_native_callbacks",
                queued_resize_and_input_remain_ordered_across_native_callbacks as fn(),
            ),
            (
                "non_keyboard_input_reports_deferred_delivery_when_carried_work_uses_the_budget",
                non_keyboard_input_reports_deferred_delivery_when_carried_work_uses_the_budget
                    as fn(),
            ),
            (
                "reentrant_keyboard_is_queued_until_the_frame_returns",
                reentrant_keyboard_is_queued_until_the_frame_returns as fn(),
            ),
        ],
    );
}
