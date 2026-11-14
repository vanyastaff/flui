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
    use std::collections::HashMap;
    use std::sync::{Arc, Weak};
    use std::time::Duration;

    use futures_util::StreamExt as _;
    use zbus::fdo::{RequestNameFlags, RequestNameReply};
    use zbus::{OwnedVariant as OwnedValue, Variant as Value};

    use super::{Forwarded, object_path, platform_data, token_of};
    use crate::executor::NativeExecutor;
    use crate::owner::panic_boundary::contain_owner_callback;
    use crate::owner::signal::OwnerSignal;
    use crate::owner::single_instance::{Delivery, HOLD, OwnerWake, Posted, Refused};

    /// The launches waiting for the owner.
    type Inbox = crate::owner::single_instance::Inbox<Forwarded>;

    /// The D-Bus interface a first instance serves.
    const INTERFACE: &str = "org.freedesktop.Application";
    /// The interface every D-Bus peer answers.
    const PEER: &str = "org.freedesktop.DBus.Peer";
    /// The most calls read off the bus that wait to be served or are being
    /// answered; the bus connection holds a later one until there is room.
    const MAX_READ: usize = 64;

    /// How long a launch waits for the session bus.
    const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
    /// How long a later process waits for the first to take its launch: the
    /// first may still be starting, and takes launches from its loop's
    /// first turn. Longer than the first's [`HOLD`], so the caller hears the
    /// answer before it stops waiting. D-Bus's own default for a call is
    /// 25 s.
    const FORWARD_TIMEOUT: Duration = Duration::from_secs(20);
    /// The longest activation token or startup id a call carries.
    const MAX_TOKEN_LEN: usize = 4096;

    /// The owner's signal, as the inbox wakes it.
    struct SignalWake(Weak<OwnerSignal>);

    impl OwnerWake for SignalWake {
        fn wake(&self) -> bool {
            let signal = self.0.upgrade();
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

    /// A refusal as D-Bus names it to the caller.
    fn refusal(refused: Refused) -> zbus::fdo::Error {
        let reason = refused.reason().to_owned();
        match refused {
            Refused::Full => zbus::fdo::Error::LimitsExceeded(reason),
            Refused::Ending => zbus::fdo::Error::Failed(reason),
        }
    }

    /// What a launch found on the bus.
    pub(crate) enum Claim {
        /// This process owns the name: it is the first instance.
        First(FirstInstance),
        /// Another process owns it and took this launch.
        Forwarded,
    }

    /// What a call to the first instance asks of it.
    enum Asked {
        /// Answer at once, with no value.
        Nothing,
        /// Answer once the owner takes the launch the call posted.
        Launch(Posted),
    }

    /// Serve `org.freedesktop.Application` at `path` from `calls` until the
    /// task is aborted, with the first instance.
    ///
    /// The calls are read off the connection's stream and answered by hand,
    /// not by the bus library's object server: one interface of three
    /// methods does not pay for the server's introspection, properties and
    /// dispatch in every application's binary.
    ///
    /// Each answer runs as a task of its own, which aborting this one does
    /// not end: a launch the owner took as the instance ends is still
    /// answered as taken, so its process does not open it a second time. At
    /// most [`MAX_READ`] answers are under way; the next call waits on the
    /// bus.
    async fn serve(
        connection: zbus::Connection,
        path: String,
        inbox: Arc<Inbox>,
        mut calls: zbus::MessageStream,
    ) {
        let answering = Arc::new(tokio::sync::Semaphore::new(MAX_READ));
        loop {
            let Ok(room) = Arc::clone(&answering).acquire_owned().await else {
                return;
            };
            // A launch is posted as its call is read, so launches are queued
            // in the order they came; only the wait for the owner runs
            // alongside.
            match calls.next().await {
                Some(Ok(call)) => {
                    let asked = ask(&inbox, &path, &call);
                    let answer = Answer::of(call, asked);
                    let (connection, inbox) = (connection.clone(), Arc::clone(&inbox));
                    tokio::spawn(async move {
                        respond(connection, inbox, answer).await;
                        drop(room);
                    });
                }
                Some(Err(error)) => {
                    tracing::debug!(%error, "an unreadable call to the running instance");
                }
                None => return,
            }
        }
    }

    /// What `call` asks: a launch is posted to `inbox` now. A call to
    /// another object, interface or method, or with arguments of another
    /// signature, is refused as D-Bus names the refusal.
    fn ask(inbox: &Inbox, path: &str, call: &zbus::Message) -> zbus::fdo::Result<Asked> {
        use zbus::fdo::Error;

        let header = call.header();
        let member = header.member().map_or("", |member| member.as_str());
        let interface = header.interface().map(zbus::names::InterfaceName::as_str);
        // Every object answers `Ping`, as D-Bus asks of each peer.
        if interface == Some(PEER) && member == "Ping" {
            return Ok(Asked::Nothing);
        }
        if header.path().map(zbus::ObjectPath::as_str) != Some(path) {
            return Err(Error::UnknownObject(format!(
                "no object at {:?}",
                header.path()
            )));
        }
        if interface.is_some_and(|interface| interface != INTERFACE) {
            return Err(Error::UnknownInterface(format!(
                "{interface:?} is not served here"
            )));
        }
        let invalid = |error: zbus::Error| Error::InvalidArgs(error.to_string());
        let body = call.body();
        let forwarded = match member {
            "Activate" => {
                let (platform_data,) = body
                    .deserialize::<(HashMap<String, OwnedValue>,)>()
                    .map_err(invalid)?;
                Forwarded {
                    uris: Vec::new(),
                    token: token(&platform_data),
                }
            }
            "Open" => {
                let (uris, platform_data) = body
                    .deserialize::<(Vec<String>, HashMap<String, OwnedValue>)>()
                    .map_err(invalid)?;
                Forwarded {
                    uris: uris
                        .into_iter()
                        .take(flui_platform::MAX_OPENED_ITEMS)
                        .filter(|uri| uri.len() <= Forwarded::MAX_URI_LEN)
                        .collect(),
                    token: token(&platform_data),
                }
            }
            // An action of the application's: FLUI declares none, so the
            // call only brings its window forward.
            "ActivateAction" => {
                let (_action, _parameter, platform_data) = body
                    .deserialize::<(String, Vec<OwnedValue>, HashMap<String, OwnedValue>)>()
                    .map_err(invalid)?;
                Forwarded {
                    uris: Vec::new(),
                    token: token(&platform_data),
                }
            }
            _ => {
                return Err(Error::UnknownMethod(format!(
                    "{member:?} is not a method of {INTERFACE}"
                )));
            }
        };
        inbox.post(forwarded).map(Asked::Launch).map_err(refusal)
    }

    /// A call's answer, until it is sent.
    enum Answer {
        /// Sent at once, with the call it answers.
        Now(zbus::Message, zbus::fdo::Result<()>),
        /// Sent once the owner takes the launch or the inbox hands it back.
        /// The answers are addressed when the call is read and the call is
        /// not kept: a launch can wait the whole hold, and its body, however
        /// large, does not wait with it.
        Launch {
            /// The answers, unless the caller asked for none.
            replies: Option<Box<Replies>>,
            taken: Posted,
        },
    }

    /// A launch's two possible answers, addressed to its caller.
    struct Replies {
        taken: zbus::message::Builder<'static>,
        refused: zbus::message::Builder<'static>,
    }

    impl Answer {
        /// The answer to `call`, which `asked` what is in it.
        fn of(call: zbus::Message, asked: zbus::fdo::Result<Asked>) -> Self {
            match asked {
                Ok(Asked::Launch(taken)) => {
                    let header = call.header();
                    let expects_answer = !header
                        .primary()
                        .flags()
                        .contains(zbus::message::Flags::NO_REPLY_EXPECTED);
                    let replies = expects_answer.then(|| {
                        Box::new(Replies {
                            taken: zbus::Message::method_return(&header),
                            refused: zbus::Message::error(
                                &header,
                                "org.freedesktop.DBus.Error.Failed",
                            ),
                        })
                    });
                    Self::Launch { replies, taken }
                }
                Ok(Asked::Nothing) => Self::Now(call, Ok(())),
                Err(refused) => Self::Now(call, Err(refused)),
            }
        }
    }

    /// Send `answer` once it is known: a launch's once the owner took it or
    /// the inbox handed it back. A call that expects no answer gets none.
    async fn respond(connection: zbus::Connection, inbox: Arc<Inbox>, answer: Answer) {
        match answer {
            Answer::Now(call, answered) => {
                let header = call.header();
                if header
                    .primary()
                    .flags()
                    .contains(zbus::message::Flags::NO_REPLY_EXPECTED)
                {
                    return;
                }
                report(match answered {
                    Ok(()) => connection.reply(&header, &()).await,
                    Err(refused) => connection.reply_dbus_error(&header, refused).await,
                });
            }
            Answer::Launch { replies, taken } => {
                let answered = inbox.answer(taken).await;
                let Some(replies) = replies else {
                    return;
                };
                let built = match answered {
                    Ok(()) => replies.taken.build(&()),
                    Err(reason) => replies.refused.build(&(reason,)),
                };
                match built {
                    Ok(message) => report(connection.send(&message).await),
                    Err(error) => {
                        tracing::debug!(%error, "a later launch's answer was not built");
                    }
                }
            }
        }
    }

    /// Log an answer the caller did not get.
    fn report(sent: zbus::Result<()>) {
        if let Err(error) = sent {
            tracing::debug!(%error, "a later launch was not answered: its caller left");
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
        /// The task that serves the calls, which holds the connection: the
        /// name goes when it ends.
        serving: tokio::task::JoinHandle<()>,
        inbox: Arc<Inbox>,
    }

    impl std::fmt::Debug for FirstInstance {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("FirstInstance").finish_non_exhaustive()
        }
    }

    impl Drop for FirstInstance {
        /// The loop ended, or never started: no call is read from now on, each
        /// launch it never took is refused to its caller, which runs as an
        /// instance of its own, one it took is answered as taken, and the name
        /// goes with the connection once those answers are sent.
        fn drop(&mut self) {
            // First, so a subscriber that panics below cannot leave the bus
            // read: a later call finds no instance and its process runs on
            // its own.
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
        /// Wake `signal` for each launch from now on, and for any that came
        /// before.
        pub(crate) fn attach(&self, signal: &Arc<OwnerSignal>) {
            self.inbox
                .attach(Arc::new(SignalWake(Arc::downgrade(signal))) as Arc<dyn OwnerWake>);
        }

        /// The launches forwarded since the last call, oldest first; each
        /// with the answer its caller waits for: taken once the application
        /// holds the launch, refused when it is dropped.
        pub(crate) fn take(&self) -> Vec<(Forwarded, Delivery)> {
            self.inbox.take()
        }

        /// How many launches wait for the owner.
        #[cfg(test)]
        pub(crate) fn waiting(&self) -> usize {
            self.inbox.waiting()
        }
    }

    /// Claim `bus_name` on the session bus (`address`, else the session's
    /// own) for this process, or hand the instance that owns it `launch`.
    ///
    /// # Errors
    ///
    /// The bus error, when there is no session bus, it does not answer in
    /// time or the call fails: the caller then starts as an instance of its
    /// own, so the launch is not lost.
    pub(crate) fn claim(
        executor: &NativeExecutor,
        address: Option<&str>,
        bus_name: &str,
        launch: Forwarded,
    ) -> zbus::Result<Claim> {
        claim_holding(executor, address, bus_name, launch, HOLD)
    }

    /// [`claim`], with a first instance that holds a launch for `hold`.
    pub(super) fn claim_holding(
        executor: &NativeExecutor,
        address: Option<&str>,
        bus_name: &str,
        launch: Forwarded,
        hold: Duration,
    ) -> zbus::Result<Claim> {
        let host = executor.handle();
        // The claim waits on a thread and a runtime of its own, not on the
        // host's: the caller may be inside the host's runtime (a
        // `#[tokio::main]` entry that lends it), where `Handle::block_on`
        // panics, and a current-thread runtime whose one thread is the caller
        // runs no timer while that thread waits here. The first instance
        // then serves on the host's runtime.
        std::thread::scope(|scope| {
            let waiting = std::thread::Builder::new()
                .name("flui-single-instance".into())
                .spawn_scoped(scope, || {
                    tokio::runtime::Builder::new_current_thread()
                        .enable_time()
                        .build()
                        .map_err(|error| {
                            zbus::Error::Failure(format!(
                                "no runtime to claim the bus name on: {error}"
                            ))
                        })?
                        .block_on(claim_on(&host, address, bus_name, launch, hold))
                })
                .map_err(|error| {
                    zbus::Error::Failure(format!("no thread to claim the bus name on: {error}"))
                })?;
            waiting
                .join()
                .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
        })
    }

    /// The error of a bus that did not answer within its time.
    fn late(_: tokio::time::error::Elapsed) -> zbus::Error {
        zbus::Error::Failure("the session bus did not answer in time".into())
    }

    async fn claim_on(
        host: &tokio::runtime::Handle,
        address: Option<&str>,
        bus_name: &str,
        launch: Forwarded,
        hold: Duration,
    ) -> zbus::Result<Claim> {
        let path = object_path(bus_name);
        let (connection, calls, reply) = tokio::time::timeout(CONNECT_TIMEOUT, async {
            let builder = match address {
                Some(address) => zbus::connection::Builder::address(address),
                None => zbus::connection::Builder::session(),
            };
            let connection = builder.build().await?;
            // Listening before the name is owned: a call can come the moment
            // it is.
            let calls = zbus::MessageStream::for_match_rule(
                zbus::MatchRule::builder()
                    .msg_type(zbus::message::Type::MethodCall)
                    .build()?,
                &connection,
                std::num::NonZeroUsize::new(MAX_READ),
            )
            .await?;
            let reply = connection
                .request_name_with_flags(bus_name, RequestNameFlags::DO_NOT_QUEUE)
                .await;
            zbus::Result::Ok((connection, calls, reply))
        })
        .await
        .map_err(late)??;
        match reply {
            Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => {
                let inbox = Arc::new(Inbox::new(hold, host.clone()));
                let serving = host.spawn(serve(connection, path, Arc::clone(&inbox), calls));
                Ok(Claim::First(FirstInstance { serving, inbox }))
            }
            Ok(RequestNameReply::Exists | RequestNameReply::InQueue)
            | Err(zbus::Error::NameTaken) => {
                drop(calls);
                tokio::time::timeout(
                    FORWARD_TIMEOUT,
                    forward(&connection, bus_name, &path, launch),
                )
                .await
                .map_err(late)??;
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

    use super::{Forwarded, launch_token, object_path, platform_data, startup_time, token_of};
    use crate::owner::single_instance::instance_name;

    /// An app id in reverse DNS is the application's bus name, whose object
    /// path GApplication derives (the rules of the name are
    /// `an_app_id_in_reverse_dns_names_the_instance`'s).
    #[test]
    fn an_app_id_names_the_bus_name_and_object_path() {
        assert_eq!(
            instance_name("com.example.Notes")
                .map(object_path)
                .as_deref(),
            Some("/com/example/Notes")
        );
        assert_eq!(object_path("org.my-app.Notes_2"), "/org/my_app/Notes_2");
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
    use crate::owner::single_instance::Delivery;

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

    /// The launches `taken`, each answered as the application's owner
    /// answers one it handed on.
    fn delivered(taken: Vec<(Forwarded, Delivery)>) -> Vec<Forwarded> {
        taken
            .into_iter()
            .map(|(launch, delivery)| {
                delivery.delivered();
                launch
            })
            .collect::<Vec<Forwarded>>()
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
                taken.extend(delivered(first.take()));
                std::thread::sleep(Duration::from_millis(2));
            }
            taken.extend(delivered(first.take()));
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

    /// The first instance answers every call D-Bus can send it: a peer's
    /// `Ping`, and a refusal named as D-Bus names it for a call to another
    /// object, interface or method, or one whose arguments have another
    /// signature, so no caller waits for an answer that never comes.
    #[test]
    fn calls_the_first_instance_does_not_serve_are_refused_by_name() {
        let Some(bus) = PrivateBus::start() else {
            eprintln!("skipped: no dbus-daemon");
            return;
        };
        let executor = NativeExecutor::own();
        let address = bus.address.as_str();
        let Claim::First(_first) =
            claim(&executor, Some(address), NAME, launch(&[], None)).expect("the bus answers")
        else {
            panic!("the first launch owns the name");
        };
        let path = super::object_path(NAME);
        let answers: Vec<Result<(), String>> = executor.handle().block_on(async {
            let caller = zbus::connection::Builder::address(address)
                .build()
                .await
                .expect("a caller connects");
            let calls: [(&str, &str, &str); 5] = [
                (path.as_str(), "org.freedesktop.DBus.Peer", "Ping"),
                ("/elsewhere", "org.freedesktop.Application", "Activate"),
                (path.as_str(), "org.example.Other", "Activate"),
                (path.as_str(), "org.freedesktop.Application", "Quit"),
                (path.as_str(), "org.freedesktop.Application", "Open"),
            ];
            let mut answers = Vec::new();
            for (object, interface, method) in calls {
                let answer = caller
                    .call_method(Some(NAME), object, Some(interface), method, &("one",))
                    .await;
                answers.push(match answer {
                    // `Ping` takes no argument, and a peer ignores the body.
                    Ok(_) => Ok(()),
                    Err(zbus::Error::FDO(refused)) => {
                        Err(zbus::DBusError::name(&*refused).as_str().to_owned())
                    }
                    Err(other) => Err(format!("not a refusal: {other}")),
                });
            }
            answers
        });
        assert_eq!(
            answers,
            [
                Ok(()),
                Err("org.freedesktop.DBus.Error.UnknownObject".to_owned()),
                Err("org.freedesktop.DBus.Error.UnknownInterface".to_owned()),
                Err("org.freedesktop.DBus.Error.UnknownMethod".to_owned()),
                Err("org.freedesktop.DBus.Error.InvalidArgs".to_owned()),
            ]
        );
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

    /// A launch the first instance does not take within its hold is refused
    /// to its caller as D-Bus's `Failed`, with the reason, after the call was
    /// released; the first instance does not take it later.
    #[test]
    fn a_launch_not_taken_in_time_is_refused_to_its_caller() {
        let Some(bus) = PrivateBus::start() else {
            eprintln!("skipped: no dbus-daemon");
            return;
        };
        let executor = NativeExecutor::own();
        let address = bus.address.as_str();
        let Claim::First(first) = super::bus::claim_holding(
            &executor,
            Some(address),
            NAME,
            launch(&[], None),
            Duration::from_millis(50),
        )
        .expect("the bus answers") else {
            panic!("the first launch owns the name");
        };
        let refused = claim(
            &executor,
            Some(address),
            NAME,
            launch(&["notes:///x"], None),
        );
        match refused {
            Err(zbus::Error::FDO(refused)) => {
                assert_eq!(
                    zbus::DBusError::name(&*refused).as_str(),
                    "org.freedesktop.DBus.Error.Failed"
                );
                assert_eq!(
                    zbus::DBusError::description(&*refused),
                    Some("the running instance did not take the launch in time")
                );
            }
            Err(other) => panic!("not a refusal: {other}"),
            Ok(_) => panic!("a launch nobody took is refused"),
        }
        assert!(first.take().is_empty(), "a launch handed back is not taken");
    }

    /// A launch the owner took as the instance ends is answered as taken: its
    /// process ends and does not open the launch a second time. The host's
    /// runtime runs only between the steps here, so the instance ends before
    /// anything of it runs after the owner took the launch.
    #[test]
    fn a_launch_taken_as_the_instance_ends_is_answered_as_taken() {
        let Some(bus) = PrivateBus::start() else {
            eprintln!("skipped: no dbus-daemon");
            return;
        };
        let host = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let executor = NativeExecutor::from_tokio(host.handle().clone());
        let address = bus.address.as_str();
        let Claim::First(first) =
            claim(&executor, Some(address), NAME, launch(&[], None)).expect("the bus answers")
        else {
            panic!("the first launch owns the name");
        };
        let later = launch(&["notes:///x"], None);
        let answered = std::thread::scope(|scope| {
            let forwarding = scope.spawn(|| claim(&executor, Some(address), NAME, later.clone()));
            let deadline = Instant::now() + Duration::from_secs(10);
            host.block_on(async {
                while first.waiting() == 0 {
                    assert!(Instant::now() < deadline, "the launch reaches the owner");
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
            });
            assert_eq!(delivered(first.take()), std::slice::from_ref(&later));
            drop(first);
            host.block_on(async {
                while !forwarding.is_finished() {
                    assert!(
                        Instant::now() < deadline,
                        "the later launch hears its answer"
                    );
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
            });
            forwarding.join().expect("the claim does not panic")
        });
        match answered {
            Ok(Claim::Forwarded) => {}
            Ok(Claim::First(_)) => panic!("the name was owned while the launch waited"),
            Err(error) => panic!("a launch the owner took is answered as taken: {error}"),
        }
    }

    /// A claim made inside a current-thread runtime the application lends
    /// the host, against a bus that takes the connection and never answers,
    /// gives up in time, so the application starts on its own: the claim's
    /// timer runs on a runtime of its own, while the lent runtime's one
    /// thread waits for the claim.
    #[test]
    fn a_claim_on_a_silent_bus_gives_up_inside_a_current_thread_runtime() {
        let dir = tempfile::tempdir().expect("a directory for the socket");
        let socket = dir.path().join("bus");
        // Connections wait in the backlog: connected, never answered.
        let _silent = std::os::unix::net::UnixListener::bind(&socket).expect("a socket");
        let address = format!("unix:path={}", socket.display());
        let (answered, answer) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a runtime");
            let refused = runtime.block_on(async {
                let executor = NativeExecutor::from_tokio(tokio::runtime::Handle::current());
                claim(&executor, Some(&address), NAME, launch(&[], None)).is_err()
            });
            answered
                .send(refused)
                .expect("the test waits for the answer");
        });
        assert_eq!(
            answer.recv_timeout(Duration::from_secs(10)),
            Ok(true),
            "the claim gives up on a silent bus"
        );
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
            taken.extend(delivered(first.take()));
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
