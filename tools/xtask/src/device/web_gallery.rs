//! `web-gallery`: the interaction gallery in a real headless Chrome, every
//! screen driven by trusted input and each outcome read twice.
//!
//! The input is what a user's devices produce: WebDriver BiDi
//! `input.performActions` mouse clicks, drags, wheel notches, keys, pen and
//! touch contacts, which Chrome delivers through its own input pipeline as
//! trusted events (with user activation), and Chrome's input method over its
//! DevTools path. Nothing reaches the host's own mouse or keyboard. After
//! each action a check reads the outcome from the gallery's semantics tree,
//! which an `agent` build serves to the page (`globalThis.__flui_agent`,
//! `examples/interaction_gallery/src/agent.rs`) — the screen's readout text,
//! a row gone, a menu open, the focused element — and from the page's
//! pixels (`browsingContext.captureScreenshot`), sampled where the right and
//! the wrong outcome paint different colours.
//!
//! Every move is one step followed by a pause longer than a frame: Chrome
//! spreads a move with a duration over its frames, so a drag is the
//! driver's own sequence of steps. Every screen is opened from the sidebar,
//! which builds it afresh, so one screen's failure leaves the next with its
//! initial state. Each check is reported as it finishes; the run fails if
//! any did.

mod checks;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use serde_json::{Value, json};

use super::bidi::{Driver, Session};
use super::gallery::{Corner, FOCUS, Placement, Rgb, SWIPE};
use super::web_input::Page;

/// The page's size in CSS pixels, the desktop gallery's window: wide enough
/// for the sidebar and a screen side by side.
const WIDTH: u32 = 1040;
const HEIGHT: u32 = 720;
/// How long one observation may take to come true.
const WITHIN: Duration = Duration::from_secs(8);
const POLL: Duration = Duration::from_millis(50);
/// Between the events of one action: longer than a frame.
const HOLD: u64 = 40;
/// Between the steps of a drag: a frame.
const STEP: u64 = 16;
/// How long something that must not change is watched.
const STILL: Duration = Duration::from_millis(600);
/// How long the page gets to load the gallery and publish its first tree.
const LOAD: Duration = Duration::from_secs(60);
/// Narrows a run to the checks whose name contains its value, such as
/// `focus:`; unset or empty runs every check.
const ONLY_ENV: &str = "FLUI_GALLERY_CHECKS";
/// `1` samples the page's script during each check and, when one fails,
/// names where its samples fell: what a page stuck in a loop the debugger
/// cannot interrupt was doing.
const PROFILE_ENV: &str = "FLUI_GALLERY_PROFILE";

/// Exit 0 when every check passed, 1 when any failed. Captures land in
/// `shots`.
pub(super) fn run(page: &Path, driver: &Path, shots: &Path) -> anyhow::Result<u8> {
    let driver = Driver::start(driver)?;
    let page = Page::serve(page)?;
    // The window a little larger than the page, whose viewport is then set
    // exactly: the window size includes the frame.
    let mut session = Session::open(&driver, WIDTH + 64, HEIGHT + 200)?;
    session.set_viewport(WIDTH, HEIGHT)?;
    session.subscribe_logs()?;
    std::fs::create_dir_all(shots).with_context(|| format!("creating {}", shots.display()))?;
    let mut web = Web {
        session,
        url: page.url().to_owned(),
        shots: shots.to_path_buf(),
    };
    let report = match drive(&mut web) {
        Ok(report) => report,
        Err(error) => {
            println!("GALLERY=FAIL: {error:#}");
            return Ok(1);
        }
    };
    Ok(report.verdict())
}

/// What each check came to.
#[derive(Default)]
struct Report {
    rows: Vec<(String, Result<(), String>)>,
}

impl Report {
    fn record(&mut self, name: &str, outcome: anyhow::Result<()>) {
        let outcome = outcome.map_err(|error| format!("{error:#}"));
        match &outcome {
            Ok(()) => println!("CHECK PASS {name}"),
            Err(why) => println!("CHECK FAIL {name}: {why}"),
        }
        self.rows.push((name.to_owned(), outcome));
    }

    fn verdict(&self) -> u8 {
        println!("web-gallery summary:");
        for (name, outcome) in &self.rows {
            match outcome {
                Ok(()) => println!("  PASS  {name}"),
                Err(why) => println!("  FAIL  {name}: {why}"),
            }
        }
        let failed = self.rows.iter().filter(|(_, row)| row.is_err()).count();
        if failed == 0 {
            println!("GALLERY=PASS ({} checks)", self.rows.len());
            0
        } else {
            println!("GALLERY=FAIL ({failed} of {} checks)", self.rows.len());
            1
        }
    }
}

fn drive(web: &mut Web) -> anyhow::Result<Report> {
    web.load()?;
    let (width, height) = web.viewport()?;
    anyhow::ensure!(
        (width, height) == (f64::from(WIDTH), f64::from(HEIGHT)),
        "the page is {width} by {height} CSS pixels, not {WIDTH} by {HEIGHT}"
    );
    let mut report = Report::default();
    let mut shot: Vec<&str> = Vec::new();
    let only = std::env::var(ONLY_ENV).ok().filter(|only| !only.is_empty());
    let profiling = std::env::var_os(PROFILE_ENV).is_some_and(|value| value == "1");
    for (name, check) in checks::CHECKS {
        if only.as_deref().is_some_and(|only| !name.contains(only)) {
            continue;
        }
        drop(web.session.take_logs());
        let profile = if profiling {
            Some(web.session.start_profile()?)
        } else {
            None
        };
        let outcome = web.open(check.screen).and_then(|()| {
            // Each screen as it first opens, for a person to look at.
            if !shot.contains(&check.screen) {
                shot.push(check.screen);
                web.shoot(&check.screen.to_lowercase().replace(' ', "-"))?;
            }
            (check.run)(web)
        });
        let hottest = profile.map(super::bidi::Profile::hottest);
        if outcome.is_err() {
            if let Some(hottest) = hottest {
                match hottest {
                    Ok(lines) => lines.iter().for_each(|line| println!("  script: {line}")),
                    Err(error) => println!("  (no profile: {error:#})"),
                }
            }
            println!("tree after {name}:");
            web.dump();
            for line in web.session.take_logs() {
                println!("  page: {line}");
            }
            if let Ok(seen) = web.script("globalThis.__fluiSeen ?? []") {
                for event in seen.as_array().into_iter().flatten() {
                    println!("  input: {}", event.as_str().unwrap_or_default());
                }
            }
            let _ = web.shoot(&format!("failed-{}", slug(name)));
            // A failed check may leave a pointer or key held, or the page
            // gone; the next check starts from a fresh page.
            if let Err(error) = web.load() {
                report.record(name, outcome);
                return Err(error.context("reloading the gallery after a failed check"));
            }
        }
        report.record(name, outcome);
    }
    Ok(report)
}

fn slug(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
}

/// One check: the screen it opens and what it does there.
pub(super) struct Check {
    pub(super) screen: &'static str,
    pub(super) run: fn(&mut Web) -> anyhow::Result<()>,
}

/// A point in the page, in CSS pixels (at a device pixel ratio of one, the
/// capture's pixels too).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Point {
    pub(super) x: f64,
    pub(super) y: f64,
}

impl Point {
    /// This point moved by `(dx, dy)`.
    pub(super) fn offset(self, dx: f64, dy: f64) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
        }
    }

    /// The whole pixel BiDi addresses.
    fn wire(self) -> (i64, i64) {
        (self.x.round() as i64, self.y.round() as i64)
    }
}

/// A node's bounds, in CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Bounds {
    pub(super) left: f64,
    pub(super) top: f64,
    pub(super) right: f64,
    pub(super) bottom: f64,
}

impl Bounds {
    pub(super) fn centre(self) -> Point {
        Point {
            x: f64::midpoint(self.left, self.right),
            y: f64::midpoint(self.top, self.bottom),
        }
    }

    /// The middle of the left edge, `inset` pixels in.
    pub(super) fn left_middle(self, inset: f64) -> Point {
        Point {
            x: self.left + inset,
            y: f64::midpoint(self.top, self.bottom),
        }
    }

    /// The middle of the right edge, `inset` pixels in.
    pub(super) fn right_middle(self, inset: f64) -> Point {
        Point {
            x: self.right - inset,
            y: f64::midpoint(self.top, self.bottom),
        }
    }

    pub(super) fn top_left(self) -> Point {
        Point {
            x: self.left,
            y: self.top,
        }
    }

    pub(super) fn bottom_left(self) -> Point {
        Point {
            x: self.left,
            y: self.bottom,
        }
    }

    fn overlaps_viewport(self) -> bool {
        self.right > self.left
            && self.bottom > self.top
            && self.left < f64::from(WIDTH)
            && self.right > 0.0
            && self.top < f64::from(HEIGHT)
            && self.bottom > 0.0
    }
}

/// The roles the checks look for, in the agent's role vocabulary.
pub(super) const BUTTON: &str = "button";
pub(super) const EDIT: &str = "text_input";
pub(super) const MENU: &str = "menu";
pub(super) const MENU_ITEM: &str = "menu_item";
pub(super) const SLIDER: &str = "slider";
/// Any node whose text is the name: a label, a readout, a merged row.
pub(super) const TEXT: &str = "";

/// One node of the published tree, as the checks read it.
#[derive(Debug, Clone)]
pub(super) struct Node {
    role: String,
    /// What a screen reader speaks: the name, else the text a role that may
    /// not be named carries (a label's value or description). A field's
    /// value is its content, not its name.
    name: String,
    /// The current value: a field's text.
    value: Option<String>,
    bounds: Option<Bounds>,
    focused: bool,
}

impl Node {
    fn from_wire(node: &Value) -> Self {
        let text = |key: &str| node[key].as_str().map(str::to_owned);
        let bounds = node["surface_rect"].as_object().map(|rect| {
            let number = |key: &str| rect.get(key).and_then(Value::as_f64).unwrap_or(0.0);
            Bounds {
                left: number("x"),
                top: number("y"),
                right: number("x") + number("width"),
                bottom: number("y") + number("height"),
            }
        });
        let role = node["role"].as_str().unwrap_or_default().to_owned();
        let value = text("value");
        let name = text("name")
            .or_else(|| text("description"))
            .or_else(|| (role != EDIT).then(|| value.clone()).flatten())
            .unwrap_or_default();
        Self {
            role,
            name,
            value,
            bounds,
            focused: node["focused"].as_bool().unwrap_or(false),
        }
    }

    /// The node's current value: a field's text.
    pub(super) fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }

    /// Whether the node is of `role`; [`TEXT`] takes any role.
    fn is(&self, role: &str) -> bool {
        role.is_empty() || self.role == role
    }

    fn visible(&self) -> bool {
        self.bounds.is_some_and(Bounds::overlaps_viewport)
    }
}

/// One capture of the page.
pub(super) struct Shot(image::RgbaImage);

impl Shot {
    pub(super) fn at(&self, point: Point) -> Option<Rgb> {
        let x = u32::try_from(point.x.floor() as i64).ok()?;
        let y = u32::try_from(point.y.floor() as i64).ok()?;
        let pixel = self.0.get_pixel_checked(x, y)?;
        Some(Rgb(pixel[0], pixel[1], pixel[2]))
    }
}

/// The gallery's page, and the input and observations a check uses.
pub(super) struct Web {
    session: Session,
    url: String,
    shots: PathBuf,
}

impl Placement for &mut Web {
    type Rect = Bounds;
    type Point = Point;

    fn text_bounds(&mut self, name: &str) -> anyhow::Result<Bounds> {
        self.bounds_of(TEXT, name)
    }

    fn at(&self, rect: Bounds, corner: Corner, dx: f64, dy: f64) -> anyhow::Result<Point> {
        let point = match corner {
            Corner::TopLeft => rect.top_left(),
            Corner::BottomLeft => rect.bottom_left(),
            Corner::LeftMiddle => rect.left_middle(0.0),
            Corner::RightMiddle => rect.right_middle(0.0),
        };
        Ok(point.offset(dx, dy))
    }
}

/// WebDriver's key values for the keys the checks press.
pub(super) mod key {
    pub(super) const TAB: &str = "\u{E004}";
    pub(super) const ENTER: &str = "\u{E007}";
    pub(super) const SHIFT: &str = "\u{E008}";
    pub(super) const CONTROL: &str = "\u{E009}";
    pub(super) const ESCAPE: &str = "\u{E00C}";
    pub(super) const SPACE: &str = " ";
    pub(super) const HOME: &str = "\u{E011}";
    pub(super) const UP: &str = "\u{E013}";
    pub(super) const RIGHT: &str = "\u{E014}";
    pub(super) const DOWN: &str = "\u{E015}";
    pub(super) const F5: &str = "\u{E035}";
    pub(super) const F10: &str = "\u{E03A}";
}

impl Web {
    // ---------------------------------------------------------------------
    // The page
    // ---------------------------------------------------------------------

    /// Load the gallery and wait for its agent to publish the sidebar.
    fn load(&mut self) -> anyhow::Result<()> {
        let url = self.url.clone();
        self.session.navigate(&url)?;
        let ready = self.session.evaluate(&format!(
            "new Promise(ready => {{ const started = performance.now(); \
             const poll = () => document.getElementById('flui-error') ? ready('the page failed: ' + document.getElementById('flui-error').textContent) \
               : globalThis.__flui_agent ? ready('') \
               : performance.now() - started > {} ? ready('the page never installed __flui_agent; is the gallery built with its agent feature?') \
               : setTimeout(poll, 50); poll(); }})",
            LOAD.as_millis()
        ))?;
        let problem = ready["value"].as_str().unwrap_or_default();
        anyhow::ensure!(problem.is_empty(), "{problem}");
        // What input reached the page, for a failed check's diagnosis: the
        // last events of each kind a check sends, as the page saw them.
        self.session.evaluate(
            "globalThis.__fluiSeen = []; \
             for (const kind of ['pointerdown', 'pointermove', 'pointerup', 'pointercancel', \
               'wheel', 'keydown', 'keyup', 'contextmenu', 'compositionstart', 'compositionend']) \
               addEventListener(kind, event => { \
                 __fluiSeen.push(`${kind} ${event.pointerType ?? ''} ${Math.round(event.clientX ?? 0)},${Math.round(event.clientY ?? 0)} ${event.key ?? ''} ${event.defaultPrevented ? 'prevented' : ''}`); \
                 if (__fluiSeen.length > 40) __fluiSeen.shift(); }, { capture: true, passive: true }); \
             globalThis.__fluiLast = {}; \
             for (const kind of ['keydown', 'wheel']) \
               addEventListener(kind, event => { __fluiLast[kind] = event; }, { capture: true }); \
             true",
        )?;
        let until = Instant::now() + LOAD;
        loop {
            if self.showing_button(SWIPE)? {
                // Let the first frames land before the page is driven.
                std::thread::sleep(Duration::from_millis(300));
                return Ok(());
            }
            anyhow::ensure!(
                Instant::now() < until,
                "the gallery never published its sidebar"
            );
            std::thread::sleep(POLL);
        }
    }

    /// The page's inner size, in CSS pixels.
    fn viewport(&mut self) -> anyhow::Result<(f64, f64)> {
        let size = self
            .session
            .string("JSON.stringify([innerWidth, innerHeight, devicePixelRatio])")?;
        let size: Vec<f64> = serde_json::from_str(&size).context("the page's size")?;
        anyhow::ensure!(
            size.get(2) == Some(&1.0),
            "the page's device pixel ratio is {:?}, not 1",
            size.get(2)
        );
        Ok((size[0], size[1]))
    }

    /// Whether the page cancelled the default action of the last event of
    /// `kind` (`keydown` or `wheel`), read off the event once its dispatch
    /// is over, whether or not the page stopped its propagation:
    /// a cancelled F5 does not reload, a cancelled Ctrl+wheel does not zoom
    /// the page. Headless Chrome performs neither default for driven input,
    /// so this, not the page's state after, is what tells a desktop Chrome's
    /// outcome.
    pub(super) fn last_prevented(&mut self, kind: &str) -> anyhow::Result<bool> {
        let last = self.script(&format!(
            "globalThis.__fluiLast?.{kind} ? {{ key: __fluiLast.{kind}.key ?? null, prevented: __fluiLast.{kind}.defaultPrevented }} : null"
        ))?;
        last["prevented"]
            .as_bool()
            .with_context(|| format!("no {kind} reached the page: {last}"))
    }

    /// Evaluate `expression` for its JSON value.
    pub(super) fn script(&mut self, expression: &str) -> anyhow::Result<Value> {
        let text = self
            .session
            .string(&format!("JSON.stringify({expression})"))?;
        serde_json::from_str(&text).context("a script's value")
    }

    // ---------------------------------------------------------------------
    // Observations
    // ---------------------------------------------------------------------

    /// Every node of the published tree, depth first.
    fn tree(&mut self) -> anyhow::Result<Vec<Node>> {
        let request = json!({ "id": 1, "method": "semantics.read", "params": {} }).to_string();
        let reply = self
            .session
            .string(&format!("globalThis.__flui_agent.call({request:?})"))?;
        let reply: Value = serde_json::from_str(&reply).context("the agent's reply")?;
        if let Some(error) = reply.get("error") {
            if error["code"] == "busy" {
                return Ok(Vec::new());
            }
            anyhow::bail!("the agent refused the read: {error}");
        }
        fn walk(nodes: &Value, into: &mut Vec<Node>) {
            for node in nodes.as_array().into_iter().flatten() {
                into.push(Node::from_wire(node));
                walk(&node["children"], into);
            }
        }
        let mut nodes = Vec::new();
        walk(&reply["result"]["roots"], &mut nodes);
        Ok(nodes)
    }

    /// The one visible node of `role` whose name satisfies `accept`, or
    /// `None` when there is none or more than one.
    pub(super) fn find(
        &mut self,
        role: &str,
        accept: impl Fn(&str) -> bool,
    ) -> anyhow::Result<Option<Node>> {
        let mut found = None;
        for node in self.tree()? {
            if !node.is(role) || !accept(&node.name) || !node.visible() {
                continue;
            }
            if found.is_some() {
                return Ok(None);
            }
            found = Some(node);
        }
        Ok(found)
    }

    /// The one visible node of `role` named `name`.
    pub(super) fn node(&mut self, role: &str, name: &str) -> anyhow::Result<Option<Node>> {
        self.find(role, |candidate| candidate == name)
    }

    /// Whether any node of `role` named `name` is published at all.
    fn published(&mut self, role: &str, name: &str) -> anyhow::Result<bool> {
        Ok(self
            .tree()?
            .iter()
            .any(|node| node.is(role) && node.name == name))
    }

    /// Waits for `observe` to yield a value.
    pub(super) fn wait<T>(
        &mut self,
        what: &str,
        mut observe: impl FnMut(&mut Self) -> anyhow::Result<Option<T>>,
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
        &mut self,
        what: &str,
        mut holds: impl FnMut(&mut Self) -> anyhow::Result<bool>,
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

    pub(super) fn wait_node(&mut self, role: &str, name: &str) -> anyhow::Result<Node> {
        self.wait(&format!("{name:?} ({role:?})"), |web| web.node(role, name))
    }

    /// Waits for the readout, or any text, to read exactly `text`.
    pub(super) fn text(&mut self, text: &str) -> anyhow::Result<()> {
        self.wait_node(TEXT, text).map(drop)
    }

    /// Waits for a text starting with `prefix` that `accept` takes, and
    /// returns its whole name.
    pub(super) fn text_where(
        &mut self,
        prefix: &str,
        accept: impl Fn(&str) -> bool,
    ) -> anyhow::Result<String> {
        self.wait(&format!("a text {prefix:?}… as expected"), |web| {
            Ok(web
                .tree()?
                .into_iter()
                .find(|node| node.name.starts_with(prefix) && accept(&node.name))
                .map(|node| node.name))
        })
    }

    /// Waits for no node of `role` named `name` to be published.
    pub(super) fn gone(&mut self, role: &str, name: &str) -> anyhow::Result<()> {
        self.wait(&format!("{name:?} to leave the tree"), |web| {
            Ok((!web.published(role, name)?).then_some(()))
        })
    }

    /// The bounds of the one visible node of `role` named `name`.
    pub(super) fn bounds_of(&mut self, role: &str, name: &str) -> anyhow::Result<Bounds> {
        let node = self.wait_node(role, name)?;
        node.bounds
            .with_context(|| format!("{name:?} has no bounds"))
    }

    /// Waits for the node of `role` named `name` to have the keyboard focus.
    pub(super) fn focused(&mut self, role: &str, name: &str) -> anyhow::Result<()> {
        self.wait(&format!("keyboard focus on {name:?}"), |web| {
            Ok(web.node(role, name)?.filter(|node| node.focused).map(drop))
        })
        .map_err(|error| {
            let now = self.focus_name();
            error.context(format!("focus is on {now:?}"))
        })
    }

    /// The name of the node with the keyboard focus, for a failure message.
    pub(super) fn focus_name(&mut self) -> String {
        self.tree()
            .ok()
            .and_then(|nodes| nodes.into_iter().rev().find(|node| node.focused))
            .map_or_else(|| "nothing".to_owned(), |node| node.name)
    }

    /// Whether any visible node of `role` has the keyboard focus.
    pub(super) fn focus_on_role(&mut self, role: &str) -> anyhow::Result<bool> {
        Ok(self
            .tree()?
            .iter()
            .any(|node| node.is(role) && node.focused && node.visible()))
    }

    /// Presses Tab until the focused node's name satisfies `accept`, at most
    /// `presses` times; returns its name.
    pub(super) fn tab_until(
        &mut self,
        presses: usize,
        accept: impl Fn(&str) -> bool,
    ) -> anyhow::Result<String> {
        let mut seen = Vec::new();
        for _ in 0..presses {
            self.chord(&[key::TAB])?;
            let deadline = Instant::now() + Duration::from_millis(500);
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
        anyhow::bail!("Tab never reached the expected control; focus went {seen:?}")
    }

    /// The page as Chrome composited it.
    pub(super) fn capture(&mut self) -> anyhow::Result<Shot> {
        let png = self.session.capture()?;
        let image = image::load_from_memory(&png).context("reading a capture")?;
        Ok(Shot(image.to_rgba8()))
    }

    /// The colour at `point` now.
    pub(super) fn sample(&mut self, point: Point) -> anyhow::Result<Rgb> {
        self.capture()?
            .at(point)
            .with_context(|| format!("{point:?} is outside the page"))
    }

    /// Waits for the page to show `expected` at `point` (within
    /// `tolerance`); fails with what it showed.
    pub(super) fn pixel(
        &mut self,
        what: &str,
        point: Point,
        expected: Rgb,
        tolerance: u8,
    ) -> anyhow::Result<()> {
        let mut seen = None;
        let found = self.wait(what, |web| {
            let color = web.sample(point)?;
            seen = Some(color);
            Ok(color.near(expected, tolerance).then_some(()))
        });
        found.map_err(|_| {
            anyhow::anyhow!(
                "pixel {what} at ({:.0}, {:.0}) is {}, expected {expected}",
                point.x,
                point.y,
                seen.map_or_else(|| "unread".to_owned(), |color| color.to_string())
            )
        })
    }

    /// Waits for the colour at `point` to leave `from`.
    pub(super) fn pixel_leaves(
        &mut self,
        what: &str,
        point: Point,
        from: Rgb,
        tolerance: u8,
    ) -> anyhow::Result<()> {
        self.wait(what, |web| {
            Ok((!web.sample(point)?.near(from, tolerance)).then_some(()))
        })
    }

    /// Saves the page as `<name>.png` beside the run's other captures.
    pub(super) fn shoot(&mut self, name: &str) -> anyhow::Result<()> {
        let path = self.shots.join(format!("{name}.png"));
        let png = self.session.capture()?;
        std::fs::write(&path, png).with_context(|| format!("writing {}", path.display()))?;
        println!("captured {}", path.display());
        Ok(())
    }

    /// The tree, with each named node's role and bounds, for a failure's
    /// diagnosis.
    pub(super) fn dump(&mut self) {
        match self.tree() {
            Ok(nodes) => {
                for node in nodes.iter().filter(|node| !node.name.is_empty()) {
                    let bounds = node.bounds.map_or_else(String::new, |b| {
                        format!(
                            " at ({:.0}, {:.0})-({:.0}, {:.0})",
                            b.left, b.top, b.right, b.bottom
                        )
                    });
                    let focus = if node.focused { " focused" } else { "" };
                    println!("  {} {:?}{bounds}{focus}", node.role, node.name);
                }
            }
            Err(error) => println!("  (unreadable: {error:#})"),
        }
    }

    // ---------------------------------------------------------------------
    // Navigation
    // ---------------------------------------------------------------------

    /// Opens a screen from the sidebar, freshly built, and waits for its
    /// title. A screen already showing is left first, so every check starts
    /// from the screen's initial state.
    fn open(&mut self, screen: &str) -> anyhow::Result<()> {
        if self.showing(screen)? {
            let other = if screen == SWIPE { FOCUS } else { SWIPE };
            self.navigate(other)?;
        }
        self.navigate(screen)?;
        // Let the screen's first frames land before it is driven.
        std::thread::sleep(Duration::from_millis(300));
        Ok(())
    }

    fn navigate(&mut self, screen: &str) -> anyhow::Result<()> {
        self.click_node(BUTTON, screen)?;
        self.wait(&format!("the {screen:?} screen"), |web| {
            Ok(web.showing(screen)?.then_some(()))
        })
    }

    /// Whether `screen` is the one showing: its title is published beside
    /// the sidebar button of the same name.
    fn showing(&mut self, screen: &str) -> anyhow::Result<bool> {
        Ok(self
            .tree()?
            .iter()
            .any(|node| node.role != BUTTON && node.name == screen))
    }

    fn showing_button(&mut self, screen: &str) -> anyhow::Result<bool> {
        Ok(self
            .tree()?
            .iter()
            .any(|node| node.role == BUTTON && node.name == screen))
    }

    // ---------------------------------------------------------------------
    // Input
    // ---------------------------------------------------------------------

    fn perform(&mut self, sources: Value) -> anyhow::Result<()> {
        self.session.perform(sources)
    }

    /// A mouse source acting `actions`.
    fn mouse(actions: Vec<Value>) -> Value {
        json!({
            "type": "pointer", "id": "mouse", "parameters": { "pointerType": "mouse" },
            "actions": actions,
        })
    }

    /// A left click at `point`.
    pub(super) fn click(&mut self, point: Point) -> anyhow::Result<()> {
        self.press(point, 0, HOLD)
    }

    /// A press of `button` at `point`, released after `hold` milliseconds.
    fn press(&mut self, point: Point, button: u8, hold: u64) -> anyhow::Result<()> {
        self.perform(json!([Self::mouse(vec![
            to(point),
            pause(HOLD),
            json!({ "type": "pointerDown", "button": button }),
            pause(hold),
            json!({ "type": "pointerUp", "button": button }),
            pause(HOLD),
        ])]))
    }

    /// Two left clicks at `point`, inside the double-click time.
    pub(super) fn double_click(&mut self, point: Point) -> anyhow::Result<()> {
        self.perform(json!([Self::mouse(vec![
            to(point),
            pause(HOLD),
            json!({ "type": "pointerDown", "button": 0 }),
            pause(30),
            json!({ "type": "pointerUp", "button": 0 }),
            pause(60),
            json!({ "type": "pointerDown", "button": 0 }),
            pause(30),
            json!({ "type": "pointerUp", "button": 0 }),
            pause(HOLD),
        ])]))
    }

    /// A left press at `point` held for `hold`.
    pub(super) fn press_and_hold(&mut self, point: Point, hold: Duration) -> anyhow::Result<()> {
        self.press(point, 0, millis(hold))
    }

    /// A right click at `point`.
    pub(super) fn right_click(&mut self, point: Point) -> anyhow::Result<()> {
        self.press(point, 2, HOLD)
    }

    /// A click at the centre of the one visible node of `role` named `name`.
    pub(super) fn click_node(&mut self, role: &str, name: &str) -> anyhow::Result<()> {
        let bounds = self.bounds_of(role, name)?;
        self.click(bounds.centre())
    }

    /// A left-button mouse drag from `from` to `to` in `steps` moves, held
    /// still at the end for `settle` before the release.
    pub(super) fn mouse_drag(
        &mut self,
        from: Point,
        to_point: Point,
        steps: u32,
        settle: Duration,
    ) -> anyhow::Result<()> {
        let mut actions = vec![
            to(from),
            pause(HOLD),
            json!({ "type": "pointerDown", "button": 0 }),
            pause(HOLD),
        ];
        actions.extend(steps_between(from, to_point, steps));
        actions.push(pause(millis(settle)));
        actions.push(json!({ "type": "pointerUp", "button": 0 }));
        actions.push(pause(HOLD));
        self.perform(json!([Self::mouse(actions)]))
    }

    /// `notches` wheel notches at `point`, positive away from the user
    /// (scrolling up), with Control held when `control`.
    pub(super) fn wheel(
        &mut self,
        point: Point,
        notches: i32,
        control: bool,
    ) -> anyhow::Result<()> {
        let (x, y) = point.wire();
        // A notch is 100 CSS pixels in Chrome's own wheel events; away from
        // the user is a negative delta.
        let delta = if notches > 0 { -100 } else { 100 };
        let mut wheel = vec![pause(0)];
        let mut keys = vec![if control {
            json!({ "type": "keyDown", "value": key::CONTROL })
        } else {
            pause(0)
        }];
        for _ in 0..notches.unsigned_abs() {
            wheel.push(json!({
                "type": "scroll", "x": x, "y": y, "deltaX": 0, "deltaY": delta, "duration": 0,
            }));
            keys.push(pause(0));
            wheel.push(pause(50));
            keys.push(pause(50));
        }
        if control {
            wheel.push(pause(0));
            keys.push(json!({ "type": "keyUp", "value": key::CONTROL }));
        }
        // The pointer rests over the point first, as a hand on a mouse does.
        self.perform(json!([Self::mouse(vec![to(point), pause(HOLD)])]))?;
        self.perform(json!([
            { "type": "key", "id": "keyboard", "actions": keys },
            { "type": "wheel", "id": "wheel", "actions": wheel },
        ]))?;
        std::thread::sleep(Duration::from_millis(HOLD));
        Ok(())
    }

    /// Presses the keys of `chord` in order and releases them in reverse.
    pub(super) fn chord(&mut self, chord: &[&str]) -> anyhow::Result<()> {
        let mut actions: Vec<Value> = chord
            .iter()
            .map(|&value| json!({ "type": "keyDown", "value": value }))
            .collect();
        actions.push(pause(HOLD));
        actions.extend(
            chord
                .iter()
                .rev()
                .map(|&value| json!({ "type": "keyUp", "value": value })),
        );
        self.perform(json!([{ "type": "key", "id": "keyboard", "actions": actions }]))?;
        std::thread::sleep(Duration::from_millis(80));
        Ok(())
    }

    /// Types `text` one key per character.
    pub(super) fn type_text(&mut self, text: &str) -> anyhow::Result<()> {
        let mut actions = Vec::new();
        for character in text.chars() {
            let value = character.to_string();
            actions.push(json!({ "type": "keyDown", "value": value }));
            actions.push(json!({ "type": "keyUp", "value": value }));
            actions.push(pause(HOLD));
        }
        self.perform(json!([{ "type": "key", "id": "keyboard", "actions": actions }]))
    }

    /// The menu key, which WebDriver's key table has no value for: a trusted
    /// key event through Chrome's DevTools input path, the one BiDi's own
    /// keys take.
    pub(super) fn menu_key(&mut self) -> anyhow::Result<()> {
        for kind in ["rawKeyDown", "keyUp"] {
            self.session.devtools(
                "Input.dispatchKeyEvent",
                json!({
                    "type": kind, "key": "ContextMenu", "code": "ContextMenu",
                    "windowsVirtualKeyCode": 93, "nativeVirtualKeyCode": 93,
                }),
            )?;
            std::thread::sleep(Duration::from_millis(HOLD));
        }
        Ok(())
    }

    /// Chrome's input method composing `text` with the caret at its end.
    pub(super) fn compose(&mut self, text: &str) -> anyhow::Result<()> {
        let end = text.encode_utf16().count();
        self.session.devtools(
            "Input.imeSetComposition",
            json!({ "text": text, "selectionStart": end, "selectionEnd": end }),
        )?;
        Ok(())
    }

    /// Chrome's input method committing `text`.
    pub(super) fn commit(&mut self, text: &str) -> anyhow::Result<()> {
        self.session
            .devtools("Input.insertText", json!({ "text": text }))?;
        Ok(())
    }

    /// One finger from `from` to `to` over `duration`, then held still for
    /// `hold` before it lifts.
    pub(super) fn touch_drag(
        &mut self,
        from: Point,
        to_point: Point,
        duration: Duration,
        hold: Duration,
    ) -> anyhow::Result<()> {
        let steps = u32::try_from((millis(duration) / STEP).max(1)).unwrap_or(1);
        let mut actions = vec![contact(from), json!({ "type": "pointerDown", "button": 0 })];
        actions.push(pause(STEP));
        for step in 1..=steps {
            actions.push(contact(lerp(from, to_point, step, steps)));
            actions.push(pause(STEP));
        }
        actions.push(pause(millis(hold)));
        actions.push(json!({ "type": "pointerUp", "button": 0 }));
        actions.push(pause(HOLD));
        self.perform(json!([finger("finger1", actions)]))
    }

    /// A finger tap at `point`.
    pub(super) fn touch_tap(&mut self, point: Point) -> anyhow::Result<()> {
        self.perform(json!([finger(
            "finger1",
            vec![
                contact(point),
                json!({ "type": "pointerDown", "button": 0 }),
                pause(HOLD),
                json!({ "type": "pointerUp", "button": 0 }),
                pause(HOLD),
            ]
        )]))
    }

    /// Two finger taps at `point`, inside the double-tap time.
    pub(super) fn touch_double_tap(&mut self, point: Point) -> anyhow::Result<()> {
        self.perform(json!([finger(
            "finger1",
            vec![
                contact(point),
                json!({ "type": "pointerDown", "button": 0 }),
                pause(HOLD),
                json!({ "type": "pointerUp", "button": 0 }),
                pause(80),
                json!({ "type": "pointerDown", "button": 0 }),
                pause(HOLD),
                json!({ "type": "pointerUp", "button": 0 }),
                pause(HOLD),
            ]
        )]))
    }

    /// Two fingers from `a.0`/`b.0` to `a.1`/`b.1`: a pinch or a spread.
    /// Both move in the same ticks, one step a frame.
    pub(super) fn touch_pinch(
        &mut self,
        a: (Point, Point),
        b: (Point, Point),
        duration: Duration,
    ) -> anyhow::Result<()> {
        let steps = u32::try_from((millis(duration) / STEP).max(1)).unwrap_or(1);
        let finger_actions = |(from, to_point): (Point, Point)| {
            let mut actions = vec![contact(from), json!({ "type": "pointerDown", "button": 0 })];
            actions.push(pause(STEP));
            for step in 1..=steps {
                actions.push(contact(lerp(from, to_point, step, steps)));
                actions.push(pause(STEP));
            }
            actions.push(pause(80));
            actions.push(json!({ "type": "pointerUp", "button": 0 }));
            actions.push(pause(HOLD));
            actions
        };
        self.perform(json!([
            finger("finger1", finger_actions(a)),
            finger("finger2", finger_actions(b)),
        ]))
    }
}

/// A pointer move to `point` in one step.
fn to(point: Point) -> Value {
    let (x, y) = point.wire();
    json!({ "type": "pointerMove", "x": x, "y": y, "duration": 0 })
}

/// A finger's move to `point` in one step, with a fingertip's contact area.
fn contact(point: Point) -> Value {
    let (x, y) = point.wire();
    json!({
        "type": "pointerMove", "x": x, "y": y, "duration": 0,
        "width": 20, "height": 20, "pressure": 0.5,
    })
}

fn finger(id: &str, actions: Vec<Value>) -> Value {
    json!({
        "type": "pointer", "id": id, "parameters": { "pointerType": "touch" },
        "actions": actions,
    })
}

fn pause(ms: u64) -> Value {
    json!({ "type": "pause", "duration": ms })
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn lerp(from: Point, to_point: Point, step: u32, steps: u32) -> Point {
    let t = f64::from(step) / f64::from(steps);
    Point {
        x: from.x + (to_point.x - from.x) * t,
        y: from.y + (to_point.y - from.y) * t,
    }
}

/// A drag's moves, one step and one frame's pause each.
fn steps_between(from: Point, to_point: Point, steps: u32) -> Vec<Value> {
    (1..=steps)
        .flat_map(|step| [to(lerp(from, to_point, step, steps)), pause(STEP)])
        .collect()
}
