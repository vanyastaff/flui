//! The Wayland host's loop: the connection to the compositor, the globals it
//! binds, and the owner thread's turns (ADR-0151 §10).
//!
//! Two halves:
//!
//! - `wait`, the pure one: how long a turn may block, by the rule ADR-0249
//!   set for the Android loop. Every producer of work wakes the loop, so it
//!   wakes on no timer of its own, and a wake deadline that came due is turned
//!   into frame requests once, never into a busy loop while no frame serves
//!   it.
//! - `event_loop`: `WaylandLoop` over `wayland-client` on libwayland. The
//!   owner thread blocks in `poll` on the compositor's socket and an `eventfd`
//!   every other producer writes (the owner signal, a redraw asked for on
//!   another thread), reads and dispatches what the compositor sent, then lets
//!   each window carry it out with no lock of the loop held.
//!
//! The loop names no feature. What it keeps of one, it keeps through a hook
//! declared here that the host's table fills (`WaylandHost`): the displays,
//! the seats' input, the clipboard, the Settings portal's preferences, the
//! notice channel and the sources of logind's and the kernel's notices. The
//! input binds the
//! globals it speaks as the compositor announces them, records what its
//! objects report, and delivers it to the windows on each turn before they
//! settle (`WaylandInput`). The last user input the seats report, which an
//! activation token names, is kept here too (`LastInput`): the input feature
//! records it, the window feature reads it. A window's own frame reaches the
//! input the same way (`FramePointer`): the window feature draws it, the
//! input tells it what the pointer does over its surfaces. The clipboard
//! binds `wl_data_device_manager` the same way and keeps a data device on each
//! seat the input holds, which the loop hands it every turn
//! (`WaylandClipboard`); it sets its selection under the last user input. Its
//! windows are the window feature's (`window::linux::wayland`): the loop
//! holds them as `LoopWindow`s and asks each to settle after every read, and
//! the window feature implements the dispatch of its own protocol objects on
//! `WaylandState`.

#[cfg(all(target_os = "linux", feature = "wayland"))]
pub use event_loop::OutputName;
#[cfg(all(
    target_os = "linux",
    feature = "wayland",
    any(test, feature = "testing")
))]
pub(crate) use event_loop::ShownCursor;
#[cfg(all(target_os = "linux", feature = "wayland"))]
pub(crate) use event_loop::{
    FramePointer, LastInput, LiveOutputs, LoopWaker, LoopWindow, MAX_OUTPUTS, OutputFacts,
    PresentationTime, SeatWindow, WaylandClipboard, WaylandDisplays, WaylandHost, WaylandInput,
    WaylandLoop, WaylandState, WindowSeed, WindowTable,
};

pub(crate) mod activation {
    //! What the loop and its windows say of an activation (`xdg-activation`):
    //! the user input a token names, the token itself, and the token a
    //! launcher handed the process.

    #![forbid(unsafe_code)]

    use std::ffi::OsString;

    /// A user input as an activation token names it: the seat it came from,
    /// the serial the compositor gave its event (a key, a button, a touch,
    /// or the keyboard focus entering a surface), and the surface it went to.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct UserInput<Seat, Surface> {
        /// The seat whose device the input came from.
        pub(crate) seat: Seat,
        /// The serial of the input's event.
        pub(crate) serial: u32,
        /// The surface the input went to.
        pub(crate) surface: Surface,
    }

    /// An activation token: what the compositor answered a token request
    /// with, or what a launcher handed the process. Opaque; only the
    /// compositor reads it.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct ActivationToken(String);

    impl ActivationToken {
        /// The token the compositor sent.
        pub(crate) const fn new(token: String) -> Self {
            Self(token)
        }

        /// The token, as `xdg_activation_v1.activate` takes it.
        pub(crate) fn into_string(self) -> String {
            self.0
        }
    }

    /// The variable a launcher hands its process's activation token in.
    pub(crate) const LAUNCH_TOKEN_VARIABLE: &str = "XDG_ACTIVATION_TOKEN";

    /// The token a launcher handed the process, from the value of
    /// [`LAUNCH_TOKEN_VARIABLE`]: none where it is unset, empty or not text.
    pub(crate) fn launch_token(value: Option<OsString>) -> Option<ActivationToken> {
        value
            .and_then(|value| value.into_string().ok())
            .filter(|token| !token.is_empty())
            .map(ActivationToken)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A launcher's token is the variable's text; unset, empty or not
        /// text is none.
        #[test]
        fn a_launch_token_is_the_variables_text() {
            assert_eq!(
                launch_token(Some("launcher-1".into())),
                Some(ActivationToken::new("launcher-1".to_owned()))
            );
            assert_eq!(launch_token(Some(OsString::new())), None);
            assert_eq!(launch_token(None), None);
            assert_eq!(
                LAUNCH_TOKEN_VARIABLE, "XDG_ACTIVATION_TOKEN",
                "the variable launchers set"
            );
            assert_eq!(
                ActivationToken::new("handed".to_owned()).into_string(),
                "handed",
                "the compositor gets back what it sent"
            );
        }
    }
}

pub(crate) mod wait {
    //! How long one turn of the Wayland loop may block in `poll` (ADR-0249's
    //! rule on this host).
    //!
    //! Every producer of work wakes the loop: the compositor's socket, the
    //! owner signal and a redraw asked for on another thread write what `poll`
    //! waits on. So the loop blocks until woken unless a window has a frame to
    //! deliver now, or the application's handler named a wake deadline.
    //!
    //! A deadline that came due is served by a frame: the loop asks every
    //! window for one, once, and arms the same instant again only after a
    //! frame ran ([`WakeDeadline`](crate::owner::wake_deadline::WakeDeadline),
    //! the rule the X11 and Win32 loops follow). Asking again with no frame
    //! since would turn a window that declines its frames, or one the
    //! compositor does not draw, into a busy loop. Only the deadline armed
    //! bounds the wait.

    #![forbid(unsafe_code)]

    use std::time::Duration;

    use web_time::Instant;

    /// What one turn of the loop knows when it chooses how long to block.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct LoopTurn {
        /// A window has a frame to deliver now.
        pub(crate) frame_due: bool,
        /// The earliest instant a window gives up on a frame callback that
        /// never came, with a frame asked for behind it.
        pub(crate) frame_wait: Option<Instant>,
        /// When the input next delivers something no event brings: a held
        /// key's repeat.
        pub(crate) input_due: Option<Instant>,
        /// The wake deadline armed: one the application's handler asked for
        /// and the last delivery does not hold back. One already due is
        /// delivered on the next turn.
        pub(crate) wake_deadline: Option<Instant>,
    }

    /// How the last flush of the connection went.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Flush {
        /// Every request reached the socket.
        Complete,
        /// The socket was full: requests wait in the connection's buffer.
        Partial,
    }

    /// What the loop does once it has dispatched and flushed, before it
    /// would block.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum BeforePoll {
        /// Dispatching recorded events: the windows carry them out before the
        /// loop sleeps, so the turn ends here and the next applies them.
        Apply,
        /// Nothing waits to be applied: block, on the socket becoming readable
        /// and, after a partial flush, writable, so the rest of the requests
        /// go out.
        Poll {
            /// Wait for the socket to take more requests.
            writable: bool,
        },
    }

    /// What follows `dispatched` events and `flush` before the loop blocks.
    pub(crate) fn before_poll(dispatched: usize, flush: Flush) -> BeforePoll {
        if dispatched > 0 {
            BeforePoll::Apply
        } else {
            BeforePoll::Poll {
                writable: flush == Flush::Partial,
            }
        }
    }

    /// How long `poll` may block; `None` blocks until something wakes the
    /// loop.
    pub(crate) fn poll_timeout(turn: LoopTurn, now: Instant) -> Option<Duration> {
        if turn.frame_due {
            return Some(Duration::ZERO);
        }
        [turn.wake_deadline, turn.frame_wait, turn.input_due]
            .into_iter()
            .flatten()
            .min()
            .map(|instant| instant.saturating_duration_since(now))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// An idle Wayland loop blocks until it is woken: no timer of its own,
        /// a deadline ahead bounds the wait, an armed deadline that came due
        /// ends it at once so the next turn delivers it, and a frame waiting
        /// on a callback that may be lost bounds it too.
        #[test]
        fn an_idle_wayland_loop_blocks_until_it_is_woken() {
            let now = Instant::now();
            let ms = Duration::from_millis;
            let behind = now.checked_sub(ms(40));
            let turn = |frame_due, frame_wait, wake_deadline| LoopTurn {
                frame_due,
                frame_wait,
                input_due: None,
                wake_deadline,
            };
            let repeating = |input_due, wake_deadline| LoopTurn {
                frame_due: false,
                frame_wait: None,
                input_due,
                wake_deadline,
            };
            for (case, turn, expected) in [
                ("idle", turn(false, None, None), None),
                (
                    "a frame to deliver",
                    turn(true, None, None),
                    Some(Duration::ZERO),
                ),
                (
                    "a frame to deliver before a deadline ahead",
                    turn(true, None, Some(now + ms(250))),
                    Some(Duration::ZERO),
                ),
                (
                    "a deadline ahead",
                    turn(false, None, Some(now + ms(250))),
                    Some(ms(250)),
                ),
                (
                    "a deadline that came due",
                    turn(false, None, behind),
                    Some(Duration::ZERO),
                ),
                (
                    "a deadline exactly due",
                    turn(false, None, Some(now)),
                    Some(Duration::ZERO),
                ),
                (
                    "a frame waits on a callback that may be lost",
                    turn(false, Some(now + ms(700)), None),
                    Some(ms(700)),
                ),
                (
                    "the earlier of a lost callback and a deadline",
                    turn(false, Some(now + ms(700)), Some(now + ms(100))),
                    Some(ms(100)),
                ),
                (
                    "a held key repeats",
                    repeating(Some(now + ms(33)), None),
                    Some(ms(33)),
                ),
                (
                    "a repeat that came due",
                    repeating(behind, None),
                    Some(Duration::ZERO),
                ),
                (
                    "the earlier of a repeat and a deadline",
                    repeating(Some(now + ms(33)), Some(now + ms(500))),
                    Some(ms(33)),
                ),
            ] {
                assert_eq!(poll_timeout(turn, now), expected, "{case}");
            }
        }

        /// Events a dispatch recorded are carried out before the loop sleeps,
        /// and a flush the socket cut short is finished when the socket takes
        /// more: neither waits for an unrelated wake.
        #[test]
        fn the_loop_sleeps_only_with_nothing_to_apply_or_send() {
            for (case, dispatched, flush, expected) in [
                (
                    "nothing recorded, all sent",
                    0,
                    Flush::Complete,
                    BeforePoll::Poll { writable: false },
                ),
                (
                    "nothing recorded, the socket full",
                    0,
                    Flush::Partial,
                    BeforePoll::Poll { writable: true },
                ),
                ("events recorded", 3, Flush::Complete, BeforePoll::Apply),
                (
                    "events recorded, the socket full",
                    1,
                    Flush::Partial,
                    BeforePoll::Apply,
                ),
            ] {
                assert_eq!(before_poll(dispatched, flush), expected, "{case}");
            }
        }
    }
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
mod event_loop {
    use std::os::fd::OwnedFd;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Weak};
    use std::thread::ThreadId;
    use std::time::Duration;

    use cursor_icon::CursorIcon;
    use flui_platform::{
        CursorError, DisplayId, DisplaySnapshot, SessionEndAnswer, SessionEndPhase, SystemNotice,
        WindowId,
    };
    use parking_lot::Mutex;
    use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll};
    use rustix::io::Errno;
    use wayland_client::backend::WaylandError;
    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::wl_compositor::WlCompositor;
    use wayland_client::protocol::wl_output::{self, WlOutput};
    use wayland_client::protocol::wl_registry::{self, WlRegistry};
    use wayland_client::protocol::wl_seat::WlSeat;
    use wayland_client::protocol::wl_shm::{self, WlShm};
    use wayland_client::protocol::wl_subcompositor::{self, WlSubcompositor};
    use wayland_client::protocol::wl_surface::WlSurface;
    use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
    use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1::{
        self, WpFractionalScaleManagerV1,
    };
    use wayland_protocols::wp::presentation_time::client::wp_presentation::{self, WpPresentation};
    use wayland_protocols::wp::viewporter::client::wp_viewporter::{self, WpViewporter};
    use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::{
        self, XdgActivationV1,
    };
    use wayland_protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1::{
        self, ZxdgDecorationManagerV1,
    };
    use wayland_protocols::xdg::foreign::zv2::client::zxdg_exporter_v2::{self, ZxdgExporterV2};
    use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};
    use web_time::Instant;

    use super::activation::{ActivationToken, LAUNCH_TOKEN_VARIABLE, UserInput, launch_token};
    use super::wait::{BeforePoll, Flush, LoopTurn, before_poll, poll_timeout};
    use crate::error::PlatformError;
    use crate::executor::BackgroundExecutor;
    use crate::host;
    use crate::owner::app_delivery::AppDelivery;
    use crate::owner::linux::{LinuxNotices, LinuxPreferences, PortalSlot, SessionLoop, xdg_open};
    use crate::owner::notices::{LifecycleHooks, NoticeChannel, StaleOnWake};
    use crate::owner::panic_boundary::contain_owner_callback;
    use crate::owner::signal::OwnerSignal;
    use crate::owner::slot::{OwnerCell, OwnerMap};
    use crate::owner::wake_deadline::{FrameCount, WakeDeadline};
    use crate::owner::watch::Watch;
    use crate::owner::window_delivery::WindowDelivery;
    use crate::owner::{DirectOwnerHooks, OpenWindowError, OwnerHooks, OwnerPlatform};
    use crate::platform::{AppHandler, Platform, PlatformExecutor};

    /// The newest `wl_compositor` the loop understands: version 6 gives a
    /// surface its preferred buffer scale.
    const COMPOSITOR_VERSION: u32 = 6;
    /// The newest `xdg_wm_base`: version 5 announces what the compositor lets
    /// a toplevel do, version 6 says when it is suspended.
    const WM_BASE_VERSION: u32 = 6;
    /// The newest `wl_output`; the loop binds every output only so that a
    /// surface's `enter` and `leave` name one.
    const OUTPUT_VERSION: u32 = 4;
    /// The outputs the loop binds. A desk has a few screens and a video wall
    /// a few dozen; a compositor announcing outputs without end is not bound
    /// past this, and a surface on one of those names no display.
    pub(crate) const MAX_OUTPUTS: usize = 64;
    /// `wp_viewporter` has one version.
    const VIEWPORTER_VERSION: u32 = 1;
    /// `zxdg_decoration_manager_v1` has one version.
    const DECORATION_VERSION: u32 = 1;
    /// `wp_fractional_scale_manager_v1` has one version.
    const FRACTIONAL_SCALE_VERSION: u32 = 1;
    /// `zxdg_exporter_v2` has one version.
    const EXPORTER_VERSION: u32 = 1;
    /// `wp_presentation` version 1; version 2 only lets a compositor round
    /// its `refresh`.
    const PRESENTATION_VERSION: u32 = 1;
    /// `xdg_activation_v1` has one version.
    const ACTIVATION_VERSION: u32 = 1;

    /// What the host's table fills for the loop (ADR-0151 §10): the
    /// displays, the preference source, the notice channel and the sources
    /// of the host's notices. The loop reaches each through the hook here
    /// and names no feature.
    pub(crate) trait WaylandHost: 'static {
        /// The displays: the compositor's outputs, read on a connection of
        /// their own.
        fn displays() -> Arc<dyn WaylandDisplays>;

        /// The seats' input, made on the owner thread, which alone reaches
        /// it, recording each user input it reports in `last_input`, which
        /// the windows' activation tokens name.
        fn input(last_input: LastInput) -> Arc<dyn WaylandInput>;

        /// The clipboard, made on the owner thread, which sets its selection
        /// under the user input `last_input` records last.
        fn clipboard(last_input: LastInput) -> Arc<dyn WaylandClipboard>;

        /// The channel the loop's system notices take to the application.
        fn notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel>;

        /// The Settings portal's preferences, observed on `executor` and
        /// waking the owner behind `signal`; started as the loop runs.
        fn preferences(
            executor: &BackgroundExecutor,
            signal: &Arc<OwnerSignal>,
        ) -> Arc<dyn LinuxPreferences>;

        /// The sources of the host's system notices (logind, the kernel's
        /// memory pressure), started once the owner can deliver what they
        /// queue.
        fn host_notices(
            executor: &BackgroundExecutor,
            signal: &Arc<OwnerSignal>,
        ) -> Arc<dyn LinuxNotices>;
    }

    /// The compositor's outputs as the loop and its windows reach them.
    pub(crate) trait WaylandDisplays: StaleOnWake {
        /// The snapshot: the last one published while nothing changed, the
        /// outputs read again otherwise.
        ///
        /// # Errors
        /// [`PlatformError::Displays`] when the compositor cannot be read.
        fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError>;

        /// The display and scale of `output`, as the last snapshot holds it;
        /// `None` for an output it does not hold yet, which marks the
        /// snapshot stale.
        fn output(&self, output: OutputName) -> Option<OutputFacts>;

        /// Keep the outputs current, waking the owner behind `signal` when
        /// they change, until the watch is dropped.
        fn observe(self: Arc<Self>, signal: &Arc<OwnerSignal>) -> Watch;
    }

    /// A window as the seats' input reaches it, from its open to its close:
    /// its identity, the surface the compositor names it by, and the delivery
    /// its input goes through.
    pub(crate) struct SeatWindow {
        /// The window's identity.
        pub(crate) id: WindowId,
        /// Its `wl_surface`.
        pub(crate) surface: WlSurface,
        /// What its input is delivered through.
        pub(crate) delivery: Arc<WindowDelivery>,
        /// Its device pixels per logical pixel now, which a theme cursor
        /// over it is drawn at.
        pub(crate) scale: Arc<dyn Fn() -> f64 + Send + Sync>,
        /// The frame it draws itself where the compositor draws none, which
        /// the pointer over the frame's surfaces reaches instead of the
        /// window's input.
        pub(crate) frame: Arc<dyn FramePointer>,
    }

    /// A window's own frame as the seats' input reaches it: the window
    /// feature's, which draws it. The input asks it whether a surface is the
    /// frame's and tells it what the pointer does there; the frame records
    /// it, and the window carries it out as it settles. Called by a dispatch
    /// on the owner thread, with no cell of the input borrowed; it runs no
    /// handler and calls back into nothing.
    pub(crate) trait FramePointer: Send + Sync {
        /// Whether `surface` is one of the frame's.
        fn holds(&self, surface: &WlSurface) -> bool;

        /// The pointer is at (`x`, `y`), surface-local, on `surface`, one of
        /// the frame's: the cursor it shows there.
        fn moved(&self, surface: &WlSurface, x: f64, y: f64) -> CursorIcon;

        /// The pointer left the frame's surfaces.
        fn left(&self);

        /// Button `code` (evdev) of `seat`'s pointer went down or up over the
        /// frame at `time` (the compositor's milliseconds), as the event
        /// numbered `serial`: a move, a resize or a window menu it starts
        /// names that seat and serial.
        fn button(&self, seat: &WlSeat, serial: u32, time: u32, code: u32, pressed: bool);
    }

    /// The cursor the input last showed, for a test.
    #[cfg(any(test, feature = "testing"))]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct ShownCursor {
        /// The name it was found by: a `cursor-shape` value's number, or the
        /// theme cursor's name.
        pub(crate) name: String,
        /// The buffer scale a theme cursor was drawn at; `0` for a shape.
        pub(crate) scale: i32,
    }

    /// What the loop and its windows ask of the seats' input: the input
    /// feature's (`input::linux::wayland`), which the table registers.
    ///
    /// Reached on the owner thread. The dispatch of the input's objects
    /// records what they report and runs no handler; [`Self::deliver`] runs
    /// the handlers, with no lock of the loop held.
    pub(crate) trait WaylandInput: Send + Sync {
        /// The compositor announced global `name`: bind it if the input
        /// speaks its interface.
        fn global(
            &self,
            registry: &WlRegistry,
            name: u32,
            interface: &str,
            version: u32,
            queue: &QueueHandle<WaylandState>,
        );

        /// The compositor removed global `name`.
        fn global_removed(&self, name: u32);

        /// Deliver to the windows what the seats reported since the last
        /// turn, and a held key's repeat that came due.
        fn deliver(&self);

        /// When the input next delivers something no event brings.
        fn next_due(&self) -> Option<Instant>;

        /// A window opened: its input starts.
        fn attach(&self, window: SeatWindow);

        /// The window `id` closed: what it held open ends, and nothing more
        /// reaches it.
        fn detach(&self, id: WindowId);

        /// Show `cursor` while the pointer is over the window `id`.
        ///
        /// # Errors
        /// [`CursorError::Unsupported`] when the compositor takes no cursor
        /// shape from the host.
        fn set_cursor(&self, id: WindowId, cursor: CursorIcon) -> Result<(), CursorError>;

        /// The active keyboard layout's name; empty before a keymap came.
        fn keyboard_layout(&self) -> String;

        /// Call `callback` on the owner thread each time the layout changes.
        fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut() + Send>);

        /// The seats the input holds now, which the clipboard keeps a data
        /// device on.
        fn seats(&self) -> Vec<WlSeat>;

        /// The loop ends: let go of the windows and the callbacks.
        fn release(&self);

        /// The cursor the input last showed, for a test.
        #[cfg(any(test, feature = "testing"))]
        fn shown_cursor(&self) -> Option<ShownCursor>;
    }

    /// What the loop asks of the clipboard: the clipboard feature's
    /// (`clipboard::linux::wayland`), which the table registers, and what
    /// [`Platform::clipboard`] hands out, with the primary selection
    /// [`Platform::primary_selection`] hands out where the compositor has
    /// one.
    ///
    /// Reached on the owner thread. The dispatch of its objects records what
    /// they report and runs no handler; a read answers through its resolver,
    /// and a request for the application's primary selection waits for
    /// [`deliver`](Self::deliver).
    pub(crate) trait WaylandClipboard:
        host::NativeClipboard + host::NativePrimarySelection + Send + Sync
    {
        /// Whether the compositor has a primary selection the clipboard
        /// bound.
        fn has_primary_selection(&self) -> bool;

        /// Answer the requests for the application's primary selection the
        /// dispatch recorded, asking its text with no lock of the loop or of
        /// the clipboard held.
        fn deliver(&self);

        /// The compositor announced global `name`: bind it if the clipboard
        /// speaks its interface.
        fn global(
            &self,
            registry: &WlRegistry,
            name: u32,
            interface: &str,
            version: u32,
            queue: &QueueHandle<WaylandState>,
        );

        /// The compositor removed global `name`.
        fn global_removed(&self, name: u32);

        /// The seats the input holds now: each has a data device, and a seat
        /// gone lets go of its own.
        fn seats(&self, seats: &[WlSeat]);

        /// The loop ends: let go of the devices and the selection.
        fn release(&self);
    }

    /// An output by its global's name, which the compositor numbers alike for
    /// every client: what a surface's `enter` names, and what the display
    /// source keys an output by. Public where it is defined, as the testing
    /// kit names it, and nameable only through it outside the crate.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct OutputName(u32);

    impl OutputName {
        /// The output whose global is `name`.
        pub(crate) const fn new(name: u32) -> Self {
            Self(name)
        }

        /// The global's name.
        pub(crate) const fn get(self) -> u32 {
            self.0
        }
    }

    /// `wp_presentation`, through which a surface asks when each commit is
    /// shown, and the clock the compositor stamps those times on.
    #[derive(Clone)]
    pub(crate) struct PresentationTime {
        /// The global, which makes a feedback object per commit.
        pub(crate) manager: WpPresentation,
        /// The clock its `presented` events are stamped on.
        pub(crate) clock: PresentationClock,
    }

    /// The clock a compositor stamps presentation times on, as its
    /// `wp_presentation.clock_id` named it (`CLOCK_MONOTONIC` on every
    /// compositor known); unknown until it has.
    #[derive(Clone, Debug, Default)]
    pub(crate) struct PresentationClock(Arc<std::sync::OnceLock<u32>>);

    impl PresentationClock {
        /// The clock's reading now, from its epoch; `None` before the
        /// compositor named it, or for a clock this process cannot read.
        pub(crate) fn now(&self) -> Option<Duration> {
            let id = i32::try_from(*self.0.get()?).ok()?;
            let clock = rustix::time::ClockId::try_from(id).ok()?;
            let now = rustix::time::clock_gettime(clock);
            Some(Duration::new(
                u64::try_from(now.tv_sec).ok()?,
                u32::try_from(now.tv_nsec).ok()?,
            ))
        }
    }

    impl Dispatch<WpPresentation, PresentationClock> for WaylandState {
        fn event(
            _: &mut Self,
            _: &WpPresentation,
            event: wp_presentation::Event,
            clock: &PresentationClock,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // Sent once, right after the bind.
            if let wp_presentation::Event::ClockId { clk_id } = event {
                let _ = clock.0.set(clk_id);
            }
        }
    }

    /// The outputs the compositor announces now, which the registry keeps
    /// current on the owner thread: an output removed while a surface was on
    /// it sends that surface no `leave`.
    #[derive(Clone, Default)]
    pub(crate) struct LiveOutputs(Arc<OwnerCell<Vec<OutputName>>>);

    impl LiveOutputs {
        /// Whether `output` is still announced; `false` off the owner thread.
        pub(crate) fn contains(&self, output: OutputName) -> bool {
            self.0.with(|live| live.contains(&output)).unwrap_or(false)
        }

        /// Every output announced now; empty off the owner thread.
        #[cfg(any(test, feature = "testing"))]
        pub(crate) fn names(&self) -> Vec<OutputName> {
            self.0.with(|live| live.clone()).unwrap_or_default()
        }

        fn add(&self, output: OutputName) {
            self.0.with(|live| {
                if !live.contains(&output) {
                    live.push(output);
                }
            });
        }

        fn remove(&self, output: OutputName) {
            self.0.with(|live| live.retain(|held| *held != output));
        }
    }

    /// A user input on this connection, as an activation token names it.
    pub(crate) type SeatInput = UserInput<WlSeat, WlSurface>;

    /// The last user input the seats reported, on the owner thread: the seats'
    /// input records it as its events come, and a window names it on the
    /// activation token it asks for. Nothing recorded, a token names no input
    /// and asks the compositor for the user's attention instead of the focus.
    #[derive(Clone, Default)]
    pub(crate) struct LastInput(Arc<OwnerCell<Option<SeatInput>>>);

    impl LastInput {
        /// The seats report `input`, which is now the last: a key, a button
        /// or a touch going down, or the keyboard entering a surface.
        pub(crate) fn record(&self, input: SeatInput) {
            self.0.with(|last| *last = Some(input));
        }

        /// The last input; `None` before any, or off the owner thread.
        pub(crate) fn latest(&self) -> Option<SeatInput> {
            self.0.with(|last| last.clone()).flatten()
        }
    }

    /// What the display snapshot says of one output.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub(crate) struct OutputFacts {
        /// The display it is.
        pub(crate) display: DisplayId,
        /// Its device pixels per logical pixel.
        pub(crate) scale: f64,
    }

    /// What the loop asks of a window it holds: the window feature's
    /// (`window::linux::wayland`), which implements it.
    pub(crate) trait LoopWindow {
        /// Carry out what the compositor and other threads asked of the
        /// window since the last turn: acknowledge a configure, publish the
        /// state, ask about a close and deliver a frame that is due. Calls the
        /// window's handler; the loop calls it with none of its locks held.
        fn settle(&self);

        /// Whether the window has a frame to deliver now.
        fn frame_due(&self) -> bool;

        /// When the window gives up on a frame callback that has not come,
        /// with a frame asked for behind it: a present that was skipped never
        /// commits the callback it armed.
        fn frame_wait(&self) -> Option<Instant>;

        /// A wake deadline came due: ask the window for a frame.
        fn request_frame(&self);

        /// The loop ends: let go of the window's handler and its surface.
        fn release(&self);
    }

    /// The windows a loop holds, by identity, on the owner thread.
    pub(crate) struct WindowTable {
        windows: OwnerMap<WindowId, Rc<dyn LoopWindow>>,
        /// The window the compositor last activated.
        active: OwnerCell<Option<WindowId>>,
        /// The last window closed since the loop last looked: it asks the
        /// application's exit policy.
        emptied: AtomicBool,
        /// The last identity issued.
        issued: AtomicU64,
        /// The activation token the process was launched with, until the
        /// first window is held.
        launch_token: OwnerCell<Option<ActivationToken>>,
    }

    impl WindowTable {
        /// A table whose first window takes `launch_token`; made on the
        /// owner thread.
        fn new(launch_token: Option<ActivationToken>) -> Self {
            let held = OwnerCell::new();
            held.with(|held| *held = launch_token);
            Self {
                windows: OwnerMap::new(),
                active: OwnerCell::new(),
                emptied: AtomicBool::new(false),
                issued: AtomicU64::new(0),
                launch_token: held,
            }
        }

        /// An identity no window of this loop had; `None` once the space is
        /// used up, never wrapped onto a window something still names.
        fn issue(&self) -> Option<WindowId> {
            let previous = self
                .issued
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |issued| {
                    issued.checked_add(1)
                })
                .ok()?;
            previous.checked_add(1).map(WindowId::new)
        }

        /// Hold `window` as `id`, on the owner thread. The first window held
        /// is given the activation token the process was launched with; a
        /// window refused leaves it for the next.
        fn register(
            &self,
            id: WindowId,
            window: Rc<dyn LoopWindow>,
        ) -> Result<Option<ActivationToken>, OpenWindowError> {
            match self.windows.insert(id, window) {
                Ok(None) => Ok(self.launch_token.with(Option::take).flatten()),
                Ok(Some(previous)) => {
                    contain_owner_callback(|| drop(previous));
                    Err(OpenWindowError::Backend {
                        message: format!("the Wayland loop already held a window as {id:?}"),
                    })
                }
                Err(refused) => {
                    contain_owner_callback(|| drop(refused));
                    Err(OpenWindowError::Unavailable {
                        message: "a Wayland window opens on the thread that owns its platform"
                            .to_owned(),
                    })
                }
            }
        }

        /// Let go of the window `id`, which closed. When it was the last, the
        /// loop asks the application's exit policy on its next turn.
        pub(crate) fn forget(&self, id: WindowId) {
            let window = self.windows.remove(&id);
            self.active.with(|active| {
                if *active == Some(id) {
                    *active = None;
                }
            });
            if window.is_some() && self.windows.is_empty() {
                self.emptied.store(true, Ordering::Release);
            }
            // The window's own drop may release what its handler held.
            contain_owner_callback(|| drop(window));
        }

        /// The compositor activated window `id`, or took the activation away.
        pub(crate) fn activation(&self, id: WindowId, activated: bool) {
            self.active.with(|active| {
                if activated {
                    *active = Some(id);
                } else if *active == Some(id) {
                    *active = None;
                }
            });
        }

        /// The active window; `None` off the owner thread.
        fn active(&self) -> Option<WindowId> {
            self.active.with(|active| *active).flatten()
        }

        fn values(&self) -> Vec<Rc<dyn LoopWindow>> {
            self.windows.values()
        }

        fn is_empty(&self) -> bool {
            self.windows.is_empty()
        }

        fn take_emptied(&self) -> bool {
            self.emptied.swap(false, Ordering::AcqRel)
        }
    }

    /// What wakes the owner thread from `poll`: an `eventfd` any thread
    /// writes.
    pub(crate) struct LoopWaker(OwnedFd);

    impl LoopWaker {
        fn try_new() -> rustix::io::Result<Self> {
            eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK).map(Self)
        }

        /// Wake the owner thread. A counter already at its limit has a wake
        /// pending, which is all a write would add.
        pub(crate) fn wake(&self) {
            match rustix::io::write(&self.0, &1_u64.to_ne_bytes()) {
                Ok(_) | Err(Errno::AGAIN) => {}
                Err(error) => tracing::error!(%error, "the Wayland loop could not be woken"),
            }
        }

        /// Clear the wakes that were counted.
        fn drain(&self) {
            let mut count = [0_u8; 8];
            // Empty already (`AGAIN`) is what a drain wants.
            let _ = rustix::io::read(&self.0, &mut count);
        }
    }

    /// What the owner's event queue dispatches into: the globals the loop
    /// binds itself. The window feature implements the dispatch of a window's
    /// own objects (`wl_surface`, `xdg_surface`, `xdg_toplevel` and frame
    /// callbacks) on it, recording into the window's own state.
    pub(crate) struct WaylandState {
        /// Every output the compositor announced, bound so that a surface's
        /// `enter` names it by its global.
        outputs: Vec<(OutputName, WlOutput)>,
        /// The same outputs, as the windows read them.
        live: LiveOutputs,
        /// The seats' input, offered every global the loop does not bind.
        input: Arc<dyn WaylandInput>,
        /// The clipboard, offered the same globals.
        clipboard: Arc<dyn WaylandClipboard>,
    }

    impl WaylandState {
        /// Offer global `name` to the features that bind theirs.
        fn offer_global(
            &self,
            registry: &WlRegistry,
            name: u32,
            interface: &str,
            version: u32,
            queue: &QueueHandle<Self>,
        ) {
            self.input.global(registry, name, interface, version, queue);
            self.clipboard
                .global(registry, name, interface, version, queue);
        }

        fn bind_output(
            &mut self,
            registry: &WlRegistry,
            name: u32,
            version: u32,
            queue: &QueueHandle<Self>,
        ) {
            let name = OutputName::new(name);
            if self.outputs.iter().any(|(bound, _)| *bound == name) {
                return;
            }
            if self.outputs.len() >= MAX_OUTPUTS {
                tracing::debug!(?name, "a Wayland output past {MAX_OUTPUTS} was not bound");
                return;
            }
            let output = registry.bind::<WlOutput, OutputName, Self>(
                name.get(),
                version.min(OUTPUT_VERSION),
                queue,
                name,
            );
            self.live.add(name);
            self.outputs.push((name, output));
        }

        fn remove_output(&mut self, name: u32) {
            let name = OutputName::new(name);
            self.live.remove(name);
            if let Some(index) = self.outputs.iter().position(|(bound, _)| *bound == name) {
                let (_, output) = self.outputs.remove(index);
                // `release` exists from version 3; below it the proxy is just
                // dropped.
                if output.version() >= 3 {
                    output.release();
                }
            }
        }
    }

    impl Dispatch<WlRegistry, GlobalListContents> for WaylandState {
        fn event(
            state: &mut Self,
            registry: &WlRegistry,
            event: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            queue: &QueueHandle<Self>,
        ) {
            match event {
                wl_registry::Event::Global {
                    name,
                    interface,
                    version,
                } if interface == WlOutput::interface().name => {
                    state.bind_output(registry, name, version, queue);
                }
                wl_registry::Event::Global {
                    name,
                    interface,
                    version,
                } => state.offer_global(registry, name, &interface, version, queue),
                wl_registry::Event::GlobalRemove { name } => {
                    state.remove_output(name);
                    state.input.global_removed(name);
                    state.clipboard.global_removed(name);
                }
                _ => {}
            }
        }
    }

    impl Dispatch<WlOutput, OutputName> for WaylandState {
        fn event(
            _: &mut Self,
            _: &WlOutput,
            _: wl_output::Event,
            _: &OutputName,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // The display source reads the outputs on its own connection; here
            // an output is only a name a surface enters.
        }
    }

    impl Dispatch<WlCompositor, ()> for WaylandState {
        fn event(
            _: &mut Self,
            _: &WlCompositor,
            _: <WlCompositor as Proxy>::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // The compositor has no events.
        }
    }

    impl Dispatch<WpViewporter, ()> for WaylandState {
        fn event(
            _: &mut Self,
            _: &WpViewporter,
            _: wp_viewporter::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // The viewporter has no events.
        }
    }

    impl Dispatch<WlShm, ()> for WaylandState {
        fn event(
            _: &mut Self,
            _: &WlShm,
            _: wl_shm::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // The formats it announces: ARGB8888, which a window's own frame
            // is drawn in, is every compositor's.
        }
    }

    impl Dispatch<WlSubcompositor, ()> for WaylandState {
        fn event(
            _: &mut Self,
            _: &WlSubcompositor,
            _: wl_subcompositor::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // The subcompositor has no events.
        }
    }

    impl Dispatch<ZxdgDecorationManagerV1, ()> for WaylandState {
        fn event(
            _: &mut Self,
            _: &ZxdgDecorationManagerV1,
            _: zxdg_decoration_manager_v1::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // The manager has no events.
        }
    }

    impl Dispatch<WpFractionalScaleManagerV1, ()> for WaylandState {
        fn event(
            _: &mut Self,
            _: &WpFractionalScaleManagerV1,
            _: wp_fractional_scale_manager_v1::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // The manager has no events.
        }
    }

    impl Dispatch<ZxdgExporterV2, ()> for WaylandState {
        fn event(
            _: &mut Self,
            _: &ZxdgExporterV2,
            _: zxdg_exporter_v2::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // The exporter has no events.
        }
    }

    impl Dispatch<XdgActivationV1, ()> for WaylandState {
        fn event(
            _: &mut Self,
            _: &XdgActivationV1,
            _: xdg_activation_v1::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // The activation global has no events; its tokens are the
            // windows'.
        }
    }

    impl Dispatch<XdgWmBase, ()> for WaylandState {
        fn event(
            _: &mut Self,
            wm_base: &XdgWmBase,
            event: xdg_wm_base::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // A client that does not answer is considered hung.
            if let xdg_wm_base::Event::Ping { serial } = event {
                wm_base.pong(serial);
            }
        }
    }

    /// The globals a window is built from, and the queue its objects report
    /// to.
    #[derive(Clone)]
    pub(crate) struct WaylandProtocol {
        /// The connection, which a surface target keeps alive.
        pub(crate) connection: Connection,
        /// The owner's event queue.
        pub(crate) queue: QueueHandle<WaylandState>,
        /// `wl_compositor`, which makes surfaces.
        pub(crate) compositor: WlCompositor,
        /// `xdg_wm_base`, which gives a surface the toplevel role.
        pub(crate) wm_base: XdgWmBase,
        /// `wp_viewporter`, which sizes a surface apart from its buffer;
        /// `None` where the compositor has none.
        pub(crate) viewporter: Option<WpViewporter>,
        /// `wl_subcompositor`, which places a window's own frame around its
        /// content; `None` where the compositor has none.
        pub(crate) subcompositor: Option<WlSubcompositor>,
        /// `wl_shm`, the shared memory a window's own frame is drawn in (and
        /// a test's frames, `testing::wayland::present`); `None` where the
        /// compositor has none.
        pub(crate) shm: Option<WlShm>,
        /// `zxdg_decoration_manager_v1`, through which a window asks the
        /// compositor to draw its frame; `None` where the compositor draws
        /// none.
        pub(crate) decoration: Option<ZxdgDecorationManagerV1>,
        /// `wp_fractional_scale_manager_v1`, which tells a surface the scale
        /// the compositor prefers to the 120th. Whether the window draws at
        /// it is `scale::buffer_scale`'s to decide: only through a viewport.
        pub(crate) fractional_scale: Option<WpFractionalScaleManagerV1>,
        /// `zxdg_exporter_v2`, which hands out a handle another client
        /// (a portal's dialog) parents its window to.
        pub(crate) exporter: Option<ZxdgExporterV2>,
        /// `wp_presentation`, which tells a surface when its commits are
        /// shown; `None` where the compositor tells nothing.
        pub(crate) presentation: Option<PresentationTime>,
        /// `xdg_activation_v1`, through which a window asks the compositor to
        /// activate it; `None` where the compositor takes no such request.
        pub(crate) activation: Option<XdgActivationV1>,
        /// The last user input the seats reported, which an activation token
        /// names.
        pub(crate) last_input: LastInput,
        /// The outputs the compositor announces now.
        pub(crate) live: LiveOutputs,
    }

    /// What a window opens with, which the loop hands the window feature
    /// (`WaylandLoop::seed`).
    pub(crate) struct WindowSeed {
        /// The window's identity.
        pub(crate) id: WindowId,
        /// The globals and the owner's queue.
        pub(crate) protocol: WaylandProtocol,
        /// The displays, by which the window names its output.
        pub(crate) displays: Arc<dyn WaylandDisplays>,
        /// The portal's colour scheme.
        pub(crate) portal: PortalSlot,
        /// The loop's windows, which the window leaves when it closes.
        pub(crate) table: Weak<WindowTable>,
        /// The owner signal, which delivers what other threads raise.
        pub(crate) signal: Arc<OwnerSignal>,
        /// What a redraw asked for on another thread wakes.
        pub(crate) waker: Weak<LoopWaker>,
        /// The channel a wake from sleep marks the window's state stale
        /// through.
        pub(crate) notices: Arc<dyn NoticeChannel>,
        /// The loop's frame count, which the window's handler counts into
        /// each frame it produces.
        pub(crate) frames: Arc<AtomicU64>,
        /// The seats' input, which the window joins as it opens and leaves
        /// as it closes, and which shows its cursor.
        pub(crate) input: Arc<dyn WaylandInput>,
        /// The loop's background work, on which a window's own frame has
        /// the host's fonts read once.
        pub(crate) executor: Arc<BackgroundExecutor>,
    }

    /// The owner's event queue and what it dispatches into, kept together:
    /// dispatching needs both.
    struct Dispatcher {
        queue: EventQueue<WaylandState>,
        state: WaylandState,
    }

    /// What the loop starts as it runs and stops as it ends.
    #[derive(Default)]
    struct Sources {
        preferences: Option<Arc<dyn LinuxPreferences>>,
        host_notices: Option<Arc<dyn LinuxNotices>>,
    }

    /// The Wayland host's loop: the connection to the compositor and what the
    /// owner thread keeps for the positions of its table.
    pub struct WaylandLoop {
        connection: Connection,
        /// The event queue: owner-only, so a dispatch re-entered from a
        /// handler fails loudly instead of waiting on itself.
        dispatcher: OwnerCell<Option<Dispatcher>>,
        protocol: WaylandProtocol,
        waker: Arc<LoopWaker>,
        signal: Arc<OwnerSignal>,
        app: Arc<AppDelivery>,
        windows: Arc<WindowTable>,
        /// The thread that made the platform, which owns its windows and
        /// runs its loop.
        owner_thread: ThreadId,
        displays: Arc<dyn WaylandDisplays>,
        input: Arc<dyn WaylandInput>,
        clipboard: Arc<dyn WaylandClipboard>,
        notices: Arc<dyn NoticeChannel>,
        portal: PortalSlot,
        start_preferences: fn(&BackgroundExecutor, &Arc<OwnerSignal>) -> Arc<dyn LinuxPreferences>,
        start_host_notices: fn(&BackgroundExecutor, &Arc<OwnerSignal>) -> Arc<dyn LinuxNotices>,
        /// The sources the loop keeps while it runs.
        sources: Mutex<Sources>,
        /// The display watch, which the owner thread starts and stops.
        display_watch: OwnerCell<Option<Watch>>,
        executor: Arc<BackgroundExecutor>,
        /// The wake deadline armed and the last one delivered.
        deadline: OwnerCell<WakeDeadline>,
        /// How many frames the windows' handlers produced, which tells the
        /// deadline a delivery a frame served from one that reached nothing.
        frames: FrameCount,
        /// The exit policy is asked again on the next turn.
        reevaluate: AtomicBool,
        /// The loop ran once; a platform runs once.
        ran: AtomicBool,
    }

    impl std::fmt::Debug for WaylandLoop {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WaylandLoop").finish_non_exhaustive()
        }
    }

    /// The connection was lost, or a request the loop made failed.
    fn lost(error: impl std::fmt::Display) -> PlatformError {
        PlatformError::EventLoop {
            message: format!("the Wayland connection: {error}"),
        }
    }

    /// What a file prompt answers on this host: the file chooser portal is
    /// not spoken here yet.
    fn no_file_prompt() -> PlatformError {
        flui_platform::Unsupported::new(flui_platform::UnsupportedReason::NotYetOnThisHost).into()
    }

    /// `duration` as `poll` takes it.
    fn timespec(duration: Duration) -> Timespec {
        Timespec {
            tv_sec: i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
            tv_nsec: duration.subsec_nanos().into(),
        }
    }

    impl WaylandLoop {
        /// Connect to the compositor the environment names and bind its
        /// globals, for host `H`, whose table makes the displays, the notice
        /// channel and the sources the loop starts.
        ///
        /// # Errors
        /// [`PlatformError::Init`] when no compositor answers, libwayland
        /// cannot be loaded, or the compositor lacks `wl_compositor` or
        /// `xdg_wm_base`.
        pub(crate) fn try_new<H: WaylandHost>(socket: &Path) -> Result<Self, PlatformError> {
            let init = |what: &str, error: &dyn std::fmt::Display| PlatformError::Init {
                message: format!("Wayland: {what}: {error}"),
            };
            let stream = std::os::unix::net::UnixStream::connect(socket)
                .map_err(|error| init(&format!("no compositor at {}", socket.display()), &error))?;
            let connection =
                Connection::from_socket(stream).map_err(|error| init("the connection", &error))?;
            let (globals, queue) = registry_queue_init::<WaylandState>(&connection)
                .map_err(|error| init("the registry", &error))?;
            let handle = queue.handle();
            let compositor = globals
                .bind::<WlCompositor, _, _>(&handle, 1..=COMPOSITOR_VERSION, ())
                .map_err(|error| init("wl_compositor", &error))?;
            let wm_base = globals
                .bind::<XdgWmBase, _, _>(&handle, 1..=WM_BASE_VERSION, ())
                .map_err(|error| init("xdg_wm_base", &error))?;
            let viewporter = globals
                .bind::<WpViewporter, _, _>(&handle, 1..=VIEWPORTER_VERSION, ())
                .ok();
            let decoration = globals
                .bind::<ZxdgDecorationManagerV1, _, _>(&handle, 1..=DECORATION_VERSION, ())
                .ok();
            let fractional_scale = globals
                .bind::<WpFractionalScaleManagerV1, _, _>(&handle, 1..=FRACTIONAL_SCALE_VERSION, ())
                .ok();
            let exporter = globals
                .bind::<ZxdgExporterV2, _, _>(&handle, 1..=EXPORTER_VERSION, ())
                .ok();
            let presentation = {
                let clock = PresentationClock::default();
                globals
                    .bind::<WpPresentation, _, _>(&handle, 1..=PRESENTATION_VERSION, clock.clone())
                    .ok()
                    .map(|manager| PresentationTime { manager, clock })
            };
            let activation = globals
                .bind::<XdgActivationV1, _, _>(&handle, 1..=ACTIVATION_VERSION, ())
                .ok();
            let subcompositor = globals
                .bind::<WlSubcompositor, _, _>(&handle, 1..=1, ())
                .ok();
            let shm = globals.bind::<WlShm, _, _>(&handle, 1..=1, ()).ok();
            let live = LiveOutputs::default();
            let last_input = LastInput::default();
            let input = H::input(last_input.clone());
            let clipboard = H::clipboard(last_input.clone());
            let mut state = WaylandState {
                outputs: Vec::new(),
                live: live.clone(),
                input: Arc::clone(&input),
                clipboard: Arc::clone(&clipboard),
            };
            let announced = globals.contents().with_list(|list| {
                list.iter()
                    .map(|global| (global.name, global.interface.clone(), global.version))
                    .collect::<Vec<_>>()
            });
            for (name, interface, version) in announced {
                if interface == WlOutput::interface().name {
                    state.bind_output(globals.registry(), name, version, &handle);
                } else {
                    state.offer_global(globals.registry(), name, &interface, version, &handle);
                }
            }
            // The seats bound so far get their data devices now: the first
            // window opens before the loop's first turn, and a seat's
            // selection reaches only a device that exists when the keyboard
            // enters.
            clipboard.seats(&input.seats());
            let waker = Arc::new(
                LoopWaker::try_new().map_err(|error| init("the loop's wake descriptor", &error))?,
            );
            let signal = {
                let waker = Arc::downgrade(&waker);
                OwnerSignal::new(Arc::new(move || {
                    if let Some(waker) = waker.upgrade() {
                        waker.wake();
                    }
                    Ok(())
                }))
            };
            let app = Arc::new(AppDelivery::new());
            Ok(Self {
                protocol: WaylandProtocol {
                    connection: connection.clone(),
                    queue: handle,
                    compositor,
                    wm_base,
                    viewporter,
                    subcompositor,
                    shm,
                    decoration,
                    fractional_scale,
                    exporter,
                    presentation,
                    activation,
                    last_input,
                    live,
                },
                connection,
                dispatcher: {
                    let dispatcher = OwnerCell::new();
                    dispatcher.with(|held| *held = Some(Dispatcher { queue, state }));
                    dispatcher
                },
                waker,
                signal,
                notices: H::notices(&app),
                app,
                // The variable stays set: unsetting it races every thread
                // reading the environment, and the opener started for a link
                // leaves it out (`xdg_open`).
                windows: Arc::new(WindowTable::new(launch_token(std::env::var_os(
                    LAUNCH_TOKEN_VARIABLE,
                )))),
                owner_thread: std::thread::current().id(),
                displays: H::displays(),
                input,
                clipboard,
                portal: PortalSlot::default(),
                start_preferences: H::preferences,
                start_host_notices: H::host_notices,
                sources: Mutex::new(Sources::default()),
                display_watch: OwnerCell::new(),
                executor: Arc::new(BackgroundExecutor::new()),
                deadline: OwnerCell::new(),
                frames: FrameCount::default(),
                reevaluate: AtomicBool::new(false),
                ran: AtomicBool::new(false),
            })
        }

        /// What a window opens with: its identity, the globals, and what it
        /// reports to.
        ///
        /// # Errors
        /// [`OpenWindowError::Unavailable`] off the thread that owns the
        /// platform, [`OpenWindowError::OwnerGone`] once the loop has ended,
        /// [`OpenWindowError::Backend`] when the window identities are used
        /// up.
        pub(crate) fn seed(&self) -> Result<WindowSeed, OpenWindowError> {
            if std::thread::current().id() != self.owner_thread {
                return Err(OpenWindowError::Unavailable {
                    message: "a Wayland window opens on the thread that owns its platform, \
                              which its handler lives on"
                        .to_owned(),
                });
            }
            if !self.signal.accepting() {
                return Err(OpenWindowError::OwnerGone { rejected: None });
            }
            let id = self
                .windows
                .issue()
                .ok_or_else(|| OpenWindowError::Backend {
                    message: "the Wayland loop has issued every window identity".to_owned(),
                })?;
            Ok(WindowSeed {
                id,
                protocol: self.protocol.clone(),
                displays: Arc::clone(&self.displays),
                portal: self.portal.clone(),
                table: Arc::downgrade(&self.windows),
                signal: Arc::clone(&self.signal),
                waker: Arc::downgrade(&self.waker),
                notices: Arc::clone(&self.notices),
                frames: self.frames.counter(),
                input: Arc::clone(&self.input),
                executor: Arc::clone(&self.executor),
            })
        }

        /// Hold the window `id` until it closes. The first window held is
        /// given the activation token the process was launched with.
        ///
        /// # Errors
        /// [`OpenWindowError::Unavailable`] off the owner thread.
        pub(crate) fn register(
            &self,
            id: WindowId,
            window: Rc<dyn LoopWindow>,
        ) -> Result<Option<ActivationToken>, OpenWindowError> {
            self.windows.register(id, window)
        }

        /// Send what was asked and wait until the compositor has answered it,
        /// dispatching what it sent meanwhile. Runs no handler: the windows
        /// settle on the loop's next turn.
        ///
        /// # Errors
        /// [`OpenWindowError::Backend`] when the connection failed.
        pub(crate) fn roundtrip(&self) -> Result<(), OpenWindowError> {
            self.dispatching(|queue, state| queue.roundtrip(state).map(drop))
                .map_err(|error| OpenWindowError::Backend {
                    message: error.to_string(),
                })?
                .map_err(|error| OpenWindowError::Backend {
                    message: format!("the Wayland connection: {error}"),
                })
        }

        /// Run `body` on the event queue and what it dispatches into, on the
        /// owner thread. The queue is borrowed for `body` alone, which runs no
        /// handler: a dispatch re-entered from inside one panics on the
        /// borrow rather than waiting on itself.
        ///
        /// # Errors
        /// [`PlatformError::EventLoop`] off the owner thread.
        fn dispatching<R>(
            &self,
            body: impl FnOnce(&mut EventQueue<WaylandState>, &mut WaylandState) -> R,
        ) -> Result<R, PlatformError> {
            self.dispatcher
                .with(|dispatcher| {
                    let Dispatcher { queue, state } = dispatcher
                        .as_mut()
                        .expect("BUG: the Wayland loop holds its event queue from construction");
                    body(queue, state)
                })
                .ok_or_else(|| PlatformError::EventLoop {
                    message: "the Wayland event queue is reached on its owner thread only".into(),
                })
        }

        /// The window the compositor last activated.
        pub(crate) fn active_window(&self) -> Option<WindowId> {
            self.windows.active()
        }

        /// The serial of the last user input the seats reported, which an
        /// activation token names, for a test.
        #[cfg(any(test, feature = "testing"))]
        pub(crate) fn last_input_serial(&self) -> Option<u32> {
            self.protocol.last_input.latest().map(|input| input.serial)
        }

        /// The cursor the seats' input last showed, for a test.
        #[cfg(any(test, feature = "testing"))]
        pub(crate) fn shown_cursor(&self) -> Option<ShownCursor> {
            self.input.shown_cursor()
        }
        /// Whether the compositor offers to draw a window's frame
        /// (`xdg-decoration`).
        pub(crate) fn offers_decorations(&self) -> bool {
            self.protocol.decoration.is_some()
        }

        /// Whether a window can draw a frame of its own: the compositor
        /// offers subsurfaces and shared memory.
        pub(crate) fn offers_frame_surfaces(&self) -> bool {
            self.protocol.subcompositor.is_some() && self.protocol.shm.is_some()
        }

        /// Whether the compositor tells a surface when its commits are shown
        /// (`wp_presentation`).
        pub(crate) fn offers_presentation(&self) -> bool {
            self.protocol.presentation.is_some()
        }
        /// Whether the compositor takes activation requests
        /// (`xdg-activation`).
        pub(crate) fn offers_activation(&self) -> bool {
            self.protocol.activation.is_some()
        }

        /// Run turns for `span` on the owner thread, for a test that drives
        /// the loop's windows without running it.
        ///
        /// # Errors
        /// What a turn reports.
        #[cfg(any(test, feature = "testing"))]
        pub(crate) fn turn_for(&self, span: Duration) -> Result<(), PlatformError> {
            let end = Instant::now() + span;
            loop {
                let now = Instant::now();
                if now >= end || self.turn(Some(end - now))? {
                    return Ok(());
                }
            }
        }

        /// The globals of the outputs the compositor announces now, for a
        /// test that moves a window between them.
        #[cfg(any(test, feature = "testing"))]
        pub(crate) fn outputs(&self) -> Vec<OutputName> {
            self.protocol.live.names()
        }

        /// The loop's binding of `output`, while the compositor announces it;
        /// `None` off the owner thread.
        #[cfg(any(test, feature = "testing"))]
        pub(crate) fn output_proxy(&self, output: OutputName) -> Option<WlOutput> {
            self.dispatching(|_, state| {
                state
                    .outputs
                    .iter()
                    .find(|(name, _)| *name == output)
                    .map(|(_, proxy)| proxy.clone())
            })
            .ok()
            .flatten()
        }

        /// Dispatch what the compositor already sent: the windows record it.
        fn dispatch_pending(&self) -> Result<usize, PlatformError> {
            self.dispatching(EventQueue::dispatch_pending)?
                .map_err(lost)
        }

        /// One turn of the loop, blocking at most `limit`. `true` when the
        /// loop is to end.
        fn turn(&self, limit: Option<Duration>) -> Result<bool, PlatformError> {
            self.dispatch_pending()?;
            // Each seat the input holds has a data device before a window it
            // focuses is told the seat's selection.
            self.clipboard.seats(&self.input.seats());
            // The input the seats reported reaches the windows before they
            // settle, so a frame they deliver follows the input it shows.
            // Neither holds a lock of the loop: a handler may open, close or
            // quit.
            self.input.deliver();
            // The application's selection is asked for its text here, at the
            // turn, not inside the dispatch that recorded the request.
            self.clipboard.deliver();
            for window in self.windows.values() {
                window.settle();
            }
            let turn = self.signal.begin_turn();
            self.deliver_host_notices();
            if turn.drive() || self.exit_due() {
                return Ok(true);
            }
            let now = Instant::now();
            let windows = self.windows.values();
            let due = self
                .deadline
                .with(|deadline| deadline.take_due(now, self.frames.get()))
                .unwrap_or(false);
            if due {
                for window in &windows {
                    window.request_frame();
                }
            }
            let frame_due = windows.iter().any(|window| window.frame_due());
            let frame_wait = windows
                .iter()
                .filter_map(|window| window.frame_wait())
                .min();
            drop(windows);
            // A loop that stopped accepting work asks for no deadline, as the
            // X11 and Win32 loops do.
            let answer = if self.signal.accepting() {
                self.app.wake_deadline().or_resume(None)
            } else {
                None
            };
            let wake_deadline = self
                .deadline
                .with(|deadline| deadline.arm(answer, self.frames.get()))
                .flatten();
            let timeout = poll_timeout(
                LoopTurn {
                    frame_due,
                    frame_wait,
                    input_due: self.input.next_due(),
                    wake_deadline,
                },
                now,
            );
            let timeout = match (timeout, limit) {
                (Some(timeout), Some(limit)) => Some(timeout.min(limit)),
                (timeout, None) => timeout,
                (None, limit) => limit,
            };
            self.wait(timeout)?;
            Ok(false)
        }

        /// Deliver what the host's notice sources queued.
        fn deliver_host_notices(&self) {
            let source = self.sources.lock().host_notices.clone();
            if let Some(source) = source {
                source.deliver(self);
            }
        }

        /// Whether the loop ends because its last window closed, or the
        /// application asked for its exit policy again with no window left:
        /// the handler's answer, and the host's own when it leaves the
        /// decision to the host (a desktop loop ends with its last window).
        fn exit_due(&self) -> bool {
            let emptied = self.windows.take_emptied();
            let reevaluate = self.reevaluate.swap(false, Ordering::AcqRel);
            if !(emptied || reevaluate) || !self.windows.is_empty() {
                return false;
            }
            self.app.exit_policy().or_resume(true)
        }

        /// Block until the compositor sends something, another producer
        /// wakes the loop, or `timeout` passes; then read what came.
        fn wait(&self, timeout: Option<Duration>) -> Result<(), PlatformError> {
            let (next, guard) = self.dispatching(|queue, state| {
                let dispatched = queue.dispatch_pending(state).map_err(lost)?;
                let flush = match self.connection.flush() {
                    Ok(()) => Flush::Complete,
                    // A full socket: the rest goes out once it takes more.
                    Err(WaylandError::Io(error))
                        if error.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        Flush::Partial
                    }
                    Err(error) => return Err(lost(error)),
                };
                let next = before_poll(dispatched, flush);
                // `None`: events were queued meanwhile, and the next turn
                // dispatches them.
                let guard = match next {
                    BeforePoll::Apply => None,
                    BeforePoll::Poll { .. } => queue.prepare_read(),
                };
                Ok((next, guard))
            })??;
            let (BeforePoll::Poll { writable }, Some(guard)) = (next, guard) else {
                return Ok(());
            };
            let (compositor, woken) = {
                let connection = guard.connection_fd();
                let watched = if writable {
                    PollFlags::IN | PollFlags::OUT
                } else {
                    PollFlags::IN
                };
                let mut descriptors = [
                    PollFd::new(&connection, watched),
                    PollFd::new(&self.waker.0, PollFlags::IN),
                ];
                let timeout = timeout.map(timespec);
                loop {
                    match poll(&mut descriptors, timeout.as_ref()) {
                        Ok(_) => break,
                        Err(Errno::INTR) => {}
                        Err(error) => return Err(lost(error)),
                    }
                }
                let ready = |descriptor: &PollFd<'_>| {
                    descriptor
                        .revents()
                        .intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR)
                };
                (ready(&descriptors[0]), ready(&descriptors[1]))
            };
            if compositor {
                match guard.read() {
                    Ok(_) => {}
                    Err(WaylandError::Io(error))
                        if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(lost(error)),
                }
            } else {
                // Dropping the guard cancels the read.
                drop(guard);
            }
            if woken {
                self.waker.drain();
            }
            Ok(())
        }

        /// Start what the loop keeps while it runs: the portal and the
        /// display watch. Before the application's bootstrap, so its first
        /// read sees them.
        fn start_sources(&self) {
            let preferences = (self.start_preferences)(&self.executor, &self.signal);
            self.portal.set(Some(&preferences));
            self.sources.lock().preferences = Some(preferences);
            let watch = Arc::clone(&self.displays).observe(&self.signal);
            self.display_watch.with(|held| *held = Some(watch));
        }

        /// Start the sources of the host's notices, once the owner can
        /// deliver what they queue: a sleep that arrived during the bootstrap
        /// would hold logind's inhibitor with no turn to deliver it.
        fn start_host_notices(&self) {
            let notices = (self.start_host_notices)(&self.executor, &self.signal);
            self.sources.lock().host_notices = Some(notices);
        }

        /// Stop what the loop started: admission first, so a source that
        /// still reports wakes nobody, then the sources. Runs no application
        /// code, so an unwinding run calls it too.
        fn stop_sources(&self) {
            self.signal.close();
            let watch = self.display_watch.with(Option::take).flatten();
            let sources = std::mem::take(&mut *self.sources.lock());
            self.portal.set(None);
            contain_owner_callback(|| drop(watch));
            contain_owner_callback(|| drop(sources));
        }

        /// End the run: the sources stop, every window still open lets go of
        /// its handler and surface, and the application hears the quit, once,
        /// before its handler is released.
        fn finish(&self) {
            self.stop_sources();
            for window in self.windows.values() {
                window.release();
            }
            self.input.release();
            self.clipboard.release();
            let quit = self.app.quit_requested();
            self.app.release();
            quit.or_resume(());
        }
    }

    /// Stops the sources of a run that unwinds; a run that ends calls
    /// [`WaylandLoop::finish`] and disarms it.
    struct RunGuard<'a>(Option<&'a WaylandLoop>);

    impl Drop for RunGuard<'_> {
        fn drop(&mut self) {
            if let Some(owner) = self.0.take() {
                owner.stop_sources();
            }
        }
    }

    impl host::Loop for WaylandLoop {
        fn run(
            self: Arc<Self>,
            platform: Arc<dyn Platform>,
            handler: Rc<dyn AppHandler>,
        ) -> Result<(), PlatformError> {
            if std::thread::current().id() != self.owner_thread {
                return Err(PlatformError::EventLoop {
                    message: "a Wayland platform runs on the thread that made it, which owns its \
                              windows"
                        .to_owned(),
                });
            }
            if self.ran.swap(true, Ordering::AcqRel) {
                return Err(PlatformError::EventLoop {
                    message: "a Wayland platform runs once".to_owned(),
                });
            }
            tracing::info!("starting the Wayland loop");
            self.signal.bind_owner();
            let hooks: Arc<dyn OwnerHooks> = Arc::new(DirectOwnerHooks::with_signal(
                Arc::clone(&platform),
                Arc::clone(&self.signal),
            ));
            // The delivery holds the run's hooks weakly and mints the owner
            // capability of every later call from them: the run keeps them.
            self.app.bind(handler, &platform, &hooks);
            self.app.resume_through_signal(&self.signal);
            let mut guard = RunGuard(Some(&self));
            self.start_sources();
            let outcome = self
                .app
                .ready(OwnerPlatform::new(platform, Arc::clone(&hooks)))
                .map_err(PlatformError::bootstrap)
                .and_then(|()| {
                    self.signal
                        .start()
                        .map_err(|error| PlatformError::EventLoop {
                            message: error.to_string(),
                        })?;
                    self.start_host_notices();
                    while !self.turn(None)? {}
                    Ok(())
                });
            guard.0 = None;
            self.finish();
            drop(hooks);
            tracing::info!("the Wayland loop ended");
            outcome
        }

        fn quit(&self) {
            // A quit before the loop runs ends it as soon as it starts.
            if let Err(error) = self.signal.request_quit() {
                tracing::debug!(%error, "the Wayland loop has already ended");
            }
        }

        fn request_exit_policy_reevaluation(&self) {
            self.reevaluate.store(true, Ordering::Release);
            self.waker.wake();
        }
    }

    impl host::Services for WaylandLoop {
        fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
            Arc::clone(&self.executor) as _
        }

        fn app_path(&self) -> Result<PathBuf, PlatformError> {
            std::env::current_exe().map_err(|error| PlatformError::AppPath {
                message: error.to_string(),
            })
        }

        fn open_url(&self, url: &str) {
            xdg_open(url.as_ref());
        }

        fn reveal_path(&self, path: &Path) {
            // The extension, not the path: the name is the user's data.
            tracing::info!(extension = ?path.extension(), "revealing a path");
            if let Some(parent) = path.parent() {
                xdg_open(parent.as_os_str());
            }
        }

        fn open_path(&self, path: &Path) {
            tracing::info!(extension = ?path.extension(), "opening a path");
            xdg_open(path.as_os_str());
        }

        /// The file chooser portal is the system capability crate's
        /// (ADR-0151 §3): this host has none, and says so.
        fn prompt_for_paths(
            &self,
            options: crate::PathPromptOptions,
        ) -> crate::Task<Result<Option<Vec<PathBuf>>, PlatformError>> {
            let _ = options;
            crate::Task::ready(Err(no_file_prompt()))
        }

        /// See [`Self::prompt_for_paths`].
        fn prompt_for_new_path(
            &self,
            directory: &Path,
            suggested_name: Option<&str>,
        ) -> crate::Task<Result<Option<PathBuf>, PlatformError>> {
            let _ = (directory, suggested_name);
            crate::Task::ready(Err(no_file_prompt()))
        }

        fn compositor_name(&self) -> &'static str {
            ""
        }
    }

    /// The displays position is the loop's: the source the table registered.
    impl host::Displays<WaylandLoop> for WaylandLoop {
        fn snapshot(owner: &WaylandLoop) -> Result<DisplaySnapshot, PlatformError> {
            owner.displays.snapshot()
        }
    }

    /// The clipboard position is the loop's: the seats' selection, through
    /// the clipboard the table registered.
    impl host::Clipboard<WaylandLoop> for WaylandLoop {
        fn clipboard(owner: &WaylandLoop) -> Rc<dyn host::NativeClipboard> {
            Rc::new(Arc::clone(&owner.clipboard))
        }
    }

    /// The primary selection position is the loop's too, where the
    /// compositor has `zwp_primary_selection_device_manager_v1`.
    impl host::PrimarySelection<WaylandLoop> for WaylandLoop {
        fn primary_selection(owner: &WaylandLoop) -> Option<Rc<dyn host::NativePrimarySelection>> {
            owner.clipboard.has_primary_selection().then(
                || -> Rc<dyn host::NativePrimarySelection> {
                    Rc::new(Arc::clone(&owner.clipboard))
                },
            )
        }
    }

    /// The input position is the loop's: the seats' input the table
    /// registered, whose keymap names the layout.
    impl host::Input<WaylandLoop> for WaylandLoop {
        fn keyboard_layout(owner: &WaylandLoop) -> String {
            owner.input.keyboard_layout()
        }

        fn on_keyboard_layout_change(owner: &WaylandLoop, callback: Box<dyn FnMut() + Send>) {
            owner.input.on_keyboard_layout_change(callback);
        }
    }

    /// The preference position is the loop's: the portal it starts as it
    /// runs, read on the owner thread only.
    impl host::Preferences<WaylandLoop> for WaylandLoop {
        fn sample(owner: &WaylandLoop) -> Result<flui_platform::SystemPreferences, PlatformError> {
            if std::thread::current().id() != owner.owner_thread || !owner.signal.accepting() {
                return Err(PlatformError::Preferences {
                    message: "the system's preferences require the live Wayland loop's owner"
                        .into(),
                });
            }
            owner
                .portal
                .source()
                .ok_or(PlatformError::PreferencesDeferred)?
                .sample()
        }
    }

    impl SessionLoop for WaylandLoop {
        fn notice(&self, notice: SystemNotice) {
            self.notices.dispatch(notice);
        }

        fn session_end(&self, phase: SessionEndPhase) -> SessionEndAnswer {
            self.app
                .session_end(phase)
                .or_resume(SessionEndAnswer::Proceed)
        }
    }

    /// logind cannot be asked to wait for the user, so a session end the
    /// application held has nothing to release.
    impl LifecycleHooks for WaylandLoop {
        fn release_session_end(&self) {}
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A window the table holds that does nothing on a turn.
        struct Idle;

        impl LoopWindow for Idle {
            fn settle(&self) {}
            fn frame_due(&self) -> bool {
                false
            }
            fn frame_wait(&self) -> Option<Instant> {
                None
            }
            fn request_frame(&self) {}
            fn release(&self) {}
        }

        /// The launcher's token goes to the first window the table holds,
        /// once: a window refused (here, one under an identity the table
        /// holds already) leaves it for the next, and no later window gets
        /// one.
        #[test]
        fn the_launch_token_goes_to_the_first_window_held() {
            let token = ActivationToken::new("launcher".to_owned());
            let table = WindowTable::new(Some(token.clone()));
            let (first, second, third) = (WindowId::new(1), WindowId::new(2), WindowId::new(3));
            assert!(
                matches!(table.windows.insert(first, Rc::new(Idle)), Ok(None)),
                "a window held before"
            );
            assert!(
                table.register(first, Rc::new(Idle)).is_err(),
                "an identity held already is refused"
            );
            assert_eq!(
                table.register(second, Rc::new(Idle)).ok(),
                Some(Some(token)),
                "the first window held takes it"
            );
            assert_eq!(table.register(third, Rc::new(Idle)).ok(), Some(None));
        }

        /// The last input recorded is the one an activation token names,
        /// on the owner thread only: another thread reads none.
        #[test]
        fn the_last_input_is_the_one_recorded_last() {
            let (ours, _theirs) = std::os::unix::net::UnixStream::pair().expect("a socket pair");
            let Ok(connection) = Connection::from_socket(ours) else {
                eprintln!("no libwayland-client to load; the case was not run");
                return;
            };
            let backend = connection.backend().downgrade();
            let input = |serial| UserInput {
                seat: WlSeat::inert(backend.clone()),
                serial,
                surface: WlSurface::inert(backend.clone()),
            };
            let last = LastInput::default();
            assert!(last.latest().is_none(), "nothing recorded yet");
            last.record(input(7));
            last.record(input(42));
            assert_eq!(last.latest().map(|input| input.serial), Some(42));
            let off_owner = std::thread::spawn(move || last.latest().map(|input| input.serial))
                .join()
                .expect("the other thread");
            assert_eq!(off_owner, None, "off the owner thread");
        }
    }
}
