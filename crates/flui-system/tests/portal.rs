//! The Linux providers end to end: a widget's launcher and dialogs reach a
//! stand-in desktop portal served on a private bus, and its answers come
//! back through the `Request` objects as the real portal sends them.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use flui_platform::capability::{Abandoned, Completion};
use flui_platform::launch::{ExternalUri, LaunchError, LaunchResult, LauncherCapability};
use flui_system::dialog::{
    DialogError, FileDialogCapability, FileFilter, OpenRequest, OpenResult, PickedFile, SaveRequest,
};
use flui_system::testing::PortalOnBus;
use flui_testing::HeadlessHost;
use zbus::message::Header;
use zbus::{OwnedFd, OwnedObjectPath, OwnedVariant, Variant};

use crate::{acquire, outcome_slot, turn};

/// A `dbus-daemon` of the test's own.
struct PrivateBus {
    daemon: Child,
    address: String,
}

impl PrivateBus {
    fn start() -> Self {
        Self::spawn(Command::new("dbus-daemon").arg("--session"))
    }

    /// A bus that starts the services `directory` describes on demand, as a
    /// desktop session starts its portal.
    fn with_services(directory: &Path) -> Self {
        let listen = format!("unix:tmpdir={}", std::env::temp_dir().display());
        Self::configured(directory, &listen)
    }

    /// A bus listening at `listen`, with its configuration and services in
    /// `directory`.
    fn configured(directory: &Path, listen: &str) -> Self {
        let config = directory.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
                 \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                 <busconfig><type>session</type><listen>{listen}</listen>\
                 <servicedir>{services}</servicedir>\
                 <policy context=\"default\"><allow send_destination=\"*\"/><allow receive_sender=\"*\"/><allow own=\"*\"/></policy>\
                 </busconfig>\n",
                services = directory.display(),
            ),
        )
        .expect("the bus configuration is written");
        Self::spawn(Command::new("dbus-daemon").arg(format!("--config-file={}", config.display())))
    }

    fn spawn(command: &mut Command) -> Self {
        let mut daemon = command
            .args(["--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon runs: the Linux test host installs it");
        let mut address = String::new();
        BufReader::new(daemon.stdout.take().expect("piped stdout"))
            .read_line(&mut address)
            .expect("dbus-daemon prints its address");
        let address = address.trim().to_owned();
        assert!(!address.is_empty(), "dbus-daemon printed no address");
        Self { daemon, address }
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

/// One call the stand-in portal received.
#[derive(Clone, Debug, PartialEq)]
enum Call {
    OpenUri {
        parent: String,
        uri: String,
    },
    OpenFile {
        parent: String,
        path: PathBuf,
    },
    OpenDirectory {
        parent: String,
        path: PathBuf,
    },
    Trash {
        path: PathBuf,
        /// Whether the descriptor is open for writing, which the portal
        /// requires of a sandboxed application.
        writable: bool,
    },
    Open {
        parent: String,
        title: String,
        options: Vec<String>,
    },
    Save {
        parent: String,
        title: String,
        options: Vec<String>,
    },
}

type Results = HashMap<String, OwnedVariant>;

/// What the stand-in received and how it answers.
#[derive(Default)]
struct Script {
    calls: Mutex<Vec<Call>>,
    /// The next `Request` answers, oldest first; `(0, {})` when none.
    responses: Mutex<VecDeque<(u32, Results)>>,
    /// What `TrashFile` answers.
    trashed: Mutex<u32>,
    /// The options each dialog was asked with, by name.
    dialog_options: Mutex<Vec<Results>>,
    /// Whether requests are held unanswered instead of answered.
    holding: AtomicBool,
    /// The held requests: the caller's unique name and the `Request` path.
    held: Mutex<Vec<(String, String)>>,
}

impl Script {
    fn record(&self, call: Call) {
        self.calls.lock().expect("calls").push(call);
    }

    fn next(&self) -> (u32, Results) {
        self.responses
            .lock()
            .expect("responses")
            .pop_front()
            .unwrap_or_default()
    }

    /// Answer the request `token` names for `header`'s sender, as the
    /// portal does: a `Response` signal on the `Request` object.
    async fn respond(
        &self,
        connection: &zbus::Connection,
        header: &Header<'_>,
        options: &Results,
    ) -> OwnedObjectPath {
        let sender = header
            .sender()
            .expect("a method call has a sender")
            .trim_start_matches(':')
            .replace('.', "_");
        let token = options
            .get("handle_token")
            .map(|token| String::try_from(token.clone()).expect("a string token"))
            .expect("the provider names its request");
        let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
        if self.holding.load(Ordering::SeqCst) {
            let caller = header
                .sender()
                .expect("a method call has a sender")
                .to_string();
            self.held.lock().expect("held").push((caller, path.clone()));
            return OwnedObjectPath::try_from(path).expect("a valid request path");
        }
        let answer = self.next();
        connection
            .emit_signal(
                None::<&str>,
                path.as_str(),
                "org.freedesktop.portal.Request",
                "Response",
                &answer,
            )
            .await
            .expect("the response is sent");
        OwnedObjectPath::try_from(path).expect("a valid request path")
    }
}

fn fd_writable(fd: &OwnedFd) -> bool {
    // The access mode is the low two bits of `fdinfo`'s octal `flags`.
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", fd.as_raw_fd()))
        .expect("the fd has its info");
    let flags = info
        .lines()
        .find_map(|line| line.strip_prefix("flags:"))
        .map(|flags| u32::from_str_radix(flags.trim(), 8).expect("octal flags"))
        .expect("fdinfo names the flags");
    flags & 0o3 == 0o2
}

fn fd_path(fd: &OwnedFd) -> PathBuf {
    std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd())).expect("the fd names a file")
}

struct OpenUri(Arc<Script>);

#[zbus::interface(name = "org.freedesktop.portal.OpenURI")]
impl OpenUri {
    #[zbus(name = "OpenURI")]
    async fn open_uri(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        parent: String,
        uri: String,
        options: Results,
    ) -> OwnedObjectPath {
        self.0.record(Call::OpenUri { parent, uri });
        self.0.respond(connection, &header, &options).await
    }

    async fn open_file(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        parent: String,
        fd: OwnedFd,
        options: Results,
    ) -> OwnedObjectPath {
        let path = fd_path(&fd);
        self.0.record(Call::OpenFile { parent, path });
        self.0.respond(connection, &header, &options).await
    }

    async fn open_directory(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        parent: String,
        fd: OwnedFd,
        options: Results,
    ) -> OwnedObjectPath {
        let path = fd_path(&fd);
        self.0.record(Call::OpenDirectory { parent, path });
        self.0.respond(connection, &header, &options).await
    }
}

struct Trash(Arc<Script>);

#[zbus::interface(name = "org.freedesktop.portal.Trash")]
impl Trash {
    fn trash_file(&self, fd: OwnedFd) -> u32 {
        self.0.record(Call::Trash {
            path: fd_path(&fd),
            writable: fd_writable(&fd),
        });
        *self.0.trashed.lock().expect("trashed")
    }
}

struct FileChooser(Arc<Script>);

fn option_names(options: &Results) -> Vec<String> {
    let mut names = options.keys().cloned().collect::<Vec<String>>();
    names.sort();
    names
}

#[zbus::interface(name = "org.freedesktop.portal.FileChooser")]
impl FileChooser {
    async fn open_file(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        parent: String,
        title: String,
        options: Results,
    ) -> OwnedObjectPath {
        let names = option_names(&options);
        self.0.record(Call::Open {
            parent,
            title,
            options: names,
        });
        self.0
            .dialog_options
            .lock()
            .expect("options")
            .push(options.clone());
        self.0.respond(connection, &header, &options).await
    }

    async fn save_file(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        parent: String,
        title: String,
        options: Results,
    ) -> OwnedObjectPath {
        let names = option_names(&options);
        self.0.record(Call::Save {
            parent,
            title,
            options: names,
        });
        self.0
            .dialog_options
            .lock()
            .expect("options")
            .push(options.clone());
        self.0.respond(connection, &header, &options).await
    }
}

/// A private bus with a stand-in portal on it.
struct StandIn {
    script: Arc<Script>,
    server: Option<zbus::Connection>,
    bus: PrivateBus,
}

impl StandIn {
    fn serve() -> Self {
        Self::serve_on(PrivateBus::start())
    }

    /// The stand-in on `bus`, which may have started it.
    fn serve_on(bus: PrivateBus) -> Self {
        let script = Arc::new(Script::default());
        let path = "/org/freedesktop/portal/desktop";
        let server = zruntime::block_on(
            zbus::connection::Builder::address(bus.address.as_str())
                .name("org.freedesktop.portal.Desktop")
                .serve_at(path, OpenUri(Arc::clone(&script)))
                .serve_at(path, Trash(Arc::clone(&script)))
                .serve_at(path, FileChooser(Arc::clone(&script)))
                .build(),
        )
        .expect("the stand-in owns the portal's name");
        Self {
            script,
            server: Some(server),
            bus,
        }
    }

    /// Hold every later request unanswered.
    fn hold(&self) {
        self.script.holding.store(true, Ordering::SeqCst);
    }

    /// The requests held so far: the caller's unique name and the path.
    fn held(&self) -> Vec<(String, String)> {
        self.script.held.lock().expect("held").clone()
    }

    /// Answer the held request at `path` as the portal.
    fn release(&self, path: &str, code: u32, results: Results) {
        let server = self.server.as_ref().expect("the stand-in is on the bus");
        zruntime::block_on(server.emit_signal(
            None::<&str>,
            path,
            "org.freedesktop.portal.Request",
            "Response",
            &(code, results),
        ))
        .expect("the response is sent");
    }

    /// Leave the bus, as a portal that exits or restarts does.
    fn depart(&mut self) {
        let server = self.server.take().expect("the stand-in is on the bus");
        zruntime::block_on(server.release_name("org.freedesktop.portal.Desktop"))
            .expect("the stand-in gives up the portal's name");
        drop(server);
    }

    fn plugin(&self) -> PortalOnBus {
        PortalOnBus::new(self.bus.address.as_str())
    }

    fn answer(&self, code: u32, results: Results) {
        self.script
            .responses
            .lock()
            .expect("responses")
            .push_back((code, results));
    }

    fn calls(&self) -> Vec<Call> {
        self.script.calls.lock().expect("calls").clone()
    }
}

/// Land `completion`'s outcome, pumping `host` until it settles.
fn settle<T: 'static>(host: &mut HeadlessHost, completion: Completion<T>) -> Result<T, Abandoned> {
    let slot = outcome_slot::<Result<T, Abandoned>>();
    let into = Rc::clone(&slot);
    completion.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
    let deadline = Instant::now() + Duration::from_secs(10);
    while slot.borrow().is_none() {
        assert!(Instant::now() < deadline, "the portal answered within 10 s");
        turn(host);
        std::thread::sleep(Duration::from_millis(5));
    }
    let outcome: Option<Result<T, Abandoned>> = RefCell::take(&slot);
    outcome.expect("settled")
}

fn uris(uris: &[&str]) -> Results {
    let uris = uris
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<String>>();
    HashMap::from([(
        "uris".to_owned(),
        OwnedVariant::try_from(Variant::from(uris)).expect("an owned array"),
    )])
}

fn scratch_file(name: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("flui-system-portal-{}-{name}", std::process::id()));
    std::fs::write(&path, b"x").expect("a scratch file");
    path
}

#[test]
fn the_launcher_reaches_the_open_uri_and_trash_portals() {
    let portal = StandIn::serve();
    let (mut host, launcher) = acquire::<LauncherCapability>(&portal.plugin());
    let launcher = launcher.expect("the portal provides a launcher on Linux");
    let file = scratch_file("launch.txt");

    let uri = ExternalUri::try_new("https://example.com/a b").expect("a web URI");
    portal.answer(0, Results::new());
    assert_eq!(settle(&mut host, launcher.open_uri(&uri)), Ok(Ok(())));

    portal.answer(1, Results::new());
    assert_eq!(
        settle(&mut host, launcher.open_path(&file)),
        Ok(Err(LaunchError::Denied)),
        "dismissing the application chooser refuses"
    );
    portal.answer(2, Results::new());
    assert_eq!(
        settle(&mut host, launcher.reveal_path(&file)),
        Ok(Err(LaunchError::NoHandler))
    );

    *portal.script.trashed.lock().expect("trashed") = 1;
    assert_eq!(settle(&mut host, launcher.trash_path(&file)), Ok(Ok(())));
    *portal.script.trashed.lock().expect("trashed") = 0;
    assert_eq!(
        settle(&mut host, launcher.trash_path(&file)),
        Ok(Err(LaunchError::Denied))
    );

    let missing = file.with_extension("missing");
    assert_eq!(
        settle(&mut host, launcher.open_path(&missing)),
        Ok(Err(LaunchError::NotFound)),
        "a missing file is refused before the portal is asked"
    );

    *portal.script.trashed.lock().expect("trashed") = 1;
    let link = file.with_extension("link");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&file, &link).expect("a symbolic link");
    assert_eq!(
        settle(&mut host, launcher.trash_path(&link)),
        Ok(Err(LaunchError::Denied)),
        "trashing a link is refused rather than trashing its target"
    );
    let _ = std::fs::remove_file(&link);

    // A file goes to the portal writable, as a sandboxed application's
    // trashing needs; a directory cannot open so and goes read-only.
    let folder = file.with_extension("folder");
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir(&folder).expect("a folder");
    assert_eq!(settle(&mut host, launcher.trash_path(&folder)), Ok(Ok(())));

    let canonical = file.canonicalize().expect("the file exists");
    assert_eq!(
        portal.calls(),
        [
            Call::OpenUri {
                parent: String::new(),
                uri: "https://example.com/a b".to_owned(),
            },
            Call::OpenFile {
                parent: String::new(),
                path: canonical.clone(),
            },
            Call::OpenDirectory {
                parent: String::new(),
                path: canonical.clone(),
            },
            Call::Trash {
                path: canonical.clone(),
                writable: true,
            },
            Call::Trash {
                path: canonical,
                writable: true,
            },
            Call::Trash {
                path: folder.canonicalize().expect("the folder exists"),
                writable: false,
            },
        ]
    );
    let _ = std::fs::remove_file(file);
    let _ = std::fs::remove_dir(folder);
}

#[test]
fn the_dialogs_reach_the_file_chooser_and_answer_its_picks() {
    let portal = StandIn::serve();
    let (mut host, dialogs) = acquire::<FileDialogCapability>(&portal.plugin());
    let dialogs = dialogs.expect("the portal provides dialogs on Linux");

    portal.answer(
        0,
        uris(&["file:///home/me/a%20b.png", "file:///home/me/c.png"]),
    );
    let request = OpenRequest::files()
        .allow_multiple()
        .with_title("Pictures")
        .with_directory("/home/me")
        .with_accept_label("Use")
        .with_filter(FileFilter::new("Images").with_extension("png"));
    assert_eq!(
        settle(&mut host, dialogs.open(request)),
        Ok(Ok(Some(vec![
            PickedFile::new("/home/me/a b.png"),
            PickedFile::new("/home/me/c.png"),
        ])))
    );

    portal.answer(1, Results::new());
    assert_eq!(
        settle(
            &mut host,
            dialogs.save(SaveRequest::new().with_suggested_name("notes.txt"))
        ),
        Ok(Ok(None)),
        "a cancelled dialog answers no pick"
    );

    portal.answer(0, uris(&["https://example.com/remote.txt"]));
    assert_eq!(
        settle(&mut host, dialogs.save(SaveRequest::new())),
        Ok(Err(DialogError::NotAFile))
    );

    portal.answer(2, Results::new());
    assert_eq!(
        settle(&mut host, dialogs.open(OpenRequest::folders())),
        Ok(Err(DialogError::Failed { code: 2 }))
    );

    let calls = portal.calls();
    assert_eq!(
        calls[0],
        Call::Open {
            parent: String::new(),
            title: "Pictures".to_owned(),
            options: [
                "accept_label",
                "current_folder",
                "directory",
                "filters",
                "handle_token",
                "multiple",
            ]
            .map(str::to_owned)
            .to_vec(),
        }
    );
    assert_eq!(
        calls[1],
        Call::Save {
            parent: String::new(),
            title: String::new(),
            options: ["current_name", "handle_token"].map(str::to_owned).to_vec(),
        }
    );
    let options = portal.script.dialog_options.lock().expect("options");
    let folder = Vec::<u8>::try_from(options[0]["current_folder"].clone()).expect("bytes");
    assert_eq!(folder, b"/home/me\0", "the folder is NUL-terminated bytes");
    let filters = Vec::<(String, Vec<(u32, String)>)>::try_from(options[0]["filters"].clone())
        .expect("a(sa(us))");
    assert_eq!(
        filters,
        [("Images".to_owned(), vec![(0, "*.png".to_owned())])]
    );
}

#[test]
fn without_a_portal_on_the_bus_the_services_are_unavailable() {
    let bus = PrivateBus::start();
    let plugin = PortalOnBus::new(bus.address.as_str());
    let (mut host, launcher) = acquire::<LauncherCapability>(&plugin);
    let launcher = launcher.expect("the launcher is handed out before the bus is asked");
    let uri = ExternalUri::try_new("mailto:someone@example.com").expect("a mail URI");
    let answered: Result<LaunchResult, Abandoned> = settle(&mut host, launcher.open_uri(&uri));
    assert!(
        matches!(answered, Ok(Err(LaunchError::Unavailable(_)))),
        "{answered:?}"
    );

    // No bus at all: the address names no socket.
    let missing = std::env::temp_dir().join(format!("flui-no-bus-{}", std::process::id()));
    let plugin = PortalOnBus::new(format!("unix:path={}", missing.display()));
    let (mut host, dialogs) = acquire::<FileDialogCapability>(&plugin);
    let dialogs = dialogs.expect("the dialogs are handed out before the bus is asked");
    let answered = settle(&mut host, dialogs.open(OpenRequest::files()));
    assert!(
        matches!(answered, Ok(Err(DialogError::Unavailable(_)))),
        "{answered:?}"
    );
}

#[test]
fn an_installed_portal_that_is_not_running_is_started_first() {
    let services =
        std::env::temp_dir().join(format!("flui-system-services-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&services);
    std::fs::create_dir_all(&services).expect("a service directory");
    let started = services.join("started");
    // The bus runs this to start the portal; the test then serves the
    // stand-in under the portal's name, which completes the start.
    std::fs::write(
        services.join("org.freedesktop.portal.Desktop.service"),
        format!(
            "[D-BUS Service]\nName=org.freedesktop.portal.Desktop\n\
             Exec=/bin/sh -c \"touch '{}'; exec sleep 20\"\n",
            started.display()
        ),
    )
    .expect("the service file is written");
    let bus = PrivateBus::with_services(&services);
    let plugin = PortalOnBus::new(bus.address.as_str());
    let (mut host, launcher) = acquire::<LauncherCapability>(&plugin);
    let launcher = launcher.expect("the portal provides a launcher on Linux");

    let uri = ExternalUri::try_new("https://example.com/").expect("a web URI");
    let slot = outcome_slot::<Result<LaunchResult, Abandoned>>();
    let into = Rc::clone(&slot);
    launcher
        .open_uri(&uri)
        .on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
    pump_until(&mut host, "the bus starts the portal", || started.exists());
    let portal = StandIn::serve_on(bus);
    pump_until(&mut host, "the started portal answers", || {
        slot.borrow().is_some()
    });
    assert_eq!(slot.borrow_mut().take(), Some(Ok(Ok(()))));
    assert_eq!(
        portal.calls(),
        [Call::OpenUri {
            parent: String::new(),
            uri: "https://example.com/".to_owned(),
        }]
    );
    let _ = std::fs::remove_dir_all(&services);
}

#[test]
fn a_session_bus_that_restarts_is_connected_to_again() {
    let directory =
        std::env::temp_dir().join(format!("flui-system-restart-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a bus directory");
    let socket = directory.join("bus");
    let listen = format!("unix:path={}", socket.display());
    // The address names the socket alone, as a session's does: the
    // restarted bus has another identity.
    let plugin = PortalOnBus::new(listen.as_str());
    let (mut host, launcher) = acquire::<LauncherCapability>(&plugin);
    let launcher = launcher.expect("the portal provides a launcher on Linux");
    let uri = ExternalUri::try_new("https://example.com/").expect("a web URI");

    let portal = StandIn::serve_on(PrivateBus::configured(&directory, &listen));
    assert_eq!(settle(&mut host, launcher.open_uri(&uri)), Ok(Ok(())));
    drop(portal);
    let _ = std::fs::remove_file(&socket);
    // The bus's end reaches the portal's connection.
    std::thread::sleep(Duration::from_millis(200));

    let portal = StandIn::serve_on(PrivateBus::configured(&directory, &listen));
    assert_eq!(
        settle(&mut host, launcher.open_uri(&uri)),
        Ok(Ok(())),
        "the request after the restart reaches the new bus"
    );
    assert_eq!(portal.calls().len(), 1);
    drop(portal);
    let _ = std::fs::remove_dir_all(&directory);
}
#[test]
fn a_named_pipe_without_a_writer_does_not_stall_the_caller() {
    let portal = StandIn::serve();
    let (mut host, launcher) = acquire::<LauncherCapability>(&portal.plugin());
    let launcher = launcher.expect("the portal provides a launcher on Linux");
    let fifo = std::env::temp_dir().join(format!("flui-system-fifo-{}", std::process::id()));
    let _ = std::fs::remove_file(&fifo);
    let status = Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo runs");
    assert!(status.success(), "a named pipe is made");
    // A blocked open would otherwise hang the test: a writer arrives late.
    let late = fifo.clone();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(3));
        let _ = std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&late);
    });

    let asked = Instant::now();
    let completion = launcher.open_path(&fifo);
    assert!(
        asked.elapsed() < Duration::from_secs(1),
        "asking returned after {:?}",
        asked.elapsed()
    );
    assert_eq!(settle(&mut host, completion), Ok(Ok(())));
    writer.join().expect("the writer ends");
    let _ = std::fs::remove_file(&fifo);
}

/// Pump `host` until `ready` holds.
fn pump_until(host: &mut HeadlessHost, what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(Instant::now() < deadline, "{what} within 10 s");
        turn(host);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_response_from_another_client_is_not_heard() {
    let portal = StandIn::serve();
    let (mut host, dialogs) = acquire::<FileDialogCapability>(&portal.plugin());
    let dialogs = dialogs.expect("the portal provides dialogs on Linux");
    portal.hold();

    let slot = outcome_slot::<Result<OpenResult, Abandoned>>();
    let into = Rc::clone(&slot);
    dialogs
        .open(OpenRequest::files())
        .on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
    pump_until(&mut host, "the dialog reached the portal", || {
        !portal.held().is_empty()
    });
    let (caller, path) = portal.held().remove(0);

    // Another client answers first, sent to the caller directly.
    let rogue =
        zruntime::block_on(zbus::connection::Builder::address(portal.bus.address.as_str()).build())
            .expect("another client connects");
    zruntime::block_on(rogue.emit_signal(
        Some(caller.as_str()),
        path.as_str(),
        "org.freedesktop.portal.Request",
        "Response",
        &(0_u32, uris(&["file:///etc/shadow"])),
    ))
    .expect("the forged response is sent");
    for _ in 0..20 {
        turn(&mut host);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(slot.borrow().is_none(), "the forged answer was not taken");

    portal.release(&path, 1, Results::new());
    pump_until(&mut host, "the portal's own answer", || {
        slot.borrow().is_some()
    });
    assert_eq!(slot.borrow_mut().take(), Some(Ok(Ok(None))));
}

#[test]
fn a_portal_that_leaves_before_answering_ends_the_dialog_and_frees_the_window() {
    let mut portal = StandIn::serve();
    let (mut host, dialogs) = acquire::<FileDialogCapability>(&portal.plugin());
    let dialogs = dialogs.expect("the portal provides dialogs on Linux");
    portal.hold();

    let slot = outcome_slot::<Result<OpenResult, Abandoned>>();
    let into = Rc::clone(&slot);
    dialogs
        .open(OpenRequest::files())
        .on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
    pump_until(&mut host, "the dialog reached the portal", || {
        !portal.held().is_empty()
    });
    portal.depart();
    pump_until(&mut host, "the dialog ended", || slot.borrow().is_some());
    let ended = slot.borrow_mut().take();
    assert!(
        matches!(ended, Some(Ok(Err(DialogError::Unavailable(_))))),
        "{ended:?}"
    );

    // The window is free again: the next dialog is not `Busy`.
    let next = settle(&mut host, dialogs.save(SaveRequest::new()));
    assert!(
        matches!(next, Ok(Err(DialogError::Unavailable(_)))),
        "{next:?}"
    );
}
