//! What the operating system opens the application with, routed to the
//! application on every host and the headless one (ADR-0339): files to its
//! [`on_open_files`](crate::Application::on_open_files) callback, URLs it
//! declares to the presentation they address, where the UI runtime opens
//! each one in that presentation's `Router` (`UiRuntime::open_url`).
//!
//! The presentation is the window's own for [`OpenTarget::Window`] (an iOS
//! scene), and for the application as a whole the one whose window was
//! focused last, else the window opened first. A URL is admitted here first,
//! so one the application does not open is refused before it is held or can
//! make a window open. URLs that arrive before their presentation exists (a
//! link that launched the application, one that came while it had no window)
//! are held by their target's rule ([`HeldUrls`]) and opened in its UI
//! runtime as it is installed, ahead of its first frame, so its first
//! `Router` opens on the link instead of its initial page; for the
//! application as a whole the run opens its main window for them
//! ([`OpenRouting::waits_for_a_window`]).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use flui_foundation::PresentationAddress;
use flui_foundation::panic::FirstPanic;
use flui_platform::{OpenTarget, Opened, OpenedFile, PlatformWindow};
use flui_runtime::ui_runtime::UiRuntime;
use flui_view::navigation::{OpenDeclarations, OpenedUrl};

use super::host::APP_RUNTIME;
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
/// settings, the presentation focused last and the URLs held for a
/// presentation that does not exist yet. Owned by the run's application
/// handler, so a run never sees another's.
///
/// Every opened URL ends one way: it is opened in a presentation's UI
/// runtime, held for a presentation still to come ([`HeldUrls`]), or refused
/// with a [`Refusal`] that is logged by its reason. URLs opened into a window
/// whose installation has not committed are on loan to it ([`OpenedLoan`]):
/// the installation commits them, or the loan's end gives them back, and they
/// are routed again at once. Held URLs are refused when the bootstrap fails
/// ([`Launching`]) or the run ends.
pub(crate) struct OpenRouting {
    settings: OpenSettings,
    /// The presentation whose window was focused last.
    active: Cell<Option<PresentationAddress>>,
    /// URLs admitted for a presentation that does not exist yet.
    held: RefCell<HeldUrls>,
    /// The origin of the page the run is in on the web, whose URLs every UI
    /// runtime of the run admits beside the declarations.
    page_origin: RefCell<Option<PageOrigin>>,
}

/// The origin of the page a web run is in: as the platform said it, which
/// each UI runtime is given, and parsed, which the runner admits by; `None`
/// when it is not a URL, and then only the declarations admit.
struct PageOrigin {
    text: String,
    parsed: Option<OpenedUrl>,
}

impl OpenRouting {
    pub(crate) fn new(settings: OpenSettings) -> Self {
        Self {
            settings,
            active: Cell::new(None),
            held: RefCell::new(HeldUrls::default()),
            page_origin: RefCell::new(None),
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

    /// The window of `address` gained focus.
    pub(crate) fn focused(&self, address: PresentationAddress) {
        self.active.set(Some(address));
    }

    /// Route what the operating system opened: its files to the
    /// application's callback; each URL the application opens to its
    /// presentation, behind the URLs held for it, showing a window that is
    /// not shown, or into the hold ([`route_held`](Self::route_held)); a URL
    /// the application does not open is refused as it arrives. Each piece of
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
        let urls = self.admitted(urls);
        if !urls.is_empty() {
            match destination(target, self.active.get()) {
                Destination::Open(host, address) => {
                    self.route_held_into(&mut first);
                    open_urls(&host, address, urls, &mut first);
                    show_if_hidden(address);
                }
                Destination::Pending => {
                    self.hold(target, urls, End::Back);
                    self.route_held_into(&mut first);
                }
                Destination::Refused(refusal) => refusal.report_urls(urls.len()),
            }
        }
        first.resume();
    }

    /// Route every held URL by where its target goes now: into its open
    /// presentation (shown if its window is not), kept while its window is
    /// coming, refused when its window closed or is not opening; and when a
    /// URL waits for the application as a whole with no window, the run's
    /// main window is opened for it through the reopen route, as a Dock
    /// click does. Called whenever URLs enter the hold: as they arrive, when
    /// the bootstrap returns and when a loan ends without committing.
    pub(crate) fn route_held(&self) {
        let mut first = FirstPanic::new();
        self.route_held_into(&mut first);
        first.resume();
    }

    fn route_held_into(&self, first: &mut FirstPanic) {
        let targets = self.held.borrow().targets();
        for target in targets {
            match destination(target, self.active.get()) {
                Destination::Open(host, address) => {
                    let urls = self.held.borrow_mut().take(&[target]);
                    open_urls(&host, address, urls.into_iter().map(|(_, url)| url), first);
                    show_if_hidden(address);
                }
                Destination::Pending => {}
                Destination::Refused(refusal) => {
                    let count = self.held.borrow_mut().take(&[target]).len();
                    refusal.report_urls(count);
                }
            }
        }
        let waits = self.held.borrow().holds_for_application()
            && matches!(
                destination(OpenTarget::Application, self.active.get()),
                Destination::Pending
            );
        if waits {
            super::host::reopen_current_run();
        }
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
        let urls = self.admitted(urls);
        if !urls.is_empty() {
            self.hold(target, urls, End::Back);
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
    /// admits them by ([`OpenDeclarations::admit`]). The rest are refused
    /// here, by reason, before any is held or can make a window open.
    fn admitted(&self, urls: Vec<String>) -> Vec<String> {
        if urls.is_empty() {
            return urls;
        }
        let declarations = self.declarations();
        // Cloned, so no borrow is held while parsing logs to a subscriber.
        let origin = self
            .page_origin
            .borrow()
            .as_ref()
            .and_then(|origin| origin.parsed.clone());
        let mut refusals: Vec<&'static str> = Vec::new();
        let admitted: Vec<String> = urls
            .into_iter()
            .filter(|url| {
                OpenedUrl::parse(url)
                    .and_then(|parsed| declarations.admit(&parsed, origin.as_ref()))
                    .map_err(|refused| refusals.push(refused.reason()))
                    .is_ok()
            })
            .collect();
        for reason in refusals {
            Refusal::NotAdmitted(reason).report_urls(1);
        }
        admitted
    }

    /// Open the URLs held for the presentation `ui_runtime` is built for in
    /// `window`, in that runtime before its root mounts, so its first Router
    /// opens on them (`UrlOwnerSlot::open_at`'s cold start). They are on loan
    /// to the installation, which owns the returned guard.
    pub(crate) fn open_held_in(
        self: &Rc<Self>,
        ui_runtime: &UiRuntime,
        window: &dyn PlatformWindow,
    ) -> Option<OpenedLoan> {
        let targets = self.held.borrow().targets();
        if targets.is_empty() {
            return None;
        }
        // The window is platform code: asked with nothing borrowed.
        let opening: Vec<OpenTarget> = targets
            .into_iter()
            .filter(|target| WindowRegistry::opens_in(*target, window))
            .collect();
        let loan = self.lend(&opening)?;
        let presentation = ui_runtime.presentation_id();
        let mut first = FirstPanic::new();
        for (_, url) in &loan.urls {
            first.contain(|| {
                if let Err(error) = ui_runtime.open_url(presentation, url) {
                    tracing::info!(refusal = error.reason(), "an opened URL was not opened");
                }
            });
        }
        first.resume();
        Some(loan)
    }

    /// Open the URLs held for the presentation at `address`, whose
    /// installation is about to commit, on loan to it like those
    /// [`open_held_in`](Self::open_held_in) opened.
    pub(crate) fn lend_to_installing(
        self: &Rc<Self>,
        address: PresentationAddress,
    ) -> Option<OpenedLoan> {
        let targets: Vec<OpenTarget> = self
            .held
            .borrow()
            .targets()
            .into_iter()
            .filter(|target| {
                matches!(
                    destination(*target, self.active.get()),
                    Destination::Open(_, opened) if opened == address
                )
            })
            .collect();
        let loan = self.lend(&targets)?;
        let host = installed_host()?;
        let mut first = FirstPanic::new();
        open_urls(
            &host,
            address,
            loan.urls.iter().map(|(_, url)| url.clone()),
            &mut first,
        );
        first.resume();
        Some(loan)
    }

    /// Take the URLs held for `targets` on loan; `None` when none is held.
    fn lend(self: &Rc<Self>, targets: &[OpenTarget]) -> Option<OpenedLoan> {
        let urls = self.held.borrow_mut().take(targets);
        (!urls.is_empty()).then(|| OpenedLoan {
            routing: Rc::clone(self),
            urls,
        })
    }

    /// Put a loan's URLs back in the hold, ahead of what each target holds;
    /// one the hold no longer takes is refused.
    fn give_back(&self, urls: Vec<(OpenTarget, String)>) {
        let mut refused = Vec::new();
        {
            let mut held = self.held.borrow_mut();
            for (target, url) in urls.into_iter().rev() {
                if let Err(refusal) = held.hold(target, url, End::Front) {
                    refused.push(refusal);
                }
            }
        }
        for refusal in refused {
            refusal.report_urls(1);
        }
    }

    /// Keep `urls` until `target` has a presentation, by the hold's rule for
    /// the target ([`HeldUrls::hold`]); what the hold does not keep is
    /// refused.
    fn hold(&self, target: OpenTarget, urls: Vec<String>, end: End) {
        let mut refused = Vec::new();
        {
            let mut held = self.held.borrow_mut();
            for url in urls {
                if let Err(refusal) = held.hold(target, url, end) {
                    refused.push(refusal);
                }
            }
        }
        for refusal in refused {
            refusal.report_urls(1);
        }
    }

    /// Refuse every held URL for `refusal`.
    fn refuse_held(&self, refusal: Refusal) {
        let held = self.held.borrow_mut().take_all();
        refusal.report_urls(held);
    }
}

/// URLs still held when the run ends were accepted and never opened: their
/// window never came. They are refused here, in the log, not dropped
/// unseen.
impl Drop for OpenRouting {
    fn drop(&mut self) {
        let held = self.held.get_mut().take_all();
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
/// the hold. [`commit`](Self::commit) makes them the window's; any other end
/// of the installation drops the guard, which gives them back to the hold
/// and routes them again at once ([`OpenRouting::route_held`]).
#[must_use = "a loan dropped unused gives its URLs back"]
pub(crate) struct OpenedLoan {
    routing: Rc<OpenRouting>,
    urls: Vec<(OpenTarget, String)>,
}

impl OpenedLoan {
    /// The installation committed: the URLs are its window's.
    pub(crate) fn commit(mut self) {
        self.urls.clear();
    }

    /// Join `other`'s URLs to this loan, after its own.
    pub(crate) fn join(&mut self, mut other: Self) {
        self.urls.append(&mut other.urls);
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
        let urls = std::mem::take(&mut self.urls);
        if urls.is_empty() {
            return;
        }
        self.routing.give_back(urls);
        let mut first = FirstPanic::new();
        self.routing.route_held_into(&mut first);
        // A navigation's panic resumes from here, unless the thread is
        // already unwinding, when the earlier panic is the one kept.
        if std::thread::panicking() {
            if let Some(payload) = first.into_payload() {
                flui_foundation::panic::discard_payload(payload);
            }
        } else {
            first.resume();
        }
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
        if !files.is_empty() {
            tracing::warn!(
                files = files.len(),
                reason = Refusal::NotStarted.reason(),
                "opened files were refused"
            );
        }
        self.routing.refuse_held(Refusal::NotStarted);
    }
}

/// Show the window presented at `address` when it is not shown: a URL the
/// application opens in a hidden window shows that window and brings it
/// forward, as relaunching it does.
fn show_if_hidden(address: PresentationAddress) {
    let Some(handle) = super::window_handle(address) else {
        return;
    };
    if handle.state().is_ok_and(|state| !state.visible) {
        if let Err(error) = handle.show() {
            tracing::debug!(%error, "a window an opened URL went to could not be shown");
        }
        if let Err(error) = handle.activate() {
            tracing::debug!(%error, "a window an opened URL went to could not come forward");
        }
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
    /// of it is under way, or its installation failed.
    WindowNotOpen,
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

/// Which end of its target's URLs a URL joins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    /// Behind them: it arrived after them.
    Back,
    /// Ahead of them: it was opened before them and comes back from a loan.
    Front,
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
    application: Option<String>,
    windows: Vec<(OpenTarget, VecDeque<String>)>,
}

impl HeldUrls {
    /// Hold `url` for `target` by the target's rule, at `end` of what it
    /// holds. For the application, a URL that arrives replaces the held one;
    /// one that comes back from a loan is held only while no newer one is.
    ///
    /// # Errors
    ///
    /// [`Refusal::Superseded`] for the application's URL that lost (the held
    /// one an arriving URL replaced, or the one coming back),
    /// [`Refusal::HeldFull`] for `url` when its window holds as many as it
    /// may.
    fn hold(&mut self, target: OpenTarget, url: String, end: End) -> Result<(), Refusal> {
        if let OpenTarget::Window(_) = target {
            let urls = self.window_urls(target);
            if urls.len() == MAX_HELD_PER_WINDOW {
                return Err(Refusal::HeldFull);
            }
            match end {
                End::Back => urls.push_back(url),
                End::Front => urls.push_front(url),
            }
            return Ok(());
        }
        match end {
            End::Back => match self.application.replace(url) {
                Some(_) => Err(Refusal::Superseded),
                None => Ok(()),
            },
            End::Front if self.application.is_some() => Err(Refusal::Superseded),
            End::Front => {
                self.application = Some(url);
                Ok(())
            }
        }
    }

    /// The URLs held for the window `target` names, a new empty list when it
    /// holds none.
    fn window_urls(&mut self, target: OpenTarget) -> &mut VecDeque<String> {
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

    /// Whether a URL is held for the application as a whole.
    fn holds_for_application(&self) -> bool {
        self.application.is_some()
    }

    /// Take the URLs held for any of `targets`, with their targets, each
    /// window's in the order they arrived.
    fn take(&mut self, targets: &[OpenTarget]) -> Vec<(OpenTarget, String)> {
        let mut taken = Vec::new();
        if targets.contains(&OpenTarget::Application)
            && let Some(url) = self.application.take()
        {
            taken.push((OpenTarget::Application, url));
        }
        self.windows.retain_mut(|(target, urls)| {
            if targets.contains(target) {
                taken.extend(urls.drain(..).map(|url| (*target, url)));
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

/// Where URLs for a target go now.
enum Destination {
    /// The installed host and the presentation that takes them.
    Open(super::InstalledHost, PresentationAddress),
    /// No presentation yet, or the loop's state is busy: they wait.
    Pending,
    /// They are refused for this reason.
    Refused(Refusal),
}

/// The run's installed host, unless the loop's state is busy.
fn installed_host() -> Option<super::InstalledHost> {
    APP_RUNTIME.with(|slot| {
        slot.try_borrow()
            .ok()
            .map(|state| state.installed_host.clone())
    })
}

/// Where what the operating system opened for `target` goes now
/// ([`InstalledHost::open_target`](super::InstalledHost::open_target)).
fn destination(target: OpenTarget, active: Option<PresentationAddress>) -> Destination {
    let Some(host) = installed_host() else {
        return Destination::Pending;
    };
    match host.open_target(target, active) {
        OpenDestination::Presentation(address) => Destination::Open(host, address),
        OpenDestination::Pending => Destination::Pending,
        OpenDestination::Closed => Destination::Refused(Refusal::WindowClosed),
        OpenDestination::NotOpen => Destination::Refused(Refusal::WindowNotOpen),
    }
}

/// Open each of `urls` in `address`, in the operating system's order, each
/// contained, so a panic in one URL's navigation does not lose the next.
fn open_urls(
    host: &super::InstalledHost,
    address: PresentationAddress,
    urls: impl IntoIterator<Item = String>,
    first: &mut FirstPanic,
) {
    for url in urls {
        first.contain(|| {
            let opened = host
                .logical()
                .presentation_dispatcher(address)
                .and_then(|target| target.open_url(url, host.effects()));
            if let Err(error) = opened {
                tracing::warn!(?address, %error, "an opened URL could not reach its window");
            }
        });
    }
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

    use flui_platform::OpenTarget;

    use super::{End, OpenRouting, OpenSettings};

    /// The application's held URL, if one is held.
    fn held(routing: &OpenRouting) -> Option<String> {
        routing.held.borrow().application.clone()
    }

    /// A loan the installation commits keeps its URL; one dropped gives it
    /// back to the hold, unless a newer URL arrived meanwhile, which stays
    /// and the old one is refused as superseded. No run is on this thread,
    /// so the URL given back finds no window and stays held.
    #[test]
    fn a_loan_is_committed_or_given_back_by_its_drop() {
        let routing = Rc::new(OpenRouting::new(OpenSettings::default()));
        let application = |url: &str| {
            routing.hold(OpenTarget::Application, vec![url.to_owned()], End::Back);
        };

        application("myapp:///a");
        let loan = routing
            .lend(&[OpenTarget::Application])
            .expect("the held URL is lent");
        assert_eq!(held(&routing), None, "the lent URL left the hold");
        loan.commit();
        assert_eq!(held(&routing), None, "a committed loan gives nothing back");

        application("myapp:///b");
        let loan = routing
            .lend(&[OpenTarget::Application])
            .expect("the held URL is lent");
        drop(loan);
        assert_eq!(
            held(&routing).as_deref(),
            Some("myapp:///b"),
            "a dropped loan gives its URL back"
        );

        let loan = routing
            .lend(&[OpenTarget::Application])
            .expect("the held URL is lent");
        application("myapp:///c");
        drop(loan);
        assert_eq!(
            held(&routing).as_deref(),
            Some("myapp:///c"),
            "a newer URL keeps its place over the one coming back"
        );
        assert!(routing.lend(&[]).is_none(), "nothing is lent for no target");
    }
}
