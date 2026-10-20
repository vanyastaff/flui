//! `web-input`: the web backend in a real Chrome.
//!
//! Headless Chrome runs `flui-platform`'s browser test target through
//! `wasm-bindgen-test-runner` and the ChromeDriver that matches it, which
//! [`place_chromedriver`] fetches.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

use anyhow::Context as _;
use serde_json::{Value, json};

use super::bidi::{Driver, Session};
use super::chrome;

/// Fetch the ChromeDriver for the installed Chrome into `cache`, when it is
/// not there yet, and place it at `driver`, the path later steps name.
/// Exits 2 when this host has no Chrome.
pub(super) fn place_chromedriver(cache: &Path, driver: &Path) -> anyhow::Result<u8> {
    let Some(chrome) = chrome::installed()? else {
        println!(
            "CANNOT_VERIFY: no Chrome found (set FLUI_CHROME to its executable to use another)"
        );
        return Ok(2);
    };
    println!("Chrome {} at {}", chrome.version, chrome.binary.display());
    let fetched = chrome::chromedriver(&chrome, cache)?;
    if fetched != driver {
        if let Some(dir) = driver.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::copy(&fetched, driver)?;
    }
    Ok(0)
}

/// A directory served over HTTP on a free localhost port, for as long as
/// this lives.
pub(super) struct Page {
    server: Arc<tiny_http::Server>,
    thread: Option<JoinHandle<()>>,
    url: String,
}

impl Page {
    /// Serve `root`.
    pub(super) fn serve(root: &Path) -> anyhow::Result<Self> {
        let server = Arc::new(
            tiny_http::Server::http("127.0.0.1:0")
                .map_err(|error| anyhow::anyhow!("serving the probe page: {error}"))?,
        );
        let port = server
            .server_addr()
            .to_ip()
            .context("a TCP address")?
            .port();
        let root = root.to_path_buf();
        let serving = Arc::clone(&server);
        let thread = std::thread::spawn(move || {
            for request in serving.incoming_requests() {
                let response = respond(&root, request.url());
                let _ = request.respond(response);
            }
        });
        Ok(Self {
            server,
            thread: Some(thread),
            url: format!("http://127.0.0.1:{port}/"),
        })
    }

    /// The page's address, ending in `/`.
    pub(super) fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The file `url` names under `root`, or 404. Paths that climb out are 404.
fn respond(root: &Path, url: &str) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let path = url.split(['?', '#']).next().unwrap_or_default();
    let relative = path.trim_start_matches('/');
    let relative = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };
    let file: PathBuf = root.join(relative);
    let inside = !relative.split('/').any(|part| part == "..");
    match std::fs::read(&file) {
        Ok(bytes) if inside => {
            let kind = match file.extension().and_then(|ext| ext.to_str()) {
                Some("html") => "text/html; charset=utf-8",
                Some("js") => "text/javascript",
                Some("wasm") => "application/wasm",
                _ => "application/octet-stream",
            };
            let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], kind.as_bytes())
                .expect("BUG: a fixed header is valid");
            tiny_http::Response::from_data(bytes).with_header(header)
        }
        _ => tiny_http::Response::from_data(b"not found".to_vec()).with_status_code(404),
    }
}

/// The recording the pointer session writes, under `recordings`, and its
/// golden's name in `crates/flui-platform/tests/native_recording/`.
const POINTER_SESSION: &str = "web_bidi_mouse_pen_touch";

/// The checks of one browser session, each a verdict line.
#[derive(Default)]
struct Verdicts {
    failed: bool,
}

impl Verdicts {
    fn check(&mut self, name: &str, passed: bool, detail: impl std::fmt::Display) {
        let verdict = if passed { "PASS" } else { "FAIL" };
        println!("{name}={verdict}: {detail}");
        self.failed |= !passed;
    }
}

/// Drive the built probe page (`examples/web_input_probe`) in headless
/// Chrome over WebDriver BiDi with trusted input, and write the native
/// recording its pointer session captured under `recordings`. Exits 1 when a
/// check fails.
pub(super) fn run(page: &Path, driver: &Path, recordings: &Path) -> anyhow::Result<u8> {
    let driver = Driver::start(driver)?;
    let page = Page::serve(page)?;
    // At 640 by 480 Chrome delivered the drags but never their drop.
    let mut session = Session::open(&driver, 800, 600)?;
    let mut verdicts = Verdicts::default();

    // The pointer session, recorded; the page has no user activation before
    // its first click, and has it after.
    let first_nonce = load_probe(&mut session, page.url())?;
    let before = session.string("window.probe = new fluiProbe.Probe(), probe.haptic('Vibrate')")?;
    for actions in pointer_session() {
        session.perform(actions)?;
    }
    let after = session.string("probe.haptic('Vibrate')")?;
    verdicts.check(
        "VIBRATION_WITHOUT_ACTIVATION",
        // Refused, and claiming no actuator: a page Chrome would refuse
        // must not advertise feedback it cannot play.
        before.starts_with("Unavailable") && before.contains("feedback: false"),
        &before,
    );
    verdicts.check(
        "VIBRATION_AFTER_ACTIVATION",
        !after.starts_with("Unavailable") && after.contains("feedback: true"),
        &after,
    );
    let report: Value =
        serde_json::from_str(&session.string("probe.finish()")?).context("the probe's report")?;
    let [recording] = report["recordings"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
    else {
        anyhow::bail!("one window, one registry: {}", report["recordings"]);
    };
    let delivered = &report["delivered"];
    let replayed = &report["replayed"][0];
    verdicts.check(
        "RECORDING_REPLAYS_TO_DELIVERED",
        delivered == replayed
            && delivered
                .as_array()
                .is_some_and(|events| !events.is_empty()),
        format!(
            "{} events delivered",
            delivered.as_array().map_or(0, Vec::len)
        ),
    );
    if delivered != replayed {
        println!("delivered: {delivered:#}\nreplayed: {replayed:#}");
    }
    std::fs::create_dir_all(recordings)
        .with_context(|| format!("creating {}", recordings.display()))?;
    let file = recordings.join(format!("{POINTER_SESSION}.jsonl"));
    std::fs::write(&file, recording.as_str().context("a recording")?)
        .with_context(|| format!("writing {}", file.display()))?;
    println!("native recording written to {}", file.display());

    let second_nonce = drag_session(&mut session, page.url(), &mut verdicts)?;
    // A drag names its process by a nonce each page mints anew: two pages
    // that mint theirs at the same point of their start sharing one would
    // take each other's drags for their own.
    verdicts.check(
        "PROCESS_NONCE_DIFFERS_BETWEEN_PAGES",
        first_nonce != second_nonce,
        format!("{first_nonce} then {second_nonce}"),
    );

    ime_session(
        &mut session,
        page.url(),
        &mut verdicts,
        Surface::EditContext,
    )?;
    ime_session(&mut session, page.url(), &mut verdicts, Surface::TextArea)?;

    Ok(u8::from(verdicts.failed))
}

/// The browser surface an input-method session runs through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Surface {
    /// Chrome's own `EditContext` on the canvas.
    EditContext,
    /// The hidden textarea of a browser without `EditContext`, which the
    /// session plays by deleting it before the window opens.
    TextArea,
}

/// The field's text and composition, as the probe reports them.
fn field(session: &mut Session) -> anyhow::Result<Value> {
    serde_json::from_str(&session.string("probe.field()")?).context("the probe's field")
}

/// A key press and release; `key` is a WebDriver key value.
fn key(key: &str) -> Value {
    json!([{
        "type": "key", "id": "keyboard",
        "actions": [{ "type": "keyDown", "value": key }, { "type": "keyUp", "value": key }],
    }])
}

/// Chrome's input method over its DevTools path, into a field the probe
/// focuses after a trusted click on the canvas: a composition that edits the
/// store as composing; Escape during it, which must reach the window marked
/// as the input method's; a commit that ends the composition with the
/// committed text; and a key the window handles, which the browser must not
/// also type into the field.
fn ime_session(
    session: &mut Session,
    url: &str,
    verdicts: &mut Verdicts,
    surface: Surface,
) -> anyhow::Result<()> {
    load_probe(session, url)?;
    let label = match surface {
        Surface::EditContext => "EDIT_CONTEXT",
        Surface::TextArea => {
            session.evaluate("delete globalThis.EditContext")?;
            "TEXTAREA"
        }
    };
    session.evaluate("window.probe = new fluiProbe.Probe(); true")?;
    session.perform(json!([{
        "type": "pointer", "id": "mouse", "parameters": { "pointerType": "mouse" },
        "actions": [
            to(200, 150, 0),
            { "type": "pointerDown", "button": 0 }, { "type": "pointerUp", "button": 0 },
        ],
    }]))?;
    session.evaluate("probe.focus_field(); true")?;
    let surface_found = session.string(
        "String(document.getElementById('flui-canvas').editContext ? 'context' \
           : document.activeElement?.tagName === 'TEXTAREA' ? 'textarea' : 'none')",
    )?;
    let expected = match surface {
        Surface::EditContext => "context",
        Surface::TextArea => "textarea",
    };
    verdicts.check(
        &format!("{label}_SERVES_THE_FIELD"),
        surface_found == expected,
        &surface_found,
    );

    session.devtools(
        "Input.imeSetComposition",
        json!({ "text": "に", "selectionStart": 1, "selectionEnd": 1 }),
    )?;
    let composing = field(session)?;
    verdicts.check(
        &format!("{label}_COMPOSITION_EDITS_THE_STORE"),
        composing["text"] == "に" && composing["composition"] == json!([0, 1]),
        &composing,
    );

    session.perform(key("\u{E00C}"))?;
    let keys: Value = serde_json::from_str(&session.string("probe.keys()")?)?;
    let escape = keys
        .as_array()
        .and_then(|keys| {
            keys.iter().rev().find(|key| {
                key["state"] == "Down"
                    && key["key"]
                        .as_str()
                        .is_some_and(|key| key.contains("Escape") || key.contains("Process"))
            })
        })
        .cloned()
        .unwrap_or(Value::Null);
    verdicts.check(
        &format!("{label}_ESCAPE_DURING_COMPOSITION_IS_THE_INPUT_METHODS"),
        escape["composing"] == true,
        format!("{escape} (field then {})", field(session)?),
    );

    session.devtools(
        "Input.imeSetComposition",
        json!({ "text": "にほん", "selectionStart": 3, "selectionEnd": 3 }),
    )?;
    session.devtools("Input.insertText", json!({ "text": "日本" }))?;
    let committed = field(session)?;
    verdicts.check(
        &format!("{label}_COMMIT_ENDS_THE_COMPOSITION"),
        committed["text"] == "日本" && committed["composition"].is_null(),
        &committed,
    );

    session.perform(key("a"))?;
    let typed = field(session)?;
    let keys: Value = serde_json::from_str(&session.string("probe.keys()")?)?;
    let delivered_a = keys.as_array().is_some_and(|keys| {
        keys.iter()
            .any(|key| key["key"].as_str().is_some_and(|key| key.contains("\"a\"")))
    });
    verdicts.check(
        &format!("{label}_A_HANDLED_KEY_IS_NOT_TYPED_TWICE"),
        typed["text"] == "日本" && delivered_a,
        format!("the window got the key: {delivered_a}; the field holds {typed}"),
    );
    session.evaluate("probe.blur_field(); true")?;
    Ok(())
}

/// Wait for the probe's module to load, and return the process nonce the
/// page mints first.
fn load_probe(session: &mut Session, url: &str) -> anyhow::Result<String> {
    session.navigate(url)?;
    session.evaluate(
        "new Promise((ready, failed) => { const started = performance.now(); \
         const poll = () => window.fluiProbeReady ? ready(true) \
           : performance.now() - started > 20000 ? failed(new Error('the probe did not load')) \
           : setTimeout(poll, 20); poll(); })",
    )?;
    session.string("fluiProbe.Probe.nonce()")
}

/// A pointer move to `(x, y)` over `millis`.
fn to(x: i32, y: i32, millis: u32) -> Value {
    json!({ "type": "pointerMove", "x": x, "y": y, "duration": millis })
}

/// Trusted mouse, pen and touch input at the probe's canvas: a click, a
/// double click, a drag, a secondary click; a pen stroke with pressure,
/// spherical angles and twist; two touches, moving and lifting in turn.
///
/// Every move is one step (no `duration`, which Chrome would interpolate at
/// its frame rate) followed by a pause longer than a frame, so each reaches
/// the page as one `pointermove` and the recording's shape does not depend
/// on the host's timing. Touches move one at a time for the same reason.
fn pointer_session() -> [Value; 3] {
    let settle = || json!({ "type": "pause", "duration": 40 });
    let mouse = json!({
        "type": "pointer", "id": "mouse", "parameters": { "pointerType": "mouse" },
        "actions": [
            to(100, 100, 0), settle(),
            { "type": "pointerDown", "button": 0 }, { "type": "pointerUp", "button": 0 },
            { "type": "pause", "duration": 600 },
            { "type": "pointerDown", "button": 0 }, { "type": "pointerUp", "button": 0 },
            { "type": "pointerDown", "button": 0 }, { "type": "pointerUp", "button": 0 },
            { "type": "pause", "duration": 600 },
            to(120, 120, 0), settle(),
            { "type": "pointerDown", "button": 0 },
            to(140, 125, 0), settle(), to(160, 130, 0), settle(),
            { "type": "pointerUp", "button": 0 },
            { "type": "pointerDown", "button": 2 }, { "type": "pointerUp", "button": 2 },
            to(130, 90, 0), settle(),
        ],
    });
    let pen = json!({
        "type": "pointer", "id": "pen", "parameters": { "pointerType": "pen" },
        "actions": [
            to(200, 150, 0), settle(),
            {
                "type": "pointerDown", "button": 0, "pressure": 0.5, "twist": 90,
                "altitudeAngle": core::f64::consts::FRAC_PI_3, "azimuthAngle": 0.5,
            },
            settle(),
            {
                "type": "pointerMove", "x": 210, "y": 155, "pressure": 0.75, "twist": 90,
                "altitudeAngle": core::f64::consts::FRAC_PI_3, "azimuthAngle": 0.5,
            },
            settle(),
            { "type": "pointerUp", "button": 0 },
        ],
    });
    let contact = |x: i32, y: i32| json!({ "type": "pointerMove", "x": x, "y": y, "width": 20, "height": 30, "pressure": 0.25 });
    let press =
        json!({ "type": "pointerDown", "button": 0, "width": 20, "height": 30, "pressure": 0.25 });
    let wait = json!({ "type": "pause", "duration": 40 });
    let idle = json!({ "type": "pause", "duration": 0 });
    let lift = json!({ "type": "pointerUp", "button": 0 });
    // Tick by tick: both fingers land, the first moves, the second moves,
    // the first lifts, the second lifts. A tick lasts as long as its longest
    // action and a move is spread over its tick, so a move shares its tick
    // only with an instant pause, and both fingers wait between steps.
    let first = json!({
        "type": "pointer", "id": "finger1", "parameters": { "pointerType": "touch" },
        "actions": [
            contact(300, 200), press, wait, contact(310, 200), wait,
            idle, wait, lift, wait, idle,
        ],
    });
    let second = json!({
        "type": "pointer", "id": "finger2", "parameters": { "pointerType": "touch" },
        "actions": [
            contact(330, 220), press, wait, idle, wait,
            contact(320, 230), wait, idle, wait, lift,
        ],
    });
    // Sources in one list act in parallel, tick by tick; the pen and the
    // touches go after the mouse, in their own lists.
    [json!([mouse]), json!([pen]), json!([first, second])]
}

/// HTML drag and drop with trusted input, on a page of its own, whose first
/// process nonce it returns: a session the probe starts on the
/// press, as a runtime does whose draggable lifts before the browser's drag
/// threshold, becomes an HTML drag the page's own element takes, and a drag
/// of the page's draggable element drops onto the canvas, whose target
/// reads the dropped text.
fn drag_session(
    session: &mut Session,
    url: &str,
    verdicts: &mut Verdicts,
) -> anyhow::Result<String> {
    let nonce = load_probe(session, url)?;
    session.evaluate(
        "window.probe = new fluiProbe.Probe(); probe.accept_drops(); probe.arm_drag(); \
         window.pageDrop = ''; const outside = document.getElementById('outside'); \
         for (const name of ['dragenter', 'dragover']) outside.addEventListener(name, event => { event.preventDefault(); event.dataTransfer.dropEffect = 'copy'; }); \
         outside.addEventListener('drop', event => { event.preventDefault(); \
           window.pageDrop = event.dataTransfer.getData('text/plain'); }); true",
    )?;
    let lift = json!({
        "type": "pointer", "id": "mouse", "parameters": { "pointerType": "mouse" },
        "actions": [
            to(100, 100, 0),
            { "type": "pointerDown", "button": 0 },
            to(104, 104, 16), to(110, 110, 16), to(120, 140, 16), to(130, 180, 16),
            to(140, 220, 16), to(150, 260, 16), to(160, 300, 16), to(170, 340, 16),
            to(180, 360, 16), to(181, 361, 16), to(182, 362, 16),
            { "type": "pause", "duration": 200 },
            { "type": "pointerUp", "button": 0 },
        ],
    });
    session.perform(json!([lift]))?;
    let dropped = session.string("window.pageDrop")?;
    let ends = session.string("probe.drag_ends()")?;
    verdicts.check(
        "HTML_DRAG_OUT_OF_THE_CANVAS",
        dropped == "dragged from FLUI" && ends.contains("Dropped"),
        format!("the page received {dropped:?}; the session ended {ends}"),
    );

    session.evaluate(
        "const source = document.getElementById('outside'); source.draggable = true; \
         source.addEventListener('dragstart', event => event.dataTransfer.setData('text/plain', 'dragged from the page')); true",
    )?;
    let into = json!({
        "type": "pointer", "id": "mouse", "parameters": { "pointerType": "mouse" },
        "actions": [
            to(50, 370, 0),
            { "type": "pointerDown", "button": 0 },
            to(54, 366, 16), to(60, 360, 16), to(100, 200, 50), to(120, 150, 50), to(121, 151, 50),
            { "type": "pause", "duration": 100 },
            { "type": "pointerUp", "button": 0 },
            { "type": "pause", "duration": 100 },
        ],
    });
    session.perform(json!([into]))?;
    let received =
        session.evaluate("new Promise(done => setTimeout(() => done(probe.dropped()), 200))")?;
    let received = received["value"].as_str().unwrap_or_default().to_owned();
    verdicts.check(
        "HTML_DROP_INTO_THE_CANVAS",
        received.contains("dragged from the page"),
        received,
    );
    Ok(nonce)
}
