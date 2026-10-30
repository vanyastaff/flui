//! Protocol 0.5's wire spelling, one table: each line is the JSON a client writes or reads,
//! and it must read into the type and write back unchanged. A field respelled, a variant
//! renamed or a default no longer left out changes a line here.

#![cfg(feature = "serde")]

use std::fmt::Debug;

use flui_protocol::method::{MethodEffect, ServedBy, Tier};
use flui_protocol::{
    ActOutcome, ActParams, ActionName, Actionability, ActionabilityCheck, AuditReport, AuditRule,
    CaptureMeta, CaptureSource, ChangedField, Condition, DeviceClass, Direction, EffectKind,
    EffectUnit, ElementRef, ErrorCode, ErrorObject, Event, EventKind, FramePhase, HandleKind,
    Hello, InputKind, InputParams, InputPath, Isolation, Journal, LocateParams, Locator, Matches,
    PerfExpectParams, PerfReport, PointerButton, PointerDevice, PressKeysParams, ProtocolVersion,
    Role, ScrollUnit, Snapshot, SwipeParams, TapParams, Target, TextMatch, UnsettledReason,
    Verdict, WaitOutcome, WaitParams,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// `json` reads into `T` and writes back as the same JSON.
fn round_trips<T: Serialize + DeserializeOwned + Debug>(json: Value) -> T {
    let value: T = serde_json::from_value(json.clone()).unwrap_or_else(|e| {
        panic!(
            "{json} does not read as {}: {e}",
            std::any::type_name::<T>()
        )
    });
    assert_eq!(
        serde_json::to_value(&value).expect("a wire value serializes"),
        json,
        "{} does not write back as it was read",
        std::any::type_name::<T>()
    );
    value
}

/// Every variant of a vocabulary enum is spelled as its `name()`.
fn spelled_as_named<T>(all: &[T], name: impl Fn(T) -> &'static str)
where
    T: Serialize + DeserializeOwned + Copy + PartialEq + Debug,
{
    for &variant in all {
        let json = Value::String(name(variant).to_owned());
        assert_eq!(serde_json::to_value(variant).ok(), Some(json.clone()));
        assert_eq!(serde_json::from_value::<T>(json).ok(), Some(variant));
    }
}

#[test]
fn every_vocabulary_enum_is_spelled_as_it_is_named() {
    spelled_as_named(ActionabilityCheck::ALL, ActionabilityCheck::name);
    spelled_as_named(AuditRule::ALL, AuditRule::name);
    spelled_as_named(CaptureSource::ALL, CaptureSource::name);
    spelled_as_named(ChangedField::ALL, ChangedField::name);
    spelled_as_named(DeviceClass::ALL, DeviceClass::name);
    spelled_as_named(Direction::ALL, Direction::name);
    spelled_as_named(EffectKind::ALL, EffectKind::name);
    spelled_as_named(EffectUnit::ALL, EffectUnit::name);
    spelled_as_named(ErrorCode::ALL, ErrorCode::name);
    spelled_as_named(EventKind::ALL, EventKind::name);
    spelled_as_named(FramePhase::ALL, FramePhase::name);
    spelled_as_named(HandleKind::ALL, HandleKind::name);
    spelled_as_named(InputKind::ALL, InputKind::name);
    spelled_as_named(InputPath::ALL, InputPath::name);
    spelled_as_named(Isolation::ALL, Isolation::name);
    spelled_as_named(MethodEffect::ALL, MethodEffect::name);
    spelled_as_named(PointerButton::ALL, PointerButton::name);
    spelled_as_named(PointerDevice::ALL, PointerDevice::name);
    spelled_as_named(ScrollUnit::ALL, ScrollUnit::name);
    spelled_as_named(ServedBy::ALL, ServedBy::name);
    spelled_as_named(Snapshot::ALL, Snapshot::name);
    spelled_as_named(Tier::ALL, Tier::name);
    spelled_as_named(UnsettledReason::ALL, UnsettledReason::name);
    spelled_as_named(Verdict::ALL, Verdict::name);
    {
        use flui_protocol::native_recording::{
            Backend, CancelReason, GestureComponent, GesturePhase, Modifier, PointerKind,
            ScrollPhase, ScrollUnit, ViewportStatus, WheelAxis,
        };
        spelled_as_named(Backend::ALL, Backend::name);
        spelled_as_named(CancelReason::ALL, CancelReason::name);
        spelled_as_named(GestureComponent::ALL, GestureComponent::name);
        spelled_as_named(GesturePhase::ALL, GesturePhase::name);
        spelled_as_named(Modifier::ALL, Modifier::name);
        spelled_as_named(PointerKind::ALL, PointerKind::name);
        spelled_as_named(ScrollPhase::ALL, ScrollPhase::name);
        spelled_as_named(ScrollUnit::ALL, ScrollUnit::name);
        spelled_as_named(ViewportStatus::ALL, ViewportStatus::name);
        spelled_as_named(WheelAxis::ALL, WheelAxis::name);
    }
}

#[test]
fn a_locator_writes_only_what_it_requires() {
    let locator: Locator = round_trips(json!({
        "window": "w3",
        "role": "button",
        "name": {"match": "exact", "text": "Increment"},
        "value": {"match": "prefix", "text": "1", "ignore_case": true},
        "identifier": "counter",
        "state": {"disabled": false, "checked": true},
        "within": {"role": "group"},
        "has": {"name": {"match": "contains", "text": "+"}},
        "pick": {"nth": 2},
    }));
    assert_eq!(locator.role, Some(Role::Button));
    let built = Locator::role(Role::Button).name("Increment");
    assert_eq!(
        serde_json::to_value(&built).ok(),
        Some(json!({"role": "button", "name": {"match": "exact", "text": "Increment"}})),
        "a strict pick and an empty state are left out"
    );
    for pick in [json!("first"), json!("last")] {
        round_trips::<Locator>(json!({"role": "button", "pick": pick}));
    }
    assert_eq!(
        serde_json::from_value::<Locator>(json!({"role": "button", "pick": "strict"})).ok(),
        Some(Locator::role(Role::Button)),
        "`strict` is the default pick, spelled out or not"
    );
    assert_eq!(
        TextMatch::contains("Save").ignoring_case(),
        serde_json::from_value(json!({"match": "contains", "text": "Save", "ignore_case": true}))
            .expect("a text match reads")
    );
}

#[test]
fn an_element_ref_is_window_slash_element() {
    let element: ElementRef = round_trips(json!("w3/e12"));
    assert_eq!(element.to_string(), "w3/e12");
    for refused in [
        "w3",
        "e12",
        "w3/",
        "/e12",
        "w3/w12",
        "e3/e12",
        "w03/e12",
        "w3/e12/e1",
    ] {
        assert!(
            refused.parse::<ElementRef>().is_err(),
            "`{refused}` is not an element ref"
        );
    }
}

#[test]
fn targets_and_acts_name_what_they_aim_at() {
    round_trips::<Target>(json!({"element": "w3/e12"}));
    round_trips::<Target>(json!({"locator": {"identifier": "save"}}));
    round_trips::<Target>(json!({"point": {"window": "w3", "at": {"x": 10.0, "y": 20.5}}}));
    let act: ActParams = round_trips(json!({
        "target": {"element": "w3/e12"},
        "action": "set_value",
        "value": "42",
        "snapshot": "delta",
    }));
    assert_eq!(act.action, ActionName::SetValue);
    round_trips::<ActParams>(json!({"target": {"element": "w3/e12"}, "action": "invoke"}));
    round_trips::<LocateParams>(json!({"locator": {"role": "button"}, "max_matches": 5}));
    round_trips::<Matches>(json!({
        "matches": [{
            "element": "w3/e12",
            "revision": 7,
            "node": {"id": "e12", "role": "button", "native_role": "Button", "identifier": "save"},
        }],
        "truncated": 3,
    }));
}

#[test]
fn input_is_a_per_device_action_sequence_and_gestures_flatten_their_options() {
    round_trips::<InputParams>(json!({
        "window": "w3",
        "actions": {"sources": [
            {"type": "pointer", "id": "finger", "device": "touch", "actions": [
                {"type": "move", "to": {"x": 10.0, "y": 10.0}},
                {"type": "down", "pressure": 0.5},
                {"type": "move", "to": {"x": 80.0, "y": 10.0}, "duration_ms": 120},
                {"type": "up"},
                {"type": "cancel"},
            ]},
            {"type": "pointer", "id": "mouse", "actions": [
                {"type": "down", "button": "secondary"},
                {"type": "pause", "duration_ms": 16},
            ]},
            {"type": "wheel", "id": "wheel", "actions": [
                {"type": "scroll", "at": {"x": 5.0, "y": 5.0}, "delta_y": 3.0, "unit": "line"},
                {"type": "pause"},
            ]},
            {"type": "keys", "id": "kbd", "actions": [
                {"type": "down", "key": "Control"},
                {"type": "down", "key": "a"},
                {"type": "up", "key": "a"},
                {"type": "up", "key": "Control"},
            ]},
            {"type": "text", "text": "héllo"},
            {"type": "ime", "actions": [
                {"type": "set_composition", "text": "ni", "cursor": 2},
                {"type": "commit", "text": "你"},
                {"type": "cancel"},
            ]},
            {"type": "trackpad", "id": "pad", "actions": [
                {"type": "begin", "at": {"x": 50.0, "y": 50.0}},
                {"type": "pinch", "scale": 1.5},
                {"type": "rotate", "degrees": 15.0},
                {"type": "pan", "dx": 4.0, "dy": -2.0},
                {"type": "end"},
            ]},
        ]},
        "via": {"prefer": "os", "require": true},
        "snapshot": "full",
    }));
    let tap: TapParams = round_trips(json!({
        "target": {"locator": {"role": "button"}},
        "via": {"prefer": "semantic"},
        "force": true,
        "timeout_ms": 1000,
        "snapshot": "delta",
    }));
    assert!(tap.options.force, "gesture options sit beside the target");
    let session_default: InputParams =
        round_trips(json!({"window": "w3", "actions": {"sources": []}}));
    assert_eq!(
        session_default.via, None,
        "an omitted path is the session's"
    );
    let synthetic: InputParams = round_trips(json!({
        "window": "w3",
        "actions": {"sources": []},
        "via": {},
    }));
    assert_eq!(
        synthetic.via.map(|policy| policy.prefer),
        Some(InputPath::Synthetic),
        "an explicit synthetic path stays explicit"
    );
    round_trips::<TapParams>(json!({"target": {"element": "w3/e12"}, "button": "secondary"}));
    round_trips::<SwipeParams>(json!({
        "target": {"element": "w3/e12"},
        "direction": "up",
        "distance": 200.0,
        "velocity": 3000.0,
    }));
    round_trips::<PressKeysParams>(json!({"window": "w3", "keys": ["Control+a", "Delete"]}));
    round_trips::<Journal>(json!({
        "entries": [{
            "seq": 41,
            "device": "touch",
            "kind": "pointer_down",
            "at": {"x": 10.0, "y": 10.0},
            "time_us": 16_667,
            "injected": true,
        }, {"seq": 42, "device": "keyboard", "kind": "key_down", "time_us": 33_334}],
        "span": {"from": 41, "to": 42},
        "dropped": 7,
    }));
}

#[test]
fn an_action_answers_its_path_its_effect_and_a_delta() {
    let outcome: ActOutcome = round_trips(json!({
        "via": "synthetic",
        "effect": {"kind": "ran"},
        "element": "w3/e12",
        "observed": {"from": 41, "to": 44},
        "snapshot": {"delta": {
            "from": 7,
            "to": 8,
            "appeared": [{"id": "e20", "role": "dialog", "native_role": "Dialog"}],
            "gone": ["w3/e15"],
            "changed": [{
                "node": {"id": "e12", "role": "button", "native_role": "Button", "name": "1"},
                "changed": ["name", "state"],
            }],
        }},
    }));
    assert_eq!(outcome.via, InputPath::Synthetic);
    round_trips::<ActOutcome>(json!({
        "via": "os",
        "effect": {"kind": "partial", "detail": "the third chord may have run", "sent": 2,
                   "total": 4, "unit": "chord"},
        "readback_failed": true,
    }));
    round_trips::<ActOutcome>(json!({
        "via": "semantic",
        "effect": {"kind": "ran"},
        "snapshot": {"full": {"protocol": "0.5", "revision": 8, "roots": [], "count": 0,
                              "truncated": false}},
    }));
    let facts: Actionability = round_trips(json!({
        "attached": true,
        "rect": {"x": 8.0, "y": 8.0, "width": 100.0, "height": 32.0},
        "visible": true,
        "stable": true,
        "enabled": true,
        "receives": false,
        "point": {"x": 58.0, "y": 24.0},
        "covered_by": "- dialog \"Saving\" [ref=e20]",
        "revision": 8,
    }));
    assert_eq!(facts.first_failed(), Some(ActionabilityCheck::Receives));
}

#[test]
fn waits_take_conditions_and_answer_the_last_observation() {
    let wait: WaitParams = round_trips(json!({
        "window": "w3",
        "condition": {"all": [
            {"appears": {"role": "dialog"}},
            {"gone": {"identifier": "spinner"}},
            {"matches": {"locator": {"identifier": "counter"},
                         "expect": {"value": {"match": "exact", "text": "1"}}}},
            {"matches": {"locator": {"role": "check_box"}, "expect": "focused"}},
            {"matches": {"locator": {"role": "check_box"}, "expect": {"count": 3}}},
            {"matches": {"locator": {"role": "check_box"},
                         "expect": {"state": {"checked": "mixed"}}}},
            {"any": ["settled", {"frames": 3}]},
            {"event": {"kind": "semantics", "window": "w3"}},
        ]},
        "timeout_ms": 2000,
        "max_frames": 120,
    }));
    assert!(matches!(wait.condition, Condition::All(ref all) if all.len() == 8));
    round_trips::<WaitOutcome>(json!({
        "satisfied": false,
        "frames": 300,
        "observation": {"revision": 9, "count": 0},
        "unsettled": [
            {"reason": "transient_callbacks", "count": 2},
            {"reason": "timer", "due_in_ms": 250},
        ],
    }));
}

#[test]
fn events_are_named_with_their_params() {
    let frame: Event = round_trips(json!({
        "event": "frame",
        "params": {"window": "w3", "frame": 120, "presented": true, "time_us": 2_000_000},
    }));
    assert_eq!(frame.kind(), EventKind::Frame);
    round_trips::<Event>(json!({"event": "gap", "params": {"missed": 12}}));
    round_trips::<Event>(json!({"event": "semantics", "params": {"window": "w3", "revision": 9}}));
    round_trips::<Event>(json!({"event": "window", "params": {"window": "w4", "opened": true}}));
}

#[test]
fn captures_perf_and_audits_report_what_they_measured() {
    let capture: CaptureMeta = round_trips(json!({
        "source": "rendered",
        "width": 720,
        "height": 96,
        "scale": 2.0,
        "frame": 120,
        "region": {"x": 0.0, "y": 0.0, "width": 360.0, "height": 48.0},
        "blob": {"id": 1, "bytes": 4096, "media": "image/png"},
    }));
    assert_eq!(capture.source, CaptureSource::Rendered);
    let report: PerfReport = round_trips(json!({
        "frames": 120,
        "frame_time": {"p50": 4.0, "p90": 6.0, "p95": 7.5, "p99": 12.0, "max": 20.0},
        "refresh_period_ms": 16.667,
        "dropped": 1,
        "janky": 0,
        "worst": [{"frame": 88, "ms": 20.0,
                   "phases": [{"phase": "layout", "ms": 9.0}], "tags": ["scrolling"]}],
    }));
    assert_eq!(report.dropped, Some(1));
    round_trips::<PerfReport>(json!({
        "frames": 60,
        "frame_time": {"p50": 1.0, "p90": 1.0, "p95": 1.0, "p99": 1.0, "max": 1.0},
    }));
    round_trips::<PerfExpectParams>(json!({
        "target": "android:SM-G991B",
        "budgets": [{"target": "android:SM-G991B",
                     "budget": {"max_dropped": 2, "p95_frame_time": {"periods": 1.0},
                                "p95_input_to_present": {"ms": 50.0}}}],
        "reports": [],
    }));
    let audit: AuditReport = round_trips(json!({
        "verdict": "cannot_verify",
        "checked": ["unnamed", "target_size"],
        "not_checked": [{"rule": "focus_order", "reason": "no keyboard on this target"}],
    }));
    assert_eq!(audit.verdict, Verdict::CannotVerify);
}

#[test]
fn an_error_carries_its_codes_fields_and_reads_a_newer_code_as_platform() {
    let ambiguous: ErrorObject = round_trips(json!({
        "code": "ambiguous",
        "message": "2 buttons match",
        "retry": "never",
        "candidates": ["- button \"OK\" [ref=e3]", "- button \"OK\" [ref=e9]"],
    }));
    assert_eq!(
        ambiguous,
        ErrorObject::ambiguous(
            "2 buttons match",
            vec![
                "- button \"OK\" [ref=e3]".into(),
                "- button \"OK\" [ref=e9]".into()
            ],
        )
    );
    round_trips::<ErrorObject>(json!({
        "code": "not_actionable",
        "message": "covered",
        "retry": "when_appears",
        "failed": "receives",
        "covered_by": "- dialog [ref=e20]",
    }));
    round_trips::<ErrorObject>(json!({
        "code": "gone",
        "message": "the app exited",
        "retry": "never",
        "kind": "process",
        "exit": 101,
    }));
    round_trips::<ErrorObject>(json!({
        "code": "timeout",
        "message": "the window did not answer in time",
        "retry": "soon",
        "effect": {"kind": "may_have_run", "detail": "the action is still queued"},
    }));
    round_trips::<ErrorObject>(json!({
        "code": "not_supported",
        "message": "no pen on this target",
        "retry": "never",
        "capability": "pen",
    }));
    let version = ErrorObject::version(ProtocolVersion::new(0, 5), ProtocolVersion::new(0, 6));
    assert_eq!(
        serde_json::to_value(&version).ok(),
        Some(json!({
            "code": "version",
            "message": "the app speaks protocol 0.5, the client 0.6",
            "retry": "never",
            "app": "0.5",
            "client": "0.6",
        }))
    );
    let newer: ErrorObject = serde_json::from_value(json!({
        "code": "rate_limited", "message": "slow down", "retry": "soon"
    }))
    .expect("an error with a newer code reads");
    assert_eq!(newer.code, ErrorCode::Platform);
}

#[test]
fn a_hello_names_the_token_version_and_client() {
    let hello: Hello = round_trips(json!({
        "token": "secret",
        "protocol": "0.5",
        "client": "flui-driver/0.1.0",
    }));
    assert_eq!(hello.protocol, ProtocolVersion::new(0, 5));
}
