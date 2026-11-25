//! The scenario primitive as a capability's simulated provider meets it
//! (ADR-0154 §10): ordered answers matched by request, a permission revoked
//! on virtual time, a service turned off, injected errors, an event stream
//! and its end, and the typed failures of a run that left the script.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_platform::capability::scripted::{
    AlreadyDriven, Answer, EventSubscription, Events, Expect, MAX_RECORDED_REQUESTS, MAX_REPORTS,
    RequestTicket, Scenario, ScenarioFailure, Script, ScriptHold, ServiceState, StreamEnd,
    StreamItem, Track,
};
use flui_platform::capability::{
    Abandoned, Capability, CapabilityRegistrar, Completion, CompletionSource, Plugin,
    ProviderContext,
};
use flui_platform::{Unsupported, UnsupportedReason};
use flui_testing::host::{HeadlessHost, HeadlessWindow};

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
    let hold = camera.captures.hold();
    let (held, ticket) = handle.capture_until_dropped(64);
    let held = land(held);
    assert_eq!(camera.captures.held(), 1);
    drop(ticket);
    assert_eq!(camera.captures.held(), 0, "a withdrawn request is not held");
    assert!(
        !camera.captures.release_oldest(Ok(vec![1])),
        "nothing to release"
    );
    drop(hold);

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
    let hold = camera.captures.hold();
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
    assert_eq!(hold.release(), 0, "and waits for the test, not the drain");
    assert_eq!(camera.captures.held(), 1);

    assert!(camera.captures.release_oldest(Err(CaptureError::Busy)));
    let _ = host.pump(Duration::ZERO);
    assert_eq!(taken(&poisoned), Some(Ok(Err(CaptureError::Busy))));
    scenario.verify().expect("every request was answered");
}

/// A script counts its holds: whichever of two goes first, requests stay
/// held until the last is gone, and only the requests the holds kept then go
/// on, oldest first, to the steps and the standing answer, as if they
/// arrived then. A request a step held waits for the test.
#[test]
fn a_scripts_last_hold_sends_on_only_what_the_holds_kept() {
    for first_hold_goes_first in [true, false] {
        let case = if first_hold_goes_first {
            "the first hold dropped first"
        } else {
            "the second hold dropped first"
        };
        let scenario = Scenario::new();
        let (mut host, camera, handle) = camera_on(&scenario);
        camera.captures.otherwise(|_| Ok(vec![0]));
        camera
            .captures
            .expect_held(Expect::exactly(CameraRequest::Capture { width: 1 }));
        let stepped = land(handle.capture(1));
        let mut holds = [Some(camera.captures.hold()), Some(camera.captures.hold())];
        let early = land(handle.capture(2));
        camera
            .captures
            .answer_next(Answer::value(Err(CaptureError::Busy)));
        let (gone, kept) = if first_hold_goes_first {
            (0, 1)
        } else {
            (1, 0)
        };
        let released = holds[gone].take().map(ScriptHold::release);
        assert_eq!(released, Some(0), "{case}: the other hold keeps them");
        let late = land(handle.capture(3));
        let _ = host.pump(Duration::ZERO);
        assert_eq!(taken(&early), None, "{case}");
        assert_eq!(taken(&late), None, "{case}");
        assert_eq!(camera.captures.held(), 3, "{case}");

        let released = holds[kept].take().map(ScriptHold::release);
        assert_eq!(released, Some(2), "{case}: the two the holds kept");
        let _ = host.pump(Duration::ZERO);
        assert_eq!(
            taken(&early),
            Some(Ok(Err(CaptureError::Busy))),
            "{case}: the step added during the hold takes the oldest"
        );
        assert_eq!(taken(&late), Some(Ok(Ok(vec![0]))), "{case}");
        assert_eq!(taken(&stepped), None, "{case}: the step's request waits");
        assert_eq!(camera.captures.held(), 1, "{case}");

        assert!(camera.captures.release_oldest(Ok(vec![1])));
        let _ = host.pump(Duration::ZERO);
        assert_eq!(taken(&stepped), Some(Ok(Ok(vec![1]))), "{case}");
        scenario.verify().expect("every request was answered");
    }
}

/// The explicit releases leave each held request in the script until its
/// own answer is given: a ticket dropped by an earlier answer withdraws a
/// later request instead of it being answered or dropped a second time.
#[test]
fn a_release_leaves_the_rest_in_the_script_while_it_answers_one() {
    #[derive(Clone, Copy, Debug)]
    enum Release {
        Held,
        Abandon,
    }
    for release in [Release::Held, Release::Abandon] {
        let scenario = Scenario::new();
        let (mut host, camera, handle) = camera_on(&scenario);
        let hold = camera.captures.hold();
        let first = land(handle.capture(1));
        let (second, ticket) = handle.capture_until_dropped(2);
        let second = land(second);
        let ticket = Rc::new(RefCell::new(Some(ticket)));
        camera.captures.on_answer({
            let ticket = Rc::clone(&ticket);
            move |request, _| {
                if *request == (CameraRequest::Capture { width: 1 }) {
                    drop(ticket.take());
                }
            }
        });
        let count = match release {
            Release::Held => camera.captures.release_held(|_| Ok(vec![0])),
            Release::Abandon => camera.captures.abandon_held(),
        };
        assert_eq!(count, 1, "{release:?}: only the first");
        assert_eq!(camera.captures.withdrawn(), 1, "{release:?}");
        assert_eq!(hold.release(), 0, "{release:?}: nothing left to send on");
        let _ = host.pump(Duration::ZERO);
        let expected_first = match release {
            Release::Held => Some(Ok(Ok(vec![0]))),
            Release::Abandon => Some(Err(Abandoned::ProviderDropped)),
        };
        assert_eq!(taken(&first), expected_first, "{release:?}");
        assert_eq!(
            taken(&second),
            Some(Err(Abandoned::ProviderDropped)),
            "{release:?}"
        );
        scenario.verify().expect("nothing is owed");
    }
}

/// The last hold's drain sends the kept requests on one at a time: a request
/// the first one's answer sends queues behind the older ones and takes no
/// step before them.
#[test]
fn a_request_sent_during_a_drain_queues_behind_the_kept_ones() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    camera
        .captures
        .answer_next(Answer::value(Err(CaptureError::Busy)));
    camera.captures.answer_next(Answer::value(Ok(vec![2])));
    camera.captures.otherwise(|_| Ok(vec![0]));
    let hold = camera.captures.hold();
    let first = land(handle.capture(1));
    let second = land(handle.capture(2));
    let sent = Rc::new(RefCell::new(None));
    camera.captures.on_answer({
        let sent = Rc::clone(&sent);
        move |request, _| {
            if *request == (CameraRequest::Capture { width: 1 }) {
                sent.replace(Some(land(handle.capture(3))));
            }
        }
    });
    assert_eq!(hold.release(), 3, "the two kept and the one queued behind");
    let _ = host.pump(Duration::ZERO);
    let third = sent.take().expect("the first answer sent a request");
    assert_eq!(taken(&first), Some(Ok(Err(CaptureError::Busy))));
    assert_eq!(
        taken(&second),
        Some(Ok(Ok(vec![2]))),
        "the older request takes the next step"
    );
    assert_eq!(taken(&third), Some(Ok(Ok(vec![0]))), "the newer one queued");
    scenario.verify().expect("every request was answered");
}

/// The last hold's drain leaves the requests it has not reached in the
/// script: a ticket the first one's answer drops withdraws the later
/// request it waits by, which is then neither answered nor owed.
#[test]
fn a_ticket_dropped_during_a_drain_withdraws_its_request() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    camera.captures.otherwise(|_| Ok(vec![0]));
    let hold = camera.captures.hold();
    let first = land(handle.capture(1));
    let (second, ticket) = handle.capture_until_dropped(2);
    let second = land(second);
    let ticket = Rc::new(RefCell::new(Some(ticket)));
    camera.captures.on_answer({
        let ticket = Rc::clone(&ticket);
        move |request, _| {
            if *request == (CameraRequest::Capture { width: 1 }) {
                drop(ticket.take());
            }
        }
    });
    assert_eq!(hold.release(), 1, "only the first was answered");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(taken(&first), Some(Ok(Ok(vec![0]))));
    assert_eq!(
        taken(&second),
        Some(Err(Abandoned::ProviderDropped)),
        "withdrawn by its ticket, not answered"
    );
    assert_eq!(camera.captures.withdrawn(), 1);
    scenario
        .verify()
        .expect("a withdrawn request is neither answered nor owed");
}

/// A test's answer that panics while the last hold sends the kept requests
/// on leaves that request held and the others answered, and its panic is
/// resumed by `release` and by the hold's drop, or discarded when the hold
/// drops while an earlier panic unwinds.
#[test]
fn a_panic_during_a_drain_is_kept_until_the_rest_are_answered() {
    #[derive(Clone, Copy, Debug)]
    enum LetGo {
        Release,
        Drop,
        DropWhileUnwinding,
    }
    for let_go in [LetGo::Release, LetGo::Drop, LetGo::DropWhileUnwinding] {
        let scenario = Scenario::new();
        let (mut host, camera, handle) = camera_on(&scenario);
        camera.captures.otherwise(|request| match request {
            CameraRequest::Capture { width: 2 } => panic!("the drained answer failed"),
            _ => Ok(vec![0]),
        });
        let hold = camera.captures.hold();
        let first = land(handle.capture(1));
        let poisoned = land(handle.capture(2));
        let third = land(handle.capture(3));

        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match let_go {
            LetGo::Release => {
                let _ = hold.release();
            }
            LetGo::Drop => drop(hold),
            LetGo::DropWhileUnwinding => {
                let _hold = hold;
                panic!("the test failed first");
            }
        }))
        .expect_err("a panic reaches the caller");
        let expected = match let_go {
            LetGo::Release | LetGo::Drop => "the drained answer failed",
            LetGo::DropWhileUnwinding => "the test failed first",
        };
        assert_eq!(
            failure.downcast_ref::<&str>(),
            Some(&expected),
            "{let_go:?}"
        );
        let _ = host.pump(Duration::ZERO);
        assert_eq!(taken(&first), Some(Ok(Ok(vec![0]))), "{let_go:?}");
        assert_eq!(taken(&third), Some(Ok(Ok(vec![0]))), "{let_go:?}");
        assert_eq!(taken(&poisoned), None, "{let_go:?}");
        assert_eq!(camera.captures.held(), 1, "{let_go:?}: it stays held");

        assert!(camera.captures.release_oldest(Err(CaptureError::Busy)));
        let _ = host.pump(Duration::ZERO);
        assert_eq!(taken(&poisoned), Some(Ok(Err(CaptureError::Busy))));
        scenario.verify().expect("every request was answered");
    }
}

/// A hold taken by the code a drain runs stops the drain there: the requests
/// after it, and those that arrive later, stay held in order until that
/// hold goes.
#[test]
fn a_hold_taken_during_a_drain_keeps_the_rest_in_order() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    camera.captures.otherwise(|request| match request {
        CameraRequest::Capture { width } => Ok(vec![u8::try_from(*width).expect("small")]),
        CameraRequest::Prompt => Ok(vec![]),
    });
    let order = Rc::new(RefCell::new(Vec::<u32>::new()));
    let taken_again = Rc::new(RefCell::new(None::<ScriptHold>));
    camera.captures.on_answer({
        let order = Rc::clone(&order);
        let captures = camera.captures.clone();
        let taken_again = Rc::clone(&taken_again);
        move |request, _| {
            if let CameraRequest::Capture { width } = request {
                order.borrow_mut().push(*width);
                if *width == 1 {
                    taken_again.replace(Some(captures.hold()));
                }
            }
        }
    });
    let hold = camera.captures.hold();
    let slots: Vec<_> = (1..=3).map(|width| land(handle.capture(width))).collect();
    assert_eq!(hold.release(), 1, "the new hold stops the drain");
    assert_eq!(camera.captures.held(), 2);
    let fourth = land(handle.capture(4));
    assert_eq!(
        camera.captures.held(),
        3,
        "a later request waits behind them"
    );

    let again = taken_again.take().expect("the observer took a hold");
    assert_eq!(again.release(), 3);
    let _ = host.pump(Duration::ZERO);
    assert_eq!(*order.borrow(), [1, 2, 3, 4], "oldest first");
    for (slot, width) in slots.iter().chain([&fourth]).zip(1_u8..) {
        assert_eq!(taken(slot), Some(Ok(Ok(vec![width]))));
    }
    scenario.verify().expect("every request was answered");
}

/// A hold does not keep its script alive: the script dropped first hands
/// the request the hold kept to the scenario as unanswered, and the hold let
/// go afterwards answers nothing.
#[test]
fn a_hold_outliving_its_script_answers_nothing() {
    let scenario = Scenario::new();
    let (_host, _camera, handle) = camera_on(&scenario);
    let script: Script<u32, u32> = scenario.script("outlived");
    script.otherwise_value(7);
    let hold = script.hold();
    let (held, resolver) = handle.completions.pending::<u32>();
    script.receive(1, resolver);
    drop(script);
    assert_eq!(hold.release(), 0);
    let failures = scenario.verify().expect_err("the kept request is owed");
    assert_eq!(
        failures.failures(),
        [ScenarioFailure::Unanswered {
            script: "outlived",
            request: "1".to_owned(),
        }]
    );
    drop(held);
}

/// A test's value that panics as a step lets go of it, mid-drain, is kept as
/// the run's failure while the drain still sends the rest on, and the script
/// answers later requests at once again.
#[test]
fn a_panicking_drop_during_a_drain_leaves_the_script_answering() {
    struct PanicOnDrop;
    impl Drop for PanicOnDrop {
        fn drop(&mut self) {
            panic!("a matcher's value panicked as it dropped");
        }
    }

    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    camera.captures.otherwise(|_| Ok(vec![0]));
    let captured = PanicOnDrop;
    camera.captures.expect(
        Expect::when("the first capture", move |request: &CameraRequest| {
            let _ = &captured;
            *request == CameraRequest::Capture { width: 1 }
        }),
        Answer::value(Ok(vec![1])),
    );
    let hold = camera.captures.hold();
    let first = land(handle.capture(1));
    let second = land(handle.capture(2));
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hold.release()))
        .expect_err("the drop's panic reaches whoever let go");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"a matcher's value panicked as it dropped")
    );
    let _ = host.pump(Duration::ZERO);
    assert_eq!(taken(&first), Some(Ok(Ok(vec![1]))));
    assert_eq!(taken(&second), Some(Ok(Ok(vec![0]))), "the drain went on");

    let later = land(handle.capture(3));
    let _ = host.pump(Duration::ZERO);
    assert_eq!(
        taken(&later),
        Some(Ok(Ok(vec![0]))),
        "no hold is left, so a later request is answered at once"
    );
    scenario.verify().expect("every request was answered");
}

/// A release counts only the requests it answered: one a step answers
/// later, one a step abandons and one nothing answers are not counted.
#[test]
fn a_release_counts_only_the_requests_it_answered() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    camera
        .captures
        .answer_next(Answer::value(Ok(vec![1])).after(Duration::from_millis(10)));
    camera.captures.answer_next(Answer::abandon());
    let hold = camera.captures.hold();
    let delayed = land(handle.capture(1));
    let abandoned = land(handle.capture(2));
    let unexpected = land(handle.capture(3));
    assert_eq!(
        hold.release(),
        0,
        "none of them was answered by the release"
    );
    let _ = host.pump(Duration::from_millis(10));
    assert_eq!(taken(&delayed), Some(Ok(Ok(vec![1]))));
    assert_eq!(taken(&abandoned), Some(Err(Abandoned::ProviderDropped)));
    assert_eq!(taken(&unexpected), Some(Err(Abandoned::ProviderDropped)));
    let failures = scenario.verify().expect_err("the third was unexpected");
    assert_eq!(
        failures.failures(),
        [ScenarioFailure::Unexpected {
            script: "camera captures",
            request: "Capture { width: 3 }".to_owned(),
        }]
    );
}

/// A hold taken and let go by the code a drain runs does not start a drain
/// of its own: the first drain sends every kept request on, in order, and
/// counts them all.
#[test]
fn a_hold_let_go_inside_a_drain_leaves_the_drain_to_finish() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    camera.captures.otherwise(|_| Ok(vec![0]));
    camera.captures.on_answer({
        let captures = camera.captures.clone();
        move |request, _| {
            if *request == (CameraRequest::Capture { width: 1 }) {
                drop(captures.hold());
            }
        }
    });
    let settled = Rc::new(RefCell::new(Vec::<u32>::new()));
    let hold = camera.captures.hold();
    for width in 1..=3 {
        let settled = Rc::clone(&settled);
        handle
            .capture(width)
            .on_complete(move |_| settled.borrow_mut().push(width));
    }
    assert_eq!(hold.release(), 3);
    let _ = host.pump(Duration::ZERO);
    assert_eq!(*settled.borrow(), [1, 2, 3], "answered oldest first");
    scenario.verify().expect("every request was answered");
}

/// `leave_held` sends nothing on: while another hold lives, the kept
/// request stays that hold's and its release answers it; as the last hold,
/// it leaves the request to the test, so a later hold's release answers
/// nothing and the run reports it.
#[test]
fn leave_held_leaves_the_kept_requests_to_another_hold_or_to_the_test() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    camera.captures.otherwise(|_| Ok(vec![0]));
    let left = camera.captures.hold();
    let other = camera.captures.hold();
    let kept = land(handle.capture(1));
    left.leave_held();
    assert_eq!(camera.captures.held(), 1);
    assert_eq!(other.release(), 1, "the other hold sends it on");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(taken(&kept), Some(Ok(Ok(vec![0]))));

    let last = camera.captures.hold();
    let left_to_the_test = land(handle.capture(2));
    last.leave_held();
    let later = land(handle.capture(3));
    assert_eq!(
        camera.captures.hold().release(),
        0,
        "a later hold's release leaves it"
    );
    let _ = host.pump(Duration::ZERO);
    assert_eq!(taken(&later), Some(Ok(Ok(vec![0]))), "no hold keeps it");
    assert_eq!(taken(&left_to_the_test), None);
    assert_eq!(camera.captures.held(), 1);
    let failures = scenario.verify().expect_err("the left request is owed");
    assert_eq!(
        failures.failures(),
        [ScenarioFailure::Unanswered {
            script: "camera captures",
            request: "Capture { width: 2 }".to_owned(),
        }]
    );
    assert!(camera.captures.release_oldest(Err(CaptureError::Busy)));
    let _ = host.pump(Duration::ZERO);
    assert_eq!(taken(&left_to_the_test), Some(Ok(Err(CaptureError::Busy))));
}

/// A hold let go after `verify` reported what it kept does not send that
/// on: the request stays held and reported, so no new failure appears at
/// teardown. A request it kept after the `verify` still goes on.
#[test]
fn a_hold_let_go_after_verify_leaves_what_verify_reported() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    let hold = camera.captures.hold();
    let reported = land(handle.capture(1));
    let owed = [ScenarioFailure::Unanswered {
        script: "camera captures",
        request: "Capture { width: 1 }".to_owned(),
    }];
    let failures = scenario.verify().expect_err("the kept request is owed");
    assert_eq!(failures.failures(), owed);

    camera.captures.otherwise(|_| Ok(vec![0]));
    let after = land(handle.capture(2));
    assert_eq!(hold.release(), 1, "only the request kept after verify");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(taken(&after), Some(Ok(Ok(vec![0]))));
    assert_eq!(taken(&reported), None, "what verify reported stays held");
    assert_eq!(camera.captures.held(), 1);
    let failures = scenario.verify().expect_err("still owed, nothing new");
    assert_eq!(failures.failures(), owed);

    assert!(camera.captures.release_oldest(Err(CaptureError::Busy)));
    let _ = host.pump(Duration::ZERO);
    assert_eq!(taken(&reported), Some(Ok(Err(CaptureError::Busy))));
}

/// An answer whose `Drop` panics when it is dropped unanswered (its
/// completion gone, or its request withdrawn), as test code may.
#[derive(Debug, PartialEq, Eq)]
struct Bomb {
    armed: bool,
}

impl Drop for Bomb {
    fn drop(&mut self) {
        assert!(
            !self.armed || std::thread::panicking(),
            "an answer panicked as it dropped"
        );
    }
}

/// A request withdrawn while `release_held` makes its own answer is not
/// answered or counted, and the answer made for it is dropped as the test's
/// code: its panic is kept and the next held request is still answered.
#[test]
fn a_request_withdrawn_while_its_release_answer_is_made_is_not_counted() {
    // The count: the withdrawn request is not counted, the next one is.
    let scenario = Scenario::new();
    let (mut host, _camera, handle) = camera_on(&scenario);
    let script: Script<u32, Bomb> = scenario.script("bombs");
    let hold = script.hold();
    let (withdrawn, resolver) = handle.completions.pending::<Bomb>();
    let withdrawn = land(withdrawn);
    let ticket = RefCell::new(Some(script.receive_with_ticket(1, resolver)));
    let (next, resolver) = handle.completions.pending::<Bomb>();
    let next = land(next);
    script.receive(2, resolver);
    let answered = script.release_held(|request| {
        if *request == 1 {
            drop(ticket.take());
        }
        Bomb { armed: false }
    });
    assert_eq!(answered, 1, "the withdrawn request is not counted");
    assert_eq!(script.withdrawn(), 1);
    assert_eq!(hold.release(), 0);
    let _ = host.pump(Duration::ZERO);
    assert_eq!(taken(&withdrawn), Some(Err(Abandoned::ProviderDropped)));
    assert_eq!(taken(&next), Some(Ok(Bomb { armed: false })));
    scenario.verify().expect("a withdrawn request is not owed");

    // The panic: the answer made for the withdrawn request drops as the
    // test's code, its panic kept and the next request still answered.
    let scenario = Scenario::new();
    let (mut host, _camera, handle) = camera_on(&scenario);
    let script: Script<u32, Bomb> = scenario.script("bombs");
    let hold = script.hold();
    let (withdrawn, resolver) = handle.completions.pending::<Bomb>();
    let withdrawn = land(withdrawn);
    let ticket = RefCell::new(Some(script.receive_with_ticket(1, resolver)));
    let (next, resolver) = handle.completions.pending::<Bomb>();
    let next = land(next);
    script.receive(2, resolver);

    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        script.release_held(|request| {
            let armed = *request == 1;
            if armed {
                drop(ticket.take());
            }
            Bomb { armed }
        })
    }))
    .expect_err("the dropped answer's panic is resumed");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"an answer panicked as it dropped")
    );
    assert_eq!(script.withdrawn(), 1, "withdrawn while its answer was made");
    assert_eq!(hold.release(), 0, "nothing is left held");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(taken(&withdrawn), Some(Err(Abandoned::ProviderDropped)));
    assert_eq!(
        taken(&next),
        Some(Ok(Bomb { armed: false })),
        "the next held request is still answered"
    );
    scenario.verify().expect("a withdrawn request is not owed");
}

/// An answer the resolver drops because its completion is gone is the
/// test's code: whether a drain or `release_held` gives it, its panic is
/// contained, the next request is still answered, and an observer's panic
/// for the same request, which came first, is the one resumed.
#[test]
fn an_answer_dropped_by_its_resolver_keeps_the_first_panic_and_the_next_request() {
    #[derive(Clone, Copy, Debug)]
    enum Answering {
        Receive,
        Drain,
        ReleaseHeld,
    }
    for answering in [Answering::Receive, Answering::Drain, Answering::ReleaseHeld] {
        let scenario = Scenario::new();
        let (mut host, _camera, handle) = camera_on(&scenario);
        let script: Script<u32, Bomb> = scenario.script("bombs");
        script.otherwise(|request| Bomb {
            armed: *request == 1,
        });
        script.on_answer(|request, _| {
            assert!(*request != 1, "the observer failed");
        });
        let (gone, gone_resolver) = handle.completions.pending::<Bomb>();
        drop(gone);
        let (next, next_resolver) = handle.completions.pending::<Bomb>();
        let next = land(next);

        let caught = |run: &mut dyn FnMut()| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(run))
                .expect_err("a panic reaches the caller")
        };
        let failure = match answering {
            Answering::Receive => {
                let mut resolver = Some(gone_resolver);
                let failure = caught(&mut || {
                    script.receive(1, resolver.take().expect("received once"));
                });
                script.receive(2, next_resolver);
                failure
            }
            Answering::Drain => {
                let hold = script.hold();
                script.receive(1, gone_resolver);
                script.receive(2, next_resolver);
                let mut hold = Some(hold);
                caught(&mut || {
                    let _ = hold.take().expect("released once").release();
                })
            }
            Answering::ReleaseHeld => {
                let hold = script.hold();
                script.receive(1, gone_resolver);
                script.receive(2, next_resolver);
                let failure = caught(&mut || {
                    let _ = script.release_held(|request| Bomb {
                        armed: *request == 1,
                    });
                });
                drop(hold);
                failure
            }
        };
        assert_eq!(
            failure.downcast_ref::<&str>(),
            Some(&"the observer failed"),
            "{answering:?}: the observer's panic came first"
        );
        let _ = host.pump(Duration::ZERO);
        assert_eq!(
            taken(&next),
            Some(Ok(Bomb { armed: false })),
            "{answering:?}: the next request is still answered"
        );
        scenario.verify().expect("every request was answered");
    }
}

/// A hold taken and left with `leave_held` by the code a drain runs leaves
/// the kept requests to that drain, as one taken and dropped does.
#[test]
fn a_hold_left_inside_a_drain_leaves_the_rest_to_it() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    camera.captures.otherwise(|_| Ok(vec![0]));
    camera.captures.on_answer({
        let captures = camera.captures.clone();
        move |request, _| {
            if *request == (CameraRequest::Capture { width: 1 }) {
                captures.hold().leave_held();
            }
        }
    });
    let hold = camera.captures.hold();
    let slots: Vec<_> = (1..=3).map(|width| land(handle.capture(width))).collect();
    assert_eq!(hold.release(), 3, "the drain answers all three");
    let _ = host.pump(Duration::ZERO);
    for slot in &slots {
        assert_eq!(taken(slot), Some(Ok(Ok(vec![0]))));
    }
    assert_eq!(camera.captures.held(), 0);
    scenario.verify().expect("every request was answered");
}

/// A value a matcher captured is the test's: when a step is used up, the
/// matcher's last handle is let go contained, on the receive path too, and
/// before the answer is given, so its panic is the first, ahead of an
/// observer's for the same request; and a matcher that re-enters and uses a
/// later step up does not lose the request it was testing when that step's
/// matcher drops.
#[test]
fn a_matcher_capture_dropped_on_the_receive_path_keeps_the_request_and_the_first_panic() {
    struct PanicOnDrop;
    impl Drop for PanicOnDrop {
        fn drop(&mut self) {
            assert!(
                std::thread::panicking(),
                "a matcher's value panicked as it dropped"
            );
        }
    }
    type Heard = Rc<RefCell<Vec<(u32, Option<Result<Vec<u8>, CaptureError>>)>>>;
    fn hear(camera: &SimulatedCamera) -> Heard {
        let heard = Heard::default();
        let log = Rc::clone(&heard);
        camera.captures.on_answer(move |request, answer| {
            if let CameraRequest::Capture { width } = request {
                log.borrow_mut().push((*width, answer.cloned()));
            }
        });
        heard
    }

    // The used step's capture drops before the observer runs.
    let scenario = Scenario::new();
    let (_host, camera, handle) = camera_on(&scenario);
    camera.captures.on_answer(|request, _| {
        assert!(
            *request != (CameraRequest::Capture { width: 1 }),
            "the observer failed"
        );
    });
    let heard = hear(&camera);
    let captured = PanicOnDrop;
    camera.captures.expect(
        Expect::when("the first capture", move |request: &CameraRequest| {
            let _ = &captured;
            *request == CameraRequest::Capture { width: 1 }
        }),
        Answer::value(Ok(vec![1])),
    );
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drop(handle.capture(1));
    }))
    .expect_err("a panic reaches the caller");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"a matcher's value panicked as it dropped")
    );
    assert_eq!(*heard.borrow(), [(1, Some(Ok(vec![1])))]);
    scenario.verify().expect("the request was answered");

    // A matcher that re-enters uses the later step up; that step's matcher
    // drops as the earlier request goes on, which still reaches the
    // standing answer.
    let scenario = Scenario::new();
    let (_host, camera, handle) = camera_on(&scenario);
    camera.captures.otherwise(|_| Ok(vec![0]));
    let heard = hear(&camera);
    let reentered = Rc::new(RefCell::new(None));
    camera.captures.expect(
        Expect::when("sends the second capture", {
            let handle = handle.clone();
            let reentered = Rc::clone(&reentered);
            move |request: &CameraRequest| {
                if *request == (CameraRequest::Capture { width: 1 }) && reentered.borrow().is_none()
                {
                    reentered.replace(Some(land(handle.capture(2))));
                }
                false
            }
        }),
        Answer::value(Ok(vec![9])),
    );
    let captured = PanicOnDrop;
    camera.captures.expect(
        Expect::when("the second capture", move |request: &CameraRequest| {
            let _ = &captured;
            *request == CameraRequest::Capture { width: 2 }
        }),
        Answer::value(Ok(vec![2])),
    );
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drop(handle.capture(1));
    }))
    .expect_err("the capture's panic reaches the caller");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"a matcher's value panicked as it dropped")
    );
    assert_eq!(
        *heard.borrow(),
        [(2, Some(Ok(vec![2]))), (1, Some(Ok(vec![0])))],
        "the first request still reaches the standing answer"
    );
    let failures = scenario.verify().expect_err("the re-entering step stays");
    assert_eq!(
        failures.failures(),
        [ScenarioFailure::Unconsumed {
            script: "camera captures",
            step: "sends the second capture".into(),
        }]
    );
    drop(reentered);
}

/// A value whose `Drop` panics every time, unguarded: two of them dropping
/// for one request abort the process unless each drop runs contained.
struct LoudCapture;

impl Drop for LoudCapture {
    fn drop(&mut self) {
        panic!("a matcher's value panicked as it dropped");
    }
}

/// An answer whose `Drop` panics every time it is armed, unguarded.
#[derive(Debug)]
struct LoudAnswer {
    armed: bool,
}

impl Drop for LoudAnswer {
    fn drop(&mut self) {
        assert!(!self.armed, "an answer panicked as it dropped");
    }
}

/// A request whose `Drop` panics when its key is `LOUD_REQUEST`, unguarded.
#[derive(Debug)]
struct LoudRequest(u32);

const LOUD_REQUEST: u32 = 9_999;

impl Drop for LoudRequest {
    fn drop(&mut self) {
        assert!(self.0 != LOUD_REQUEST, "a request panicked as it dropped");
    }
}

/// The test's values a step lets go of and the answer it gives drop one at
/// a time, each contained: the used step's matcher goes before the answer
/// is given, so its panic is the first, and the answer's, dropped by a
/// resolver whose completion is gone, is contained where it drops; neither
/// is still held while the other unwinds, which would abort.
#[test]
fn every_value_a_step_lets_go_of_drops_contained_before_the_answer() {
    let scenario = Scenario::new();
    let (_host, _camera, handle) = camera_on(&scenario);
    let script: Script<u32, LoudAnswer> = scenario.script("loud");
    let captured = LoudCapture;
    script.expect(
        Expect::when("the request", move |_| {
            let _ = &captured;
            true
        }),
        Answer::value(LoudAnswer { armed: true }),
    );
    let (gone, resolver) = handle.completions.pending::<LoudAnswer>();
    drop(gone);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        script.receive(1, resolver);
    }))
    .expect_err("the first panic reaches the caller");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"a matcher's value panicked as it dropped"),
        "the used step is let go before its answer is given"
    );
    scenario.verify().expect("the request was answered");
}

/// The answer's drop is contained inside the delivery itself, whether the
/// answer is given at once or by the clock: when the request's last handle
/// is the one the answering code holds (a request past
/// `MAX_RECORDED_REQUESTS`), that handle is let go inside the containment
/// after the delivery returned, never while the answer's panic unwinds, and
/// the answer's panic, the earlier, is the one resumed.
#[test]
fn an_answers_drop_is_contained_before_the_request_lets_go() {
    for delayed in [false, true] {
        let scenario = Scenario::new();
        let (mut host, _camera, handle) = camera_on(&scenario);
        let script: Script<LoudRequest, LoudAnswer> = scenario.script("loud");
        script.otherwise(|request| LoudAnswer {
            armed: request.0 == LOUD_REQUEST,
        });
        for filler in 0..u32::try_from(MAX_RECORDED_REQUESTS).expect("small") {
            let (answered, resolver) = handle.completions.pending::<LoudAnswer>();
            drop(answered);
            script.receive(LoudRequest(filler), resolver);
        }
        if delayed {
            script.answer_next(
                Answer::computed(|request: &LoudRequest| LoudAnswer {
                    armed: request.0 == LOUD_REQUEST,
                })
                .after(Duration::from_millis(10)),
            );
        }
        let (gone, resolver) = handle.completions.pending::<LoudAnswer>();
        drop(gone);
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            script.receive(LoudRequest(LOUD_REQUEST), resolver);
            let _ = host.pump(Duration::from_millis(10));
        }))
        .expect_err("the answer's panic reaches the caller");
        assert_eq!(
            failure.downcast_ref::<&str>(),
            Some(&"an answer panicked as it dropped"),
            "delayed: {delayed}: the answer's drop came before the request's"
        );
        assert_eq!(script.received(), MAX_RECORDED_REQUESTS + 1);
        scenario.verify().expect("every request was answered");
    }
}

/// A request stays in the script while its answer is made: a ticket the
/// answering code drops withdraws it, and a request kept for the test while
/// a newer one entered meanwhile keeps its place ahead of it.
#[test]
fn a_request_being_answered_stays_in_the_script() {
    // Withdrawn by the matcher that is answering it.
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    let hold = camera.captures.hold();
    let (held, ticket) = handle.capture_until_dropped(1);
    let held = land(held);
    let ticket = Rc::new(RefCell::new(Some(ticket)));
    camera
        .captures
        .expect_held(Expect::when("drops its own ticket", {
            let ticket = Rc::clone(&ticket);
            move |_: &CameraRequest| {
                drop(ticket.borrow_mut().take());
                true
            }
        }));
    assert_eq!(hold.release(), 0);
    let _ = host.pump(Duration::ZERO);
    assert_eq!(camera.captures.withdrawn(), 1, "the ticket found it");
    assert_eq!(camera.captures.held(), 0, "not kept for the test");
    assert_eq!(taken(&held), Some(Err(Abandoned::ProviderDropped)));
    let failures = scenario
        .verify()
        .expect_err("the matcher withdrew its request before it returned");
    assert_eq!(
        failures.failures(),
        [ScenarioFailure::Unconsumed {
            script: "camera captures",
            step: "drops its own ticket".into(),
        }],
        "a withdrawn request is not owed, and the step its matcher withdrew it from is not used up"
    );

    // Kept for the test where it is, ahead of the request its matcher sent.
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    let answered = Rc::new(RefCell::new(Vec::<u32>::new()));
    camera.captures.on_answer({
        let answered = Rc::clone(&answered);
        move |request, answer| {
            if let (CameraRequest::Capture { width }, Some(_)) = (request, answer) {
                answered.borrow_mut().push(*width);
            }
        }
    });
    camera
        .captures
        .expect_held(Expect::exactly(CameraRequest::Capture { width: 2 }));
    let sent = Rc::new(RefCell::new(None));
    camera.captures.expect(
        Expect::when("sends a capture, then panics", {
            let handle = handle.clone();
            let sent = Rc::clone(&sent);
            move |request: &CameraRequest| {
                if *request == (CameraRequest::Capture { width: 1 }) {
                    sent.replace(Some(land(handle.capture(2))));
                    panic!("the matcher failed");
                }
                false
            }
        }),
        Answer::value(Ok(vec![9])),
    );
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drop(handle.capture(1));
    }))
    .expect_err("the matcher's panic reaches the caller");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"the matcher failed"));
    assert_eq!(camera.captures.held(), 2);
    assert!(camera.captures.release_oldest(Ok(vec![1])));
    assert_eq!(
        *answered.borrow(),
        [1],
        "the older request is released first"
    );
    assert!(camera.captures.release_oldest(Ok(vec![2])));
    let _ = host.pump(Duration::ZERO);
    let second = sent.take().expect("the matcher sent a capture");
    assert_eq!(taken(&second), Some(Ok(Ok(vec![2]))));
    let failures = scenario.verify().expect_err("the panicking step stays");
    assert_eq!(
        failures.failures(),
        [ScenarioFailure::Unconsumed {
            script: "camera captures",
            step: "sends a capture, then panics".into(),
        }]
    );
}

/// The handles `send_on` lets go of hold a matcher's or a standing answer's
/// last handle when the test's code re-entered meanwhile: a later step a
/// matcher used up, or a standing answer that replaced itself. Each is let
/// go contained before the answer is given, so its capture's panic is the
/// first, ahead of an observer's for the same request.
#[test]
fn the_last_handles_send_on_lets_go_drop_before_the_answer() {
    struct PanicOnDrop;
    impl Drop for PanicOnDrop {
        fn drop(&mut self) {
            assert!(
                std::thread::panicking(),
                "a captured value panicked as it dropped"
            );
        }
    }
    fn observe_panics_for_the_first(camera: &SimulatedCamera) {
        camera.captures.on_answer(|request, _| {
            assert!(
                *request != (CameraRequest::Capture { width: 1 }),
                "the observer failed"
            );
        });
    }

    // A matcher that accepts its request re-enters and uses a later step up.
    let scenario = Scenario::new();
    let (_host, camera, handle) = camera_on(&scenario);
    observe_panics_for_the_first(&camera);
    let sent = Rc::new(RefCell::new(None));
    camera.captures.expect(
        Expect::when("accepts the first after sending the second", {
            let handle = handle.clone();
            let sent = Rc::clone(&sent);
            move |request: &CameraRequest| {
                let first = *request == (CameraRequest::Capture { width: 1 });
                if first && sent.borrow().is_none() {
                    sent.replace(Some(land(handle.capture(2))));
                }
                first
            }
        }),
        Answer::value(Ok(vec![1])),
    );
    let captured = PanicOnDrop;
    camera.captures.expect(
        Expect::when("the second", move |request: &CameraRequest| {
            let _ = &captured;
            *request == CameraRequest::Capture { width: 2 }
        }),
        Answer::value(Ok(vec![2])),
    );
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drop(handle.capture(1));
    }))
    .expect_err("a panic reaches the caller");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"a captured value panicked as it dropped"),
        "the later step's matcher drops before the answer is given"
    );
    scenario.verify().expect("both steps were used");
    drop(sent);

    // A standing answer that replaces itself while it runs.
    let scenario = Scenario::new();
    let (_host, camera, handle) = camera_on(&scenario);
    observe_panics_for_the_first(&camera);
    let captured = PanicOnDrop;
    let script = camera.captures.clone();
    camera.captures.otherwise(move |_| {
        let _ = &captured;
        script.otherwise(|_| Ok(vec![7]));
        Ok(vec![0])
    });
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drop(handle.capture(1));
    }))
    .expect_err("a panic reaches the caller");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"a captured value panicked as it dropped"),
        "the replaced standing answer drops before the answer is given"
    );
    scenario.verify().expect("the request was answered");
}

/// A request's ticket, shared with the test's code that drops it.
type SharedTicket = Rc<RefCell<Option<RequestTicket>>>;

/// Drops the ticket it shares when it drops, as test code may.
struct DropsTheTicket(SharedTicket);

impl Drop for DropsTheTicket {
    fn drop(&mut self) {
        drop(self.0.borrow_mut().take());
    }
}

fn withdraw(ticket: &SharedTicket) {
    drop(ticket.borrow_mut().take());
}

/// Where a capture's outcome lands.
type CaptureSlot = Slot<Result<Vec<u8>, CaptureError>>;

/// A hold over a capture of width 1, received with its ticket in `ticket`,
/// and a capture of width 2: what the withdrawal tests drain.
fn first_withdrawable_then_second(
    camera: &SimulatedCamera,
    handle: &CameraHandle,
    ticket: &SharedTicket,
) -> (ScriptHold, CaptureSlot, CaptureSlot) {
    let hold = camera.captures.hold();
    let (withdrawn, kept) = handle.capture_until_dropped(1);
    let withdrawn = land(withdrawn);
    ticket.replace(Some(kept));
    let other = land(handle.capture(2));
    (hold, withdrawn, other)
}

/// A ticket the code making a request's answer drops stops that answer
/// where it is: no step is tried after the one that ran, nothing is
/// recorded as unexpected and nothing is scheduled. The step whose matcher
/// withdrew the request stays for a later request; the step whose own drop
/// withdrew it after its matcher returned is used up. The request counts as
/// withdrawn and the release counts only the other request it answered.
#[test]
fn a_request_withdrawn_while_its_answer_is_made_gets_no_more_of_it() {
    #[derive(Clone, Copy, Debug)]
    enum Withdrawer {
        /// A matcher that declines the request.
        Matcher,
        /// The used step's capture, dropped before its delayed answer.
        UsedStepCapture,
        /// The matcher of a step that abandons.
        AbandonMatcher,
    }
    for withdrawer in [
        Withdrawer::Matcher,
        Withdrawer::UsedStepCapture,
        Withdrawer::AbandonMatcher,
    ] {
        let scenario = Scenario::new();
        let (mut host, camera, handle) = camera_on(&scenario);
        let first = CameraRequest::Capture { width: 1 };
        let ticket = SharedTicket::default();
        let (hold, withdrawn, other) = first_withdrawable_then_second(&camera, &handle, &ticket);
        let mut unused = Vec::new();
        match withdrawer {
            Withdrawer::Matcher => {
                let shared = Rc::clone(&ticket);
                camera.captures.expect(
                    Expect::when("withdraws the first", move |request| {
                        if *request == first {
                            withdraw(&shared);
                        }
                        false
                    }),
                    Answer::value(Ok(vec![9])),
                );
                unused.push("withdraws the first");
            }
            Withdrawer::UsedStepCapture => {
                // Its matcher returns with the request still asked for: the
                // step is used up, and its drop withdraws the request.
                let capture = DropsTheTicket(Rc::clone(&ticket));
                camera.captures.expect(
                    Expect::when("the first", move |request| {
                        let _ = &capture;
                        *request == first
                    }),
                    Answer::value(Ok(vec![1])).after(Duration::from_millis(500)),
                );
            }
            Withdrawer::AbandonMatcher => {
                let shared = Rc::clone(&ticket);
                camera.captures.expect(
                    Expect::when("abandons the first", move |request| {
                        let accepted = *request == first;
                        if accepted {
                            withdraw(&shared);
                        }
                        accepted
                    }),
                    Answer::abandon(),
                );
                unused.push("abandons the first");
            }
        }
        // A later step and the standing answer that count how often they
        // run for the withdrawn request: they must not run at all.
        let later_calls = Rc::new(Cell::new(0_u32));
        camera.captures.expect(
            Expect::when("a later step", {
                let later_calls = Rc::clone(&later_calls);
                move |request: &CameraRequest| {
                    if *request == (CameraRequest::Capture { width: 1 }) {
                        later_calls.set(later_calls.get() + 1);
                    }
                    false
                }
            }),
            Answer::value(Ok(vec![8])),
        );
        unused.push("a later step");
        let standing_calls = Rc::new(Cell::new(0_u32));
        camera.captures.otherwise({
            let standing_calls = Rc::clone(&standing_calls);
            move |request| {
                if *request == (CameraRequest::Capture { width: 1 }) {
                    standing_calls.set(standing_calls.get() + 1);
                }
                Ok(vec![2])
            }
        });
        assert_eq!(
            hold.release(),
            1,
            "{withdrawer:?}: only the other request is counted"
        );
        let _ = host.pump(Duration::from_millis(10));
        assert_eq!(camera.captures.withdrawn(), 1, "{withdrawer:?}");
        assert_eq!(
            later_calls.get(),
            0,
            "{withdrawer:?}: no step is tried after the one that ran"
        );
        assert_eq!(
            standing_calls.get(),
            0,
            "{withdrawer:?}: the standing answer does not run"
        );
        assert_eq!(
            taken(&withdrawn),
            Some(Err(Abandoned::ProviderDropped)),
            "{withdrawer:?}"
        );
        assert_eq!(taken(&other), Some(Ok(Ok(vec![2]))), "{withdrawer:?}");
        assert_eq!(
            scenario.next_due(),
            None,
            "{withdrawer:?}: nothing is scheduled for the withdrawn request"
        );
        let expected: Vec<ScenarioFailure> = unused
            .into_iter()
            .map(|step| ScenarioFailure::Unconsumed {
                script: "camera captures",
                step: step.into(),
            })
            .collect::<Vec<ScenarioFailure>>();
        match scenario.verify() {
            Ok(()) => assert!(
                expected.is_empty(),
                "{withdrawer:?}: these steps should stay unused: {expected:?}"
            ),
            Err(failures) => assert_eq!(failures.failures(), expected, "{withdrawer:?}"),
        }
    }
}

/// A used step whose own drop withdraws its request runs none of its
/// answer: its computed answer is never called.
#[test]
fn a_used_step_withdrawn_by_its_own_drop_runs_none_of_its_answer() {
    let scenario = Scenario::new();
    let (mut host, camera, handle) = camera_on(&scenario);
    let ticket = SharedTicket::default();
    let (hold, withdrawn, other) = first_withdrawable_then_second(&camera, &handle, &ticket);
    let computed = Rc::new(Cell::new(0_u32));
    let capture = DropsTheTicket(Rc::clone(&ticket));
    camera.captures.expect(
        Expect::when("the first", move |request: &CameraRequest| {
            let _ = &capture;
            *request == CameraRequest::Capture { width: 1 }
        }),
        Answer::computed({
            let computed = Rc::clone(&computed);
            move |_| {
                computed.set(computed.get() + 1);
                Ok(vec![1])
            }
        }),
    );
    camera.captures.otherwise(|_| Ok(vec![2]));
    assert_eq!(hold.release(), 1);
    let _ = host.pump(Duration::ZERO);
    assert_eq!(
        computed.get(),
        0,
        "the withdrawn request's answer is not computed"
    );
    assert_eq!(camera.captures.withdrawn(), 1);
    assert_eq!(taken(&withdrawn), Some(Err(Abandoned::ProviderDropped)));
    assert_eq!(taken(&other), Some(Ok(Ok(vec![2]))));
    scenario.verify().expect("the step was used up");
}

/// A used step whose own drop withdraws its request schedules none of a
/// delayed answer: the request is never described for the timeline.
#[test]
fn a_used_step_withdrawn_by_its_own_drop_schedules_none_of_its_answer() {
    /// A request that counts how often it is described.
    struct Counted(Rc<Cell<u32>>);
    impl std::fmt::Debug for Counted {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            self.0.set(self.0.get() + 1);
            f.write_str("Counted")
        }
    }
    let scenario = Scenario::new();
    let (mut host, _camera, handle) = camera_on(&scenario);
    let script: Script<Counted, u32> = scenario.script("counted");
    let ticket = SharedTicket::default();
    let capture = DropsTheTicket(Rc::clone(&ticket));
    script.expect(
        Expect::when("the request", move |_| {
            let _ = &capture;
            true
        }),
        Answer::value(1).after(Duration::from_millis(10)),
    );
    let hold = script.hold();
    let described = Rc::new(Cell::new(0_u32));
    let (answer, resolver) = handle.completions.pending::<u32>();
    let answer = land(answer);
    ticket.replace(Some(
        script.receive_with_ticket(Counted(Rc::clone(&described)), resolver),
    ));
    assert_eq!(hold.release(), 0);
    assert_eq!(described.get(), 0, "the withdrawn request is not described");
    assert_eq!(scenario.next_due(), None, "nothing is scheduled");
    let _ = host.pump(Duration::from_millis(10));
    assert_eq!(script.withdrawn(), 1);
    assert_eq!(taken(&answer), Some(Err(Abandoned::ProviderDropped)));
    scenario.verify().expect("the step was used up");
}

/// A request its own answer withdraws (a computed answer, the standing
/// answer, a computed answer the clock gives) is neither answered nor
/// counted: the release counts only the other request.
#[test]
fn a_request_its_own_answer_withdraws_is_not_answered_or_counted() {
    #[derive(Clone, Copy, Debug)]
    enum Withdrawer {
        Computed,
        Standing,
        Delayed,
    }
    for withdrawer in [
        Withdrawer::Computed,
        Withdrawer::Standing,
        Withdrawer::Delayed,
    ] {
        let scenario = Scenario::new();
        let (mut host, camera, handle) = camera_on(&scenario);
        let first = CameraRequest::Capture { width: 1 };
        let ticket = SharedTicket::default();
        let (hold, withdrawn, other) = first_withdrawable_then_second(&camera, &handle, &ticket);
        let shared = Rc::clone(&ticket);
        let withdrawing = move |_: &CameraRequest| {
            withdraw(&shared);
            Ok(vec![1])
        };
        match withdrawer {
            Withdrawer::Computed => {
                camera
                    .captures
                    .expect(Expect::exactly(first), Answer::computed(withdrawing));
                camera.captures.otherwise(|_| Ok(vec![2]));
            }
            Withdrawer::Standing => {
                camera.captures.otherwise(move |request| {
                    if *request == first {
                        withdrawing(request)
                    } else {
                        Ok(vec![2])
                    }
                });
            }
            Withdrawer::Delayed => {
                camera.captures.expect(
                    Expect::exactly(first),
                    Answer::computed(withdrawing).after(Duration::from_millis(10)),
                );
                camera.captures.otherwise(|_| Ok(vec![2]));
            }
        }
        assert_eq!(
            hold.release(),
            1,
            "{withdrawer:?}: only the other request is counted"
        );
        let _ = host.pump(Duration::from_millis(10));
        assert_eq!(camera.captures.withdrawn(), 1, "{withdrawer:?}");
        assert_eq!(
            taken(&withdrawn),
            Some(Err(Abandoned::ProviderDropped)),
            "{withdrawer:?}"
        );
        assert_eq!(taken(&other), Some(Ok(Ok(vec![2]))), "{withdrawer:?}");
        scenario.verify().expect("every step was used");
    }
}

/// The request's `Debug` is the test's code too: one that drops its own
/// ticket while the script describes it leaves nothing recorded as
/// unexpected and nothing scheduled.
#[test]
fn a_request_whose_debug_withdraws_it_is_neither_unexpected_nor_scheduled() {
    type Ticket = Rc<RefCell<Option<RequestTicket>>>;
    /// A request that drops its own ticket when it is described.
    struct Withdrawing(Ticket);
    impl std::fmt::Debug for Withdrawing {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            drop(self.0.borrow_mut().take());
            f.write_str("Withdrawing")
        }
    }
    for delayed_step in [false, true] {
        let scenario = Scenario::new();
        let (mut host, _camera, handle) = camera_on(&scenario);
        let script: Script<Withdrawing, u32> = scenario.script("describes");
        if delayed_step {
            script.answer_next(Answer::value(1).after(Duration::from_millis(10)));
        }
        let hold = script.hold();
        let ticket = Ticket::default();
        let (answer, resolver) = handle.completions.pending::<u32>();
        let answer = land(answer);
        ticket.replace(Some(
            script.receive_with_ticket(Withdrawing(Rc::clone(&ticket)), resolver),
        ));
        assert_eq!(hold.release(), 0, "delayed step: {delayed_step}");
        assert_eq!(
            scenario.next_due(),
            None,
            "delayed step: {delayed_step}: nothing is left scheduled"
        );
        let _ = host.pump(Duration::from_millis(10));
        assert_eq!(script.withdrawn(), 1, "delayed step: {delayed_step}");
        assert_eq!(taken(&answer), Some(Err(Abandoned::ProviderDropped)));
        scenario
            .verify()
            .expect("neither unexpected nor a change not reached");
    }
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
    let hold = camera.captures.hold();
    let released = land(handle.capture(4));
    let abandoned = land(handle.capture(5));
    assert!(camera.captures.release_oldest(Ok(vec![])));
    assert_eq!(camera.captures.abandon_held(), 1);
    assert_eq!(
        hold.release(),
        0,
        "the release and the abandon left none held"
    );
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
    let _held = camera.captures.hold();
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
