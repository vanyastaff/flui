//! Backend conformance (ADR-0215 §7): one table every input backend passes.
//!
//! A backend implements [`BackendUnderTest`]: it turns a backend-neutral
//! [`Step`] into its own native reading, and decodes that reading into the
//! [`HostInput`] it delivers. [`assert_backend_conforms`] then runs every row
//! of the table against it, statically dispatched, and fails with the name of
//! each row the backend's output broke:
//!
//! - **pointer assembly**: a contact is one pointer from down to up, a reused
//!   touch label is a new pointer, a second touch is additional, a named
//!   device is announced before its first contact;
//! - **clocks**: event times follow the reading clock and never go back, the
//!   coarse clock survives its wrap, precise readings are stamped from the
//!   counter (`QueryPerformanceCounter` on Win32), a sequence keeps the clock it
//!   started on, and a device whose reading once lacked the counter stays on
//!   its coarse stamp (ADR-0207 §2);
//! - **scroll grammar**: a two-finger pan is a phased `Scroll` with momentum
//!   phases, and `PanZoom` carries only scale and rotation about a focal point
//!   that moves (ADR-0217 §4);
//! - **invalid input**: an undecodable reading cancels its open contact as
//!   `CancelReason::InvalidInput`, and one that belongs to no contact emits
//!   nothing (ADR-0217 §3);
//! - **availability**: pen, precise scrolling and trackpad gestures are
//!   delivered exactly when the backend's [`InputCapabilities`] claim them
//!   (ADR-0213);
//! - **ownership**: no backend cancels a contact as `CancelReason::Claimed`,
//!   which only the gesture arena emits (ADR-0208); every row checks it on
//!   everything it decoded, and one row drives a platform cancel;
//! - **haptics**: a backend never queues a request, tier 1 reports a tier-1
//!   outcome, a stale source plays on the window's actuator, and the
//!   actuator's capabilities match what it plays (ADR-0210).
//!
//! - **drag-and-drop** (ADR-0211): a backend claims outgoing drags exactly when
//!   it has a drag source, and a drag that needs the OS on one without fails as
//!   unavailable (the X11 refusal); its source starts its sessions when the
//!   backend declares, which for iOS and the web is the start of the drag,
//!   never mid-drag, and counts a sibling presentation of the source's runtime
//!   (an identity-only offer advertising `within_app`).
//!
//! IME event order is a named [`RowGroup`] with no rows yet: the ticket that
//! builds it adds its rows to the same table.
//!
//! A row the backend cannot take part in (the clock rows of a backend with no
//! performance counter, the trackpad rows of a backend without trackpad
//! gestures) is reported as not applicable, never as passed.

mod drag_rows;
mod haptic_rows;
mod rows;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use flui_platform_api::input_capabilities::InputCapabilities;
use flui_platform_api::pointer::PointerKind;
/// The input a backend delivers, re-exported for implementors.
pub use flui_runtime::HostInput;

/// An input backend, as the conformance table drives it.
///
/// Statically dispatched, never `dyn`: [`Self::decode`] returns an iterator
/// borrowing the backend.
pub trait BackendUnderTest {
    /// The backend's own native reading: what its OS layer produces and its
    /// decoder consumes (`NativeReading` for the shared pointer assembly).
    type Reading;

    /// The input capabilities the backend reports for a new surface.
    fn capabilities(&self) -> InputCapabilities;

    /// The clocks the backend stamps readings on.
    fn clock(&self) -> ClockProfile;

    /// Start over: no open contact, no open gesture, no device seen. Every
    /// row starts with this.
    fn restart(&mut self);

    /// The native reading that performs `step`, or `None` when the backend has
    /// no path for it (a touch on a backend without touch, a two-finger pan on
    /// one without phased scrolling).
    fn reading(&mut self, step: Step) -> Option<Self::Reading>;

    /// Decode `reading` into the input the backend delivers.
    fn decode(&mut self, reading: Self::Reading)
    -> impl Iterator<Item = HostInput> + use<'_, Self>;

    /// The haptic actuator of a window the backend opens, for the haptics
    /// rows; `None`, the default, when the backend has none, and those rows
    /// are reported as not applicable.
    fn haptics(&mut self) -> Option<std::sync::Arc<dyn flui_platform_api::PlatformHaptics>> {
        None
    }

    /// The drag source of a window the backend opens, for the drag-and-drop
    /// rows; `None`, the default, when the backend starts no OS drag session.
    fn drag_source(
        &mut self,
    ) -> Option<std::sync::Arc<dyn flui_platform_api::drag::PlatformDragSource>> {
        None
    }

    /// When the backend documents that its OS drag session starts (ADR-0211
    /// §11); the default is when the pointer leaves the window. The source's
    /// own answer must agree.
    fn drag_start(&self) -> flui_platform_api::drag::OsDragStart {
        flui_platform_api::drag::OsDragStart::OnLeavingWindow
    }
}

/// The clocks a backend stamps its readings on (ADR-0207 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ClockProfile {
    /// The resolution of the stamp every reading carries.
    pub resolution: Duration,
    /// The resolution of the high-resolution counter some readings add, when
    /// the backend has one.
    pub counter: Option<Duration>,
    /// The period after which the coarse stamp wraps, when it does (Win32's
    /// 32-bit millisecond tick).
    pub wraps_after: Option<Duration>,
}

impl ClockProfile {
    /// A coarse clock of `resolution` with no counter and no wrap.
    #[must_use]
    pub const fn coarse(resolution: Duration) -> Self {
        Self {
            resolution,
            counter: None,
            wraps_after: None,
        }
    }

    /// This profile with a counter of `resolution`.
    #[must_use]
    pub const fn with_counter(self, resolution: Duration) -> Self {
        Self {
            counter: Some(resolution),
            ..self
        }
    }

    /// This profile whose coarse stamp wraps after `period`.
    #[must_use]
    pub const fn wrapping_after(self, period: Duration) -> Self {
        Self {
            wraps_after: Some(period),
            ..self
        }
    }
}

/// When a step happens, on the backend's OS clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Stamp {
    /// Time since the OS clock's origin (system uptime).
    pub uptime: Duration,
    /// Whether the reading also carries the high-resolution counter, read at
    /// the same instant. A backend without a counter ignores it.
    pub counter: bool,
}

impl Stamp {
    /// A reading stamped only on the coarse clock.
    #[must_use]
    pub const fn coarse(uptime: Duration) -> Self {
        Self {
            uptime,
            counter: false,
        }
    }

    /// A reading that also carries the counter.
    #[must_use]
    pub const fn precise(uptime: Duration) -> Self {
        Self {
            uptime,
            counter: true,
        }
    }
}

/// One thing the user (or the OS) does, in terms every backend shares.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Step {
    /// A contact of a pointer device.
    Contact(ContactStep),
    /// A reading of the contact the backend cannot decode (its position is not
    /// finite).
    Undecodable(ContactStep),
    /// A two-finger trackpad pan, or its momentum.
    Pan(PanStep),
    /// A trackpad pinch or rotation.
    Pinch(PinchStep),
}

/// What a contact does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContactAction {
    /// The primary button went down, or the contact touched.
    Press,
    /// The contact moved.
    Move,
    /// The primary button came up, or the contact lifted.
    Release,
    /// The OS ended the contact's sequence (a system gesture took it, a
    /// touch-cancel).
    Cancel,
}

/// A step of one contact.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct ContactStep {
    /// The backend's label for the contact; a label is reused after release.
    pub contact: u32,
    /// The backend's label for the device, when the OS names one.
    pub device: Option<u32>,
    /// The kind of device.
    pub kind: PointerKind,
    /// What it does.
    pub action: ContactAction,
    /// When.
    pub at: Stamp,
    /// Where, in device pixels at a scale of one.
    pub x: f64,
    /// Where, in device pixels at a scale of one.
    pub y: f64,
}

/// The phase of a native trackpad gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StepPhase {
    /// It began.
    Began,
    /// It changed.
    Changed,
    /// It ended.
    Ended,
    /// The OS cancelled it.
    Cancelled,
}

/// A two-finger pan step.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PanStep {
    /// The fingers' phase, or the momentum phase when `momentum`.
    pub phase: StepPhase,
    /// Whether this is the platform's momentum after the fingers lifted.
    pub momentum: bool,
    /// The scroll, in pixels, positive toward later content.
    pub dx: f64,
    /// The scroll, in pixels, positive toward later content.
    pub dy: f64,
    /// When.
    pub at: Stamp,
}

/// What one pinch reading changes.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum PinchChange {
    /// Multiply the scale by this factor.
    Scale(f64),
    /// Rotate by this many radians, clockwise.
    Rotation(f64),
}

/// A trackpad pinch or rotation step.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PinchStep {
    /// The component's phase.
    pub phase: StepPhase,
    /// The change it makes.
    pub change: PinchChange,
    /// When.
    pub at: Stamp,
    /// The focal point, in device pixels at a scale of one.
    pub x: f64,
    /// The focal point, in device pixels at a scale of one.
    pub y: f64,
}

/// The families of rows. The ones without rows yet are the extension points
/// their features fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RowGroup {
    /// Contact identity, roles and device announcement.
    PointerAssembly,
    /// Clock choice, order and wrap.
    Clocks,
    /// The `Scroll` and `PanZoom` grammar.
    ScrollGrammar,
    /// Readings the backend cannot decode.
    InvalidInput,
    /// What the backend's capabilities claim against what it delivers.
    Availability,
    /// Ownership the backend must leave to the gesture arena.
    Ownership,
    /// Drag-and-drop sessions (ADR-0211): the drag source and when it starts.
    DragAndDrop,
    /// Haptic requests and outcomes (ADR-0210).
    Haptics,
    /// IME event order; no rows yet.
    Ime,
}

/// What one row concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowOutcome {
    /// The backend's output held the row.
    Held,
    /// The row does not apply to this backend, and why.
    NotApplicable(&'static str),
    /// The backend's output broke the row: what did not hold.
    Failed(String),
}

/// The outcome of every row for one backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceReport {
    /// Each row's name, group and outcome, in running order.
    pub rows: Vec<(&'static str, RowGroup, RowOutcome)>,
}

impl ConformanceReport {
    /// The rows that failed, with what did not hold.
    pub fn failures(&self) -> impl Iterator<Item = (&'static str, &str)> {
        self.rows
            .iter()
            .filter_map(|(name, _, outcome)| match outcome {
                RowOutcome::Failed(message) => Some((*name, message.as_str())),
                RowOutcome::Held | RowOutcome::NotApplicable(_) => None,
            })
    }

    /// The outcome of the row `name`.
    #[must_use]
    pub fn outcome(&self, name: &str) -> Option<&RowOutcome> {
        self.rows
            .iter()
            .find(|(row, _, _)| *row == name)
            .map(|(_, _, outcome)| outcome)
    }
}

/// Run every row against `backend`, each after [`BackendUnderTest::restart`].
/// A panic inside a row, the backend's or the table's, fails that row.
pub fn run_backend_conformance<B: BackendUnderTest>(backend: &mut B) -> ConformanceReport {
    let rows = rows::rows::<B>()
        .into_iter()
        .map(|row| {
            backend.restart();
            let outcome = match catch_unwind(AssertUnwindSafe(|| (row.check)(backend))) {
                Ok(Ok(outcome)) => outcome,
                Ok(Err(message)) => RowOutcome::Failed(message),
                Err(payload) => {
                    let text = payload
                        .downcast_ref::<&str>()
                        .map(|text| (*text).to_owned())
                        .or_else(|| payload.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "a non-string panic payload".to_owned());
                    // The table owns the caught opaque payload; an aggregate's
                    // Drop may panic before any catch regains control.
                    flui_foundation::panic::retain_opaque_payload(payload);
                    RowOutcome::Failed(format!("panicked: {text}"))
                }
            };
            (row.name, row.group, outcome)
        })
        .collect();
    ConformanceReport { rows }
}

/// [`run_backend_conformance`], panicking with every failed row.
///
/// # Panics
///
/// When a row fails.
#[track_caller]
pub fn assert_backend_conforms<B: BackendUnderTest>(backend: &mut B) {
    let report = run_backend_conformance(backend);
    let failures: Vec<String> = report
        .failures()
        .map(|(name, message)| format!("  - {name}: {message}"))
        .collect();
    assert!(
        failures.is_empty(),
        "the backend does not conform:\n{}",
        failures.join("\n")
    );
}
