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

/// The D-Bus interface a first instance serves.
pub(crate) const INTERFACE: &str = "org.freedesktop.Application";

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
    use zbus::fdo::{RequestNameFlags, RequestNameReply};
    use zbus::{OwnedVariant as OwnedValue, Variant as Value};

    use super::{Forwarded, INTERFACE, object_path, platform_data, token_of};
    use crate::executor::NativeExecutor;
    use crate::owner::panic_boundary::contain_owner_callback;
    use crate::owner::signal::OwnerSignal;

    /// The most launches that wait for the owner at once; a later one is
    /// dropped with a warning, as the application handler's own queue drops
    /// one.
    pub(super) const MAX_WAITING: usize = 16;
    /// How long a launch waits for the session bus.
    const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
    /// How long a second instance waits for the first to take its launch.
    const FORWARD_TIMEOUT: Duration = Duration::from_secs(5);
    /// The longest activation token or startup id a call carries.
    const MAX_TOKEN_LEN: usize = 4096;

    /// What a launch found on the bus.
    pub(crate) enum Claim {
        /// This process owns the name: it is the first instance.
        First(FirstInstance),
        /// Another process owns it and took this launch.
        Forwarded,
    }

    /// The launches waiting for the owner, and the way to wake it.
    #[derive(Default)]
    struct Inbox {
        queue: Mutex<VecDeque<Forwarded>>,
        signal: Mutex<Option<Weak<OwnerSignal>>>,
    }

    impl Inbox {
        /// Queue `forwarded` and wake the owner, once it is attached.
        ///
        /// # Errors
        ///
        /// `LimitsExceeded` past [`MAX_WAITING`]: the launching process hears
        /// the refusal and runs as an instance of its own, so the launch is
        /// not lost.
        fn post(&self, forwarded: Forwarded) -> zbus::fdo::Result<()> {
            let refused = {
                let mut queue = self.queue.lock();
                if queue.len() >= MAX_WAITING {
                    Some(forwarded)
                } else {
                    queue.push_back(forwarded);
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
                return Ok(());
            }
            // The owner's loop ended: what waits is never taken.
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
        /// window comes forward.
        fn activate(&self, platform_data: HashMap<String, OwnedValue>) -> zbus::fdo::Result<()> {
            self.0.post(Forwarded {
                uris: Vec::new(),
                token: token(&platform_data),
            })
        }

        /// The application was launched again with `uris`.
        fn open(
            &self,
            uris: Vec<String>,
            hint: String,
            platform_data: HashMap<String, OwnedValue>,
        ) -> zbus::fdo::Result<()> {
            // What `uris` are opened as is decided here, whatever the hint.
            drop(hint);
            self.0.post(Forwarded {
                uris: uris
                    .into_iter()
                    .take(flui_platform::MAX_OPENED_ITEMS)
                    .filter(|uri| uri.len() <= flui_platform::MAX_OPENED_URL_LEN)
                    .collect(),
                token: token(&platform_data),
            })
        }

        /// An action of the application's: FLUI declares none, so the call
        /// only brings its window forward.
        fn activate_action(
            &self,
            action: String,
            parameter: Vec<OwnedValue>,
            platform_data: HashMap<String, OwnedValue>,
        ) -> zbus::fdo::Result<()> {
            // The action and its parameter name nothing FLUI declares.
            drop((action, parameter));
            self.activate(platform_data)
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
        /// The loop ended: launches it never took are named in the log, and
        /// the name goes with the connection.
        fn drop(&mut self) {
            let left = self.inbox.queue.lock().len();
            if left > 0 {
                tracing::warn!(
                    launches = left,
                    "the instance ends with forwarded launches it never took"
                );
            }
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

        /// The launches forwarded since the last call, oldest first.
        pub(crate) fn take(&self) -> Vec<Forwarded> {
            self.inbox.queue.lock().drain(..).collect()
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
        runtime.block_on(async {
            tokio::time::timeout(
                CONNECT_TIMEOUT + FORWARD_TIMEOUT,
                claim_on(address, bus_name, launch),
            )
            .await
            .map_err(|_| zbus::Error::Failure("the session bus did not answer in time".into()))?
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
        let inbox = Arc::new(Inbox::default());
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
                    &(launch.uris, "", data),
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

    use super::{Claim, Forwarded, claim};
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

    /// The first launch owns the application's name; a second hands it what
    /// it opens with its launcher's token (`Open`), and one with nothing to
    /// open brings it forward (`Activate`), each waking the owner and each
    /// told it was taken; once the first ends, the next launch is first.
    #[test]
    fn a_second_launch_hands_the_first_what_it_opens() {
        let Some(bus) = PrivateBus::start() else {
            eprintln!("skipped: no dbus-daemon");
            return;
        };
        let executor = NativeExecutor::own();
        let address = Some(bus.address.as_str());
        let Claim::First(first) =
            claim(&executor, address, NAME, launch(&[], None)).expect("the bus answers")
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
        let claimed = claim(&executor, address, NAME, second.clone()).expect("the bus answers");
        assert!(
            matches!(claimed, Claim::Forwarded),
            "a second launch forwards"
        );
        // The call returns once the first served it.
        assert_eq!(first.take(), [second]);
        assert!(wakes.load(Ordering::SeqCst) >= 1, "the owner was woken");

        let bare = launch(&[], Some("token-2"));
        let claimed = claim(&executor, address, NAME, bare.clone()).expect("the bus answers");
        assert!(matches!(claimed, Claim::Forwarded));
        assert_eq!(first.take(), [bare]);

        // An owner that takes nothing refuses the launch past the bound,
        // which then runs as an instance of its own.
        for _ in 0..super::bus::MAX_WAITING {
            let claimed =
                claim(&executor, address, NAME, launch(&[], None)).expect("the bus answers");
            assert!(matches!(claimed, Claim::Forwarded));
        }
        assert!(
            claim(&executor, address, NAME, launch(&[], None)).is_err(),
            "a launch past the bound is refused"
        );
        assert_eq!(first.take().len(), super::bus::MAX_WAITING);

        drop(first);
        // The bus frees the name once the first's connection closes.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match claim(&executor, address, NAME, launch(&[], None)) {
                Ok(Claim::First(_)) => break,
                // Still owned, or owned by a connection that is closing.
                Ok(Claim::Forwarded) | Err(_) => {
                    assert!(Instant::now() < deadline, "the name stays owned");
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }

    /// The bus a second process claims on, which the test hands the copy of
    /// itself it starts as that process.
    const SECOND_BUS: &str = "FLUI_SINGLE_INSTANCE_TEST_BUS";
    /// The arguments the second process is launched with, one per line.
    const SECOND_ARGUMENTS: &str = "FLUI_SINGLE_INSTANCE_TEST_ARGUMENTS";
    const SECOND_PROCESS: &str = "owner::linux::single_instance::bus_tests::a_second_process_forwards_its_arguments_and_ends";

    /// A second process of the application hands the first its launch
    /// arguments (a link, and a file it names by a relative path, sent as an
    /// absolute `file:` URI) with its launcher's token and ends with code 0;
    /// the first hears them as a running launch.
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

        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args(["--exact", SECOND_PROCESS])
            .stdout(Stdio::null())
            .env(SECOND_BUS, &bus.address)
            .env(SECOND_ARGUMENTS, "notes:///note/3\na b.note\n--verbose")
            .current_dir(dir.path())
            .status()
            .expect("the second process starts");
        assert!(
            status.success(),
            "the second process ends with code 0: {status}"
        );

        let taken = first.take();
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
