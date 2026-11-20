//! OS lanes: what delivers real input to a target, watches for the user's own, and owns the thread
//! an OS API wants (ADR-0238 §5, §11).
//!
//! A [`Lane`] sends [`OsEvent`]s one at a time and releases what it pressed. It runs on a
//! [`LaneWorker`], a thread of its own, since the platforms' input and accessibility APIs bind to
//! the thread that set them up (UI Automation's apartment, AppKit's main thread). The rule that
//! ends a run when the user takes the desktop back is held here, at the seam, not by each lane:
//! [`send_guarded`] asks the lane's [`ActivityGuard`] before every event, and on the first foreign
//! one stops before the next event, releases everything and fails the run with
//! [`Error::InterruptedByUser`], which a report records as cannot-verify.
//!
//! What a lane cannot send at all (a key it has no code for, a shell hotkey, a device it lacks) is
//! refused by [`Lane::check`] before the guard is armed and before any event goes out. A lane also
//! reads what the OS itself shows of the app: its accessibility tree ([`Lane::read_tree`]), the
//! tree assistive technology reads, projected onto the wire's nodes, and the OS's own capture of
//! the window ([`Lane::capture_window`]).
//!
//! The lanes: [`windows`], [`macos`], [`x11`], [`wayland`] (its OS half with the `wayland`
//! feature) and `chrome` (feature `chrome`); both Linux lanes read the app through [`atspi`].
//! An input-method composition goes only to a lane that says it composes
//! ([`LaneCapabilities::compositions`]), a rule [`send_guarded`] holds.

pub mod atspi;
#[cfg(feature = "chrome")]
pub mod chrome;
pub mod macos;
pub mod wayland;
pub mod windows;
pub mod x11;

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use flui_foundation::panic::{FirstPanic, discard_payload};
use flui_protocol::input::PointerDevice;
use flui_protocol::session::{Isolation, Placement};
use flui_protocol::tree::Tree;

use crate::coordinates::WindowGeometry;
use crate::error::{Error, Result};
use crate::steps::OsEvent;

/// Which positions a lane reproduces exactly: what replay and golden holds compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PositionFidelity {
    /// Positions are reproduced to the device pixel.
    Exact,
    /// Only the order and kinds of events are reproduced: the platform resamples moves.
    OrderOnly,
}

/// The OS accessibility API a lane reads an app through ([`Lane::read_tree`]), which decides the
/// vocabulary of the native roles it reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AccessibilityApi {
    /// UI Automation, on Windows: native roles are control types.
    UiAutomation,
    /// AT-SPI, on Linux: native roles are AT-SPI role names.
    AtSpi,
    /// The macOS Accessibility API (AX): native roles are `AX` role strings.
    Ax,
}

/// What a lane can do.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LaneCapabilities {
    /// The pointer devices it sends.
    pub pointers: Vec<PointerDevice>,
    /// Whether it sends keys and text.
    pub keys: bool,
    /// Where its input lands.
    pub isolation: Isolation,
    /// What replay compares of its runs.
    pub fidelity: PositionFidelity,
    /// The accessibility API it reads the app through; `None` for a lane that reads none.
    pub accessibility: Option<AccessibilityApi>,
    /// Whether it sends an input method's composition ([`OsEvent::Composition`]) through the
    /// platform's own input-method path; every other lane leaves a composition to the app's own
    /// input path (`input.inject`).
    pub compositions: bool,
}

impl LaneCapabilities {
    /// A lane sending `pointers`, keys when `keys`, into `isolation`, reproducing positions as
    /// `fidelity` says. Whether it is guarded is [`Lane::guard`]'s to say, and only its.
    #[must_use]
    pub fn new(
        pointers: Vec<PointerDevice>,
        keys: bool,
        isolation: Isolation,
        fidelity: PositionFidelity,
    ) -> Self {
        Self {
            pointers,
            keys,
            isolation,
            fidelity,
            accessibility: None,
            compositions: false,
        }
    }

    /// The lane reads the app through `api`.
    #[must_use]
    pub fn accessibility(mut self, api: AccessibilityApi) -> Self {
        self.accessibility = Some(api);
        self
    }

    /// The lane sends input-method compositions.
    #[must_use]
    pub fn compositions(mut self) -> Self {
        self.compositions = true;
        self
    }
}

/// Input the lane did not send, seen while it ran.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ForeignInput {
    /// What it was, for people: "a mouse move", "a key press".
    pub description: String,
}

impl ForeignInput {
    /// Input described as `description`.
    #[must_use]
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }
}

/// Watches for input on the desktop that its lane did not inject: a physical mouse move, key,
/// touch or pen event. Best effort, not a security boundary: an OS hook misses input that does not
/// pass through it, and an event already sent cannot be recalled.
pub trait ActivityGuard: Send {
    /// Start watching; input before this is not reported.
    ///
    /// # Errors
    ///
    /// When the guard cannot watch, so the lane cannot tell the user's input from its own.
    fn arm(&mut self) -> Result<()>;

    /// The first foreign input since [`Self::arm`], if any came.
    fn foreign_input(&mut self) -> Option<ForeignInput>;

    /// Stop watching.
    fn disarm(&mut self);
}

/// Real input into a target: see the [module docs](self).
pub trait Lane: Send {
    /// The lane's name: `windows`, `x11`, `android`, …
    fn name(&self) -> &str;

    /// What it can do.
    fn capabilities(&self) -> LaneCapabilities;

    /// Its user-activity guard; `None` for a lane that cannot tell the user's input from its own,
    /// which runs only in an isolated session.
    fn guard(&mut self) -> Option<&mut dyn ActivityGuard>;

    /// Where the window `placement` describes sits on the lane's screen. The default reads the
    /// position the app reported.
    ///
    /// # Errors
    ///
    /// [`Error::NotSupported`] naming `window_position` when neither the app nor the lane knows it.
    fn locate_window(&mut self, placement: &Placement) -> Result<WindowGeometry> {
        let origin = placement.position.ok_or_else(|| {
            Error::not_supported(
                "window_position",
                format!(
                    "the app does not know where window {} is on screen, and the {} lane does not \
                     say",
                    placement.window,
                    self.name()
                ),
            )
        })?;
        Ok(WindowGeometry::new(origin, placement.size, placement.scale))
    }

    /// Whether the lane can send every one of `events`, asked before the first is sent: a key it
    /// has no code for, a device it lacks, a chord the OS shell takes before the app sees it. The
    /// default accepts every sequence.
    ///
    /// # Errors
    ///
    /// [`Error::NotSupported`] naming what the lane cannot send.
    fn check(&mut self, events: &[OsEvent]) -> Result<()> {
        let _ = events;
        Ok(())
    }

    /// Send one event; a pause waits.
    ///
    /// # Errors
    ///
    /// When the OS refused it.
    fn send(&mut self, event: &OsEvent) -> Result<()>;

    /// The OS accessibility tree of the window `placement` describes, projected onto the wire's
    /// nodes (ADR-0095 §4): what a screen reader reads of the app.
    ///
    /// # Errors
    ///
    /// [`Error::NotSupported`] naming `os_accessibility_tree` for a lane that reads none, which is
    /// the default.
    fn read_tree(&mut self, placement: &Placement) -> Result<Tree> {
        let _ = placement;
        Err(Error::not_supported(
            "os_accessibility_tree",
            format!("the {} lane reads no OS accessibility tree", self.name()),
        ))
    }

    /// Activate the element of the window `placement` describes whose OS automation id is
    /// `identifier` (an ADR-0240 identifier, as the adapter publishes it) through the OS
    /// accessibility API, as a screen reader's activate command does.
    ///
    /// # Errors
    ///
    /// [`Error::NotSupported`] naming `os_invoke` for a lane that acts through no accessibility
    /// API, which is the default; [`Error::NotFound`] when no element has the identifier.
    fn invoke(&mut self, placement: &Placement, identifier: &str) -> Result<()> {
        let _ = (placement, identifier);
        Err(Error::not_supported(
            "os_invoke",
            format!("the {} lane acts through no accessibility API", self.name()),
        ))
    }

    /// The OS's own capture of the window `placement` describes: its surface only, never the
    /// screen around it.
    ///
    /// # Errors
    ///
    /// [`Error::NotSupported`] naming `os_capture` for a lane that captures nothing, which is the
    /// default.
    fn capture_window(&mut self, placement: &Placement) -> Result<OsCapture> {
        let _ = placement;
        Err(Error::not_supported(
            "os_capture",
            format!("the {} lane captures no window", self.name()),
        ))
    }

    /// Release every key, button and contact the lane holds.
    ///
    /// # Errors
    ///
    /// When the OS refused a release.
    fn release_all(&mut self) -> Result<()>;
}

/// The OS's capture of a window's surface: tightly packed RGBA rows, top row first.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OsCapture {
    /// Width in device pixels.
    pub width: u32,
    /// Height in device pixels.
    pub height: u32,
    /// Four bytes a pixel, red first, `width * 4` bytes a row.
    pub rgba: Vec<u8>,
}

impl OsCapture {
    /// A `width` by `height` capture of `rgba`.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] when `rgba` is not four bytes for each of its pixels.
    pub fn try_new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self> {
        let expected = u64::from(width) * u64::from(height) * 4;
        if u64::try_from(rgba.len()).ok() != Some(expected) {
            return Err(Error::invalid_argument(format!(
                "a {width} x {height} capture holds {expected} bytes, not {}",
                rgba.len()
            )));
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    /// The pixel at (`x`, `y`) as red, green, blue and alpha; `None` past the capture.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let at = usize::try_from((u64::from(y) * u64::from(self.width) + u64::from(x)) * 4).ok()?;
        let bytes = self.rgba.get(at..at + 4)?;
        Some([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
}

impl fmt::Debug for OsCapture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OsCapture")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

/// Whether `lane` may send OS input: [`os_input::admit`](crate::os_input::admit) over where its
/// input lands and whether it has a guard, the one source of which is [`Lane::guard`].
///
/// # Errors
///
/// [`Error::NotSupported`] naming `os_input` for a lane whose input lands in process, and naming
/// `user_activity_guard` for one with no guard on the shared desktop.
pub fn admit(lane: &mut dyn Lane) -> Result<()> {
    let isolation = lane.capabilities().isolation;
    let guarded = lane.guard().is_some();
    crate::os_input::admit(isolation, guarded)
}

/// What a lane's release of everything came to, from the release of its keys and then of its
/// pointer, each answering [`Error::ReleaseFailed`] when it leaves anything held and its own error
/// only when it leaves nothing: the first failure, as one release failure naming what both still
/// hold when either does.
///
/// # Errors
///
/// [`Error::ReleaseFailed`] naming what both halves still hold, with the first failure's message,
/// when either half left anything held; the first half's error when neither did.
pub fn release_outcome(keys: Result<()>, pointer: Result<()>) -> Result<()> {
    let (first, second) = match (keys, pointer) {
        (Ok(()), outcome) | (outcome, Ok(())) => return outcome,
        (Err(first), Err(second)) => (first, second),
    };
    match (first, second) {
        (
            Error::ReleaseFailed {
                lane,
                mut held,
                message,
            },
            Error::ReleaseFailed { held: more, .. },
        ) => {
            held.extend(more);
            Err(Error::ReleaseFailed {
                lane,
                held,
                message,
            })
        }
        (first, Error::ReleaseFailed { lane, held, .. }) => Err(Error::ReleaseFailed {
            lane,
            held,
            message: first.to_string(),
        }),
        (first, _) => Err(first),
    }
}

/// Send `events` through `lane`, asking its guard before each: on foreign input, stop before the
/// next event, release everything and fail with [`Error::InterruptedByUser`]. Everything the lane
/// pressed is released on every exit, a failed send included; a release that leaves input held
/// fails with [`Error::ReleaseFailed`] naming it, the send's own failure kept in its message.
///
/// # Errors
///
/// As [`admit`] and [`Lane::check`], before any event, and [`Error::NotSupported`] naming
/// `os_ime_composition` for a composition a lane that does not compose was handed;
/// [`Error::InterruptedByUser`]; the lane's own failure.
pub fn send_guarded(lane: &mut dyn Lane, events: &[OsEvent]) -> Result<()> {
    admit(lane)?;
    let composes = lane.capabilities().compositions;
    if !composes
        && events
            .iter()
            .any(|event| matches!(event, OsEvent::Composition(_)))
    {
        return Err(Error::not_supported(
            "os_ime_composition",
            format!(
                "the {} lane sends no input-method composition; use the synthetic path",
                lane.name()
            ),
        ));
    }
    lane.check(events)?;
    let name = lane.name().to_owned();
    if let Some(guard) = lane.guard() {
        guard.arm()?;
    }
    let sent = send_each(lane, &name, events);
    if let Some(guard) = lane.guard() {
        guard.disarm();
    }
    let released = lane.release_all();
    match (sent, released) {
        // What stays held outranks why the send stopped, which its message keeps.
        (
            Err(first),
            Err(Error::ReleaseFailed {
                lane,
                held,
                message,
            }),
        ) => Err(Error::ReleaseFailed {
            lane,
            held,
            message: format!("{first}; then the release failed: {message}"),
        }),
        (sent, released) => sent.and(released),
    }
}

fn send_each(lane: &mut dyn Lane, name: &str, events: &[OsEvent]) -> Result<()> {
    for event in events {
        if let Some(foreign) = lane.guard().and_then(ActivityGuard::foreign_input) {
            return Err(Error::InterruptedByUser {
                lane: name.to_owned(),
                event: foreign.description,
            });
        }
        lane.send(event)?;
    }
    // Input that came during the last event still ends the run: it may have changed what the
    // last event did.
    if let Some(foreign) = lane.guard().and_then(ActivityGuard::foreign_input) {
        return Err(Error::InterruptedByUser {
            lane: name.to_owned(),
            event: foreign.description,
        });
    }
    Ok(())
}

/// Release what `lane` holds where no caller is left to retry (the worker ending, a job that
/// panicked): once, and once more if that failed. What a second failure leaves held is logged at
/// error level with the lane's error, which names it; nothing waits.
fn release_at_end(lane: &mut dyn Lane) {
    let Err(first) = lane.release_all() else {
        return;
    };
    if let Err(last) = lane.release_all() {
        tracing::error!(
            lane = lane.name(),
            first = %first,
            "input stays held after the lane let go of it: {last}"
        );
    }
}

/// A job for the lane's thread.
type Job = Box<dyn FnOnce(&mut dyn Lane) + Send>;

/// A lane on a thread of its own: jobs run there in order. A queued job is cancelled by one
/// compare-and-swap ([`Ticket::cancel`]); one already running finishes.
pub struct LaneWorker {
    jobs: Option<mpsc::Sender<(Arc<AtomicU8>, Job)>>,
    thread: Option<JoinHandle<()>>,
    name: String,
    capabilities: LaneCapabilities,
}

const QUEUED: u8 = 0;
const RUNNING: u8 = 1;
const CANCELLED: u8 = 2;

impl LaneWorker {
    /// Move `lane` onto a thread of its own.
    ///
    /// # Errors
    ///
    /// When the thread cannot be started.
    pub fn start(lane: impl Lane + 'static) -> Result<Self> {
        let name = lane.name().to_owned();
        let capabilities = lane.capabilities();
        let (jobs, inbox) = mpsc::channel::<(Arc<AtomicU8>, Job)>();
        let thread = thread::Builder::new()
            .name(format!("flui-driver-lane-{name}"))
            .spawn(move || {
                let mut lane = lane;
                while let Ok((state, job)) = inbox.recv() {
                    if state
                        .compare_exchange(QUEUED, RUNNING, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                    {
                        job(&mut lane);
                    }
                }
                // The worker is ending: nothing may stay pressed. A panic of the lane or of a log
                // subscriber here has no one left to be raised to, so it is destroyed under
                // containment as the collector drops.
                let mut ending = FirstPanic::new();
                ending.contain(|| release_at_end(&mut lane));
                drop(ending);
            })?;
        Ok(Self {
            jobs: Some(jobs),
            thread: Some(thread),
            name,
            capabilities,
        })
    }

    /// The lane's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What the lane can do.
    #[must_use]
    pub fn capabilities(&self) -> &LaneCapabilities {
        &self.capabilities
    }

    /// Queue `job` for the lane's thread.
    ///
    /// # Errors
    ///
    /// [`Error::Lane`] when the lane's thread has ended.
    pub fn submit<R: Send + 'static>(
        &self,
        job: impl FnOnce(&mut dyn Lane) -> R + Send + 'static,
    ) -> Result<Ticket<R>> {
        let (answer, answered) = mpsc::channel();
        let state = Arc::new(AtomicU8::new(QUEUED));
        let job: Job = Box::new(move |lane| {
            // A lane is code the driver does not own: its panic is resumed on the thread that
            // waits for the job, after the lane released what it held, and the worker serves on.
            // The release after it runs code the driver does not own either (the lane's, a log
            // subscriber's): a panic there is held behind the job's, never in its place.
            let mut first = FirstPanic::new();
            let value = first.contain(|| job(&mut *lane));
            if first.is_held() {
                first.contain(|| release_at_end(&mut *lane));
            }
            let outcome = value.ok_or_else(|| {
                first
                    .into_payload()
                    .expect("BUG: a job that returned no value held its panic")
            });
            if let Err(mpsc::SendError(Err(payload))) = answer.send(outcome) {
                // The caller stopped waiting: nobody resumes the job's panic.
                discard_payload(payload);
            }
        });
        let sent = self
            .jobs
            .as_ref()
            .is_some_and(|jobs| jobs.send((Arc::clone(&state), job)).is_ok());
        if !sent {
            return Err(self.ended());
        }
        Ok(Ticket {
            state,
            answered,
            lane: self.name.clone(),
        })
    }

    /// Run `job` on the lane's thread and wait for it.
    ///
    /// # Errors
    ///
    /// [`Error::Lane`] when the lane's thread has ended or panicked.
    pub fn run<R: Send + 'static>(
        &self,
        job: impl FnOnce(&mut dyn Lane) -> R + Send + 'static,
    ) -> Result<R> {
        self.submit(job)?.wait()
    }

    fn ended(&self) -> Error {
        Error::Lane {
            lane: self.name.clone(),
            unavailable: false,
            message: "the lane's thread has ended".into(),
        }
    }
}

impl Drop for LaneWorker {
    fn drop(&mut self) {
        drop(self.jobs.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl fmt::Debug for LaneWorker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LaneWorker")
            .field("name", &self.name)
            .field("capabilities", &self.capabilities)
            .finish_non_exhaustive()
    }
}

/// A job queued on a [`LaneWorker`].
#[must_use = "a ticket is waited for or cancelled"]
pub struct Ticket<R> {
    state: Arc<AtomicU8>,
    answered: mpsc::Receiver<std::thread::Result<R>>,
    lane: String,
}

impl<R> Ticket<R> {
    /// Cancel the job if it has not started; whether it was.
    pub fn cancel(&self) -> bool {
        self.state
            .compare_exchange(QUEUED, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Wait for the job's answer.
    ///
    /// # Errors
    ///
    /// [`Error::Lane`] when the job was cancelled, or the lane's thread ended before answering.
    ///
    /// # Panics
    ///
    /// Resumes the panic the job raised on the lane's thread.
    pub fn wait(self) -> Result<R> {
        match self.answered.recv() {
            Ok(Ok(answer)) => return Ok(answer),
            Ok(Err(payload)) => std::panic::resume_unwind(payload),
            Err(_) => {}
        }
        Err(Error::Lane {
            lane: self.lane,
            unavailable: false,
            message: if self.state.load(Ordering::Acquire) == CANCELLED {
                "the job was cancelled before it ran".into()
            } else {
                "the lane's thread ended before the job answered".into()
            },
        })
    }
}

impl<R> fmt::Debug for Ticket<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ticket")
            .field("lane", &self.lane)
            .field("state", &self.state.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}
