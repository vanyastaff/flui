//! The Chrome lane: a web build of the app in headless Chrome, driven through ChromeDriver's
//! WebDriver BiDi session (ADR-0238 §5).
//!
//! - **Input.** BiDi `input.performActions`, which Chrome delivers as trusted input through its
//!   own input pipeline (the page sees the events a person's devices make, with user activation):
//!   the mouse, touch and pen pointers, the wheel and keys, one command per event ([`actions`]), so
//!   every move is a single step and every pause the lane's own wait. Text is typed a grapheme
//!   cluster at a time ([`keys`]). An input method's composition goes through Chrome's DevTools
//!   input domain over ChromeDriver's `goog:cdp.sendCommand` ([`ime`]), the path an OS input
//!   method's text takes; it is the one lane that sends compositions.
//! - **Held input.** A key, a button or a contact is recorded as held once the browser answered
//!   the command that pressed it, and an open composition once the browser showed it; a command
//!   the browser did not answer in time (a text's included) leaves what it pressed recorded as
//!   possibly held. The release of everything cancels an open composition and sends
//!   `input.releaseActions` whatever the lane recorded, since the browser's own input state is the
//!   truth; it runs on every exit, and a release the browser refuses fails with
//!   [`Error::ReleaseFailed`] naming what stays held.
//! - **Where.** The lane's screen is the page's viewport in device pixels; the window is the
//!   canvas `#flui-canvas` the web host draws the first window into, located again before every
//!   sequence, and refused when its size or the page's device pixel ratio is not the window's.
//! - **Capture.** `browsingContext.captureScreenshot` clipped to the canvas, its base64 bounded and
//!   its PNG decoded by the driver's one decoder ([`screenshot`]), a [`crate::pixels::Image`].
//! - **Isolation.** The browser is always headless (`chromedriver::chrome_args`), so the lane is
//!   isolated and needs no user-activity guard.
//! - **Reading.** None: the canvas exposes no DOM accessibility tree for FLUI's nodes to a client
//!   of the browser, so `os_accessibility_tree` and `os_invoke` stay not supported.
//! - **Replay fidelity.** Positions are exact: one step a move, never interpolated.
//! - **Process.** The lane's session (`bidi`) owns the browser it runs in: the page's server
//!   (`page`) and the ChromeDriver (`chromedriver`) end once its last handle went.
//!
//! A launch of a web build (`Artifact::WasmBundle`) serves the build, starts the ChromeDriver its
//! [`ChromeDriverSource`] gives (by default none it would have to download), opens a headless
//! session at the launch's viewport, installs the page's launch object (the agent's endpoint and
//! token) with a preload script before the app's module runs, and connects to the page's export
//! (`connection::bidi`): no URL carries the token.

pub mod actions;
pub(crate) mod bidi;
pub(crate) mod chromedriver;
pub mod ime;
pub mod keys;
pub(crate) mod page;
pub mod screenshot;

pub use self::bidi::{MAX_MESSAGE, MAX_PENDING, PAGE_BYTES, PAGE_ROOM};

use std::path::{Path, PathBuf};
use std::time::Duration;

use flui_protocol::act::{Effect, EffectKind};
use flui_protocol::geometry::LogicalPoint;
use flui_protocol::input::{ImeAction, PointerButton, PointerDevice};
use flui_protocol::launch::{ENDPOINT_EXTRA, PAGE_LAUNCH_GLOBAL, TOKEN_EXTRA};
use flui_protocol::session::{Isolation, Placement};
use serde_json::{Value, json};

use self::bidi::{COMMAND_TIMEOUT, Failure, PageLines, Session};
use self::chromedriver::{ChromeDriver, Fetcher};
use self::page::PageServer;
use crate::coordinates::WindowGeometry;

use crate::error::{Error, Result};
use crate::lane::{ActivityGuard, Lane, LaneCapabilities, PositionFidelity, release_outcome};
use crate::steps::OsEvent;

/// The lane's name.
pub const NAME: &str = "chrome";

/// The id the web host gives the canvas of the page's first window.
pub const CANVAS_ID: &str = "flui-canvas";

/// How far the canvas's CSS size may be from the window's logical size: the page lays the canvas
/// out in fractions of a pixel.
const SIZE_TOLERANCE: f64 = 1.0;

/// How far the page's device pixel ratio may be from the window's scale.
const SCALE_TOLERANCE: f64 = 1e-3;

/// How long a navigation may take to load the page.
const LOAD_TIMEOUT: Duration = Duration::from_secs(60);

/// Reads the canvas's rect in the viewport and the device pixel ratio; `null` without the canvas.
const LOCATE: &str = "(id) => {
    const canvas = document.getElementById(id);
    if (canvas === null) { return null; }
    const rect = canvas.getBoundingClientRect();
    return [rect.left, rect.top, rect.width, rect.height, globalThis.devicePixelRatio];
}";

/// Where the canvas is: its top-left and size in CSS pixels, and the device pixel ratio.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Canvas {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
    ratio: f64,
}

/// What the lane holds in the page, by name, each recorded once the browser answered.
#[derive(Debug, Default)]
struct Held {
    /// Keys down, in the protocol's spelling.
    keys: Vec<String>,
    /// Buttons and contacts down: the pointer's source id and the button.
    buttons: Vec<(String, PointerButton)>,
    /// A composition shown and not ended.
    composing: bool,
    /// What a command the browser did not answer in time may have pressed.
    unconfirmed: Vec<String>,
}

/// How a release that failed names an open composition.
const COMPOSITION: &str = "an input-method composition";

impl Held {
    /// The keys, buttons and contacts held, by name, those an unanswered command may have pressed
    /// included: what `input.releaseActions` lets go of.
    fn pressed(&self) -> Vec<String> {
        let mut names: Vec<String> = self.keys.clone();
        names.extend(
            self.buttons
                .iter()
                .map(|(pointer, button)| format!("{pointer} {}", button.name())),
        );
        names.extend(
            self.unconfirmed
                .iter()
                .map(|what| format!("possibly {what}")),
        );
        names
    }

    fn clear_pressed(&mut self) {
        self.keys.clear();
        self.buttons.clear();
        self.unconfirmed.clear();
    }

    /// Record what `event`, answered by the browser, pressed or released.
    fn record(&mut self, event: &OsEvent) {
        match event {
            OsEvent::KeyDown(key) => self.keys.push(key.clone()),
            OsEvent::KeyUp(key) => {
                if let Some(at) = self.keys.iter().position(|held| held == key) {
                    self.keys.remove(at);
                }
            }
            OsEvent::PointerDown {
                pointer, button, ..
            } => {
                self.buttons.push((pointer.clone(), *button));
            }
            OsEvent::PointerUp {
                pointer, button, ..
            } => {
                if let Some(at) = self
                    .buttons
                    .iter()
                    .position(|(held, pressed)| held == pointer && pressed == button)
                {
                    self.buttons.remove(at);
                }
            }
            OsEvent::Composition(action) => self.composing = ime::composing_after(action),
            _ => {}
        }
    }

    /// Record what `event`, which the browser did not answer in time, may have pressed or opened:
    /// a key, a button or the keys typing a text as possibly held (a text's command presses and
    /// releases each key, and may have stopped between the two), a composition as open, which a
    /// release cancels either way.
    fn record_unanswered(&mut self, event: &OsEvent) {
        match event {
            OsEvent::KeyDown(key) => self.unconfirmed.push(key.clone()),
            OsEvent::Text(text) => self.unconfirmed.push(format!("a key typing {text:?}")),
            OsEvent::PointerDown {
                pointer, button, ..
            } => self
                .unconfirmed
                .push(format!("{pointer} {}", button.name())),
            OsEvent::Composition(ImeAction::SetComposition { .. }) => self.composing = true,
            _ => {}
        }
    }
}

/// The Chrome lane on one tab of a BiDi session: see the [module docs](self).
pub struct ChromeLane {
    session: Session,
    canvas: Option<Canvas>,
    held: Held,
    /// The DevTools session of the tab, once asked for.
    devtools: Option<String>,
}

impl std::fmt::Debug for ChromeLane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChromeLane")
            .field("context", &self.session.context())
            .field("canvas", &self.canvas)
            .field("held", &self.held)
            .finish_non_exhaustive()
    }
}

impl ChromeLane {
    pub(crate) fn new(session: Session) -> Self {
        Self {
            session,
            canvas: None,
            held: Held::default(),
            devtools: None,
        }
    }

    fn locate(&mut self) -> Result<Canvas> {
        let located = self.session.call_function(
            LOCATE,
            None,
            vec![json!({ "type": "string", "value": CANVAS_ID })],
            "none",
        )?;
        let numbers: Option<Vec<f64>> =
            located
                .get("value")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|value| value.get("value").and_then(Value::as_f64))
                        .collect()
                });
        let canvas = match numbers.as_deref() {
            Some(&[left, top, width, height, ratio])
                if [left, top, width, height, ratio]
                    .iter()
                    .all(|n| n.is_finite())
                    && ratio > 0.0 =>
            {
                Canvas {
                    left,
                    top,
                    width,
                    height,
                    ratio,
                }
            }
            _ if located.get("type").and_then(Value::as_str) == Some("null") => {
                return Err(Error::not_supported(
                    "window_position",
                    format!("the page shows no canvas #{CANVAS_ID} to place the window on"),
                ));
            }
            _ => {
                return Err(Error::Lane {
                    lane: NAME.to_owned(),
                    unavailable: false,
                    message: format!("the page placed its canvas as {located}"),
                });
            }
        };
        self.canvas = Some(canvas);
        Ok(canvas)
    }

    /// The canvas located last; the lane locates it before every sequence.
    fn canvas(&self) -> Result<Canvas> {
        self.canvas.ok_or_else(|| Error::Lane {
            lane: NAME.to_owned(),
            unavailable: false,
            message: "the window was not located before its input".to_owned(),
        })
    }

    fn compose(&mut self, action: &ImeAction) -> Result<()> {
        let (method, params) = ime::devtools_command(action)?;
        let session = self.devtools_session()?;
        self.session
            .command(
                "goog:cdp.sendCommand",
                json!({ "method": method, "params": params, "session": session }),
                COMMAND_TIMEOUT,
            )
            .map(drop)
            .map_err(|failure| acting(failure, method))
    }

    /// The tab's DevTools session, asked of ChromeDriver once.
    fn devtools_session(&mut self) -> Result<String> {
        if let Some(session) = &self.devtools {
            return Ok(session.clone());
        }
        let answer = self
            .session
            .command(
                "goog:cdp.getSession",
                json!({ "context": self.session.context() }),
                COMMAND_TIMEOUT,
            )
            .map_err(|failure| failure.into_error("goog:cdp.getSession"))?;
        let session = answer
            .get("session")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Lane {
                lane: NAME.to_owned(),
                unavailable: false,
                message: "ChromeDriver named no DevTools session for the tab".to_owned(),
            })?
            .to_owned();
        self.devtools = Some(session.clone());
        Ok(session)
    }
}

/// An acting command's failure: one the browser did not answer may have run.
fn acting(failure: Failure, method: &str) -> Error {
    match failure {
        Failure::Timeout => Error::Timeout {
            effect: Some(Effect::new(EffectKind::MayHaveRun).detail(format!(
                "the browser did not answer `{method}` in time; it may have taken the input"
            ))),
            message: format!("the browser did not answer `{method}` in time"),
            retry: flui_protocol::error::Retry::Never,
        },
        other => other.into_error(method),
    }
}

impl Lane for ChromeLane {
    fn name(&self) -> &str {
        NAME
    }

    fn capabilities(&self) -> LaneCapabilities {
        LaneCapabilities::new(
            vec![
                PointerDevice::Mouse,
                PointerDevice::Touch,
                PointerDevice::Pen,
            ],
            true,
            Isolation::HeadlessBrowser,
            PositionFidelity::Exact,
        )
        .compositions()
    }

    fn guard(&mut self) -> Option<&mut dyn ActivityGuard> {
        None
    }

    fn locate_window(&mut self, placement: &Placement) -> Result<WindowGeometry> {
        let canvas = self.locate()?;
        if (canvas.ratio - placement.scale).abs() > SCALE_TOLERANCE {
            return Err(Error::invalid_argument(format!(
                "the page's device pixel ratio is {}, and window {} is at scale {}",
                canvas.ratio, placement.window, placement.scale
            )));
        }
        let same_size = (canvas.width - placement.size.width).abs() <= SIZE_TOLERANCE
            && (canvas.height - placement.size.height).abs() <= SIZE_TOLERANCE;
        if !same_size {
            return Err(Error::invalid_argument(format!(
                "the canvas #{CANVAS_ID} is {} x {} CSS pixels, and window {} is {} x {}",
                canvas.width,
                canvas.height,
                placement.window,
                placement.size.width,
                placement.size.height
            )));
        }
        Ok(WindowGeometry::new(
            LogicalPoint::new(canvas.left, canvas.top),
            placement.size,
            placement.scale,
        ))
    }

    fn check(&mut self, events: &[OsEvent]) -> Result<()> {
        for event in events {
            match event {
                OsEvent::Composition(action) => drop(ime::devtools_command(action)?),
                // Every other event is checked by building its command, which sends nothing.
                other => drop(actions::perform_actions(other, 1.0)?),
            }
        }
        Ok(())
    }

    fn send(&mut self, event: &OsEvent) -> Result<()> {
        let sent = match event {
            OsEvent::Pause(duration) => {
                std::thread::sleep(*duration);
                return Ok(());
            }
            OsEvent::Composition(action) => self.compose(action),
            other => {
                let Some(actions) = actions::perform_actions(other, self.canvas()?.ratio)? else {
                    return Ok(());
                };
                self.session
                    .command(
                        "input.performActions",
                        json!({ "context": self.session.context(), "actions": actions }),
                        COMMAND_TIMEOUT,
                    )
                    .map(drop)
                    .map_err(|failure| acting(failure, "input.performActions"))
            }
        };
        match &sent {
            Ok(()) => self.held.record(event),
            Err(Error::Timeout { .. }) => self.held.record_unanswered(event),
            Err(_) => {}
        }
        sent
    }

    fn capture_window(&mut self, placement: &Placement) -> Result<crate::pixels::Image> {
        self.locate_window(placement)?;
        let canvas = self.canvas()?;
        let shot = self
            .session
            .command(
                "browsingContext.captureScreenshot",
                json!({
                    "context": self.session.context(),
                    "origin": "viewport",
                    "format": { "type": "image/png" },
                    "clip": {
                        "type": "box",
                        "x": canvas.left,
                        "y": canvas.top,
                        "width": canvas.width,
                        "height": canvas.height,
                    },
                }),
                COMMAND_TIMEOUT,
            )
            .map_err(|failure| failure.into_error("browsingContext.captureScreenshot"))?;
        let data = shot
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Lane {
                lane: NAME.to_owned(),
                unavailable: false,
                message: "the browser's screenshot carries no data".to_owned(),
            })?;
        screenshot::decode(data)
    }

    fn release_all(&mut self) -> Result<()> {
        // Two parts, each run whatever the other came to: the composition, then the keys,
        // buttons and contacts.
        let composition = if self.held.composing {
            match self.compose(&ImeAction::Cancel) {
                Ok(()) => {
                    self.held.composing = false;
                    Ok(())
                }
                Err(error) => Err(Error::ReleaseFailed {
                    lane: NAME.to_owned(),
                    held: vec![COMPOSITION.to_owned()],
                    message: error.to_string(),
                    first: None,
                }),
            }
        } else {
            Ok(())
        };
        // Sent whatever the lane recorded: the browser's own input state is the truth, and a
        // command that stopped part-way (a text's keys) may have left in it what no answer
        // reported. The release is idempotent.
        let pressed = self.held.pressed();
        let actions = match self.session.command(
            "input.releaseActions",
            json!({ "context": self.session.context() }),
            COMMAND_TIMEOUT,
        ) {
            Ok(_) => {
                self.held.clear_pressed();
                Ok(())
            }
            // A session that ended holds nothing of the lane's any more.
            Err(Failure::Gone(_)) if pressed.is_empty() => Ok(()),
            Err(failure) if pressed.is_empty() => Err(failure.into_error("input.releaseActions")),
            Err(failure) => Err(Error::ReleaseFailed {
                lane: NAME.to_owned(),
                held: pressed,
                message: failure.into_error("input.releaseActions").to_string(),
                first: None,
            }),
        };
        release_outcome([composition, actions])
    }
}

/// Where a launch's ChromeDriver comes from. The default reaches no network.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ChromeDriverSource {
    /// The one the environment names (the `CHROMEDRIVER` file, or the directory
    /// `CHROMEWEBDRIVER` names), else the one a fetch kept for the installed Chrome's version,
    /// its SHA-256 checked against its record before it runs. Without either, the lane is one the
    /// host cannot run, with the install hint.
    #[default]
    Installed,
    /// This file, whatever the environment names.
    Path(PathBuf),
    /// As [`Self::Installed`], and on a miss the ChromeDriver fetched over HTTPS from Chrome for
    /// Testing into the cache: the installed Chrome's version when it is published, else the newest
    /// release of the same build (`major.minor.build`), kept under the installed version, with the
    /// binary's SHA-256 and the version fetched recorded beside it.
    FetchAllowed,
}

/// A web build at `bundle` launched in headless Chrome: served on the loopback, opened in a
/// session at `viewport` with the ChromeDriver `source` gives, its launch object holding `token`
/// installed before its module runs. The session owns the server and the ChromeDriver.
///
/// # Errors
///
/// [`Error::Lane`] marked as a host that cannot run the lane when it has no Chrome or no
/// ChromeDriver for it; the failure of a step.
pub(crate) fn launch(
    bundle: &Path,
    viewport: (u32, u32),
    token: &str,
    source: &ChromeDriverSource,
) -> Result<(Session, PageLines)> {
    let page = PageServer::serve(bundle)?;
    let chrome = chromedriver::installed()?.ok_or_else(|| Error::Lane {
        lane: NAME.to_owned(),
        unavailable: true,
        message: "no Chrome on this host; set FLUI_CHROME to its executable".to_owned(),
    })?;
    let cache = chromedriver::cache_dir().ok_or_else(|| Error::Lane {
        lane: NAME.to_owned(),
        unavailable: true,
        message: "no cache directory to keep ChromeDriver in; set CHROMEDRIVER".to_owned(),
    })?;
    let fetcher = Fetcher::chrome_for_testing();
    let (named, fetch) = match source {
        ChromeDriverSource::Path(path) => (Some(path.clone()), None),
        ChromeDriverSource::FetchAllowed => (chromedriver::named_by_environment(), Some(&fetcher)),
        ChromeDriverSource::Installed => (chromedriver::named_by_environment(), None),
    };
    let path = chromedriver::resolve(&chrome, &cache, named, fetch)?;
    let mut driver = ChromeDriver::start(&path)?;
    let url = driver.open_session(Some(&chrome), viewport)?;
    let url_of_page = page.url();
    let owned: Box<dyn std::any::Any + Send + Sync> = Box::new((driver, page));
    let (session, lines) = Session::connect(&url, Some(owned))?;

    let (width, height) = viewport;
    session
        .command(
            "browsingContext.setViewport",
            json!({
                "context": session.context(),
                "viewport": { "width": width, "height": height },
                "devicePixelRatio": 1,
            }),
            COMMAND_TIMEOUT,
        )
        .map_err(|failure| failure.into_error("browsingContext.setViewport"))?;
    session
        .command(
            "script.addPreloadScript",
            json!({ "functionDeclaration": preload(token), "contexts": [session.context()] }),
            COMMAND_TIMEOUT,
        )
        .map_err(|failure| failure.into_error("script.addPreloadScript"))?;
    session
        .command(
            "browsingContext.navigate",
            json!({ "context": session.context(), "url": url_of_page, "wait": "complete" }),
            LOAD_TIMEOUT,
        )
        .map_err(|failure| failure.into_error("browsingContext.navigate"))?;
    Ok((session, lines))
}

/// The preload that installs the page's launch object before the app's module runs: the agent's
/// endpoint, the export's global, and `token`. Only a top-level document gets it: a frame inside
/// the page is no app this session started.
#[must_use]
pub fn preload(token: &str) -> String {
    let object = json!({
        ENDPOINT_EXTRA: crate::connection::bidi::PAGE_EXPORT,
        TOKEN_EXTRA: token,
    });
    format!(
        "() => {{ if (globalThis.parent !== globalThis) {{ return; }} globalThis[{global}] = \
         {object}; }}",
        global = Value::from(PAGE_LAUNCH_GLOBAL),
    )
}
