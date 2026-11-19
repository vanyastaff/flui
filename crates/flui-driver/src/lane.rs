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

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use flui_protocol::input::PointerDevice;
use flui_protocol::session::{Isolation, Placement};

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
        }
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

    /// Send one event; a pause waits.
    ///
    /// # Errors
    ///
    /// When the OS refused it.
    fn send(&mut self, event: &OsEvent) -> Result<()>;

    /// Release every key, button and contact the lane holds.
    ///
    /// # Errors
    ///
    /// When the OS refused a release.
    fn release_all(&mut self) -> Result<()>;
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

/// Send `events` through `lane`, asking its guard before each: on foreign input, stop before the
/// next event, release everything and fail with [`Error::InterruptedByUser`]. Everything the lane
/// pressed is released on every exit, a failed send included.
///
/// # Errors
///
/// As [`admit`], before any event; [`Error::InterruptedByUser`]; the lane's own failure.
pub fn send_guarded(lane: &mut dyn Lane, events: &[OsEvent]) -> Result<()> {
    admit(lane)?;
    let name = lane.name().to_owned();
    if let Some(guard) = lane.guard() {
        guard.arm()?;
    }
    let sent = send_each(lane, &name, events);
    if let Some(guard) = lane.guard() {
        guard.disarm();
    }
    let released = lane.release_all();
    sent.and(released)
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
                // The worker is ending: nothing may stay pressed.
                let _ = lane.release_all();
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
            #[expect(
                clippy::disallowed_methods,
                reason = "ADR-0287: a lane's panic is resumed on the thread waiting for the job"
            )]
            let outcome =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(&mut *lane)));
            if outcome.is_err() {
                let _ = lane.release_all();
            }
            let _ = answer.send(outcome);
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
