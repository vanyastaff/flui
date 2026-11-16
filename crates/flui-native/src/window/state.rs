//! The window state every backend keeps: the snapshot it last published and
//! the rule for when it is read from the OS again (ADR-0237), built like
//! the display source (`display::source`), and the one rule that decides a
//! window's mode from what its window system says of it (`mode_of`).

#![forbid(unsafe_code)]

use std::sync::atomic::{AtomicU64, Ordering};

use flui_foundation::geometry::WindowMetrics;
use flui_platform::{
    InvalidGeometry, PublicationMint, WindowAppearance, WindowCapabilities, WindowExecutionState,
    WindowExtent, WindowGeometry, WindowInsets, WindowInteraction, WindowMode, WindowRevision, WindowState,
};
use parking_lot::Mutex;

use crate::owner::window_delivery::{Publication, WindowDelivery};

/// The facts a window reports beyond its geometry, mode and flags, as each
/// host reads them its own way, put onto a sample in one place
/// ([`Self::onto`]): a fact the snapshot gains is added here, and every host
/// that builds this value is then told by the compiler to read it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReportedFacts {
    /// The drawable area and scale; `None` for a report that is not one.
    pub(crate) metrics: Option<WindowMetrics>,
    /// The user's theme as applied to the window (ADR-0222).
    pub(crate) appearance: WindowAppearance,
    /// The safe area, the soft keyboard and the system-gesture edges, in
    /// logical pixels (ADR-0277).
    pub(crate) insets: WindowInsets,
    /// Whether the platform lets the window run its UI (ADR-0035).
    pub(crate) execution: WindowExecutionState,
    /// Whether the window system says none of the window reaches the screen,
    /// as it says it: the snapshot keeps it only for a shown, unminimized
    /// window (`WindowState::successor`).
    pub(crate) occluded: bool,
    /// What the last hover report said.
    pub(crate) hovered: bool,
    /// Whether the window is the active window of its desktop; its focus where
    /// the window system has no notion apart from it.
    pub(crate) active: bool,
    /// What the window honours now.
    pub(crate) capabilities: WindowCapabilities,
    /// Whether the user is placing the window through the window system, as
    /// the host knows it; [`WindowInteraction::Unknown`] where it cannot tell
    /// (ADR-0343).
    pub(crate) interaction: WindowInteraction,
}

impl ReportedFacts {
    /// `state` with these facts.
    pub(crate) fn onto(self, state: WindowState) -> WindowState {
        state
            .with_metrics(self.metrics)
            .with_appearance(self.appearance)
            .with_insets(self.insets)
            .with_execution(self.execution)
            .with_occluded(self.occluded)
            .with_hovered(self.hovered)
            .with_active(self.active)
            .with_capabilities(self.capabilities)
            .with_interaction(self.interaction)
    }
}

/// The rectangles one read of a window found: what a snapshot's geometry is
/// built from, each valid by its type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SampledGeometry {
    /// The outer rectangle, frame included.
    pub(crate) outer: WindowGeometry,
    /// The content area's extent.
    pub(crate) content: WindowExtent,
    /// The rectangle the window returns to from a mode other than normal.
    pub(crate) normal: Option<WindowGeometry>,
}

impl SampledGeometry {
    /// No rectangle: what a window holds before a read describes one.
    const EMPTY: Self = Self {
        outer: WindowGeometry::EMPTY,
        content: WindowExtent::ZERO,
        normal: None,
    };

    /// A snapshot with this geometry and every other fact at its default.
    fn state(self) -> WindowState {
        WindowState::new(self.outer, self.content).with_normal_geometry(self.normal)
    }
}

/// One read of a window: every fact the host read, and whether the
/// rectangles it read describe a window.
///
/// A host builds it with [`Self::read`], handing the geometry it read as a
/// `Result` and the other facts apart from it, so a geometry the window
/// system reported as numbers no window has (a size over a scale of zero, a
/// NaN) costs the sample its geometry and nothing else: the cell keeps the
/// published geometry and publishes the focus, visibility, mode, occlusion,
/// execution state and every other fact the read found
/// ([`WindowStateCell`]).
#[derive(Debug)]
pub(crate) struct Sample {
    /// The facts read, on the geometry read or, when that was refused, on
    /// no geometry, which the cell replaces with the published one.
    state: WindowState,
    /// Why the geometry read describes no window.
    refused: Option<InvalidGeometry>,
    /// What the host found while reading that its window should say, said by
    /// the window's delivery in its turn, never from inside the read.
    note: Option<Note>,
}

impl Sample {
    /// The facts `facts` puts onto the snapshot `geometry` describes; when
    /// `geometry` was refused, onto no geometry, and the sample carries the
    /// refusal.
    pub(crate) fn read(
        geometry: Result<SampledGeometry, InvalidGeometry>,
        facts: impl FnOnce(WindowState) -> WindowState,
    ) -> Self {
        match geometry {
            Ok(geometry) => Self {
                state: facts(geometry.state()),
                refused: None,
                note: None,
            },
            Err(refused) => Self {
                state: facts(SampledGeometry::EMPTY.state()),
                refused: Some(refused),
                note: None,
            },
        }
    }

    /// This read, with `note` for its window to say in the window's turn: a
    /// fact the host could not publish, which a `tracing` event inside the
    /// read would report under the reader's borrows. The host says it once
    /// per run of such reads, as the cell says a refused geometry.
    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn noting(mut self, note: impl FnOnce() + Send + 'static) -> Self {
        self.note = Some(Note(Box::new(note)));
        self
    }
}

/// What a read found for its window to say (`Sample::noting`).
pub(crate) struct Note(Box<dyn FnOnce() + Send>);

impl std::fmt::Debug for Note {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Note")
    }
}

impl From<WindowState> for Sample {
    /// A read whose every fact is `state`'s, its geometry included.
    fn from(state: WindowState) -> Self {
        Self {
            state,
            refused: None,
            note: None,
        }
    }
}

/// `sample`, a read whose geometry was refused, on the geometry `published`
/// holds, or on no geometry for a window's first read (`None`). The one rule
/// for a refused geometry, at a window's opening and after it:
///
/// - the outer rectangle and the content extent are the published ones;
/// - the drawable metrics go with them: the sample carries none, so the
///   published ones stand (`WindowState::successor` keeps them for none) and a
///   first snapshot has none, never a drawable area on an empty window;
/// - `normal_geometry` is what the contract says it is, the rectangle a
///   window in another mode than normal returns to, whatever mode the read
///   reports: none for a normal window; the published outer rectangle for a
///   window that left the normal mode since, since that is where it was in
///   it (none when that rectangle encloses no area: a window whose first
///   read was refused has nowhere known to return to); the published one
///   for a window that was already away from it.
fn on_published_geometry(mut sample: WindowState, published: Option<&WindowState>) -> WindowState {
    sample.metrics = None;
    let Some(published) = published else {
        sample.outer = WindowGeometry::EMPTY;
        sample.content_size = WindowExtent::ZERO;
        sample.normal_geometry = None;
        return sample;
    };
    sample.outer = published.outer;
    sample.content_size = published.content_size;
    sample.normal_geometry = match (published.mode, sample.mode) {
        (_, WindowMode::Normal) => None,
        // A window whose published rectangle encloses no area (its first read
        // was refused too) has nowhere known to return to.
        (WindowMode::Normal, _) => {
            let size = published.outer.size();
            (size.width > 0.0 && size.height > 0.0).then_some(published.outer)
        }
        _ => published.normal_geometry,
    };
    sample
}

/// What the cell has to say about its window, said by the window's delivery
/// in its own turn ([`WindowDelivery::diagnose`]), never from inside a read:
/// a `tracing` subscriber is code the reader does not own, and a reader may
/// hold borrows it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StateDiagnostic {
    /// A read's geometry describes no window; the published geometry stands
    /// until a read describes one. Said once per run of such reads.
    Refused(InvalidGeometry),
    /// The window's revisions are used up: the window publishes nothing again,
    /// and its capabilities are withdrawn.
    Frozen(flui_platform::WindowRevisionExhausted),
    /// The process has no capability publication left: the window's
    /// capabilities are withdrawn, and its other facts go on publishing.
    Withdrawn,
}

impl StateDiagnostic {
    /// Say it.
    pub(crate) fn emit(self) {
        match self {
            Self::Refused(refused) => tracing::warn!(
                %refused,
                "a window system's report describes no window; its published geometry stands"
            ),
            Self::Withdrawn => tracing::warn!(
                "the process has no capability publication left; a window takes no command"
            ),
            Self::Frozen(error) => {
                tracing::warn!(%error, "a window stops publishing state changes");
            }
        }
    }
}

/// The answer to a command a host has no way to carry out. `admit` refuses it
/// first, through a capability the host names absent, and a token opens only
/// under the publication it was admitted against
/// ([`AdmittedCommand::against`](flui_platform::AdmittedCommand::against)), so a
/// window reaches this only for a command its published capabilities honour and
/// its `apply` has no arm for: a command the contract gained before the host
/// learnt it. The window system cannot do it, and the caller hears so; no host
/// answers `Ok` for a command it did not carry out.
pub(crate) fn not_carried_out(command: &flui_platform::WindowCommand) -> flui_platform::ApplyError {
    flui_platform::ApplyError::failed(format!("this window system does not carry out {command:?}"))
}

/// The frame a window reports whose window system says only whether its own
/// frame is on: [`WindowFrame::System`](flui_platform::WindowFrame::System) or
/// [`WindowFrame::None`](flui_platform::WindowFrame::None). A host that
/// draws no title bar of the application's
/// ([`WindowCapabilities::app_title_bar`](flui_platform::WindowCapabilities::app_title_bar)
/// `Unavailable`) has no third answer.
#[cfg(any(windows, target_os = "macos", wayland_host))]
pub(crate) const fn system_frame_or_none(decorated: bool) -> flui_platform::WindowFrame {
    if decorated {
        flui_platform::WindowFrame::System
    } else {
        flui_platform::WindowFrame::None
    }
}

/// The frame a window reports whose window system says only whether its own
/// title bar is on (`titled`), on a host that takes a title bar of the
/// application's: one the application asked to draw reads back as it was asked
/// for while the window keeps its title bar, the others as
/// [`system_frame_or_none`] says.
#[cfg(any(windows, target_os = "macos"))]
pub(crate) const fn reported_frame(
    titled: bool,
    asked: flui_platform::WindowFrame,
) -> flui_platform::WindowFrame {
    match asked {
        flui_platform::WindowFrame::AppTitleBar(_) if titled => asked,
        _ => system_frame_or_none(titled),
    }
}

/// `size` held to `constraints` the way a resize by the user is: raised to the
/// minimum, then lowered to the maximum, each axis on its own, so a maximum
/// below the minimum wins. A window system that applies a size as given
/// (AppKit's `setContentSize:`, a headless model) is handed this instead.
pub(crate) fn held_to(
    size: flui_foundation::geometry::Size<f64>,
    constraints: flui_platform::SizeConstraints,
) -> flui_foundation::geometry::Size<f64> {
    let mut size = size;
    if let Some(min) = constraints.min {
        size.width = size.width.max(min.width());
        size.height = size.height.max(min.height());
    }
    if let Some(max) = constraints.max {
        size.width = size.width.min(max.width());
        size.height = size.height.min(max.height());
    }
    size
}

/// What a window system says of a window, as the three facts its mode is
/// decided from. Each platform reads them from its own report (AppKit's style
/// mask and flags, the X11 window manager's properties, the Wayland
/// toplevel's configured states, the Win32 window's tracked mode) and
/// [`mode_of`] decides.
#[cfg(any(windows, target_os = "macos", x11_host, wayland_host, test))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ModeFlags {
    /// The window is minimized: iconic, in the Dock or on the taskbar.
    pub(crate) minimized: bool,
    /// The window covers its monitor without a frame.
    pub(crate) fullscreen: bool,
    /// The window is maximized.
    pub(crate) maximized: bool,
}

/// The mode of a window with these flags. A minimized window is hidden
/// whatever else is set; a fullscreen one often keeps its maximized flag, so
/// fullscreen comes before maximized.
#[cfg(any(windows, target_os = "macos", x11_host, wayland_host, test))]
pub(crate) const fn mode_of(flags: ModeFlags) -> flui_platform::WindowMode {
    use flui_platform::WindowMode;
    if flags.minimized {
        WindowMode::Minimized
    } else if flags.fullscreen {
        WindowMode::Fullscreen
    } else if flags.maximized {
        WindowMode::Maximized
    } else {
        WindowMode::Normal
    }
}

/// A window's logical side `logical` at `ratio` in whole device pixels, as
/// every host hands a window extent to its window system or draws a surface
/// of the window: the one conversion every layer uses
/// ([`flui_foundation::geometry::device_extent`]: snapped, at least one pixel),
/// and at most
/// [`MAX_SURFACE_EXTENT`](flui_platform::MAX_SURFACE_EXTENT), past which the
/// side is clamped, since no host gives a window a larger surface. `None` for a
/// side that is not finite or not positive.
#[cfg(any(windows, x11_host, wayland_host, test))]
pub(crate) fn device_extent(
    logical: f64,
    ratio: flui_foundation::geometry::DevicePixelRatio,
) -> Option<u32> {
    flui_foundation::geometry::device_extent(logical, ratio)
        .map(|pixels| pixels.min(flui_platform::MAX_SURFACE_EXTENT))
}

/// [`device_extent`] with the device pixels rounded by `round` instead of
/// snapped, as [`device_limits`] rounds a limit. The range is the same: at
/// least one pixel, clamped at the largest surface, `None` for a side that is
/// not finite or not positive.
#[cfg(any(windows, x11_host, test))]
fn device_extent_rounded(
    logical: f64,
    ratio: flui_foundation::geometry::DevicePixelRatio,
    round: fn(f64) -> f64,
) -> Option<u32> {
    use flui_platform::MAX_SURFACE_EXTENT;
    if !(logical.is_finite() && logical > 0.0) {
        return None;
    }
    // A product past `f64::MAX` is infinite, which the clamp also bounds.
    let pixels = round(ratio.to_device(logical)).clamp(1.0, f64::from(MAX_SURFACE_EXTENT));
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a whole number clamped to 1..=MAX_SURFACE_EXTENT just above"
    )]
    let pixels = pixels as u32;
    Some(pixels)
}

/// A window's content-size limits in device pixels, as every host hands them
/// to its window system.
#[cfg(any(windows, x11_host, test))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DeviceLimits {
    /// The smallest content size, (width, height).
    pub(crate) min: Option<(u32, u32)>,
    /// The largest content size, never below `min` on either side.
    pub(crate) max: Option<(u32, u32)>,
}

/// The logical limits `min` and `max` at `ratio` in device pixels, by the
/// hosts' one rule: a minimum rounds up and a maximum down, so the content
/// never ends outside the logical limits, each side at least a pixel and
/// clamped at the largest surface; a limit with a side that is no size is no
/// limit. Where rounding leaves the maximum below the minimum (both equal at a
/// fractional ratio), the maximum is raised to it, so the pair is ordered by
/// construction and the window is held at the minimum.
#[cfg(any(windows, x11_host, test))]
pub(crate) fn device_limits(
    min: Option<flui_foundation::geometry::Size<f64>>,
    max: Option<flui_foundation::geometry::Size<f64>>,
    ratio: flui_foundation::geometry::DevicePixelRatio,
) -> DeviceLimits {
    let device = |size: flui_foundation::geometry::Size<f64>, round: fn(f64) -> f64| {
        Some((
            device_extent_rounded(size.width, ratio, round)?,
            device_extent_rounded(size.height, ratio, round)?,
        ))
    };
    let min = min.and_then(|size| device(size, f64::ceil));
    let max = max
        .and_then(|size| device(size, f64::floor))
        .map(|max| min.map_or(max, |min: (u32, u32)| (max.0.max(min.0), max.1.max(min.1))));
    DeviceLimits { min, max }
}

/// What a read of the cell answered.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Published {
    /// The snapshot now published.
    pub(crate) state: WindowState,
    /// Whether this call published a higher revision. A backend raises
    /// `WindowHandler::state_changed` exactly when this is `true`, after the call returned
    /// and so outside the cell's lock.
    pub(crate) changed: bool,
}

impl Published {
    /// What a state publisher answers for this read.
    pub(crate) const fn outcome(&self) -> Publication {
        if self.changed {
            Publication::Changed
        } else {
            Publication::Unchanged
        }
    }
}

struct Slot {
    state: WindowState,
    /// The invalidations counted when the published state was read: the
    /// snapshot is current while no invalidation has come since.
    read_after: u64,
    /// The revision space was used up: nothing is published any more.
    frozen: bool,
    /// The last read's geometry was refused: what the window system reported
    /// describes no window. Kept so a window system that goes on reporting it
    /// is diagnosed once, not on every read.
    refusing: bool,
    /// What the cell has to say and has not handed to a delivery yet: at
    /// most one refusal (the first of the current run of refused reads) and
    /// the freeze, which happens once.
    unreported_refusal: Option<InvalidGeometry>,
    unreported_freeze: Option<flui_platform::WindowRevisionExhausted>,
    /// The window's capabilities were withdrawn for want of a publication,
    /// not yet said.
    unreported_withdrawal: bool,
    /// A note the first read carried, which no delivery has taken yet.
    unreported_note: Option<Note>,
}

/// One window's last published [`WindowState`] and its staleness.
///
/// A backend marks the cell stale on every OS event that may change the window
/// (`WM_SIZE`, `windowDidResize`, `fullscreenchange`, or a
/// command it just issued). The next read of the cell reads the OS again,
/// with no lock held across the read, and publishes the sample; a sample equal
/// to the published one keeps its revision. A host reads through two methods:
/// [`read`](Self::read) for a caller of `PlatformWindow::state`, which
/// announces a new revision at the owner's turn, and
/// [`refresh`](Self::refresh) or [`publish_report`](Self::publish_report)
/// for a change its window system reported, which announce it as the host's
/// own event.
///
/// A read whose geometry describes no window (a size over a scale of zero, a
/// NaN the window system sent) loses its geometry and nothing else
/// ([`Sample`]): the published geometry stands, the read's other facts are
/// published, and the next invalidation reads again. This is the one place
/// that decides it, for every host. What the cell has to say of it is said
/// by the window's delivery in its turn ([`StateDiagnostic`]).
///
/// Threads may read at once. A sample begun before an invalidation that
/// another read has already published past is discarded, so once the read
/// after a change has published, no slower read puts the window from before
/// the change back. Reads begun before the same invalidation may still publish
/// in either order until then: the samples cannot be ordered without a lock
/// held across them, and a backend's sample may wait on its owner thread.
pub(crate) struct WindowStateCell {
    last: Mutex<Slot>,
    /// Counts every invalidation, from any thread. A read takes the count
    /// before it reads the OS, so an invalidation that lands during the read
    /// leaves the cell stale. At one per nanosecond it would take centuries to
    /// wrap.
    invalidations: AtomicU64,
    /// Where the window's capability publications come from: the process's
    /// mint, or an exhausted one in a test.
    mint: PublicationMint,
    /// The snapshots the cell publishes, its first included, journalled while
    /// a native capture is installed (ADR-0215 §4): every host publishes
    /// through a cell, so this is the one place a window's state is recorded.
    #[cfg(all(debug_assertions, feature = "native-capture"))]
    journal: crate::journal::capture::Journal,
}

impl crate::owner::notices::StaleOnWake for WindowStateCell {
    fn mark_stale(&self) {
        self.invalidate();
    }
}

impl WindowStateCell {
    /// Whether the next read asks the operating system again.
    #[cfg(test)]
    pub(crate) fn is_stale(&self) -> bool {
        self.last.lock().read_after != self.invalidations.load(Ordering::Acquire)
    }

    /// A cell publishing `initial`, stale: the first read asks the OS.
    #[cfg(any(test, target_vendor = "apple", x11_host, wayland_host))]
    pub(crate) fn new(initial: WindowState) -> Self {
        Self::open(Sample::from(initial))
    }

    /// A cell publishing the first read of its window, stale: the next read
    /// asks the OS again. A first read whose geometry describes no window
    /// opens with no geometry and its other facts (the capabilities among
    /// them, so commands are admitted against what the window honours), and
    /// the refusal is said with the window's first delivery that hands the
    /// cell's diagnostics on.
    ///
    /// The capabilities are published (`WindowCapabilities::publish`): a
    /// publication no other capabilities of the process are, which later
    /// snapshots keep while the capabilities stay the same, so a command
    /// admitted for another window, or against capabilities built by hand or
    /// edited, never opens on this one.
    pub(crate) fn open(first_read: Sample) -> Self {
        Self::open_from(first_read, flui_platform::PublicationMint::process())
    }

    /// [`open`](Self::open) publishing from `mint`: the process's in
    /// production, an exhausted one in a test of what a window does once the
    /// process has no publication left.
    pub(crate) fn open_from(first_read: Sample, mint: PublicationMint) -> Self {
        let Sample {
            state: mut initial,
            refused,
            note,
        } = first_read;
        if refused.is_some() {
            initial = on_published_geometry(initial, None);
        }
        // A process with no publication left opens the window with its
        // capabilities withdrawn: it takes no command, its other facts publish
        // as any window's do, and the withdrawal is said with its first
        // delivery. Refusing the open is not every host's to do: an Android
        // activity or a UIKit scene exists before its window is built.
        let withdrawn = if let Ok(published) = initial.capabilities.publish_from(mint) {
            initial.capabilities = published;
            false
        } else {
            initial.capabilities = initial.capabilities.withdraw();
            true
        };
        // A first snapshot already at the last revision is the last one the
        // window will publish: frozen, its capabilities withdrawn, so that
        // revision never names a second snapshot.
        let frozen = initial.revision == WindowRevision::LAST;
        if frozen {
            initial.capabilities = initial.capabilities.withdraw();
        }
        #[cfg(all(debug_assertions, feature = "native-capture"))]
        let journal = {
            let journal = crate::journal::capture::Journal::open_stream(
                flui_protocol::native_recording::Stream::WindowState,
            );
            journal.record(crate::journal::capture::Captured::WindowState(&initial));
            journal
        };
        Self {
            last: Mutex::new(Slot {
                state: initial,
                read_after: 0,
                frozen,
                refusing: refused.is_some(),
                unreported_refusal: refused,
                unreported_freeze: frozen.then_some(flui_platform::WindowRevisionExhausted),
                unreported_withdrawal: withdrawn,
                unreported_note: note,
            }),
            invalidations: AtomicU64::new(1),
            mint,
            #[cfg(all(debug_assertions, feature = "native-capture"))]
            journal,
        }
    }

    /// Record that the window may have changed. One atomic increment, so a
    /// window procedure, a notification block or a foreign thread may call it
    /// at any point.
    pub(crate) fn invalidate(&self) {
        self.invalidations.fetch_add(1, Ordering::Release);
    }

    /// The capabilities of the last published snapshot, without reading the
    /// OS: what an admitted command is opened against
    /// ([`AdmittedCommand::against`](flui_platform::AdmittedCommand::against)),
    /// so a command admitted against the snapshot a caller read applies
    /// unless what the window honours was published as changed since.
    pub(crate) fn published_capabilities(&self) -> WindowCapabilities {
        self.last.lock().state.capabilities
    }

    /// The last published snapshot, without reading the OS.
    #[cfg(any(windows, x11_host, wayland_host))]
    pub(crate) fn published(&self) -> WindowState {
        self.last.lock().state.clone()
    }

    /// The snapshot for a caller that reads the window
    /// (`PlatformWindow::state`): the published one, read again with
    /// `sample` first when something invalidated the cell.
    ///
    /// The reader may hold borrows the handler or a `tracing` subscriber
    /// takes again, so nothing is said or announced from here: a revision
    /// this read publishes is announced, and what the cell has to say is
    /// said, by `delivery` in the window's own turn
    /// ([`WindowDelivery::state_changed_after_read`],
    /// [`WindowDelivery::diagnose`]).
    pub(crate) fn read(
        &self,
        delivery: &WindowDelivery,
        sample: impl FnOnce() -> Sample,
    ) -> WindowState {
        let published = self.noted_read(delivery, sample);
        if published.changed {
            delivery.state_changed_after_read();
        }
        published.state
    }

    /// The window system reported a change: read the window again with
    /// `sample` for its state publisher, announcing nothing (the publisher's
    /// caller raises `state_changed` for [`Publication::Changed`]).
    pub(crate) fn refresh(
        &self,
        delivery: &WindowDelivery,
        sample: impl FnOnce() -> Sample,
    ) -> Publication {
        self.invalidate();
        let published = self.noted_read(delivery, sample);
        published.outcome()
    }

    /// The published snapshot, read again with `sample` when something
    /// invalidated the cell, announcing nothing: for a host that announces a
    /// revision itself, later or under its own rule (the X11 window reads
    /// inside a command and announces at the command's end, or follows a
    /// marked window once a turn). What the cell has to say goes to
    /// `delivery` as from every read.
    #[cfg(x11_host)]
    pub(crate) fn read_unannounced(
        &self,
        delivery: &WindowDelivery,
        sample: impl FnOnce() -> Sample,
    ) -> Published {
        self.noted_read(delivery, sample)
    }

    /// The window system reported a change, or a command was just carried
    /// out: read the window again with `sample`, and raise
    /// `WindowHandler::state_changed` through `delivery` when that published a
    /// new revision. Answers the snapshot now published.
    pub(crate) fn publish_report(
        &self,
        delivery: &WindowDelivery,
        sample: impl FnOnce() -> Sample,
    ) -> WindowState {
        self.invalidate();
        let published = self.noted_read(delivery, sample);
        if published.changed {
            delivery.dispatch_state_changed();
        }
        published.state
    }

    /// A read through [`Self::state_with`] whose note, and whatever the cell
    /// has to say, go to `delivery`, which says them in the window's turn.
    fn noted_read(&self, delivery: &WindowDelivery, sample: impl FnOnce() -> Sample) -> Published {
        let mut note = None;
        let published = self.state_with(|| {
            let mut read = sample();
            note = read.note.take();
            read
        });
        let (refusal, freeze, withdrawal, opening_note) = {
            let mut slot = self.last.lock();
            (
                slot.unreported_refusal.take(),
                slot.unreported_freeze.take(),
                std::mem::take(&mut slot.unreported_withdrawal),
                slot.unreported_note.take(),
            )
        };
        if let Some(refused) = refusal {
            delivery.diagnose(move || StateDiagnostic::Refused(refused).emit());
        }
        if withdrawal {
            delivery.diagnose(|| StateDiagnostic::Withdrawn.emit());
        }
        if let Some(error) = freeze {
            delivery.diagnose(move || StateDiagnostic::Frozen(error).emit());
        }
        for Note(note) in opening_note.into_iter().chain(note) {
            delivery.diagnose(note);
        }
        published
    }

    /// A read that announces and says nothing, for a test that only needs the
    /// cell current.
    #[cfg(test)]
    pub(crate) fn read_quietly(&self, sample: impl FnOnce() -> Sample) -> Published {
        self.state_with(sample)
    }

    /// The snapshot: the last published one while nothing has invalidated the
    /// cell, otherwise the result of `sample` published. Announces nothing
    /// and says nothing: a host reads through [`Self::read`],
    /// [`Self::refresh`] or [`Self::publish_report`].
    ///
    /// `sample` runs with no lock held, so it may call the OS.
    fn state_with(&self, sample: impl FnOnce() -> Sample) -> Published {
        let invalidations = self.invalidations.load(Ordering::Acquire);
        {
            let slot = self.last.lock();
            if slot.read_after == invalidations {
                return Published {
                    state: slot.state.clone(),
                    changed: false,
                };
            }
        }
        self.commit(sample(), invalidations)
    }

    /// Publish a sample read after `invalidations`: the previous snapshot when
    /// nothing differs or a read begun later has published already, otherwise
    /// the next revision. A sample whose geometry was refused publishes its
    /// other facts on the published geometry. The snapshot that takes the last
    /// revision is the cell's final one: its capabilities are withdrawn, it is
    /// announced as any change is, one diagnostic is said, and nothing is
    /// published again.
    fn commit(&self, sample: Sample, invalidations: u64) -> Published {
        let mut slot = self.last.lock();
        if slot.read_after > invalidations {
            return Published {
                state: slot.state.clone(),
                changed: false,
            };
        }
        slot.read_after = invalidations;
        if slot.frozen {
            return Published {
                state: slot.state.clone(),
                changed: false,
            };
        }
        let Sample {
            state: mut sample,
            refused,
            note: _,
        } = sample;
        match refused {
            None => slot.refusing = false,
            Some(refused) => {
                sample = on_published_geometry(sample, Some(&slot.state));
                if !std::mem::replace(&mut slot.refusing, true) {
                    slot.unreported_refusal = Some(refused);
                }
            }
        }
        match slot.state.successor_from(sample, self.mint) {
            Ok(mut next) => {
                let changed = next.revision != slot.state.revision;
                if changed {
                    // The last revision is the last snapshot the window
                    // publishes: it freezes there, and as it can no longer say
                    // what it honours, that snapshot withdraws its capabilities
                    // (ADR-0341). No revision names two snapshots.
                    if next.revision == WindowRevision::LAST {
                        next.capabilities = next.capabilities.withdraw();
                        slot.frozen = true;
                        slot.unreported_freeze = Some(flui_platform::WindowRevisionExhausted);
                    }
                    if next.capabilities.is_withdrawn() && !slot.state.capabilities.is_withdrawn() {
                        slot.unreported_withdrawal = true;
                    }
                    slot.state = next.clone();
                }
                // The capture's sink runs with the slot released, as the
                // handler's `state_changed` does.
                drop(slot);
                #[cfg(all(debug_assertions, feature = "native-capture"))]
                if changed {
                    self.journal
                        .record(crate::journal::capture::Captured::WindowState(&next));
                }
                Published {
                    state: next,
                    changed,
                }
            }
            // Reached only by a snapshot already at the last revision and not
            // frozen, which `open` rules out: freeze on it and publish nothing.
            Err(error) => {
                slot.frozen = true;
                slot.unreported_freeze = Some(error);
                Published {
                    state: slot.state.clone(),
                    changed: false,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use flui_foundation::geometry::Size;
    use flui_platform::{
        InvalidGeometry, WindowExtent, WindowGeometry, WindowRevision, WindowState,
    };

    use super::{Sample, SampledGeometry, WindowStateCell, held_to};

    /// A size is held to the window's limits on each axis alone, and a
    /// maximum below the minimum wins, as the window systems that clamp a
    /// resize themselves do.
    #[test]
    fn a_size_is_held_to_the_constraints() {
        use flui_platform::SizeConstraints;

        let size = Size::new;
        let limits = |min, max| SizeConstraints::try_new(min, max).expect("finite limits");
        let both = limits(Some(size(400.0, 300.0)), Some(size(900.0, 700.0)));
        let rows = [
            ("inside", both, size(640.0, 480.0), size(640.0, 480.0)),
            ("below", both, size(200.0, 100.0), size(400.0, 300.0)),
            ("above", both, size(2000.0, 2000.0), size(900.0, 700.0)),
            (
                "one axis each way",
                both,
                size(200.0, 2000.0),
                size(400.0, 700.0),
            ),
            (
                "no limits",
                SizeConstraints::NONE,
                size(1.0, 1e9),
                size(1.0, 1e9),
            ),
            (
                "a minimum only",
                limits(Some(size(400.0, 300.0)), None),
                size(200.0, 5000.0),
                size(400.0, 5000.0),
            ),
            (
                "a maximum only",
                limits(None, Some(size(900.0, 700.0))),
                size(200.0, 5000.0),
                size(200.0, 700.0),
            ),
            (
                "a maximum below the minimum",
                limits(Some(size(500.0, 500.0)), Some(size(300.0, 300.0))),
                size(400.0, 400.0),
                size(300.0, 300.0),
            ),
        ];
        for (case, constraints, asked, held) in rows {
            assert_eq!(held_to(asked, constraints), held, "{case}");
        }
    }

    /// A read of a window `width` wide, as a host's sample answers it, with
    /// `focused` as its focus: its geometry refused for a width no window
    /// has.
    fn read(width: f64, focused: bool) -> Sample {
        let geometry =
            WindowExtent::try_new(Size::new(width, 100.0)).map(|extent| SampledGeometry {
                outer: WindowGeometry::unplaced(extent),
                content: extent,
                normal: None,
            });
        Sample::read(geometry, |state| state.with_focused(focused))
    }

    fn state(width: f64) -> Sample {
        read(width, false)
    }

    fn window(width: f64) -> WindowState {
        let sample = state(width);
        assert_eq!(sample.refused, None, "a finite width");
        sample.state
    }

    /// A read whose geometry describes no window publishes nothing of its
    /// geometry and is not repeated until the window is invalidated again;
    /// the next read that describes a window publishes it.
    #[test]
    fn a_read_that_describes_no_window_publishes_nothing() {
        let cell = WindowStateCell::new(window(1.0));
        let first = cell.state_with(|| state(2.0));
        assert!(first.changed);
        for width in [f64::INFINITY, f64::NAN, -1.0] {
            cell.invalidate();
            let refused = cell.state_with(|| state(width));
            assert!(!refused.changed, "{width}");
            assert_eq!(refused.state, first.state, "{width}");
            let kept = cell.state_with(|| panic!("a refused read is not repeated"));
            assert_eq!(kept.state, first.state, "{width}");
        }
        cell.invalidate();
        let recovered = cell.state_with(|| state(3.0));
        assert!(recovered.changed);
        assert_eq!(
            recovered.state.revision.get(),
            first.state.revision.get() + 1,
            "the refused reads took no revision"
        );
        assert_eq!(recovered.state.content_size, Size::new(3.0, 100.0));
    }

    /// A read whose geometry describes no window costs it only its geometry:
    /// the published geometry and metrics stand, and every other fact it read
    /// is published.
    #[test]
    fn a_refused_geometry_keeps_the_published_one_and_publishes_the_other_facts() {
        let cell = WindowStateCell::new(window(1.0));
        let first = cell.state_with(|| state(2.0));
        cell.invalidate();
        let focused = cell.state_with(|| read(f64::NAN, true));
        assert!(focused.changed, "the focus is published");
        assert!(focused.state.focused);
        assert_eq!(focused.state.outer, first.state.outer);
        assert_eq!(focused.state.content_size, first.state.content_size);
        assert_eq!(focused.state.metrics, first.state.metrics);
        cell.invalidate();
        let unchanged = cell.state_with(|| read(f64::INFINITY, true));
        assert!(!unchanged.changed, "nothing else changed");
    }

    /// A read of a window `width` wide with `facts` put on it.
    fn read_as(width: f64, facts: impl FnOnce(WindowState) -> WindowState) -> Sample {
        let geometry =
            WindowExtent::try_new(Size::new(width, 100.0)).map(|extent| SampledGeometry {
                outer: WindowGeometry::unplaced(extent),
                content: extent,
                normal: None,
            });
        Sample::read(geometry, facts)
    }

    fn metrics(scale: f64) -> flui_foundation::geometry::WindowMetrics {
        flui_foundation::geometry::WindowMetrics::try_new(Size::new(200, 100), scale)
            .expect("drawable metrics")
    }

    /// The drawable metrics go with the geometry: a refused read whose own
    /// metrics are drawable (a scale its window system reported apart from
    /// the rectangle) publishes none of them over the published geometry.
    /// The rule predates the cell's shared refused-geometry function; this
    /// row holds it through that function, and fails if it publishes the
    /// read's metrics.
    #[test]
    fn a_refused_geometry_keeps_the_published_metrics() {
        let cell = WindowStateCell::new(window(1.0));
        let first =
            cell.state_with(|| read_as(2.0, |state| state.with_metrics(Some(metrics(1.0)))));
        cell.invalidate();
        let refused = cell.state_with(|| {
            read_as(f64::NAN, |state| {
                state.with_metrics(Some(metrics(2.0))).with_focused(true)
            })
        });
        assert!(refused.changed, "the focus is published");
        assert_eq!(refused.state.metrics, first.state.metrics);
    }

    /// A first read whose geometry is refused follows the rule every later
    /// one does: no geometry, and so no drawable metrics either.
    #[test]
    fn a_refused_first_read_opens_with_no_metrics() {
        let cell = WindowStateCell::open(read_as(f64::NAN, |state| {
            state.with_metrics(Some(metrics(2.0)))
        }));
        let opened = cell.last.lock().state.clone();
        assert_eq!(opened.content_size, WindowExtent::ZERO);
        assert_eq!(opened.metrics, None);
    }

    /// A window whose first read was refused has no rectangle to return to:
    /// a later refused read that reports it maximized publishes none, never
    /// the empty rectangle it opened with.
    #[test]
    fn a_window_that_never_had_a_rectangle_returns_nowhere() {
        use flui_platform::WindowMode;

        let cell = WindowStateCell::open(state(f64::NAN));
        cell.invalidate();
        let maximized = cell
            .state_with(|| read_as(f64::NAN, |state| state.with_mode(WindowMode::Maximized)))
            .state;
        assert_eq!(maximized.mode, WindowMode::Maximized);
        assert_eq!(maximized.outer, WindowGeometry::EMPTY);
        assert_eq!(maximized.normal_geometry, None);
    }

    /// On a refused read the rectangle a window returns to stays what the
    /// contract says it is, whatever mode the read reports: the last
    /// published rectangle for a window that left the normal mode, none for
    /// one that came back to it.
    #[test]
    fn a_refused_geometry_keeps_the_normal_geometry_true_to_the_mode() {
        use flui_platform::WindowMode;

        let cell = WindowStateCell::new(window(1.0));
        let normal = cell.state_with(|| state(2.0)).state;
        cell.invalidate();
        let maximized = cell
            .state_with(|| read_as(f64::NAN, |state| state.with_mode(WindowMode::Maximized)))
            .state;
        assert_eq!(maximized.mode, WindowMode::Maximized);
        assert_eq!(
            maximized.normal_geometry,
            Some(normal.outer),
            "it returns to where it was"
        );
        cell.invalidate();
        let fullscreen = cell
            .state_with(|| read_as(f64::NAN, |state| state.with_mode(WindowMode::Fullscreen)))
            .state;
        assert_eq!(
            fullscreen.normal_geometry,
            Some(normal.outer),
            "and still does"
        );
        cell.invalidate();
        let restored = cell
            .state_with(|| read_as(f64::NAN, |state| state.with_mode(WindowMode::Normal)))
            .state;
        assert_eq!(restored.mode, WindowMode::Normal);
        assert_eq!(
            restored.normal_geometry, None,
            "a normal window returns nowhere"
        );
    }

    /// A note a read carries (a fact its host could not publish) is said by
    /// the window's delivery in its turn, never from inside the read, where
    /// the reader's borrows are held.
    #[test]
    fn a_reads_note_is_said_in_the_windows_turn() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        use flui_platform::input_capabilities::InputCapabilities;
        use flui_testing::window_handler::RecordingWindowHandler;

        use crate::owner::window_delivery::WindowDelivery;

        let delivery = WindowDelivery::new(
            RecordingWindowHandler::new(),
            InputCapabilities::UNAVAILABLE,
        );
        let cell = WindowStateCell::new(window(1.0));
        let said = Arc::new(AtomicBool::new(false));
        let noted = Arc::clone(&said);
        let _ = cell.read(&delivery, || {
            state(2.0).noting(move || noted.store(true, Ordering::SeqCst))
        });
        assert!(
            !said.load(Ordering::SeqCst),
            "nothing is said from the read"
        );
        delivery.deliver_queued();
        assert!(said.load(Ordering::SeqCst), "the window's turn says it");
    }

    /// The first refusal of a run of refused reads is said, once; a read that
    /// describes a window ends the run, and the next refusal is said again.
    #[test]
    fn a_run_of_refused_reads_is_said_once() {
        let cell = WindowStateCell::new(window(1.0));
        let unreported = |cell: &WindowStateCell| cell.last.lock().unreported_refusal.take();
        for width in [f64::NAN, -1.0, f64::INFINITY] {
            cell.invalidate();
            let _ = cell.state_with(|| state(width));
        }
        assert!(
            matches!(
                unreported(&cell),
                Some(InvalidGeometry::Extent { width, .. }) if width.is_nan()
            ),
            "the first of the run"
        );
        cell.invalidate();
        let _ = cell.state_with(|| state(-2.0));
        assert_eq!(unreported(&cell), None, "the run goes on");
        cell.invalidate();
        let _ = cell.state_with(|| state(5.0));
        cell.invalidate();
        let _ = cell.state_with(|| state(-3.0));
        assert_eq!(
            unreported(&cell),
            Some(InvalidGeometry::Extent {
                width: -3.0,
                height: 100.0
            }),
            "a new run"
        );
    }

    /// A first read whose geometry describes no window opens the cell with
    /// no geometry and the read's other facts, and the refusal is waiting to
    /// be said.
    #[test]
    fn a_cell_opens_on_a_refused_first_read_with_its_other_facts() {
        let cell = WindowStateCell::open(read(f64::NAN, true));
        let opened = cell.last.lock().state.clone();
        assert_eq!(opened.outer, WindowGeometry::EMPTY);
        assert!(opened.focused);
        assert!(cell.last.lock().unreported_refusal.is_some());
    }

    #[test]
    fn invalidation_during_a_sample_stays_stale() {
        let cell = WindowStateCell::new(window(1.0));
        let samples = Cell::new(0);
        let _ = cell.state_with(|| {
            samples.set(samples.get() + 1);
            // The OS changes while the window is being read.
            cell.invalidate();
            state(2.0)
        });
        let _ = cell.state_with(|| {
            samples.set(samples.get() + 1);
            state(2.0)
        });
        assert_eq!(samples.get(), 2, "the next call reads again");
        let _ = cell.state_with(|| {
            samples.set(samples.get() + 1);
            state(2.0)
        });
        assert_eq!(samples.get(), 2, "and then the snapshot is kept");
    }

    /// Two threads read at once: the one that began first finishes last, after
    /// the window changed and the other read published the change.
    #[test]
    fn a_read_overtaken_by_a_later_one_publishes_nothing() {
        let cell = WindowStateCell::new(window(1.0));
        let _ = cell.state_with(|| state(1.0));
        cell.invalidate();
        let overtaken = cell.state_with(|| {
            // Read before the change, still to be published.
            let older = state(1.0);
            cell.invalidate();
            let newer = cell.state_with(|| state(2.0));
            assert!(newer.changed);
            older
        });
        assert!(!overtaken.changed, "the older read publishes no revision");
        assert_eq!(overtaken.state.content_size, Size::new(2.0, 100.0));
        let kept = cell.state_with(|| panic!("the newer snapshot is current"));
        assert_eq!(kept.state, overtaken.state);
    }

    /// A mint with no publication left, for what a window does once the
    /// process has none.
    const EXHAUSTED: flui_platform::PublicationMint = flui_platform::PublicationMint::exhausted();

    /// A window whose first snapshot already sits at the last revision is
    /// frozen from the start, its capabilities withdrawn, and publishes
    /// nothing again: that revision names one snapshot.
    #[test]
    fn a_window_opened_at_the_last_revision_is_frozen() {
        let mut last = window(1.0).with_capabilities(flui_platform::WindowCapabilities::ALL);
        last.revision = WindowRevision::LAST;
        let cell = WindowStateCell::new(last);
        let opened = cell.state_with(|| state(3.0));
        assert!(!opened.changed, "frozen from the start");
        assert!(opened.state.capabilities.is_withdrawn());
        for attempt in 0..3 {
            cell.invalidate();
            let published = cell.state_with(|| state(2.0 + f64::from(attempt)));
            assert!(!published.changed, "attempt {attempt}");
            assert_eq!(published.state, opened.state, "attempt {attempt}");
        }
        assert!(
            !cell.is_stale(),
            "a frozen cell is current until invalidated"
        );
    }

    /// The snapshot that takes the last revision is the window's final one:
    /// it is announced, its commands are withdrawn even though the sample
    /// carried the same capabilities, its reports and observations keep their
    /// values, a command admitted before answers `Superseded`, and nothing is
    /// admitted or published afterwards, also after the window system takes
    /// maximizing away. No revision names two snapshots.
    #[test]
    fn the_snapshot_that_takes_the_last_revision_withdraws_the_capabilities() {
        use flui_platform::{
            AdmissionError, ApplyError, CapabilityAbsence, WindowCapabilities, WindowCapability,
            WindowCommand,
        };

        let mut before_last = window(1.0).with_capabilities(WindowCapabilities::ALL);
        before_last.revision = WindowRevision::from_raw(u64::MAX - 1);
        let cell = WindowStateCell::new(before_last);
        let maximize = WindowCommand::SetMode(flui_platform::WindowMode::Maximized);
        let admitted = maximize
            .clone()
            .admit(&cell.published_capabilities())
            .expect("the window maximizes");

        cell.invalidate();
        let last = cell
            .state_with(|| Sample::from(window(2.0).with_capabilities(WindowCapabilities::ALL)));
        assert!(last.changed, "the final snapshot is announced");
        assert_eq!(last.state.revision, WindowRevision::LAST);
        assert_eq!(
            last.state.capabilities.maximizes(),
            WindowCapability::Absent(CapabilityAbsence::WITHDRAWN)
        );
        assert_eq!(
            last.state.capabilities.reports_presentation(),
            WindowCapability::Native,
            "a report no command reads keeps its value"
        );
        assert_eq!(
            admitted.against(&cell.published_capabilities()),
            Err(ApplyError::Superseded {
                command: maximize.clone()
            })
        );

        let mut without_maximize = WindowCapabilities::ALL.into_parts();
        without_maximize.maximizes = WindowCapability::NOT_ON_THIS_PLATFORM;
        cell.invalidate();
        let later = cell.state_with(|| {
            Sample::from(window(1.0).with_capabilities(without_maximize.into_capabilities()))
        });
        assert!(!later.changed, "frozen");
        assert_eq!(
            later.state, last.state,
            "the last revision names one snapshot"
        );
        assert_eq!(
            maximize.admit(&cell.published_capabilities()),
            Err(AdmissionError::Unpublished),
            "nothing is admitted once frozen"
        );
    }

    /// A window opened when the process has no publication left opens with
    /// its capabilities withdrawn, not frozen: the withdrawal waits to be said,
    /// and its other facts go on publishing at new revisions while its
    /// capabilities stay withdrawn.
    #[test]
    fn a_window_opened_with_no_publication_left_publishes_on_withdrawn() {
        use flui_platform::{AdmissionError, WindowCapabilities, WindowCommand};

        let cell = WindowStateCell::open_from(
            Sample::from(window(1.0).with_capabilities(WindowCapabilities::ALL)),
            EXHAUSTED,
        );
        assert!(cell.published_capabilities().is_withdrawn());
        assert!(
            cell.last.lock().unreported_withdrawal,
            "the withdrawal is said with the first delivery"
        );
        assert_eq!(
            WindowCommand::Hide.admit(&cell.published_capabilities()),
            Err(AdmissionError::Unpublished)
        );

        let delivery = crate::owner::window_delivery::WindowDelivery::new(
            flui_testing::window_handler::RecordingWindowHandler::new(),
            flui_platform::input_capabilities::InputCapabilities::UNAVAILABLE,
        );
        cell.invalidate();
        let focused = cell.read(&delivery, || read(1.0, true));
        assert!(focused.focused, "not frozen: the focus publishes");
        assert!(focused.capabilities.is_withdrawn(), "withdrawn for good");
        assert!(
            !cell.last.lock().unreported_withdrawal,
            "the read hands the withdrawal to the window's turn to say, once"
        );
    }

    /// A published snapshot whose capabilities change once the mint has no
    /// publication left withdraws them, and its other facts publish with it.
    #[test]
    fn a_change_with_no_publication_left_withdraws_the_capabilities() {
        use flui_platform::{AdmissionError, WindowCapabilities, WindowCommand};

        let cell = WindowStateCell::new(window(1.0).with_capabilities(WindowCapabilities::ALL));
        let published = cell.last.lock().state.clone();
        assert!(!published.capabilities.is_withdrawn());
        let next = published
            .successor_from(
                window(1.0)
                    .with_capabilities(WindowCapabilities::NONE)
                    .with_focused(true),
                EXHAUSTED,
            )
            .expect("a revision is left");
        assert!(next.capabilities.is_withdrawn());
        assert!(next.focused, "the other facts publish");
        assert_eq!(
            WindowCommand::Hide.admit(&next.capabilities),
            Err(AdmissionError::Unpublished)
        );
    }
}

/// The table of the one mode rule, which every window system's reading of its
/// own report is held to: a row's facts, put as that system reports them and
/// read back by the system's own reading, decide the row's mode. AppKit reads
/// a full-screen window as full screen whatever else it says (it does not
/// miniaturize one), and a Wayland compositor never tells its client that a
/// window was minimized, so the rows where a minimized window also says
/// something else name those systems' modes apart.
#[cfg(test)]
mod mapping {
    use flui_foundation::geometry::{Bounds, Point, Size};
    use flui_platform::WindowMode::{self, Fullscreen, Maximized, Minimized, Normal};

    use super::{ModeFlags, mode_of};
    use crate::window::apple::geometry::mode::WindowFlags;
    use crate::window::linux::wayland::configure::ToplevelStates;
    use crate::window::linux::x11::mode::{NetWmState, WmState, mode_flags};
    use crate::window::windows::mode::NativeMode;

    /// One window as its window system reports it, and the mode it is in.
    struct Row {
        case: &'static str,
        said: ModeFlags,
        mode: WindowMode,
        /// The mode AppKit reads from the same flags.
        appkit: WindowMode,
        /// The mode a Wayland client reads from the states its compositor
        /// configures for the same window.
        wayland: WindowMode,
    }

    const fn row(
        case: &'static str,
        (minimized, fullscreen, maximized): (bool, bool, bool),
        mode: WindowMode,
        [appkit, wayland]: [WindowMode; 2],
    ) -> Row {
        Row {
            case,
            said: ModeFlags {
                minimized,
                fullscreen,
                maximized,
            },
            mode,
            appkit,
            wayland,
        }
    }

    const ROWS: [Row; 8] = [
        row(
            "nothing set",
            (false, false, false),
            Normal,
            [Normal, Normal],
        ),
        row(
            "maximized",
            (false, false, true),
            Maximized,
            [Maximized, Maximized],
        ),
        row(
            "fullscreen",
            (false, true, false),
            Fullscreen,
            [Fullscreen, Fullscreen],
        ),
        row(
            "minimized",
            (true, false, false),
            Minimized,
            [Minimized, Normal],
        ),
        row(
            "fullscreen that kept its maximized flag",
            (false, true, true),
            Fullscreen,
            [Fullscreen, Fullscreen],
        ),
        row(
            "minimized from fullscreen",
            (true, true, false),
            Minimized,
            [Fullscreen, Fullscreen],
        ),
        row(
            "minimized from maximized",
            (true, false, true),
            Minimized,
            [Minimized, Maximized],
        ),
        row(
            "all three",
            (true, true, true),
            Minimized,
            [Fullscreen, Fullscreen],
        ),
    ];

    /// The states a Wayland compositor configures for such a window: the
    /// maximized and fullscreen states as they stand, and nothing of its
    /// being minimized, which xdg-shell has no state for.
    fn wayland(said: ModeFlags) -> ModeFlags {
        ToplevelStates {
            maximized: said.maximized,
            fullscreen: said.fullscreen,
            ..ToplevelStates::default()
        }
        .mode_flags()
    }

    /// The properties an X11 window manager keeps for such a window:
    /// `WM_STATE` iconic and `_NET_WM_STATE_HIDDEN` for a minimized one, the
    /// fullscreen atom, and both maximized atoms.
    fn x11(said: ModeFlags) -> ModeFlags {
        let wm_state = if said.minimized {
            WmState::Iconic
        } else {
            WmState::Normal
        };
        mode_flags(
            Some(wm_state),
            NetWmState {
                hidden: said.minimized,
                maximized_horz: said.maximized,
                maximized_vert: said.maximized,
                fullscreen: said.fullscreen,
            },
        )
    }

    /// AppKit's flags for such a window as they stand: the full-screen style
    /// mask, `isMiniaturized`, and a zoomed resizable window for a maximized
    /// one.
    fn appkit(said: ModeFlags) -> ModeFlags {
        WindowFlags {
            on_screen: !said.minimized,
            fullscreen: said.fullscreen,
            miniaturized: said.minimized,
            zoomed: said.maximized,
            resizable: true,
        }
        .mode_flags()
    }

    /// The mode a Win32 window tracks for such a window: a fullscreen window
    /// keeps the rectangle it returns to, also while it is minimized, and a
    /// fullscreen window's maximized flag is its own business.
    fn win32(said: ModeFlags) -> ModeFlags {
        let back = Bounds::new(Point::new(10, 20), Size::new(800, 600));
        let mode = if said.minimized {
            NativeMode::Minimized {
                from_fullscreen: said.fullscreen.then_some(back),
            }
        } else if said.fullscreen {
            NativeMode::Fullscreen {
                restore_bounds: back,
            }
        } else if said.maximized {
            NativeMode::Maximized
        } else {
            NativeMode::Normal
        };
        mode.flags()
    }

    /// Every window system decides a row's mode by the one rule from its own
    /// reading; `said` is the flags as they stand.
    #[test]
    fn every_window_system_decides_the_mode_alike() {
        for Row {
            case,
            said,
            mode,
            appkit: appkit_mode,
            wayland: wayland_mode,
        } in ROWS
        {
            let modes = [
                ("flags", mode_of(said), mode),
                ("X11", mode_of(x11(said)), mode),
                ("Win32", mode_of(win32(said)), mode),
                ("AppKit", mode_of(appkit(said)), appkit_mode),
                ("Wayland", mode_of(wayland(said)), wayland_mode),
            ];
            for (system, decided, expected) in modes {
                assert_eq!(decided, expected, "{system}: {case}");
            }
        }
    }
}

#[cfg(test)]
mod extent {
    use flui_foundation::geometry::{DevicePixelRatio, Size};
    use flui_platform::MAX_SURFACE_EXTENT;

    use super::{DeviceLimits, device_extent, device_limits};

    /// The limits every host hands its window system: a minimum up, a
    /// maximum down, and the pair ordered by construction, so rounding never
    /// sends a maximum below the minimum.
    #[test]
    fn device_limits_round_outward_and_stay_ordered() {
        let ratio = |ratio: f64| DevicePixelRatio::try_new(ratio).expect("a positive ratio");
        let square = |side: f64| Some(Size::new(side, side));
        // (case, min, max, ratio, limits)
        for (case, min, max, scale, limits) in [
            (
                "whole limits",
                square(300.0),
                square(900.0),
                1.0,
                (Some((300, 300)), Some((900, 900))),
            ),
            (
                "a fractional ratio rounds outward",
                square(100.3),
                square(500.3),
                1.5,
                (Some((151, 151)), Some((750, 750))),
            ),
            (
                "equal fractional limits hold the window at the minimum",
                square(100.3),
                square(100.3),
                1.5,
                (Some((151, 151)), Some((151, 151))),
            ),
            (
                "a maximum just above the minimum is raised to it",
                square(100.3),
                square(100.4),
                1.0,
                (Some((101, 101)), Some((101, 101))),
            ),
            (
                "a limit with a side that is no size is no limit",
                Some(Size::new(f64::NAN, 10.0)),
                square(100.0),
                1.0,
                (None, Some((100, 100))),
            ),
            ("no limits", None, None, 1.0, (None, None)),
        ] {
            assert_eq!(
                device_limits(min, max, ratio(scale)),
                DeviceLimits {
                    min: limits.0,
                    max: limits.1,
                },
                "{case}"
            );
        }
    }

    /// The one logical-to-device extent: snapped, at least a pixel, clamped
    /// at the largest surface, and no extent for a side that is none.
    #[test]
    fn a_logical_side_becomes_a_device_extent_every_host_can_draw() {
        let ratio = |ratio: f64| DevicePixelRatio::try_new(ratio).expect("a positive ratio");
        let max = f64::from(MAX_SURFACE_EXTENT);
        for (case, logical, scale, extent) in [
            ("at one", 800.0, 1.0, Some(800)),
            ("at a fractional scale", 401.0, 1.5, Some(602)),
            ("a half pixel rounds up", 0.5, 5.0, Some(3)),
            ("a sliver is one pixel", 0.01, 1.0, Some(1)),
            (
                "just under the largest surface",
                max - 1.0,
                1.0,
                Some(MAX_SURFACE_EXTENT - 1),
            ),
            (
                "one past the largest surface",
                max + 1.0,
                1.0,
                Some(MAX_SURFACE_EXTENT),
            ),
            (
                "the largest surface",
                max / 2.0,
                2.0,
                Some(MAX_SURFACE_EXTENT),
            ),
            (
                "past the largest surface",
                max,
                2.0,
                Some(MAX_SURFACE_EXTENT),
            ),
            ("past every number", f64::MAX, 2.0, Some(MAX_SURFACE_EXTENT)),
            ("no size", 0.0, 1.0, None),
            ("negative", -10.0, 1.0, None),
            ("not a number", f64::NAN, 1.0, None),
            ("infinite", f64::INFINITY, 1.0, None),
        ] {
            assert_eq!(device_extent(logical, ratio(scale)), extent, "{case}");
        }
    }
}
