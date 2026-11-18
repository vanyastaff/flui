//! The scenario primitive as a capability's simulated provider meets it
//! (ADR-0154 §10): ordered answers matched by request, a permission revoked
//! on virtual time, a service turned off, injected errors, an event stream
//! and its end, and the typed failures of a run that left the script.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_platform::capability::scripted::{
    AlreadyDriven, Answer, EventSubscription, Events, Expect, MAX_REPORTS, RequestTicket, Scenario,
    ScenarioFailure, Script, ServiceState, StreamEnd, StreamItem, Track,
};
use flui_platform::capability::{
    Abandoned, Capability, CapabilityRegistrar, Completion, CompletionSource, Plugin,
    ProviderContext,
};
use flui_platform::{Unsupported, UnsupportedReason};
use flui_testing::{HeadlessHost, HeadlessWindow};

use crate::capability_seam::{acquire_capability, acquire_capability_in};

/// What the user let the application do with the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    Undetermined,
    Granted,
    Denied,
    Revoked,
}

/// What a camera handle asks its platform for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum CameraRequest {
    /// Ask the user for access.
    Prompt,
    /// Take a picture this wide.
    Capture { width: u32 },
}

/// Why a capture failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaptureError {
    Busy,
}

struct Camera;

impl Capability for Camera {
    type Handle = CameraHandle;
    const NAME: &'static str = "test.scenario.camera";
}

/// A camera handle built the way a capability crate's simulated provider
/// builds one: every call it answers later goes through the scenario.
#[derive(Clone, Debug)]
struct CameraHandle {
    prompts: Script<CameraRequest, Access>,
    captures: Script<CameraRequest, Result<Vec<u8>, CaptureError>>,
    access: Track<Access>,
    frames: Events<u32>,
    completions: CompletionSource,
}

impl CameraHandle {
    fn request_access(&self) -> Completion<Access> {
        let (completion, resolver) = self.completions.pending::<Access>();
        self.prompts.receive(CameraRequest::Prompt, resolver);
        completion
    }

    fn capture(&self, width: u32) -> Completion<Result<Vec<u8>, CaptureError>> {
        let (completion, resolver) = self.completions.pending();
        self.captures
            .receive(CameraRequest::Capture { width }, resolver);
        completion
    }

    /// A capture its caller can call off: it waits while the ticket is held.
    fn capture_until_dropped(
        &self,
        width: u32,
    ) -> (Completion<Result<Vec<u8>, CaptureError>>, RequestTicket) {
        let (completion, resolver) = self.completions.pending();
        let ticket = self
            .captures
            .receive_with_ticket(CameraRequest::Capture { width }, resolver);
        (completion, ticket)
    }

    fn access_changed(&self) -> Completion<Access> {
        self.access.changed(&self.completions)
    }

    fn frames(&self) -> EventSubscription<u32> {
        self.frames.subscribe()
    }
}

/// The simulated camera's plugin: what a capability crate's `testing`
/// module ships, every part on one scenario.
#[derive(Clone)]
struct SimulatedCamera {
    prompts: Script<CameraRequest, Access>,
    captures: Script<CameraRequest, Result<Vec<u8>, CaptureError>>,
    access: Track<Access>,
    service: Track<ServiceState>,
    frames: Events<u32>,
}

impl SimulatedCamera {
    fn on(scenario: &Scenario) -> Self {
        Self {
            prompts: scenario.script("camera prompts"),
            captures: scenario.script("camera captures"),
            access: scenario.track("camera access", Access::Undetermined),
            service: scenario.service("camera"),
            frames: scenario.events("camera frames"),
        }
    }
}

impl Plugin for SimulatedCamera {
    fn register(&self, registrar: &mut CapabilityRegistrar) {
        let camera = self.clone();
        registrar.provide::<Camera>(move |ctx: &ProviderContext<'_>| {
            camera.service.check()?;
            Ok(CameraHandle {
                prompts: camera.prompts.clone(),
                captures: camera.captures.clone(),
                access: camera.access.clone(),
                frames: camera.frames.clone(),
                completions: ctx.completions().clone(),
            })
        });
    }
}

type Slot<T> = Rc<RefCell<Option<Result<T, Abandoned>>>>;

fn land<T: 'static>(completion: Completion<T>) -> Slot<T> {
    let slot = Slot::<T>::default();
    let into = Rc::clone(&slot);
    completion.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
    slot
}

fn taken<T>(slot: &Slot<T>) -> Option<Result<T, Abandoned>> {
    slot.borrow_mut().take()
}

/// A host on `scenario` whose widget acquired the simulated camera.
fn camera_on(scenario: &Scenario) -> (HeadlessHost, SimulatedCamera, CameraHandle) {
    let camera = SimulatedCamera::on(scenario);
    let mut host = HeadlessHost::with_scenario(HeadlessWindow::new(10, 10), scenario, &camera);
    let handle = acquire_capability::<Camera>(&mut host).expect("the simulated camera is on");
    (host, camera, handle)
}

#[test]
fn deny_then_allow_then_revoke_at_three_seconds() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    camera.prompts.expect(
        Expect::exactly(CameraRequest::Prompt),
        Answer::value(Access::Denied),
    );
    camera.prompts.expect(
        Expect::exactly(CameraRequest::Prompt),
        Answer::computed({
            let access = camera.access.clone();
            move |_| {
                access.set(Access::Granted);
                Access::Granted
            }
        }),
    );
    camera
        .access
        .set_at(Duration::from_secs(3), Access::Revoked);

    let first = land(handle.request_access());
    assert_eq!(
        taken(&first),
        None,
        "an answer never settles inside the call"
    );
    let _ = host.pump(Duration::ZERO);
    assert_eq!(taken(&first), Some(Ok(Access::Denied)));

    let second = land(handle.request_access());
    let _ = host.pump(Duration::from_secs(1));
    assert_eq!(taken(&second), Some(Ok(Access::Granted)));
    assert_eq!(camera.access.get(), Access::Granted);

    let revoked = land(handle.access_changed());
    let _ = host.pump(Duration::from_millis(1_999));
    assert_eq!(taken(&revoked), None, "2.999 s: still granted");
    assert_eq!(camera.access.get(), Access::Granted);
    let _ = host.pump(Duration::from_millis(1));
    assert_eq!(
        taken(&revoked),
        Some(Ok(Access::Revoked)),
        "the revocation settles in the first turn at 3 s"
    );
    assert_eq!(camera.access.get(), Access::Revoked);

    scenario.verify().expect("the run followed the scenario");
}

#[test]
fn a_request_nothing_answers_ends_its_completion_and_fails_the_run() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    camera.prompts.answer_next(Answer::value(Access::Granted));

    let answered = land(handle.request_access());
    let unexpected = land(handle.request_access());
    let _ = host.pump(Duration::ZERO);
    let _ = host.pump(Duration::ZERO);

    assert_eq!(taken(&answered), Some(Ok(Access::Granted)));
    assert_eq!(
        taken(&unexpected),
        Some(Err(Abandoned::ProviderDropped)),
        "no default answer: the caller sees its request end"
    );
    let failures = scenario.verify().expect_err("one request was not scripted");
    assert_eq!(
        failures.failures(),
        [ScenarioFailure::Unexpected {
            script: "camera prompts",
            request: "Prompt".to_owned(),
        }]
    );
}

#[test]
fn a_step_answers_the_request_it_matches_not_the_next_one_in_line() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    camera.captures.expect(
        Expect::when("a wide capture", |request: &CameraRequest| {
            matches!(request, CameraRequest::Capture { width } if *width >= 1_000)
        }),
        Answer::value(Err(CaptureError::Busy)),
    );
    camera
        .captures
        .expect(Expect::any(), Answer::value(Ok(vec![1, 2, 3])));

    let narrow = land(handle.capture(320));
    let wide = land(handle.capture(4_000));
    let _ = host.pump(Duration::ZERO);

    assert_eq!(
        taken(&narrow),
        Some(Ok(Ok(vec![1, 2, 3]))),
        "the narrow capture skips the wide step"
    );
    assert_eq!(taken(&wide), Some(Ok(Err(CaptureError::Busy))));
    assert_eq!(
        camera.captures.requests(),
        [
            CameraRequest::Capture { width: 320 },
            CameraRequest::Capture { width: 4_000 },
        ]
    );
    scenario.verify().expect("both steps were used");
}

#[test]
fn a_dropped_ticket_withdraws_a_waiting_request_and_the_run_owes_nothing_for_it() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);

    // Held, then called off.
    camera.captures.hold();
    let (held, ticket) = handle.capture_until_dropped(64);
    let held = land(held);
    assert_eq!(camera.captures.held(), 1);
    drop(ticket);
    assert_eq!(camera.captures.held(), 0, "a withdrawn request is not held");
    assert!(
        !camera.captures.release_oldest(Ok(vec![1])),
        "nothing to release"
    );
    camera.captures.resume();

    // Scheduled, then called off before its instant.
    camera
        .captures
        .answer_next(Answer::value(Ok(vec![2])).after(Duration::from_millis(500)));
    let (scheduled, ticket) = handle.capture_until_dropped(32);
    let scheduled = land(scheduled);
    drop(ticket);
    let _ = host.pump(Duration::ZERO);
    assert_eq!(taken(&held), Some(Err(Abandoned::ProviderDropped)));
    assert_eq!(taken(&scheduled), Some(Err(Abandoned::ProviderDropped)));
    assert_eq!(camera.captures.withdrawn(), 2);
    scenario
        .verify()
        .expect("the withdrawn answer is off the timeline before its instant");

    // Answered, then the ticket dropped: nothing to withdraw.
    camera.captures.answer_next(Answer::value(Ok(vec![3])));
    let (answered, ticket) = handle.capture_until_dropped(16);
    let answered = land(answered);
    drop(ticket);
    let _ = host.pump(Duration::ZERO);
    assert_eq!(taken(&answered), Some(Ok(Ok(vec![3]))));
    assert_eq!(
        camera.captures.withdrawn(),
        2,
        "an answered request stays answered"
    );

    scenario
        .verify()
        .expect("a withdrawn request is neither unanswered nor a change not reached");
}

#[test]
fn a_delayed_answer_settles_at_the_first_turn_at_or_after_its_instant() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    camera
        .captures
        .answer_next(Answer::value(Ok(vec![9])).after(Duration::from_millis(500)));

    let capture = land(handle.capture(64));
    let _ = host.pump(Duration::from_millis(499));
    assert_eq!(taken(&capture), None);
    let unreached = scenario.verify().expect_err("the answer is due at 500 ms");
    assert!(
        matches!(
            unreached.failures(),
            [ScenarioFailure::NotReached { at, .. }] if *at == Duration::from_millis(500)
        ),
        "{unreached}"
    );

    let _ = host.pump(Duration::from_millis(1));
    assert_eq!(taken(&capture), Some(Ok(Ok(vec![9]))));
    scenario.verify().expect("the answer was given");
}

#[test]
fn a_service_turned_off_refuses_acquisition_until_it_is_back() {
    let scenario = Scenario::new();
    let camera = SimulatedCamera::on(&scenario);
    let refusal = Unsupported::of::<Camera>(UnsupportedReason::Unavailable);
    camera.service.set(ServiceState::Off(refusal));
    camera
        .service
        .set_at(Duration::from_secs(1), ServiceState::On);
    let mut host = HeadlessHost::with_scenario(HeadlessWindow::new(10, 10), &scenario, &camera);

    let refused = acquire_capability::<Camera>(&mut host).expect_err("the camera is off");
    assert_eq!(refused, refusal);

    let _ = host.pump(Duration::from_secs(1));
    assert_eq!(camera.service.get(), ServiceState::On, "back on at 1 s");
    // A widget mounted after 1 s, in the next host to drive the scenario.
    drop(host);
    let mut later = HeadlessHost::with_scenario(HeadlessWindow::new(10, 10), &scenario, &camera);
    assert!(acquire_capability::<Camera>(&mut later).is_ok());
    scenario.verify().expect("the service came back");
}

#[test]
fn a_scenario_drives_one_host_at_a_time() {
    let scenario = Scenario::new();
    let camera = SimulatedCamera::on(&scenario);
    let first = HeadlessHost::with_scenario(HeadlessWindow::new(10, 10), &scenario, &camera);
    let second = HeadlessHost::try_with_scenario(HeadlessWindow::new(10, 10), &scenario, &camera);
    assert!(
        matches!(second, Err(AlreadyDriven)),
        "a second host would move the first's time"
    );
    let panicking = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        HeadlessHost::with_scenario(HeadlessWindow::new(10, 10), &scenario, &camera)
    }));
    assert!(panicking.is_err(), "the panicking form refuses too");
    drop(first);
    let _third = HeadlessHost::try_with_scenario(HeadlessWindow::new(10, 10), &scenario, &camera)
        .expect("free once the first host is gone");
}

#[test]
fn a_failure_repeated_after_verify_is_new() {
    let ended = std::panic::catch_unwind(|| {
        let scenario = Scenario::new();
        let (mut host, camera, handle) = camera_on(&scenario);
        let first = land(handle.request_access());
        let _ = host.pump(Duration::ZERO);
        assert_eq!(taken(&first), Some(Err(Abandoned::ProviderDropped)));
        assert_eq!(
            scenario.verify().expect_err("unscripted").failures().len(),
            1
        );
        // The same unscripted request again: equal by value, but new.
        let again = land(handle.request_access());
        let _ = host.pump(Duration::ZERO);
        assert_eq!(taken(&again), Some(Err(Abandoned::ProviderDropped)));
        drop(camera);
    });
    let payload = ended.expect_err("the repeat was never verified");
    let text = flui_foundation::panic::payload_text(&*payload).unwrap_or_default();
    assert!(text.contains("camera prompts"), "{text}");
}

#[test]
fn a_dropped_script_keeps_its_debts_when_a_requests_debug_panics() {
    /// A request whose `Debug` fails.
    struct Opaque;

    impl std::fmt::Debug for Opaque {
        fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            panic!("this request cannot be written")
        }
    }

    let scenario = Scenario::new();
    let (_host, _camera, handle) = camera_on(&scenario);
    let script: Script<Opaque, u32> = scenario.script("opaque");
    script.expect_held(Expect::any());
    script.expect(Expect::when("never", |_| false), Answer::value(1));
    let (held, resolver) = handle.completions.pending::<u32>();
    script.receive(Opaque, resolver);

    let dropped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(script)));
    assert!(dropped.is_err(), "the Debug's panic is resumed");
    let failures = scenario.verify().expect_err("both debts were handed over");
    assert_eq!(
        failures.failures(),
        [
            ScenarioFailure::Unanswered {
                script: "opaque",
                request: "<a value whose Debug panicked>".to_owned(),
            },
            ScenarioFailure::Unconsumed {
                script: "opaque",
                step: "never".into(),
            },
        ]
    );
    drop(held);
}

#[test]
fn reports_from_other_threads_are_bounded_and_refused_after_the_end() {
    let scenario = Scenario::new();
    let report = scenario.failure_report();
    std::thread::scope(|threads| {
        threads.spawn(|| {
            for n in 0..MAX_REPORTS + 3 {
                assert!(report.report(ScenarioFailure::Unexpected {
                    script: "http",
                    request: format!("https://example.com/{n}"),
                }));
            }
        });
    });
    let failures = scenario.verify().expect_err("reported");
    assert_eq!(failures.failures().len(), MAX_REPORTS + 1);
    assert_eq!(
        failures.failures().last(),
        Some(&ScenarioFailure::ReportsDropped { dropped: 3 })
    );

    drop(scenario);
    assert!(
        !report.report(ScenarioFailure::Unexpected {
            script: "http",
            request: "late".to_owned(),
        }),
        "nobody reads a report after the scenario ended"
    );
}

#[test]
fn a_script_dropped_with_debts_hands_them_to_the_scenario() {
    let scenario = Scenario::new();
    let (_host, _camera, handle) = camera_on(&scenario);
    let script: Script<u32, u32> = scenario.script("dropped");
    script.expect(Expect::exactly(7), Answer::value(1));
    script.expect_held(Expect::any());
    let (held, resolver) = handle.completions.pending::<u32>();
    script.receive(3, resolver);
    drop(script);

    let failures = scenario
        .verify()
        .expect_err("the dropped script still owes");
    assert_eq!(
        failures.failures(),
        [
            ScenarioFailure::Unanswered {
                script: "dropped",
                request: "3".to_owned(),
            },
            ScenarioFailure::Unconsumed {
                script: "dropped",
                step: "7".into(),
            },
        ]
    );
    drop(held);
}

#[test]
fn a_scenario_dropped_unverified_with_debts_fails_the_test() {
    let ended = std::panic::catch_unwind(|| {
        let scenario = Scenario::new();
        let script: Script<u32, u32> = scenario.script("forgotten");
        script.answer_next(Answer::value(1));
    });
    let payload = ended.expect_err("an unverified debt panics as the scenario drops");
    let text = flui_foundation::panic::payload_text(&*payload).unwrap_or_default();
    assert!(text.contains("forgotten"), "{text}");

    // Verified, the same debt is acknowledged and the drop is quiet.
    let scenario = Scenario::new();
    let script: Script<u32, u32> = scenario.script("acknowledged");
    script.answer_next(Answer::value(1));
    assert!(scenario.verify().is_err());
}

#[test]
fn a_panicking_release_answer_keeps_its_request_held_and_answers_the_rest() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    camera.captures.hold();
    let first = land(handle.capture(1));
    let poisoned = land(handle.capture(2));
    let third = land(handle.capture(3));

    let released = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        camera.captures.release_held(|request| match request {
            CameraRequest::Capture { width: 2 } => panic!("the test's answer failed"),
            _ => Ok(vec![]),
        })
    }));
    assert!(released.is_err(), "the answer's panic is resumed");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(taken(&first), Some(Ok(Ok(vec![]))));
    assert_eq!(
        taken(&third),
        Some(Ok(Ok(vec![]))),
        "not dropped with the panic"
    );
    assert_eq!(taken(&poisoned), None);
    assert_eq!(camera.captures.held(), 1, "the failed one stays held");

    assert!(camera.captures.release_oldest(Err(CaptureError::Busy)));
    let _ = host.pump(Duration::ZERO);
    assert_eq!(taken(&poisoned), Some(Ok(Err(CaptureError::Busy))));
    scenario.verify().expect("every request was answered");
}

/// A script's observer hears every answer before the caller's completion
/// settles, whoever gave it: a step, a delayed step, the standing answer, a
/// release; and hears `None` for a request that ends unanswered. An
/// observer added later hears beside it rather than replacing it.
#[test]
fn a_scripts_observer_hears_every_answer_however_it_is_given() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    // Each capture's width, and whether it was answered `Ok`, `Err` or not.
    let heard = Rc::new(RefCell::new(Vec::<(u32, Option<bool>)>::new()));
    let log = Rc::clone(&heard);
    camera.captures.on_answer(move |request, answer| {
        if let CameraRequest::Capture { width } = request {
            log.borrow_mut().push((*width, answer.map(Result::is_ok)));
        }
    });
    let also_heard = Rc::new(std::cell::Cell::new(0_usize));
    let counted = Rc::clone(&also_heard);
    camera
        .captures
        .on_answer(move |_, _| counted.set(counted.get() + 1));
    camera.captures.otherwise(|_| Ok(vec![]));
    camera
        .captures
        .answer_next(Answer::value(Err(CaptureError::Busy)));
    camera
        .captures
        .answer_next(Answer::value(Ok(vec![1])).after(Duration::from_millis(10)));
    let stepped = land(handle.capture(1));
    let delayed = land(handle.capture(2));
    let standing = land(handle.capture(3));
    camera.captures.hold();
    let released = land(handle.capture(4));
    let abandoned = land(handle.capture(5));
    camera.captures.resume();
    assert!(camera.captures.release_oldest(Ok(vec![])));
    assert_eq!(camera.captures.abandon_held(), 1);
    let _ = host.pump(Duration::from_millis(10));
    assert_eq!(
        *heard.borrow(),
        [
            (1, Some(false)),
            (3, Some(true)),
            (4, Some(true)),
            (5, None),
            (2, Some(true))
        ]
    );
    assert_eq!(also_heard.get(), 5, "the later observer heard every answer");
    for slot in [&stepped, &delayed, &standing, &released] {
        assert!(taken(slot).is_some(), "every answered request settled");
    }
    assert_eq!(taken(&abandoned), Some(Err(Abandoned::ProviderDropped)));
    scenario.verify().expect("every request was answered");
}

#[test]
fn a_panicking_matcher_keeps_its_request_held() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    camera.captures.expect(
        Expect::when("a matcher that fails", |_: &CameraRequest| {
            panic!("the test's matcher failed")
        }),
        Answer::value(Ok(vec![])),
    );

    let asked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handle.capture(5)));
    assert!(asked.is_err(), "the matcher's panic is resumed");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(camera.captures.held(), 1, "the request is held, not lost");
    let failures = scenario.verify().expect_err("held and unused");
    assert!(
        failures.failures().contains(&ScenarioFailure::Unanswered {
            script: "camera captures",
            request: "Capture { width: 5 }".to_owned(),
        }),
        "{failures}"
    );
}

#[test]
fn the_host_waits_for_a_scheduled_change_as_for_any_deadline() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    camera
        .captures
        .answer_next(Answer::value(Ok(vec![4])).after(Duration::from_millis(100)));
    let capture = land(handle.capture(4));

    assert_eq!(
        host.time_to_next_frame(),
        Some(Duration::from_millis(100)),
        "the scheduled answer is the next deadline"
    );
    host.pump_until_settled(Duration::from_millis(16), 20)
        .expect("settles once the answer arrives");
    assert_eq!(taken(&capture), Some(Ok(Ok(vec![4]))));
    assert_eq!(host.time_to_next_frame(), None);
    scenario.verify().expect("the answer was given");
}

#[test]
fn time_moved_between_inputs_applies_what_falls_due() {
    let scenario = Scenario::new();
    let (mut host, camera, _handle) = camera_on(&scenario);
    camera
        .access
        .set_at(Duration::from_secs(3), Access::Revoked);
    camera
        .access
        .set_at(Duration::from_secs(7), Access::Granted);

    // A pointer script with no frame step: time moves only between inputs.
    let point = flui_foundation::geometry::Offset::new(5.0, 5.0);
    let script = flui_testing::replay::TouchScript::new("a press held 5 s")
        .down(1, point)
        .wait(Duration::from_secs(5))
        .up(1)
        .build()
        .expect("a paired press");
    host.run_script(&script);
    assert_eq!(
        camera.access.get(),
        Access::Revoked,
        "the input at 5 s sees the revocation at 3 s"
    );

    host.advance(Duration::from_secs(2));
    assert_eq!(camera.access.get(), Access::Granted, "granted again at 7 s");
    scenario.verify().expect("both changes applied");
}

#[test]
fn an_unscripted_http_request_fails_the_hosts_scenario() {
    let scenario = Scenario::new();
    let (host, _camera, _handle) = camera_on(&scenario);
    let mut sent = host
        .http()
        .handle()
        .send(flui_platform::http::HttpRequest::get(
            "https://example.com/missing",
        ));
    let answer = sent
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()));
    assert!(
        matches!(
            answer,
            std::task::Poll::Ready(Err(flui_platform::http::HttpError::Connect { .. }))
        ),
        "the caller sees the connection fail"
    );
    let failures = scenario.verify().expect_err("the scenario sees it too");
    assert_eq!(
        failures.failures(),
        [ScenarioFailure::Unexpected {
            script: "http",
            request: "https://example.com/missing".to_owned(),
        }]
    );
}

#[test]
fn the_clipboard_service_turns_off_and_back_on() {
    let scenario = Scenario::new();
    let (mut host, _camera, _handle) = camera_on(&scenario);
    let clipboard = host.clipboard();
    let refusal = Unsupported::new(UnsupportedReason::Unavailable);
    clipboard.service().set(ServiceState::Off(refusal));
    clipboard
        .service()
        .set_at(Duration::from_secs(2), ServiceState::On);

    let window = host.open_window(HeadlessWindow::new(10, 10));
    let off = acquire_capability_in::<flui_interaction::text_input::ClipboardCapability>(
        &mut host, window,
    );
    assert_eq!(off.err(), Some(refusal));

    let _ = host.pump(Duration::from_secs(2));
    let second = host.open_window(HeadlessWindow::new(10, 10));
    let on = acquire_capability_in::<flui_interaction::text_input::ClipboardCapability>(
        &mut host, second,
    );
    assert!(on.is_ok(), "the host's own clipboard is back at 2 s");
    scenario.verify().expect("the clipboard came back");
}

#[test]
fn an_event_stream_delivers_on_virtual_time_and_ends_once() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    let frames = handle.frames();
    camera.frames.emit_at(Duration::from_millis(16), 1);
    camera.frames.emit_at(Duration::from_millis(32), 2);
    camera
        .frames
        .end_at(Duration::from_millis(32), StreamEnd::Finished);

    let first = land(frames.next(&handle.completions));
    let _ = host.pump(Duration::from_millis(15));
    assert_eq!(taken(&first), None, "nothing sent before 16 ms");
    let _ = host.pump(Duration::from_millis(1));
    assert_eq!(taken(&first), Some(Ok(StreamItem::Event(1))));

    let _ = host.pump(Duration::from_millis(16));
    assert_eq!(
        frames.queued(),
        1,
        "the event waits for a reader, the end behind it"
    );
    let second = land(frames.next(&handle.completions));
    let end = land(frames.next(&handle.completions));
    let after_end = land(frames.next(&handle.completions));
    let _ = host.pump(Duration::ZERO);
    assert_eq!(taken(&second), Some(Ok(StreamItem::Event(2))));
    assert_eq!(taken(&end), Some(Ok(StreamItem::End(StreamEnd::Finished))));
    assert_eq!(
        taken(&after_end),
        Some(Ok(StreamItem::End(StreamEnd::Finished))),
        "an ended stream stays ended"
    );

    camera.frames.emit(3);
    let failures = scenario.verify().expect_err("an event after the end");
    assert_eq!(
        failures.failures(),
        [ScenarioFailure::AfterEnd {
            stream: "camera frames",
            sent: "3".to_owned(),
        }]
    );
}

#[test]
fn a_caller_that_asks_again_from_its_answer_is_answered_at_the_next_turn() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    camera.prompts.answer_next(Answer::value(Access::Denied));
    camera.prompts.answer_next(Answer::value(Access::Granted));

    let again: Slot<Access> = Slot::default();
    let into = Rc::clone(&again);
    let asker = handle.clone();
    let first = Slot::<Access>::default();
    let first_into = Rc::clone(&first);
    let settled_inline = Rc::new(std::cell::Cell::new(None::<bool>));
    let inline = Rc::clone(&settled_inline);
    handle.request_access().on_complete(move |outcome| {
        *first_into.borrow_mut() = Some(outcome);
        // Asked again from inside the answer: the script holds no borrow.
        let landed = Rc::clone(&into);
        asker
            .request_access()
            .on_complete(move |outcome| *landed.borrow_mut() = Some(outcome));
        inline.set(Some(into.borrow().is_some()));
    });
    let _ = host.pump(Duration::ZERO);
    assert_eq!(taken(&first), Some(Ok(Access::Denied)));
    assert_eq!(
        settled_inline.get(),
        Some(false),
        "the request asked from the answer did not settle inside it"
    );
    assert_eq!(
        taken(&again),
        Some(Ok(Access::Granted)),
        "it settled at a later owner turn of the same pump"
    );
    scenario.verify().expect("both prompts were scripted");
}

#[test]
fn a_held_request_and_an_unused_step_fail_the_run() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    camera.captures.hold();
    camera.prompts.expect(
        Expect::exactly(CameraRequest::Prompt),
        Answer::value(Access::Granted),
    );

    let held = land(handle.capture(8));
    let _ = host.pump(Duration::ZERO);
    assert_eq!(taken(&held), None);
    assert_eq!(camera.captures.held(), 1);

    let failures = scenario.verify().expect_err("held and unused");
    assert_eq!(
        failures.failures(),
        [
            ScenarioFailure::Unconsumed {
                script: "camera prompts",
                step: "Prompt".into(),
            },
            ScenarioFailure::Unanswered {
                script: "camera captures",
                request: "Capture { width: 8 }".to_owned(),
            },
        ]
    );

    assert!(camera.captures.release_oldest(Err(CaptureError::Busy)));
    let _ = host.pump(Duration::ZERO);
    assert_eq!(taken(&held), Some(Ok(Err(CaptureError::Busy))));
}
