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
//! the Settings portal's preferences, the notice channel and the sources of
//! logind's and the kernel's notices. Its windows are the window feature's
//! (`window::linux::wayland`): the loop holds them as `LoopWindow`s and asks
//! each to settle after every read, and the window feature implements the
//! dispatch of its own protocol objects on `WaylandState`.

#[cfg(all(target_os = "linux", feature = "wayland"))]
pub(crate) use event_loop::{
    LoopWindow, LoopWaker, OutputFacts, PortalSlot, WaylandDisplays, WaylandHost, WaylandLoop,
    WaylandNotices, WaylandPreferences, WaylandState, WindowSeed, WindowTable,
};

pub(crate) mod wait {
    //! How long one turn of the Wayland loop may block in `poll`, and when a
    //! wake deadline is offered to the windows (ADR-0249's rule on this host).
    //!
    //! Every producer of work wakes the loop: the compositor's socket, the
    //! owner signal and a redraw asked for on another thread write what `poll`
    //! waits on. So the loop blocks until woken unless a window has a frame to
    //! deliver now, or the application's handler named a wake deadline.
    //!
    //! A deadline that came due is served by a frame: the loop asks every
    //! window for one, once. A deadline it already offered waits for a frame
    //! that serves it (a frame callback, a configure) and then for the
    //! handler to name another; asking again would turn a window that declines
    //! its frames, or one the compositor does not draw, into a busy loop.

    #![forbid(unsafe_code)]

    use std::time::Duration;

    use web_time::Instant;

    /// What one turn of the loop knows when it chooses how long to block.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct LoopTurn {
        /// A window has a frame to deliver now.
        pub(crate) frame_due: bool,
        /// The earliest instant the application's handler asked to be woken
        /// at.
        pub(crate) wake_deadline: Option<Instant>,
        /// The deadline already offered to the windows.
        pub(crate) offered: Option<Instant>,
    }

    /// Whether the loop asks its windows for a frame for `deadline` now: it
    /// is due and was not offered yet.
    pub(crate) fn offer_now(deadline: Option<Instant>, offered: Option<Instant>, now: Instant) -> bool {
        deadline.is_some_and(|deadline| deadline <= now && Some(deadline) != offered)
    }

    /// How long `poll` may block; `None` blocks until something wakes the
    /// loop.
    pub(crate) fn poll_timeout(turn: LoopTurn, now: Instant) -> Option<Duration> {
        if turn.frame_due {
            return Some(Duration::ZERO);
        }
        let deadline = turn.wake_deadline?;
        if deadline > now {
            Some(deadline.saturating_duration_since(now))
        } else if Some(deadline) == turn.offered {
            // Offered and waiting for a frame that serves it.
            None
        } else {
            Some(Duration::ZERO)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// An idle Wayland loop blocks until it is woken: no timer of its own,
        /// a deadline ahead bounds the wait, a deadline that came due is
        /// offered once, and one already offered never turns the wait into a
        /// busy loop.
        #[test]
        fn an_idle_wayland_loop_blocks_until_it_is_woken() {
            let now = Instant::now();
            let ms = Duration::from_millis;
            let behind = now.checked_sub(ms(40));
            let turn = |frame_due, wake_deadline, offered| LoopTurn {
                frame_due,
                wake_deadline,
                offered,
            };
            for (case, turn, expected) in [
                ("idle", turn(false, None, None), None),
                (
                    "a frame to deliver",
                    turn(true, None, None),
                    Some(Duration::ZERO),
                ),
                (
                    "a frame to deliver with an offered deadline",
                    turn(true, behind, behind),
                    Some(Duration::ZERO),
                ),
                (
                    "a deadline ahead",
                    turn(false, Some(now + ms(250)), None),
                    Some(ms(250)),
                ),
                (
                    "a deadline ahead, an earlier one offered",
                    turn(false, Some(now + ms(250)), behind),
                    Some(ms(250)),
                ),
                (
                    "a deadline that came due",
                    turn(false, behind, None),
                    Some(Duration::ZERO),
                ),
                (
                    "a deadline exactly due",
                    turn(false, Some(now), None),
                    Some(Duration::ZERO),
                ),
                (
                    "a due deadline already offered",
                    turn(false, behind, behind),
                    None,
                ),
                (
                    "a due deadline other than the one offered",
                    turn(false, behind, Some(now + ms(5))),
                    Some(Duration::ZERO),
                ),
            ] {
                assert_eq!(poll_timeout(turn, now), expected, "{case}");
            }
        }

        /// A deadline is offered to the windows once, when it is due, and a
        /// new deadline is offered again.
        #[test]
        fn a_due_deadline_is_offered_once() {
            let now = Instant::now();
            let behind = now.checked_sub(Duration::from_millis(10));
            let ahead = Some(now + Duration::from_millis(10));
            assert!(offer_now(behind, None, now), "due and not offered");
            assert!(!offer_now(behind, behind, now), "offered already");
            assert!(!offer_now(ahead, None, now), "not due");
            assert!(!offer_now(None, None, now), "no deadline");
            let later = Some(now);
            assert!(offer_now(later, behind, now), "a new deadline");
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

    use flui_platform::{
        DisplayId, DisplaySnapshot, SessionEndAnswer, SessionEndPhase, SystemNotice,
        WindowAppearance, WindowId,
    };
    use parking_lot::Mutex;
    use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll};
    use rustix::io::Errno;
    use wayland_client::backend::WaylandError;
    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::wl_compositor::WlCompositor;
    use wayland_client::protocol::wl_output::{self, WlOutput};
    use wayland_client::protocol::wl_registry::{self, WlRegistry};
    use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
    use wayland_protocols::wp::viewporter::client::wp_viewporter::{self, WpViewporter};
    use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};
    use web_time::Instant;

    use super::wait::{LoopTurn, offer_now, poll_timeout};
    use crate::error::PlatformError;
    use crate::executor::BackgroundExecutor;
    use crate::host;
    use crate::owner::app_delivery::AppDelivery;
    use crate::owner::notices::{LifecycleHooks, NoticeChannel, StaleOnWake};
    use crate::owner::panic_boundary::contain_owner_callback;
    use crate::owner::signal::OwnerSignal;
    use crate::owner::slot::{OwnerCell, OwnerMap};
    use crate::owner::sources::SystemPreferenceSource;
    use crate::owner::watch::Watch;
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
    /// `wp_viewporter` has one version.
    const VIEWPORTER_VERSION: u32 = 1;

    /// What the host's table fills for the loop (ADR-0151 §10): the
    /// displays, the preference source, the notice channel and the sources
    /// of the host's notices. The loop reaches each through the hook here
    /// and names no feature.
    pub(crate) trait WaylandHost: 'static {
        /// The displays: the compositor's outputs, read on a connection of
        /// their own.
        fn displays() -> Arc<dyn WaylandDisplays>;

        /// The channel the loop's system notices take to the application.
        fn notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel>;

        /// The Settings portal's preferences, observed on `executor` and
        /// waking the owner behind `signal`; started as the loop runs.
        fn preferences(
            executor: &BackgroundExecutor,
            signal: &Arc<OwnerSignal>,
        ) -> Arc<dyn WaylandPreferences>;

        /// The sources of the host's system notices (logind, the kernel's
        /// memory pressure), started once the owner can deliver what they
        /// queue.
        fn host_notices(
            executor: &BackgroundExecutor,
            signal: &Arc<OwnerSignal>,
        ) -> Arc<dyn WaylandNotices>;
    }

    /// The compositor's outputs as the loop and its windows reach them.
    pub(crate) trait WaylandDisplays: StaleOnWake {
        /// The snapshot: the last one published while nothing changed, the
        /// outputs read again otherwise.
        ///
        /// # Errors
        /// [`PlatformError::Displays`] when the compositor cannot be read.
        fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError>;

        /// The display and scale of the output whose global is `name`, as
        /// the last snapshot holds it; `None` for an output it does not hold
        /// yet, which marks the snapshot stale.
        fn output(&self, name: u32) -> Option<OutputFacts>;

        /// Keep the outputs current, waking the owner behind `signal` when
        /// they change, until the watch is dropped.
        fn observe(self: Arc<Self>, signal: &Arc<OwnerSignal>) -> Watch;
    }

    /// What the display snapshot says of one output.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub(crate) struct OutputFacts {
        /// The display it is.
        pub(crate) display: DisplayId,
        /// Its device pixels per logical pixel.
        pub(crate) scale: f64,
    }

    /// The Settings portal's preferences as the loop keeps them: the
    /// snapshot, and the colour scheme every window follows.
    pub(crate) trait WaylandPreferences: SystemPreferenceSource + Send + Sync {
        /// The colour scheme the portal reports.
        fn appearance(&self) -> WindowAppearance;
    }

    /// The sources of the host's system notices the loop keeps.
    pub(crate) trait WaylandNotices: Send + Sync {
        /// Deliver what the sources queued since the loop's last turn,
        /// oldest first, through `owner`'s notice channel and its
        /// application delivery.
        fn deliver(&self, owner: &WaylandLoop);
    }

    /// The portal's source as a window reaches it: weakly, through the loop,
    /// which starts it as it runs and stops it as it ends.
    #[derive(Clone, Default)]
    pub(crate) struct PortalSlot(Arc<Mutex<Option<Weak<dyn WaylandPreferences>>>>);

    impl PortalSlot {
        /// The colour scheme the portal reports; light while it is not
        /// running.
        pub(crate) fn appearance(&self) -> WindowAppearance {
            self.source()
                .map_or(WindowAppearance::Light, |source| source.appearance())
        }

        fn source(&self) -> Option<Arc<dyn WaylandPreferences>> {
            self.0.lock().as_ref().and_then(Weak::upgrade)
        }

        fn set(&self, source: Option<&Arc<dyn WaylandPreferences>>) {
            *self.0.lock() = source.map(Arc::downgrade);
        }
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

        /// A wake deadline came due: ask the window for a frame.
        fn request_frame(&self);

        /// The loop ends: let go of the window's handler and its surface.
        fn release(&self);
    }

    /// The windows a loop holds, by identity, on the owner thread.
    pub(crate) struct WindowTable {
        windows: OwnerMap<WindowId, Rc<dyn LoopWindow>>,
        /// The window the compositor last activated.
        active: Mutex<Option<WindowId>>,
        /// The last window closed since the loop last looked: it asks the
        /// application's exit policy.
        emptied: AtomicBool,
        /// The last identity issued.
        issued: AtomicU64,
    }

    impl WindowTable {
        fn new() -> Self {
            Self {
                windows: OwnerMap::new(),
                active: Mutex::new(None),
                emptied: AtomicBool::new(false),
                issued: AtomicU64::new(0),
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

        /// Hold `window` as `id`, on the owner thread.
        fn register(&self, id: WindowId, window: Rc<dyn LoopWindow>) -> Result<(), OpenWindowError> {
            match self.windows.insert(id, window) {
                Ok(None) => Ok(()),
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
            {
                let mut active = self.active.lock();
                if *active == Some(id) {
                    *active = None;
                }
            }
            if window.is_some() && self.windows.is_empty() {
                self.emptied.store(true, Ordering::Release);
            }
            // The window's own drop may release what its handler held.
            contain_owner_callback(|| drop(window));
        }

        /// The compositor activated window `id`, or took the activation away.
        pub(crate) fn activation(&self, id: WindowId, activated: bool) {
            let mut active = self.active.lock();
            if activated {
                *active = Some(id);
            } else if *active == Some(id) {
                *active = None;
            }
        }

        fn active(&self) -> Option<WindowId> {
            *self.active.lock()
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
        fn new() -> rustix::io::Result<Self> {
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
        outputs: Vec<(u32, WlOutput)>,
    }

    impl WaylandState {
        fn bind_output(
            &mut self,
            registry: &WlRegistry,
            name: u32,
            version: u32,
            queue: &QueueHandle<Self>,
        ) {
            if self.outputs.iter().any(|(bound, _)| *bound == name) {
                return;
            }
            let output = registry.bind::<WlOutput, u32, Self>(
                name,
                version.min(OUTPUT_VERSION),
                queue,
                name,
            );
            self.outputs.push((name, output));
        }

        fn remove_output(&mut self, name: u32) {
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
                wl_registry::Event::GlobalRemove { name } => state.remove_output(name),
                _ => {}
            }
        }
    }

    impl Dispatch<WlOutput, u32> for WaylandState {
        fn event(
            _: &mut Self,
            _: &WlOutput,
            _: wl_output::Event,
            _: &u32,
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
        /// `wl_shm`, which a test draws a window's frames with in place of a
        /// renderer (`testing::wayland::present`).
        #[cfg(any(test, feature = "testing"))]
        pub(crate) shm: Option<wayland_client::protocol::wl_shm::WlShm>,
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
        preferences: Option<Arc<dyn WaylandPreferences>>,
        host_notices: Option<Arc<dyn WaylandNotices>>,
    }

    /// The Wayland host's loop: the connection to the compositor and what the
    /// owner thread keeps for the positions of its table.
    pub struct WaylandLoop {
        connection: Connection,
        dispatcher: Mutex<Dispatcher>,
        protocol: WaylandProtocol,
        waker: Arc<LoopWaker>,
        signal: Arc<OwnerSignal>,
        app: Arc<AppDelivery>,
        windows: Arc<WindowTable>,
        /// The thread that made the platform, which owns its windows and
        /// runs its loop.
        owner_thread: ThreadId,
        displays: Arc<dyn WaylandDisplays>,
        notices: Arc<dyn NoticeChannel>,
        portal: PortalSlot,
        start_preferences:
            fn(&BackgroundExecutor, &Arc<OwnerSignal>) -> Arc<dyn WaylandPreferences>,
        start_host_notices: fn(&BackgroundExecutor, &Arc<OwnerSignal>) -> Arc<dyn WaylandNotices>,
        /// The sources the loop keeps while it runs.
        sources: Mutex<Sources>,
        /// The display watch, which the owner thread starts and stops.
        display_watch: OwnerCell<Option<Watch>>,
        executor: Arc<BackgroundExecutor>,
        /// The wake deadline already offered to the windows.
        offered: Mutex<Option<Instant>>,
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
        pub(crate) fn new<H: WaylandHost>() -> Result<Self, PlatformError> {
            let init = |what: &str, error: &dyn std::fmt::Display| PlatformError::Init {
                message: format!("Wayland: {what}: {error}"),
            };
            let connection =
                Connection::connect_to_env().map_err(|error| init("no compositor", &error))?;
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
            #[cfg(any(test, feature = "testing"))]
            let shm = globals
                .bind::<wayland_client::protocol::wl_shm::WlShm, _, _>(&handle, 1..=1, ())
                .ok();
            let mut state = WaylandState {
                outputs: Vec::new(),
            };
            let outputs = globals.contents().with_list(|list| {
                list.iter()
                    .filter(|global| global.interface == WlOutput::interface().name)
                    .map(|global| (global.name, global.version))
                    .collect::<Vec<_>>()
            });
            for (name, version) in outputs {
                state.bind_output(globals.registry(), name, version, &handle);
            }
            let waker = Arc::new(
                LoopWaker::new().map_err(|error| init("the loop's wake descriptor", &error))?,
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
                    #[cfg(any(test, feature = "testing"))]
                    shm,
                },
                connection,
                dispatcher: Mutex::new(Dispatcher { queue, state }),
                waker,
                signal,
                notices: H::notices(&app),
                app,
                windows: Arc::new(WindowTable::new()),
                owner_thread: std::thread::current().id(),
                displays: H::displays(),
                portal: PortalSlot::default(),
                start_preferences: H::preferences,
                start_host_notices: H::host_notices,
                sources: Mutex::new(Sources::default()),
                display_watch: OwnerCell::new(),
                executor: Arc::new(BackgroundExecutor::new()),
                offered: Mutex::new(None),
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
            let id = self.windows.issue().ok_or_else(|| OpenWindowError::Backend {
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
            })
        }

        /// Hold the window `id` until it closes.
        ///
        /// # Errors
        /// [`OpenWindowError::Unavailable`] off the owner thread.
        pub(crate) fn register(
            &self,
            id: WindowId,
            window: Rc<dyn LoopWindow>,
        ) -> Result<(), OpenWindowError> {
            self.windows.register(id, window)
        }

        /// Send what was asked and wait until the compositor has answered it,
        /// dispatching what it sent meanwhile. Runs no handler: the windows
        /// settle on the loop's next turn.
        ///
        /// # Errors
        /// [`OpenWindowError::Backend`] when the connection failed.
        pub(crate) fn roundtrip(&self) -> Result<(), OpenWindowError> {
            let mut dispatcher = self.dispatcher.lock();
            let Dispatcher { queue, state } = &mut *dispatcher;
            queue
                .roundtrip(state)
                .map(drop)
                .map_err(|error| OpenWindowError::Backend {
                    message: format!("the Wayland connection: {error}"),
                })
        }

        /// The window the compositor last activated.
        pub(crate) fn active_window(&self) -> Option<WindowId> {
            self.windows.active()
        }

        /// The application's handler about a phase of the session ending;
        /// with none bound the session goes on.
        pub(crate) fn session_end(&self, phase: SessionEndPhase) -> SessionEndAnswer {
            self.app
                .session_end(phase)
                .or_resume(SessionEndAnswer::Proceed)
        }

        /// A system notice for the application, through the notice channel.
        pub(crate) fn notice(&self, notice: SystemNotice) {
            self.notices.dispatch(notice);
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

        /// Dispatch what the compositor already sent: the windows record it.
        fn dispatch_pending(&self) -> Result<(), PlatformError> {
            let mut dispatcher = self.dispatcher.lock();
            let Dispatcher { queue, state } = &mut *dispatcher;
            queue.dispatch_pending(state).map(drop).map_err(lost)
        }

        /// One turn of the loop, blocking at most `limit`. `true` when the
        /// loop is to end.
        fn turn(&self, limit: Option<Duration>) -> Result<bool, PlatformError> {
            self.dispatch_pending()?;
            // Each window carries out what was recorded, with no lock of the
            // loop held: its handler may open, close or quit.
            for window in self.windows.values() {
                window.settle();
            }
            let turn = self.signal.begin_turn();
            self.deliver_host_notices();
            if turn.drive() || self.exit_due() {
                return Ok(true);
            }
            let now = Instant::now();
            let deadline = self.app.wake_deadline().or_resume(None);
            let windows = self.windows.values();
            let offer = {
                let mut offered = self.offered.lock();
                let offer = offer_now(deadline, *offered, now);
                if offer {
                    *offered = deadline;
                }
                offer
            };
            if offer {
                for window in &windows {
                    window.request_frame();
                }
            }
            let frame_due = windows.iter().any(|window| window.frame_due());
            drop(windows);
            let timeout = poll_timeout(
                LoopTurn {
                    frame_due,
                    wake_deadline: deadline,
                    offered: *self.offered.lock(),
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
            let guard = {
                let mut dispatcher = self.dispatcher.lock();
                let Dispatcher { queue, state } = &mut *dispatcher;
                queue.dispatch_pending(state).map_err(lost)?;
                match self.connection.flush() {
                    Ok(()) => {}
                    // A full socket: the rest goes out on a later turn.
                    Err(WaylandError::Io(error))
                        if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(lost(error)),
                }
                queue.prepare_read()
            };
            // Events were queued meanwhile: the next turn dispatches them.
            let Some(guard) = guard else {
                return Ok(());
            };
            let (compositor, woken) = {
                let connection = guard.connection_fd();
                let mut descriptors = [
                    PollFd::new(&connection, PollFlags::IN),
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

        // The Wayland host opens no URL and shows no file prompt: those
        // services move to the system capability crate (ADR-0151 §3).
        fn open_url(&self, url: &str) {
            let _ = url;
        }

        fn reveal_path(&self, path: &Path) {
            // The extension, not the path: the name is the user's data.
            tracing::info!(extension = ?path.extension(), "revealing a path");
            if let Some(parent) = path.parent() {
                let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
            }
        }

        fn open_path(&self, path: &Path) {
            tracing::info!(extension = ?path.extension(), "opening a path");
            let _ = std::process::Command::new("xdg-open").arg(path).spawn();
        }

        fn prompt_for_paths(
            &self,
            options: crate::PathPromptOptions,
        ) -> crate::Task<Result<Option<Vec<PathBuf>>, PlatformError>> {
            let _ = options;
            crate::Task::ready(Ok(None))
        }

        fn prompt_for_new_path(
            &self,
            directory: &Path,
            suggested_name: Option<&str>,
        ) -> crate::Task<Result<Option<PathBuf>, PlatformError>> {
            let _ = (directory, suggested_name);
            crate::Task::ready(Ok(None))
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

    /// logind cannot be asked to wait for the user, so a session end the
    /// application held has nothing to release.
    impl LifecycleHooks for WaylandLoop {
        fn release_session_end(&self) {}
    }
}
