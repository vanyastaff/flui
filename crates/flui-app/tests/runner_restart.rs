//! The development restart (ADR-0393 §4): a version of another layout
//! retires the running application, runs its entry again and composes what
//! the entry hands over into the window the loop kept. Each row runs a whole
//! application on the headless platform with the testing kit's scripted
//! hook.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_app::testing::{HeadlessRun, HeadlessRunHandle, ReloadReport, ScriptedReload};
use flui_app::{AppConfig, Application, ExitPolicy};
use flui_view::dev_reload::{CodeVersion, LayoutEpoch, Revision};

fn version(layout: u64, revision: u64) -> CodeVersion {
    CodeVersion::new(
        LayoutEpoch::try_new(layout).expect("a test layout is not zero"),
        Revision::try_new(revision).expect("a test revision is not zero"),
    )
}

/// How many states of one [`Note`] were created and disposed.
#[derive(Default)]
struct Lives {
    created: AtomicUsize,
    disposed: AtomicUsize,
}

impl Lives {
    fn created(&self) -> usize {
        self.created.load(Ordering::SeqCst)
    }

    fn disposed(&self) -> usize {
        self.disposed.load(Ordering::SeqCst)
    }
}

/// A stateful root that counts its states' lives.
#[derive(Clone)]
struct Note {
    lives: Arc<Lives>,
}

struct NoteState {
    lives: Arc<Lives>,
}

impl flui_view::StatefulView for Note {
    type State = NoteState;

    fn create_state(&self, _ctx: &dyn flui_view::LifecycleContext) -> Self::State {
        self.lives.created.fetch_add(1, Ordering::SeqCst);
        NoteState {
            lives: Arc::clone(&self.lives),
        }
    }
}

impl flui_view::ViewState<Note> for NoteState {
    fn build(&self, _view: &Note, _ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
        flui_widgets::SizedBox::square(10.0)
    }

    fn dispose(&mut self) {
        self.lives.disposed.fetch_add(1, Ordering::SeqCst);
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
    }
}

/// A run of `first` with `hook` installed, its window open and on the first
/// version.
fn running(hook: &ScriptedReload, first: Note) -> HeadlessRunHandle {
    let application = Application::new().config(
        AppConfig::new()
            .exit_policy(ExitPolicy::ExplicitQuit)
            .dev_reload(hook.clone()),
    );
    let run = HeadlessRun::start(application, first);
    run.run_owner_turn();
    hook.load(version(1, 1));
    turns(&run);
    assert_eq!(
        hook.take_reports(),
        [ReloadReport::Reassembled(version(1, 1))],
        "the first version is the one the application was composed from, which its UI \
         runtime, on no known revision yet, reassembles to once"
    );
    run
}

fn turns(run: &HeadlessRunHandle) {
    for _ in 0..4 {
        run.run_owner_turn();
    }
}

/// §4 steps 1 to 4: the earlier application's states are disposed before
/// its entry runs, the entry's application is composed into the same
/// window, and the hook hears `Restarted`.
fn a_restart_disposes_every_state_before_the_entry_runs() {
    let (first, later) = (Arc::new(Lives::default()), Arc::new(Lives::default()));
    let hook = ScriptedReload::new();
    let disposed_when_entered = Arc::new(AtomicUsize::new(usize::MAX));
    {
        let (first, later, seen) = (
            Arc::clone(&first),
            Arc::clone(&later),
            Arc::clone(&disposed_when_entered),
        );
        hook.entry(move || {
            seen.store(first.disposed(), Ordering::SeqCst);
            Application::new()
                .run(note(&later))
                .expect("a run inside a restart hands its application over");
        });
    }
    let run = running(&hook, note(&first));
    let window = run.open_windows();
    assert_eq!(window.len(), 1, "the main window is open");

    hook.load(version(2, 2));
    turns(&run);

    assert_eq!(
        hook.take_reports(),
        [ReloadReport::Restarted(version(2, 2))]
    );
    assert_eq!(hook.entries_run(), 1);
    assert_eq!(
        disposed_when_entered.load(Ordering::SeqCst),
        1,
        "the earlier state was disposed before the entry ran"
    );
    assert_eq!(later.created(), 1, "the later application was composed");
    let kept = run.open_windows();
    assert_eq!(kept.len(), 1, "one window, the kept one");
    assert_eq!(
        flui_platform::PlatformWindow::id(&kept[0]),
        flui_platform::PlatformWindow::id(&window[0]),
        "the restart kept the window"
    );

    hook.load(version(2, 3));
    turns(&run);
    assert_eq!(
        hook.take_reports(),
        [ReloadReport::Reassembled(version(2, 3))],
        "a body edit of the restart's layout reassembles"
    );
    run.join().expect("the run ends");
    assert_eq!(
        later.disposed(),
        1,
        "the later application ends with the run"
    );
}

#[test]
fn runner_restart() {
    crate::run_table(
        "runner_restart",
        &[(
            "a_restart_disposes_every_state_before_the_entry_runs",
            a_restart_disposes_every_state_before_the_entry_runs,
        )],
    );
}
