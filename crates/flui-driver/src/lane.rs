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
//! [`android`] drives a device or an emulator from any host, through `adb`, and [`ios_simulator`]
//! an iOS Simulator through `xcrun simctl`, which sends no input and reads the app through the
//! Mac's accessibility API on the Simulator's window. The macOS lane and the iOS Simulator lane
//! share that API's rules and, on macOS, its client (`ax`); no lane imports another. An input-method composition
//! goes only to a lane that says it composes ([`LaneCapabilities::compositions`]), a rule
//! [`send_guarded`] holds.

pub mod android;
pub mod atspi;
pub(crate) mod ax;
#[cfg(feature = "chrome")]
pub mod chrome;
pub mod ios_simulator;
pub mod macos;
pub mod pacing;
pub mod screen;
mod tool;
pub mod walk;
pub mod wayland;
pub mod windows;
pub mod x11;

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use flui_foundation::panic::{FirstPanic, discard_payload};
use flui_protocol::act::{Effect, EffectUnit};
use flui_protocol::input::PointerDevice;
use flui_protocol::session::{Isolation, Placement};
use flui_protocol::tree::Tree;

use crate::coordinates::WindowGeometry;
use crate::error::{Error, Result};
use crate::pixels::Image;
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
    /// Android's accessibility nodes (`AccessibilityNodeInfo`, the tree TalkBack reads) as
    /// UI Automator dumps them: native roles are class names.
    UiAutomator,
    /// The macOS Accessibility API (AX) on the iOS Simulator's window, under which the Simulator
    /// bridges the iOS app's UIKit elements: native roles are the `AX` role strings the bridge
    /// gives each element. Apple documents no part of the bridge, so it stands for VoiceOver on
    /// iOS only as far as runs confirm it: the roles it gives each trait are inferred, not
    /// measured, and whether it carries the app's identifier is unconfirmed (`docs/gaps.md`).
    SimulatorAx,
}

impl AccessibilityApi {
    /// Whether the lane reading the API finds the app's nodes by matching the OS tree to the
    /// app's own tree ([`Lane::read_tree`]'s `app`): Android's tree names no identifier the app
    /// sets, and the Simulator's bridge carries one no run has confirmed, so both are matched by
    /// role, name, state and bounds.
    #[must_use]
    pub fn matches_app_tree(self) -> bool {
        matches!(self, Self::UiAutomator | Self::SimulatorAx)
    }

    /// Whether the API performs an element's action, as a screen reader's activate command does
    /// ([`Lane::invoke`]): every one but UI Automator, which reads the tree and acts on nothing.
    #[must_use]
    pub fn activates(self) -> bool {
        !matches!(self, Self::UiAutomator)
    }
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

    /// Send one event; a pause waits. Said on its own, a failure is the lane's: what of a run went
    /// out before it is the run's to say ([`send_guarded`], through [`Self::send_counted`]).
    ///
    /// # Errors
    ///
    /// When the OS refused it.
    fn send(&mut self, event: &OsEvent) -> Result<()>;

    /// Send `event` as [`Self::send`] does, saying in `delivered` each input of it that reached
    /// the device: what [`send_guarded`] says of a run that fails part way. The default says an
    /// event sent whole is one input where it acts ([`Delivered::sent`]), which holds for a lane
    /// that sends each event as one input; a lane that sends an event as none or several (typed
    /// keystrokes, Android's commands) says each input itself ([`Delivered::input`],
    /// [`Delivered::command`]), so an event cut part way is partly sent.
    ///
    /// # Errors
    ///
    /// As [`Self::send`].
    fn send_counted(&mut self, event: &OsEvent, delivered: &mut Delivered) -> Result<()> {
        let sent = self.send(event);
        if sent.is_ok() {
            delivered.sent(event);
        }
        sent
    }

    /// The OS accessibility tree of the window `placement` describes, projected onto the wire's
    /// nodes (ADR-0095 §4): what a screen reader reads of the app. `app` is the window's tree as
    /// the app published it when the lane's API [matches the app's tree](AccessibilityApi::matches_app_tree)
    /// (Android's and the iOS Simulator's), and an empty tree for the others, which read without
    /// it.
    ///
    /// # Errors
    ///
    /// [`Error::NotSupported`] naming `os_accessibility_tree` for a lane that reads none, which is
    /// the default.
    fn read_tree(&mut self, placement: &Placement, app: &Tree) -> Result<Tree> {
        let _ = (placement, app);
        Err(Error::not_supported(
            "os_accessibility_tree",
            format!("the {} lane reads no OS accessibility tree", self.name()),
        ))
    }

    /// Activate the element of the window `placement` describes whose identifier is `identifier`
    /// (an ADR-0240 identifier, as the adapter publishes it) through the OS accessibility API, as
    /// a screen reader's activate command does. `app` is handed as [`Self::read_tree`] is handed
    /// it: a lane whose API [matches the app's tree](AccessibilityApi::matches_app_tree) finds the
    /// element as the app node of that identifier matched; the others find it by the OS automation
    /// id.
    ///
    /// # Errors
    ///
    /// [`Error::NotSupported`] naming `os_invoke` for a lane that acts through no accessibility
    /// API, which is the default, and for one that reads a tree but performs no action through it
    /// (Android's); [`Error::NotFound`] when no element has the identifier.
    fn invoke(&mut self, placement: &Placement, identifier: &str, app: &Tree) -> Result<()> {
        let _ = (placement, identifier, app);
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
    fn capture_window(&mut self, placement: &Placement) -> Result<Image> {
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
    /// When the OS refused a release: [`Error::ReleaseFailed`] naming everything still held, as
    /// [`release_outcome`] gathers it from the lane's parts.
    fn release_all(&mut self) -> Result<()>;
}

/// What releasing everything came to, from the releases of a lane's parts (its keys, its mouse,
/// its contacts) in the order they ran: everything each part still holds, named, with each
/// part's first failure, so one part's failure never hides what another left held or why.
///
/// # Errors
///
/// [`Error::ReleaseFailed`] naming what every part that answered one still holds; its message is
/// each failed part's first failure in the order the parts ran, joined by `; `: a part's
/// [`Error::ReleaseFailed`] gives its own message, any other error its whole text. When no part
/// holds anything, the first failure itself (a part answers [`Error::ReleaseFailed`] whenever it
/// still holds something, so only a failure that left nothing held, such as the Windows keys'
/// mask, arrives bare).
pub fn release_outcome(parts: impl IntoIterator<Item = Result<()>>) -> Result<()> {
    let mut failures: Vec<Error> = Vec::new();
    let mut stuck: Option<(String, Vec<String>)> = None;
    for mut error in parts.into_iter().filter_map(Result::err) {
        if let Error::ReleaseFailed { lane, held, .. } = &mut error {
            stuck
                .get_or_insert_with(|| (lane.clone(), Vec::new()))
                .1
                .append(held);
        }
        failures.push(error);
    }
    let Some((lane, held)) = stuck else {
        return failures.into_iter().next().map_or(Ok(()), Err);
    };
    let message = failures
        .into_iter()
        .map(|failure| match failure {
            Error::ReleaseFailed { message, .. } => message,
            other => other.to_string(),
        })
        .collect::<Vec<String>>()
        .join("; ");
    Err(Error::ReleaseFailed {
        lane,
        held,
        message,
        first: None,
    })
}

/// The end of a lane's release whose parts each ran contained in `panicked`: what
/// [`release_outcome`] makes of the parts that returned. When a part panicked, the panic is all
/// the caller gets, so what the others leave held is logged at error level first, never dropped.
///
/// # Panics
///
/// The first part's panic, resumed; discarded instead when the thread is already unwinding, as
/// [`FirstPanic::resume`] does.
pub(crate) fn release_settled<const N: usize>(
    mut panicked: FirstPanic,
    parts: [Option<Result<()>>; N],
) -> Result<()> {
    let outcome = release_outcome(parts.into_iter().flatten());
    if panicked.is_held()
        && let Err(error) = &outcome
    {
        // A subscriber is user code: its panic stays behind the part's.
        panicked.contain(|| {
            tracing::error!("input stays held after a release that panicked: {error}");
        });
    }
    panicked.resume();
    outcome
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
/// pressed is released on every exit, a failed send included. When the release leaves input
/// held, the run names it in [`Error::ReleaseFailed`]: the failure that ended the run is its
/// `first`, and the run's effect and verdict are that failure's ([`Error::effect`],
/// [`Error::is_cannot_verify`]).
///
/// Whatever ends the run once an input of it reached the device ([`Lane::send_counted`]), a
/// refusal, an interruption, a failed command or a release that fails after the last event, the
/// run answers as [`Error::PartlySent`] around it, and no retry, which would send that input
/// again. Its effect counts in events ([`Delivered`]): every event the lane took whole, a pause
/// waited out and a move included, and the failing event where part of it reached the device;
/// a lane that counts its commands names them in the effect's detail. A failure that names an
/// effect of its own and invites no retry (a command that timed out) stays as it is. A release
/// that fails after a run that ended for its own reason is that `ReleaseFailed`, the run's
/// failure, partly sent or not, its `first`.
///
/// # Errors
///
/// As [`admit`] and [`Lane::check`], before any event, and [`Error::NotSupported`] naming
/// `os_ime_composition` for a composition a lane that does not compose was handed;
/// [`Error::PartlySent`] around what ended a run some of which went out;
/// [`Error::ReleaseFailed`] naming what stays held, with the run's own failure as its `first`
/// when there was one; [`Error::InterruptedByUser`]; the lane's own failure.
pub fn send_guarded(lane: &mut dyn Lane, events: &[OsEvent]) -> Result<()> {
    admit(lane)?;
    let composes = lane.capabilities().compositions;
    if !composes
        && events
            .iter()
            .any(|event| matches!(event, OsEvent::Composition(_)))
    {
        return Err(no_composition(lane.name()));
    }
    lane.check(events)?;
    let name = lane.name().to_owned();
    if let Some(guard) = lane.guard() {
        guard.arm()?;
    }
    let (delivered, sent) = send_each(lane, &name, events);
    if let Some(guard) = lane.guard() {
        guard.disarm();
    }
    // Once an input of the run reached the device, whatever ends it says how much went out.
    let partly = |failure: Error| match delivered.effect(events.len()) {
        Some(effect) => failure.after_partly_sent(effect),
        None => failure,
    };
    let sent = sent.map_err(partly);
    let released = lane.release_all();
    match (sent, released) {
        // The error names what stays down whatever ended the run, and keeps that first failure,
        // which the verdict is read from: a run the person interrupted stays cannot-verify, since
        // it did not complete because of a person, while the key left down is named all the same.
        (
            Err(failure),
            Err(Error::ReleaseFailed {
                lane,
                held,
                message,
                ..
            }),
        ) => Err(Error::ReleaseFailed {
            lane,
            held,
            message: format!("{message}; the run had failed first: {failure}"),
            first: Some(Box::new(failure)),
        }),
        (Ok(()), Err(release)) => Err(partly(release)),
        (sent, released) => sent.and(released),
    }
}

/// What a lane that does not compose answers for an input-method composition: refused at
/// [`send_guarded`] before any event, and by the lane's own `send` when it is called directly.
pub(crate) fn no_composition(lane: &str) -> Error {
    Error::not_supported(
        "os_ime_composition",
        format!("the {lane} lane sends no input-method composition; use the synthetic path"),
    )
}

/// What a run put on the device, the one record [`send_guarded`]'s partly-sent answer is read
/// from. The run counts the events: every event the lane took whole, a pause waited out and a
/// move included, and the failing event where part of it reached the device. The lane says, in
/// [`Lane::send_counted`], each input of an event that reached the device: whether any did is
/// whether the run acted, and whether one of the failing event's did is whether that event went
/// out in part.
#[derive(Debug, Default)]
pub struct Delivered {
    /// The events counted as sent: each taken whole, and the failing one where part of it went
    /// out.
    events: u32,
    /// Whether an input of the event being sent reached the device.
    part: bool,
    /// Whether any input of the run reached the device.
    acted: bool,
    /// The commands a lane that counts them said, for the effect's detail.
    commands: u32,
}

impl Delivered {
    /// An input of the event being sent reached the device: a keystroke, a button, a click of
    /// the wheel. An event cut after one is partly sent.
    pub fn input(&mut self) {
        self.part = true;
        self.acted = true;
    }

    /// An input that is a command the lane counts (Android's `input` lines and pen reports): as
    /// [`Self::input`], and named in the effect's detail.
    pub fn command(&mut self) {
        self.input();
        self.commands = self.commands.saturating_add(1);
    }

    /// The lane sent `event` whole as one input: an input where the event acts
    /// ([`OsEvent::acts`]: not a move or a pause).
    pub fn sent(&mut self, event: &OsEvent) {
        if event.acts() {
            self.input();
        }
    }

    /// The run starts sending its next event.
    fn begin_event(&mut self) {
        self.part = false;
    }

    /// The event being sent was taken whole, or failed: counted when taken, or when part of it
    /// went out.
    fn end_event(&mut self, taken: bool) {
        if taken || self.part {
            self.events = self.events.saturating_add(1);
        }
    }

    /// What went out of a run of `total` events, when any input did: the events counted as sent
    /// of `total`, with the commands in the detail where the lane counted them.
    fn effect(&self, total: usize) -> Option<Effect> {
        if !self.acted {
            return None;
        }
        let effect = Effect::partial(
            self.events,
            u32::try_from(total).unwrap_or(u32::MAX),
            EffectUnit::Event,
        );
        Some(if self.commands > 0 {
            effect.detail(format!(
                "the device took {} command(s) of the run",
                self.commands
            ))
        } else {
            effect
        })
    }
}

/// Send `events` one at a time, asking the guard before each and after the last: what the
/// device took of them, and what ended the run.
fn send_each(lane: &mut dyn Lane, name: &str, events: &[OsEvent]) -> (Delivered, Result<()>) {
    let interrupted = |lane: &mut dyn Lane| {
        lane.guard()
            .and_then(ActivityGuard::foreign_input)
            .map(|foreign| Error::InterruptedByUser {
                lane: name.to_owned(),
                event: foreign.description,
            })
    };
    let mut delivered = Delivered::default();
    for event in events {
        if let Some(interruption) = interrupted(lane) {
            return (delivered, Err(interruption));
        }
        delivered.begin_event();
        let sent = lane.send_counted(event, &mut delivered);
        delivered.end_event(sent.is_ok());
        if let Err(failure) = sent {
            return (delivered, Err(failure));
        }
    }
    // Input that came during the last event still ends the run: it may have changed what the
    // last event did.
    match interrupted(lane) {
        Some(interruption) => (delivered, Err(interruption)),
        None => (delivered, Ok(())),
    }
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
