//! Behavior every system provider shares, driven through public acquisition
//! and a headless host. Backends adapt their answers and request records to
//! these typed scenarios; protocol and provider-failure tests stay separate.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

use flui_platform::capability::{Abandoned, Completion, Plugin};
use flui_platform::launch::{ExternalUri, LaunchError, LaunchResult, Launcher, LauncherCapability};
use flui_platform::{PlatformWindow, WindowId};
use flui_sdk::testing::HeadlessHost;
use flui_system::dialog::{
    DialogError, FileDialogCapability, FileDialogs, FileFilter, OpenRequest, OpenResult,
    PickedFile, SaveRequest, SaveResult,
};
use flui_system::testing::{
    DialogRequest, LaunchRequest, ScriptedFileDialogs, ScriptedLauncher, ScriptedSystem,
};

use crate::{acquire, outcome_slot, turn};

/// How quickly a backend can answer once its script has an answer.
#[derive(Clone, Copy)]
pub(super) enum SettlePolicy {
    /// The answer is already queued, so exactly one owner turn settles it.
    NextTurn,
    /// An asynchronous backend answers during a later owner turn.
    #[cfg(target_os = "linux")]
    Eventual,
}

/// A normalized open request observed at a backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct OpenAsk {
    pub(super) title: Option<String>,
    pub(super) directory: Option<PathBuf>,
    pub(super) filters: Vec<FileFilter>,
    pub(super) accept_label: Option<String>,
    pub(super) multiple: bool,
    pub(super) folders: bool,
}

impl From<OpenRequest> for OpenAsk {
    fn from(request: OpenRequest) -> Self {
        Self {
            title: request.title().map(str::to_owned),
            directory: request.directory().map(PathBuf::from),
            filters: request.filters().to_vec(),
            accept_label: request.accept_label().map(str::to_owned),
            multiple: request.is_multiple(),
            folders: request.picks_folders(),
        }
    }
}

/// A normalized save request observed at a backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SaveAsk {
    pub(super) title: Option<String>,
    pub(super) directory: Option<PathBuf>,
    pub(super) filters: Vec<FileFilter>,
    pub(super) accept_label: Option<String>,
    pub(super) suggested_name: Option<String>,
}

impl From<SaveRequest> for SaveAsk {
    fn from(request: SaveRequest) -> Self {
        Self {
            title: request.title().map(str::to_owned),
            directory: request.directory().map(PathBuf::from),
            filters: request.filters().to_vec(),
            accept_label: request.accept_label().map(str::to_owned),
            suggested_name: request.suggested_name().map(str::to_owned),
        }
    }
}

/// One normalized dialog request, independent of its wire representation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum DialogAsk {
    Open(OpenAsk),
    Save(SaveAsk),
}

/// The backend-specific controls the launcher scenarios need.
pub(super) trait LauncherConformance {
    fn plugin(&self) -> &dyn Plugin;
    fn answer_next(&self, result: LaunchResult);
    fn hold(&self);
    fn release(&self, result: LaunchResult) -> bool;
    fn requests(&self) -> Vec<LaunchRequest>;

    fn settle_policy(&self) -> SettlePolicy {
        SettlePolicy::NextTurn
    }
}

/// The backend-specific controls the dialog scenarios need.
pub(super) trait DialogConformance {
    fn plugin(&self) -> &dyn Plugin;
    fn answer_next_open(&self, result: OpenResult);
    fn answer_next_save(&self, result: SaveResult);
    fn hold(&self);
    fn release_open(&self, result: OpenResult) -> bool;
    fn release_save(&self, result: SaveResult) -> bool;
    fn requests(&self) -> Vec<DialogAsk>;
    fn assert_modal_to(&self, window: WindowId);

    fn settle_policy(&self) -> SettlePolicy {
        SettlePolicy::NextTurn
    }
}

enum Scenario {
    Launcher {
        name: &'static str,
        run: fn(&dyn LauncherConformance),
    },
    Dialog {
        name: &'static str,
        run: fn(&dyn DialogConformance),
    },
}

/// The crate's one conformance table (ADR-0154 §10).
const SCENARIOS: &[Scenario] = &[
    Scenario::Launcher {
        name: "open URI results and normalized requests",
        run: open_uri_results_and_normalized_requests,
    },
    Scenario::Launcher {
        name: "relative paths refused at the seam",
        run: relative_paths_refused_at_the_seam,
    },
    Scenario::Launcher {
        name: "held request released at an owner turn",
        run: held_request_released_at_an_owner_turn,
    },
    Scenario::Dialog {
        name: "open picks and normalized request",
        run: open_picks_and_normalized_request,
    },
    Scenario::Dialog {
        name: "cancelled save answers none",
        run: cancelled_save_answers_none,
    },
    Scenario::Dialog {
        name: "open failure carries its backend code",
        run: open_failure_carries_its_backend_code,
    },
    Scenario::Dialog {
        name: "one dialog per window and busy cleanup",
        run: one_dialog_per_window_and_busy_cleanup,
    },
    Scenario::Dialog {
        name: "malformed requests refused at the seam",
        run: malformed_requests_refused_at_the_seam,
    },
];

/// Run every row against a fresh backend.
pub(super) fn run_scenarios<B: LauncherConformance + DialogConformance + 'static>(
    mut backend: impl FnMut() -> B,
) {
    for scenario in SCENARIOS {
        match scenario {
            Scenario::Launcher { name, run } => {
                eprintln!("launcher conformance: {name}");
                run(&backend());
            }
            Scenario::Dialog { name, run } => {
                eprintln!("dialog conformance: {name}");
                run(&backend());
            }
        }
    }
}

struct ScriptedConformance {
    launcher: ScriptedLauncher,
    dialogs: ScriptedFileDialogs,
    plugin: ScriptedSystem,
}

impl ScriptedConformance {
    fn new() -> Self {
        let launcher = ScriptedLauncher::new();
        let dialogs = ScriptedFileDialogs::new();
        let plugin = ScriptedSystem::new(launcher.clone(), dialogs.clone());
        Self {
            launcher,
            dialogs,
            plugin,
        }
    }
}

impl LauncherConformance for ScriptedConformance {
    fn plugin(&self) -> &dyn Plugin {
        &self.plugin
    }

    fn answer_next(&self, result: LaunchResult) {
        self.launcher.answer_next(result);
    }

    fn hold(&self) {
        self.launcher.hold();
    }

    fn release(&self, result: LaunchResult) -> bool {
        self.launcher.release(result)
    }

    fn requests(&self) -> Vec<LaunchRequest> {
        self.launcher.requests()
    }
}

impl DialogConformance for ScriptedConformance {
    fn plugin(&self) -> &dyn Plugin {
        &self.plugin
    }

    fn answer_next_open(&self, result: OpenResult) {
        self.dialogs.answer_next_open(result);
    }

    fn answer_next_save(&self, result: SaveResult) {
        self.dialogs.answer_next_save(result);
    }

    fn hold(&self) {
        self.dialogs.hold();
    }

    fn release_open(&self, result: OpenResult) -> bool {
        self.dialogs.release_open(result)
    }

    fn release_save(&self, result: SaveResult) -> bool {
        self.dialogs.release_save(result)
    }

    fn requests(&self) -> Vec<DialogAsk> {
        self.dialogs
            .requests()
            .into_iter()
            .map(|request| match request {
                DialogRequest::Open { request, .. } => DialogAsk::Open(OpenAsk::from(request)),
                DialogRequest::Save { request, .. } => DialogAsk::Save(SaveAsk::from(request)),
                _ => panic!("an unknown scripted dialog request"),
            })
            .collect::<Vec<DialogAsk>>()
    }

    fn assert_modal_to(&self, window: WindowId) {
        for request in self.dialogs.requests() {
            let (DialogRequest::Open { window: asked, .. }
            | DialogRequest::Save { window: asked, .. }) = request
            else {
                panic!("an unknown scripted dialog request");
            };
            assert_eq!(asked, window);
        }
    }
}

#[test]
fn scripted_system_conformance() {
    run_scenarios(ScriptedConformance::new);
}

type Slot<T> = Rc<RefCell<Option<Result<T, Abandoned>>>>;

fn land<T: 'static>(completion: Completion<T>) -> Slot<T> {
    let slot = outcome_slot::<Result<T, Abandoned>>();
    let into = Rc::clone(&slot);
    completion.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
    assert!(
        slot.borrow().is_none(),
        "the completion did not run inside the request"
    );
    slot
}

fn take<T>(slot: &Slot<T>) -> Result<T, Abandoned> {
    RefCell::take(slot).expect("the completion settled")
}

fn settle_slot<T: 'static>(
    policy: SettlePolicy,
    host: &mut HeadlessHost,
    slot: &Slot<T>,
) -> Result<T, Abandoned> {
    match policy {
        SettlePolicy::NextTurn => turn(host),
        #[cfg(target_os = "linux")]
        SettlePolicy::Eventual => {
            pump_until(host, "the backend answered", || slot.borrow().is_some());
        }
    }
    take(slot)
}

fn settle_with<T: 'static>(
    policy: SettlePolicy,
    host: &mut HeadlessHost,
    completion: Completion<T>,
) -> Result<T, Abandoned> {
    let slot = land(completion);
    settle_slot(policy, host, &slot)
}

/// Land an asynchronous portal completion, pumping until its owner turn.
#[cfg(target_os = "linux")]
pub(super) fn settle<T: 'static>(
    host: &mut HeadlessHost,
    completion: Completion<T>,
) -> Result<T, Abandoned> {
    settle_with(SettlePolicy::Eventual, host, completion)
}

/// Pump `host` until `ready` holds.
#[cfg(target_os = "linux")]
pub(super) fn pump_until(host: &mut HeadlessHost, what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(Instant::now() < deadline, "{what} within 10 s");
        turn(host);
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn launcher(backend: &dyn LauncherConformance) -> (HeadlessHost, Launcher) {
    let (host, launcher) = acquire::<LauncherCapability>(backend.plugin());
    (
        host,
        launcher.expect("the conformance provider supplies a launcher"),
    )
}

fn dialogs(backend: &dyn DialogConformance) -> (HeadlessHost, FileDialogs) {
    let (host, dialogs) = acquire::<FileDialogCapability>(backend.plugin());
    (
        host,
        dialogs.expect("the conformance provider supplies dialogs"),
    )
}

fn open_uri_results_and_normalized_requests(backend: &dyn LauncherConformance) {
    let (mut host, launcher) = launcher(backend);
    let uri = ExternalUri::try_new("https://example.com/a b").expect("a web URI");
    let results = [
        Ok(()),
        Err(LaunchError::Denied),
        Err(LaunchError::NoHandler),
    ];

    for result in results {
        backend.answer_next(result);
        assert_eq!(
            settle_with(backend.settle_policy(), &mut host, launcher.open_uri(&uri)),
            Ok(result)
        );
    }
    assert_eq!(
        backend.requests(),
        vec![LaunchRequest::OpenUri(uri); results.len()]
    );
}

fn relative_paths_refused_at_the_seam(backend: &dyn LauncherConformance) {
    let (mut host, launcher) = launcher(backend);
    let relative = std::path::Path::new("notes/today.txt");
    let answers = [
        launcher.open_path(relative),
        launcher.reveal_path(relative),
        launcher.trash_path(relative),
    ]
    .map(land);

    turn(&mut host);
    assert!(backend.requests().is_empty(), "the system was never asked");
    for answer in answers {
        assert_eq!(take(&answer), Ok(Err(LaunchError::RelativePath)));
    }
}

fn held_request_released_at_an_owner_turn(backend: &dyn LauncherConformance) {
    let (mut host, launcher) = launcher(backend);
    let uri = ExternalUri::try_new("https://example.com/held").expect("a web URI");
    backend.hold();
    let answered = land(launcher.open_uri(&uri));

    turn(&mut host);
    assert!(answered.borrow().is_none(), "held until released");
    assert_eq!(backend.requests(), [LaunchRequest::OpenUri(uri)]);
    assert!(backend.release(Err(LaunchError::Denied)));
    assert_eq!(
        settle_slot(backend.settle_policy(), &mut host, &answered),
        Ok(Err(LaunchError::Denied))
    );
}

fn open_picks_and_normalized_request(backend: &dyn DialogConformance) {
    let (mut host, dialogs) = dialogs(backend);
    let directory = std::env::temp_dir();
    assert!(directory.is_absolute());
    let picks = vec![
        PickedFile::new(directory.join("a.png")),
        PickedFile::new(directory.join("b.png")),
    ];
    backend.answer_next_open(Ok(Some(picks.clone())));
    let request = OpenRequest::files()
        .allow_multiple()
        .with_title("Choose pictures")
        .with_directory(&directory)
        .with_accept_label("Use")
        .with_filter(
            FileFilter::new("Images")
                .with_extension(".png")
                .with_mime_type("image/png"),
        );

    let answered = land(dialogs.open(request.clone()));
    assert_eq!(
        settle_slot(backend.settle_policy(), &mut host, &answered),
        Ok(Ok(Some(picks)))
    );
    let requests = backend.requests();
    assert_eq!(requests, [DialogAsk::Open(OpenAsk::from(request))]);
    let [DialogAsk::Open(asked)] = requests.as_slice() else {
        panic!("one normalized open request: {requests:?}");
    };
    assert_eq!(
        asked.filters[0].extensions(),
        ["png"],
        "the leading dot is dropped"
    );
    backend.assert_modal_to(host.window().id());
}

fn cancelled_save_answers_none(backend: &dyn DialogConformance) {
    let (mut host, dialogs) = dialogs(backend);
    backend.answer_next_save(Ok(None));
    let request = SaveRequest::new().with_suggested_name("notes.txt");

    assert_eq!(
        settle_with(
            backend.settle_policy(),
            &mut host,
            dialogs.save(request.clone())
        ),
        Ok(Ok(None))
    );
    assert_eq!(
        backend.requests(),
        [DialogAsk::Save(SaveAsk::from(request))]
    );
    backend.assert_modal_to(host.window().id());
}

fn open_failure_carries_its_backend_code(backend: &dyn DialogConformance) {
    let (mut host, dialogs) = dialogs(backend);
    backend.answer_next_open(Err(DialogError::Failed { code: 2 }));
    let request = OpenRequest::folders();

    assert_eq!(
        settle_with(
            backend.settle_policy(),
            &mut host,
            dialogs.open(request.clone())
        ),
        Ok(Err(DialogError::Failed { code: 2 }))
    );
    assert_eq!(
        backend.requests(),
        [DialogAsk::Open(OpenAsk::from(request))]
    );
    backend.assert_modal_to(host.window().id());
}

fn one_dialog_per_window_and_busy_cleanup(backend: &dyn DialogConformance) {
    let (mut host, dialogs) = dialogs(backend);
    backend.hold();
    let first_request = OpenRequest::files();
    let first = land(dialogs.open(first_request.clone()));
    // A second handle of the same window shares the rule.
    let other = dialogs.clone();
    let second = land(other.save(SaveRequest::new()));

    turn(&mut host);
    assert!(first.borrow().is_none(), "the first dialog is still open");
    assert_eq!(take(&second), Ok(Err(DialogError::Busy)));
    assert_eq!(
        backend.requests(),
        [DialogAsk::Open(OpenAsk::from(first_request.clone()))],
        "the busy request never reached the system"
    );
    backend.assert_modal_to(host.window().id());

    assert!(backend.release_open(Ok(None)));
    assert_eq!(
        settle_slot(backend.settle_policy(), &mut host, &first),
        Ok(Ok(None))
    );

    let third_request = SaveRequest::new();
    let third = land(dialogs.save(third_request.clone()));
    turn(&mut host);
    assert!(third.borrow().is_none(), "shown once the first answered");
    let picked = PickedFile::new(std::env::temp_dir().join("out.txt"));
    assert!(backend.release_save(Ok(Some(picked.clone()))));
    assert_eq!(
        settle_slot(backend.settle_policy(), &mut host, &third),
        Ok(Ok(Some(picked)))
    );
    assert_eq!(
        backend.requests(),
        [
            DialogAsk::Open(OpenAsk::from(first_request)),
            DialogAsk::Save(SaveAsk::from(third_request)),
        ]
    );
}

fn malformed_requests_refused_at_the_seam(backend: &dyn DialogConformance) {
    let (mut host, dialogs) = dialogs(backend);
    let images = || FileFilter::new("Images").with_extension("png");
    let opens = [
        OpenRequest::files().with_filter(FileFilter::new("").with_extension("png")),
        OpenRequest::files().with_filter(FileFilter::new("Nothing")),
        OpenRequest::files().with_filter(FileFilter::new("Glob").with_extension("*.png")),
        OpenRequest::files().with_filter(FileFilter::new("Path").with_extension("a/b")),
        OpenRequest::files().with_filter(FileFilter::new("Mime").with_mime_type("image")),
        OpenRequest::folders().with_filter(images()),
        OpenRequest::files().with_directory("relative/dir"),
    ];
    let saves = [
        SaveRequest::new().with_suggested_name("dir/name.txt"),
        SaveRequest::new().with_suggested_name(".."),
        SaveRequest::new().with_suggested_name("  "),
        SaveRequest::new().with_filter(FileFilter::new("Images")),
        SaveRequest::new().with_directory("relative"),
    ];
    let open_answers = opens.map(|request| land(dialogs.open(request)));
    let save_answers = saves.map(|request| land(dialogs.save(request)));

    turn(&mut host);
    assert!(backend.requests().is_empty(), "no dialog was shown");
    for answer in open_answers {
        assert_eq!(take(&answer), Ok(Err(DialogError::InvalidRequest)));
    }
    for answer in save_answers {
        assert_eq!(take(&answer), Ok(Err(DialogError::InvalidRequest)));
    }

    // A refused request leaves the window free for the next one.
    let valid_request = OpenRequest::files().with_filter(images());
    assert_eq!(
        settle_with(
            backend.settle_policy(),
            &mut host,
            dialogs.open(valid_request.clone())
        ),
        Ok(Ok(None))
    );
    assert_eq!(
        backend.requests(),
        [DialogAsk::Open(OpenAsk::from(valid_request))]
    );
}
