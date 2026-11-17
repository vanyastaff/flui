//! The Linux providers: the XDG desktop portal over the session bus
//! (`org.freedesktop.portal.Desktop`), which a sandboxed and an unsandboxed
//! application reach alike.
//!
//! Each UI runtime keeps one [`Portal`] ([`ProviderContext::runtime_local`]):
//! a thread of its own runs a `zruntime` executor, with one session-bus
//! connection made on first use. A request is a task on that executor; it
//! answers its [`Resolver`](flui_platform::capability::Resolver) from there.
//! When the runtime ends, the portal cancels the tasks still waiting, which
//! drops their resolvers, so each waiting completion ends as
//! `ProviderDropped`, and the thread stops.
//!
//! [`ProviderContext::runtime_local`]: flui_platform::capability::ProviderContext::runtime_local

mod dialogs;
mod launcher;
mod request;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use flui_platform::capability::WindowRef;
use flui_platform::capability::{CapabilityRegistrar, ProviderContext};
use flui_platform::launch::{Launcher, LauncherCapability};
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

use crate::dialog::{FileDialogCapability, FileDialogs};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub(crate) use dialogs::PortalDialogs;
pub(crate) use launcher::PortalLauncher;

/// Why a portal request gave no answer of its own.
#[derive(Debug)]
pub(crate) enum PortalError {
    /// No session bus, or no portal on it.
    Unsupported(Unsupported),
    /// The portal or the bus failed, as the [`SystemFailure`] says.
    Failed(SystemFailure),
}

impl From<zbus::Error> for PortalError {
    fn from(error: zbus::Error) -> Self {
        match error {
            zbus::Error::Address(_)
            | zbus::Error::Connection(..)
            | zbus::Error::InputOutput(_)
            | zbus::Error::Handshake(_) => {
                Self::Unsupported(Unsupported::new(UnsupportedReason::Unavailable))
            }
            zbus::Error::FDO(error)
                if matches!(
                    *error,
                    zbus::fdo::Error::ServiceUnknown(_)
                        | zbus::fdo::Error::NameHasNoOwner(_)
                        | zbus::fdo::Error::UnknownInterface(_)
                        | zbus::fdo::Error::UnknownMethod(_)
                        | zbus::fdo::Error::UnknownObject(_)
                ) =>
            {
                Self::Unsupported(Unsupported::new(UnsupportedReason::Unavailable))
            }
            zbus::Error::IncorrectType
            | zbus::Error::SignatureMismatch(..)
            | zbus::Error::ExcessData
            | zbus::Error::MissingField
            | zbus::Error::InvalidReply => Self::Failed(SystemFailure::MalformedAnswer),
            _ => Self::Failed(SystemFailure::Bus),
        }
    }
}

/// What one request's owner side and its task share: whether the caller let
/// the request go.
struct DismissalState {
    dismissed: AtomicBool,
    event: zruntime::Event,
}

/// The owner side of a request: dropping it tells the request's task that
/// nobody waits for the answer any more, so the task closes the portal's
/// `Request` (the dialog goes down) instead of waiting for the user.
pub(crate) struct DismissOnDrop(Arc<DismissalState>);

impl Drop for DismissOnDrop {
    fn drop(&mut self) {
        self.0.dismissed.store(true, Ordering::Release);
        self.0.event.notify(usize::MAX);
    }
}

/// The task side of a request: when its caller let it go.
pub(crate) struct Dismissal(Arc<DismissalState>);

impl Dismissal {
    /// A request's two sides.
    pub(crate) fn pair() -> (DismissOnDrop, Self) {
        let state = Arc::new(DismissalState {
            dismissed: AtomicBool::new(false),
            event: zruntime::Event::new(),
        });
        (DismissOnDrop(Arc::clone(&state)), Self(state))
    }

    /// Whether the caller has let the request go.
    pub(crate) fn is_dismissed(&self) -> bool {
        self.0.dismissed.load(Ordering::Acquire)
    }

    /// Completes once the caller has let the request go.
    pub(crate) async fn dismissed(&self) {
        // Listen before the check, so a drop the check misses is heard.
        let listener = self.0.event.listen();
        if !self.is_dismissed() {
            listener.await;
        }
    }
}

/// The bus connection the requests of one portal share, made by the first
/// request that needs it: the session bus, or the bus at `address`.
pub(crate) struct Bus {
    address: Option<Arc<str>>,
    connection: Mutex<Option<zbus::Connection>>,
}

impl Bus {
    pub(crate) async fn connection(&self) -> Result<zbus::Connection, PortalError> {
        let existing = self.lock().clone();
        // A bus that went away closes the connection for good; the next
        // request connects again rather than reuse it.
        if let Some(connection) = existing.filter(|connection| !connection.is_closed()) {
            return Ok(connection);
        }
        let builder = match &self.address {
            Some(address) => zbus::connection::Builder::address(&**address),
            None => zbus::connection::Builder::session(),
        };
        let connection = builder.build().await?;
        // Two requests may both connect; the open one stored first wins and
        // the other connection closes with its last clone.
        let mut stored = self.lock();
        match &*stored {
            Some(winner) if !winner.is_closed() => Ok(winner.clone()),
            _ => Ok(stored.insert(connection).clone()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<zbus::Connection>> {
        // A panic while the lock is held leaves only a connection or none.
        self.connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// The executor thread of one portal and the requests still running on it.
struct Worker {
    runtime: zruntime::SharedRuntime,
    stop: Arc<zruntime::Event>,
    bus: Arc<Bus>,
    tasks: Vec<zruntime::Task<(), zruntime::Shared>>,
}

impl Worker {
    fn start(address: Option<Arc<str>>) -> std::io::Result<Self> {
        let runtime = zruntime::SharedRuntime::new()?;
        let stop = Arc::new(zruntime::Event::new());
        let stopped = stop.listen();
        let driven = runtime.clone();
        std::thread::Builder::new()
            .name("flui-portal".to_owned())
            .spawn(move || driven.block_on(stopped))?;
        Ok(Self {
            runtime,
            stop,
            bus: Arc::new(Bus {
                address,
                connection: Mutex::new(None),
            }),
            tasks: Vec::new(),
        })
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Cancelling drops each waiting request's future, and with it its
        // resolver; then the thread's `block_on` returns.
        self.tasks.clear();
        self.stop.notify(usize::MAX);
    }
}

/// One UI runtime's way to the desktop portal: started on first use and
/// ended with the runtime.
#[derive(Default)]
pub(crate) struct Portal {
    worker: RefCell<Option<Worker>>,
    next_token: Cell<u64>,
    address: RefCell<Option<Arc<str>>>,
}

impl Portal {
    /// Talk to the portal on the bus at `address` instead of the session
    /// bus, from the next start on: a scenario's private bus.
    pub(crate) fn use_address(&self, address: Option<Arc<str>>) {
        *self.address.borrow_mut() = address;
    }

    /// Start the portal's thread unless it runs; a request asks this first,
    /// so a refusal is answered rather than lost with the request.
    ///
    /// # Errors
    ///
    /// The thread or its executor could not be made.
    pub(crate) fn start(&self) -> Result<(), PortalError> {
        let mut worker = self.worker.borrow_mut();
        if worker.is_none() {
            *worker = Some(Worker::start(self.address.borrow().clone()).map_err(|_| {
                PortalError::Unsupported(Unsupported::new(UnsupportedReason::Unavailable))
            })?);
        }
        Ok(())
    }

    /// Run `request` on the portal's thread, with the session bus.
    ///
    /// # Panics
    ///
    /// When [`Portal::start`] has not succeeded first.
    pub(crate) fn spawn<F, Fut>(&self, request: F)
    where
        F: FnOnce(Arc<Bus>) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let mut worker = self.worker.borrow_mut();
        let worker = worker
            .as_mut()
            .expect("BUG: a request is spawned only after `Portal::start` succeeded");
        worker.tasks.retain(|task| !task.is_finished());
        let task = worker
            .runtime
            .spawn("portal request", request(Arc::clone(&worker.bus)));
        worker.tasks.push(task);
    }

    /// A handle token no other request of this portal used: the last part of
    /// the `Request` object's path, which the response is matched to.
    pub(crate) fn token(&self) -> String {
        let token = self.next_token.get();
        // A u64 counted up once per request does not wrap.
        self.next_token.set(token + 1);
        format!("flui{token}")
    }
}

/// The `parent_window` a portal dialog or application chooser is attached
/// to: `x11:<XID>` for an X11 window, empty (not attached) for a Wayland
/// one, which needs an exported `xdg-foreign` handle the host does not hand
/// out yet, and for a window with no handle.
pub(crate) fn parent_window(window: &WindowRef) -> String {
    match window.window_handle().map(|handle| handle.as_raw()) {
        Ok(RawWindowHandle::Xlib(handle)) => format!("x11:{:x}", handle.window),
        Ok(RawWindowHandle::Xcb(handle)) => format!("x11:{:x}", handle.window.get()),
        _ => String::new(),
    }
}

/// Register the portal's providers: on the session bus, or on the bus at
/// `address`.
pub(crate) fn register(registrar: &mut CapabilityRegistrar, address: Option<Arc<str>>) {
    let launcher_address = address.clone();
    registrar.provide::<LauncherCapability>(move |ctx: &ProviderContext<'_>| {
        let portal = ctx.runtime_local::<Portal>();
        portal.use_address(launcher_address.clone());
        Ok::<_, Unsupported>(Launcher::new(
            PortalLauncher::new(portal, ctx.window()),
            ctx,
        ))
    });
    registrar.provide::<FileDialogCapability>(move |ctx: &ProviderContext<'_>| {
        let portal = ctx.runtime_local::<Portal>();
        portal.use_address(address.clone());
        FileDialogs::try_new(PortalDialogs::new(portal), ctx)
    });
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::rc::Rc;
    use std::sync::Arc;

    use flui_platform::capability::{WindowCloser, WindowRef};
    use flui_platform::{SurfaceTarget, SwapChainStatistics, WindowIdMint};
    use flui_sdk::foundation::FrameStamp;
    use raw_window_handle::{
        DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle,
        WindowHandle, XcbWindowHandle,
    };

    use super::{Portal, PortalLauncher, parent_window};

    /// A window whose native handle names the X11 window `xid`, and nothing
    /// else: no display, nothing to draw into.
    struct X11Window {
        xid: NonZeroU32,
    }

    impl HasWindowHandle for X11Window {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let raw = RawWindowHandle::Xcb(XcbWindowHandle::new(self.xid));
            #[expect(
                unsafe_code,
                reason = "a raw handle is borrowed only through this upstream call"
            )]
            // SAFETY: the handle carries an X11 window id and no pointer, and
            // the code under test only formats that id into a string; nothing
            // hands it to X or reads through it, so it needs no live window.
            let handle = unsafe { WindowHandle::borrow_raw(raw) };
            Ok(handle)
        }
    }

    impl HasDisplayHandle for X11Window {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            Err(HandleError::Unavailable)
        }
    }

    impl SurfaceTarget for X11Window {
        fn pre_present_notify(&self, _: Option<FrameStamp>, _: SwapChainStatistics) {}

        fn keep_past_close(&self) {}
    }

    fn x11_window(xid: u32) -> (WindowRef, WindowCloser) {
        let (closer, lifetime) = WindowCloser::new();
        let id = WindowIdMint::new()
            .next()
            .expect("BUG: a fresh mint issues its first id");
        let target = X11Window {
            xid: NonZeroU32::new(xid).expect("the test names a window"),
        };
        (WindowRef::new(id, 1.0, Arc::new(target), lifetime), closer)
    }

    #[test]
    fn a_launch_is_attached_to_its_widgets_window_while_it_is_open() {
        let (window, closer) = x11_window(0x2a);
        assert_eq!(parent_window(&window), "x11:2a");

        let launcher = PortalLauncher::new(Rc::new(Portal::default()), Some(window));
        assert_eq!(launcher.parent(), "x11:2a", "the chooser names the window");
        closer.close();
        assert_eq!(
            launcher.parent(),
            "",
            "a closed window's id is not handed on: it may name another window"
        );

        let windowless = PortalLauncher::new(Rc::new(Portal::default()), None);
        assert_eq!(windowless.parent(), "");
    }
}
