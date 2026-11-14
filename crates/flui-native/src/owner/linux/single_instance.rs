//! One instance of an application on a Linux session (the activation spec,
//! design §5): the first process of an application owns its app id as a
//! name on the session bus and serves `org.freedesktop.Application` there, as
//! GApplication does; a second, finding the name owned, hands the first what
//! its own launch opens (`Open`, with the activation token its launcher gave
//! it) and starts no platform.
//!
//! The rules (the bus name and object path an app id gives, what a launch
//! forwards, what a forwarded call opens) are pure and checked on every host;
//! the bus itself is a Linux host's, on its IO runtime. Only the session bus
//! is asked: it is the user's own, so a call on it comes from the user's
//! session, and what it carries is read as any launch's arguments are.

use flui_platform::{Launch, OpenTarget, Opened};

use crate::owner::file_uri::{PathConvention, opened_by_urls};

/// The `platform_data` key of a Wayland activation token.
const ACTIVATION_TOKEN: &str = "activation-token";
/// The `platform_data` key of an X11 startup notification id.
const STARTUP_ID: &str = "desktop-startup-id";

/// The bus name an application of `app_id` owns: the app id itself, when it
/// is one (reverse DNS: two or more elements of ASCII letters, digits, `_`
/// and `-`, none starting with a digit, 255 bytes at most); `None` otherwise.
pub(crate) fn bus_name(app_id: &str) -> Option<&str> {
    let elements = app_id.split('.');
    let well_formed = app_id.len() <= 255
        && app_id.split('.').count() >= 2
        && elements.into_iter().all(|element| {
            let mut chars = element.chars();
            chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, '_' | '-'))
                && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        });
    well_formed.then_some(app_id)
}

/// The object path of the application `bus_name` names, as GApplication
/// derives it: `/` and each element, `-` written `_`.
pub(crate) fn object_path(bus_name: &str) -> String {
    let mut path = String::with_capacity(bus_name.len() + 1);
    for element in bus_name.split('.') {
        path.push('/');
        path.extend(element.chars().map(|c| if c == '-' { '_' } else { c }));
    }
    path
}

/// What a second instance hands the first: the URIs its launch opens and the
/// token its launcher gave it to activate a window with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Forwarded {
    /// Links as they were given, files as absolute `file:` URIs.
    pub(crate) uris: Vec<String>,
    /// The Wayland activation token or X11 startup id, if the launcher gave
    /// one.
    pub(crate) token: Option<String>,
}

impl Forwarded {
    /// The longest URI a forwarded launch carries: a link as long as
    /// [`Opened`] keeps one ([`flui_platform::MAX_OPENED_URL_LEN`], which the
    /// first instance applies again as it opens it), or the `file:` URI of a
    /// path as long as Linux names one (`PATH_MAX`, every byte escaped).
    pub(crate) const MAX_URI_LEN: usize = {
        let file = "file://".len() + 3 * 4096;
        if file > flui_platform::MAX_OPENED_URL_LEN {
            file
        } else {
            flui_platform::MAX_OPENED_URL_LEN
        }
    };

    /// Whether the first instance takes every URI of this launch: one longer
    /// than [`Self::MAX_URI_LEN`] would be dropped there, so a launch that
    /// carries one runs as an instance of its own instead.
    pub(crate) fn fits(&self) -> bool {
        self.uris.iter().all(|uri| uri.len() <= Self::MAX_URI_LEN)
    }

    /// What `launch`, this process's own cold launch, forwards: its links,
    /// then its files as `file:` URIs (they are absolute already: the launch
    /// read them against this process's working directory, which the first
    /// instance does not share), with `token`.
    pub(crate) fn of_launch(launch: Opened, token: Option<String>) -> Self {
        let mut uris = launch.urls;
        uris.extend(launch.files.iter().filter_map(|file| {
            let uri = crate::owner::file_uri::file_uri(&file.path);
            if uri.is_none() {
                tracing::debug!(path = %file.path.display(), "a relative path is not forwarded");
            }
            uri
        }));
        Self { uris, token }
    }

    /// What a forwarded call opens in this instance: its URIs, split into
    /// links and documents, as reaching the running application as a whole.
    pub(crate) fn opened(&self) -> Opened {
        opened_by_urls(
            self.uris.iter().cloned(),
            PathConvention::Unix,
            OpenTarget::Application,
            Launch::Running,
        )
    }
}

/// The token a launch was handed: the Wayland activation token
/// (`XDG_ACTIVATION_TOKEN`), else the X11 startup notification id
/// (`DESKTOP_STARTUP_ID`); an empty value is none.
pub(crate) fn launch_token(
    activation_token: Option<std::ffi::OsString>,
    startup_id: Option<std::ffi::OsString>,
) -> Option<String> {
    [activation_token, startup_id]
        .into_iter()
        .flatten()
        .filter_map(|value| value.into_string().ok())
        .find(|token| !token.is_empty())
}

/// The X server time an X11 startup id names (the startup notification
/// specification's `_TIME<timestamp>` suffix, which GTK reads too): the time
/// of the user action that launched the application, which a window manager
/// that prevents focus stealing compares with the user's last input. `None`
/// for an id without one, a Wayland token among them.
#[cfg(any(x11_host, test))]
pub(crate) fn startup_time(startup_id: &str) -> Option<u32> {
    let at = startup_id.rfind("_TIME")?;
    startup_id[at + "_TIME".len()..].parse::<u32>().ok()
}

/// The token of a call's `platform_data`, by either key GApplication sends.
pub(crate) fn token_of<'a>(
    platform_data: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<String> {
    let mut startup = None;
    for (key, value) in platform_data {
        match key {
            ACTIVATION_TOKEN if !value.is_empty() => return Some(value.to_owned()),
            STARTUP_ID if !value.is_empty() => startup = Some(value.to_owned()),
            _ => {}
        }
    }
    startup
}

/// The `platform_data` a second instance sends with `token`, under both
/// keys, as GApplication does: the first instance reads whichever its
/// display server takes.
pub(crate) fn platform_data(token: Option<&str>) -> Vec<(&'static str, String)> {
    match token {
        Some(token) => vec![
            (ACTIVATION_TOKEN, token.to_owned()),
            (STARTUP_ID, token.to_owned()),
        ],
        None => Vec::new(),
    }
}

#[cfg(any(x11_host, wayland_host))]
pub(crate) use bus::{Claim, FirstInstance, claim};

#[cfg(any(x11_host, wayland_host))]
mod bus {
    use std::collections::{HashMap, VecDeque};
    use std::sync::{Arc, Weak};
    use std::time::Duration;

    use parking_lot::Mutex;
    use tokio::sync::oneshot;
    use zbus::fdo::{RequestNameFlags, RequestNameReply};
    use zbus::{OwnedVariant as OwnedValue, Variant as Value};

    use super::{Forwarded, object_path, platform_data, token_of};
    use crate::executor::NativeExecutor;
    use crate::owner::panic_boundary::contain_owner_callback;
    use crate::owner::signal::OwnerSignal;

    /// The D-Bus interface a first instance serves.
    const INTERFACE: &str = "org.freedesktop.Application";

    /// The most launches that wait for the owner at once; a later one is
    /// refused, and its process runs as an instance of its own.
    pub(super) const MAX_WAITING: usize = 16;
    /// How long a launch waits for the session bus.
    const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
    /// How long a later process waits for the first to take its launch: the
    /// first may still be starting, and takes launches from its loop's
    /// first turn. D-Bus's own default for a call is 25 s.
    const FORWARD_TIMEOUT: Duration = Duration::from_secs(20);
    /// How long the first holds a launch for its owner before it hands it
    /// back: shorter than [`FORWARD_TIMEOUT`], so the caller hears the answer
    /// before it stops waiting, and a launch handed back, which its process
    /// then runs on its own, is never delivered here too.
    const HOLD: Duration = Duration::from_secs(15);
    /// The longest activation token or startup id a call carries.
    const MAX_TOKEN_LEN: usize = 4096;

    /// What a launch found on the bus.
    pub(crate) enum Claim {
        /// This process owns the name: it is the first instance.
        First(FirstInstance),
        /// Another process owns it and took this launch.
        Forwarded,
    }

    /// A launch waiting for the owner, and the answer its caller waits for.
    type Waiting = (Forwarded, oneshot::Sender<()>);

    /// The launches waiting for the owner, and the way to wake it.
    pub(super) struct Inbox {
        /// Each launch with its caller's answer. The owner takes and answers
        /// under this lock, and a caller that stops waiting withdraws under
        /// it, so a launch is either taken and answered or handed back.
        queue: Mutex<VecDeque<Waiting>>,
        signal: Mutex<Option<Weak<OwnerSignal>>>,
        /// How long a launch waits for the owner before it is handed back.
        hold: Duration,
        /// The runtime whose timer measures the hold: the bus library runs
        /// the interface's calls on an executor of its own, which has none.
        timers: tokio::runtime::Handle,
    }

    impl Inbox {
        /// An inbox that hands a launch back after `hold`, measured on
        /// `timers`.
        pub(super) fn new(hold: Duration, timers: tokio::runtime::Handle) -> Self {
            Self {
                queue: Mutex::default(),
                signal: Mutex::default(),
                hold,
                timers,
            }
        }

        /// Queue `forwarded` and wake the owner, once it is attached: the
        /// receiver resolves once the owner takes it, and fails if the owner
        /// ends first.
        ///
        /// # Errors
        ///
        /// `LimitsExceeded` past [`MAX_WAITING`], `Failed` once the owner's
        /// loop has ended: the launching process hears the refusal and runs
        /// as an instance of its own, so the launch is not lost.
        pub(super) fn post(
            &self,
            forwarded: Forwarded,
        ) -> zbus::fdo::Result<oneshot::Receiver<()>> {
            let (taken, answer) = oneshot::channel();
            let refused = {
                let mut queue = self.queue.lock();
                if queue.len() >= MAX_WAITING {
                    Some(forwarded)
                } else {
                    queue.push_back((forwarded, taken));
                    None
                }
            };
            // Logged with the queue released: a subscriber is user code.
            if let Some(refused) = refused {
                tracing::warn!(
                    uris = refused.uris.len(),
                    "the owner is not taking forwarded launches; one is refused"
                );
                return Err(zbus::fdo::Error::LimitsExceeded(
                    "the running instance is not taking launches".to_owned(),
                ));
            }
            if self.wake() {
                return Ok(answer);
            }
            // The owner's loop ended: what waits is never taken, and each
            // waiting caller hears it as its sender drops.
            let dropped = std::mem::take(&mut *self.queue.lock());
            tracing::warn!(
                launches = dropped.len(),
                "the running instance is ending; its waiting launches are refused"
            );
            drop(dropped);
            Err(zbus::fdo::Error::Failed(
                "the running instance is ending".to_owned(),
            ))
        }

        /// The waiting launches, oldest first, each caller told its launch
        /// was taken. A launch whose caller stopped waiting (it disconnected
        /// and, if it still runs, runs on its own) is not taken.
        pub(super) fn take(&self) -> Vec<Forwarded> {
            let mut queue = self.queue.lock();
            // Answered under the lock a withdrawing caller takes: a oneshot's
            // send wakes only the caller's task, no application code.
            queue
                .drain(..)
                .filter_map(|(forwarded, taken)| taken.send(()).is_ok().then_some(forwarded))
                .collect()
        }

        /// Wait until the owner takes the launch [`post`](Self::post) queued,
        /// at most the inbox's hold; past it the launch is withdrawn and
        /// refused, so its process runs on its own and the owner never takes
        /// it.
        ///
        /// # Errors
        ///
        /// `post`'s refusal, `Failed` when the owner ended before taking the
        /// launch or held it past the hold.
        pub(super) async fn answer(
            &self,
            posted: zbus::fdo::Result<oneshot::Receiver<()>>,
        ) -> zbus::fdo::Result<()> {
            let mut answer = posted?;
            let ended = || {
                zbus::fdo::Error::Failed(
                    "the running instance ended before it took the launch".to_owned(),
                )
            };
            // The timer is made on its runtime, not on the bus library's executor.
            let hold = self.hold;
            let mut held = self
                .timers
                .spawn(async move { tokio::time::sleep(hold).await });
            let taken = tokio::select! {
                biased;
                taken = &mut answer => Some(taken),
                _ = &mut held => None,
            };
            held.abort();
            if let Some(taken) = taken {
                return taken.map_err(|_| ended());
            }
            let taken = {
                let mut queue = self.queue.lock();
                let taken = answer.try_recv();
                // Closing the answer withdraws the launch: `take` skips it,
                // and it leaves the queue now.
                drop(answer);
                queue.retain(|(_, waiting)| !waiting.is_closed());
                taken
            };
            match taken {
                Ok(()) => Ok(()),
                Err(oneshot::error::TryRecvError::Closed) => Err(ended()),
                Err(oneshot::error::TryRecvError::Empty) => Err(zbus::fdo::Error::Failed(
                    "the running instance did not take the launch in time".to_owned(),
                )),
            }
        }

        /// Wake the owner, once it is attached: `false` when its loop no
        /// longer takes work.
        fn wake(&self) -> bool {
            let signal = self.signal.lock().as_ref().and_then(Weak::upgrade);
            // The upgraded handle can be the last owner of the signal, and
            // releasing it drops the owner-turn callback, which is
            // application code.
            let mut taking = true;
            contain_owner_callback(|| {
                if let Some(signal) = signal {
                    taking = signal.wake().is_ok();
                }
            });
            taking
        }
    }

    /// `org.freedesktop.Application` as a first instance serves it.
    struct Application(Arc<Inbox>);

    #[zbus::interface(name = "org.freedesktop.Application")]
    impl Application {
        /// The application was launched again with nothing to open: its
        /// window comes forward. Answered once the owner takes the launch.
        async fn activate(
            &self,
            platform_data: HashMap<String, OwnedValue>,
        ) -> zbus::fdo::Result<()> {
            self.0
                .answer(self.0.post(Forwarded {
                    uris: Vec::new(),
                    token: token(&platform_data),
                }))
                .await
        }

        /// The application was launched again with `uris`. Answered once
        /// the owner takes the launch.
        async fn open(
            &self,
            uris: Vec<String>,
            platform_data: HashMap<String, OwnedValue>,
        ) -> zbus::fdo::Result<()> {
            self.0
                .answer(
                    self.0.post(Forwarded {
                        uris: uris
                            .into_iter()
                            .take(flui_platform::MAX_OPENED_ITEMS)
                            .filter(|uri| uri.len() <= Forwarded::MAX_URI_LEN)
                            .collect(),
                        token: token(&platform_data),
                    }),
                )
                .await
        }

        /// An action of the application's: FLUI declares none, so the call
        /// only brings its window forward.
        async fn activate_action(
            &self,
            action: String,
            parameter: Vec<OwnedValue>,
            platform_data: HashMap<String, OwnedValue>,
        ) -> zbus::fdo::Result<()> {
            // The action and its parameter name nothing FLUI declares.
            drop((action, parameter));
            self.activate(platform_data).await
        }
    }

    /// The token in `platform_data`, whose values are strings.
    fn token(platform_data: &HashMap<String, OwnedValue>) -> Option<String> {
        let strings: Vec<(&str, &str)> = platform_data
            .iter()
            .filter_map(|(key, value)| match &**value {
                // A token is short; a longer value is no token.
                Value::Str(text) if text.len() <= MAX_TOKEN_LEN => {
                    Some((key.as_str(), text.as_str()))
                }
                _ => None,
            })
            .collect();
        token_of(strings)
    }

    /// The first instance: it owns the name while it lives.
    pub(crate) struct FirstInstance {
        _connection: zbus::Connection,
        inbox: Arc<Inbox>,
    }

    impl std::fmt::Debug for FirstInstance {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("FirstInstance").finish_non_exhaustive()
        }
    }

    impl Drop for FirstInstance {
        /// The loop ended, or never started: each launch it never took is
        /// refused to its caller, which runs as an instance of its own, and
        /// the name goes with the connection.
        fn drop(&mut self) {
            let left = std::mem::take(&mut *self.inbox.queue.lock());
            if !left.is_empty() {
                tracing::warn!(
                    launches = left.len(),
                    "the instance ends with forwarded launches it never took; they are refused"
                );
            }
            drop(left);
        }
    }

    impl FirstInstance {
        /// Wake `signal` for each launch from now on, and for any that came
        /// before.
        pub(crate) fn attach(&self, signal: &Arc<OwnerSignal>) {
            *self.inbox.signal.lock() = Some(Arc::downgrade(signal));
            if !self.inbox.queue.lock().is_empty() {
                self.inbox.wake();
            }
        }

        /// The launches forwarded since the last call, oldest first; each
        /// caller is answered that its launch was taken.
        pub(crate) fn take(&self) -> Vec<Forwarded> {
            self.inbox.take()
        }

        /// How many launches wait for the owner.
        #[cfg(test)]
        pub(crate) fn waiting(&self) -> usize {
            self.inbox.queue.lock().len()
        }
    }

    /// Claim `bus_name` on the session bus (`address`, else the session's
    /// own) for this process, or hand the instance that owns it `launch`.
    ///
    /// # Errors
    ///
    /// The bus error, when there is no session bus or the call fails: the
    /// caller then starts as an instance of its own, so the launch is not
    /// lost.
    pub(crate) fn claim(
        executor: &NativeExecutor,
        address: Option<&str>,
        bus_name: &str,
        launch: Forwarded,
    ) -> zbus::Result<Claim> {
        let runtime = executor.handle();
        let claimed = async {
            tokio::time::timeout(
                CONNECT_TIMEOUT + FORWARD_TIMEOUT,
                claim_on(address, bus_name, launch),
            )
            .await
            .map_err(|_| zbus::Error::Failure("the session bus did not answer in time".into()))?
        };
        // The caller may itself be inside the runtime's asynchronous context
        // (a `#[tokio::main]` entry that lends its runtime), where
        // `Handle::block_on` panics: the claim waits on a thread of its own.
        std::thread::scope(|scope| {
            let waiting = std::thread::Builder::new()
                .name("flui-single-instance".into())
                .spawn_scoped(scope, || runtime.block_on(claimed))
                .map_err(|error| {
                    zbus::Error::Failure(format!("no thread to claim the bus name on: {error}"))
                })?;
            waiting
                .join()
                .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
        })
    }

    async fn claim_on(
        address: Option<&str>,
        bus_name: &str,
        launch: Forwarded,
    ) -> zbus::Result<Claim> {
        let builder = match address {
            Some(address) => zbus::connection::Builder::address(address),
            None => zbus::connection::Builder::session(),
        };
        let inbox = Arc::new(Inbox::new(HOLD, tokio::runtime::Handle::current()));
        let path = object_path(bus_name);
        let connection = builder
            .serve_at(path.as_str(), Application(Arc::clone(&inbox)))
            .build()
            .await?;
        let reply = connection
            .request_name_with_flags(bus_name, RequestNameFlags::DO_NOT_QUEUE)
            .await;
        match reply {
            Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => {
                Ok(Claim::First(FirstInstance {
                    _connection: connection,
                    inbox,
                }))
            }
            Ok(RequestNameReply::Exists | RequestNameReply::InQueue)
            | Err(zbus::Error::NameTaken) => {
                forward(&connection, bus_name, &path, launch).await?;
                Ok(Claim::Forwarded)
            }
            Err(error) => Err(error),
        }
    }

    /// Hand `launch` to the instance that owns `bus_name`: `Open` with its
    /// URIs, or `Activate` when it opens nothing.
    async fn forward(
        connection: &zbus::Connection,
        bus_name: &str,
        path: &str,
        launch: Forwarded,
    ) -> zbus::Result<()> {
        let data: HashMap<&str, Value<'_>> = platform_data(launch.token.as_deref())
            .into_iter()
            .map(|(key, value)| (key, Value::from(value)))
            .collect();
        if launch.uris.is_empty() {
            connection
                .call_method(Some(bus_name), path, Some(INTERFACE), "Activate", &(data,))
                .await?;
        } else {
            connection
                .call_method(
                    Some(bus_name),
                    path,
                    Some(INTERFACE),
                    "Open",
                    &(launch.uris, data),
                )
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use flui_platform::{Launch, OpenTarget, Opened, OpenedFile};

    use super::{
        Forwarded, bus_name, launch_token, object_path, platform_data, startup_time, token_of,
    };

    /// An app id in reverse DNS is the application's bus name, whose object
    /// path GApplication derives; anything else names none, and the
    /// application then runs as many instances as it is launched.
    #[test]
    fn an_app_id_names_the_bus_name_and_object_path() {
        assert_eq!(bus_name("com.example.Notes"), Some("com.example.Notes"));
        assert_eq!(object_path("com.example.Notes"), "/com/example/Notes");
        assert_eq!(object_path("org.my-app.Notes_2"), "/org/my_app/Notes_2");
        for refused in [
            "Notes",
            "com..example",
            "com.example.",
            "com.1example",
            "com.exam ple",
            "com.example.Nötes",
            "",
        ] {
            assert_eq!(bus_name(refused), None, "{refused:?}");
        }
        let long = format!("com.{}", "a".repeat(252));
        assert_eq!(bus_name(&long), None, "over 255 bytes");
    }

    /// A second launch forwards its links as given and its files as absolute
    /// `file:` URIs, with its launcher's token; the first instance opens them
    /// as running, for the application as a whole, by the same split as any
    /// launch: links stay links, files come back as their paths.
    #[test]
    fn a_second_launch_forwards_what_it_opens() {
        let launch = Opened::new(
            ["notes:///note/3".to_owned()],
            [OpenedFile::new("/home/ana/Notes/a b.note")],
            OpenTarget::Application,
            Launch::Cold,
        );
        let forwarded = Forwarded::of_launch(launch, Some("token-1".to_owned()));
        assert_eq!(
            forwarded.uris,
            ["notes:///note/3", "file:///home/ana/Notes/a%20b.note"]
        );
        let opened = forwarded.opened();
        assert_eq!(opened.urls, ["notes:///note/3"]);
        assert_eq!(
            opened.files,
            [OpenedFile::new(PathBuf::from("/home/ana/Notes/a b.note"))]
        );
        assert_eq!(opened.target, OpenTarget::Application);
        assert_eq!(opened.launch, Launch::Running);

        // A launch with nothing to open still brings the window forward.
        let bare = Forwarded::of_launch(
            Opened::new([], [], OpenTarget::Application, Launch::Cold),
            None,
        );
        assert!(bare.uris.is_empty() && bare.opened().is_empty());

        // A path as long as Linux names one still travels; a launch that
        // carries a longer URI runs on its own instead of losing it.
        let long_path = format!("/{}", "ü/".repeat(1365));
        let long = Forwarded::of_launch(
            Opened::new(
                [],
                [OpenedFile::new(long_path)],
                OpenTarget::Application,
                Launch::Cold,
            ),
            None,
        );
        assert!(long.uris[0].len() > flui_platform::MAX_OPENED_URL_LEN);
        assert!(long.fits(), "a long path's URI is within the bound");
        let too_long = Forwarded {
            uris: vec!["x".repeat(Forwarded::MAX_URI_LEN + 1)],
            token: None,
        };
        assert!(!too_long.fits());
    }

    /// The token a launcher gave travels under both of GApplication's keys
    /// and is read back from either, the Wayland token first; an empty value
    /// is none.
    #[test]
    fn the_launchers_token_travels_with_the_launch() {
        assert_eq!(
            launch_token(Some("wayland-1".into()), Some("x11-1".into())),
            Some("wayland-1".to_owned())
        );
        assert_eq!(
            launch_token(Some("".into()), Some("x11-1".into())),
            Some("x11-1".to_owned())
        );
        assert_eq!(launch_token(None, None), None);

        let data = platform_data(Some("token-2"));
        assert_eq!(
            token_of(data.iter().map(|(key, value)| (*key, value.as_str()))),
            Some("token-2".to_owned())
        );
        assert_eq!(
            token_of([("desktop-startup-id", "x11-2")]),
            Some("x11-2".to_owned())
        );
        assert_eq!(
            token_of([
                ("desktop-startup-id", "x11-2"),
                ("activation-token", "wl-2")
            ]),
            Some("wl-2".to_owned())
        );
        assert_eq!(token_of([("activation-token", "")]), None);
        assert_eq!(platform_data(None), []);

        // An X11 startup id names the time of the launching input.
        assert_eq!(
            startup_time("gnome-shell/notes/1234-0-host_TIME98765"),
            Some(98765)
        );
        assert_eq!(startup_time("a_TIME1_TIME42"), Some(42), "the last suffix");
        assert_eq!(startup_time("wayland-token-abc"), None);
        assert_eq!(startup_time("id_TIMEsoon"), None);
    }
}

/// The claim against a `dbus-daemon` of the test's own (skipped when none is
/// installed): two launches of one application, as two processes make them.
#[cfg(all(test, any(x11_host, wayland_host)))]
mod bus_tests {
    use std::io::{BufRead as _, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use super::{Claim, FirstInstance, Forwarded, claim};
    use crate::executor::NativeExecutor;
    use crate::owner::signal::OwnerSignal;

    /// A private session bus, stopped when dropped.
    struct PrivateBus {
        daemon: Child,
        address: String,
    }

    impl PrivateBus {
        fn start() -> Option<Self> {
            let mut daemon = Command::new("dbus-daemon")
                .args(["--session", "--nofork", "--print-address"])
                .stdout(Stdio::piped())
                .spawn()
                .ok()?;
            let mut address = String::new();
            BufReader::new(daemon.stdout.take()?)
                .read_line(&mut address)
                .ok()?;
            let address = address.trim().to_owned();
            (!address.is_empty()).then_some(Self { daemon, address })
        }
    }

    impl Drop for PrivateBus {
        fn drop(&mut self) {
            let _ = self.daemon.kill();
            let _ = self.daemon.wait();
        }
    }

    const NAME: &str = "dev.flui.SingleInstanceTest";

    fn launch(uris: &[&str], token: Option<&str>) -> Forwarded {
        Forwarded {
            uris: uris.iter().map(|uri| (*uri).to_owned()).collect(),
            token: token.map(str::to_owned),
        }
    }

    /// Claim with `launch` from another thread, as a later process does,
    /// while this thread, the owner, takes what arrives: the claim's answer
    /// and what the owner took.
    fn forward_while_taking(
        executor: &NativeExecutor,
        address: &str,
        first: &FirstInstance,
        launch: Forwarded,
    ) -> (zbus::Result<Claim>, Vec<Forwarded>) {
        std::thread::scope(|scope| {
            let claiming = scope.spawn(|| claim(executor, Some(address), NAME, launch));
            let mut taken = Vec::new();
            while !claiming.is_finished() {
                taken.extend(first.take());
                std::thread::sleep(Duration::from_millis(2));
            }
            taken.extend(first.take());
            (claiming.join().expect("the claim does not panic"), taken)
        })
    }

    /// The first launch owns the application's name; a second hands it what
    /// it opens with its launcher's token (`Open`), and one with nothing to
    /// open brings it forward (`Activate`), each waking the owner and each
    /// answered once the owner took it. A launch the owner never takes is
    /// refused when the owner ends, so its process runs on its own; then the
    /// name is free and the next launch is first.
    #[test]
    fn a_second_launch_hands_the_first_what_it_opens() {
        let Some(bus) = PrivateBus::start() else {
            eprintln!("skipped: no dbus-daemon");
            return;
        };
        let executor = NativeExecutor::own();
        let address = bus.address.as_str();
        let Claim::First(first) =
            claim(&executor, Some(address), NAME, launch(&[], None)).expect("the bus answers")
        else {
            panic!("the first launch owns the name");
        };
        let wakes = Arc::new(AtomicUsize::new(0));
        let signal = {
            let wakes = Arc::clone(&wakes);
            OwnerSignal::new(Arc::new(move || {
                wakes.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }))
        };
        first.attach(&signal);
        signal.start().expect("the owner starts its turns");

        let second = launch(
            &["notes:///note/3", "file:///home/ana/a%20b.note"],
            Some("token-1"),
        );
        let (claimed, taken) = forward_while_taking(&executor, address, &first, second.clone());
        assert!(
            matches!(claimed, Ok(Claim::Forwarded)),
            "a second launch forwards"
        );
        assert_eq!(taken, [second]);
        assert!(wakes.load(Ordering::SeqCst) >= 1, "the owner was woken");

        let bare = launch(&[], Some("token-2"));
        let (claimed, taken) = forward_while_taking(&executor, address, &first, bare.clone());
        assert!(matches!(claimed, Ok(Claim::Forwarded)));
        assert_eq!(taken, [bare]);

        // The owner takes nothing more and ends while a launch waits.
        std::thread::scope(|scope| {
            let claiming = scope.spawn(|| {
                claim(
                    &executor,
                    Some(address),
                    NAME,
                    launch(&["notes:///x"], None),
                )
            });
            let deadline = Instant::now() + Duration::from_secs(10);
            while first.waiting() == 0 {
                assert!(Instant::now() < deadline, "the launch reaches the owner");
                std::thread::sleep(Duration::from_millis(2));
            }
            drop(first);
            let refused = claiming.join().expect("the claim does not panic");
            assert!(refused.is_err(), "a launch the owner never took is refused");
        });

        // The bus frees the name once the first's connection closes.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match claim(&executor, Some(address), NAME, launch(&[], None)) {
                Ok(Claim::First(_)) => break,
                // Still owned, or owned by a connection that is closing.
                Ok(Claim::Forwarded) | Err(_) => {
                    assert!(Instant::now() < deadline, "the name stays owned");
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }

    /// An application whose entry runs inside the runtime it lends the host
    /// (`#[tokio::main]`) claims its name from that runtime's asynchronous
    /// context, where `Handle::block_on` panics: the claim waits on a thread
    /// of its own.
    #[test]
    fn a_claim_made_inside_the_lent_runtime_answers() {
        let Some(bus) = PrivateBus::start() else {
            eprintln!("skipped: no dbus-daemon");
            return;
        };
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("a runtime");
        let address = bus.address.as_str();
        let claimed = runtime.block_on(async {
            let executor = NativeExecutor::from_tokio(tokio::runtime::Handle::current());
            claim(&executor, Some(address), NAME, launch(&[], None))
        });
        assert!(
            matches!(claimed, Ok(Claim::First(_))),
            "the first launch owns the name"
        );
    }

    /// A runtime whose timer measures an inbox's hold.
    fn timers() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("a runtime")
    }

    /// At most sixteen launches wait for an owner that takes none; the next
    /// is refused, and the ones taken are answered.
    #[test]
    fn launches_waiting_for_the_owner_are_bounded() {
        let runtime = timers();
        let inbox = super::bus::Inbox::new(Duration::from_secs(15), runtime.handle().clone());
        let mut answers: Vec<_> = (0..super::bus::MAX_WAITING)
            .map(|_| inbox.post(launch(&[], None)).expect("room to wait"))
            .collect();
        assert!(inbox.post(launch(&[], None)).is_err(), "past the bound");
        assert_eq!(inbox.take().len(), super::bus::MAX_WAITING);
        for answer in &mut answers {
            assert_eq!(
                answer.try_recv(),
                Ok(()),
                "each caller hears its launch was taken"
            );
        }
        assert!(inbox.post(launch(&[], None)).is_ok(), "room again");
    }

    /// A launch the owner does not take within the hold is handed back: its
    /// caller hears a refusal, runs on its own, and the owner never takes
    /// it too. One taken in time is answered.
    #[test]
    fn a_launch_held_past_the_hold_is_handed_back_once() {
        let runtime = timers();
        let inbox = super::bus::Inbox::new(Duration::from_millis(20), runtime.handle().clone());
        let handed_back = runtime.block_on(inbox.answer(inbox.post(launch(&["flui:a"], None))));
        assert!(handed_back.is_err(), "the caller hears the refusal");
        assert!(
            inbox.take().is_empty(),
            "a launch handed back is not delivered as well"
        );

        let posted = inbox.post(launch(&["flui:b"], None));
        assert_eq!(inbox.take().len(), 1);
        assert_eq!(runtime.block_on(inbox.answer(posted)), Ok(()));
    }

    /// The bus a second process claims on, which the test hands the copy of
    /// itself it starts as that process.
    const SECOND_BUS: &str = "FLUI_SINGLE_INSTANCE_TEST_BUS";
    /// The arguments the second process is launched with, one per line.
    const SECOND_ARGUMENTS: &str = "FLUI_SINGLE_INSTANCE_TEST_ARGUMENTS";
    const SECOND_PROCESS: &str = "owner::linux::single_instance::bus_tests::a_second_process_forwards_its_arguments_and_ends";

    /// A second process of the application hands the first its launch
    /// arguments (a link, and a file it names by a relative path, sent as an
    /// absolute `file:` URI) with its launcher's token and ends with code 0
    /// once the first took them; the first hears them as a running launch.
    #[test]
    fn a_second_process_forwards_its_arguments_and_ends() {
        if let Some(address) = std::env::var_os(SECOND_BUS) {
            let address = address
                .into_string()
                .expect("the test's bus address is text");
            let arguments = std::env::var(SECOND_ARGUMENTS).unwrap_or_default();
            let launch = Forwarded::of_launch(
                crate::owner::launch_arguments::launched_by(
                    arguments.lines().map(std::ffi::OsString::from).collect(),
                ),
                super::launch_token(Some("token-3".into()), None),
            );
            let claimed = claim(&NativeExecutor::own(), Some(&address), NAME, launch)
                .expect("the bus answers");
            assert!(
                matches!(claimed, Claim::Forwarded),
                "the first owns the name"
            );
            return;
        }
        let Some(bus) = PrivateBus::start() else {
            eprintln!("skipped: no dbus-daemon");
            return;
        };
        let executor = NativeExecutor::own();
        let Claim::First(first) =
            claim(&executor, Some(&bus.address), NAME, launch(&[], None)).expect("the bus answers")
        else {
            panic!("the first launch owns the name");
        };
        let dir = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(dir.path().join("a b.note"), b"note").expect("the note is written");

        let mut second = Command::new(std::env::current_exe().expect("the test binary"))
            .args(["--exact", SECOND_PROCESS])
            .stdout(Stdio::null())
            .env(SECOND_BUS, &bus.address)
            .env(SECOND_ARGUMENTS, "notes:///note/3\na b.note\n--verbose")
            .current_dir(dir.path())
            .spawn()
            .expect("the second process starts");
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut taken = Vec::new();
        let status = loop {
            taken.extend(first.take());
            if let Some(status) = second.try_wait().expect("the second process is waited on") {
                break status;
            }
            assert!(Instant::now() < deadline, "the second process ends");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(
            status.success(),
            "the second process ends with code 0: {status}"
        );

        let [forwarded] = taken.as_slice() else {
            panic!("one forwarded launch, not {taken:?}");
        };
        assert_eq!(forwarded.token.as_deref(), Some("token-3"));
        let opened = forwarded.opened();
        assert_eq!(opened.urls, ["notes:///note/3"]);
        let note = std::fs::canonicalize(dir.path().join("a b.note")).expect("the note exists");
        let files: Vec<_> = opened
            .files
            .iter()
            .map(|file| std::fs::canonicalize(&file.path).expect("the forwarded file exists"))
            .collect();
        assert_eq!(files, [note]);
        assert_eq!(opened.launch, flui_platform::Launch::Running);
        assert_eq!(opened.target, flui_platform::OpenTarget::Application);
    }
}
