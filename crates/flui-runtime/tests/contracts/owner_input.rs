use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::PresentationAddress;
use flui_interaction::{events::Code, testing::input::KeyEventBuilder};
use flui_platform::{PlatformInput, WindowExecutionState};
use flui_runtime::owner::{
    Delivery, InputOutcome, OwnerEffects, OwnerHost, PresentationDispatcher,
};
use flui_runtime::ui_runtime::UiRuntime;

use crate::HostInputExt as _;

// Host input is plain data: it crosses from an adapter thread to its owner
// and into recordings (ADR-0207 §8).
static_assertions::assert_impl_all!(flui_runtime::HostInput: Send, Clone);

fn key(code: Code) -> PlatformInput {
    PlatformInput::Keyboard(KeyEventBuilder::new(code).build())
}

/// Native focus loss retains the mounted target; an absent release in host
/// HTML creates no replay obligation when the canvas owns the next press.
fn mounted_shortcut_recovers_after_native_focus_loss_without_replay() {
    use flui_interaction::FocusNode;
    use flui_platform::keyboard::{Key, KeyEvent, KeyRepeat, KeyState, Modifiers};
    use flui_runtime::owner::WindowObservation;
    use flui_widgets::{
        Actions, CallbackAction, Focus, Intent, Shortcuts, SingleActivator, SizedBox,
    };

    struct SaveIntent;
    impl Intent for SaveIntent {}

    let owner = OwnerHost::new();
    let mut runtime = crate::owner_publication::runtime();
    let field = FocusNode::with_debug_label("restored shortcut field");
    let saves = Rc::new(Cell::new(0));
    let action_saves = Rc::clone(&saves);
    runtime
        .attach_root_widget_with_size(
            &Actions::new(
                Shortcuts::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&field)))
                    .shortcut(
                        SingleActivator::character("s")
                            .control()
                            .allow_repeats(false),
                        SaveIntent,
                    ),
            )
            .action(CallbackAction::new(move |_cx, _intent: &SaveIntent| {
                action_saves.set(action_saves.get() + 1);
            })),
            160.0,
            80.0,
        )
        .expect("mount real Focus and Shortcuts consumers");
    runtime.synchronize_window_snapshot(
        runtime.presentation_id(),
        WindowExecutionState::Running,
        true,
        true,
    );
    struct Sink;
    impl flui_runtime::sink::FrameSink for Sink {
        fn surface_size(&mut self) -> (u32, u32) {
            (160, 80)
        }
        fn submit(&mut self, _: flui_layer::Scene) -> flui_runtime::sink::SubmitVerdict {
            flui_runtime::sink::SubmitVerdict::Presented
        }
    }
    let _ = runtime.pump(
        &mut flui_runtime::pump::SampledClock(web_time::Instant::now()),
        &mut Sink,
    );
    let _ = field.request_focus();
    assert!(
        field.has_primary_focus(),
        "the mounted field owns logical focus"
    );
    let address = owner
        .publication(owner.prepare_runtime(runtime))
        .expect("publish")
        .commit();
    let effects = Effects {
        input: owner
            .presentation_dispatcher(address)
            .expect("input authority"),
        trace: Rc::new(RefCell::new(Vec::new())),
        nested_input: Cell::new(false),
        wakes: Cell::new(0),
    };
    let event = |state, repeat| {
        PlatformInput::Keyboard(
            KeyEvent::new(
                state,
                Key::character("s"),
                Code::KeyS,
                flui_platform::EventTime::from_nanos(0),
            )
            .with_modifiers(Modifiers::CONTROL)
            .with_repeat(repeat),
        )
    };
    assert_eq!(
        effects
            .input
            .input(event(KeyState::Down, KeyRepeat::First), &effects),
        Ok(InputOutcome::Handled)
    );
    assert_eq!(
        saves.get(),
        1,
        "the initial press reached the mounted action"
    );
    effects
        .input
        .observe(WindowObservation::Focus(false), &effects)
        .expect("native focus loss");
    assert!(
        field.has_primary_focus(),
        "native loss retains logical primary focus"
    );
    assert_eq!(saves.get(), 1, "focus loss did not replay activation");
    // The producer rejects releases belonging to foreign DOM nodes. Deliver
    // only the return observation and subsequently owned key packets here.
    effects
        .input
        .observe(WindowObservation::Focus(true), &effects)
        .expect("native focus return");
    assert!(
        field.has_primary_focus(),
        "restoration resumes the same target"
    );
    assert_eq!(
        saves.get(),
        1,
        "restoration did not replay the missing release or press"
    );
    for (state, repeat) in [
        (KeyState::Up, KeyRepeat::First),
        (KeyState::Down, KeyRepeat::AutoRepeat),
    ] {
        assert_eq!(
            effects.input.input(event(state, repeat), &effects),
            Ok(InputOutcome::Unhandled)
        );
        assert_eq!(
            saves.get(),
            1,
            "release and excluded repeat do not activate"
        );
    }
    assert_eq!(
        effects
            .input
            .input(event(KeyState::Down, KeyRepeat::First), &effects),
        Ok(InputOutcome::Handled)
    );
    assert_eq!(
        saves.get(),
        2,
        "a fresh owned press activates once after recovery"
    );
}

pub(super) struct Effects {
    pub(super) input: PresentationDispatcher,
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
        _: flui_foundation::geometry::WindowMetrics,
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

pub(super) fn fixture(handled: bool) -> (OwnerHost, PresentationAddress, Effects) {
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
                    frame.deliver(crate::VSYNC, &effects),
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
            frame.deliver(crate::VSYNC, &effects).expect("frame");
        }
        for width in [810.0, 820.0] {
            effects
                .input
                .observe(
                    flui_runtime::owner::WindowObservation::Metrics(crate::window_metrics(
                        width, 600.0, 1.0,
                    )),
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
            frame.deliver(crate::VSYNC, &effects).expect("frame");
        }
    }
    assert_eq!(
        effects.input.input(
            PlatformInput::Ime(flui_platform::ImeEvent::Enabled),
            &effects
        ),
        Ok(InputOutcome::Queued)
    );
    owner.continue_work(&effects);
    assert_eq!(
        effects.input.input(
            PlatformInput::Ime(flui_platform::ImeEvent::Disabled),
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
        .deliver(crate::VSYNC, &effects)
        .expect("frame");
    assert_eq!(*effects.trace.borrow(), ["frame", "queued key"]);
    assert_eq!(effects.wakes.get(), 0);
}

fn pending_metrics_do_not_cross_ordered_operations() {
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
                frame
                    .deliver(crate::VSYNC, &effects)
                    .expect("consume callback budget");
            }
            let metrics =
                |width| WindowObservation::Metrics(crate::window_metrics(width, 600.0, 1.0));
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
                    frame
                        .deliver(crate::VSYNC, &effects)
                        .expect("frame barrier");
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
                            WindowObservation::Brightness(flui_platform::Brightness::Dark),
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
            Ok(InputOutcome::Handled)
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

/// Records what a presentation publishes to assistive technology, which is
/// attached from the start.
#[derive(Default)]
struct RecordingBridge {
    updates: std::sync::Mutex<Vec<accesskit::TreeUpdate>>,
}

impl flui_semantics::platform::PlatformAccessibility for RecordingBridge {
    fn publish(&self, update: accesskit::TreeUpdate) {
        self.updates.lock().expect("updates").push(update);
    }
    fn is_active(&self) -> bool {
        true
    }
    fn set_activation_listener(
        &self,
        _: flui_semantics::platform::AccessibilityActivationListener,
    ) {
    }
    fn set_action_listener(&self, _: flui_semantics::platform::AccessibilityActionListener) {}

    fn assistive_focus_support(&self) -> flui_semantics::AssistiveFocusSupport {
        flui_semantics::AssistiveFocusSupport::Unreported
    }
}

/// `HostInput::Accessibility` takes the accessibility bridge's path: it waits
/// in the runtime's command inbox and is decoded and invoked by the addressed
/// presentation's semantics owner at the next owner turn's Idle drain
/// (ADR-0207, amendment). A request for a node the presentation never
/// published runs nothing; a panicking handler wakes the owner for the
/// requests behind it; a request queued before a frame is applied, and
/// recorded, before that frame; a full inbox refuses the request.
fn accessibility_input_reaches_the_semantics_owner() {
    use flui_runtime::presentation::PresentationWindow;
    use flui_runtime::{HostInput, InputRecord};
    use flui_widgets::{Row, Semantics, SizedBox};
    use std::sync::atomic::{AtomicUsize, Ordering};

    let bridge = std::sync::Arc::new(RecordingBridge::default());
    let window = flui_native::headless_platform()
        .open_window(
            flui_platform::WindowOptions::default(),
            flui_testing::window_handler::RecordingWindowHandler::new(),
        )
        .expect("headless window");
    let wakes = std::sync::Arc::new(AtomicUsize::new(0));
    let runtime = UiRuntime::new(
        PresentationWindow::new(
            window,
            Some(std::sync::Arc::clone(&bridge) as std::sync::Arc<_>),
        ),
        1.0,
        flui_runtime::ui_runtime::RuntimeHostServices::new(
            {
                let wakes = std::sync::Arc::clone(&wakes);
                std::sync::Arc::new(move || {
                    wakes.fetch_add(1, Ordering::Relaxed);
                })
            },
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            std::sync::Arc::new(flui_platform::InMemoryClipboard::new()),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Platform,
        ),
    )
    .expect("runtime");
    let clicks = Rc::new(Cell::new(0));
    let clicked = Rc::clone(&clicks);
    runtime
        .attach_root_widget_with_size(
            &Row::new((
                Semantics::new()
                    .container(true)
                    .label("Send")
                    .on_click(move |_cx| clicked.set(clicked.get() + 1))
                    .child(SizedBox::new(10.0, 10.0)),
                Semantics::new()
                    .container(true)
                    .label("Boom")
                    .on_click(|_cx| {
                        // Typed `()`: a handler that only diverges has no outcome.
                        if std::hint::black_box(true) {
                            std::panic::panic_any("assistive handler failure");
                        }
                    })
                    .child(SizedBox::new(10.0, 10.0)),
            )),
            40.0,
            40.0,
        )
        .expect("mount");
    struct Sink;
    impl flui_runtime::sink::FrameSink for Sink {
        fn surface_size(&mut self) -> (u32, u32) {
            (40, 40)
        }
        fn submit(&mut self, _: flui_layer::Scene) -> flui_runtime::sink::SubmitVerdict {
            flui_runtime::sink::SubmitVerdict::Presented
        }
    }
    let mut runtime = runtime;
    let _ = runtime.pump(
        &mut flui_runtime::pump::SampledClock(web_time::Instant::now()),
        &mut Sink,
    );
    let node = |label: &str| {
        bridge
            .updates
            .lock()
            .expect("updates")
            .iter()
            .flat_map(|update| update.nodes.iter())
            .find(|(_, node)| node.label() == Some(label))
            .map(|(id, _)| *id)
            .expect("the node was published")
    };
    let (send, boom) = (node("Send"), node("Boom"));
    let taps = runtime.input_taps().clone();

    let owner = OwnerHost::new();
    let address = owner
        .publication(owner.prepare_runtime(runtime))
        .expect("publish")
        .commit();
    let effects = Effects {
        input: owner
            .presentation_dispatcher(address)
            .expect("input authority"),
        trace: Rc::new(RefCell::new(Vec::new())),
        nested_input: Cell::new(false),
        wakes: Cell::new(0),
    };
    let click = |target_node| {
        HostInput::Accessibility(accesskit::ActionRequest {
            action: accesskit::Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node,
            data: None,
        })
    };
    let dispatch = |node| effects.input.dispatch(click(node), &effects);
    let owner_turn = || {
        owner
            .runtime_dispatcher(address.ui_runtime_id)
            .expect("runtime authority")
            .deliver(flui_runtime::owner::RuntimeOperation::Background, &effects)
            .expect("background turn");
    };
    for (node, ran) in [(accesskit::NodeId(u64::MAX), 0), (send, 1)] {
        assert_eq!(
            dispatch(node),
            Ok(InputOutcome::Queued),
            "the request waits in the inbox, as the bridge's does"
        );
        assert_eq!(clicks.get(), 0, "nothing runs before the next owner turn");
        owner_turn();
        assert_eq!(
            clicks.get(),
            ran,
            "the next turn's Idle drain ran the node's handler, or nothing for an unpublished node"
        );
        clicks.set(0);
    }

    // A panicking handler: the request behind it is woken for, not stranded.
    assert_eq!(dispatch(boom), Ok(InputOutcome::Queued));
    assert_eq!(dispatch(send), Ok(InputOutcome::Queued));
    let woken = wakes.load(Ordering::Relaxed);
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(owner_turn));
    assert!(failed.is_err(), "the handler's panic resumes");
    assert_eq!(clicks.get(), 0, "the request behind it waits");
    assert!(
        wakes.load(Ordering::Relaxed) > woken,
        "the drain woke the owner for the accepted tail"
    );
    owner_turn();
    assert_eq!(clicks.get(), 1, "the woken turn ran the request behind it");
    clicks.set(0);

    // A request queued before a frame is applied, and recorded, before it.
    let rows = Rc::new(RefCell::new(Vec::new()));
    let tap = taps
        .install({
            let rows = Rc::clone(&rows);
            move |record: &InputRecord| {
                if let InputRecord::Input(row) = record {
                    rows.borrow_mut().push(match row.input {
                        HostInput::Accessibility(_) => "accessibility",
                        HostInput::Frame { .. } => "frame",
                        _ => "other",
                    });
                }
            }
        })
        .expect("registration");
    assert_eq!(dispatch(send), Ok(InputOutcome::Queued));
    owner
        .frame_dispatcher(address)
        .expect("frames")
        .deliver(crate::VSYNC, &effects)
        .expect("frame");
    assert_eq!(clicks.get(), 1, "the frame applied the queued request");
    assert_eq!(*rows.borrow(), ["accessibility", "frame"]);
    tap.finish().expect("complete recording");
    clicks.set(0);

    // A full inbox refuses the request; the drain makes room again.
    let mut admitted = 0;
    let refused = loop {
        match dispatch(accesskit::NodeId(u64::MAX)) {
            Ok(InputOutcome::Queued) => admitted += 1,
            other => break other,
        }
        assert!(admitted <= 4096, "the inbox is bounded");
    };
    assert_eq!(refused, Err(flui_runtime::owner::DispatchError::InboxFull));
    assert!(admitted > 0, "it admitted requests until it was full");
    owner_turn();
    assert_eq!(
        dispatch(send),
        Ok(InputOutcome::Queued),
        "the drain made room"
    );
    owner_turn();
    assert_eq!(clicks.get(), 1);
}

/// A 40x40 leaf whose hit test panics.
#[derive(Debug)]
struct PanicOnHit;
impl flui_foundation::Diagnosticable for PanicOnHit {}

impl flui_rendering::traits::RenderBox for PanicOnHit {
    type Arity = flui_foundation::Leaf;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> flui_rendering::RenderResult<flui_foundation::geometry::Size> {
        Ok(ctx
            .constraints()
            .constrain(flui_foundation::geometry::Size::new(40.0, 40.0)))
    }

    fn hit_test(
        &self,
        _ctx: &mut flui_rendering::context::BoxHitTestContext<'_, Self::Arity, Self::ParentData>,
    ) -> bool {
        panic!("render object hit test failed");
    }
}

/// A view's hit test says when its tree cannot answer, instead of handing
/// back an empty path that reads as "nothing is here", and contains a render
/// object's panic: the path to every other target is complete, and the panic
/// is the caller's to report once the event reached it.
fn a_view_hit_test_reports_a_busy_tree_and_contains_a_render_object_panic() {
    use flui_rendering::binding::{RendererBinding as _, ViewHitTest};
    use flui_rendering::traits::RenderObject;

    let binding = flui_runtime::renderer_binding::RenderingBinding::new(
        flui_rendering::TextContextHandle::standalone(),
    );
    let pipeline = binding.root_pipeline_owner();
    let healthy = pipeline.with_mut(|owner| {
        let root = owner.set_root_render_object(Box::new(flui_objects::RenderStack::new())
            as Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>);
        let healthy = owner
            .insert_child_render_object(
                root,
                Box::new(flui_objects::RenderColoredBox::red(40.0, 40.0)),
            )
            .expect("healthy child attaches");
        owner
            .insert_child_render_object(root, Box::new(PanicOnHit))
            .expect("panicking child attaches");
        owner.set_root_constraints(Some(flui_rendering::constraints::BoxConstraints::tight(
            flui_foundation::geometry::Size::new(100.0, 100.0),
        )));
        let (returned, frame) = owner.take_idle().run_frame();
        frame.expect("the tree paints");
        *owner = returned;
        healthy
    });
    let position = flui_foundation::geometry::Offset::new(10.0, 10.0);
    let kind = flui_interaction::events::PointerKind::Touch;

    pipeline.withdraw_layout();
    let mut result = flui_interaction::pointer::HitTestResult::new();
    assert!(matches!(
        binding.hit_test_in_view(&mut result, position, kind, 0),
        ViewHitTest::TreeBusy
    ));
    assert!(result.is_empty(), "a busy tree leaves the result untouched");

    pipeline.publish_layout();
    let ViewHitTest::Answered { panic } = binding.hit_test_in_view(&mut result, position, kind, 0)
    else {
        panic!("a published layout answers");
    };
    assert_eq!(
        panic
            .as_deref()
            .and_then(flui_foundation::panic::payload_text),
        Some("render object hit test failed")
    );
    assert_eq!(
        result.path().first().map(|entry| entry.target),
        Some(healthy),
        "the panicking child above is a miss; the one below it is hit"
    );
}

/// What a press's handler saw of a text store that a nested message loop
/// asked for locks from inside the handler.
type NestedLocks = Rc<RefCell<Vec<String>>>;

/// A platform entry that reaches a text store while its UI runtime is
/// checked out for an owner turn (a text service called from a message loop
/// nested in a press's handler) runs no owner code: its synchronous lock is
/// refused and its asynchronous one waits, running once the turn ends and
/// before the runtime returns to its slot. The owner's own call in that turn
/// (the press committing the field's composition) is served (ADR-0142
/// item 6).
///
/// Red-check: drop `set_text_commits_checked_out(true)` from the lease's
/// checkout — the nested synchronous lock is granted inside the handler;
/// drop `finish_checkout` from `return_to_owner` — the queued grant has not
/// run when the dispatch returns.
fn a_platform_entry_while_the_runtime_is_checked_out_runs_no_owner_code() {
    use flui_platform::text_store::{
        Composition, InMemoryTextStore, LockGrant, LockOutcome, LockTiming, TextStore, Utf16Offset,
        Utf16Range,
    };

    let owner = OwnerHost::new();
    let text_input: std::sync::Arc<dyn flui_platform::PlatformTextInput> =
        std::sync::Arc::new(flui_native::FakeTextInput::new());
    let mut runtime = UiRuntime::for_test_with_text_input(Some(text_input));
    let store = InMemoryTextStore::new("ab");
    let erased: Rc<dyn TextStore> = store.clone(); // the presentation holds the field's store through the erased contract.
    let _token = runtime
        .text_input_handle()
        .attach(flui_interaction::TextInputClient::new(Rc::clone(&erased)))
        .expect("the headless window takes text input");
    let composing = Composition {
        range: Utf16Range::new(Utf16Offset::new(2), Utf16Offset::new(4)).expect("ordered"),
        hides_caret: false,
    };
    assert_eq!(
        erased.request_lock(
            LockGrant::read_write(move |session| {
                session.insert_at_selection("東京").expect("in range");
                session.set_composition(Some(composing)).expect("in range");
            }),
            LockTiming::Sync,
        ),
        Ok(LockOutcome::Granted),
        "in its slot, the runtime admits a platform entry"
    );

    let log: NestedLocks = Rc::default();
    let (nested, seen) = (Rc::clone(&erased), Rc::clone(&log));
    runtime
        .attach_root_widget_with_size(
            &flui_widgets::Listener::new()
                .behavior(flui_rendering::hit_testing::HitTestBehavior::Opaque)
                .on_pointer_down(move |_, _| {
                    let read = Rc::clone(&seen);
                    let sync = nested.request_lock(
                        LockGrant::read(move |_| read.borrow_mut().push("sync grant".into())),
                        LockTiming::Sync,
                    );
                    seen.borrow_mut().push(format!("sync {sync:?}"));
                    let queued = Rc::clone(&seen);
                    let deferred = nested.request_lock(
                        LockGrant::read(move |session| {
                            queued.borrow_mut().push(format!(
                                "async grant, composing {:?}",
                                session.composition()
                            ));
                        }),
                        LockTiming::Async,
                    );
                    seen.borrow_mut().push(format!("async {deferred:?}"));
                })
                .child(flui_widgets::SizedBox::new(40.0, 40.0)),
            40.0,
            40.0,
        )
        .expect("mount the press's handler");
    runtime.synchronize_window_snapshot(
        runtime.presentation_id(),
        WindowExecutionState::Running,
        true,
        true,
    );
    struct Sink;
    impl flui_runtime::sink::FrameSink for Sink {
        fn surface_size(&mut self) -> (u32, u32) {
            (40, 40)
        }
        fn submit(&mut self, _: flui_layer::Scene) -> flui_runtime::sink::SubmitVerdict {
            flui_runtime::sink::SubmitVerdict::Presented
        }
    }
    let _ = runtime.pump(
        &mut flui_runtime::pump::SampledClock(web_time::Instant::now()),
        &mut Sink,
    );
    let address = owner
        .publication(owner.prepare_runtime(runtime))
        .expect("publish")
        .commit();
    let effects = Effects {
        input: owner
            .presentation_dispatcher(address)
            .expect("input authority"),
        trace: Rc::new(RefCell::new(Vec::new())),
        nested_input: Cell::new(false),
        wakes: Cell::new(0),
    };
    let press = flui_interaction::events::make_down_event(
        flui_foundation::geometry::Offset::new(4.0, 6.0),
        flui_interaction::events::PointerKind::Mouse,
    )
    .expect("finite test position");
    let _ = effects
        .input
        .input(PlatformInput::Pointer(press), &effects)
        .expect("the press is delivered");

    assert_eq!(
        store.composition(),
        None,
        "the press committed the composition"
    );
    assert_eq!(store.text(), "ab東京", "keeping its text");
    assert_eq!(
        *log.borrow(),
        [
            "sync Err(SyncLockUnavailable)".to_owned(),
            "async Ok(Deferred)".to_owned(),
            "async grant, composing None".to_owned(),
        ],
        "the nested entry ran no grant inside the handler; its queued one ran at the turn's end"
    );
    assert_eq!(
        erased.request_lock(LockGrant::read(|_| {}), LockTiming::Sync),
        Ok(LockOutcome::Granted),
        "back in its slot, the runtime admits a platform entry again"
    );
}
#[test]
fn owner_input_contract() {
    crate::table_test::run_table(
        "owner_input_contract",
        &[
            (
                "mounted_shortcut_recovers_after_native_focus_loss_without_replay",
                mounted_shortcut_recovers_after_native_focus_loss_without_replay as fn(),
            ),
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
            (
                "accessibility_input_reaches_the_semantics_owner",
                accessibility_input_reaches_the_semantics_owner as fn(),
            ),
            (
                "a_view_hit_test_reports_a_busy_tree_and_contains_a_render_object_panic",
                a_view_hit_test_reports_a_busy_tree_and_contains_a_render_object_panic as fn(),
            ),
            (
                "a_platform_entry_while_the_runtime_is_checked_out_runs_no_owner_code",
                a_platform_entry_while_the_runtime_is_checked_out_runs_no_owner_code as fn(),
            ),
        ],
    );
}
