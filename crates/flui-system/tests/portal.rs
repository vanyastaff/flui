//! The Linux providers end to end: a widget's launcher and dialogs reach a
//! stand-in desktop portal served on a private bus, and its answers come
//! back through the `Request` objects as the real portal sends them.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use flui_platform::capability::{Abandoned, Completion};
use flui_platform::launch::{ExternalUri, LaunchError, LaunchResult, LauncherCapability};
use flui_system::dialog::{
    DialogError, FileDialogCapability, FileFilter, OpenRequest, PickedFile, SaveRequest,
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
        let mut daemon = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address"])
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
        self.0.record(Call::Trash { path: fd_path(&fd) });
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
    _server: zbus::Connection,
    bus: PrivateBus,
}

impl StandIn {
    fn serve() -> Self {
        let bus = PrivateBus::start();
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
            _server: server,
            bus,
        }
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
            },
            Call::Trash { path: canonical },
        ]
    );
    let _ = std::fs::remove_file(file);
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
}
