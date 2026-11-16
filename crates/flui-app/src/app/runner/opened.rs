//! What the operating system opens the application with, routed to the
//! application on every host and the headless one (ADR-0339): files to its
//! [`on_open_files`](crate::Application::on_open_files) callback, URLs it
//! declares to the presentation they address, where the UI runtime opens
//! each one in that presentation's `Router` (`UiRuntime::open_url`).
//!
//! The presentation is the window's own for [`OpenTarget::Window`] (an iOS
//! scene), and for the application as a whole the main window's
//! ([`destination`], the one authority the routing step, the loans and the
//! reopen ask). A URL is admitted here first, so one the application does
//! not open is refused before it is held or can make a window open. URLs
//! that arrive before their presentation exists (a link that launched the
//! application, one that came while it had no window) are held by their
//! target's rule ([`HeldUrls`]) and opened in its UI runtime as it is
//! installed, ahead of its first frame, so its first `Router` opens on the
//! link instead of its initial page; for the application as a whole the run
//! opens its main window for them ([`OpenRouting::route_held`]).
//!
//! What waits is routed from the owner's turn, which the routing asks for
//! through the owner's proxy it keeps ([`OpenRouting::bind_owner`]), on every
//! runner and from any borrow: once asked, the next turn runs the routing
//! step, with or without a window.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use flui_foundation::PresentationAddress;
use flui_foundation::panic::FirstPanic;
use flui_native::PlatformProxy;
use flui_platform::{OpenTarget, Opened, OpenedFile, PlatformWindow};
use flui_runtime::ui_runtime::UiRuntime;
use flui_view::navigation::{OpenDeclarations, OpenedUrl};

use super::host::{APP_RUNTIME, MainWindow, ReopenAnswer};
use crate::app::window_registry::{OpenDestination, WindowRegistry};

/// The callback an application hears its opened files with.
pub(crate) type OpenFilesCallback = Rc<dyn Fn(&[OpenedFile])>;

/// What an [`Application`](crate::Application) set about what the operating
/// system opens it with.
#[derive(Clone, Default)]
pub(crate) struct OpenSettings {
    /// The URL schemes and domains it opens.
    pub(crate) declarations: Option<OpenDeclarations>,
    /// Where its opened files go.
    pub(crate) on_open_files: Option<OpenFilesCallback>,
    /// Whether the application runs as one instance; `None` leaves it to
    /// the declarations.
    #[cfg(feature = "single-instance")]
    pub(crate) single_instance: Option<bool>,
    /// Whether an unpackaged Windows run registers its URL schemes; `None`
    /// leaves it to the build (`associations`).
    pub(crate) register_associations: Option<bool>,
}

impl OpenSettings {
    /// These settings, declaring what the entry function `#[flui::main]`
    /// marks declares when the application gave no
    /// [`declares`](crate::Application::declares): what its crate's
    /// `[package.metadata.flui.declares]` registers with the operating system.
    /// Read as the run starts, while the entry function that starts it runs.
    pub(crate) fn or_entry_declarations(mut self) -> Self {
        if self.declarations.is_none() {
            self.declarations = super::entry_declarations();
        }
        self
    }

    /// The application id the run keeps one instance under: when the
    /// application asked for one instance, or, without
    /// [`single_instance`](crate::Application::single_instance), declared a
    /// URL scheme, whose links a second launch would otherwise open in a
    /// second process. `None` without an application id in `identity`,
    /// which is warned about: the bus name and the pipe are named by it.
    #[cfg(all(
        feature = "single-instance",
        not(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))
    ))]
    pub(crate) fn single_instance_id(&self, identity: &flui_log::AppIdentity) -> Option<String> {
        // The Linux and Windows hosts keep one instance; macOS, iOS and
        // Android keep one themselves, and each page on the Web is an
        // application of its own.
        let kept_here = cfg!(any(target_os = "linux", windows));
        if !kept_here || !self.single_instance.unwrap_or(self.declares_a_launch()) {
            return None;
        }
        let app_id = identity.app_id().map(|app_id| app_id.as_str().to_owned());
        if app_id.is_none() {
            tracing::warn!(
                "one instance needs an application id (`AppConfig::application_identity`): \
                 the application runs as many instances as it is launched"
            );
        }
        app_id
    }

    /// None: a build without `single-instance` keeps no instance. An
    /// application that declares a URL scheme, whose links a second launch
    /// then opens in a second process, is warned about on the hosts that
    /// would keep one (`flui build` refuses such a build of its project).
    #[cfg(all(
        not(feature = "single-instance"),
        not(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))
    ))]
    pub(crate) fn single_instance_id(&self, _identity: &flui_log::AppIdentity) -> Option<String> {
        if cfg!(any(target_os = "linux", windows)) && self.declares_a_launch() {
            tracing::warn!(
                "the application declares a URL scheme or a file type but is built without flui's \
                 `single-instance` feature: each link or file it is launched with opens a second \
                 process"
            );
        }
        None
    }

    /// Whether the application declares a URL scheme or a file type: what
    /// the system launches it with again while it runs.
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    fn declares_a_launch(&self) -> bool {
        self.declarations.is_some_and(|declarations| {
            !declarations.declared_url_schemes().is_empty()
                || !declarations.declared_file_types().is_empty()
        })
    }
}

impl std::fmt::Debug for OpenSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("OpenSettings");
        debug
            .field("declarations", &self.declarations)
            .field("on_open_files", &self.on_open_files.is_some());
        #[cfg(feature = "single-instance")]
        debug.field("single_instance", &self.single_instance);
        debug.field("register_associations", &self.register_associations);
        debug.finish()
    }
}

/// One run's routing of what the operating system opened: the application's
/// settings, the main window being installed, the URLs held for a presentation
/// that does not exist yet and those an installation gave back. Owned by the
/// run's application handler, so a run never sees another's.
///
/// Every opened URL ends one way: it is opened in a presentation's UI
/// runtime, held for a presentation still to come ([`HeldUrls`]), or refused
/// with a [`Refusal`] that is logged by its reason. URLs opened into a window
/// whose installation has not committed are on loan to it ([`OpenedLoan`]);
/// a loan that ends without the commit returns them, and the owner's next
/// turn routes what came back ([`route_owed`](Self::route_owed)), once: an
/// open window takes them, or they are refused. A URL causes one opening of
/// the main window at most.
pub(crate) struct OpenRouting {
    settings: OpenSettings,
    /// The presentation of the main window whose UI runtime was built last:
    /// the installation a URL for the application is bound to while the
    /// main window is opening.
    main_installing: Cell<Option<PresentationAddress>>,
    /// URLs admitted for a presentation that does not exist yet.
    held: RefCell<HeldUrls>,
    /// URLs a loan returned, with how it ended, for the owner's next turn.
    returned: RefCell<Vec<(HeldUrl, LoanEnd)>>,
    /// The order the next admitted URL gets.
    next_order: Cell<u64>,
    /// The order of the newest URL admitted for the application as a whole:
    /// one coming back from a loan older than it is superseded.
    newest_for_application: Cell<Option<u64>>,
    /// The origin of the page the run is in on the web, whose URLs every UI
    /// runtime of the run admits beside the declarations.
    page_origin: RefCell<Option<PageOrigin>>,
    /// The run's owner, which a loan's end asks for a turn: set as the run
    /// starts (`AppHandler::ready`), before any URL is lent.
    owner: OnceCell<PlatformProxy>,
}

/// The origin of the page a web run is in: as the platform said it, which
/// each UI runtime is given, and parsed, which the runner admits by; `None`
/// when it is not a URL, and then only the declarations admit.
struct PageOrigin {
    text: String,
    parsed: Option<OpenedUrl>,
}

/// An admitted URL, the target it was addressed to and its place in the
/// order URLs were admitted.
struct HeldUrl {
    target: OpenTarget,
    url: String,
    order: u64,
    /// The opening of the main window this URL is bound to, by the count of
    /// failed openings when it was bound (`host::main_window_failures`):
    /// once that count grows the opening failed and the URL is refused. A
    /// URL causes one opening at most.
    bound: Option<u64>,
}

/// How a loan ended without its installation committing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoanEnd {
    /// The installation failed, or ended before it was prepared.
    Failed,
    /// The window was closed before the installation committed.
    WindowClosed,
    /// Opening one of the loan's URLs into the window panicked, which fails
    /// the installation: the URLs are refused for that panic.
    NavigationPanicked,
}

impl OpenRouting {
    pub(crate) fn new(settings: OpenSettings) -> Self {
        Self {
            settings,
            main_installing: Cell::new(None),
            held: RefCell::new(HeldUrls::default()),
            returned: RefCell::new(Vec::new()),
            next_order: Cell::new(0),
            newest_for_application: Cell::new(None),
            page_origin: RefCell::new(None),
            owner: OnceCell::new(),
        }
    }

    /// The run's owner, through whose proxy every loan's end asks for the
    /// turn that routes what it returned: from any borrow, destructor or
    /// thread state, on every runner. Set once, as the run starts.
    pub(crate) fn bind_owner(&self, proxy: PlatformProxy) {
        assert!(
            self.owner.set(proxy).is_ok(),
            "BUG: a run's routing is bound to its owner once"
        );
    }

    /// Ask the owner for a turn: the next one runs the routing step
    /// ([`route_owed`](Self::route_owed)). Runs no code it does not own, so
    /// it is safe from a destructor; a refused wake means the owner is
    /// ending, and the run's end refuses what still waits, so it is not
    /// logged here.
    fn request_turn(&self) {
        if let Some(owner) = self.owner.get() {
            let _ = owner.wake();
        }
    }

    /// Ask the owner for a turn while anything waits, or while the router is
    /// mid-move and cannot say.
    pub(crate) fn request_turn_if_owed(&self) {
        let returned = self
            .returned
            .try_borrow()
            .map_or(true, |returned| !returned.is_empty());
        let held = self.held.try_borrow().map_or(true, |held| !held.is_empty());
        if returned || held {
            self.request_turn();
        }
    }

    /// The URL schemes and domains the application opens, which every UI
    /// runtime of the run is given before its first presentation opens.
    pub(crate) fn declarations(&self) -> OpenDeclarations {
        self.settings.declarations.unwrap_or_default()
    }

    /// The run is a page of `origin` on the web: the platform said so before
    /// the bootstrap opened its first presentation.
    pub(crate) fn set_page_origin(&self, origin: String) {
        let parsed = OpenedUrl::parse(&origin).ok();
        *self.page_origin.borrow_mut() = Some(PageOrigin {
            text: origin,
            parsed,
        });
    }

    /// Give `ui_runtime` what it admits: the declarations, and on the web
    /// the page's origin. Before its first presentation can hear an opened
    /// URL.
    pub(crate) fn admit_in(&self, ui_runtime: &UiRuntime) {
        ui_runtime.set_open_declarations(self.declarations());
        // Cloned, so no borrow is held while the log line reaches its subscriber.
        let origin = self
            .page_origin
            .borrow()
            .as_ref()
            .map(|origin| origin.text.clone());
        if let Some(origin) = origin.as_deref()
            && let Err(refused) = ui_runtime.set_page_origin(origin)
        {
            tracing::warn!(
                reason = refused.reason(),
                "the page's origin is not an origin; only the declarations admit its URLs"
            );
        }
    }

    /// Route what the operating system opened: its files to the
    /// application's callback; each URL the application opens to its
    /// presentation, behind the URLs held for it, making its window visible,
    /// or into the hold ([`route_held`](Self::route_held)); a URL the
    /// application does not open is refused as it arrives. Each piece of
    /// user code (the callback, each URL's navigation) runs contained, and
    /// the first panic resumes once every URL had its turn, which ends the
    /// run (ADR-0258 §4); what is held then is refused as the run ends.
    pub(crate) fn deliver(&self, opened: Opened) {
        let Opened {
            urls,
            files,
            target,
            ..
        } = opened;
        let mut first = FirstPanic::new();
        self.files_to_callback(files, &mut first);
        let urls = self.admitted(urls, target);
        if !urls.is_empty() {
            match destination(target) {
                Destination::Open(host, address) => {
                    self.route_held_into(&mut first);
                    let urls = urls.into_iter().map(|held| held.url);
                    if open_urls(&host, address, urls, &mut first) > 0 {
                        reveal(address);
                    }
                }
                Destination::Waiting(wait) => {
                    self.hold(urls);
                    // Nothing else may wake an idle application for a URL
                    // that found the loop's state borrowed.
                    if wait == Wait::Busy {
                        self.request_turn();
                    }
                    self.route_held_into(&mut first);
                }
                Destination::Refused(refusal) => refusal.report_urls(urls.len()),
            }
        }
        first.resume();
    }

    /// Route what waits, by [`destination`] alone: first what a loan
    /// returned, once (into an open window, else refused, never opening
    /// one); then every held URL: into its open presentation (made visible),
    /// kept while its window is coming, refused when its window closed or is
    /// not opening. The URL held for the application is bound to the main
    /// window's opening: to the one under way, or to the one the reopen route
    /// starts for it (as a Dock click does), once; when that opening fails it
    /// is refused. Called as URLs arrive, when the bootstrap returns and from
    /// every owner turn while anything waits.
    pub(crate) fn route_held(&self) {
        let mut first = FirstPanic::new();
        self.route_held_into(&mut first);
        first.resume();
    }

    /// [`route_held`](Self::route_held), while anything waits: the owner's
    /// turn calls it.
    pub(crate) fn route_owed(&self) {
        if !self.returned.borrow().is_empty() || !self.held.borrow().is_empty() {
            self.route_held();
        }
    }

    fn route_held_into(&self, first: &mut FirstPanic) {
        self.route_returned(first);
        let failures = super::host::main_window_failures();
        let targets = self.held.borrow().targets();
        for target in targets {
            match destination(target) {
                Destination::Open(host, address) => {
                    let urls = self.held.borrow_mut().take(&[target]);
                    let urls = urls.into_iter().map(|held| held.url);
                    if open_urls(&host, address, urls, first) > 0 {
                        reveal(address);
                    }
                }
                Destination::Waiting(wait) => self.bind_to_opening(wait, failures),
                Destination::Refused(refusal) => {
                    let count = self.held.borrow_mut().take(&[target]).len();
                    refusal.report_urls(count);
                }
            }
        }
    }

    /// Bind the URL held for the application to the main window's opening,
    /// while `wait` is for the main window: the one under way, or one the
    /// reopen route takes now; a URL bound to an opening that failed since
    /// (`failures` grew) is refused, and so is one whose reopen the route
    /// did not take. A URL for a window waits for it.
    fn bind_to_opening(&self, wait: Wait, failures: u64) {
        let refused = {
            let mut held = self.held.borrow_mut();
            let Some(url) = held.application.as_mut() else {
                return;
            };
            match (url.bound, wait) {
                (Some(bound), _) if failures > bound => Some(Refusal::WindowFailed),
                (Some(_), _) | (None, Wait::Later | Wait::Busy) => None,
                (None, Wait::MainOpening) => {
                    url.bound = Some(failures);
                    None
                }
                (None, Wait::MainClosed) => {
                    // The route is the application's code path to the main
                    // window's controller: asked with nothing borrowed.
                    drop(held);
                    match super::host::reopen_current_run() {
                        ReopenAnswer::Taken => {
                            if let Some(url) = self.held.borrow_mut().application.as_mut() {
                                url.bound = Some(failures);
                            }
                            None
                        }
                        ReopenAnswer::NotTaken => Some(Refusal::ReopenNotTaken),
                        ReopenAnswer::NoRoute => None,
                    }
                }
            }
        };
        if let Some(refusal) = refused {
            let count = self
                .held
                .borrow_mut()
                .take(&[OpenTarget::Application])
                .len();
            refusal.report_urls(count);
        }
    }

    /// Route once what loans returned: a URL whose window was closed before
    /// it committed is refused, so is one whose opening into the window
    /// panicked; one whose installation failed goes into a
    /// window open now, unless a newer URL for the application was admitted
    /// since, and is refused otherwise: it caused its one opening.
    fn route_returned(&self, first: &mut FirstPanic) {
        let returned = std::mem::take(&mut *self.returned.borrow_mut());
        for (held, end) in returned {
            let refusal = match end {
                LoanEnd::WindowClosed => Refusal::WindowClosed,
                LoanEnd::NavigationPanicked => Refusal::NavigationPanicked,
                LoanEnd::Failed if self.superseded(&held) => Refusal::Superseded,
                LoanEnd::Failed => match destination(held.target) {
                    Destination::Open(host, address) => {
                        if open_urls(&host, address, [held.url], first) > 0 {
                            reveal(address);
                        }
                        continue;
                    }
                    Destination::Waiting(_) | Destination::Refused(_) => Refusal::WindowFailed,
                },
            };
            refusal.report_urls(1);
        }
    }

    /// Whether a newer URL for the application was admitted after `held`.
    fn superseded(&self, held: &HeldUrl) -> bool {
        held.target == OpenTarget::Application
            && self
                .newest_for_application
                .get()
                .is_some_and(|newest| newest > held.order)
    }

    /// Hold the URLs that launched the application for the presentation
    /// they address, which the bootstrap is about to open, and keep its
    /// files for the application, which hears them once the bootstrap
    /// returns ([`Launching::started`]).
    pub(crate) fn hold_launch(self: &Rc<Self>, launch: Opened) -> Launching {
        let Opened {
            urls,
            files,
            target,
            ..
        } = launch;
        let urls = self.admitted(urls, target);
        if !urls.is_empty() {
            self.hold(urls);
        }
        Launching {
            routing: Rc::clone(self),
            files: Some(files),
        }
    }

    /// Hand `files` to the application's callback, if it has one.
    fn open_files(&self, files: Vec<OpenedFile>) {
        let mut first = FirstPanic::new();
        self.files_to_callback(files, &mut first);
        first.resume();
    }

    fn files_to_callback(&self, files: Vec<OpenedFile>, first: &mut FirstPanic) {
        if files.is_empty() {
            return;
        }
        if let Some(callback) = self.settings.on_open_files.clone() {
            first.contain(|| callback(&files));
        } else {
            tracing::info!(
                files = files.len(),
                "the operating system opened files and the application has no on_open_files"
            );
        }
    }

    /// The URLs of `urls` the application opens, by the rule each UI runtime
    /// admits them by ([`OpenDeclarations::admit`]), each given its place in
    /// the order of admission. The rest are refused here, by reason, before
    /// any is held or can make a window open.
    fn admitted(&self, urls: Vec<String>, target: OpenTarget) -> Vec<HeldUrl> {
        if urls.is_empty() {
            return Vec::new();
        }
        let declarations = self.declarations();
        // Cloned, so no borrow is held while parsing logs to a subscriber.
        let origin = self
            .page_origin
            .borrow()
            .as_ref()
            .and_then(|origin| origin.parsed.clone());
        let mut refusals: Vec<&'static str> = Vec::new();
        let mut admitted = Vec::new();
        for url in urls {
            match OpenedUrl::parse(&url)
                .and_then(|parsed| declarations.admit(&parsed, origin.as_ref()))
            {
                Ok(()) => {
                    let order = self.next_order.get();
                    self.next_order.set(
                        order
                            .checked_add(1)
                            .expect("BUG: a run admits fewer than u64::MAX URLs"),
                    );
                    if target == OpenTarget::Application {
                        self.newest_for_application.set(Some(order));
                    }
                    admitted.push(HeldUrl {
                        target,
                        url,
                        order,
                        bound: None,
                    });
                }
                Err(refused) => refusals.push(refused.reason()),
            }
        }
        for reason in refusals {
            Refusal::NotAdmitted(reason).report_urls(1);
        }
        admitted
    }

    /// Open the URLs held for the presentation `ui_runtime` is built for in
    /// `window`, in that runtime before its root mounts, so its first Router
    /// opens on them (`UrlOwnerSlot::open_at`'s cold start). Only what
    /// [`destination`] gives this window is lent: the URL for the application
    /// to the main window while it is being opened (`is_main_window`, which
    /// each runner states: true for the desktop main-window controller's
    /// window and for the one window of iOS, Android and the web), never to
    /// another window that happens to be built first. They are on loan to the
    /// installation, which owns the returned guard.
    pub(crate) fn open_held_in(
        self: &Rc<Self>,
        ui_runtime: &UiRuntime,
        window: &dyn PlatformWindow,
        is_main_window: bool,
    ) -> Option<OpenedLoan> {
        let presentation = ui_runtime.presentation_id();
        let builds_main =
            is_main_window && !matches!(super::host::main_window(), MainWindow::Open(_));
        if builds_main {
            self.main_installing.set(Some(PresentationAddress {
                ui_runtime_id: ui_runtime.id(),
                presentation_id: presentation,
            }));
        }
        let targets = self.held.borrow().targets();
        // `destination` and the window, platform code, asked with nothing
        // borrowed.
        let lent: Vec<OpenTarget> = targets
            .into_iter()
            .filter(|target| match destination(*target) {
                Destination::Waiting(Wait::MainOpening | Wait::MainClosed) => builds_main,
                Destination::Waiting(Wait::Later | Wait::Busy) => {
                    WindowRegistry::names_window(*target, window)
                }
                Destination::Open(..) | Destination::Refused(_) => false,
            })
            .collect();
        let mut loan = self.lend(&lent)?;
        let refused = loan.open_each(|url| ui_runtime.open_url(presentation, url));
        for error in refused {
            // A subscriber is user code: its panic is not the navigation's.
            super::terminal::contain_diagnostic(|| {
                tracing::info!(refusal = error.reason(), "an opened URL was not opened");
            });
        }
        Some(loan)
    }

    /// Open the URLs [`destination`] gives the presentation at `address`,
    /// whose installation is about to commit, on loan to it like those
    /// [`open_held_in`](Self::open_held_in) opened: its window's own, and the
    /// URL for the application when it is the main window being opened.
    pub(crate) fn lend_to_installing(
        self: &Rc<Self>,
        address: PresentationAddress,
    ) -> Option<OpenedLoan> {
        let installs_main = self.main_installing.get() == Some(address);
        let targets: Vec<OpenTarget> = self
            .held
            .borrow()
            .targets()
            .into_iter()
            .filter(|target| match destination(*target) {
                Destination::Open(_, opened) => opened == address,
                Destination::Waiting(Wait::MainOpening | Wait::MainClosed) => installs_main,
                Destination::Waiting(Wait::Later | Wait::Busy) | Destination::Refused(_) => false,
            })
            .collect();
        let mut loan = self.lend(&targets)?;
        let host = installed_host()?;
        let refused = loan.open_each(|url| dispatch_url(&host, address, url.to_owned()));
        for error in refused {
            // A subscriber is user code: its panic is not the navigation's.
            super::terminal::contain_diagnostic(|| report_unreached(address, &error));
        }
        Some(loan)
    }

    /// Take the URLs held for `targets` on loan; `None` when none is held.
    fn lend(self: &Rc<Self>, targets: &[OpenTarget]) -> Option<OpenedLoan> {
        let urls = self.held.borrow_mut().take(targets);
        (!urls.is_empty()).then(|| OpenedLoan {
            routing: Rc::clone(self),
            urls,
            dropped_as: LoanEnd::Failed,
        })
    }

    /// Keep `urls` by the hold's rule for their target ([`HeldUrls::hold`]);
    /// what the hold does not keep is refused.
    fn hold(&self, urls: Vec<HeldUrl>) {
        let mut refused = Vec::new();
        {
            let mut held = self.held.borrow_mut();
            for url in urls {
                if let Err(refusal) = held.hold(url) {
                    refused.push(refusal);
                }
            }
        }
        for refusal in refused {
            refusal.report_urls(1);
        }
    }

    /// Refuse every held and returned URL for `refusal`.
    fn refuse_held(&self, refusal: Refusal) {
        let returned = std::mem::take(&mut *self.returned.borrow_mut()).len();
        let held = self.held.borrow_mut().take_all();
        refusal.report_urls(held + returned);
    }
}

/// URLs still held when the run ends were accepted and never opened: their
/// window never came. They are refused here, in the log, not dropped
/// unseen.
impl Drop for OpenRouting {
    fn drop(&mut self) {
        let returned = std::mem::take(self.returned.get_mut()).len();
        let held = self.held.get_mut().take_all() + returned;
        let refusal = if super::terminal::is_terminal() {
            Refusal::RunFailed
        } else {
            Refusal::RunEnded
        };
        refusal.report_urls(held);
    }
}

/// URLs opened into the UI runtime of a window whose installation has not
/// committed: a guard the installation owns from the moment the URLs leave
/// the hold. [`commit`](Self::commit) makes them the window's;
/// [`window_closed`](Self::window_closed) or the guard's drop returns them, and the owner's
/// next turn routes them ([`OpenRouting::route_owed`]).
///
/// Its drop only moves state and asks the owner for a turn: it runs no
/// code it does not own, so it may run anywhere, an unwind included.
#[must_use = "a loan dropped unused returns its URLs"]
pub(crate) struct OpenedLoan {
    routing: Rc<OpenRouting>,
    urls: Vec<HeldUrl>,
    /// How the loan ends if it is dropped: a failed installation, unless
    /// opening one of its URLs panicked.
    dropped_as: LoanEnd,
}

impl OpenedLoan {
    /// Open each lent URL into the window with `open`, each contained, so a
    /// panic in one does not lose the next. When one panics, the loan ends
    /// as that panic: the panic resumes once every URL had its turn, failing
    /// the installation that runs this, and the loan's URLs are refused for
    /// it (ADR-0339 §6).
    ///
    /// `open` runs the navigation alone and returns why a URL did not reach
    /// the window; the refusals come back to be logged outside it, so a
    /// subscriber's panic is never taken for the navigation's.
    fn open_each<E>(&mut self, mut open: impl FnMut(&str) -> Result<(), E>) -> Vec<E> {
        let mut first = FirstPanic::new();
        let mut refused = Vec::new();
        for held in &self.urls {
            if let Some(Err(error)) = first.contain(|| open(&held.url)) {
                refused.push(error);
            }
        }
        if first.is_held() {
            self.dropped_as = LoanEnd::NavigationPanicked;
        }
        first.resume();
        refused
    }

    /// The installation committed: the URLs are its window's.
    pub(crate) fn commit(mut self) {
        self.urls.clear();
    }

    /// The installation ended without committing because its window was
    /// closed: the URLs are refused on the owner's next turn.
    pub(crate) fn window_closed(mut self) {
        self.give_back(LoanEnd::WindowClosed);
    }

    /// Join `other`'s URLs to this loan, after its own.
    pub(crate) fn join(&mut self, mut other: Self) {
        self.urls.append(&mut other.urls);
    }

    fn give_back(&mut self, end: LoanEnd) {
        let urls = std::mem::take(&mut self.urls);
        if urls.is_empty() {
            return;
        }
        // The router borrows its returned list only to move it, running no
        // code meanwhile, so a loan never finds it borrowed.
        self.routing
            .returned
            .try_borrow_mut()
            .expect("BUG: a loan ends while the router moves its returned URLs")
            .extend(urls.into_iter().map(|held| (held, end)));
        self.routing.request_turn();
    }
}

/// Counts only: a URL may carry a token.
impl std::fmt::Debug for OpenedLoan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenedLoan")
            .field("urls", &self.urls.len())
            .finish_non_exhaustive()
    }
}

impl Drop for OpenedLoan {
    fn drop(&mut self) {
        self.give_back(self.dropped_as);
    }
}

/// What launched the application while its bootstrap runs: the files the
/// application hears once the bootstrap returns. Dropped without
/// [`started`](Self::started), because the bootstrap failed or panicked, it
/// refuses the files and the URLs held for the first window, which this run
/// never opens, in the log.
#[must_use = "a launch not started refuses what launched the application"]
pub(crate) struct Launching {
    routing: Rc<OpenRouting>,
    /// `None` once [`started`](Self::started) handed them over.
    files: Option<Vec<OpenedFile>>,
}

impl Launching {
    /// The bootstrap returned: the application hears the launch's files.
    pub(crate) fn started(mut self) {
        if let Some(files) = self.files.take() {
            self.routing.open_files(files);
        }
    }
}

impl Drop for Launching {
    fn drop(&mut self) {
        let Some(files) = self.files.take() else {
            return;
        };
        // Dropped as a failed bootstrap unwinds: the log lines reach a
        // subscriber, user code, so each runs contained.
        if !files.is_empty() {
            super::terminal::contain_diagnostic(|| {
                tracing::warn!(
                    files = files.len(),
                    reason = Refusal::NotStarted.reason(),
                    "opened files were refused"
                );
            });
        }
        let routing = Rc::clone(&self.routing);
        super::terminal::contain_diagnostic(move || routing.refuse_held(Refusal::NotStarted));
    }
}

/// Make the window presented at `address`, which an opened URL went to,
/// visible to the user: shown if hidden, restored if minimized, and brought
/// forward under the system's focus rules, as relaunching an application
/// does.
fn reveal(address: PresentationAddress) {
    let Some(handle) = super::window_handle(address) else {
        return;
    };
    let Ok(state) = handle.state() else {
        return;
    };
    if !state.visible
        && let Err(error) = handle.show()
    {
        tracing::debug!(%error, "a window an opened URL went to could not be shown");
    }
    if state.mode == flui_platform::WindowMode::Minimized
        && let Err(error) = handle.set_mode(flui_platform::WindowMode::Normal)
    {
        tracing::debug!(%error, "a window an opened URL went to could not be restored");
    }
    if let Err(error) = handle.activate() {
        tracing::debug!(%error, "a window an opened URL went to could not come forward");
    }
}

/// How many URLs one window still to be installed holds at most; each window
/// holds its own, so one window's URLs never take another's place.
const MAX_HELD_PER_WINDOW: usize = 8;

/// Why an opened URL or file was not opened, logged by its
/// [`reason`](Self::reason) and never with the URL, which may carry a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    /// The application does not open the URL: its
    /// [`OpenUrlRefused::reason`](flui_view::navigation::OpenUrlRefused::reason).
    NotAdmitted(&'static str),
    /// A newer URL for the application as a whole took its place: the
    /// window it waits for opens on one location.
    Superseded,
    /// The window it waits for holds [`MAX_HELD_PER_WINDOW`] URLs already.
    HeldFull,
    /// The window the URL was addressed to has closed.
    WindowClosed,
    /// The window the URL was addressed to is not open and no installation
    /// of it is under way.
    WindowNotOpen,
    /// The window opened for the URL failed to open, and no other window
    /// takes it.
    WindowFailed,
    /// Opening the URL into the window being installed panicked, which
    /// failed that installation.
    NavigationPanicked,
    /// The main window's controller did not take the reopen the URL asked
    /// for: the application no longer opens windows.
    ReopenNotTaken,
    /// The application's bootstrap failed or panicked.
    NotStarted,
    /// The run ended before a window opened for the URL.
    RunEnded,
    /// The run ended on the application's panic before a window opened for
    /// the URL.
    RunFailed,
}

impl Refusal {
    fn reason(self) -> &'static str {
        match self {
            Self::NotAdmitted(reason) => reason,
            Self::Superseded => "a newer URL took its place",
            Self::HeldFull => "too many URLs wait for its window",
            Self::WindowClosed => "its window closed",
            Self::WindowNotOpen => "its window is not open",
            Self::WindowFailed => "its window could not be opened",
            Self::NavigationPanicked => "its navigation panicked as its window opened",
            Self::ReopenNotTaken => "the application opens no window now",
            Self::NotStarted => "the application did not start",
            Self::RunEnded => "the run ended before a window opened",
            Self::RunFailed => "the run ended on the application's panic",
        }
    }

    /// Log that `urls` opened URLs were refused for this reason.
    fn report_urls(self, urls: usize) {
        if urls > 0 {
            tracing::warn!(urls, reason = self.reason(), "opened URLs were refused");
        }
    }
}

/// URLs admitted for presentations that do not exist yet, by the rule of
/// their target:
///
/// - for the application as a whole, the newest alone: the window it opens
///   in builds its first Router on one location (`UrlOwnerSlot::open_at`
///   keeps the last), so a newer URL supersedes the held one;
/// - for a window whose installation is under way, each in the order it
///   arrived, at most [`MAX_HELD_PER_WINDOW`] per window, the later ones
///   refused; a window's URLs never take another's place.
///
/// So the hold is bounded by one URL plus [`MAX_HELD_PER_WINDOW`] per window
/// being installed, each URL at most
/// [`MAX_OPENED_URL_LEN`](flui_platform::MAX_OPENED_URL_LEN) bytes.
#[derive(Default)]
struct HeldUrls {
    application: Option<HeldUrl>,
    windows: Vec<(OpenTarget, VecDeque<HeldUrl>)>,
}

impl HeldUrls {
    /// Hold `url` for its target by the target's rule.
    ///
    /// # Errors
    ///
    /// [`Refusal::Superseded`] when `url` replaced the application's held URL
    /// (the older one is the refused one), [`Refusal::HeldFull`] for `url`
    /// when its window holds as many as it may.
    fn hold(&mut self, url: HeldUrl) -> Result<(), Refusal> {
        if let OpenTarget::Window(_) = url.target {
            let urls = self.window_urls(url.target);
            if urls.len() == MAX_HELD_PER_WINDOW {
                return Err(Refusal::HeldFull);
            }
            urls.push_back(url);
            return Ok(());
        }
        if self.application.replace(url).is_some() {
            Err(Refusal::Superseded)
        } else {
            Ok(())
        }
    }

    /// The URLs held for the window `target` names, a new empty list when it
    /// holds none.
    fn window_urls(&mut self, target: OpenTarget) -> &mut VecDeque<HeldUrl> {
        let index = self
            .windows
            .iter()
            .position(|(held, _)| *held == target)
            .unwrap_or_else(|| {
                self.windows.push((target, VecDeque::new()));
                self.windows.len() - 1
            });
        &mut self.windows[index].1
    }

    /// The targets URLs are held for, each once.
    fn targets(&self) -> Vec<OpenTarget> {
        self.application
            .iter()
            .map(|_| OpenTarget::Application)
            .chain(self.windows.iter().map(|(target, _)| *target))
            .collect()
    }

    /// Whether nothing is held.
    fn is_empty(&self) -> bool {
        self.application.is_none() && self.windows.is_empty()
    }

    /// Take the URLs held for any of `targets`, each window's in the order
    /// they arrived.
    fn take(&mut self, targets: &[OpenTarget]) -> Vec<HeldUrl> {
        let mut taken = Vec::new();
        if targets.contains(&OpenTarget::Application) {
            taken.extend(self.application.take());
        }
        self.windows.retain_mut(|(target, urls)| {
            if targets.contains(target) {
                taken.extend(urls.drain(..));
                false
            } else {
                true
            }
        });
        taken
    }

    /// Take every URL held; how many there were.
    fn take_all(&mut self) -> usize {
        let windows: usize = self.windows.drain(..).map(|(_, urls)| urls.len()).sum();
        usize::from(self.application.take().is_some()) + windows
    }
}

/// Counts only: a held URL may carry a token, which a log of this would print.
impl std::fmt::Debug for HeldUrls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeldUrls")
            .field("application", &self.application.is_some())
            .field("windows", &self.windows.len())
            .finish()
    }
}

/// Where URLs for a target go now: the one answer the routing step, the
/// loans and the reopen all ask for.
enum Destination {
    /// The installed host and the presentation that takes them.
    Open(super::InstalledHost, PresentationAddress),
    /// No presentation takes them yet: they wait, as `Wait` says.
    Waiting(Wait),
    /// They are refused for this reason.
    Refused(Refusal),
}

/// What a URL with no presentation yet waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wait {
    /// The main window, being opened: the URL is bound to that opening.
    MainOpening,
    /// The main window, not open and not being opened: the reopen route
    /// starts its opening for the URL, where the run has one.
    MainClosed,
    /// Its own window's installation, under way.
    Later,
    /// The loop's state is borrowed, so no destination can be read now: the
    /// next owner turn routes the URL.
    Busy,
}

/// The run's installed host, unless the loop's state is busy or gone.
fn installed_host() -> Option<super::InstalledHost> {
    APP_RUNTIME
        .try_with(|slot| {
            slot.try_borrow()
                .ok()
                .map(|state| state.installed_host.clone())
        })
        .ok()
        .flatten()
}

/// Where what the operating system opened for `target` goes now (ADR-0339
/// §2): a URL for a window to that window; a URL for the application to the
/// run's main window ([`MainWindow`]), whatever window was focused or opened
/// first, waiting while it is being opened or closed.
fn destination(target: OpenTarget) -> Destination {
    let Some(host) = installed_host() else {
        return Destination::Waiting(Wait::Busy);
    };
    let main = super::host::main_window();
    let main_address = match main {
        MainWindow::Open(address) => Some(address),
        MainWindow::Opening | MainWindow::Closed | MainWindow::Busy => None,
    };
    match host.open_target(target, main_address) {
        OpenDestination::Presentation(address) => Destination::Open(host, address),
        OpenDestination::Pending => Destination::Waiting(match (target, main) {
            (OpenTarget::Window(_), _) => Wait::Later,
            (_, MainWindow::Busy) => Wait::Busy,
            (_, MainWindow::Opening) => Wait::MainOpening,
            (_, MainWindow::Open(_) | MainWindow::Closed) => Wait::MainClosed,
        }),
        OpenDestination::Closed => Destination::Refused(Refusal::WindowClosed),
        OpenDestination::NotOpen => Destination::Refused(Refusal::WindowNotOpen),
    }
}

/// Open each of `urls` in `address`, in the operating system's order, each
/// contained, so a panic in one URL's navigation does not lose the next; how
/// many reached the window.
fn open_urls(
    host: &super::InstalledHost,
    address: PresentationAddress,
    urls: impl IntoIterator<Item = String>,
    first: &mut FirstPanic,
) -> usize {
    let mut reached = 0_usize;
    for url in urls {
        match first.contain(|| dispatch_url(host, address, url)) {
            Some(Ok(())) => reached += 1,
            Some(Err(error)) => report_unreached(address, &error),
            None => {}
        }
    }
    reached
}

/// Hand `url` to the presentation at `address`, in order with its accepted
/// work; why it did not reach it.
fn dispatch_url(
    host: &super::InstalledHost,
    address: PresentationAddress,
    url: String,
) -> Result<(), flui_runtime::owner::DispatchError> {
    host.logical()
        .presentation_dispatcher(address)
        .and_then(|target| target.open_url(url, host.effects()))
        .map(|_delivery| ())
}

/// Log that an opened URL did not reach the presentation at `address`.
fn report_unreached(address: PresentationAddress, error: &flui_runtime::owner::DispatchError) {
    tracing::warn!(?address, %error, "an opened URL could not reach its window");
}
#[cfg(all(test, feature = "single-instance", any(target_os = "linux", windows)))]
mod tests {
    use flui_log::{AppId, AppIdentity};
    use flui_view::navigation::{FileTypeDeclaration, OpenDeclarations};

    use super::OpenSettings;

    /// One instance is kept under the application id when the application
    /// declares a URL scheme or a file type or asks for it, and not when it
    /// turns it off, declares only web domains, or has no id.
    #[test]
    fn one_instance_follows_the_declarations_unless_the_application_says() {
        let named = AppIdentity::try_new("Notes")
            .expect("a display name")
            .with_app_id(AppId::try_new("com.example.Notes").expect("an id"));
        let unnamed = AppIdentity::try_new("Notes").expect("a display name");
        let settings =
            |declarations: Option<OpenDeclarations>, single: Option<bool>| OpenSettings {
                declarations,
                single_instance: single,
                ..OpenSettings::default()
            };
        let scheme = Some(OpenDeclarations::new().url_schemes(&["notes"]));
        let domain = Some(OpenDeclarations::new().associated_domains(&["example.com"]));
        const NOTE: &[FileTypeDeclaration] = &[FileTypeDeclaration::new(
            "note",
            "application/x-notes-note",
            "Note",
        )];
        let file_type = Some(OpenDeclarations::new().file_types(NOTE));
        let id = Some("com.example.Notes".to_owned());
        let cases = [
            (
                "a declared scheme",
                settings(scheme, None),
                &named,
                id.clone(),
            ),
            (
                "a declared file type",
                settings(file_type, None),
                &named,
                id.clone(),
            ),
            ("turned off", settings(scheme, Some(false)), &named, None),
            ("asked for", settings(None, Some(true)), &named, id),
            ("domains only", settings(domain, None), &named, None),
            ("nothing declared", settings(None, None), &named, None),
            (
                "no application id",
                settings(scheme, Some(true)),
                &unnamed,
                None,
            ),
        ];
        for (case, settings, identity, expected) in cases {
            assert_eq!(settings.single_instance_id(identity), expected, "{case}");
        }
    }
}

#[cfg(test)]
mod loan_tests {
    use std::rc::Rc;

    use flui_platform::{Launch, OpenTarget, Opened};
    use flui_view::navigation::OpenDeclarations;

    use super::{LoanEnd, OpenRouting, OpenSettings};

    /// A routing that declares `myapp`, with no run on this thread: a URL
    /// finds no window, so one for the application stays held.
    fn routing() -> Rc<OpenRouting> {
        Rc::new(OpenRouting::new(OpenSettings {
            declarations: Some(OpenDeclarations::new().url_schemes(&["myapp"])),
            ..OpenSettings::default()
        }))
    }

    /// Hold `url` for the application, as a launch does.
    fn hold(routing: &Rc<OpenRouting>, url: &str) {
        routing
            .hold_launch(
                Opened::bounded([url.to_owned()], [], OpenTarget::Application, Launch::Cold).0,
            )
            .started();
    }

    /// How each URL a loan returned ended, in order.
    fn returned(routing: &OpenRouting) -> Vec<(String, LoanEnd)> {
        routing
            .returned
            .borrow()
            .iter()
            .map(|(held, end)| (held.url.clone(), *end))
            .collect()
    }

    /// A committed loan keeps its URLs; one dropped or ended by its window's
    /// close returns them, untouched by any routing, with how it ended; a
    /// URL returned after a newer one for the application was admitted is
    /// the older and loses to it.
    #[test]
    fn a_loan_is_committed_or_returns_its_urls_without_routing() {
        let routing = routing();

        hold(&routing, "myapp:///a");
        let loan = routing
            .lend(&[OpenTarget::Application])
            .expect("the held URL is lent");
        loan.commit();
        assert!(
            returned(&routing).is_empty(),
            "a committed loan returns nothing"
        );

        hold(&routing, "myapp:///b");
        let loan = routing
            .lend(&[OpenTarget::Application])
            .expect("the held URL is lent");
        drop(loan);
        assert_eq!(
            returned(&routing),
            [("myapp:///b".to_owned(), LoanEnd::Failed)]
        );
        assert!(
            routing.held.borrow().application.is_none(),
            "a returned URL is not held again: it caused its one opening"
        );
        routing.returned.borrow_mut().clear();

        hold(&routing, "myapp:///c");
        let loan = routing
            .lend(&[OpenTarget::Application])
            .expect("the held URL is lent");
        loan.window_closed();
        assert_eq!(
            returned(&routing),
            [("myapp:///c".to_owned(), LoanEnd::WindowClosed)]
        );
        routing.returned.borrow_mut().clear();

        hold(&routing, "myapp:///d");
        let loan = routing
            .lend(&[OpenTarget::Application])
            .expect("the held URL is lent");
        hold(&routing, "myapp:///e");
        drop(loan);
        let order = |url: &str| {
            routing
                .returned
                .borrow()
                .iter()
                .find(|(held, _)| held.url == url)
                .map(|(held, _)| held.order)
        };
        let older = order("myapp:///d").expect("d came back");
        assert!(
            routing
                .newest_for_application
                .get()
                .is_some_and(|newest| newest > older),
            "the URL that came back is older than the one admitted while it was out"
        );
    }

    /// A loan dropped while its thread unwinds only returns its URLs: it
    /// logs nothing and routes nothing, so no subscriber or route runs in
    /// the unwinding destructor.
    #[test]
    fn a_loan_dropped_during_an_unwind_only_returns_its_urls() {
        flui_testing::disarm_interest_cache();
        let routing = routing();
        hold(&routing, "myapp:///a");
        let loan = routing
            .lend(&[OpenTarget::Application])
            .expect("the held URL is lent");
        let (unwound, log) = flui_testing::capture(|| {
            #[expect(
                clippy::disallowed_methods,
                reason = "the test unwinds through the loan on purpose"
            )]
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let _loan = loan;
                panic!("the installation panics while it holds the loan");
            }))
        });
        assert!(unwound.is_err(), "the closure unwound");
        assert!(log.is_empty(), "the unwinding drop logged nothing: {log}");
        assert_eq!(
            returned(&routing),
            [("myapp:///a".to_owned(), LoanEnd::Failed)]
        );
    }

    /// A URL whose opening into the window panics ends its loan as that
    /// panic: the panic leaves the loan, which returns the URL marked so, and
    /// the next routing refuses it for the panic, not as a failed window.
    #[test]
    fn a_loan_whose_navigation_panics_is_refused_for_the_panic() {
        flui_testing::disarm_interest_cache();
        let routing = routing();
        hold(&routing, "myapp:///a");
        let mut loan = routing
            .lend(&[OpenTarget::Application])
            .expect("the held URL is lent");
        #[expect(
            clippy::disallowed_methods,
            reason = "the test unwinds out of the navigation on purpose"
        )]
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _refused: Vec<()> = loan.open_each(|_| panic!("the navigation panics"));
        }));
        assert!(unwound.is_err(), "the navigation's panic leaves the loan");
        assert_eq!(
            returned(&routing),
            [("myapp:///a".to_owned(), LoanEnd::NavigationPanicked)]
        );
        let ((), log) = flui_testing::capture(|| routing.route_owed());
        assert_eq!(
            log.count_containing("its navigation panicked as its window opened"),
            1,
            "{log}"
        );
    }

    /// A loan that ends while the loop's state is borrowed (an installation
    /// dropped under a borrow of it) still asks the owner for a turn, through
    /// the proxy the routing keeps, and that turn routes what it returned.
    // The headless kit that runs an owner is built where `crate::testing` is.
    #[cfg(all(
        feature = "testing",
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    #[test]
    fn a_loan_dropped_while_the_loops_state_is_borrowed_asks_for_its_turn() {
        use super::{APP_RUNTIME, OpenedLoan};

        flui_testing::disarm_interest_cache();
        let run = crate::testing::HeadlessRun::start_resident(
            crate::app::Application::new()
                .declares(OpenDeclarations::new().url_schemes(&["myapp"]))
                .config(
                    crate::app::AppConfig::new().exit_policy(crate::app::ExitPolicy::ExplicitQuit),
                ),
            |_: &crate::app::AppHandle| flui_widgets::SizedBox::new(1.0, 1.0),
        );
        run.run_owner_turn();
        run.run_owner_turn();
        let routing = super::super::host::current_open_routing()
            .expect("the run routes what the system opens");
        // A link for a window nobody opens, on loan as an installation holds it.
        let window = flui_platform::WindowId::try_from_raw(999).expect("a nonzero id");
        let loan = OpenedLoan {
            urls: routing.admitted(vec!["myapp:///a".to_owned()], OpenTarget::Window(window)),
            routing: Rc::clone(&routing),
            dropped_as: LoanEnd::Failed,
        };
        let ((), log) = flui_testing::capture(|| {
            APP_RUNTIME.with(|slot| {
                let _busy = slot.borrow_mut();
                drop(loan);
            });
            run.run_owner_turn();
        });
        assert_eq!(
            log.count_containing("its window could not be opened"),
            1,
            "the turn the loan asked for routed it: {log}"
        );
        drop(run);
    }

    /// A link the platform reports while the loop's state is borrowed finds
    /// no destination yet; with nothing else happening in the resident
    /// application, the turn its delivery asked for routes it: it opens the
    /// main window.
    // The headless kit that runs an owner is built where `crate::testing` is.
    #[cfg(all(
        feature = "testing",
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    #[test]
    fn a_link_that_finds_the_loops_state_borrowed_is_routed_on_the_turn_it_asks_for() {
        use super::APP_RUNTIME;

        flui_testing::disarm_interest_cache();
        let run = crate::testing::HeadlessRun::start_resident(
            crate::app::Application::new()
                .declares(OpenDeclarations::new().url_schemes(&["myapp"]))
                .config(
                    crate::app::AppConfig::new().exit_policy(crate::app::ExitPolicy::ExplicitQuit),
                ),
            |_: &crate::app::AppHandle| flui_widgets::SizedBox::new(1.0, 1.0),
        );
        run.run_owner_turn();
        run.run_owner_turn();
        assert!(
            run.open_windows().is_empty(),
            "a resident run starts with no window"
        );
        let routing = super::super::host::current_open_routing()
            .expect("the run routes what the system opens");
        APP_RUNTIME.with(|slot| {
            let _busy = slot.borrow_mut();
            routing.deliver(
                Opened::bounded(
                    ["myapp:///a".to_owned()],
                    [],
                    OpenTarget::Application,
                    Launch::Running,
                )
                .0,
            );
        });
        for _ in 0..4 {
            run.run_owner_turn();
        }
        assert_eq!(
            run.open_windows().len(),
            1,
            "the turn the delivery asked for routed the link, which opened the main window"
        );
        drop(run);
    }
}
