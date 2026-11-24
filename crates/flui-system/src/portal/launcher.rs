//! The launcher over the `OpenURI` and `Trash` portals.

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use flui_platform::SystemFailure;
use flui_platform::capability::{CompletionSource, Resolver, WindowRef};
use flui_platform::launch::{
    ExternalUri, LaunchError, LaunchResult, LauncherBackend, runs_when_opened,
};
use zbus::Variant;

use super::request::{self, Response};
use super::{Interface, Portal, PortalError, with_parent};

/// `OpenURI` at the version each method needs: `OpenFile` came with 2,
/// `OpenDirectory` with 3.
const OPEN_URI: Interface = Interface::new("org.freedesktop.portal.OpenURI", 1);
const OPEN_FILE: Interface = Interface::new("org.freedesktop.portal.OpenURI", 2);
const OPEN_DIRECTORY: Interface = Interface::new("org.freedesktop.portal.OpenURI", 3);
const TRASH: Interface = Interface::new("org.freedesktop.portal.Trash", 1);

/// [`LauncherBackend`] over the desktop portal.
///
/// Every request touches the file system on a blocking pool thread, never
/// on the owner's or the portal's: a file on a stalled mount delays that
/// request alone, and its answer goes to the resolver of the request that
/// asked.
pub(crate) struct PortalLauncher {
    portal: Rc<Portal>,
    /// The acquiring widget's window, which the portal's application
    /// chooser is attached to; `None` for a widget in no window.
    window: Option<WindowRef>,
    completions: CompletionSource,
}

impl PortalLauncher {
    pub(crate) fn new(
        portal: Rc<Portal>,
        window: Option<WindowRef>,
        completions: CompletionSource,
    ) -> Self {
        Self {
            portal,
            window,
            completions,
        }
    }

    /// Ask the `OpenURI` method `method` of `interface`, which answers
    /// through a `Request`, attached to the acquiring widget's window while
    /// it is open; the body `argument` makes on the portal's thread from the
    /// parent and the request's token, and a refusal it answers instead
    /// reaches `answer` from there. The request holds the window's parent
    /// until it ends.
    fn ask_open_uri<A>(
        &self,
        interface: Interface,
        method: &'static str,
        answer: Resolver<LaunchResult>,
        argument: A,
    ) where
        A: FnOnce(&str, String) -> Result<OpenUriBody, LaunchError> + Send + 'static,
    {
        let reservation = match self.portal.reserve() {
            Ok(reservation) => reservation,
            Err(error) => {
                let _ = answer.resolve(Err(launch_error(error)));
                return;
            }
        };
        let portal = Rc::clone(&self.portal);
        with_parent(self.window.as_ref(), &self.completions, move |parent| {
            let token = portal.token();
            portal.spawn_reserved(reservation, move |bus| async move {
                let named = parent.as_str().to_owned();
                let asked = token.clone();
                let made = zruntime::unblock(move || argument(&named, asked)).await;
                let body = match made {
                    Ok(body) => body,
                    Err(refusal) => {
                        let _ = answer.resolve(Err(refusal));
                        return;
                    }
                };
                let result = async {
                    let desktop = bus.desktop().await?;
                    match body {
                        OpenUriBody::Uri(body) => {
                            request::call(&desktop, interface, method, &token, &body, None).await
                        }
                        OpenUriBody::File(body) => {
                            request::call(&desktop, interface, method, &token, &body, None).await
                        }
                    }
                }
                .await;
                // The window's export lasts until the request ended.
                drop(parent);
                let _ = answer.resolve(match result {
                    Ok(Response::Answered(_)) => Ok(()),
                    // The user dismissed the application chooser.
                    Ok(Response::Cancelled) => Err(LaunchError::Denied),
                    Ok(Response::Ended(code)) => Err(SystemFailure::PortalResponse(code).into()),
                    // The UI runtime retired: nobody hears the answer.
                    Ok(Response::Dismissed) => return,
                    Err(error) => Err(launch_error(error)),
                });
            });
        });
    }

    fn ask_with_file(
        &self,
        interface: Interface,
        method: &'static str,
        purpose: Purpose,
        path: PathBuf,
        answer: Resolver<LaunchResult>,
    ) {
        self.ask_open_uri(interface, method, answer, move |parent, token| {
            let file = open(&path, purpose)?;
            Ok(OpenUriBody::File((
                parent.to_owned(),
                zbus::OwnedFd::from(std::os::fd::OwnedFd::from(file)),
                options(&token),
            )))
        });
    }
}

/// Open `path` for the portal, which takes a descriptor rather than a
/// path, so a sandboxed application hands over only what it can reach.
///
/// Never `O_PATH`: the portal refuses it from an application kind that has
/// not opted in, a Snap among them. To open or show a file the descriptor is
/// read-only and follows links; an open then judges the file the descriptor
/// holds ([`judge_opened`]), so the portal is handed exactly what was judged.
/// To trash one it is the directory entry itself, so a symbolic link is
/// refused instead of followed (the portal acts on the path its descriptor
/// resolves to, and trashing a link must not trash its target), and
/// read-write where the file allows: the portal trashes a sandboxed
/// application's file only through a writable descriptor. A directory, or a
/// file this process may not write, goes read-only, which the portal accepts
/// from an unsandboxed application.
///
/// Non-blocking: opening a named pipe with no writer would otherwise hold
/// the portal's thread until one comes. The portal is never read through
/// the descriptor, so the mode costs nothing.
fn open(path: &Path, purpose: Purpose) -> Result<File, LaunchError> {
    let open = |write: bool, follow: bool| {
        let flags = libc::O_CLOEXEC | libc::O_NONBLOCK | if follow { 0 } else { libc::O_NOFOLLOW };
        std::fs::OpenOptions::new()
            .read(true)
            .write(write)
            .custom_flags(flags)
            .open(path)
    };
    let opened = match purpose {
        Purpose::Open | Purpose::Reveal => open(false, true),
        Purpose::Trash => open(true, false).or_else(|error| match error.raw_os_error() {
            Some(libc::EISDIR | libc::EACCES | libc::EPERM | libc::EROFS | libc::ETXTBSY) => {
                open(false, false)
            }
            _ => Err(error),
        }),
    };
    let file = opened.map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => LaunchError::NotFound,
        io::ErrorKind::PermissionDenied => LaunchError::Denied,
        // `O_NOFOLLOW` met a symbolic link.
        _ if error.raw_os_error() == Some(libc::ELOOP) => LaunchError::Denied,
        // std answers without an errno only for a path it cannot hand the
        // system (an interior NUL), and such a path names no file.
        _ => error
            .raw_os_error()
            .map_or(LaunchError::NotFound, |code| SystemFailure::Os(code).into()),
    })?;
    if purpose == Purpose::Open {
        judge_opened(&file)?;
    }
    Ok(file)
}

/// Whether the file `file` holds may be opened as a document: its resolved
/// name (`/proc/self/fd`, which follows every link the open followed) is not
/// one that [`runs_when_opened`], a desktop entry included, and it is not a
/// regular file with an execute bit, which a file manager may run. A
/// directory is a folder to show, whatever its bits.
fn judge_opened(file: &File) -> Result<(), LaunchError> {
    // Both calls read a descriptor this process holds open; a failure is
    // the system's, named by its errno.
    let failed = |error: io::Error| -> LaunchError {
        SystemFailure::Os(error.raw_os_error().unwrap_or(libc::EIO)).into()
    };
    let resolved =
        std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd())).map_err(failed)?;
    if runs_when_opened(&resolved) {
        return Err(LaunchError::NotADocument);
    }
    let metadata = file.metadata().map_err(failed)?;
    if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
        return Err(LaunchError::NotADocument);
    }
    Ok(())
}

type Options = HashMap<&'static str, Variant<'static>>;

enum OpenUriBody {
    Uri((String, String, Options)),
    File((String, zbus::OwnedFd, Options)),
}

fn options(token: &str) -> Options {
    HashMap::from([("handle_token", Variant::from(token.to_owned()))])
}

fn launch_error(error: PortalError) -> LaunchError {
    match error {
        PortalError::Unsupported(unsupported) => LaunchError::Unsupported(unsupported),
        PortalError::Failed(failure) => LaunchError::Failed(failure),
    }
}

impl LauncherBackend for PortalLauncher {
    fn open_uri(&self, uri: ExternalUri, answer: Resolver<LaunchResult>) {
        let uri = uri.as_str().to_owned();
        self.ask_open_uri(OPEN_URI, "OpenURI", answer, move |parent, token| {
            Ok(OpenUriBody::Uri((parent.to_owned(), uri, options(&token))))
        });
    }

    fn open_path(&self, path: PathBuf, answer: Resolver<LaunchResult>) {
        self.ask_with_file(OPEN_FILE, "OpenFile", Purpose::Open, path, answer);
    }

    fn reveal_path(&self, path: PathBuf, answer: Resolver<LaunchResult>) {
        self.ask_with_file(
            OPEN_DIRECTORY,
            "OpenDirectory",
            Purpose::Reveal,
            path,
            answer,
        );
    }

    /// Trashing shows nothing, so it names no window.
    fn trash_path(&self, path: PathBuf, answer: Resolver<LaunchResult>) {
        let reservation = match self.portal.reserve() {
            Ok(reservation) => reservation,
            Err(error) => {
                let _ = answer.resolve(Err(launch_error(error)));
                return;
            }
        };
        self.portal
            .spawn_reserved(reservation, move |bus| async move {
                let opened = zruntime::unblock(move || open(&path, Purpose::Trash)).await;
                let file = match opened {
                    Ok(file) => zbus::OwnedFd::from(std::os::fd::OwnedFd::from(file)),
                    Err(refusal) => {
                        let _ = answer.resolve(Err(refusal));
                        return;
                    }
                };
                let result = async {
                    let desktop = bus.desktop().await?;
                    desktop.require(TRASH).await?;
                    let reply = desktop
                        .connection
                        .call_method(
                            Some(desktop.owner.as_str()),
                            request::PATH,
                            Some(TRASH.name),
                            "TrashFile",
                            &(file,),
                        )
                        .await?;
                    Ok::<_, PortalError>(reply.body().deserialize::<u32>()?)
                }
                .await;
                let _ = answer.resolve(match result {
                    Ok(1) => Ok(()),
                    // The portal does not say why; a file it may not trash is
                    // the usual reason.
                    Ok(_) => Err(LaunchError::Denied),
                    Err(error) => Err(launch_error(error)),
                });
            });
    }
}

/// What a descriptor handed to the portal is for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Purpose {
    /// Opening: a symbolic link opens its target, which is judged.
    Open,
    /// Revealing: a symbolic link shows its target; nothing runs.
    Reveal,
    /// Trashing: the directory entry itself; a symbolic link is refused.
    Trash,
}
