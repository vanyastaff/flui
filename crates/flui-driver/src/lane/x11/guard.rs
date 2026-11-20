//! The X11 lane's user-activity guard: XInput 2's raw events, read on a thread of its own from a
//! connection of its own, classified on the lane's thread ([`activity`](super::activity)).
//!
//! The watcher only reads and posts what it saw; the guard, which the lane owns, keeps the count
//! of the lane's own events and the first foreign one. A watcher whose connection broke posts
//! that it stopped, which ends the run as the person's input does: a guard that no longer watches
//! cannot tell the person's input from the lane's.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use x11rb::connection::Connection as _;
use x11rb::protocol::Event;
use x11rb::protocol::xinput::{self, ConnectionExt as _, XIEventMask};
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

use super::activity::{EchoCount, Observed, RawKind, XTEST_DEVICES};
use super::safety::LANE;
use crate::error::{Error, Result};
use crate::lane::pacing::ECHO_WITHIN;
use crate::lane::{ActivityGuard, ForeignInput};

/// What the guard reports when its watcher stopped while armed.
pub const STOPPED_WATCHING: &str = "the user-activity guard stopped watching the X server";

/// How long arming waits for the watcher to select its events.
const ARM_WITHIN: Duration = Duration::from_secs(5);

/// How long the watcher sleeps when the server has nothing for it.
const IDLE: Duration = Duration::from_millis(2);

/// How many events the watcher holds for the lane between two looks. The lane looks after every
/// event it sends and every slice of a pause, and its own events come back one at a time, so a
/// full queue is input the lane did not send, more of it than the guard keeps.
const ROOM: usize = 1024;

/// What the guard reports when the watcher's queue overflowed.
const OVERFLOWED: &str = "more input than the user-activity guard keeps between two looks";

enum Seen {
    /// The watcher selected its events: input from here on is seen.
    Ready,
    /// A raw event.
    Raw(Observed),
    /// The watcher stopped, and why.
    Stopped(String),
}

struct Watcher {
    stop: Arc<AtomicBool>,
    /// Set by the watcher when the queue had no room for what it saw.
    overflowed: Arc<AtomicBool>,
    seen: mpsc::Receiver<Seen>,
    thread: Option<JoinHandle<()>>,
}

/// The watcher's end of the queue: a post that finds no room is recorded, never dropped unheard.
struct Post {
    queue: mpsc::SyncSender<Seen>,
    overflowed: Arc<AtomicBool>,
}

impl Post {
    /// Post `seen`; whether the guard still listens.
    fn send(&self, seen: Seen) -> bool {
        match self.queue.try_send(seen) {
            Ok(()) => true,
            Err(mpsc::TrySendError::Full(_)) => {
                self.overflowed.store(true, Ordering::Release);
                true
            }
            Err(mpsc::TrySendError::Disconnected(_)) => false,
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The X11 lane's user-activity guard: XInput 2's raw events, read on a thread of its own, the
/// lane's XTEST input told from anyone else's by counting it (see the [lane's docs](super)).
pub struct X11Guard {
    display: Option<String>,
    watcher: Option<Watcher>,
    count: EchoCount,
    foreign: Option<ForeignInput>,
}

impl std::fmt::Debug for X11Guard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("X11Guard")
            .field("display", &self.display)
            .field("armed", &self.watcher.is_some())
            .field("count", &self.count)
            .finish_non_exhaustive()
    }
}

impl X11Guard {
    /// A guard watching the X server `display` names (`DISPLAY` when `None`), not armed yet.
    #[must_use]
    pub fn new(display: Option<String>) -> Self {
        Self {
            display,
            watcher: None,
            count: EchoCount::new(),
            foreign: None,
        }
    }

    /// The lane is about to fake one event through XTEST.
    pub(super) fn will_send(&mut self) {
        if self.watcher.is_some() {
            self.count.sent();
        }
    }

    /// Classify what the watcher saw since the last look.
    fn drain(&mut self) {
        let Some(watcher) = &self.watcher else {
            return;
        };
        while let Ok(seen) = watcher.seen.try_recv() {
            let foreign = match seen {
                Seen::Ready => None,
                Seen::Raw(observed) => self.count.foreign(observed).map(ForeignInput::new),
                Seen::Stopped(_) => Some(ForeignInput::new(STOPPED_WATCHING)),
            };
            if self.foreign.is_none() {
                self.foreign = foreign;
            }
        }
        if self.foreign.is_none() && watcher.overflowed.load(Ordering::Acquire) {
            self.foreign = Some(ForeignInput::new(OVERFLOWED));
        }
    }

    /// Wait for every event the lane faked to come back through the watcher: a watcher that does
    /// not see the lane's input would not see the person's. A guard that is not armed waits for
    /// nothing.
    ///
    /// # Errors
    ///
    /// [`Error::Lane`] with `unavailable` when the echo does not come within [`ECHO_WITHIN`].
    pub(super) fn await_echo(&mut self) -> Result<()> {
        if self.watcher.is_none() {
            return Ok(());
        }
        let deadline = Instant::now() + ECHO_WITHIN;
        loop {
            self.drain();
            if self.count.settled() || self.foreign.is_some() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Error::Lane {
                    lane: LANE.to_owned(),
                    unavailable: true,
                    message: format!(
                        "the user-activity guard did not see the lane's own input within {} ms: \
                         it would not see yours",
                        ECHO_WITHIN.as_millis()
                    ),
                });
            }
            thread::sleep(Duration::from_millis(1));
        }
    }
}

impl ActivityGuard for X11Guard {
    fn arm(&mut self) -> Result<()> {
        self.disarm();
        let (queue, seen) = mpsc::sync_channel(ROOM);
        let stop = Arc::new(AtomicBool::new(false));
        let overflowed = Arc::new(AtomicBool::new(false));
        let post = Post {
            queue,
            overflowed: Arc::clone(&overflowed),
        };
        let display = self.display.clone();
        let stopping = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("flui-driver-x11-guard".to_owned())
            .spawn(move || watch(display.as_deref(), &stopping, &post))?;
        let watcher = Watcher {
            stop,
            overflowed,
            seen,
            thread: Some(thread),
        };
        match watcher.seen.recv_timeout(ARM_WITHIN) {
            Ok(Seen::Ready) => {}
            Ok(Seen::Stopped(why)) => {
                return Err(Error::Lane {
                    lane: LANE.to_owned(),
                    unavailable: true,
                    message: format!("the user-activity guard cannot watch: {why}"),
                });
            }
            Ok(Seen::Raw(_)) | Err(_) => {
                return Err(Error::Lane {
                    lane: LANE.to_owned(),
                    unavailable: true,
                    message: "the user-activity guard did not start watching".to_owned(),
                });
            }
        }
        self.count = EchoCount::new();
        self.foreign = None;
        self.watcher = Some(watcher);
        Ok(())
    }

    fn foreign_input(&mut self) -> Option<ForeignInput> {
        self.drain();
        self.foreign.clone()
    }

    fn disarm(&mut self) {
        drop(self.watcher.take());
        self.count = EchoCount::new();
        self.foreign = None;
    }
}

/// The watcher: select the raw events on the root, then post each until `stop`.
fn watch(display: Option<&str>, stop: &AtomicBool, post: &Post) {
    let connection = match select(display) {
        Ok(selected) => selected,
        Err(why) => {
            post.send(Seen::Stopped(why));
            return;
        }
    };
    let (connection, xtest) = connection;
    if !post.send(Seen::Ready) {
        return;
    }
    while !stop.load(Ordering::Acquire) {
        match connection.poll_for_event() {
            Ok(Some(event)) => {
                let Some((kind, source)) = raw(&event) else {
                    continue;
                };
                let observed = Observed::new(kind, xtest.contains(&source));
                if !post.send(Seen::Raw(observed)) {
                    return;
                }
            }
            Ok(None) => thread::sleep(IDLE),
            Err(error) => {
                post.send(Seen::Stopped(error.to_string()));
                return;
            }
        }
    }
}

/// A connection with XInput 2's raw events selected on the root, and the ids of the XTEST
/// devices.
fn select(display: Option<&str>) -> std::result::Result<(RustConnection, Vec<u16>), String> {
    let (connection, screen) = RustConnection::connect(display).map_err(|e| e.to_string())?;
    let version = connection
        .xinput_xi_query_version(2, 2)
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| format!("the X server has no XInput 2: {e}"))?;
    if (version.major_version, version.minor_version) < (2, 2) {
        return Err(format!(
            "the X server has XInput {}.{}, and raw touch events need 2.2",
            version.major_version, version.minor_version
        ));
    }
    let devices = connection
        .xinput_xi_query_device(xinput::Device::ALL)
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?;
    let xtest: Vec<u16> = devices
        .infos
        .iter()
        .filter(|info| {
            XTEST_DEVICES
                .iter()
                .any(|name| info.name.as_slice() == name.as_bytes())
        })
        .map(|info| info.deviceid)
        .collect();
    if xtest.is_empty() {
        return Err("the X server lists no XTEST device".to_owned());
    }
    let root = connection.setup().roots[screen].root;
    let mask = XIEventMask::RAW_KEY_PRESS
        | XIEventMask::RAW_KEY_RELEASE
        | XIEventMask::RAW_BUTTON_PRESS
        | XIEventMask::RAW_BUTTON_RELEASE
        | XIEventMask::RAW_MOTION
        | XIEventMask::RAW_TOUCH_BEGIN
        | XIEventMask::RAW_TOUCH_UPDATE
        | XIEventMask::RAW_TOUCH_END;
    connection
        .xinput_xi_select_events(
            root,
            &[xinput::EventMask {
                deviceid: u16::from(xinput::Device::ALL_MASTER),
                mask: vec![mask],
            }],
        )
        .map_err(|e| e.to_string())?;
    // A round trip: the selection is in force before arming returns.
    connection
        .get_input_focus()
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?;
    Ok((connection, xtest))
}

/// What a raw event was, and the device it came from.
fn raw(event: &Event) -> Option<(RawKind, u16)> {
    Some(match event {
        Event::XinputRawKeyPress(e) | Event::XinputRawKeyRelease(e) => (RawKind::Key, e.sourceid),
        Event::XinputRawButtonPress(e) | Event::XinputRawButtonRelease(e) => {
            (RawKind::Button, e.sourceid)
        }
        Event::XinputRawMotion(e) => (RawKind::Motion, e.sourceid),
        Event::XinputRawTouchBegin(e)
        | Event::XinputRawTouchUpdate(e)
        | Event::XinputRawTouchEnd(e) => (RawKind::Touch, e.sourceid),
        _ => return None,
    })
}
