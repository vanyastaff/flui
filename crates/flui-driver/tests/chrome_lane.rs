//! The Chrome lane on every host: its tables (keys, actions, input-method steps, screenshots)
//! and, against a stand-in BiDi session
//! (`ScriptedBrowser`), the session, the lane and the page's connection: one step a move, held
//! input released or named, answers matched by id, untrusted sizes refused, the canvas located
//! and captured.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use base64::Engine as _;
use flui_driver::coordinates::DevicePoint;
use flui_driver::lane::chrome::{PAGE_ROOM, actions, ime, keys, screenshot};
use flui_driver::protocol::act::EffectKind;
use flui_driver::protocol::capture::CaptureSource;
use flui_driver::protocol::geometry::LogicalPoint;
use flui_driver::protocol::input::{
    ActionSequence, ImeAction, InputSource, KeyAction, PointerAction, PointerButton, PointerDevice,
};
use flui_driver::protocol::method::{self, Method};
use flui_driver::protocol::tree::WindowId;
use flui_driver::steps::OsEvent;
use flui_driver::testing::{BrowserReply, Reply, Script, ScriptedBrowser};
use flui_driver::{Driver, Error, InputPath, InputPolicy, Isolation};
use serde_json::{Value, json};

// --- Tables ---

#[test]
fn a_key_name_is_its_webdriver_value_and_text_its_grapheme_clusters() {
    for (name, value) in [
        ("Enter", Some("\u{E007}")),
        ("Tab", Some("\u{E004}")),
        ("Control", Some("\u{E009}")),
        ("Meta", Some("\u{E03D}")),
        ("ArrowLeft", Some("\u{E012}")),
        ("F12", Some("\u{E03C}")),
        ("a", Some("a")),
        ("A", Some("A")),
        (" ", Some(" ")),
        ("日", Some("日")),
        ("e\u{301}", Some("e\u{301}")),
        ("CapsLock", None),
        ("F13", None),
        ("ab", None),
        ("\u{7}", None),
        ("", None),
    ] {
        assert_eq!(keys::key_value(name).as_deref(), value, "{name:?}");
    }
    assert_eq!(
        keys::text_keys("Grüße e\u{301}\r\n\t日").expect("typable"),
        [
            "G", "r", "ü", "ß", "e", " ", "e\u{301}", "\u{E007}", "\u{E004}", "日"
        ]
    );
    assert_eq!(keys::text_keys("a\u{1b}b"), Err('\u{1b}'));
}

fn single(actions: &Value) -> (&Value, &Value) {
    let sources = actions.as_array().expect("a source list");
    assert_eq!(sources.len(), 1, "one source: {actions}");
    let steps = sources[0]["actions"].as_array().expect("its actions");
    (&sources[0], &steps[0])
}

#[test]
fn each_event_is_one_command_of_one_instant_action() {
    let mouse = |to: DevicePoint| OsEvent::PointerMove {
        pointer: "m".into(),
        device: PointerDevice::Mouse,
        to,
    };
    let moved = actions::perform_actions(&mouse(DevicePoint::new(200, 101)), 2.0)
        .expect("a move")
        .expect("a command");
    let (source, step) = single(&moved);
    assert_eq!(source["type"], "pointer");
    assert_eq!(source["parameters"]["pointerType"], "mouse");
    assert_eq!(
        step,
        &json!({ "type": "pointerMove", "x": 100, "y": 50.5, "duration": 0, "origin": "viewport" })
    );
    for (device, kind) in [(PointerDevice::Touch, "touch"), (PointerDevice::Pen, "pen")] {
        let down = actions::perform_actions(
            &OsEvent::PointerDown {
                pointer: "p".into(),
                device,
                button: PointerButton::Primary,
            },
            1.0,
        )
        .expect("a press")
        .expect("a command");
        let (source, step) = single(&down);
        assert_eq!(source["parameters"]["pointerType"], kind);
        assert_eq!(step, &json!({ "type": "pointerDown", "button": 0 }));
    }
    for (button, number) in [
        (PointerButton::Primary, 0),
        (PointerButton::Middle, 1),
        (PointerButton::Secondary, 2),
        (PointerButton::Back, 3),
        (PointerButton::Forward, 4),
    ] {
        assert_eq!(actions::button_number(button).expect("numbered"), number);
    }
    let scrolled = actions::perform_actions(
        &OsEvent::Scroll {
            at: DevicePoint::new(301, 41),
            delta_x: 0,
            delta_y: 81,
        },
        2.0,
    )
    .expect("a scroll")
    .expect("a command");
    let (source, step) = single(&scrolled);
    assert_eq!(source["type"], "wheel");
    assert_eq!(
        step,
        &json!({ "type": "scroll", "x": 150, "y": 20, "deltaX": 0, "deltaY": 41, "duration": 0, "origin": "viewport" })
    );
    let typed = actions::perform_actions(&OsEvent::Text("hé".into()), 1.0)
        .expect("text")
        .expect("a command");
    assert_eq!(
        typed[0]["actions"],
        json!([
            { "type": "keyDown", "value": "h" }, { "type": "keyUp", "value": "h" },
            { "type": "keyDown", "value": "é" }, { "type": "keyUp", "value": "é" },
        ])
    );
    let key = actions::perform_actions(&OsEvent::KeyDown("Shift".into()), 1.0)
        .expect("a key")
        .expect("a command");
    assert_eq!(
        key[0]["actions"],
        json!([{ "type": "keyDown", "value": "\u{E008}" }])
    );
    assert_eq!(
        actions::perform_actions(&OsEvent::Pause(Duration::from_millis(16)), 1.0).expect("a pause"),
        None,
        "a pause is the lane's own wait"
    );
    let refused = actions::perform_actions(&OsEvent::KeyDown("CapsLock".into()), 1.0);
    assert!(
        matches!(refused, Err(Error::NotSupported { capability: Some(ref c), .. }) if c == "os_key_CapsLock"),
        "{refused:?}"
    );
    for ratio in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(
            matches!(
                actions::perform_actions(&mouse(DevicePoint::new(1, 1)), ratio),
                Err(Error::InvalidArgument { .. })
            ),
            "ratio {ratio}"
        );
    }
}

#[test]
fn a_composition_step_is_its_devtools_command() {
    let set = |text: &str, cursor: Option<u32>, caret_hidden: bool| ImeAction::SetComposition {
        text: text.into(),
        cursor,
        caret_hidden,
    };
    // A cursor counts characters; Chrome counts UTF-16 units, two for 𠮷.
    assert_eq!(
        ime::devtools_command(&set("𠮷あ", Some(1), false)).expect("a composition"),
        (
            "Input.imeSetComposition",
            json!({ "text": "𠮷あ", "selectionStart": 2, "selectionEnd": 2 })
        )
    );
    assert_eq!(
        ime::devtools_command(&set("にほん", None, false))
            .expect("a composition")
            .1["selectionStart"],
        3
    );
    assert_eq!(
        ime::devtools_command(&set("にほん", Some(1), true))
            .expect("a hidden caret")
            .1["selectionEnd"],
        3,
        "a hidden caret sits at the end"
    );
    assert!(matches!(
        ime::devtools_command(&set("に", Some(2), false)),
        Err(Error::InvalidArgument { .. })
    ));
    assert_eq!(
        ime::devtools_command(&ImeAction::Commit {
            text: "日本".into()
        })
        .expect("a commit"),
        ("Input.insertText", json!({ "text": "日本" }))
    );
    assert_eq!(
        ime::devtools_command(&ImeAction::Cancel).expect("a cancel"),
        (
            "Input.imeSetComposition",
            json!({ "text": "", "selectionStart": 0, "selectionEnd": 0 })
        )
    );
    assert!(ime::composing_after(&set("に", None, false)));
    assert!(!ime::composing_after(&set("", None, false)));
    assert!(!ime::composing_after(&ImeAction::Commit {
        text: "日".into()
    }));
    assert!(!ime::composing_after(&ImeAction::Cancel));
}

/// A PNG of `width` by `height` pixels of `color` and `depth`, filled by `pixel`.
fn png(width: u32, height: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(color);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("a header");
    writer.write_image_data(data).expect("the pixels");
    writer.finish().expect("the end");
    bytes
}

fn base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[test]
fn a_screenshot_is_decoded_to_rgba_within_its_bounds() {
    let rgba = png(2, 1, png::ColorType::Rgba, &[1, 2, 3, 4, 5, 6, 7, 8]);
    let capture = screenshot::decode(&base64(&rgba)).expect("an RGBA screenshot");
    assert_eq!((capture.width(), capture.height()), (2, 1));
    assert_eq!(capture.pixel(1, 0), Some([5, 6, 7, 8]));
    let rgb = png(1, 1, png::ColorType::Rgb, &[9, 8, 7]);
    assert_eq!(
        screenshot::decode(&base64(&rgb))
            .expect("an RGB screenshot")
            .pixel(0, 0),
        Some([9, 8, 7, 255])
    );
    let gray = png(1, 1, png::ColorType::GrayscaleAlpha, &[100, 50]);
    assert_eq!(
        screenshot::decode(&base64(&gray))
            .expect("a gray screenshot")
            .pixel(0, 0),
        Some([100, 100, 100, 50])
    );
    // A header naming more pixels than a capture takes is refused before any pixel is decoded:
    // the few bytes of image data after it could never fill it.
    let mut header = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut header, 10_000, 10_000);
        encoder.set_color(png::ColorType::Rgba);
        let mut writer = encoder.write_header().expect("a header");
        writer
            .write_chunk(png::chunk::IDAT, &[0; 8])
            .expect("a scrap of image data");
        drop(writer);
    }
    let refused = screenshot::decode(&base64(&header));
    assert!(
        matches!(&refused, Err(Error::InvalidArgument { message, .. }) if message.contains("10000 x 10000")),
        "{refused:?}"
    );
    let long = "A".repeat(screenshot::MAX_BASE64 + 4);
    assert!(
        matches!(screenshot::decode(&long), Err(Error::InvalidArgument { message, .. }) if message.contains("bytes of base64")),
        "base64 past the bound is refused before it is decoded"
    );
    assert!(matches!(
        screenshot::decode("not base64!"),
        Err(Error::InvalidArgument { .. })
    ));
    assert!(matches!(
        screenshot::decode(&base64(b"not a png")),
        Err(Error::InvalidArgument { .. })
    ));
}

// --- Through a stand-in session ---

const WINDOW: &str = "w1";

/// The agent of a page with one 800 by 600 window at scale 1, whose journal shows every kind of
/// input once it was asked for once.
fn page_agent() -> Script {
    let mut reads = 0_u64;
    Box::new(move |name, _| match name {
        "window.placement" => Reply::Result(json!({
            "window": WINDOW,
            "size": { "width": 800.0, "height": 600.0 },
            "scale": 1.0,
            "visible": true,
            "focused": true,
        })),
        "input.journal" => {
            reads += 1;
            if reads == 1 {
                return Reply::Result(json!({ "entries": [] }));
            }
            let entry = |seq: u64, device: &str, kind: &str| json!({ "seq": seq, "device": device, "kind": kind, "time_us": seq });
            Reply::Result(json!({ "entries": [
                entry(1, "mouse", "pointer_down"), entry(2, "mouse", "pointer_up"),
                entry(3, "keyboard", "key_down"), entry(4, "keyboard", "key_up"),
                entry(5, "keyboard", "composition"),
            ] }))
        }
        _ => Reply::Result(json!({ "windows": [WINDOW] })),
    })
}

const METHODS: [&str; 3] = [
    method::WindowsList::NAME,
    method::WindowPlacement::NAME,
    method::InputJournal::NAME,
];

fn attach(browser: &ScriptedBrowser) -> Driver {
    Driver::attach_page(browser.url(), browser.token(), Duration::from_secs(5))
        .expect("the driver attaches to the page")
        .with_input_policy(InputPolicy::require(InputPath::Os))
}

fn window() -> WindowId {
    WINDOW.parse().expect("a window handle")
}

fn press(at: LogicalPoint) -> ActionSequence {
    ActionSequence::new(vec![InputSource::Pointer {
        id: "scenario.mouse".into(),
        device: PointerDevice::Mouse,
        actions: vec![
            PointerAction::Move {
                to: at,
                duration_ms: 32,
                pressure: None,
            },
            PointerAction::Down {
                button: PointerButton::Primary,
                pressure: None,
            },
            PointerAction::Pause { duration_ms: 20 },
            PointerAction::Up {
                button: PointerButton::Primary,
            },
        ],
    }])
}

#[test]
fn a_page_is_reached_through_its_export_and_driven_one_step_a_move() {
    let browser = ScriptedBrowser::start(
        &METHODS,
        page_agent(),
        Box::new(|method, params| {
            // The canvas sits 10 CSS pixels down the viewport, as under a page's header.
            if method == "script.callFunction"
                && params["functionDeclaration"]
                    .as_str()
                    .is_some_and(|f| f.contains("getBoundingClientRect"))
            {
                return BrowserReply::Result(ScriptedBrowser::canvas(0.0, 10.0, 800.0, 600.0, 1.0));
            }
            BrowserReply::Played
        }),
    )
    .expect("the stand-in binds");
    let mut driver = attach(&browser);
    assert_eq!(driver.lane_name(), Some("chrome"));
    assert_eq!(driver.isolation(), Isolation::HeadlessBrowser);
    let compositions = driver.lane_capabilities().is_some_and(|c| c.compositions);
    assert!(compositions, "the Chrome lane composes");
    let outcome = driver
        .window(window())
        .perform(press(LogicalPoint::new(100.0, 50.0)))
        .expect("the press goes through the lane");
    assert_eq!(outcome.via, InputPath::Os);
    assert_eq!(outcome.effect.kind, EffectKind::Ran, "{outcome:?}");

    let log = browser.log();
    // The page's export was connected with a channel of the session's own.
    let connect = &log.of("script.callFunction")[0];
    let channel = connect
        .pointer("/arguments/1/value/channel")
        .and_then(Value::as_str)
        .expect("a channel");
    assert!(channel.starts_with("flui-driver-"), "{channel}");
    // The token went in the hello, as a line without its newline.
    let hello = &log.of("script.callFunction")[1];
    let line = hello
        .pointer("/arguments/0/value")
        .and_then(Value::as_str)
        .expect("a line");
    assert!(
        line.contains(browser.token()) && !line.ends_with('\n'),
        "{line}"
    );
    let performed: Vec<Value> = log
        .of("input.performActions")
        .into_iter()
        .map(|params| params["actions"][0]["actions"].clone())
        .collect();
    // Two steps of the 32 ms move (from where it starts, which is where it ends), the press and
    // the release, each its own command of one action of no duration.
    assert_eq!(
        performed,
        [
            json!([{ "type": "pointerMove", "x": 100, "y": 60, "duration": 0, "origin": "viewport" }]),
            json!([{ "type": "pointerMove", "x": 100, "y": 60, "duration": 0, "origin": "viewport" }]),
            json!([{ "type": "pointerDown", "button": 0 }]),
            json!([{ "type": "pointerUp", "button": 0 }]),
        ]
    );
    assert_eq!(
        log.of("input.releaseActions").len(),
        1,
        "the browser's input state is released at the sequence's end whatever the lane recorded"
    );
}

#[test]
fn a_composition_goes_through_the_devtools_input_method() {
    let browser = ScriptedBrowser::start(
        &METHODS,
        page_agent(),
        Box::new(|_, _| BrowserReply::Played),
    )
    .expect("the stand-in binds");
    let mut driver = attach(&browser);
    let composition = ActionSequence::new(vec![InputSource::Ime {
        actions: vec![
            ImeAction::SetComposition {
                text: "にほん".into(),
                cursor: None,
                caret_hidden: false,
            },
            ImeAction::Commit {
                text: "日本".into(),
            },
        ],
    }]);
    let outcome = driver
        .window(window())
        .perform(composition)
        .expect("the composition goes out");
    assert_eq!(outcome.via, InputPath::Os);
    // The journal shows a composition, so the input ran; the run announced the keyboard.
    assert_eq!(outcome.effect.kind, EffectKind::Ran, "{outcome:?}");
    let announced = driver.take_announcements();
    assert!(
        announced.iter().any(|line| line.contains("keyboard")),
        "{announced:?}"
    );
    let log = browser.log();
    assert_eq!(
        log.of("goog:cdp.getSession").len(),
        1,
        "one DevTools session for the tab"
    );
    let sent: Vec<(Value, Value)> = log
        .of("goog:cdp.sendCommand")
        .into_iter()
        .map(|params| (params["method"].clone(), params["params"].clone()))
        .collect();
    assert_eq!(
        sent,
        [
            (
                json!("Input.imeSetComposition"),
                json!({ "text": "にほん", "selectionStart": 3, "selectionEnd": 3 })
            ),
            (json!("Input.insertText"), json!({ "text": "日本" })),
        ]
    );
    assert!(
        log.of("goog:cdp.sendCommand")
            .iter()
            .all(|params| params["session"] == "devtools-1")
    );
}

fn held_keys() -> ActionSequence {
    ActionSequence::new(vec![InputSource::Keys {
        id: "keys".into(),
        actions: vec![KeyAction::Down {
            key: "Control".into(),
        }],
    }])
}

#[test]
fn held_input_is_released_and_a_refused_release_names_what_stays_held() {
    let browser = ScriptedBrowser::start(
        &METHODS,
        page_agent(),
        Box::new(|_, _| BrowserReply::Played),
    )
    .expect("the stand-in binds");
    let mut driver = attach(&browser);
    driver
        .window(window())
        .perform(held_keys())
        .expect("the key goes down");
    assert_eq!(
        browser.log().of("input.releaseActions").len(),
        1,
        "released at the sequence's end"
    );

    let refusing = ScriptedBrowser::start(
        &METHODS,
        page_agent(),
        Box::new(|method, _| match method {
            "input.releaseActions" => BrowserReply::Error {
                error: "unknown error".into(),
                message: "the tab is busy".into(),
            },
            _ => BrowserReply::Played,
        }),
    )
    .expect("the stand-in binds");
    let mut driver = attach(&refusing);
    let refused = driver.window(window()).perform(held_keys());
    assert!(
        matches!(&refused, Err(Error::ReleaseFailed { lane, held, message, .. }) if lane == "chrome" && held == &["Control".to_owned()] && message.contains("the tab is busy")),
        "{refused:?}"
    );
}

/// A composition left open is cancelled by the release, and a cancel the browser refuses names it
/// as held beside whatever else stays down, the keys still let go of.
#[test]
fn an_open_composition_is_cancelled_by_the_release() {
    let open = || {
        ActionSequence::new(vec![InputSource::Ime {
            actions: vec![ImeAction::SetComposition {
                text: "に".into(),
                cursor: None,
                caret_hidden: false,
            }],
        }])
    };
    let browser = ScriptedBrowser::start(
        &METHODS,
        page_agent(),
        Box::new(|_, _| BrowserReply::Played),
    )
    .expect("the stand-in binds");
    let mut driver = attach(&browser);
    driver
        .window(window())
        .perform(open())
        .expect("the composition opens and is cancelled");
    let sent: Vec<Value> = browser
        .log()
        .of("goog:cdp.sendCommand")
        .into_iter()
        .map(|params| params["params"].clone())
        .collect();
    assert_eq!(
        sent,
        [
            json!({ "text": "に", "selectionStart": 1, "selectionEnd": 1 }),
            json!({ "text": "", "selectionStart": 0, "selectionEnd": 0 }),
        ]
    );

    let refusing = ScriptedBrowser::start(
        &METHODS,
        page_agent(),
        Box::new(|method, params| match method {
            "goog:cdp.sendCommand" if params["params"]["text"] == "" => BrowserReply::Error {
                error: "unknown error".into(),
                message: "no composition to cancel".into(),
            },
            _ => BrowserReply::Played,
        }),
    )
    .expect("the stand-in binds");
    let mut driver = attach(&refusing);
    let refused = driver.window(window()).perform(open());
    assert!(
        matches!(&refused, Err(Error::ReleaseFailed { held, .. }) if held == &["an input-method composition".to_owned()]),
        "{refused:?}"
    );
}

/// A press the browser never answers may have gone down: it is named as possibly held, and the
/// release still runs.
#[test]
fn a_press_the_browser_does_not_answer_is_released_as_possibly_held() {
    let browser = ScriptedBrowser::start(
        &METHODS,
        page_agent(),
        Box::new(|method, params| match method {
            "input.performActions"
                if params["actions"][0]["actions"][0]["type"] == "pointerDown" =>
            {
                BrowserReply::Silent
            }
            "input.releaseActions" => BrowserReply::Error {
                error: "unknown error".into(),
                message: "refused".into(),
            },
            _ => BrowserReply::Played,
        }),
    )
    .expect("the stand-in binds");
    let mut driver = attach(&browser);
    let refused = driver
        .window(window())
        .perform(press(LogicalPoint::new(10.0, 10.0)));
    match &refused {
        Err(Error::ReleaseFailed { held, message, .. }) => {
            assert_eq!(held, &["possibly scenario.mouse primary".to_owned()]);
            assert!(
                message.contains("did not answer"),
                "the send's failure is kept: {message}"
            );
        }
        other => panic!("the unanswered press is possibly held: {other:?}"),
    }
    assert_eq!(browser.log().of("input.releaseActions").len(), 1);
    assert!(
        browser
            .log()
            .of("input.performActions")
            .iter()
            .all(|params| params["actions"][0]["actions"][0]["type"] != "pointerUp"),
        "nothing after the unanswered press was sent"
    );
}

#[test]
fn an_answer_is_matched_by_its_id_and_a_message_past_the_bound_ends_the_session() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let browser = ScriptedBrowser::start(
        &METHODS,
        page_agent(),
        Box::new(move |method, _| {
            if method != "script.callFunction" {
                return BrowserReply::Played;
            }
            // The connect and the hello, then a stale answer before the windows' request, then a
            // message past the bound.
            match counted.fetch_add(1, Ordering::SeqCst) {
                2 => BrowserReply::StaleFirst,
                3 => BrowserReply::Oversized,
                _ => BrowserReply::Played,
            }
        }),
    )
    .expect("the stand-in binds");
    let mut driver = attach(&browser);
    assert_eq!(
        driver.windows().expect("the windows, past a stale answer"),
        [window()]
    );
    let started = Instant::now();
    let gone = driver.windows();
    assert!(
        matches!(&gone, Err(Error::Gone { .. })),
        "a message past the bound breaks the session: {gone:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(9),
        "it fails as soon as the frame is read"
    );
    assert!(
        matches!(driver.windows(), Err(Error::Gone { .. })),
        "and every call after"
    );
}

#[test]
fn a_page_that_sends_more_lines_than_the_driver_reads_breaks_its_connection() {
    let flood: Vec<Value> = (0..=PAGE_ROOM)
        .map(|frame| json!({ "event": "frame", "params": { "window": WINDOW, "frame": frame, "presented": true } }))
        .collect();
    let mut first = true;
    let browser = ScriptedBrowser::start(
        &METHODS,
        Box::new(move |_, _| {
            if std::mem::take(&mut first) {
                return Reply::ResultThenLines {
                    result: json!({ "windows": [WINDOW] }),
                    lines: flood.clone(),
                    then: flui_driver::testing::AfterLines::Serve,
                };
            }
            Reply::Result(json!({ "windows": [WINDOW] }))
        }),
        Box::new(|_, _| BrowserReply::Played),
    )
    .expect("the stand-in binds");
    let mut driver = attach(&browser);
    driver.windows().expect("the answer came before the flood");
    // Give the session's thread time to hand the flood over.
    std::thread::sleep(Duration::from_millis(500));
    let broken = driver.windows();
    assert!(
        matches!(&broken, Err(Error::Gone { message, .. }) if message.contains(&format!("more than {PAGE_ROOM} lines"))),
        "{broken:?}"
    );
}

#[test]
fn the_app_closing_its_connection_ends_the_conversation() {
    let browser = ScriptedBrowser::start(
        &METHODS,
        Box::new(|_, _| Reply::ResultThenClose(json!({ "windows": [WINDOW] }))),
        Box::new(|_, _| BrowserReply::Played),
    )
    .expect("the stand-in binds");
    let mut driver = attach(&browser);
    driver.windows().expect("the answer before the close");
    let gone = driver.windows();
    assert!(
        matches!(&gone, Err(Error::Gone { message, .. }) if message.contains("closed the connection")),
        "{gone:?}"
    );
}

#[test]
fn a_wrong_token_is_refused_at_the_hello() {
    let browser = ScriptedBrowser::start(
        &METHODS,
        page_agent(),
        Box::new(|_, _| BrowserReply::Played),
    )
    .expect("the stand-in binds");
    let refused = Driver::attach_page(browser.url(), "not the token", Duration::from_secs(5));
    assert!(
        matches!(refused, Err(Error::Protocol { .. })),
        "{refused:?}"
    );
    let off_loopback =
        Driver::attach_page("ws://192.0.2.1:9222/session/x", "t", Duration::from_secs(1));
    assert!(
        matches!(off_loopback, Err(Error::InvalidArgument { .. })),
        "{off_loopback:?}"
    );
}

#[test]
fn the_canvas_is_captured_clipped_to_itself() {
    let pixels = base64(&png(2, 2, png::ColorType::Rgba, &[255; 16]));
    let browser = ScriptedBrowser::start(
        &METHODS,
        page_agent(),
        Box::new(move |method, params| match method {
            "browsingContext.captureScreenshot" => BrowserReply::Result(json!({ "data": pixels })),
            "script.callFunction"
                if params["functionDeclaration"]
                    .as_str()
                    .is_some_and(|f| f.contains("getBoundingClientRect")) =>
            {
                BrowserReply::Result(ScriptedBrowser::canvas(0.0, 24.0, 800.0, 600.0, 1.0))
            }
            _ => BrowserReply::Played,
        }),
    )
    .expect("the stand-in binds");
    let mut driver = attach(&browser);
    let capture = driver
        .window(window())
        .capture(CaptureSource::Os)
        .expect("the canvas's capture");
    let capture = capture.image();
    assert_eq!((capture.width(), capture.height()), (2, 2));
    assert_eq!(capture.pixel(1, 1), Some([255; 4]));
    let shot = &browser.log().of("browsingContext.captureScreenshot")[0];
    assert_eq!(
        shot["clip"],
        json!({ "type": "box", "x": 0.0, "y": 24.0, "width": 800.0, "height": 600.0 })
    );
    assert_eq!(shot["origin"], "viewport");
}

#[test]
fn a_canvas_of_another_size_or_ratio_is_refused_before_any_input() {
    for canvas in [
        ScriptedBrowser::canvas(0.0, 0.0, 640.0, 480.0, 1.0),
        ScriptedBrowser::canvas(0.0, 0.0, 800.0, 600.0, 2.0),
        json!({ "type": "success", "realm": "realm-1", "result": { "type": "null" } }),
    ] {
        let browser = ScriptedBrowser::start(
            &METHODS,
            page_agent(),
            Box::new(move |method, params| {
                if method == "script.callFunction"
                    && params["functionDeclaration"]
                        .as_str()
                        .is_some_and(|f| f.contains("getBoundingClientRect"))
                {
                    return BrowserReply::Result(canvas.clone());
                }
                BrowserReply::Played
            }),
        )
        .expect("the stand-in binds");
        let mut driver = attach(&browser);
        let refused = driver
            .window(window())
            .perform(press(LogicalPoint::new(10.0, 10.0)));
        assert!(
            matches!(
                refused,
                Err(Error::InvalidArgument { .. } | Error::NotSupported { .. })
            ),
            "{refused:?}"
        );
        assert!(
            browser.log().of("input.performActions").is_empty(),
            "nothing was sent"
        );
    }
}

/// A canvas laid out in fractions of a pixel, or at a ratio the OS rounded, is still the window.
#[test]
fn a_canvas_within_a_pixel_and_a_rounded_ratio_is_the_window() {
    let browser = ScriptedBrowser::start(
        &METHODS,
        page_agent(),
        Box::new(|method, params| {
            if method == "script.callFunction"
                && params["functionDeclaration"]
                    .as_str()
                    .is_some_and(|f| f.contains("getBoundingClientRect"))
            {
                return BrowserReply::Result(ScriptedBrowser::canvas(
                    0.0, 0.0, 800.75, 599.25, 1.0005,
                ));
            }
            BrowserReply::Played
        }),
    )
    .expect("the stand-in binds");
    let mut driver = attach(&browser);
    driver
        .window(window())
        .perform(press(LogicalPoint::new(10.0, 10.0)))
        .expect("the canvas is the window");
    assert_eq!(browser.log().of("input.performActions").len(), 4);
}

/// What the lane holds follows what it pressed and released, button by button and key by key:
/// a refused release names exactly what is still down.
#[test]
fn a_refused_release_names_the_buttons_and_keys_still_down() {
    let browser = ScriptedBrowser::start(
        &METHODS,
        page_agent(),
        Box::new(|method, _| match method {
            "input.releaseActions" => BrowserReply::Error {
                error: "unknown error".into(),
                message: "refused".into(),
            },
            _ => BrowserReply::Played,
        }),
    )
    .expect("the stand-in binds");
    let mut driver = attach(&browser);
    let sequence = ActionSequence::new(vec![
        InputSource::Pointer {
            id: "scenario.mouse".into(),
            device: PointerDevice::Mouse,
            actions: vec![
                PointerAction::Down {
                    button: PointerButton::Secondary,
                    pressure: None,
                },
                PointerAction::Down {
                    button: PointerButton::Primary,
                    pressure: None,
                },
                PointerAction::Up {
                    button: PointerButton::Primary,
                },
            ],
        },
        InputSource::Keys {
            id: "keys".into(),
            actions: vec![
                KeyAction::Down { key: "a".into() },
                KeyAction::Up { key: "a".into() },
                KeyAction::Down {
                    key: "Shift".into(),
                },
            ],
        },
    ]);
    let refused = driver.window(window()).perform(sequence);
    match &refused {
        Err(Error::ReleaseFailed { held, .. }) => assert_eq!(
            held,
            &["Shift".to_owned(), "scenario.mouse secondary".to_owned()]
        ),
        other => panic!("the release names what is still down: {other:?}"),
    }
}

/// The driver leaving closes the page's connection and lets go of its handle.
#[test]
fn a_driver_that_leaves_closes_the_pages_connection() {
    let browser = ScriptedBrowser::start(
        &METHODS,
        page_agent(),
        Box::new(|_, _| BrowserReply::Played),
    )
    .expect("the stand-in binds");
    let driver = attach(&browser);
    drop(driver);
    let log = browser.log();
    assert!(
        log.of("script.callFunction").iter().any(|params| {
            params["functionDeclaration"]
                .as_str()
                .is_some_and(|f| f.contains("this.close()"))
                && params["this"]["handle"] == "page-connection"
        }),
        "the page's connection was closed"
    );
    assert_eq!(
        log.of("script.disown")
            .first()
            .map(|params| params["handles"].clone()),
        Some(json!(["page-connection"]))
    );
}

/// Another tab closing leaves the conversation alone; the page's own tab closing ends it.
#[test]
fn only_the_pages_own_tab_closing_ends_the_conversation() {
    let destroyed = |context: &str| {
        json!({
            "type": "event",
            "method": "browsingContext.contextDestroyed",
            "params": { "context": context, "url": "about:blank", "children": [] },
        })
    };
    let sends = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&sends);
    let browser = ScriptedBrowser::start(
        &METHODS,
        page_agent(),
        Box::new(move |method, params| {
            let send = method == "script.callFunction"
                && params["functionDeclaration"]
                    .as_str()
                    .is_some_and(|f| f.contains("this.send(line)"));
            if !send {
                return BrowserReply::Played;
            }
            // The hello, then the first request with another tab gone, then the page's own.
            match counted.fetch_add(1, Ordering::SeqCst) {
                1 => BrowserReply::EventFirst(destroyed("tab-2")),
                2 => BrowserReply::EventFirst(destroyed("tab-1")),
                _ => BrowserReply::Played,
            }
        }),
    )
    .expect("the stand-in binds");
    let mut driver = attach(&browser);
    assert_eq!(
        driver
            .windows()
            .expect("another tab closing changes nothing"),
        [window()]
    );
    let gone = driver.windows();
    assert!(
        matches!(&gone, Err(Error::Gone { message, .. }) if message.contains("tab closed")),
        "{gone:?}"
    );
}

/// A text whose command the browser never answers may have left a key down: the release is sent
/// all the same, names it as possibly held when the browser refuses it, and the call still says it
/// may have run.
#[test]
fn an_unanswered_text_is_released_and_named_as_possibly_held() {
    let browser = ScriptedBrowser::start(
        &METHODS,
        page_agent(),
        Box::new(|method, params| match method {
            "input.performActions" if params["actions"][0]["type"] == "key" => BrowserReply::Silent,
            "input.releaseActions" => BrowserReply::Error {
                error: "unknown error".into(),
                message: "refused".into(),
            },
            _ => BrowserReply::Played,
        }),
    )
    .expect("the stand-in binds");
    let mut driver = attach(&browser);
    let refused = driver
        .window(window())
        .perform(ActionSequence::new(vec![InputSource::Text {
            text: "hi".into(),
        }]));
    match &refused {
        Err(error @ Error::ReleaseFailed { held, .. }) => {
            assert_eq!(held, &["possibly a key typing \"hi\"".to_owned()]);
            assert!(
                error.may_have_run(),
                "the unanswered text may have run: {error:?}"
            );
        }
        other => panic!("the text's keys are possibly held: {other:?}"),
    }
    assert_eq!(browser.log().of("input.releaseActions").len(), 1);
}

/// A release that fails keeps what the failure before it said: the input the user interrupted is
/// still cannot-verify (the scripted lane, guarded), and input the browser did not answer still
/// may have run (the Chrome lane).
#[test]
fn a_failed_release_keeps_the_verdict_and_effect_of_what_stopped_the_input() {
    use flui_driver::coordinates::DevicePoint;
    use flui_driver::lane::send_guarded;
    use flui_driver::protocol::report::Check;
    use flui_driver::protocol::{report::Outcome, verdict::Verdict};
    use flui_driver::testing::ScriptedLane;

    let (lane, _log) = ScriptedLane::new(Isolation::Xvfb);
    let mut lane = lane
        .foreign_input_after(1, "a mouse move")
        .failing_releases(1);
    let events = [
        OsEvent::KeyDown("Control".into()),
        OsEvent::PointerMove {
            pointer: "m".into(),
            device: PointerDevice::Mouse,
            to: DevicePoint::new(1, 1),
        },
    ];
    let interrupted = send_guarded(&mut lane, &events);
    assert!(
        matches!(&interrupted, Err(Error::ReleaseFailed { held, .. }) if held == &["Control".to_owned()]),
        "{interrupted:?}"
    );
    assert!(
        interrupted.as_ref().is_err_and(Error::is_cannot_verify),
        "the interruption stays cannot-verify: {interrupted:?}"
    );
    let check = Check::from_result("interrupted", &interrupted);
    assert!(
        matches!(check.outcome, Outcome::CannotVerify { .. }),
        "{check:?}"
    );
    assert_eq!(check.outcome.verdict(), Verdict::CannotVerify);

    let browser = ScriptedBrowser::start(
        &METHODS,
        page_agent(),
        Box::new(|method, params| match method {
            "input.performActions"
                if params["actions"][0]["actions"][0]["type"] == "pointerDown" =>
            {
                BrowserReply::Silent
            }
            "input.releaseActions" => BrowserReply::Error {
                error: "unknown error".into(),
                message: "refused".into(),
            },
            _ => BrowserReply::Played,
        }),
    )
    .expect("the stand-in binds");
    let mut driver = attach(&browser);
    let refused = driver
        .window(window())
        .perform(press(LogicalPoint::new(10.0, 10.0)));
    let error = refused.expect_err("the release was refused");
    assert!(matches!(error, Error::ReleaseFailed { .. }), "{error:?}");
    assert!(
        error.may_have_run(),
        "the unanswered press may have run: {error:?}"
    );
    assert_eq!(
        error.effect().map(|effect| effect.kind),
        Some(EffectKind::MayHaveRun)
    );
    assert!(
        !error.is_cannot_verify(),
        "a timeout is a failure: {error:?}"
    );
}

/// A composition for a lane that does not compose is refused before the desktop is locked or the
/// run announced.
#[test]
fn a_composition_for_a_lane_that_does_not_compose_is_refused_before_the_lock() {
    use flui_driver::testing::ScriptedLane;

    let browser = ScriptedBrowser::start(
        &METHODS,
        page_agent(),
        Box::new(|_, _| BrowserReply::Played),
    )
    .expect("the stand-in binds");
    let dir = tempfile::tempdir().expect("a temporary directory");
    let lock = dir.path().join("os-input.lock");
    let (lane, log) = ScriptedLane::new(Isolation::Xvfb);
    let mut driver = attach(&browser)
        .with_os_input_lock(&lock)
        .with_lane(lane)
        .expect("the scripted lane takes over");
    let refused = driver
        .window(window())
        .perform(ActionSequence::new(vec![InputSource::Ime {
            actions: vec![ImeAction::Commit {
                text: "日本".into(),
            }],
        }]));
    assert!(
        matches!(&refused, Err(Error::NotSupported { capability: Some(capability), .. }) if capability == "os_ime_composition"),
        "{refused:?}"
    );
    assert!(!lock.exists(), "the desktop was not locked");
    assert!(
        driver.take_announcements().is_empty(),
        "nothing was announced"
    );
    assert!(log.sent().is_empty(), "nothing was sent");
}

/// A browser that stops answering costs the driver's leaving one bounded wait, not a command
/// timeout for each of its two commands.
#[test]
fn closing_the_pages_connection_is_bounded_when_the_browser_stops_answering() {
    let browser = ScriptedBrowser::start(
        &METHODS,
        page_agent(),
        Box::new(|method, params| {
            let closing = method == "script.callFunction"
                && params["functionDeclaration"]
                    .as_str()
                    .is_some_and(|f| f.contains("this.close()"));
            if closing || method == "script.disown" {
                return BrowserReply::Silent;
            }
            BrowserReply::Played
        }),
    )
    .expect("the stand-in binds");
    let driver = attach(&browser);
    let started = Instant::now();
    drop(driver);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the driver left in {:?}",
        started.elapsed()
    );
}

/// A trace of a page session names the Chrome lane, the headless browser its input lands in and
/// the exact positions the lane reproduces, and holds the OS input as one of its commands, as a
/// trace through any other lane does.
#[test]
fn a_trace_of_a_page_session_names_the_chrome_lane_and_holds_its_input() {
    use flui_driver::protocol::report::Report;
    use flui_driver::trace::{Positions, Trace, TraceOptions};

    let browser = ScriptedBrowser::start(
        &METHODS,
        page_agent(),
        Box::new(|_, _| BrowserReply::Played),
    )
    .expect("the stand-in binds");
    let mut driver = attach(&browser);
    let root = tempfile::tempdir().expect("a temporary directory");
    let dir = root.path().join("page.flui-trace");
    driver
        .start_trace(TraceOptions::new(&dir))
        .expect("the trace starts");
    let mut report = Report::new();
    report
        .check("a press", || {
            driver
                .window(window())
                .perform(press(LogicalPoint::new(100.0, 50.0)))
        })
        .expect("the press goes through the lane");
    driver
        .finish_trace(&mut report)
        .expect("the trace finishes");
    let trace = Trace::open(&dir).expect("the trace reads back");
    assert!(trace.is_complete());
    let header = trace.header();
    assert_eq!(header.target.lane.as_deref(), Some("chrome"));
    assert_eq!(header.target.kind, "attached");
    assert_eq!(header.isolation, Isolation::HeadlessBrowser);
    assert_eq!(header.fidelity.positions, Positions::Exact);
    let rows = std::fs::read_to_string(dir.join("trace.jsonl")).expect("the trace's rows");
    assert!(
        rows.lines().any(|row| row.contains("input.perform")),
        "the OS input is a command of the trace: {rows}"
    );
}

/// A page journals its backend as a native host does (ADR-0390): its agent lists the `native`
/// layer, so a trace of its session asks for both layers and writes each native stream the page
/// sends beside the input, read back through the protocol's schema: the application's notices,
/// the window's input method events and snapshots, and the reads of the preferences and the
/// displays, each stream's header naming the `web` backend.
#[test]
fn a_trace_of_a_page_session_holds_the_pages_native_layer() {
    use flui_driver::protocol::native_recording::{Backend, Row as NativeRow, Stream};
    use flui_driver::protocol::record::RecordLayer;
    use flui_driver::protocol::report::Report;
    use flui_driver::protocol::session::Capabilities;
    use flui_driver::testing::AfterLines;
    use flui_driver::trace::{Trace, TraceOptions};

    let parsed = |text: &str| serde_json::from_str::<Value>(text).expect("a JSON line");
    let native = |stream: u64, line: &str| {
        json!({
            "event": "record_native",
            "params": { "recording": 1, "window": WINDOW, "stream": stream, "line": parsed(line) },
        })
    };
    let header = |stream: &str| {
        format!(r#"{{"header":{{"flui_native_recording":4,"backend":"web","stream":"{stream}"}}}}"#)
    };
    let lines = vec![
        native(0, &header("app")),
        native(0, r#"{"row":{"row":"notice","notice":"sleep"}}"#),
        native(0, r#"{"row":{"row":"reopen"}}"#),
        native(1, &header("window")),
        native(1, r#"{"row":{"row":"ime","ime":{"event":"enabled"}}}"#),
        native(
            1,
            r#"{"row":{"row":"ime","ime":{"event":{"commit":"日本"}}}}"#,
        ),
        native(2, &header("platform")),
        native(
            2,
            r#"{"row":{"row":"preferences","read":{"observed":{"motion":"reduce","text_weight":"bold","gestures":{},"wheel_vertical":"page"}}}}"#,
        ),
        native(
            2,
            r#"{"row":{"row":"displays","read":{"observed":{"revision":1,"displays":[{"label":1,"name":"Screen","origin":[0,0],"extent":[1280.0,800.0],"usable":[0.0,0.0,1280.0,800.0],"scale_factor":2.0,"primary":true,"color":{"known":{"gamut":"srgb","dynamic_range":"standard","headroom":{"multiple":1.0}}}}]}}}}"#,
        ),
        native(3, &header("window_state")),
        native(
            3,
            r#"{"row":{"row":"state","state":{"revision":0,"outer":{"size":[800.0,600.0]},"content_size":[800.0,600.0],"mode":"normal","visible":true,"occluded":false,"focused":true,"constraints":{},"resizable":true,"frame":"system","metrics":{"physical":[1600,1200],"scale":2.0},"appearance":"light","safe_area":[0.0,0.0,0.0,0.0],"execution":"running","hovered":false,"active":true,"capabilities":{"sets_title":"native","sets_position":"native","sets_size":"native","sets_constraints":"native","sets_resizable":"native","sets_frame":"native","app_title_bar":"native","minimizes":"native","maximizes":"native","enters_fullscreen":"native","hides":"native","shows":"native","activates":"native","closes":"native","material":"native","reports_presentation":"native","observes_position":"native","observes_minimized":"native","places_at_open":"native","frame_rate_hint":"native"}}}}"#,
        ),
    ];
    let mut capabilities = Capabilities::default();
    capabilities.methods = METHODS
        .iter()
        .copied()
        .chain([method::RecordStart::NAME, method::RecordStop::NAME])
        .map(str::to_owned)
        .collect();
    capabilities.record = vec![RecordLayer::Input, RecordLayer::Native];
    let mut page = page_agent();
    let mut asked_native = false;
    let browser = ScriptedBrowser::start_with(
        capabilities,
        Box::new(move |name, params| match name {
            "record.start" => {
                asked_native = params["layers"] == json!(["input", "native"]);
                Reply::ResultThenLines {
                    result: json!({
                        "recording": 1,
                        "header": { "flui_recording": 6, "clock": "monotonic", "windows": [{ "window": 1, "scale_factor": 1.0 }] },
                    }),
                    lines: lines.clone(),
                    then: AfterLines::Serve,
                }
            }
            // The page's agent sends the recording's `end` row before it answers the stop, as
            // every agent does (ADR-0288 §11).
            "record.stop" => Reply::LinesThenResult {
                lines: vec![json!({
                    "event": "record",
                    "params": { "recording": 1, "window": WINDOW, "row": { "end": { "lost_rows": 0 } } },
                })],
                result: json!({ "rows": 1, "lost_rows": 0, "native_lost_rows": 0, "complete": asked_native }),
            },
            _ => page(name, params),
        }),
        Box::new(|_, _| BrowserReply::Played),
    )
    .expect("the stand-in binds");
    let mut driver = attach(&browser);
    let root = tempfile::tempdir().expect("a temporary directory");
    let dir = root.path().join("page-native.flui-trace");
    driver
        .start_trace(TraceOptions::new(&dir))
        .expect("the trace starts");
    let mut report = Report::new();
    report
        .check("a press", || {
            driver
                .window(window())
                .perform(press(LogicalPoint::new(100.0, 50.0)))
        })
        .expect("the press goes through the lane");
    let summary = driver
        .finish_trace(&mut report)
        .expect("the trace finishes");
    assert!(
        summary.recording.is_some_and(|stopped| stopped.complete),
        "record.start asked the page for the input and native layers"
    );
    assert_eq!(summary.native.len(), 4, "{:?}", summary.native);

    let trace = Trace::open(&dir).expect("the trace reads back");
    assert_eq!(
        trace.header().layers,
        [RecordLayer::Input, RecordLayer::Native]
    );
    let native = trace
        .native_recordings()
        .expect("the native layer reads through the protocol");
    let streams: Vec<(Backend, Stream, Vec<&NativeRow>)> = native
        .iter()
        .map(|recording| {
            (
                recording.header.backend,
                recording.header.stream,
                recording.rows().collect(),
            )
        })
        .collect();
    assert!(
        streams
            .iter()
            .all(|(backend, _, _)| *backend == Backend::Web),
        "{streams:?}"
    );
    let rows_of = |stream: Stream| -> Vec<&NativeRow> {
        let Some((_, _, rows)) = streams.iter().find(|(_, found, _)| *found == stream) else {
            panic!("the trace holds the page's {stream:?} stream: {streams:?}");
        };
        rows.clone()
    };
    assert!(
        matches!(
            rows_of(Stream::App).as_slice(),
            [NativeRow::Notice { .. }, NativeRow::Reopen]
        ),
        "the lifecycle the page reported: {streams:?}"
    );
    assert!(
        matches!(
            rows_of(Stream::Window).as_slice(),
            [NativeRow::Ime { .. }, NativeRow::Ime { .. }]
        ),
        "the input method's composition: {streams:?}"
    );
    assert!(
        matches!(
            rows_of(Stream::Platform).as_slice(),
            [NativeRow::Preferences { .. }, NativeRow::Displays { .. }]
        ),
        "the reads of the preferences and the displays: {streams:?}"
    );
    assert!(
        matches!(
            rows_of(Stream::WindowState).as_slice(),
            [NativeRow::State { .. }]
        ),
        "the window's snapshot: {streams:?}"
    );
}
