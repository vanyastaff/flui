//! The lines of a conversation (ADR-0238 §2): each [`ServerLine`] is read by its keys, each
//! client line round-trips, and an app serves every older client no break separates from it.

#![cfg(feature = "serde")]

use flui_protocol::version::BREAKING;
use flui_protocol::{
    ErrorCode, ErrorObject, Event, FrameEvent, GapEvent, Greeting, Hello, HelloReply,
    PROTOCOL_VERSION, ProtocolVersion, Request, RequestId, ServerLine, WindowId,
};
use serde_json::{Value, json};

fn read(line: &Value) -> Result<ServerLine<Value>, String> {
    serde_json::from_value::<ServerLine<Value>>(line.clone()).map_err(|error| error.to_string())
}

fn window(id: u64) -> WindowId {
    WindowId::from_u64(id).expect("a window id is not zero")
}

#[test]
fn a_server_line_is_read_by_its_keys_and_writes_back_the_same() {
    let frame = Event::Frame(FrameEvent::new(window(2), 7, true));
    let table: &[(&str, Value, ServerLine<Value>)] = &[
        (
            "the hello",
            json!({"hello": {"protocol": "0.9"}}),
            ServerLine::hello(HelloReply::new(ProtocolVersion::new(0, 9))),
        ),
        (
            "a result",
            json!({"id": 7, "result": {"windows": ["w1"]}}),
            ServerLine::answer(RequestId::Number(7), Ok(json!({"windows": ["w1"]}))),
        ),
        (
            "a null result is still a result",
            json!({"id": "a", "result": null}),
            ServerLine::answer(RequestId::Text("a".into()), Ok(Value::Null)),
        ),
        (
            "an error",
            json!({"id": 8, "error": {"code": "busy", "message": "full", "retry": "soon"}}),
            ServerLine::answer(
                RequestId::Number(8),
                Err(ErrorObject::new(ErrorCode::Busy, "full")
                    .with_retry(flui_protocol::Retry::Soon)),
            ),
        ),
        (
            "an error that answers no request",
            json!({"error": {"code": "version", "message": "old", "retry": "never"}}),
            ServerLine::refusal(ErrorObject::new(ErrorCode::Version, "old")),
        ),
        (
            "an event",
            json!({"event": "frame", "params": {"window": "w2", "frame": 7, "presented": true}}),
            ServerLine::Event(frame),
        ),
    ];
    for (case, line, expected) in table {
        assert_eq!(read(line).as_ref(), Ok(expected), "{case}: read");
        assert_eq!(
            serde_json::to_value(expected).ok().as_ref(),
            Some(line),
            "{case}: written"
        );
    }
}

#[test]
fn a_server_line_reads_past_what_it_does_not_know_and_says_what_is_wrong() {
    // A null id on an error is an error that answers no request, as an older app wrote it.
    assert_eq!(
        read(&json!({"id": null, "error": {"code": "busy", "message": "m", "retry": "never"}})),
        Ok(ServerLine::refusal(
            ErrorObject::new(ErrorCode::Busy, "m").with_retry(flui_protocol::Retry::Never)
        ))
    );
    // A key this reader does not know is a newer app's addition.
    assert_eq!(
        read(&json!({"id": 1, "result": 3, "trace": {"span": 9}})),
        Ok(ServerLine::answer(RequestId::Number(1), Ok(json!(3))))
    );
    // The event's own fields are its params', wherever the line's other keys are.
    assert_eq!(
        read(&json!({"note": 1, "event": "gap", "params": {"missed": 4}})),
        Ok(ServerLine::Event(Event::Gap(GapEvent::new(4))))
    );

    // Written as text: key order and repeated keys are what is under test.
    let refused: &[(&str, &str, &str)] = &[
        ("an empty object", "{}", "names none"),
        (
            "a result and an error",
            r#"{"id":1,"result":1,"error":{"code":"busy","message":"m","retry":"never"}}"#,
            "names several",
        ),
        (
            "a hello and an event",
            r#"{"hello":{"protocol":"0.9"},"event":"gap","params":{"missed":1}}"#,
            "names several",
        ),
        (
            "a result without an id",
            r#"{"result":1}"#,
            "missing field `id`",
        ),
        (
            "an event without params",
            r#"{"event":"gap"}"#,
            "missing field `params`",
        ),
        (
            "params before the event's kind",
            r#"{"params":{"missed":1},"event":"gap"}"#,
            "names its `event` before its `params`",
        ),
        (
            "an event of a kind this build does not know",
            r#"{"event":"teleport","params":{}}"#,
            "unknown variant `teleport`",
        ),
        (
            "a repeated key",
            r#"{"id":1,"id":2,"result":1}"#,
            "duplicate field `id`",
        ),
        ("not an object", "[1,2]", "a server line"),
    ];
    for (case, line, says) in refused {
        let refused = serde_json::from_str::<ServerLine<Value>>(line);
        assert!(
            refused
                .as_ref()
                .is_err_and(|why| why.to_string().contains(says)),
            "{case}: expected an error saying {says:?}, got {refused:?}"
        );
    }
}

#[test]
fn a_client_line_round_trips() {
    let greeting = Greeting::new(Hello::new("t".repeat(32), PROTOCOL_VERSION).with_client("c/1"));
    let written = serde_json::to_value(&greeting).expect("a greeting writes");
    assert_eq!(
        written,
        json!({"hello": {"token": "t".repeat(32), "protocol": PROTOCOL_VERSION.to_string(), "client": "c/1"}})
    );
    assert_eq!(
        serde_json::from_value::<Greeting>(written).ok(),
        Some(greeting)
    );

    let request = Request::new(7_u64, "windows.list", json!({}));
    let written = serde_json::to_value(&request).expect("a request writes");
    assert_eq!(
        written,
        json!({"id": 7, "method": "windows.list", "params": {}})
    );
    assert_eq!(
        serde_json::from_value::<Request<Value>>(written).ok(),
        Some(request)
    );
    // Absent params read as the default, a string id as text.
    assert_eq!(
        serde_json::from_value::<Request<Value>>(json!({"id": "x", "method": "windows.list"})).ok(),
        Some(Request::new(
            RequestId::Text("x".into()),
            "windows.list",
            Value::Null
        ))
    );
    for refused in [
        json!({"id": 1.5, "method": "m"}),
        json!({"id": true, "method": "m"}),
    ] {
        assert!(
            serde_json::from_value::<Request<Value>>(refused.clone()).is_err(),
            "{refused} carries no request id"
        );
    }
}

#[test]
fn an_app_serves_every_older_client_no_break_separates_from_it() {
    let v = ProtocolVersion::new;
    let broke = BREAKING
        .iter()
        .map(|broke| broke.version)
        .max()
        .expect("a break is listed");
    let table: &[(&str, ProtocolVersion, ProtocolVersion, bool)] = &[
        ("the same version", PROTOCOL_VERSION, PROTOCOL_VERSION, true),
        ("an older minor", v(0, 9), v(0, 8), true),
        ("the version that broke", PROTOCOL_VERSION, broke, true),
        (
            "a minor before the break",
            PROTOCOL_VERSION,
            v(broke.major, broke.minor - 1),
            false,
        ),
        ("a newer client", v(0, 8), v(0, 9), false),
        ("another major", v(1, 0), v(0, 9), false),
        ("an older major", v(0, 9), v(1, 0), false),
    ];
    for (case, app, client, served) in table {
        assert_eq!(
            app.serves(*client),
            *served,
            "{case}: {app} serving {client}"
        );
    }
}
