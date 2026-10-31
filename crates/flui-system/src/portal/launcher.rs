//! The launcher over the `OpenURI` and `Trash` portals.

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use flui_platform::capability::Resolver;
use flui_platform::launch::{ExternalUri, LaunchError, LaunchResult, LauncherBackend};
use zbus::Variant;

use super::request::{self, DESTINATION, PATH, Response};
use super::{Portal, PortalError};

const OPEN_URI: &str = "org.freedesktop.portal.OpenURI";

/// [`LauncherBackend`] over the desktop portal.
pub(crate) struct PortalLauncher {
    portal: Rc<Portal>,
}

impl PortalLauncher {
    pub(crate) fn new(portal: Rc<Portal>) -> Self {
        Self { portal }
    }

    /// Open `path` for the portal, which takes a descriptor rather than a
    /// path, so a sandboxed application hands over only what it can reach.
    ///
    /// A plain read-only descriptor, not `O_PATH`: the portal refuses
    /// `O_PATH` from an application kind that has not opted in, a Snap among
    /// them. `entry` opens the directory entry itself: a symbolic link there
    /// is refused instead of followed, since the portal acts on the path its
    /// descriptor resolves to, and trashing a link must not trash its target.
    fn open(path: &Path, entry: Entry) -> Result<File, LaunchError> {
        let flags = match entry {
            Entry::FollowLinks => libc::O_CLOEXEC,
            Entry::Itself => libc::O_CLOEXEC | libc::O_NOFOLLOW,
        };
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(flags)
            .open(path)
            .map_err(|error| match error.kind() {
                io::ErrorKind::NotFound => LaunchError::NotFound,
                io::ErrorKind::PermissionDenied => LaunchError::Denied,
                // `O_NOFOLLOW` met a symbolic link.
                _ if error.raw_os_error() == Some(libc::ELOOP) => LaunchError::Denied,
                _ => LaunchError::Failed {
                    code: error.raw_os_error().unwrap_or(0),
                },
            })
    }

    /// Ask an `OpenURI` method that answers through a `Request`.
    fn ask_open_uri<A>(&self, method: &'static str, answer: Resolver<LaunchResult>, argument: A)
    where
        A: FnOnce(String) -> Result<OpenUriBody, LaunchError>,
    {
        let token = self.portal.token();
        let body = argument(token.clone()).and_then(|body| {
            self.portal.start().map_err(launch_error)?;
            Ok(body)
        });
        let body = match body {
            Ok(body) => body,
            Err(error) => {
                let _ = answer.resolve(Err(error));
                return;
            }
        };
        self.portal.spawn(move |bus| async move {
            let result = async {
                let connection = bus.connection().await?;
                let response = match body {
                    OpenUriBody::Uri(body) => {
                        request::call(&connection, OPEN_URI, method, &token, &body).await?
                    }
                    OpenUriBody::File(body) => {
                        request::call(&connection, OPEN_URI, method, &token, &body).await?
                    }
                };
                Ok::<_, PortalError>(response)
            }
            .await;
            let _ = answer.resolve(match result {
                Ok(Response::Answered(_)) => Ok(()),
                // The user dismissed the application chooser.
                Ok(Response::Cancelled) => Err(LaunchError::Denied),
                Ok(Response::Ended) => Err(LaunchError::NoHandler),
                Err(error) => Err(launch_error(error)),
            });
        });
    }

    fn ask_with_file(&self, method: &'static str, path: PathBuf, answer: Resolver<LaunchResult>) {
        self.ask_open_uri(method, answer, move |token| {
            let file = Self::open(&path, Entry::FollowLinks)?;
            Ok(OpenUriBody::File((
                String::new(),
                zbus::OwnedFd::from(std::os::fd::OwnedFd::from(file)),
                options(&token),
            )))
        });
    }
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
        PortalError::Unavailable(reason) => LaunchError::Unavailable(reason),
        PortalError::Failed => LaunchError::Failed { code: 0 },
    }
}

impl LauncherBackend for PortalLauncher {
    fn open_uri(&self, uri: ExternalUri, answer: Resolver<LaunchResult>) {
        let uri = uri.as_str().to_owned();
        self.ask_open_uri("OpenURI", answer, move |token| {
            Ok(OpenUriBody::Uri((String::new(), uri, options(&token))))
        });
    }

    fn open_path(&self, path: PathBuf, answer: Resolver<LaunchResult>) {
        self.ask_with_file("OpenFile", path, answer);
    }

    fn reveal_path(&self, path: PathBuf, answer: Resolver<LaunchResult>) {
        self.ask_with_file("OpenDirectory", path, answer);
    }

    fn trash_path(&self, path: PathBuf, answer: Resolver<LaunchResult>) {
        let file = Self::open(&path, Entry::Itself).and_then(|file| {
            self.portal.start().map_err(launch_error)?;
            Ok(zbus::OwnedFd::from(std::os::fd::OwnedFd::from(file)))
        });
        let file = match file {
            Ok(file) => file,
            Err(error) => {
                let _ = answer.resolve(Err(error));
                return;
            }
        };
        self.portal.spawn(move |bus| async move {
            let result = async {
                let connection = bus.connection().await?;
                let reply = connection
                    .call_method(
                        Some(DESTINATION),
                        PATH,
                        Some("org.freedesktop.portal.Trash"),
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

/// Which file a path names for the portal.
#[derive(Clone, Copy)]
enum Entry {
    /// What a symbolic link points to: opening or revealing a link shows
    /// its target.
    FollowLinks,
    /// The directory entry itself; a symbolic link is refused.
    Itself,
}
