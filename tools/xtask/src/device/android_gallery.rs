//! `android-gallery`: the interaction gallery on an attached Android device,
//! every screen driven by the system's own input and each outcome read
//! twice.
//!
//! The input is what a user of the phone has: touches from a virtual
//! multi-touch screen registered with the device's `uinput` command (the
//! kernel device the real `InputReader` reads, so taps, drags, flings, long
//! presses, double taps and pinches arrive with a touchscreen's timing),
//! `input stylus` and `input mouse` events, hardware key events, and the
//! device's own soft keyboard tapped through the keys its accessibility tree
//! names.
//!
//! FLUI publishes no Android accessibility tree, so the outcome is read from
//! the gallery's development agent (ADR-0095 §3), built in with its `agent`
//! feature: a Unix socket in the application's files, reached as the
//! application's own user (`run-as … nc -U`), that answers the semantics
//! tree with each element's name, focus and rectangle on the surface. The
//! second reading is the screen's pixels (`screencap`), sampled where the
//! right outcome and the wrong one paint different colours.
//!
//! Every screen is opened from the tab strip, which builds it afresh, so one
//! screen's failure leaves the next with its initial state. Each check is
//! reported as it finishes; the run fails if any did. No SDK, no device, a
//! locked one or an agent that never answers cannot verify (exit 2). The
//! gallery is uninstalled at the end.

mod checks;

use std::cell::{Cell, RefCell};
use std::fmt;
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};
use serde_json::{Value, json};

use super::android::{Apk, Device, Registration, Screen, Script, Sdk, keyboard_keys};
use super::gallery::{Corner, Placement, Rgb};

/// The gallery's application.
const GALLERY: Apk = Apk {
    package: "dev.flui.interaction_gallery",
    library: "flui_interaction_gallery",
    label: "FLUI interaction gallery",
};
/// The agent's directory and socket, relative to the application's data
/// directory, where `run-as` starts.
const AGENT_DIR: &str = "files/agent";
const AGENT_SOCKET: &str = "files/agent/agent.sock";
/// How long one observation may take to come true.
const WITHIN: Duration = Duration::from_secs(8);
const POLL: Duration = Duration::from_millis(100);
/// How long something that must not change is watched.
const STILL: Duration = Duration::from_millis(800);
/// How long the gallery may take to launch and answer its first read.
const READY_WITHIN: Duration = Duration::from_secs(40);
/// How long the agent may take to answer one request.
const REPLY_WITHIN: Duration = Duration::from_secs(10);
/// Narrows a run to the checks whose name contains its value.
const ONLY_ENV: &str = "FLUI_GALLERY_CHECKS";
/// Keeps the gallery installed after the run when set.
const KEEP_ENV: &str = "FLUI_GALLERY_KEEP";
/// The tab the gallery opens on, which proves its window is up.
const FIRST_SCREEN: &str = "Swipe actions";
/// The virtual touchscreen's `uinput` id.
const TOUCH_ID: u32 = 7;
/// Time for the system to add a registered `uinput` device.
const REGISTER_SETTLE: Duration = Duration::from_millis(2000);

/// Why a run stops: the host or the device refused the measurement.
#[derive(Debug)]
struct CannotVerify(String);

impl fmt::Display for CannotVerify {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CannotVerify {}

fn host(message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(CannotVerify(message.into()))
}

/// Package, install and drive the gallery built at `library`. Exit 0 when
/// every check passed, 1 when any failed, 2 when the run could not verify.
pub(super) fn run(library: &Path, work: &Path, shots: &Path) -> anyhow::Result<u8> {
    let Some(sdk) = Sdk::locate() else {
        println!(
            "android-gallery: no Android SDK with build-tools, a platform and platform-tools (set ANDROID_HOME)"
        );
        return Ok(2);
    };
    let Some(device) = Device::find(&sdk.adb)? else {
        println!(
            "android-gallery: no Android device is attached and authorized (`adb devices`; FLUI_ANDROID_SERIAL picks one)"
        );
        return Ok(2);
    };
    println!("android-gallery: device {}", device.describe());
    println!(
        "android-gallery: this drives the device for several minutes; do not touch it until it ends"
    );
    let apk = sdk.package(&GALLERY, library, work)?;
    device.adb(["install", "-r", "-t"]).arg(&apk).check()?;
    std::fs::create_dir_all(shots).with_context(|| format!("creating {}", shots.display()))?;
    let outcome = drive(&device, shots);
    // The gallery leaves the device with the run, whatever the run came to,
    // unless it is kept for a look at what a failure left.
    if std::env::var_os(KEEP_ENV).is_some_and(|keep| !keep.is_empty()) {
        println!("android-gallery: {KEEP_ENV} is set; the gallery stays installed");
    } else if let Err(error) = device.adb(["uninstall", GALLERY.package]).check() {
        println!("NOTE: the gallery could not be uninstalled: {error:#}");
    } else {
        println!("android-gallery: uninstalled {}", GALLERY.package);
    }
    match outcome {
        Ok(report) => Ok(report.verdict()),
        Err(error) if error.downcast_ref::<CannotVerify>().is_some() => {
            println!("GALLERY=CANNOT_VERIFY: {error:#}");
            Ok(2)
        }
        Err(error) => {
            println!("GALLERY=FAIL: {error:#}");
            Ok(1)
        }
    }
}

/// What each check came to.
#[derive(Default)]
struct Report {
    rows: Vec<(String, Result<(), String>)>,
}

impl Report {
    fn record(&mut self, name: &str, outcome: anyhow::Result<()>) -> anyhow::Result<()> {
        let outcome = match outcome {
            Ok(()) => Ok(()),
            // A refusal ends the run: the device can no longer be measured.
            Err(error) if error.downcast_ref::<CannotVerify>().is_some() => return Err(error),
            Err(error) => Err(format!("{error:#}")),
        };
        match &outcome {
            Ok(()) => println!("CHECK PASS {name}"),
            Err(why) => println!("CHECK FAIL {name}: {why}"),
        }
        self.rows.push((name.to_owned(), outcome));
        Ok(())
    }

    fn verdict(&self) -> u8 {
        println!("android-gallery summary:");
        for (name, outcome) in &self.rows {
            match outcome {
                Ok(()) => println!("  PASS  {name}"),
                Err(why) => println!("  FAIL  {name}: {why}"),
            }
        }
        let failed = self
            .rows
            .iter()
            .filter(|(_, outcome)| outcome.is_err())
            .count();
        if failed == 0 {
            println!("GALLERY=PASS ({} checks)", self.rows.len());
            0
        } else {
            println!("GALLERY=FAIL ({failed} of {} checks)", self.rows.len());
            1
        }
    }
}

fn drive(device: &Device, shots: &Path) -> anyhow::Result<Report> {
    device.wake()?;
    let token = launch_token();
    device.shell(&format!(
        "run-as {pkg} sh -c 'rm -rf {AGENT_DIR} && mkdir -p files && mkdir -m 700 {AGENT_DIR} && echo {token} > {AGENT_DIR}/token'",
        pkg = GALLERY.package
    ))?;
    device.shell(&format!("am force-stop {}", GALLERY.package))?;
    device.shell(&format!("am start -W -n {}", GALLERY.activity()))?;
    let agent = Agent::connect(device, &token)?;
    let screen = device.screen()?;
    let touch = Touchscreen::register(device, screen)?;
    let driver = Driver {
        device,
        agent: RefCell::new(agent),
        touch: RefCell::new(touch),
        scale: density_scale(device)?,
        origin: Cell::new(Point { x: 0, y: 0 }),
        shots: shots.to_path_buf(),
        lost_taps: Cell::new(0),
    };
    driver.wait("the gallery's tab strip", |driver| {
        Ok(driver.node(BUTTON, FIRST_SCREEN)?.map(drop))
    })?;
    driver.locate_surface()?;
    println!(
        "android-gallery: scale {}, surface at ({}, {})",
        driver.scale,
        driver.origin.get().x,
        driver.origin.get().y
    );
    println!("tree at launch:");
    driver.dump();
    let mut report = Report::default();
    let mut shot: Vec<&str> = Vec::new();
    let only = std::env::var(ONLY_ENV).ok().filter(|only| !only.is_empty());
    for (name, check) in checks::CHECKS {
        if only.as_deref().is_some_and(|only| !name.contains(only)) {
            continue;
        }
        let outcome = driver.open(check.screen).and_then(|()| {
            if !shot.contains(&check.screen) {
                shot.push(check.screen);
                driver.shoot(&check.screen.to_lowercase().replace(' ', "-"))?;
            }
            (check.run)(&driver)
        });
        if outcome.is_err() {
            println!("tree after {name}:");
            driver.dump();
            // Whatever the failure left open (a keyboard, a menu) goes.
            let _ = driver.device.key("KEYCODE_ESCAPE");
        }
        report.record(name, outcome)?;
    }
    if driver.lost_taps.get() > 0 {
        println!(
            "NOTE: {} tab taps did nothing and were retried",
            driver.lost_taps.get()
        );
    }
    Ok(report)
}

/// A launch token: the device's clock and this process, which only this
/// run's client presents.
fn launch_token() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    format!("{nanos:032x}{:08x}", std::process::id())
}

/// Physical pixels per logical pixel: the display's density over 160.
fn density_scale(device: &Device) -> anyhow::Result<f64> {
    let density = device.shell("wm density")?;
    let value = |prefix: &str| {
        density
            .lines()
            .find_map(|line| line.trim().strip_prefix(prefix))
            .and_then(|value| value.trim().parse::<f64>().ok())
    };
    let dpi = value("Override density: ")
        .or_else(|| value("Physical density: "))
        .context("`wm density` names no density")?;
    Ok(dpi / 160.0)
}

/// One check: the screen it opens and what it does there.
pub(super) struct Check {
    pub(super) screen: &'static str,
    pub(super) run: fn(&Driver<'_>) -> anyhow::Result<()>,
}

/// A point in physical screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Point {
    pub(super) x: i32,
    pub(super) y: i32,
}

/// A rectangle in physical screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Rect {
    pub(super) left: i32,
    pub(super) top: i32,
    pub(super) right: i32,
    pub(super) bottom: i32,
}

impl Rect {
    pub(super) fn centre(self) -> Point {
        Point {
            x: i32::midpoint(self.left, self.right),
            y: i32::midpoint(self.top, self.bottom),
        }
    }
}

impl Placement for &Driver<'_> {
    type Rect = Rect;
    type Point = Point;

    fn text_bounds(&mut self, name: &str) -> anyhow::Result<Rect> {
        self.bounds_of(TEXT, name)
    }

    fn at(&self, rect: Rect, corner: Corner, dx: f64, dy: f64) -> anyhow::Result<Point> {
        let middle = i32::midpoint(rect.top, rect.bottom);
        let (x, y) = match corner {
            Corner::TopLeft => (rect.left, rect.top),
            Corner::BottomLeft => (rect.left, rect.bottom),
            Corner::LeftMiddle => (rect.left, middle),
            Corner::RightMiddle => (rect.right, middle),
        };
        Ok(self.offset(Point { x, y }, dx, dy))
    }
}

/// The agent's roles the checks look for.
pub(super) const BUTTON: &str = "button";
pub(super) const TEXT: &str = "label";
pub(super) const HEADING: &str = "heading";
pub(super) const EDIT: &str = "text_input";
pub(super) const MENU: &str = "menu";
pub(super) const MENU_ITEM: &str = "menu_item";
pub(super) const SLIDER: &str = "slider";

/// One element of the tree, flattened, with its rectangle on the screen.
#[derive(Debug, Clone)]
pub(super) struct Element {
    pub(super) role: String,
    /// Its name, else its description: the text a reader speaks.
    pub(super) name: String,
    pub(super) rect: Option<Rect>,
    pub(super) focused: bool,
}

/// The gallery's development agent, over `run-as … nc -U`.
struct Agent {
    child: Child,
    input: ChildStdin,
    lines: Receiver<String>,
    next: u64,
    window: String,
}

impl Agent {
    /// Connects once the gallery's socket answers the hello, and finds its
    /// window.
    fn connect(device: &Device, token: &str) -> anyhow::Result<Self> {
        let deadline = Instant::now() + READY_WITHIN;
        let mut last = String::new();
        while Instant::now() < deadline {
            match Self::try_connect(device, token) {
                Ok(agent) => return Ok(agent),
                Err(error) => last = format!("{error:#}"),
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        Err(host(format!(
            "the gallery's agent never answered within {READY_WITHIN:?} (is the device locked?): {last}"
        )))
    }

    fn try_connect(device: &Device, token: &str) -> anyhow::Result<Self> {
        let mut child = Command::new(&device.adb)
            .args(["-s", device.serial.as_str(), "shell", "-T", "run-as"])
            .args([GALLERY.package, "nc", "-U", AGENT_SOCKET])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("starting the agent bridge")?;
        let input = child.stdin.take().context("the bridge's stdin")?;
        let output = child.stdout.take().context("the bridge's stdout")?;
        let (send, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        let mut agent = Self {
            child,
            input,
            lines,
            next: 1,
            window: String::new(),
        };
        let hello = agent.exchange(
            &json!({ "hello": { "token": token } }),
            Duration::from_secs(3),
        )?;
        anyhow::ensure!(hello.get("hello").is_some(), "no hello: {hello}");
        let windows = agent.call(json!({ "op": "windows" }))?;
        let window = windows["windows"]
            .as_array()
            .and_then(|windows| windows.first())
            .and_then(Value::as_str)
            .context("the agent serves no window yet")?;
        window.clone_into(&mut agent.window);
        Ok(agent)
    }

    fn exchange(&mut self, request: &Value, within: Duration) -> anyhow::Result<Value> {
        writeln!(self.input, "{request}").context("writing to the agent")?;
        self.input.flush()?;
        match self.lines.recv_timeout(within) {
            Ok(line) => {
                serde_json::from_str(&line).with_context(|| format!("an agent reply {line:?}"))
            }
            Err(RecvTimeoutError::Timeout) => bail!("the agent did not answer within {within:?}"),
            Err(RecvTimeoutError::Disconnected) => bail!("the agent closed the connection"),
        }
    }

    /// One request; its `result`, or its error as a failure.
    fn call(&mut self, mut request: Value) -> anyhow::Result<Value> {
        let id = self.next;
        self.next += 1;
        request["id"] = json!(id);
        let reply = self
            .exchange(&request, REPLY_WITHIN)
            .map_err(|error| host(format!("{error:#}")))?;
        if let Some(error) = reply.get("error") {
            bail!("the agent answered {error}");
        }
        anyhow::ensure!(reply["id"] == json!(id), "a reply out of order: {reply}");
        Ok(reply["result"].clone())
    }

    /// The window's tree, flattened in tree order, with surface rectangles.
    fn read(&mut self) -> anyhow::Result<Vec<(Element, Option<Rect>)>> {
        let window = self.window.clone();
        let tree = self.call(json!({ "op": "read", "window": window, "query": {} }))?;
        let mut flat = Vec::new();
        for root in tree["roots"].as_array().into_iter().flatten() {
            flatten(root, &mut flat);
        }
        Ok(flat)
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn flatten(node: &Value, into: &mut Vec<(Element, Option<Rect>)>) {
    let text = |key: &str| node[key].as_str().filter(|text| !text.is_empty());
    let name = text("name")
        .or_else(|| text("description"))
        .unwrap_or_default();
    let surface = node["surface_rect"].as_object().and_then(|rect| {
        let field = |key: &str| rect.get(key).and_then(Value::as_i64);
        let (x, y, width, height) = (field("x")?, field("y")?, field("width")?, field("height")?);
        Some(Rect {
            left: i32::try_from(x).ok()?,
            top: i32::try_from(y).ok()?,
            right: i32::try_from(x + width).ok()?,
            bottom: i32::try_from(y + height).ok()?,
        })
    });
    into.push((
        Element {
            role: node["role"].as_str().unwrap_or_default().to_owned(),
            name: name.to_owned(),
            rect: None,
            focused: node["focused"].as_bool().unwrap_or(false),
        },
        surface,
    ));
    for child in node["children"].as_array().into_iter().flatten() {
        flatten(child, into);
    }
}

/// A virtual multi-touch screen the size of the display, fed by one
/// long-lived `uinput -` process: registered once, then written report by
/// report as each falls due.
struct Touchscreen {
    child: Child,
    input: ChildStdin,
}

impl Touchscreen {
    fn register(device: &Device, screen: Screen) -> anyhow::Result<Self> {
        let mut child = Command::new(&device.adb)
            .args(["-s", device.serial.as_str(), "shell", "-T", "uinput", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("starting the uinput touchscreen")?;
        let input = child.stdin.take().context("the touchscreen's stdin")?;
        let mut touch = Self { child, input };
        let registration = Registration {
            id: TOUCH_ID,
            name: "FLUI Gallery Touchscreen",
            product: 0xabd0,
            keys: &["BTN_TOUCH", "BTN_TOOL_FINGER"],
            axes: &[
                ("ABS_MT_SLOT", 0, 9, 0),
                ("ABS_MT_TRACKING_ID", 0, 65535, 0),
                ("ABS_MT_POSITION_X", 0, screen.width - 1, 0),
                ("ABS_MT_POSITION_Y", 0, screen.height - 1, 0),
                ("ABS_MT_TOUCH_MAJOR", 0, 255, 0),
                ("ABS_MT_PRESSURE", 0, 255, 0),
            ],
            props: &["INPUT_PROP_DIRECT"],
        };
        touch.send(&Script::new(&registration).finish())?;
        std::thread::sleep(REGISTER_SETTLE);
        Ok(touch)
    }

    fn send(&mut self, script: &str) -> anyhow::Result<()> {
        self.input
            .write_all(script.as_bytes())
            .and_then(|()| self.input.flush())
            .map_err(|error| host(format!("the uinput touchscreen refused input: {error}")))
    }

    /// Plays `gesture`, timing it here: a `delay` read from the command's
    /// standard input does not hold back the reports after it, so each
    /// report is written when it is due.
    fn play(&mut self, gesture: Gesture) -> anyhow::Result<()> {
        let start = Instant::now();
        let mut due = Duration::ZERO;
        for step in gesture.steps {
            match step {
                Step::Report(line) => {
                    if let Some(wait) = due.checked_sub(start.elapsed()) {
                        std::thread::sleep(wait);
                    }
                    self.send(&line)?;
                }
                Step::Wait(millis) => due += Duration::from_millis(u64::from(millis)),
            }
        }
        if let Some(wait) = due.checked_sub(start.elapsed()) {
            std::thread::sleep(wait);
        }
        std::thread::sleep(Duration::from_millis(150));
        Ok(())
    }
}

impl Drop for Touchscreen {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One step of a gesture: a report to write, or time to let pass.
enum Step {
    Report(String),
    Wait(u32),
}

/// A touch gesture for the virtual touchscreen.
pub(super) struct Gesture {
    steps: Vec<Step>,
    /// Which slots are down, so the last report lifts them all.
    down: Vec<usize>,
}

impl Gesture {
    fn new() -> Self {
        Self {
            steps: Vec::new(),
            down: Vec::new(),
        }
    }

    fn wait(&mut self, millis: u32) {
        if millis > 0 {
            self.steps.push(Step::Wait(millis));
        }
    }

    fn inject(&mut self, events: &[(&str, &str, i32)]) {
        let mut script = Script::continuing(TOUCH_ID);
        script.inject(events);
        self.steps.push(Step::Report(script.finish()));
    }

    /// One report placing every contact in `points`, slot by slot; the
    /// first report of a contact puts it down.
    fn report(&mut self, points: &[Point]) {
        let mut events: Vec<(&str, &str, i32)> = Vec::new();
        let first = self.down.is_empty();
        for (slot, point) in points.iter().enumerate() {
            events.push(("EV_ABS", "ABS_MT_SLOT", slot as i32));
            if !self.down.contains(&slot) {
                self.down.push(slot);
                events.push(("EV_ABS", "ABS_MT_TRACKING_ID", 100 + slot as i32));
                events.push(("EV_ABS", "ABS_MT_TOUCH_MAJOR", 8));
                events.push(("EV_ABS", "ABS_MT_PRESSURE", 60));
            }
            events.push(("EV_ABS", "ABS_MT_POSITION_X", point.x));
            events.push(("EV_ABS", "ABS_MT_POSITION_Y", point.y));
        }
        if first {
            events.push(("EV_KEY", "BTN_TOUCH", 1));
            events.push(("EV_KEY", "BTN_TOOL_FINGER", 1));
        }
        self.inject(&events);
    }

    /// Lifts every contact.
    fn lift(&mut self) {
        let mut events: Vec<(&str, &str, i32)> = Vec::new();
        for &slot in &self.down {
            events.push(("EV_ABS", "ABS_MT_SLOT", slot as i32));
            events.push(("EV_ABS", "ABS_MT_TRACKING_ID", -1));
        }
        events.push(("EV_KEY", "BTN_TOUCH", 0));
        events.push(("EV_KEY", "BTN_TOOL_FINGER", 0));
        self.down.clear();
        self.inject(&events);
    }

    /// Contacts moving from `strokes[i].0` to `strokes[i].1` together in
    /// `steps` reports `step` milliseconds apart, held still for `hold`
    /// before they lift.
    pub(super) fn strokes(strokes: &[(Point, Point)], steps: u32, step: u32, hold: u32) -> Self {
        let mut gesture = Self::new();
        let at = |t: f64| -> Vec<Point> {
            strokes
                .iter()
                .map(|(from, to)| Point {
                    x: from.x + (f64::from(to.x - from.x) * t).round() as i32,
                    y: from.y + (f64::from(to.y - from.y) * t).round() as i32,
                })
                .collect()
        };
        gesture.report(&at(0.0));
        for index in 1..=steps.max(1) {
            gesture.wait(step);
            gesture.report(&at(f64::from(index) / f64::from(steps.max(1))));
        }
        gesture.wait(hold);
        gesture.lift();
        gesture.wait(50);
        gesture
    }

    /// One finger held still at `from` for `press` milliseconds, then moved
    /// to `to` over `millis` and held there for `hold` before it lifts: a
    /// long press that picks something up and carries it.
    pub(super) fn hold_then_drag(
        from: Point,
        to: Point,
        press: u32,
        millis: u32,
        hold: u32,
    ) -> Self {
        let mut gesture = Self::new();
        gesture.report(&[from]);
        gesture.wait(press);
        let steps = (millis / 16).max(1);
        for index in 1..=steps {
            let t = f64::from(index) / f64::from(steps);
            gesture.wait(16);
            gesture.report(&[Point {
                x: from.x + (f64::from(to.x - from.x) * t).round() as i32,
                y: from.y + (f64::from(to.y - from.y) * t).round() as i32,
            }]);
        }
        gesture.wait(hold);
        gesture.lift();
        gesture.wait(50);
        gesture
    }

    /// One finger down at `at` for `hold` milliseconds.
    pub(super) fn press(at: Point, hold: u32) -> Self {
        let mut gesture = Self::new();
        gesture.report(&[at]);
        gesture.wait(hold);
        gesture.lift();
        gesture.wait(50);
        gesture
    }

    /// Two quick taps at `at`, `gap` milliseconds apart.
    pub(super) fn double_tap(at: Point, gap: u32) -> Self {
        let mut gesture = Self::new();
        gesture.report(&[at]);
        gesture.wait(50);
        gesture.lift();
        gesture.wait(gap);
        gesture.report(&[at]);
        gesture.wait(50);
        gesture.lift();
        gesture.wait(50);
        gesture
    }
}

/// A screen capture, as `screencap` writes it raw: a header of
/// little-endian words (width, height, format and, on newer releases, the
/// colour space), then RGBA rows.
pub(super) struct Capture {
    width: usize,
    height: usize,
    offset: usize,
    data: Vec<u8>,
}

impl Capture {
    fn take(device: &Device) -> anyhow::Result<Self> {
        let output = device.adb(["exec-out", "screencap"]).output()?;
        let data = output.stdout;
        let word = |at: usize| -> anyhow::Result<usize> {
            let bytes: [u8; 4] = data
                .get(at..at + 4)
                .context("a short screencap")?
                .try_into()
                .expect("BUG: four bytes");
            Ok(u32::from_le_bytes(bytes) as usize)
        };
        let (width, height) = (word(0)?, word(4)?);
        let pixels = width * height * 4;
        anyhow::ensure!(
            data.len() >= pixels + 12,
            "a screencap of {} bytes for {width}x{height}",
            data.len()
        );
        Ok(Self {
            width,
            height,
            offset: data.len() - pixels,
            data,
        })
    }

    pub(super) fn at(&self, point: Point) -> Option<Rgb> {
        let (x, y) = (
            usize::try_from(point.x).ok()?,
            usize::try_from(point.y).ok()?,
        );
        if x >= self.width || y >= self.height {
            return None;
        }
        let at = self.offset + (y * self.width + x) * 4;
        Some(Rgb(self.data[at], self.data[at + 1], self.data[at + 2]))
    }
}

/// The gallery on the device, and the input and observations a check uses.
pub(super) struct Driver<'a> {
    device: &'a Device,
    agent: RefCell<Agent>,
    touch: RefCell<Touchscreen>,
    /// Physical pixels per logical pixel.
    scale: f64,
    /// The surface's top-left corner on the screen.
    origin: Cell<Point>,
    shots: PathBuf,
    /// Tab taps that changed nothing and were retried.
    lost_taps: Cell<u32>,
}

impl Driver<'_> {
    // ---------------------------------------------------------------------
    // Geometry
    // ---------------------------------------------------------------------

    /// Where the window's surface lies on the screen: its frame, as the
    /// window manager reports it.
    fn locate_surface(&self) -> anyhow::Result<()> {
        let dump = self
            .device
            .shell(&format!("dumpsys window {}", GALLERY.package))?;
        let frame = regex::Regex::new(r"mFrame=\[(-?\d+),(-?\d+)\]\[(-?\d+),(-?\d+)\]")
            .expect("BUG: a valid pattern");
        if let Some(found) = frame.captures(&dump) {
            let number = |index: usize| found[index].parse::<i32>().unwrap_or(0);
            self.origin.set(Point {
                x: number(1),
                y: number(2),
            });
        }
        Ok(())
    }

    /// `point` moved by a logical offset.
    pub(super) fn offset(&self, point: Point, dx: f64, dy: f64) -> Point {
        Point {
            x: point.x + (dx * self.scale).round() as i32,
            y: point.y + (dy * self.scale).round() as i32,
        }
    }

    // ---------------------------------------------------------------------
    // Observations
    // ---------------------------------------------------------------------

    /// The tree now, each rectangle on the screen.
    pub(super) fn tree(&self) -> anyhow::Result<Vec<Element>> {
        let origin = self.origin.get();
        Ok(self
            .agent
            .borrow_mut()
            .read()?
            .into_iter()
            .map(|(mut element, surface)| {
                element.rect = surface.map(|rect| Rect {
                    left: rect.left + origin.x,
                    top: rect.top + origin.y,
                    right: rect.right + origin.x,
                    bottom: rect.bottom + origin.y,
                });
                element
            })
            .collect())
    }

    /// The one on-screen element of `role` whose name `accept`s, or `None`
    /// when there is none or more than one.
    pub(super) fn find(
        &self,
        role: &str,
        accept: impl Fn(&str) -> bool,
    ) -> anyhow::Result<Option<Element>> {
        let screen = self.device.screen()?;
        let mut found = None;
        for element in self.tree()? {
            if element.role != role || !accept(&element.name) {
                continue;
            }
            let Some(rect) = element.rect else { continue };
            if rect.right <= 0
                || rect.bottom <= 0
                || rect.left >= screen.width
                || rect.top >= screen.height
                || rect.right <= rect.left
                || rect.bottom <= rect.top
            {
                continue;
            }
            if found.is_some() {
                return Ok(None);
            }
            found = Some(element);
        }
        Ok(found)
    }

    pub(super) fn node(&self, role: &str, name: &str) -> anyhow::Result<Option<Element>> {
        self.find(role, |candidate| candidate == name)
    }

    /// Whether any element of `role` named `name` is in the tree at all.
    pub(super) fn published(&self, role: &str, name: &str) -> anyhow::Result<bool> {
        Ok(self
            .tree()?
            .iter()
            .any(|element| element.role == role && element.name == name))
    }

    /// Waits for `observe` to yield a value.
    pub(super) fn wait<T>(
        &self,
        what: &str,
        mut observe: impl FnMut(&Self) -> anyhow::Result<Option<T>>,
    ) -> anyhow::Result<T> {
        let until = Instant::now() + WITHIN;
        loop {
            if let Some(value) = observe(self)? {
                return Ok(value);
            }
            anyhow::ensure!(Instant::now() < until, "timed out waiting for {what}");
            std::thread::sleep(POLL);
        }
    }

    /// Requires `holds` to stay true for a while.
    pub(super) fn stays(
        &self,
        what: &str,
        mut holds: impl FnMut(&Self) -> anyhow::Result<bool>,
    ) -> anyhow::Result<()> {
        let until = Instant::now() + STILL;
        loop {
            anyhow::ensure!(holds(self)?, "{what} did not hold");
            if Instant::now() >= until {
                return Ok(());
            }
            std::thread::sleep(POLL);
        }
    }

    pub(super) fn wait_node(&self, role: &str, name: &str) -> anyhow::Result<Element> {
        self.wait(&format!("{name:?} ({role})"), |driver| {
            driver.node(role, name)
        })
    }

    /// Waits for the readout, or any text, to read exactly `text`.
    pub(super) fn text(&self, text: &str) -> anyhow::Result<()> {
        self.wait_node(TEXT, text).map(drop)
    }

    /// Waits for a text starting with `prefix` that `accept` takes, and
    /// returns it.
    pub(super) fn text_where(
        &self,
        prefix: &str,
        accept: impl Fn(&str) -> bool,
    ) -> anyhow::Result<String> {
        self.wait(&format!("a text {prefix:?}… as expected"), |driver| {
            Ok(driver
                .tree()?
                .into_iter()
                .find(|element| {
                    element.role == TEXT
                        && element.name.starts_with(prefix)
                        && accept(&element.name)
                })
                .map(|element| element.name))
        })
    }

    /// Waits for no element of `role` named `name` to be in the tree.
    pub(super) fn gone(&self, role: &str, name: &str) -> anyhow::Result<()> {
        self.wait(&format!("{name:?} to leave the tree"), |driver| {
            Ok((!driver.published(role, name)?).then_some(()))
        })
    }

    /// The screen rectangle of the one visible element of `role` named
    /// `name`.
    pub(super) fn bounds_of(&self, role: &str, name: &str) -> anyhow::Result<Rect> {
        self.wait_node(role, name)?
            .rect
            .context("an on-screen element has a rectangle")
    }

    /// Waits for the element of `role` named `name` to have the focus.
    pub(super) fn focused(&self, role: &str, name: &str) -> anyhow::Result<()> {
        self.wait(&format!("keyboard focus on {name:?}"), |driver| {
            Ok(driver
                .node(role, name)?
                .filter(|element| element.focused)
                .map(drop))
        })
    }

    /// The name of the focused element, for a failure message.
    pub(super) fn focus_name(&self) -> String {
        self.tree()
            .ok()
            .and_then(|tree| tree.into_iter().find(|element| element.focused))
            .map_or_else(|| "nothing".to_owned(), |element| element.name)
    }

    /// The screen now.
    pub(super) fn capture(&self) -> anyhow::Result<Capture> {
        Capture::take(self.device)
    }

    /// The colour at `point` now.
    pub(super) fn sample(&self, point: Point) -> anyhow::Result<Rgb> {
        self.capture()?
            .at(point)
            .with_context(|| format!("{point:?} is off the screen"))
    }

    /// Waits for the screen to show `expected` at `point`; fails with what
    /// it showed.
    pub(super) fn pixel(
        &self,
        what: &str,
        point: Point,
        expected: Rgb,
        tolerance: u8,
    ) -> anyhow::Result<()> {
        let mut seen = None;
        let found = self.wait(what, |driver| {
            let color = driver.sample(point)?;
            seen = Some(color);
            Ok(color.near(expected, tolerance).then_some(()))
        });
        found.map_err(|_| {
            anyhow::anyhow!(
                "pixel {what} at ({}, {}) is {}, expected {expected}",
                point.x,
                point.y,
                seen.map_or_else(|| "unread".to_owned(), |color| color.to_string())
            )
        })
    }

    /// Saves the screen as `<name>.png` beside the run's other captures.
    pub(super) fn shoot(&self, name: &str) -> anyhow::Result<()> {
        let path = self.shots.join(format!("{name}.png"));
        let output = self.device.adb(["exec-out", "screencap", "-p"]).output()?;
        std::fs::write(&path, output.stdout)
            .with_context(|| format!("writing {}", path.display()))?;
        println!("captured {}", path.display());
        Ok(())
    }

    /// The tree, with each named element's rectangle, for a diagnosis.
    pub(super) fn dump(&self) {
        match self.tree() {
            Ok(tree) => {
                for element in tree.iter().filter(|element| !element.name.is_empty()) {
                    let rect = element.rect.map_or_else(String::new, |rect| {
                        format!(
                            " at ({}, {})-({}, {})",
                            rect.left, rect.top, rect.right, rect.bottom
                        )
                    });
                    let focus = if element.focused { " focused" } else { "" };
                    println!("  {} {:?}{rect}{focus}", element.role, element.name);
                }
            }
            Err(error) => println!("  (no tree: {error:#})"),
        }
    }

    // ---------------------------------------------------------------------
    // Navigation
    // ---------------------------------------------------------------------

    /// Opens a screen from the tab strip, freshly built. A screen already
    /// showing is left first, so every check starts from its initial state.
    fn open(&self, screen: &str) -> anyhow::Result<()> {
        if self.showing(screen)? {
            let other = if screen == FIRST_SCREEN {
                "Focus and keys"
            } else {
                FIRST_SCREEN
            };
            self.navigate(other)?;
        }
        self.navigate(screen)?;
        std::thread::sleep(Duration::from_millis(400));
        Ok(())
    }

    /// Brings the tab into view, taps it and waits for its screen. A tap
    /// that changed nothing is reported and retried once.
    fn navigate(&self, screen: &str) -> anyhow::Result<()> {
        let shown = |driver: &Self| Ok(driver.showing(screen)?.then_some(()));
        self.tab_into_view(screen)?;
        self.tap_node(BUTTON, screen)?;
        if self.wait(&format!("the {screen:?} screen"), shown).is_ok() {
            return Ok(());
        }
        self.lost_taps.set(self.lost_taps.get() + 1);
        println!("NOTE: a tap on the {screen:?} tab changed nothing; tapping again");
        self.tap_node(BUTTON, screen)?;
        self.wait(&format!("the {screen:?} screen"), shown)
    }

    /// Swipes the tab strip until the tab `screen` lies wholly on the
    /// screen.
    fn tab_into_view(&self, screen: &str) -> anyhow::Result<()> {
        let width = self.device.screen()?.width;
        for _ in 0..6 {
            let tab = self
                .tree()?
                .into_iter()
                .find(|element| element.role == BUTTON && element.name == screen)
                .and_then(|element| element.rect)
                .with_context(|| format!("no {screen:?} tab in the tree"))?;
            if tab.left >= 0 && tab.right <= width {
                return Ok(());
            }
            // Drag the strip by up to half the screen towards the tab, and
            // hold before lifting so it does not fling past it.
            let y = tab.centre().y;
            let distance = if tab.right > width {
                -(tab.right - width + width / 4).min(width / 2)
            } else {
                (width / 4 - tab.left).min(width / 2)
            };
            let from = Point { x: width / 2, y };
            let to = Point {
                x: width / 2 + distance,
                y,
            };
            self.gesture(Gesture::strokes(&[(from, to)], 12, 16, 200))?;
            std::thread::sleep(Duration::from_millis(300));
        }
        bail!("the {screen:?} tab never came into view")
    }

    /// Whether `screen` is showing: its title is in the tree beside its tab.
    fn showing(&self, screen: &str) -> anyhow::Result<bool> {
        Ok(self
            .tree()?
            .iter()
            .any(|element| element.role == HEADING && element.name == screen))
    }

    // ---------------------------------------------------------------------
    // Input
    // ---------------------------------------------------------------------

    /// Plays a touch gesture on the virtual touchscreen.
    pub(super) fn gesture(&self, gesture: Gesture) -> anyhow::Result<()> {
        self.touch.borrow_mut().play(gesture)
    }

    /// A finger tap at `point`.
    pub(super) fn tap(&self, point: Point) -> anyhow::Result<()> {
        self.gesture(Gesture::press(point, 40))
    }

    /// A tap at the centre of the one visible element of `role` named
    /// `name`.
    pub(super) fn tap_node(&self, role: &str, name: &str) -> anyhow::Result<()> {
        let rect = self.bounds_of(role, name)?;
        self.tap(rect.centre())
    }

    /// A long press of `press` milliseconds at `from` that then carries the
    /// finger to `to` over `millis`, held there for `hold`.
    pub(super) fn hold_then_drag(
        &self,
        from: Point,
        to: Point,
        press: u32,
        millis: u32,
        hold: u32,
    ) -> anyhow::Result<()> {
        self.gesture(Gesture::hold_then_drag(from, to, press, millis, hold))
    }

    /// One finger from `from` to `to` over `millis`, held still for `hold`.
    pub(super) fn drag(
        &self,
        from: Point,
        to: Point,
        millis: u32,
        hold: u32,
    ) -> anyhow::Result<()> {
        let steps = (millis / 16).max(1);
        self.gesture(Gesture::strokes(&[(from, to)], steps, 16, hold))
    }

    /// Two fingers from `a.0`/`b.0` to `a.1`/`b.1`: a pinch or a spread.
    pub(super) fn pinch(
        &self,
        a: (Point, Point),
        b: (Point, Point),
        millis: u32,
    ) -> anyhow::Result<()> {
        let steps = (millis / 16).max(1);
        self.gesture(Gesture::strokes(&[a, b], steps, 16, 80))
    }

    /// A hardware key, by its `KEYCODE_` name.
    pub(super) fn key(&self, code: &str) -> anyhow::Result<()> {
        self.device.key(code)?;
        std::thread::sleep(Duration::from_millis(150));
        Ok(())
    }

    /// Presses Tab until the focused element's name satisfies `accept`, at
    /// most `presses` times; returns its name.
    pub(super) fn tab_until(
        &self,
        presses: usize,
        accept: impl Fn(&str) -> bool,
    ) -> anyhow::Result<String> {
        let mut seen = Vec::new();
        for _ in 0..presses {
            self.key("KEYCODE_TAB")?;
            let deadline = Instant::now() + Duration::from_millis(600);
            loop {
                let name = self.focus_name();
                if accept(&name) {
                    return Ok(name);
                }
                if Instant::now() >= deadline {
                    seen.push(name);
                    break;
                }
                std::thread::sleep(POLL);
            }
        }
        bail!("Tab never reached the expected control; focus went {seen:?}")
    }

    /// Keys pressed together, as a keyboard chord.
    pub(super) fn chord(&self, codes: &[&str]) -> anyhow::Result<()> {
        self.device
            .shell(&format!("input keycombination {}", codes.join(" ")))?;
        std::thread::sleep(Duration::from_millis(150));
        Ok(())
    }

    /// An `input` command of another source (`stylus`, `mouse`) at a point.
    pub(super) fn input(&self, command: &str) -> anyhow::Result<()> {
        self.device.shell(&format!("input {command}")).map(drop)
    }

    /// The soft keyboard's keys, once the input method shows them.
    pub(super) fn keyboard(&self) -> anyhow::Result<(String, Vec<super::android::ImeKey>)> {
        let ime = self
            .device
            .shell("settings get secure default_input_method")?
            .trim()
            .to_owned();
        let package = ime.split('/').next().unwrap_or_default().to_owned();
        self.wait("the soft keyboard", |driver| {
            let shown = driver
                .device
                .shell("dumpsys input_method")?
                .contains("mInputShown=true");
            if !shown {
                return Ok(None);
            }
            // A dump taken while the keyboard animates in can fail; the
            // next poll takes another.
            let Ok(dump) = driver.device.shell(
                "uiautomator dump --windows /sdcard/flui-gallery-ime.xml >/dev/null && cat /sdcard/flui-gallery-ime.xml",
            ) else {
                return Ok(None);
            };
            let keys = keyboard_keys(&dump, &package);
            Ok((!keys.is_empty()).then_some(keys))
        })
        .map(|keys| (package, keys))
    }
}
