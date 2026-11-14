//! One instance of an application on a Windows session (the activation spec,
//! design §5; ADR-0254): the first process of an application holds a mutex
//! and serves a named pipe, both named by its app id and the user's session;
//! a later one, finding the mutex, hands the first the right to come to the
//! foreground and what its own launch opens ([`Forwarded`], the pipe's
//! frame), and starts no platform.
//!
//! The mutex and the pipe grant the current user alone (a DACL of their
//! SID), the pipe rejects remote clients, and the frame is bounded and read
//! as untrusted input ([`frame`](crate::owner::single_instance::frame)). A
//! later launch hands its launch only to a pipe served by a process of its
//! own user: a pipe another user made under the name first is not the
//! application's.
//!
//! The pipe is served on the host's IO runtime; what a launch opens reaches
//! the owner as a message posted to its message-only window
//! ([`FORWARDED_LAUNCHES`]), which delivers it to the application and brings
//! a window forward.

use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, PipeMode, ServerOptions};
use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, GetLastError, HANDLE, HLOCAL,
    LRESULT, LocalFree,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId};
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, WM_APP};
use windows::core::HSTRING;

use super::control::{OwnerMessages, OwnerPost, OwnerWindow};
use crate::executor::NativeExecutor;
pub(crate) use crate::owner::single_instance::frame::Forwarded;
use crate::owner::single_instance::frame::{self, FrameError};
use crate::owner::single_instance::{HOLD, Inbox, MAX_WAITING, OwnerWake};

/// The first instance's answer to a launch it took.
const TAKEN: u8 = 1;
/// The first instance's answer to a launch it refused.
const REFUSED: u8 = 0;

/// The owner window's message for the launches that wait: posted as a launch is
/// queued, it delivers every launch waiting.
pub(crate) const FORWARDED_LAUNCHES: u32 = WM_APP + 23;

/// How long a later launch tries to reach the first instance's pipe: the
/// first may hold the mutex and not serve the pipe yet, or serve every
/// instance of it.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// How long between two tries to reach the pipe.
const CONNECT_RETRY: Duration = Duration::from_millis(20);
/// How long a later process waits for the first to take its launch: longer
/// than the first's [`HOLD`], so it hears the answer before it stops
/// waiting.
const FORWARD_TIMEOUT: Duration = Duration::from_secs(20);
/// How long the first waits for a launch that reached its pipe to send its
/// frame, and for its caller to read the answer.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// What a launch found.
pub(crate) enum Claim {
    /// This process holds the mutex: it is the first instance.
    First(FirstInstance),
    /// Another process is, and took this launch.
    Forwarded,
}

/// The first instance: it holds the mutex and serves the pipe while it
/// lives.
pub(crate) struct FirstInstance {
    /// The task that serves the pipe; the pipe goes when it ends.
    serving: tokio::task::JoinHandle<()>,
    inbox: Arc<Inbox<Forwarded>>,
    /// The mutex, closed after the task is told to end.
    _mutex: OwnedHandle,
    /// The runtime the pipe is served on, kept while the instance lives:
    /// `flui-app` hands a Windows host an executor of its own, whose last
    /// clone shuts its runtime down, and the host keeps none. Dropped last.
    _executor: NativeExecutor,
}

impl std::fmt::Debug for FirstInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FirstInstance").finish_non_exhaustive()
    }
}

impl Drop for FirstInstance {
    /// The loop ended, or never started: no launch is read from now on, each
    /// launch it never took is refused to its caller, which runs as an
    /// instance of its own, one it took is answered as taken, and the mutex
    /// goes, then the pipe once those answers are sent.
    fn drop(&mut self) {
        // First, so a subscriber that panics below cannot leave the pipe
        // served: a later launch finds no instance and runs on its own.
        self.serving.abort();
        let count = self.inbox.refuse_waiting();
        if count > 0 {
            tracing::warn!(
                launches = count,
                "the instance ends with forwarded launches it never took; they are refused"
            );
        }
    }
}

impl FirstInstance {
    /// The owner window's part of the first instance: it delivers what
    /// later launches open to the application and brings a window forward.
    pub(crate) fn into_hook(self) -> Arc<dyn OwnerMessages> {
        Arc::new(LaunchHook { instance: self })
    }
}

/// Wakes the owner by posting [`FORWARDED_LAUNCHES`] to its window.
struct PostWake(OwnerPost);

impl OwnerWake for PostWake {
    fn wake(&self) -> bool {
        self.0.post(FORWARDED_LAUNCHES)
    }
}

/// The first instance's part of the owner window. It holds no owner state:
/// the window hands it what a message reaches.
struct LaunchHook {
    instance: FirstInstance,
}

impl LaunchHook {
    /// Deliver the launches later processes forwarded: what each opens
    /// reaches the application as running, then the shown window highest
    /// in the order comes forward, with the foreground right the launch
    /// handed over.
    fn deliver(&self, owner: &OwnerWindow) {
        // Without the owner's state the platform is ending: what waits is
        // then refused as the instance ends, never taken without its
        // delivery.
        let (Some(app), Some(windows)) = (owner.app(), owner.windows()) else {
            return;
        };
        for forwarded in self.instance.inbox.take() {
            // An application that shows no window (one that outlives its
            // last, or hid it) is asked to reopen, as a Dock click asks on
            // macOS; the window it shows comes forward with the right.
            if !windows.shows_any() {
                app.reopen()
                    .or_contain((), "a launch another instance forwarded");
            }
            let opened = forwarded.opened();
            if !opened.is_empty() {
                app.opened(opened)
                    .or_contain((), "a launch another instance forwarded");
            }
            windows.activate_forwarded();
        }
    }
}

impl OwnerMessages for LaunchHook {
    fn attach(&self, owner: OwnerPost) {
        self.instance
            .inbox
            .attach(Arc::new(PostWake(owner)) as Arc<dyn OwnerWake>);
    }

    fn message(&self, owner: &OwnerWindow, message: u32) -> Option<LRESULT> {
        (message == FORWARDED_LAUNCHES).then(|| {
            self.deliver(owner);
            LRESULT(0)
        })
    }

    fn destroyed(&self) {
        // The owner's state is gone: what waits is never delivered, and a
        // launch from now on is refused as its wake fails.
        let count = self.instance.inbox.refuse_waiting();
        if count > 0 {
            tracing::warn!(
                launches = count,
                "the owner window closed with forwarded launches it never took; they are refused"
            );
        }
    }
}

/// The mutex's and the pipe's names for the instance `name` in `session`.
fn object_names(name: &str, session: u32) -> (String, String) {
    (
        format!(r"Local\flui-{name}-{session}"),
        format!(r"\\.\pipe\flui-{name}-{session}"),
    )
}

/// The session this process runs in.
fn session_id() -> std::io::Result<u32> {
    let mut session = 0_u32;
    // SAFETY: `GetCurrentProcessId` takes nothing; `session` is a live
    // out-pointer for the call.
    unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &raw mut session) }
        .map_err(std::io::Error::from)?;
    Ok(session)
}

/// A security descriptor made from SDDL, freed when dropped.
struct Security(PSECURITY_DESCRIPTOR);

impl Security {
    /// The descriptor `sddl` states.
    fn from_sddl(sddl: &str) -> std::io::Result<Self> {
        let sddl = HSTRING::from(sddl);
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `sddl` is a NUL-terminated wide string that outlives the
        // call; `descriptor` is a live out-pointer the call fills with a
        // LocalAlloc'd descriptor this value owns.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &sddl,
                SDDL_REVISION_1,
                &raw mut descriptor,
                None,
            )
        }
        .map_err(std::io::Error::from)?;
        Ok(Self(descriptor))
    }

    /// Attributes that apply the descriptor to an object made with them,
    /// whose handle is not inherited; valid while `self` lives.
    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: u32::try_from(std::mem::size_of::<SECURITY_ATTRIBUTES>())
                .expect("BUG: SECURITY_ATTRIBUTES is a few bytes"),
            lpSecurityDescriptor: self.0.0,
            bInheritHandle: false.into(),
        }
    }
}

impl Drop for Security {
    fn drop(&mut self) {
        // SAFETY: the descriptor was allocated by the system for this value,
        // is freed once, and is not read after.
        let _: HLOCAL = unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

/// The mutex `name`, granted as `sddl` states when this call makes it, and
/// whether it existed already.
fn open_mutex(name: &str, sddl: &str) -> std::io::Result<(OwnedHandle, bool)> {
    let security = Security::from_sddl(sddl)?;
    let attributes = security.attributes();
    let name = HSTRING::from(name);
    // SAFETY: `attributes` and the descriptor it points to live past the
    // call; `name` is a NUL-terminated wide string. The last error is read
    // at once, before anything else can set it.
    let (handle, existed) = unsafe {
        let handle = CreateMutexW(Some(&raw const attributes), false, &name)
            .map_err(std::io::Error::from)?;
        (handle, GetLastError() == ERROR_ALREADY_EXISTS)
    };
    // SAFETY: the handle was just opened for this process and is owned by
    // nothing else.
    let handle = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    Ok((handle, existed))
}

/// An instance of the pipe `name`, granted as `sddl` states; `first` when it
/// must be the pipe's first, which fails if any instance exists already.
/// Called inside the runtime that serves it.
fn pipe_instance(name: &str, sddl: &str, first: bool) -> std::io::Result<NamedPipeServer> {
    let security = Security::from_sddl(sddl)?;
    let mut attributes = security.attributes();
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        .access_inbound(true)
        .access_outbound(true)
        .pipe_mode(PipeMode::Byte);
    // SAFETY: `attributes` is a live, initialised `SECURITY_ATTRIBUTES`
    // whose descriptor lives past the call, which reads both only while it
    // runs.
    unsafe { options.create_with_security_attributes_raw(name, (&raw mut attributes).cast()) }
}

/// Claim the instance `name` for this process, or hand the instance that
/// holds it `launch`.
///
/// # Errors
///
/// The system's error when the session, the user, the mutex or the pipe
/// cannot be had, the first instance cannot be reached in time, is not of
/// this user or refused the launch: the caller then starts as an instance of
/// its own, so the launch is not lost.
pub(crate) fn claim(
    executor: &NativeExecutor,
    name: &str,
    launch: &Forwarded,
) -> std::io::Result<Claim> {
    let session = session_id()?;
    let user = crate::process::current_user_sid()?;
    let (mutex_name, pipe_name) = object_names(name, session);
    // The current user alone, named by SID (`current_user_sid` says why).
    let sddl = format!("D:P(A;;GA;;;{user})");
    let (mutex, existed) = open_mutex(&mutex_name, &sddl)?;
    if !existed {
        let host = executor.handle();
        // Made here, inside the host's runtime, so a later launch finds the
        // pipe once this call returns.
        let server = {
            let _inside = host.enter();
            pipe_instance(&pipe_name, &sddl, true)?
        };
        let inbox = Arc::new(Inbox::new(HOLD, host.clone()));
        let serving = host.spawn(serve(server, pipe_name, sddl, Arc::clone(&inbox)));
        return Ok(Claim::First(FirstInstance {
            serving,
            inbox,
            _mutex: mutex,
            _executor: executor.clone(),
        }));
    }
    drop(mutex);
    let frame = launch.encode().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the launch is past what the running instance takes",
        )
    })?;
    // The launch waits on a thread and a runtime of its own, as a Linux
    // launch does: the caller may be inside the host's runtime, where
    // `Handle::block_on` panics.
    std::thread::scope(|scope| {
        let forwarding = std::thread::Builder::new()
            .name("flui-single-instance".into())
            .spawn_scoped(scope, || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?
                    .block_on(async {
                        tokio::time::timeout(FORWARD_TIMEOUT, forward(&pipe_name, &user, &frame))
                            .await
                            .map_err(|_| {
                                std::io::Error::new(
                                    std::io::ErrorKind::TimedOut,
                                    "the running instance did not answer in time",
                                )
                            })?
                    })
            })?;
        forwarding
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    })?;
    Ok(Claim::Forwarded)
}

/// Hand `frame` to the process serving `pipe_name`, once it is known to run
/// as `user`, with the right to come to the foreground.
async fn forward(pipe_name: &str, user: &str, frame: &[u8]) -> std::io::Result<()> {
    let mut pipe = connect(pipe_name).await?;
    let mut server = 0_u32;
    // SAFETY: the handle is the open client end of the pipe, which `pipe`
    // keeps open past the call; `server` is a live out-pointer for it.
    unsafe { GetNamedPipeServerProcessId(HANDLE(pipe.as_raw_handle()), &raw mut server) }
        .map_err(std::io::Error::from)?;
    if crate::process::process_user_sid(server)? != user {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "the application's pipe is served by another user",
        ));
    }
    // The right goes with the launch: the first brings its window forward
    // with it. A process that holds no right to give is refused, and the
    // first's window then asks for attention instead.
    // SAFETY: a plain process id; the call reads nothing of this process.
    if let Err(error) = unsafe { AllowSetForegroundWindow(server) } {
        tracing::debug!(%error, "this launch holds no foreground right to hand over");
    }
    pipe.write_all(frame).await?;
    let mut answer = [REFUSED];
    pipe.read_exact(&mut answer).await?;
    if answer[0] == TAKEN {
        Ok(())
    } else {
        Err(std::io::Error::other(
            "the running instance refused the launch",
        ))
    }
}

/// The client end of `pipe_name`, once an instance of it is free, within
/// [`CONNECT_TIMEOUT`].
async fn connect(
    pipe_name: &str,
) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    let deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
    loop {
        match ClientOptions::new().open(pipe_name) {
            Ok(pipe) => return Ok(pipe),
            Err(error)
                if [ERROR_PIPE_BUSY, ERROR_FILE_NOT_FOUND]
                    .into_iter()
                    .any(|waits| error.raw_os_error() == i32::try_from(waits.0).ok())
                    && tokio::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(CONNECT_RETRY).await;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Why a launch's frame was not read.
#[derive(Debug, thiserror::Error)]
enum ReadError {
    #[error(transparent)]
    Pipe(#[from] std::io::Error),
    #[error(transparent)]
    Frame(#[from] FrameError),
}

/// Serve the pipe `name` from `server`, its first instance, until the task
/// is aborted. Each launch that reaches it is read and answered by a task of
/// its own, which aborting this one does not end: a launch the owner took as
/// the instance ends is still answered as taken, so its process does not
/// open it a second time. At most [`MAX_WAITING`] launches are under way; a
/// later one waits for the pipe.
async fn serve(
    mut server: NamedPipeServer,
    name: String,
    sddl: String,
    inbox: Arc<Inbox<Forwarded>>,
) {
    let answering = Arc::new(tokio::sync::Semaphore::new(MAX_WAITING));
    loop {
        let Ok(room) = Arc::clone(&answering).acquire_owned().await else {
            return;
        };
        let connected = server.connect().await;
        // The next instance is made before this one is handed on, so the
        // pipe's name never lapses while the instance serves.
        let next = match pipe_instance(&name, &sddl, false) {
            Ok(next) => next,
            Err(error) => {
                tracing::warn!(
                    %error,
                    "the running instance cannot serve its pipe again; later launches run on \
                     their own"
                );
                if connected.is_ok() {
                    tokio::spawn(answer(server, Arc::clone(&inbox), room));
                }
                return;
            }
        };
        let reached = std::mem::replace(&mut server, next);
        match connected {
            Ok(()) => {
                tokio::spawn(answer(reached, Arc::clone(&inbox), room));
            }
            Err(error) => {
                tracing::debug!(%error, "a later launch left before it was read");
            }
        }
    }
}

/// Read the launch on `pipe`, hand it to the owner and answer whether the
/// owner took it, holding `_room`, its place among the launches under way.
async fn answer(
    mut pipe: NamedPipeServer,
    inbox: Arc<Inbox<Forwarded>>,
    _room: tokio::sync::OwnedSemaphorePermit,
) {
    let read = tokio::time::timeout(READ_TIMEOUT, read_launch(&mut pipe)).await;
    let answered = match read {
        Err(_) => {
            tracing::debug!("a later launch did not send its frame in time");
            REFUSED
        }
        Ok(Err(error)) => {
            tracing::debug!(%error, "a later launch's frame is refused");
            REFUSED
        }
        Ok(Ok(forwarded)) => match inbox.post(forwarded) {
            Err(refused) => {
                tracing::debug!(reason = refused.reason(), "a later launch is refused");
                REFUSED
            }
            Ok(taken) => match inbox.answer(taken).await {
                Ok(()) => TAKEN,
                Err(reason) => {
                    tracing::debug!(reason, "a later launch is handed back");
                    REFUSED
                }
            },
        },
    };
    if let Err(error) = pipe.write_all(&[answered]).await {
        tracing::debug!(%error, "a later launch was not answered: its caller left");
        return;
    }
    // The instance stays open until the caller closes its end, so the answer
    // is read before the instance goes.
    let mut end = [0_u8; 1];
    let _ = tokio::time::timeout(READ_TIMEOUT, pipe.read(&mut end)).await;
}

/// The launch a later process wrote on `pipe`: its length is checked before
/// the body is read.
async fn read_launch(pipe: &mut NamedPipeServer) -> Result<Forwarded, ReadError> {
    let mut prefix = [0_u8; 4];
    pipe.read_exact(&mut prefix).await?;
    let mut body = vec![0_u8; frame::body_len(prefix)?];
    pipe.read_exact(&mut body).await?;
    Ok(Forwarded::decode(&body)?)
}
