//! What the operating system opens an application with reaches it through
//! the runner: a URL the application declared opens in the `Router` of the
//! window it addresses, the window focused last when it addresses the
//! application as a whole, and opened files reach `on_open_files`.
//!
//! # Scenarios
//!
//! A link that launched the application, reported before its first window
//! exists, opens the first window's Router on its page; with two windows a
//! link goes to the one focused last; an undeclared scheme opens nothing;
//! files reach the callback in order; a panic while the first of two URLs
//! navigates does not lose the second.
//!
//! With no window: a link the application declares reopens its main window,
//! whose Router opens on it, after the last window closed, in a resident run,
//! and for the link that launched one; a burst opens one window; a link it
//! does not declare opens none. A link for a window still to open never takes
//! the place of the application's, one for a window that closed is refused
//! as it arrives, and one that arrives as the application quits opens no
//! window and is refused when the run ends.

use std::cell::RefCell;
use std::rc::Rc;

use flui_app::app::{
    AppConfig, AppHandle, Application, CloseReason, CloseRequestHandler, CloseResponse, ExitPolicy,
    WindowPolicy,
};
use flui_app::testing::{HeadlessRun, HeadlessRunHandle};
use flui_native::HeadlessPlatform;
use flui_platform::{Launch, OpenTarget, Opened, OpenedFile, PlatformWindow, WindowId};
use flui_view::navigation::OpenDeclarations;
use flui_view::{CloseGuard, PendingClose};
use flui_widgets::prelude::*;
use flui_widgets::{Router, SizedBox};

#[derive(Routable, Debug, Clone, PartialEq)]
enum Page {
    #[route("/")]
    Home,
    #[route("/note/:id")]
    Note { id: u32 },
}

/// The pages each window's Router built, in order, by window name.
type Built = Rc<RefCell<Vec<(&'static str, Page)>>>;

/// The application declares its own scheme.
const DECLARES: OpenDeclarations = OpenDeclarations::new().url_schemes(&["myapp"]);

/// The page each window opens on when no link names another.
const INITIAL: Page = Page::Note { id: 9 };

/// A Router opening on [`INITIAL`] that records each page it builds as
/// `window`'s; a page is built again on each rebuild, so a test asks which
/// pages were built, not how often.
fn router(window: &'static str, built: &Built) -> Router<Page> {
    let built = Rc::clone(built);
    Router::new(INITIAL, move |page: &Page, _: &dyn BuildContext| {
        built.borrow_mut().push((window, page.clone()));
        SizedBox::new(10.0, 10.0).into_view().boxed()
    })
}

/// What the operating system opens for the application as a whole.
fn link(url: &str, launch: Launch) -> Opened {
    Opened::new([url.to_owned()], [], OpenTarget::Application, launch)
}

/// Whether `window` built `page`.
fn built_page(built: &Built, window: &str, page: &Page) -> bool {
    built
        .borrow()
        .iter()
        .any(|(name, built)| *name == window && built == page)
}

/// The windows that built `page`.
fn windows_that_built(built: &Built, page: &Page) -> Vec<&'static str> {
    let mut windows: Vec<&'static str> = built
        .borrow()
        .iter()
        .filter(|(_, built)| built == page)
        .map(|(name, _)| *name)
        .collect();
    windows.sort_unstable();
    windows.dedup();
    windows
}

pub(crate) fn a_link_that_launched_the_app_opens_its_first_router_on_it() {
    let built = Built::default();
    let platform = HeadlessPlatform::new();
    // Reported before the run binds its handler, as AppKit reports
    // `application:openURLs:` before `applicationDidFinishLaunching:`.
    platform
        .host_events()
        .open(link("myapp:///note/3", Launch::Cold));
    let run = HeadlessRun::start_on(
        Application::new().declares(DECLARES),
        router("main", &built),
        platform,
    );
    run.run_owner_turn();
    run.run_owner_turn();
    assert!(
        built_page(&built, "main", &Page::Note { id: 3 }),
        "the link's page is built: {:?}",
        built.borrow()
    );
    assert!(
        !built_page(&built, "main", &INITIAL),
        "the first Router opens on the link, never on its initial page: {:?}",
        built.borrow()
    );
    drop(run);
}

pub(crate) fn a_link_goes_to_the_window_focused_last() {
    let built = Built::default();
    let secondary = router("secondary", &built);
    let application = Application::new()
        .declares(DECLARES)
        .config(AppConfig::new().exit_policy(ExitPolicy::OnLastWindowClosed))
        .on_ready(move |_| {
            drop(
                flui_app::open_window(AppConfig::new(), WindowPolicy::Isolated, secondary)
                    .expect("an isolated window opens on the headless platform"),
            );
        });
    let run = HeadlessRun::start(application, router("main", &built));
    run.run_owner_turn();
    let windows = run.open_windows();
    assert_eq!(windows.len(), 2, "both windows are open");

    // Which headless window is which is the platform's order: the window
    // that built the link's page names the one it reached.
    for window in &windows {
        window.simulate_focus(false);
    }
    run.run_owner_turn();
    let mut reached = Vec::new();
    for (window, id) in windows.iter().zip([1, 2]) {
        window.simulate_focus(true);
        run.run_owner_turn();
        run.open(link(&format!("myapp:///note/{id}"), Launch::Running));
        run.run_owner_turn();
        let page = Page::Note { id };
        let built_it = windows_that_built(&built, &page);
        assert_eq!(
            built_it.len(),
            1,
            "one window opened {page:?}: {built_it:?}"
        );
        reached.extend(built_it);
        window.simulate_focus(false);
        run.run_owner_turn();
    }
    reached.sort_unstable();
    assert_eq!(
        reached,
        ["main", "secondary"],
        "each link reached the window focused last"
    );
    drop(run);
}

pub(crate) fn an_undeclared_scheme_opens_nothing() {
    let built = Built::default();
    let run = HeadlessRun::start(
        Application::new().declares(DECLARES),
        router("main", &built),
    );
    run.run_owner_turn();
    run.open(link("other:///note/3", Launch::Running));
    run.open(link("https://example.com/note/3", Launch::Running));
    run.run_owner_turn();
    assert!(
        !built_page(&built, "main", &Page::Note { id: 3 }),
        "the stack is unchanged"
    );
    drop(run);
}

pub(crate) fn opened_files_reach_on_open_files() {
    let built = Built::default();
    let heard = Rc::new(RefCell::new(Vec::new()));
    let files = Rc::clone(&heard);
    let application = Application::new().on_open_files(move |opened: &[OpenedFile]| {
        files
            .borrow_mut()
            .extend(opened.iter().map(|file| file.path.clone()));
    });
    let run = HeadlessRun::start(application, router("main", &built));
    run.run_owner_turn();
    run.open(Opened::new(
        [],
        ["/a.note", "/b.note"].map(OpenedFile::new),
        OpenTarget::Application,
        Launch::Running,
    ));
    run.run_owner_turn();
    assert_eq!(
        *heard.borrow(),
        [
            std::path::PathBuf::from("/a.note"),
            std::path::PathBuf::from("/b.note")
        ]
    );
    drop(run);
}

pub(crate) fn a_panic_in_the_first_urls_navigation_does_not_lose_the_second() {
    let built = Built::default();
    let main = router("main", &built).on_unknown(|_| panic!("the unknown-route hook panics"));
    let run = HeadlessRun::start(Application::new().declares(DECLARES), main);
    run.run_owner_turn();
    run.open(Opened::new(
        ["myapp:///nowhere".to_owned(), "myapp:///note/2".to_owned()],
        [],
        OpenTarget::Application,
        Launch::Running,
    ));
    run.run_owner_turn();
    assert!(
        built_page(&built, "main", &Page::Note { id: 2 }),
        "the second link opened: {:?}",
        built.borrow()
    );
    drop(run);
}

/// Whether a run started with `application` opens `url` in its window.
fn opens(application: Application, url: &str) -> bool {
    let built = Built::default();
    let run = HeadlessRun::start(application, router("main", &built));
    run.run_owner_turn();
    run.open(link(url, Launch::Running));
    run.run_owner_turn();
    let opened = built_page(&built, "main", &Page::Note { id: 3 });
    drop(run);
    opened
}

pub(crate) fn the_entry_functions_manifest_declares_what_a_run_opens() {
    use flui_app::app::__entry::{
        FileTypeDeclaration, OpenDeclarations, declare, join, joined_len,
    };

    // What `#[flui::main]` writes at the top of the function it marks for a
    // manifest declaring `url-schemes = ["myapp"]`, a `file-types` entry, and
    // a scheme under each of two features: this test crate is compiled with
    // `unstable-windows` and without `driver`.
    const URL_SCHEME_PARTS: &[&[&str]] = &[
        &["myapp"],
        {
            const PART: &[&str] = &["myapp-windows"];
            if cfg!(feature = "unstable-windows") {
                PART
            } else {
                &[]
            }
        },
        {
            const PART: &[&str] = &["myapp-driver"];
            if cfg!(feature = "driver") { PART } else { &[] }
        },
    ];
    const URL_SCHEMES: &[&str] =
        &join::<&str, { joined_len(URL_SCHEME_PARTS) }>(URL_SCHEME_PARTS, "");
    const NOTE: FileTypeDeclaration =
        FileTypeDeclaration::new("note", "application/x-myapp-note", "Note");
    const FILE_TYPE_PARTS: &[&[FileTypeDeclaration]] = &[&[NOTE]];
    const FILE_TYPES: &[FileTypeDeclaration] =
        &join::<FileTypeDeclaration, { joined_len(FILE_TYPE_PARTS) }>(
            FILE_TYPE_PARTS,
            FileTypeDeclaration::new("", "", ""),
        );
    assert_eq!(FILE_TYPES, [NOTE], "the file types join as the schemes do");
    let declared = declare(
        OpenDeclarations::new()
            .url_schemes(URL_SCHEMES)
            .file_types(FILE_TYPES),
    );
    assert!(
        opens(Application::new(), "myapp:///note/3"),
        "a run the entry starts opens what its manifest declares"
    );
    assert_eq!(
        (
            opens(Application::new(), "myapp-windows:///note/3"),
            opens(Application::new(), "myapp-driver:///note/3"),
        ),
        (cfg!(feature = "unstable-windows"), cfg!(feature = "driver")),
        "a feature's scheme counts only when the crate is compiled with the feature"
    );
    let own = OpenDeclarations::new().url_schemes(&["other"]);
    assert!(
        !opens(Application::new().declares(own), "myapp:///note/3"),
        "an application's own declarations replace the manifest's"
    );
    assert!(opens(Application::new().declares(own), "other:///note/3"));
    drop(declared);
    assert!(
        !opens(Application::new(), "myapp:///note/3"),
        "once the entry returned, a run declares nothing"
    );
}

/// A resident run of `application` that declares [`DECLARES`] and keeps
/// running with no window, whose main windows are [`router`]s named `main`.
fn resident(built: &Built, application: Application) -> HeadlessRunHandle {
    let built = Rc::clone(built);
    HeadlessRun::start_resident(
        application
            .declares(DECLARES)
            .config(AppConfig::new().exit_policy(ExitPolicy::ExplicitQuit)),
        move |_: &AppHandle| router("main", &built),
    )
}

/// Run `count` owner turns.
fn turns(run: &HeadlessRunHandle, count: usize) {
    for _ in 0..count {
        run.run_owner_turn();
    }
}

pub(crate) fn a_link_with_no_window_reopens_the_main_window_on_it() {
    let built = Built::default();
    let run = resident(&built, Application::new());
    turns(&run, 2);
    assert!(
        run.open_windows().is_empty(),
        "a resident run starts with no window"
    );

    run.open(link("myapp:///note/3", Launch::Running));
    turns(&run, 4);
    assert_eq!(
        run.open_windows().len(),
        1,
        "the link opened the main window"
    );
    assert!(
        built_page(&built, "main", &Page::Note { id: 3 }),
        "the window opened on the link: {:?}",
        built.borrow()
    );
    assert!(
        !built_page(&built, "main", &INITIAL),
        "the reopened window's Router opens on the link, never on its initial page: {:?}",
        built.borrow()
    );

    // The last window closes and the application keeps running, as a macOS
    // application does: the next link opens a window again.
    for window in run.open_windows() {
        assert!(window.simulate_close(), "the user closes the window");
    }
    turns(&run, 3);
    assert!(run.open_windows().is_empty(), "the window closed");
    assert!(run.is_running(), "an explicit quit keeps the run alive");
    run.open(link("myapp:///note/4", Launch::Running));
    turns(&run, 4);
    assert_eq!(run.open_windows().len(), 1, "the next link opened a window");
    assert!(
        built_page(&built, "main", &Page::Note { id: 4 }),
        "the window opened on the next link: {:?}",
        built.borrow()
    );
    drop(run);
}

pub(crate) fn a_link_that_launched_a_resident_app_opens_its_main_window_on_it() {
    let built = Built::default();
    let platform = HeadlessPlatform::new();
    platform
        .host_events()
        .open(link("myapp:///note/3", Launch::Cold));
    let factory_built = Rc::clone(&built);
    let run = HeadlessRun::start_resident_on(
        Application::new()
            .declares(DECLARES)
            .config(AppConfig::new().exit_policy(ExitPolicy::ExplicitQuit)),
        move |_: &AppHandle| router("main", &factory_built),
        platform,
    );
    turns(&run, 4);
    assert_eq!(
        run.open_windows().len(),
        1,
        "the link opened the main window"
    );
    assert!(
        built_page(&built, "main", &Page::Note { id: 3 }),
        "the window opened on the link: {:?}",
        built.borrow()
    );
    assert!(
        !built_page(&built, "main", &INITIAL),
        "its Router opens on the link: {:?}",
        built.borrow()
    );
    drop(run);
}

pub(crate) fn a_link_no_declaration_admits_opens_no_window() {
    let built = Built::default();
    let run = resident(&built, Application::new());
    turns(&run, 2);
    run.open(link("other:///note/3", Launch::Running));
    run.open(link("https://example.com/note/3", Launch::Running));
    run.open(link("not a url", Launch::Running));
    turns(&run, 4);
    assert!(
        run.open_windows().is_empty(),
        "a link the application does not open shows no window"
    );
    assert!(built.borrow().is_empty(), "nothing was built");
    drop(run);
}

pub(crate) fn a_burst_of_links_with_no_window_opens_one_window_on_the_last() {
    let built = Built::default();
    let run = resident(&built, Application::new());
    turns(&run, 2);
    // Nine links near the longest an opened URL may be, all before the
    // window can open: the window opens on the last whatever came before.
    let host = "a".repeat(8000);
    for id in 21..=29 {
        run.open(link(&format!("myapp://{host}/note/{id}"), Launch::Running));
    }
    turns(&run, 4);
    assert_eq!(run.open_windows().len(), 1, "one window for the burst");
    assert!(
        built_page(&built, "main", &Page::Note { id: 29 }),
        "the window shows the last link: {:?}",
        built.borrow()
    );
    assert!(
        !built_page(&built, "main", &INITIAL),
        "never its initial page: {:?}",
        built.borrow()
    );
    drop(run);
}

pub(crate) fn a_link_for_a_window_nobody_opens_is_refused_and_leaves_the_applications_link() {
    flui_testing::disarm_interest_cache();
    let built = Built::default();
    let handle = Rc::new(RefCell::new(None));
    let ready = Rc::clone(&handle);
    let run = resident(
        &built,
        Application::new().on_ready(move |app: &AppHandle| {
            *ready.borrow_mut() = Some(app.clone());
        }),
    );
    turns(&run, 2);
    // A window the platform names that no installation in this run opens.
    let elsewhere = WindowId::try_from_raw(999).expect("a nonzero id");
    let ((), log) = flui_testing::capture(|| {
        run.open(link("myapp:///note/3", Launch::Running));
        run.open(Opened::new(
            ["myapp:///note/4".to_owned()],
            [],
            OpenTarget::Window(elsewhere),
            Launch::Running,
        ));
        run.run_owner_turn();
    });
    assert_eq!(
        log.count_containing("its window is not open"),
        1,
        "the link for a window nobody opens is refused as it arrives: {log}"
    );
    // The application shows its main window itself, as it may on any turn.
    let app = handle.borrow_mut().take().expect("on_ready ran");
    drop(
        app.request_show_main_window()
            .expect("the run accepts the show"),
    );
    turns(&run, 4);
    assert_eq!(run.open_windows().len(), 1, "one main window opened");
    assert!(
        built_page(&built, "main", &Page::Note { id: 3 }),
        "the application's link opened in its first window: {:?}",
        built.borrow()
    );
    assert!(
        !built_page(&built, "main", &Page::Note { id: 4 }),
        "another window's link never opens here: {:?}",
        built.borrow()
    );
    drop(run);
}

pub(crate) fn a_link_for_a_closed_window_is_refused_as_it_arrives() {
    flui_testing::disarm_interest_cache();
    let built = Built::default();
    let secondary = router("secondary", &built);
    let application = Application::new()
        .declares(DECLARES)
        .config(AppConfig::new().exit_policy(ExitPolicy::ExplicitQuit))
        .on_ready(move |_| {
            drop(
                flui_app::open_window(AppConfig::new(), WindowPolicy::Isolated, secondary)
                    .expect("an isolated window opens on the headless platform"),
            );
        });
    let run = HeadlessRun::start(application, router("main", &built));
    turns(&run, 2);
    let windows = run.open_windows();
    assert_eq!(windows.len(), 2, "both windows are open");
    let closed = PlatformWindow::id(&windows[1]);
    assert!(windows[1].simulate_close(), "the user closes one window");
    turns(&run, 3);
    assert_eq!(run.open_windows().len(), 1, "one window is left");

    let ((), log) = flui_testing::capture(|| {
        run.open(Opened::new(
            ["myapp:///note/5".to_owned()],
            [],
            OpenTarget::Window(closed),
            Launch::Running,
        ));
        turns(&run, 3);
        drop(run);
    });
    assert_eq!(
        log.count_containing("its window closed"),
        1,
        "the link is refused as it arrives: {log}"
    );
    assert_eq!(
        log.count_containing("the run ended"),
        0,
        "nothing waits for the closed window until the run ends: {log}"
    );
    assert!(
        !built_page(&built, "main", &Page::Note { id: 5 }),
        "the window left never opens another window's link: {:?}",
        built.borrow()
    );
}

pub(crate) fn a_link_after_the_quit_opens_no_window_and_is_refused_at_the_end() {
    flui_testing::disarm_interest_cache();
    let built = Built::default();
    let handle = Rc::new(RefCell::new(None));
    let ready = Rc::clone(&handle);
    let run = resident(
        &built,
        Application::new().on_ready(move |app: &AppHandle| {
            *ready.borrow_mut() = Some(app.clone());
        }),
    );
    turns(&run, 2);
    let app = handle.borrow_mut().take().expect("on_ready ran");
    let ((), log) = flui_testing::capture(|| {
        app.request_quit().expect("the run accepts its quit");
        run.open(link("myapp:///note/3", Launch::Running));
        turns(&run, 4);
        assert!(
            run.open_windows().is_empty(),
            "a link that arrives as the application quits opens no window"
        );
        drop(run);
    });
    assert_eq!(
        log.count_containing("the run ended"),
        1,
        "the link is refused when the run ends: {log}"
    );
    assert!(built.borrow().is_empty(), "nothing was built");
}

pub(crate) fn a_panic_in_on_open_files_ends_the_run_and_refuses_the_link() {
    flui_testing::disarm_interest_cache();
    let built = Built::default();
    let run = resident(
        &built,
        Application::new().on_open_files(|_: &[OpenedFile]| panic!("the files callback panics")),
    );
    turns(&run, 2);
    let ((), log) = flui_testing::capture(|| {
        run.open(Opened::new(
            ["myapp:///note/3".to_owned()],
            [OpenedFile::new("/a.note")],
            OpenTarget::Application,
            Launch::Running,
        ));
        turns(&run, 4);
        assert!(
            !run.is_running(),
            "the callback's panic ends the run (ADR-0258 §4)"
        );
        assert!(run.open_windows().is_empty(), "no window opens as it ends");
        let report = run.join().expect("the run reports its end");
        assert!(
            matches!(&report.exit, flui_app::testing::RunExit::Panicked { message } if message == "the files callback panics"),
            "the run ends with the callback's panic: {:?}",
            report.exit
        );
    });
    assert_eq!(
        log.count_containing("the run ended"),
        1,
        "the link that came with the files is refused as the run ends, not left waiting: {log}"
    );
}

pub(crate) fn a_link_outlives_a_window_that_failed_to_install() {
    let built = Built::default();
    let handle = Rc::new(RefCell::new(None));
    let ready = Rc::clone(&handle);
    let run = HeadlessRun::start(
        Application::new()
            .declares(DECLARES)
            .config(AppConfig::new().exit_policy(ExitPolicy::ExplicitQuit))
            .on_ready(move |app: &AppHandle| {
                *ready.borrow_mut() = Some(app.clone());
            }),
        router("main", &built),
    );
    turns(&run, 2);
    let window = run.open_windows().pop().expect("the main window opened");
    let renderer = run.renderer(&window).expect("it renders with a stand-in");
    assert!(window.simulate_close(), "the user closes it");
    turns(&run, 3);
    assert!(run.open_windows().is_empty(), "no window is left");

    // The window the link reopens is refused its GPU slot after the link
    // was opened into its runtime.
    renderer.refuse_next_window();
    run.open(link("myapp:///note/3", Launch::Running));
    turns(&run, 4);
    assert!(
        run.open_windows().is_empty(),
        "the window the GPU refused did not open"
    );

    let app = handle.borrow_mut().take().expect("on_ready ran");
    drop(
        app.request_show_main_window()
            .expect("the run accepts the show"),
    );
    turns(&run, 4);
    assert_eq!(run.open_windows().len(), 1, "the next main window opened");
    assert!(
        built_page(&built, "main", &Page::Note { id: 3 }),
        "the link came back from the failed window and opened in the next: {:?}",
        built.borrow()
    );
    drop(run);
}

/// A root that hands out its window's close guard.
#[derive(Clone, StatefulView)]
struct Guarded {
    guard: Rc<RefCell<Option<CloseGuard>>>,
}

struct GuardedState {
    guard: Rc<RefCell<Option<CloseGuard>>>,
}

impl StatefulView for Guarded {
    type State = GuardedState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        GuardedState {
            guard: Rc::clone(&self.guard),
        }
    }
}

impl ViewState<Guarded> for GuardedState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.guard.replace(ctx.close_guard());
    }

    fn build(&self, _view: &Guarded, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

/// A resident run with no main window, whose only window keeps itself open
/// for a quit, so the quit is deferred on its recorded close; the user then
/// clicks the application's Dock icon. `resolve` decides the recorded close.
fn reopen_during_a_deferred_quit(resolve: impl FnOnce(PendingClose)) -> (HeadlessRunHandle, Built) {
    let built = Built::default();
    let guard = Rc::new(RefCell::new(None));
    let handle = Rc::new(RefCell::new(None));
    let ready = Rc::clone(&handle);
    let root = Guarded {
        guard: Rc::clone(&guard),
    };
    let platform = HeadlessPlatform::new();
    let events = platform.host_events();
    let factory_built = Rc::clone(&built);
    let run = HeadlessRun::start_resident_on(
        Application::new()
            .declares(DECLARES)
            .config(AppConfig::new().exit_policy(ExitPolicy::ExplicitQuit))
            .on_ready(move |app: &AppHandle| {
                *ready.borrow_mut() = Some(app.clone());
                let keeps_open_for_a_quit = CloseRequestHandler::new(|request| {
                    if request.reason() == CloseReason::Program {
                        CloseResponse::KeepOpen
                    } else {
                        CloseResponse::Close
                    }
                });
                drop(
                    flui_app::open_window(
                        AppConfig::new().close_request_handler(keeps_open_for_a_quit),
                        WindowPolicy::Isolated,
                        root,
                    )
                    .expect("an isolated window opens on the headless platform"),
                );
            }),
        move |_: &AppHandle| router("main", &factory_built),
        platform,
    );
    turns(&run, 2);
    assert_eq!(run.open_windows().len(), 1, "the guarded window is open");
    let app = handle.borrow_mut().take().expect("on_ready ran");
    app.request_quit().expect("the run accepts its quit");
    turns(&run, 2);
    let pending = guard
        .borrow()
        .as_ref()
        .expect("the guarded root mounted")
        .pending()
        .expect("the quit waits on the recorded close");
    events.reopen();
    turns(&run, 3);
    assert_eq!(
        run.open_windows().len(),
        1,
        "no main window opens while the quit is deferred"
    );
    resolve(pending);
    turns(&run, 4);
    (run, built)
}

pub(crate) fn a_reopen_during_a_deferred_quit_opens_when_the_quit_is_abandoned() {
    let (run, built) = reopen_during_a_deferred_quit(|pending| {
        pending.stay_open().expect("the record is the current one");
    });
    assert!(run.is_running(), "the abandoned quit leaves the run going");
    assert_eq!(
        run.open_windows().len(),
        2,
        "the main window the reopen asked for opened"
    );
    assert!(built_page(&built, "main", &INITIAL), "{:?}", built.borrow());
    drop(run);
}

pub(crate) fn a_reopen_during_a_deferred_quit_is_dropped_when_the_quit_goes_ahead() {
    let (run, built) = reopen_during_a_deferred_quit(|pending| {
        pending
            .discard_and_close()
            .expect("the record is the current one");
    });
    assert!(!run.is_running(), "the quit went ahead");
    assert!(built.borrow().is_empty(), "no main window was built");
    drop(run);
}
