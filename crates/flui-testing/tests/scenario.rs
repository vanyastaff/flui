//! The scenario primitive as a capability's simulated provider meets it
//! (ADR-0154 §10): ordered answers matched by request, a permission revoked
//! on virtual time, a service turned off, injected errors, an event stream
//! and its end, and the typed failures of a run that left the script.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_platform::capability::scripted::{
    Answer, EventSubscription, Events, Expect, Scenario, ScenarioFailure, Script, ServiceState,
    StreamEnd, StreamItem, Track,
};
use flui_platform::capability::{
    Abandoned, Capability, CapabilityRegistrar, Completion, CompletionSource, Plugin,
    ProviderContext,
};
use flui_platform::{Unsupported, UnsupportedReason};
use flui_testing::{HeadlessHost, HeadlessWindow};

use crate::capability_seam::acquire_capability;

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
    // A widget mounted after 1 s, in a host on the same timeline.
    let mut later = HeadlessHost::with_scenario(HeadlessWindow::new(10, 10), &scenario, &camera);
    assert!(acquire_capability::<Camera>(&mut later).is_ok());
    scenario.verify().expect("the service came back");
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
