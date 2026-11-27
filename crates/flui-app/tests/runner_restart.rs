//! The development restart (ADR-0393 §4): a version of another layout
//! retires the running application, runs its entry again and composes what
//! the entry hands over into the window the loop kept. Each row runs a whole
//! application on the headless platform with the testing kit's scripted
//! hook.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use flui_app::testing::{
    HeadlessRun, HeadlessRunHandle, RestartReason, ScriptedOutcome, ScriptedReload,
};
use flui_app::{
    AppConfig, AppHandle, Application, CloseRequestHandler, CloseResponse, ExitPolicy, LaunchError,
    ServiceDefinition, ServiceLifetime, WindowPolicy, open_secondary_window,
};
use flui_view::dev_reload::{CodeVersion, LayoutEpoch, Revision, Unapplied};

fn version(layout: u64, revision: u64) -> CodeVersion {
    CodeVersion::new(
        LayoutEpoch::try_new(layout).expect("a test layout is not zero"),
        Revision::try_new(revision).expect("a test revision is not zero"),
    )
}

/// A restart to `version` that applied everything the application
/// configured.
fn restarted(version: CodeVersion) -> ScriptedOutcome {
    ScriptedOutcome::Restarted(version, Unapplied::default())
}

/// How many states of one [`Note`] were created and disposed.
#[derive(Default)]
struct Lives {
    created: AtomicUsize,
    disposed: AtomicUsize,
}

impl Lives {
    fn new() -> Arc<Self> {
        Arc::default()
    }

    fn created(&self) -> usize {
        self.created.load(Ordering::SeqCst)
    }

    fn disposed(&self) -> usize {
        self.disposed.load(Ordering::SeqCst)
    }
}

/// A stateful root that counts its states' lives, and panics where a test
/// asks it to.
#[derive(Clone)]
struct Note {
    lives: Arc<Lives>,
    build_panics: bool,
    dispose_panics: bool,
}

struct NoteState {
    lives: Arc<Lives>,
    dispose_panics: bool,
}

impl flui_view::StatefulView for Note {
    type State = NoteState;

    fn create_state(&self, _ctx: &dyn flui_view::LifecycleContext) -> Self::State {
        self.lives.created.fetch_add(1, Ordering::SeqCst);
        NoteState {
            lives: Arc::clone(&self.lives),
            dispose_panics: self.dispose_panics,
        }
    }
}

impl flui_view::ViewState<Note> for NoteState {
    fn build(&self, view: &Note, _ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
        assert!(!view.build_panics, "the later root's build fails");
        flui_widgets::SizedBox::square(10.0)
    }

    fn dispose(&mut self) {
        self.lives.disposed.fetch_add(1, Ordering::SeqCst);
        assert!(!self.dispose_panics, "the earlier state's dispose fails");
    }
}

impl flui_view::View for Note {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

fn note(lives: &Arc<Lives>) -> Note {
    Note {
        lives: Arc::clone(lives),
        build_panics: false,
        dispose_panics: false,
    }
}

fn development(hook: &ScriptedReload) -> AppConfig {
    AppConfig::new()
        .exit_policy(ExitPolicy::ExplicitQuit)
        .dev_reload(hook.clone())
}

/// `application` running `first` with `hook` installed, its main window
/// open and on the first version.
fn running_with(
    hook: &ScriptedReload,
    application: Application,
    first: impl flui_view::View + Clone + 'static,
) -> HeadlessRunHandle {
    let run = HeadlessRun::start(application, first);
    run.run_owner_turn();
    hook.load(version(1, 1));
    turns(&run);
    let reports = hook.take_outcomes();
    assert!(
        !reports.is_empty()
            && reports
                .iter()
                .all(|report| *report == ScriptedOutcome::Reassembled(version(1, 1))),
        "the first version is the one the application was composed from, which each UI \
         runtime, on no known revision yet, reassembles to once: {reports:?}"
    );
    run
}

fn running(hook: &ScriptedReload, first: Note) -> HeadlessRunHandle {
    running_with(hook, Application::new().config(development(hook)), first)
}

/// An entry that runs `later` as the application's root.
fn runs(later: Note) -> impl FnMut() + Send + 'static {
    move || {
        Application::new()
            .run(later.clone())
            .expect("a run inside a restart hands its application over");
    }
}

fn turns(run: &HeadlessRunHandle) {
    for _ in 0..4 {
        run.run_owner_turn();
    }
}

fn window_ids(run: &HeadlessRunHandle) -> Vec<flui_platform::WindowId> {
    run.open_windows()
        .iter()
        .map(flui_platform::PlatformWindow::id)
        .collect()
}

/// §4 steps 1 to 4: the earlier application's states are disposed before
/// its entry runs, the entry's application is composed into the same
/// window, and the hook hears `Restarted`.
fn a_restart_disposes_every_state_before_the_entry_runs() {
    let (first, later) = (Lives::new(), Lives::new());
    let hook = ScriptedReload::new();
    let disposed_when_entered = Arc::new(AtomicUsize::new(usize::MAX));
    {
        let (first, seen, mut entry) = (
            Arc::clone(&first),
            Arc::clone(&disposed_when_entered),
            runs(note(&later)),
        );
        hook.entry(move || {
            seen.store(first.disposed(), Ordering::SeqCst);
            entry();
        });
    }
    let run = running(&hook, note(&first));
    let window = window_ids(&run);
    assert_eq!(window.len(), 1, "the main window is open");

    hook.load(version(2, 2));
    turns(&run);

    assert_eq!(hook.take_outcomes(), [restarted(version(2, 2))]);
    assert_eq!(hook.entries_run(), 1);
    assert_eq!(
        disposed_when_entered.load(Ordering::SeqCst),
        1,
        "the earlier state was disposed before the entry ran"
    );
    assert_eq!(later.created(), 1, "the later application was composed");
    assert_eq!(window_ids(&run), window, "the restart kept the window");
    run.join().expect("the run ends");
    assert_eq!(
        later.disposed(),
        1,
        "the later application ends with the run"
    );
}

/// §4 step 1: a `dispose` that panics does not stop the unmount or the
/// restart; every other state is disposed.
fn a_panicking_dispose_does_not_stop_a_restart() {
    let (first, later) = (Lives::new(), Lives::new());
    let hook = ScriptedReload::new();
    hook.entry(runs(note(&later)));
    let failing = Note {
        dispose_panics: true,
        ..note(&first)
    };
    let run = running_with(
        &hook,
        Application::new().config(development(&hook)),
        flui_widgets::Column::new((failing, note(&first))),
    );

    hook.load(version(2, 2));
    turns(&run);
    assert_eq!(hook.take_outcomes(), [restarted(version(2, 2))]);
    assert_eq!(first.disposed(), 2, "both earlier states were disposed");
    assert_eq!(later.created(), 1);
    assert!(
        run.is_running(),
        "a retired application's panic ends no loop"
    );
    run.join().expect("the run ends");
}

/// §4 step 1: the windows the earlier application opened close as the end
/// of a run closes them; their close handlers are not asked.
fn a_restart_closes_the_windows_the_application_opened_without_asking() {
    let later = Lives::new();
    let hook = ScriptedReload::new();
    hook.entry(runs(note(&later)));
    let asked = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&asked);
    let application = Application::new()
        .config(development(&hook))
        .on_ready(move |_| {
            let counted = Arc::clone(&counted);
            let refuses = CloseRequestHandler::new(move |_| {
                counted.fetch_add(1, Ordering::SeqCst);
                CloseResponse::KeepOpen
            });
            open_secondary_window(
                AppConfig::new().close_request_handler(refuses),
                WindowPolicy::Isolated,
            )
            .expect("an isolated window opens on the headless platform");
        });
    let run = running_with(&hook, application, note(&Lives::new()));
    let opened = window_ids(&run);
    assert_eq!(opened.len(), 2, "the main window and the application's");

    hook.load(version(2, 2));
    turns(&run);
    assert_eq!(hook.take_outcomes(), [restarted(version(2, 2))]);
    let main = window_ids(&run);
    assert_eq!(main.len(), 1, "only the main window is kept: {main:?}");
    assert!(opened.contains(&main[0]));
    assert_eq!(
        asked.load(Ordering::SeqCst),
        0,
        "no close request was asked"
    );
    run.join().expect("the run ends");
}

/// §4 step 1: the earlier application's scope is dropped: its services are
/// stopped before the entry runs.
fn a_restart_drops_the_earlier_scope() {
    struct Stopped(Arc<AtomicBool>);
    impl Drop for Stopped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let stopped = Arc::new(AtomicBool::new(false));
    let marker = Arc::clone(&stopped);
    let service = ServiceDefinition::new(
        "earlier",
        ServiceLifetime::StopsWithLastWindow,
        move |context| {
            let held = Stopped(Arc::clone(&marker));
            let cancellation = context.cancellation().clone();
            Box::pin(async move {
                let _held = held;
                cancellation.cancelled().await;
            })
        },
    );
    let hook = ScriptedReload::new();
    let stopped_when_entered = Arc::new(AtomicBool::new(false));
    {
        let (stopped, seen, mut entry) = (
            Arc::clone(&stopped),
            Arc::clone(&stopped_when_entered),
            runs(note(&Lives::new())),
        );
        hook.entry(move || {
            seen.store(stopped.load(Ordering::SeqCst), Ordering::SeqCst);
            entry();
        });
    }
    let run = running_with(
        &hook,
        Application::new().config(development(&hook).service(service)),
        note(&Lives::new()),
    );
    assert!(!stopped.load(Ordering::SeqCst), "the service runs");

    hook.load(version(2, 2));
    turns(&run);
    assert_eq!(hook.take_outcomes(), [restarted(version(2, 2))]);
    assert!(
        stopped_when_entered.load(Ordering::SeqCst),
        "the earlier service was stopped before the entry ran"
    );
    run.join().expect("the run ends");
}

/// §4: an `AppHandle` of the earlier application is refused once it
/// retired.
fn an_app_handle_of_the_earlier_application_is_refused() {
    let hook = ScriptedReload::new();
    hook.entry(runs(note(&Lives::new())));
    let earlier: Arc<Mutex<Option<AppHandle>>> = Arc::default();
    let kept = Arc::clone(&earlier);
    let application = Application::new()
        .config(development(&hook))
        .on_ready(move |handle| {
            kept.lock()
                .expect("the test's lock is not poisoned")
                .replace(handle.clone());
        });
    let run = running_with(&hook, application, note(&Lives::new()));

    hook.load(version(2, 2));
    turns(&run);
    assert_eq!(hook.take_outcomes(), [restarted(version(2, 2))]);
    let handle = earlier
        .lock()
        .expect("the test's lock is not poisoned")
        .take()
        .expect("the earlier application was ready");
    assert!(
        handle.request_quit().is_err(),
        "the earlier application's handle reaches nothing"
    );
    turns(&run);
    assert!(run.is_running(), "the later application was not quit");
    run.join().expect("the run ends");
}

/// §4 step 2: an entry that panics fails the restart and is not tried again
/// for that version; the hook stays, the window is kept, and a later
/// version restarts into it.
fn a_panicking_entry_fails_the_restart_and_is_not_tried_again() {
    let later = Lives::new();
    let hook = ScriptedReload::new();
    hook.entry(|| panic!("the entry fails"));
    let run = running(&hook, note(&Lives::new()));
    let window = window_ids(&run);

    hook.load(version(2, 2));
    turns(&run);
    assert_eq!(
        hook.take_outcomes(),
        [ScriptedOutcome::RestartFailed(
            version(2, 2),
            RestartReason::EntryPanicked
        )]
    );
    assert!(hook.is_attached(), "the entry's panic is not the hook's");
    assert_eq!(window_ids(&run), window, "the window is kept");

    hook.load(version(2, 2));
    turns(&run);
    assert_eq!(
        hook.take_outcomes(),
        [],
        "the same version is not tried again"
    );
    assert_eq!(hook.entries_run(), 1);

    hook.entry(runs(note(&later)));
    hook.load(version(3, 3));
    turns(&run);
    assert_eq!(hook.take_outcomes(), [restarted(version(3, 3))]);
    assert_eq!(later.created(), 1);
    assert_eq!(window_ids(&run), window, "into the kept window");
    run.join().expect("the run ends");
}

/// §4 step 2: an entry that hands no application over fails the restart.
fn an_entry_without_an_application_fails_the_restart() {
    let hook = ScriptedReload::new();
    hook.entry(|| {});
    let run = running(&hook, note(&Lives::new()));
    hook.load(version(2, 2));
    turns(&run);
    assert_eq!(
        hook.take_outcomes(),
        [ScriptedOutcome::RestartFailed(
            version(2, 2),
            RestartReason::NoApplication
        )]
    );
    assert_eq!(run.open_windows().len(), 1, "the window is kept");
    run.join().expect("the run ends");
}

/// §4 step 2: a second `run` in one entry is refused, and the first
/// application is the one composed.
fn a_second_run_in_one_entry_is_refused_and_the_first_is_composed() {
    let (composed, refused) = (Lives::new(), Lives::new());
    let hook = ScriptedReload::new();
    let second = Arc::new(Mutex::new(None));
    {
        let (composed, refused, second) = (note(&composed), note(&refused), Arc::clone(&second));
        hook.entry(move || {
            Application::new()
                .run(composed.clone())
                .expect("the first run hands its application over");
            let answer = Application::new().run(refused.clone());
            second
                .lock()
                .expect("the test's lock is not poisoned")
                .replace(answer);
        });
    }
    let run = running(&hook, note(&Lives::new()));
    hook.load(version(2, 2));
    turns(&run);
    assert_eq!(hook.take_outcomes(), [restarted(version(2, 2))]);
    assert!(matches!(
        second
            .lock()
            .expect("the test's lock is not poisoned")
            .take(),
        Some(Err(LaunchError::AlreadyHandedOver))
    ));
    assert_eq!((composed.created(), refused.created()), (1, 0));
    run.join().expect("the run ends");
}

/// §4 step 3: a composition that fails (here the later application's `ready`
/// panics) fails the restart and retires what it built; the window stays.
fn a_failed_composition_retires_what_it_built() {
    let (later, after) = (Lives::new(), Lives::new());
    let hook = ScriptedReload::new();
    {
        let later = note(&later);
        hook.entry(move || {
            Application::new()
                .on_ready(|_| panic!("the later application's ready fails"))
                .run(later.clone())
                .expect("the run hands its application over");
        });
    }
    let run = running(&hook, note(&Lives::new()));
    let window = window_ids(&run);
    hook.load(version(2, 2));
    turns(&run);
    let reports = hook.take_outcomes();
    assert!(
        matches!(
            reports.as_slice(),
            [ScriptedOutcome::RestartFailed(failed, RestartReason::Launch(_))]
                if *failed == version(2, 2)
        ),
        "{reports:?}"
    );
    assert_eq!(
        later.created(),
        later.disposed(),
        "what the composition built was retired"
    );
    assert_eq!(later.created(), 1, "the composition mounted the later root");
    assert_eq!(window_ids(&run), window, "the window is kept");

    hook.entry(runs(note(&after)));
    hook.load(version(3, 3));
    turns(&run);
    assert_eq!(hook.take_outcomes(), [restarted(version(3, 3))]);
    assert_eq!(after.created(), 1, "a later version restarts into it");
    run.join().expect("the run ends");
}

/// §4 step 3: a panic in the new root's build shows what a build's panic
/// shows and is still a restart.
fn a_panic_in_the_new_roots_mount_is_still_a_restart() {
    let later = Lives::new();
    let hook = ScriptedReload::new();
    hook.entry(runs(Note {
        build_panics: true,
        ..note(&later)
    }));
    let run = running(&hook, note(&Lives::new()));
    hook.load(version(2, 2));
    turns(&run);
    assert_eq!(hook.take_outcomes(), [restarted(version(2, 2))]);
    assert_eq!(
        later.created(),
        1,
        "the later root was mounted, and its build panicked"
    );
    assert!(run.is_running());
    assert_eq!(run.open_windows().len(), 1);
    run.join().expect("the run ends");
}

/// §4 step 4: the exit question is answered by the later application's
/// policy.
fn a_resident_application_is_asked_to_exit_by_its_new_policy() {
    let hook = ScriptedReload::new();
    hook.entry(|| {
        Application::new()
            .config(AppConfig::new().exit_policy(ExitPolicy::OnLastWindowClosed))
            .run_resident(|_: &AppHandle| -> flui_widgets::Text {
                panic!("the later application opens no main window")
            })
            .expect("the run hands its application over");
    });
    let application = Application::new().config(development(&hook)).on_ready(|_| {
        open_secondary_window(AppConfig::new(), WindowPolicy::Isolated)
            .expect("an isolated window opens on the headless platform");
    });
    let run = HeadlessRun::start_resident(application, |_: &AppHandle| -> flui_widgets::Text {
        panic!("the earlier application opens no main window")
    });
    run.run_owner_turn();
    hook.load(version(1, 1));
    turns(&run);
    let _first = hook.take_outcomes();
    assert!(run.is_running(), "the earlier policy quits explicitly");

    hook.load(version(2, 2));
    turns(&run);
    assert_eq!(hook.take_outcomes(), [restarted(version(2, 2))]);
    assert!(
        !run.is_running(),
        "the later application, with no window, exits by its policy"
    );
    run.join().expect("the run ends");
}

/// §3: a version of another layout restarts, and a later version of the
/// restart's layout reassembles the UI runtime the restart built.
fn a_changed_layout_restarts_and_a_later_body_edit_reassembles() {
    let later = Lives::new();
    let hook = ScriptedReload::new();
    hook.entry(runs(note(&later)));
    let run = running(&hook, note(&Lives::new()));

    hook.load(version(2, 2));
    turns(&run);
    assert_eq!(
        hook.take_outcomes(),
        [restarted(version(2, 2))],
        "nothing reassembles across layouts"
    );

    hook.load(version(2, 3));
    turns(&run);
    assert_eq!(
        hook.take_outcomes(),
        [ScriptedOutcome::Reassembled(version(2, 3))],
        "a body edit of the restart's layout reassembles"
    );
    assert_eq!(later.created(), 1, "a reassemble keeps the later state");
    assert_eq!(hook.entries_run(), 1);
    run.join().expect("the run ends");
}

/// §2, §3: a restart-tier save and a body save that arrive before one poll
/// restart once, to the latest version, and the restart's UI runtime is
/// built from it, so nothing reassembles after.
fn a_restart_tier_save_then_a_body_save_restart_once() {
    let later = Lives::new();
    let hook = ScriptedReload::new();
    hook.entry(runs(note(&later)));
    let run = running(&hook, note(&Lives::new()));

    hook.load(version(2, 2));
    hook.load(version(2, 3));
    turns(&run);
    turns(&run);
    assert_eq!(hook.take_outcomes(), [restarted(version(2, 3))]);
    assert_eq!(hook.entries_run(), 1);
    assert_eq!(later.created(), 1);
    run.join().expect("the run ends");
}

/// §3: the wake of the hook that reports the version gives the loop the
/// turn that polls it and restarts: one owner turn, no frame asked for.
fn an_idle_loop_restarts_on_the_wake_that_reported_the_version() {
    let later = Lives::new();
    let hook = ScriptedReload::new();
    hook.entry(runs(note(&later)));
    let run = running(&hook, note(&Lives::new()));

    hook.load(version(2, 2));
    run.run_owner_turn();
    assert_eq!(hook.take_outcomes(), [restarted(version(2, 2))]);
    assert_eq!(later.created(), 1);
    run.join().expect("the run ends");
}

/// §3: a loop with no UI runtime polls the hook when its wake arrives, and
/// restarts on a version of another layout.
fn a_loop_without_a_ui_runtime_polls_on_the_wake() {
    let hook = ScriptedReload::new();
    let later_ready = Arc::new(AtomicBool::new(false));
    {
        let later_ready = Arc::clone(&later_ready);
        hook.entry(move || {
            let later_ready = Arc::clone(&later_ready);
            Application::new()
                .config(AppConfig::new().exit_policy(ExitPolicy::ExplicitQuit))
                .on_ready(move |_| later_ready.store(true, Ordering::SeqCst))
                .run_resident(|_: &AppHandle| -> flui_widgets::Text {
                    panic!("the later application opens no main window")
                })
                .expect("the run hands its application over");
        });
    }
    let run = HeadlessRun::start_resident(
        Application::new().config(development(&hook)),
        |_: &AppHandle| -> flui_widgets::Text {
            panic!("the earlier application opens no main window")
        },
    );
    run.run_owner_turn();
    assert!(run.open_windows().is_empty(), "no window, so no UI runtime");

    hook.load(version(1, 1));
    turns(&run);
    assert_eq!(
        hook.take_outcomes(),
        [],
        "the first version is the one the application was composed from"
    );
    hook.load(version(2, 2));
    run.run_owner_turn();
    assert_eq!(hook.take_outcomes(), [restarted(version(2, 2))]);
    assert!(
        later_ready.load(Ordering::SeqCst),
        "the later application is ready"
    );
    assert!(run.is_running());
    run.join().expect("the run ends");
}

/// A capability whose handle says which application provided it.
enum Provided {}

impl flui_platform::capability::Capability for Provided {
    type Handle = u32;
    const NAME: &'static str = "restart-provided";
}

/// A root that asks for [`Provided`] when its state is created.
#[derive(Clone)]
struct Asks(Arc<Mutex<Vec<Option<u32>>>>);

struct AsksState;

impl flui_view::StatefulView for Asks {
    type State = AsksState;

    fn create_state(&self, ctx: &dyn flui_view::LifecycleContext) -> Self::State {
        use flui_view::LifecycleContextExt as _;
        let answer = ctx.capability::<Provided>().ok();
        self.0.lock().expect("the answers lock").push(answer);
        AsksState
    }
}

impl flui_view::ViewState<Asks> for AsksState {
    fn build(&self, _: &Asks, _ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
        flui_widgets::SizedBox::square(10.0)
    }
}

impl flui_view::View for Asks {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

/// §4 step 3: the later application's capabilities are the ones its
/// widgets acquire; the earlier application's provider is gone with its
/// scope.
fn a_restart_composes_the_later_applications_capabilities() {
    let answers = Arc::new(Mutex::new(Vec::new()));
    let hook = ScriptedReload::new();
    {
        let asks = Asks(Arc::clone(&answers));
        hook.entry(move || {
            Application::new()
                .capability::<Provided>(|_: &flui_platform::capability::ProviderContext<'_>| Ok(2))
                .run(asks.clone())
                .expect("the run hands its application over");
        });
    }
    let application = Application::new()
        .config(development(&hook))
        .capability::<Provided>(|_: &flui_platform::capability::ProviderContext<'_>| Ok(1));
    let run = running_with(&hook, application, Asks(Arc::clone(&answers)));

    hook.load(version(2, 2));
    turns(&run);
    assert_eq!(hook.take_outcomes(), [restarted(version(2, 2))]);
    assert_eq!(
        *answers.lock().expect("the answers lock"),
        [Some(1), Some(2)],
        "each application's widgets acquire its own provider"
    );
    run.join().expect("the run ends");
}

const PROBE_FONT: &[u8] = include_bytes!("../../flui-painting/assets/fonts/probe-sans-400.ttf");

/// §4 step 3: the window stays, and the font registrations, which are the
/// application's, are made again by the later application.
fn a_restart_keeps_the_window_and_registers_the_fonts_again() {
    fn registers(seen: &Arc<Mutex<Vec<String>>>) -> impl FnOnce(&AppHandle) + 'static {
        let seen = Arc::clone(seen);
        move |_| {
            let outcome = flui_app::register_font(PROBE_FONT);
            seen.lock()
                .expect("the outcomes lock")
                .push(format!("{outcome:?}"));
        }
    }
    let outcomes = Arc::new(Mutex::new(Vec::new()));
    let hook = ScriptedReload::new();
    {
        let (outcomes, later) = (Arc::clone(&outcomes), note(&Lives::new()));
        hook.entry(move || {
            Application::new()
                .on_ready(registers(&outcomes))
                .run(later.clone())
                .expect("the run hands its application over");
        });
    }
    let application = Application::new()
        .config(development(&hook))
        .on_ready(registers(&outcomes));
    let run = running_with(&hook, application, note(&Lives::new()));
    let window = window_ids(&run);

    hook.load(version(2, 2));
    turns(&run);
    assert_eq!(hook.take_outcomes(), [restarted(version(2, 2))]);
    assert_eq!(window_ids(&run), window, "the restart kept the window");
    assert_eq!(
        *outcomes.lock().expect("the outcomes lock"),
        ["Ok(())", "Ok(())"],
        "the later application registers the font the earlier one did"
    );
    run.join().expect("the run ends");
}

/// §4: what the operating system opens after a restart reaches the later
/// application's callback; the earlier application's is gone with it.
fn opened_files_after_a_restart_reach_the_later_application() {
    fn hears(
        heard: &Arc<Mutex<Vec<&'static str>>>,
        who: &'static str,
    ) -> impl Fn(&[flui_platform::OpenedFile]) + 'static {
        let heard = Arc::clone(heard);
        move |files| {
            heard
                .lock()
                .expect("the heard lock")
                .extend(files.iter().map(|_| who));
        }
    }
    fn open(run: &HeadlessRunHandle) {
        run.open(
            flui_platform::Opened::bounded(
                [],
                [flui_platform::OpenedFile::new("/a.note")],
                flui_platform::OpenTarget::Application,
                flui_platform::Launch::Running,
            )
            .0,
        );
        run.run_owner_turn();
    }
    let heard = Arc::new(Mutex::new(Vec::new()));
    let hook = ScriptedReload::new();
    {
        let (heard, later) = (Arc::clone(&heard), note(&Lives::new()));
        hook.entry(move || {
            Application::new()
                .on_open_files(hears(&heard, "later"))
                .run(later.clone())
                .expect("the run hands its application over");
        });
    }
    let application = Application::new()
        .config(development(&hook))
        .on_open_files(hears(&heard, "earlier"));
    let run = running_with(&hook, application, note(&Lives::new()));
    open(&run);

    hook.load(version(2, 2));
    turns(&run);
    assert_eq!(hook.take_outcomes(), [restarted(version(2, 2))]);
    open(&run);
    assert_eq!(*heard.lock().expect("the heard lock"), ["earlier", "later"]);
    run.join().expect("the run ends");
}

/// §4: the executor pools are the loop's: a later application that
/// configures its own is composed on the loop's, and the outcome says a
/// process restart applies them.
fn a_restart_reports_the_executors_it_did_not_apply() {
    let later = Lives::new();
    let hook = ScriptedReload::new();
    {
        let later = note(&later);
        hook.entry(move || {
            Application::new()
                .config(
                    AppConfig::new()
                        .executors(flui_app::DeterministicExecutors::new().host_executors()),
                )
                .run(later.clone())
                .expect("the run hands its application over");
        });
    }
    let run = running(&hook, note(&Lives::new()));

    hook.load(version(2, 2));
    turns(&run);
    assert_eq!(
        hook.take_outcomes(),
        [ScriptedOutcome::Restarted(
            version(2, 2),
            Unapplied::default().executors(true)
        )]
    );
    assert_eq!(later.created(), 1, "composed on the loop's pools");
    run.join().expect("the run ends");
}

/// A root whose copy panics: the main window's factory copies the root, so
/// an application run with it fails to install its main window.
struct PanicsWhenCopied;

impl Clone for PanicsWhenCopied {
    fn clone(&self) -> Self {
        panic!("the later main window's factory fails")
    }
}

impl flui_view::View for PanicsWhenCopied {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl flui_view::StatelessView for PanicsWhenCopied {
    fn build(&self, _ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
        flui_widgets::SizedBox::square(10.0)
    }
}

/// §4 step 3: an install into the kept window that fails is a failed
/// restart, not a `Restarted` with no window; the next version opens a main
/// window anew.
fn a_failed_install_into_the_kept_window_fails_the_restart() {
    let after = Lives::new();
    let hook = ScriptedReload::new();
    hook.entry(|| {
        Application::new()
            .config(AppConfig::new().exit_policy(ExitPolicy::ExplicitQuit))
            .run(PanicsWhenCopied)
            .expect("the run hands its application over");
    });
    let run = running(&hook, note(&Lives::new()));

    hook.load(version(2, 2));
    turns(&run);
    let outcomes = hook.take_outcomes();
    assert!(
        matches!(
            outcomes.as_slice(),
            [ScriptedOutcome::RestartFailed(failed, RestartReason::Launch(_))]
                if *failed == version(2, 2)
        ),
        "{outcomes:?}"
    );
    assert!(run.is_running(), "a failed restart ends no loop");

    // The same edit again: the main window it opens anew fails as well,
    // which the observer hears and which ends no loop.
    hook.load(version(3, 3));
    turns(&run);
    let _second = hook.take_outcomes();
    assert!(run.is_running(), "a second failing window ends no loop");
    assert!(run.open_windows().is_empty());

    hook.entry(runs(note(&after)));
    hook.load(version(4, 4));
    turns(&run);
    assert_eq!(hook.take_outcomes(), [restarted(version(4, 4))]);
    assert_eq!(
        after.created(),
        1,
        "the next version restarts into a new window"
    );
    assert_eq!(run.open_windows().len(), 1);
    run.join().expect("the run ends");
}

#[test]
fn runner_restart() {
    crate::run_table(
        "runner_restart",
        &[
            (
                "a_failed_install_into_the_kept_window_fails_the_restart",
                a_failed_install_into_the_kept_window_fails_the_restart,
            ),
            (
                "a_restart_reports_the_executors_it_did_not_apply",
                a_restart_reports_the_executors_it_did_not_apply,
            ),
            (
                "opened_files_after_a_restart_reach_the_later_application",
                opened_files_after_a_restart_reach_the_later_application,
            ),
            (
                "a_changed_layout_restarts_and_a_later_body_edit_reassembles",
                a_changed_layout_restarts_and_a_later_body_edit_reassembles,
            ),
            (
                "a_restart_tier_save_then_a_body_save_restart_once",
                a_restart_tier_save_then_a_body_save_restart_once,
            ),
            (
                "an_idle_loop_restarts_on_the_wake_that_reported_the_version",
                an_idle_loop_restarts_on_the_wake_that_reported_the_version,
            ),
            (
                "a_loop_without_a_ui_runtime_polls_on_the_wake",
                a_loop_without_a_ui_runtime_polls_on_the_wake,
            ),
            (
                "a_restart_composes_the_later_applications_capabilities",
                a_restart_composes_the_later_applications_capabilities,
            ),
            (
                "a_restart_keeps_the_window_and_registers_the_fonts_again",
                a_restart_keeps_the_window_and_registers_the_fonts_again,
            ),
            (
                "a_restart_disposes_every_state_before_the_entry_runs",
                a_restart_disposes_every_state_before_the_entry_runs,
            ),
            (
                "a_panicking_dispose_does_not_stop_a_restart",
                a_panicking_dispose_does_not_stop_a_restart,
            ),
            (
                "a_restart_closes_the_windows_the_application_opened_without_asking",
                a_restart_closes_the_windows_the_application_opened_without_asking,
            ),
            (
                "a_restart_drops_the_earlier_scope",
                a_restart_drops_the_earlier_scope,
            ),
            (
                "an_app_handle_of_the_earlier_application_is_refused",
                an_app_handle_of_the_earlier_application_is_refused,
            ),
            (
                "a_panicking_entry_fails_the_restart_and_is_not_tried_again",
                a_panicking_entry_fails_the_restart_and_is_not_tried_again,
            ),
            (
                "an_entry_without_an_application_fails_the_restart",
                an_entry_without_an_application_fails_the_restart,
            ),
            (
                "a_second_run_in_one_entry_is_refused_and_the_first_is_composed",
                a_second_run_in_one_entry_is_refused_and_the_first_is_composed,
            ),
            (
                "a_failed_composition_retires_what_it_built",
                a_failed_composition_retires_what_it_built,
            ),
            (
                "a_panic_in_the_new_roots_mount_is_still_a_restart",
                a_panic_in_the_new_roots_mount_is_still_a_restart,
            ),
            (
                "a_resident_application_is_asked_to_exit_by_its_new_policy",
                a_resident_application_is_asked_to_exit_by_its_new_policy,
            ),
        ],
    );
}
