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

use std::cell::RefCell;
use std::rc::Rc;

use flui_app::app::{AppConfig, Application, ExitPolicy, WindowPolicy};
use flui_app::testing::HeadlessRun;
use flui_native::HeadlessPlatform;
use flui_platform::{Launch, OpenTarget, Opened, OpenedFile};
use flui_view::navigation::LinkDeclarations;
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
const DECLARES: LinkDeclarations = LinkDeclarations::new().url_schemes(&["myapp"]);

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
