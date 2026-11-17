//! The Linux providers: the XDG desktop portal over the session bus
//! (`org.freedesktop.portal.Desktop`), which a sandboxed and an unsandboxed
//! application reach alike, and the launcher entry's progress signal.
//!
//! Each UI runtime keeps one [`Portal`] ([`ProviderContext::runtime_local`]):
//! a thread of its own runs a `zruntime` executor, with one session-bus
//! connection made on first use. A request is a task on that executor; it
//! answers its [`Resolver`](flui_platform::capability::Resolver) from there.
//! When the runtime ends, the portal cancels the tasks still waiting, which
//! drops their resolvers, so each waiting completion ends as
//! `ProviderDropped`, and the thread stops.
//!
//! A request that shows something names the acquiring widget's window as its
//! `parent_window` ([`with_parent`]): the window's
//! [`PortalParent`](flui_platform::capability::PortalParent), held by the
//! request until it ends, so a Wayland window's export lasts exactly as long.
//!
//! An unsandboxed application is named to the portal by its id
//! ([`Bus::desktop`]), so the portal attributes its requests, its autostart
//! entry and its shortcuts to it; inside Flatpak or Snap the sandbox names it.
//!
//! [`ProviderContext::runtime_local`]: flui_platform::capability::ProviderContext::runtime_local

mod background;
mod dialogs;
mod launcher;
mod launcher_entry;
mod request;
mod shortcuts;
mod user_dirs;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use flui_platform::capability::{
    CapabilityRegistrar, CompletionSource, PortalParent, ProviderContext, WindowRef,
};
use flui_platform::launch::{Launcher, LauncherCapability};
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

use crate::autostart::{Autostart, AutostartCapability};
use crate::dialog::{FileDialogCapability, FileDialogs};
use crate::shortcuts::{GlobalShortcutCapability, GlobalShortcuts};
use crate::taskbar::{TaskbarCapability, TaskbarEntry};
use crate::user_dirs::{UserDirCapability, UserDirs};

pub(crate) use background::PortalAutostart;
pub(crate) use dialogs::PortalDialogs;
pub(crate) use launcher::PortalLauncher;
pub(crate) use launcher_entry::LauncherEntry;
pub(crate) use shortcuts::PortalShortcuts;
pub(crate) use user_dirs::XdgUserDirs;

/// Why a portal request gave no answer of its own.
#[derive(Debug)]
pub(crate) enum PortalError {
    /// No session bus, no portal on it, or a portal that lacks the interface
    /// or the version the request needs.
    Unsupported(Unsupported),
    /// The portal or the bus failed, as the [`SystemFailure`] says.
    Failed(SystemFailure),
}

impl PortalError {
    /// The portal on this desktop lacks what the request needs: an interface
    /// its backend does not implement, or an older version of it. Trying
    /// again does not help.
    fn too_old() -> Self {
        Self::Unsupported(Unsupported::new(UnsupportedReason::NotOnThisDevice))
    }
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
                    zbus::fdo::Error::ServiceUnknown(_) | zbus::fdo::Error::NameHasNoOwner(_)
                ) =>
            {
                Self::Unsupported(Unsupported::new(UnsupportedReason::Unavailable))
            }
            zbus::Error::FDO(error)
                if matches!(
                    *error,
                    zbus::fdo::Error::UnknownInterface(_)
                        | zbus::fdo::Error::UnknownMethod(_)
                        | zbus::fdo::Error::UnknownObject(_)
                        | zbus::fdo::Error::UnknownProperty(_)
                ) =>
            {
                Self::too_old()
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

impl From<zbus::fdo::Error> for PortalError {
    fn from(error: zbus::fdo::Error) -> Self {
        Self::from(zbus::Error::from(error))
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

/// The task side of a request: when its caller let it go. The portal's own
/// retirement is one too, which every task of the portal shares
/// ([`Desktop::retired`]).
#[derive(Clone)]
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

/// The portal's interfaces, each at the version a request needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Interface {
    pub(crate) name: &'static str,
    /// The oldest version that has what the request uses.
    pub(crate) since: u32,
}

impl Interface {
    pub(crate) const fn new(name: &'static str, since: u32) -> Self {
        Self { name, since }
    }
}

/// The portal on the bus, as one request reaches it: the connection, and the
/// unique name that owns `org.freedesktop.portal.Desktop` now, which every
/// call of the request goes to and every signal it hears must come from.
pub(crate) struct Desktop {
    pub(crate) connection: zbus::Connection,
    pub(crate) owner: zbus::names::OwnedUniqueName,
    /// Ends when the UI runtime that owns the portal retires: a request
    /// still showing something closes it then.
    pub(crate) retired: Dismissal,
}

impl Desktop {
    /// Check that the portal has `interface` at its version at least, before
    /// a request depends on it: an absent interface or an older version is
    /// [`UnsupportedReason::NotOnThisDevice`], never a call that hangs or
    /// fails in a way nobody can tell from a broken portal.
    pub(crate) async fn require(&self, interface: Interface) -> Result<(), PortalError> {
        let properties = zbus::fdo::PropertiesProxy::builder(&self.connection)
            .destination(self.owner.as_str())
            .path(request::PATH)
            .build()
            .await?;
        let name = zbus::names::InterfaceName::try_from(interface.name)?;
        let version = match properties.get(name, "version").await {
            Ok(version) => version,
            // xdg-desktop-portal answers a property of an interface it does
            // not serve with `InvalidArgs`.
            Err(zbus::fdo::Error::InvalidArgs(_)) => return Err(PortalError::too_old()),
            Err(error) => return Err(error.into()),
        };
        let version = u32::try_from(version).map_err(|_| SystemFailure::MalformedAnswer);
        match version {
            Ok(version) if version >= interface.since => Ok(()),
            Ok(_) => Err(PortalError::too_old()),
            Err(failure) => Err(PortalError::Failed(failure)),
        }
    }
}

/// The application's id as the portal and the shell know it: the
/// composition root's checked `AppId` (ADR-0351), carried as its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AppName(Arc<str>);

impl AppName {
    /// The name the composition root gave the application, if any.
    fn of(ctx: &ProviderContext<'_>) -> Option<Self> {
        ctx.app_id().map(|id| Self(id.as_str().into()))
    }

    /// A scenario's name for the application.
    #[cfg(feature = "testing")]
    pub(crate) fn scripted(id: &str) -> Self {
        Self(id.into())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Where the application stands with the portal's `Registry`.
#[derive(Default)]
struct Registration {
    /// The portal instance the application was named to, or found it could
    /// not be named to; a portal that restarted needs naming again.
    settled_with: Option<zbus::names::OwnedUniqueName>,
}

/// The bus connection the requests of one portal share, made by the first
/// request that needs it: the session bus, or the bus at `address`.
pub(crate) struct Bus {
    address: Option<Arc<str>>,
    /// The application's id, which an unsandboxed application names itself
    /// to the portal by.
    app_id: Option<AppName>,
    connection: Mutex<Option<zbus::Connection>>,
    registration: Mutex<Registration>,
    /// Whether the process runs inside Flatpak or Snap, read once on the
    /// portal's thread.
    sandboxed: OnceLock<bool>,
    /// Ends with the UI runtime: dropped by the worker as it stops.
    retire: Mutex<Option<DismissOnDrop>>,
    retired: Dismissal,
    /// The portal's executor, whose clock a renewal waits on.
    timer: zruntime::SharedRuntime,
}

/// How long one call to the bus may take before it fails: the reference
/// D-Bus implementation's default, which bounds every call a request makes,
/// the `Close` sent as the UI runtime retires included.
const METHOD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);

impl Bus {
    fn new(
        address: Option<Arc<str>>,
        app_id: Option<AppName>,
        timer: zruntime::SharedRuntime,
    ) -> Self {
        let (retire, retired) = Dismissal::pair();
        Self {
            address,
            app_id,
            connection: Mutex::new(None),
            registration: Mutex::new(Registration::default()),
            sandboxed: OnceLock::new(),
            retire: Mutex::new(Some(retire)),
            retired,
            timer,
        }
    }

    /// The UI runtime retired: every request still showing something closes
    /// it, and every watch ends.
    fn retire(&self) {
        drop(lock(&self.retire).take());
    }

    /// Completes once the UI runtime retired.
    pub(crate) async fn retired(&self) {
        self.retired.dismissed().await;
    }

    /// Completes after `wait`, on the portal's executor.
    pub(crate) async fn sleep(&self, wait: std::time::Duration) {
        self.timer.sleep(wait).await;
    }

    pub(crate) async fn connection(&self) -> Result<zbus::Connection, PortalError> {
        let existing = lock(&self.connection).clone();
        // A bus that went away closes the connection for good; the next
        // request connects again rather than reuse it.
        if let Some(connection) = existing.filter(|connection| !connection.is_closed()) {
            return Ok(connection);
        }
        let builder = match &self.address {
            Some(address) => zbus::connection::Builder::address(&**address),
            None => zbus::connection::Builder::session(),
        };
        let connection = builder.method_timeout(METHOD_TIMEOUT).build().await?;
        // Two requests may both connect; the open one stored first wins and
        // the other connection closes with its last clone.
        let mut stored = lock(&self.connection);
        match &*stored {
            Some(winner) if !winner.is_closed() => Ok(winner.clone()),
            _ => {
                // A new connection is a new peer: the portal knows it by no
                // id yet.
                lock(&self.registration).settled_with = None;
                Ok(stored.insert(connection).clone())
            }
        }
    }

    /// The portal as a request reaches it: connected, its current owner
    /// found (an installed portal that is not running is started first), and
    /// the application named to that owner when it runs unsandboxed with an
    /// id (`org.freedesktop.host.portal.Registry`), which the portal takes
    /// only before the connection's first portal call.
    pub(crate) async fn desktop(&self) -> Result<Desktop, PortalError> {
        let connection = self.connection().await?;
        let bus = zbus::fdo::DBusProxy::new(&connection).await?;
        let owner = request::owner(&bus).await?;
        self.register(&connection, &owner).await;
        Ok(Desktop {
            connection,
            owner,
            retired: self.retired.clone(),
        })
    }

    /// Name the application to `owner` once per portal instance. A portal
    /// older than the registry, one that refuses (the connection called it
    /// before, or it judges the process sandboxed), or a sandbox that names
    /// the application itself, leaves the requests attributed as the portal
    /// sees fit: no request fails for it.
    async fn register(&self, connection: &zbus::Connection, owner: &zbus::names::OwnedUniqueName) {
        let Some(app_id) = &self.app_id else {
            return;
        };
        if *self.sandboxed.get_or_init(is_sandboxed) {
            return;
        }
        if lock(&self.registration).settled_with.as_ref() == Some(owner) {
            return;
        }
        let options = std::collections::HashMap::<&str, zbus::Variant<'_>>::new();
        let _ = connection
            .call_method(
                Some(owner.as_str()),
                request::PATH,
                Some("org.freedesktop.host.portal.Registry"),
                "Register",
                &(app_id.as_str(), options),
            )
            .await;
        lock(&self.registration).settled_with = Some(owner.clone());
    }
}

/// Whether the process runs in a sandbox that names it to the portal
/// itself: Flatpak (`/.flatpak-info`) or Snap (`SNAP` set by `snap run`).
fn is_sandboxed() -> bool {
    std::path::Path::new("/.flatpak-info").exists() || std::env::var_os("SNAP").is_some()
}

/// `mutex`'s guard: a panic while one is held leaves only a value that is
/// whole (a connection, a registration), never one half written.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The executor thread of one portal and the requests still running on it.
struct Worker {
    runtime: zruntime::SharedRuntime,
    stop: Arc<zruntime::Event>,
    bus: Arc<Bus>,
    tasks: Vec<zruntime::Task<(), zruntime::Shared>>,
}

impl Worker {
    fn start(address: Option<Arc<str>>, app_id: Option<AppName>) -> std::io::Result<Self> {
        let runtime = zruntime::SharedRuntime::new()?;
        let stop = Arc::new(zruntime::Event::new());
        let stopped = stop.listen();
        let driven = runtime.clone();
        std::thread::Builder::new()
            .name("flui-portal".to_owned())
            .spawn(move || driven.block_on(stopped))?;
        let bus = Arc::new(Bus::new(address, app_id, runtime.clone()));
        Ok(Self {
            runtime,
            stop,
            bus,
            tasks: Vec::new(),
        })
    }
}

/// How long the portal's thread outlives its UI runtime at most, while its
/// requests close what they show.
const DRAIN_BOUND: std::time::Duration = std::time::Duration::from_secs(30);

impl Drop for Worker {
    fn drop(&mut self) {
        // Each request hears the retirement, closes its `Request` or
        // `Session` (the dialog goes down, the shortcuts are unbound) and
        // ends, dropping its resolver and the window parent it holds; the
        // thread stops once all have, or at the bound, cancelling the rest.
        self.bus.retire();
        let tasks = std::mem::take(&mut self.tasks);
        let stop = Arc::clone(&self.stop);
        let timer = self.runtime.clone();
        self.runtime
            .spawn("portal drain", async move {
                let drained = std::pin::pin!(async {
                    for task in tasks {
                        // A request that panicked has ended all the same.
                        let _ = task.await;
                    }
                });
                let bound = std::pin::pin!(timer.sleep(DRAIN_BOUND));
                let _ = futures_util::future::select(drained, bound).await;
                stop.notify(usize::MAX);
            })
            .detach();
    }
}

/// One UI runtime's way to the desktop portal: started on first use and
/// ended with the runtime.
#[derive(Default)]
pub(crate) struct Portal {
    worker: RefCell<Option<Worker>>,
    next_token: Cell<u64>,
    address: RefCell<Option<Arc<str>>>,
    app_id: RefCell<Option<AppName>>,
}

impl Portal {
    /// Talk to the portal on the bus at `address` instead of the session
    /// bus, naming the application `app_id`, from the next start on.
    pub(crate) fn configure(&self, address: Option<Arc<str>>, app_id: Option<AppName>) {
        *self.address.borrow_mut() = address;
        *self.app_id.borrow_mut() = app_id;
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
            let started =
                Worker::start(self.address.borrow().clone(), self.app_id.borrow().clone());
            *worker = Some(started.map_err(|_| {
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

/// Hand `then` the parent a portal request names, on the owner thread: no
/// window at once for a request with no window, else what the window answers
/// ([`WindowRef::portal_parent`]), a turn later. A window whose host let the
/// answer go names no window. When the owner retires first, `then` is
/// dropped unrun, and with it whatever it holds.
pub(crate) fn with_parent(
    window: Option<&WindowRef>,
    completions: &CompletionSource,
    then: impl FnOnce(PortalParent) + 'static,
) {
    let Some(window) = window else {
        then(PortalParent::none());
        return;
    };
    let (parent, answer) = completions.pending::<PortalParent>();
    window.portal_parent(answer);
    let mut then = Some(then);
    parent.on_complete(move |outcome| {
        if let Some(then) = then.take() {
            then(outcome.unwrap_or_default());
        }
    });
}

/// Register the portal's providers: on the session bus, or on the bus at
/// `address`, naming the application by `app_id` when given, else by the id
/// the composition root names it by.
pub(crate) fn register(
    registrar: &mut CapabilityRegistrar,
    address: Option<Arc<str>>,
    app_id: Option<AppName>,
) {
    let configure = move |ctx: &ProviderContext<'_>| {
        let portal = ctx.runtime_local::<Portal>();
        portal.configure(address.clone(), app_id.clone().or_else(|| AppName::of(ctx)));
        portal
    };
    let launcher = configure.clone();
    registrar.provide::<LauncherCapability>(move |ctx: &ProviderContext<'_>| {
        Ok::<_, Unsupported>(Launcher::new(
            PortalLauncher::new(launcher(ctx), ctx.window(), ctx.completions().clone()),
            ctx,
        ))
    });
    let dialogs = configure.clone();
    registrar.provide::<FileDialogCapability>(move |ctx: &ProviderContext<'_>| {
        FileDialogs::try_new(
            PortalDialogs::new(dialogs(ctx), ctx.completions().clone()),
            ctx,
        )
    });
    let autostart = configure.clone();
    registrar.provide::<AutostartCapability>(move |ctx: &ProviderContext<'_>| {
        Ok::<_, Unsupported>(Autostart::new(
            PortalAutostart::new(autostart(ctx), ctx),
            ctx,
        ))
    });
    let shortcuts = configure.clone();
    registrar.provide::<GlobalShortcutCapability>(move |ctx: &ProviderContext<'_>| {
        Ok::<_, Unsupported>(GlobalShortcuts::new(
            PortalShortcuts::new(shortcuts(ctx), ctx),
            ctx,
        ))
    });
    let entry = configure.clone();
    registrar.provide::<TaskbarCapability>(move |ctx: &ProviderContext<'_>| {
        let portal = entry(ctx);
        let app_id = portal.app_id.borrow().clone();
        LauncherEntry::try_new(portal, app_id).map(|entry| TaskbarEntry::new(entry, ctx))
    });
    let dirs = configure;
    registrar.provide::<UserDirCapability>(move |ctx: &ProviderContext<'_>| {
        Ok::<_, Unsupported>(UserDirs::new(XdgUserDirs::new(dirs(ctx)), ctx))
    });
}
