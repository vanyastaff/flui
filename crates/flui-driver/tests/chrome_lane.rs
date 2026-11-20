//! The Chrome lane on every host: its tables (keys, actions, input-method steps, screenshots, the
//! page's server, ChromeDriver's versions and archives) and, against a stand-in BiDi session
//! (`ScriptedBrowser`), the session, the lane and the page's connection: one step a move, held
//! input released or named, answers matched by id, untrusted sizes refused, the canvas located
//! and captured.

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use base64::Engine as _;
use flui_driver::coordinates::DevicePoint;
use flui_driver::lane::chrome::chromedriver::{
    self, ChromeForTesting, archive_url, chrome_args, is_version, newest_version, reported_port,
};
use flui_driver::lane::chrome::page::{MAX_HEAD, PageServer, resolve};
use flui_driver::lane::chrome::{PAGE_ROOM, actions, ime, keys, screenshot};
use flui_driver::protocol::act::EffectKind;
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
    assert_eq!((capture.width, capture.height), (2, 1));
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
    // the image data after it is never there.
    let mut header = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut header, 10_000, 10_000);
        encoder.set_color(png::ColorType::Rgba);
        drop(encoder.write_header().expect("a header"));
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

/// The status line and body of `request` sent to `server`.
fn http(server: &PageServer, request: &[u8]) -> (String, Vec<u8>) {
    let url = server.url();
    let authority = url
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_owned();
    let mut stream = TcpStream::connect(authority).expect("the server accepts");
    stream.write_all(request).expect("the request goes out");
    let mut answer = Vec::new();
    stream
        .read_to_end(&mut answer)
        .expect("the answer comes back");
    let text = String::from_utf8_lossy(&answer).into_owned();
    let status = text.lines().next().unwrap_or("").to_owned();
    let body = text
        .split_once("\r\n\r\n")
        .map_or_else(Vec::new, |(_, body)| body.as_bytes().to_vec());
    (status, body)
}

#[test]
fn the_page_server_serves_its_root_and_nothing_outside_it() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir.path().join("site");
    std::fs::create_dir_all(root.join("pkg")).expect("the build's directories");
    std::fs::write(root.join("index.html"), "<p>app</p>").expect("the page");
    std::fs::write(root.join("pkg").join("app_bg.wasm"), b"\0asm").expect("the module");
    std::fs::write(dir.path().join("secret.txt"), "outside").expect("a file outside");
    let server = PageServer::serve(&root).expect("the build is served");

    let (status, body) = http(&server, b"GET / HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!(
        (status.as_str(), body.as_slice()),
        ("HTTP/1.1 200 OK", b"<p>app</p>".as_slice())
    );
    let (status, body) = http(&server, b"GET /pkg/app_bg.wasm?v=1 HTTP/1.1\r\n\r\n");
    assert_eq!(
        (status.as_str(), body.as_slice()),
        ("HTTP/1.1 200 OK", b"\0asm".as_slice())
    );
    let (status, body) = http(&server, b"HEAD /index.html HTTP/1.1\r\n\r\n");
    assert_eq!((status.as_str(), body.len()), ("HTTP/1.1 200 OK", 0));
    for target in [
        "/../secret.txt",
        "/pkg/../../secret.txt",
        "/%2e%2e/secret.txt",
        "/pkg\\..\\..\\secret.txt",
        "/missing.js",
        "//etc/passwd",
    ] {
        let (status, _) = http(&server, format!("GET {target} HTTP/1.1\r\n\r\n").as_bytes());
        assert_eq!(status, "HTTP/1.1 404 Not Found", "{target}");
    }
    let (status, _) = http(&server, b"POST / HTTP/1.1\r\n\r\n");
    assert_eq!(status, "HTTP/1.1 405 Method Not Allowed");
    let mut long = b"GET / HTTP/1.1\r\nX: ".to_vec();
    long.extend(std::iter::repeat_n(b'a', MAX_HEAD));
    long.extend(b"\r\n\r\n");
    let (status, _) = http(&server, &long);
    assert_eq!(status, "HTTP/1.1 431 Request Header Fields Too Large");

    let canonical = root.canonicalize().expect("the root");
    assert_eq!(resolve(&canonical, "/"), Some(canonical.join("index.html")));
    assert_eq!(
        resolve(&canonical, "index.html"),
        None,
        "a target starts at the root"
    );
    assert_eq!(resolve(&canonical, "/pkg/./app_bg.wasm"), None);
    assert_eq!(resolve(&canonical, "/pkg"), None, "a directory is no file");
}

#[test]
fn chromedriver_versions_ports_archives_and_arguments() {
    assert!(is_version("157.0.8103.79"));
    for not in [
        "157.0.8103",
        "Google",
        "157.0..79",
        "157.0.8103.79.1",
        "1.2.3.x",
    ] {
        assert!(!is_version(not), "{not}");
    }
    let names = [
        "99.0.1.2",
        "157.0.8103.79",
        "SetupMetrics",
        "157.0.8103.110",
    ]
    .map(str::to_owned);
    assert_eq!(newest_version(&names).as_deref(), Some("157.0.8103.110"));
    assert_eq!(
        reported_port("ChromeDriver was started successfully on port 51234.\n"),
        Some(51234)
    );
    assert_eq!(reported_port("Starting ChromeDriver 157.0.8103.79"), None);
    assert_eq!(
        archive_url(
            &ChromeForTesting::default().archives,
            "157.0.8103.79",
            "win64"
        ),
        "https://storage.googleapis.com/chrome-for-testing-public/157.0.8103.79/win64/chromedriver-win64.zip"
    );
    let args = chrome_args(800, 600);
    assert_eq!(
        args.first().map(String::as_str),
        Some("--headless=new"),
        "always headless"
    );
    assert!(args.contains(&"--window-size=800,600".to_owned()));
    assert!(args.contains(&"--force-device-scale-factor=1".to_owned()));
}

/// A ChromeDriver archive holding `name` with `contents`.
fn archive(name: &str, contents: &[u8]) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut bytes);
        zip.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .expect("an entry");
        zip.write_all(contents).expect("its contents");
        zip.finish().expect("the archive");
    }
    bytes.into_inner()
}

/// A stand-in for the Chrome for Testing hosts on a loopback port, answering each path from
/// `files` and 404 otherwise; the count of requests served.
fn archives(files: Vec<(String, Vec<u8>)>) -> (ChromeForTesting, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let root = format!("http://{}", listener.local_addr().expect("its address"));
    let served = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&served);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut head = Vec::new();
            let mut byte = [0_u8; 1];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
                head.push(byte[0]);
            }
            counted.fetch_add(1, Ordering::SeqCst);
            let path = String::from_utf8_lossy(&head)
                .split_whitespace()
                .nth(1)
                .unwrap_or("")
                .to_owned();
            let answer = match files.iter().find(|(name, _)| *name == path) {
                Some((_, body)) => {
                    let mut answer = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes();
                    answer.extend(body);
                    answer
                }
                None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_vec(),
            };
            let _ = stream.write_all(&answer);
        }
    });
    (
        ChromeForTesting {
            archives: format!("{root}/archives"),
            latest_of_build: format!("{root}/latest/LATEST_RELEASE"),
        },
        served,
    )
}

#[test]
fn a_chromedriver_is_fetched_whole_and_its_unpublished_patch_falls_back_to_its_build() {
    let name = chromedriver::executable_name();
    let platform = "linux64";
    let (hosts, _) = archives(vec![
        (
            format!("/archives/157.0.8103.79/{platform}/chromedriver-{platform}.zip"),
            archive(&format!("chromedriver-{platform}/{name}"), b"driver 79"),
        ),
        (
            "/latest/LATEST_RELEASE_157.0.8103".to_owned(),
            b"157.0.8103.79\n".to_vec(),
        ),
        (
            format!("/archives/158.0.1.1/{platform}/chromedriver-{platform}.zip"),
            archive("chromedriver/LICENSE", b"no driver here"),
        ),
    ]);
    let dir = tempfile::tempdir().expect("a temporary directory");
    let kept = dir.path().join("157.0.8103.79").join(name);
    chromedriver::fetch(&hosts, "157.0.8103.79", platform, &kept).expect("a published patch");
    assert_eq!(std::fs::read(&kept).expect("the executable"), b"driver 79");
    let fallback = dir.path().join("157.0.8103.80").join(name);
    chromedriver::fetch(&hosts, "157.0.8103.80", platform, &fallback)
        .expect("an unpublished patch takes the newest of its build");
    assert_eq!(
        std::fs::read(&fallback).expect("the executable"),
        b"driver 79"
    );
    assert!(
        !fallback.with_extension("partial").exists(),
        "written whole, then renamed"
    );
    let refused = chromedriver::fetch(
        &hosts,
        "158.0.1.1",
        platform,
        &dir.path().join("158").join(name),
    );
    assert!(
        matches!(&refused, Err(Error::Lane { unavailable: true, message, .. }) if message.contains("holds no")),
        "{refused:?}"
    );
    let missing = chromedriver::fetch(
        &hosts,
        "159.0.0.1",
        platform,
        &dir.path().join("159").join(name),
    );
    assert!(
        matches!(
            missing,
            Err(Error::Lane {
                unavailable: true,
                ..
            })
        ),
        "{missing:?}"
    );
}

#[test]
fn a_kept_chromedriver_is_taken_without_a_fetch() {
    let (hosts, served) = archives(Vec::new());
    let dir = tempfile::tempdir().expect("a temporary directory");
    let kept = dir
        .path()
        .join("157.0.8103.79")
        .join(chromedriver::executable_name());
    std::fs::create_dir_all(kept.parent().expect("its directory")).expect("the cache");
    std::fs::write(&kept, b"kept").expect("a kept driver");
    let chrome = chromedriver::Chrome {
        binary: dir.path().join("chrome"),
        version: "157.0.8103.79".to_owned(),
    };
    // `CHROMEDRIVER` would take precedence: the test runs only where it is not set.
    if std::env::var_os("CHROMEDRIVER").is_some() {
        return;
    }
    assert_eq!(
        chromedriver::resolve(&chrome, dir.path(), &hosts).expect("the kept one"),
        kept
    );
    assert_eq!(served.load(Ordering::SeqCst), 0, "nothing was fetched");
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
    assert!(
        log.of("input.releaseActions").is_empty(),
        "nothing stayed held, so nothing needed releasing"
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
        matches!(&refused, Err(Error::ReleaseFailed { lane, held, message }) if lane == "chrome" && held == &["Control".to_owned()] && message.contains("the tab is busy")),
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
        .capture_os()
        .expect("the canvas's capture");
    assert_eq!((capture.width, capture.height), (2, 2));
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
