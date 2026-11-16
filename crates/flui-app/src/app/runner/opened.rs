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
/// settings, the presentation focused last, the URLs held for a presentation
/// that does not exist yet and those lent to a window being installed.
/// Owned by the run's application handler, so a run never sees another's.
///
/// Every opened URL ends one way: it is opened in a presentation's UI
/// runtime, held for a presentation still to come ([`HeldUrls`]), or refused
/// with a [`Refusal`] that is logged by its reason. URLs opened into a window
/// whose installation then fails come back to the hold ([`Lent`]). Held URLs
/// are refused when the bootstrap fails ([`Launching`]) or the run ends.
pub(crate) struct OpenRouting {
    settings: OpenSettings,
    /// The presentation whose window was focused last.
    active: Cell<Option<PresentationAddress>>,
    /// URLs admitted for a presentation that does not exist yet.
    held: RefCell<HeldUrls>,
    /// URLs opened into the UI runtime of a window whose installation has
    /// not committed yet.
    lent: RefCell<Vec<Lent>>,
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

/// URLs opened into the UI runtime of the presentation at `address` before
/// its root mounted, kept until its installation commits: a failed
/// installation drops the runtime, and they go back to the hold.
struct Lent {
    address: PresentationAddress,
    urls: Vec<(OpenTarget, String)>,
}

impl OpenRouting {
    pub(crate) fn new(settings: OpenSettings) -> Self {
        Self {
            settings,
            active: Cell::new(None),
            held: RefCell::new(HeldUrls::default()),
            lent: RefCell::new(Vec::new()),
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
    /// presentation, behind the URLs held for it, or held until it exists;
    /// a URL the application does not open, one for a window that closed or
    /// was never opened, and one past the hold's bound are refused. Each
    /// piece of user code (the callback, each URL's navigation) runs
    /// contained, and the first panic resumes once every URL had its turn,
    /// which ends the run (ADR-0258 §4); what is held then is refused as the
    /// run ends.
    ///
    /// The handler then asks [`waits_for_a_window`](Self::waits_for_a_window)
    /// whether the run should open one.
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
            self.settle();
            match destination(target, self.active.get()) {
                Destination::Open(host, address) => {
                    self.deliver_held_into(&mut first);
                    open_urls(&host, address, urls, &mut first);
                }
                Destination::Pending => self.hold(target, urls),
                Destination::Refused(refusal) => refusal.report_urls(urls.len()),
            }
        }
        first.resume();
    }

    /// Whether a URL is held for the application as a whole while it has no
    /// presentation: the run then opens its main window for it, as a Dock
    /// click does, and that window's first Router opens on it. A URL held
    /// for one window waits for that window's installation, which is under
    /// way.
    pub(crate) fn waits_for_a_window(&self) -> bool {
        self.settle();
        self.held.borrow().holds_for_application()
            && matches!(
                destination(OpenTarget::Application, self.active.get()),
                Destination::Pending
            )
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
            self.hold(target, urls);
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
    /// opens on them (`UrlOwnerSlot::open_at`'s cold start). They are lent
    /// to the installation: [`installed`](Self::installed) settles them, and
    /// a failed installation gives them back to the hold.
    pub(crate) fn open_held_in(&self, ui_runtime: &UiRuntime, window: &dyn PlatformWindow) {
        // What a window that failed before it was submitted was lent is held
        // again, for this one.
        self.reclaim_lent();
        let targets = self.held.borrow().targets();
        if targets.is_empty() {
            return;
        }
        // The window is platform code: asked with nothing borrowed.
        let opening: Vec<OpenTarget> = targets
            .into_iter()
            .filter(|target| WindowRegistry::opens_in(*target, window))
            .collect();
        let urls = self.held.borrow_mut().take(&opening);
        if urls.is_empty() {
            return;
        }
        let presentation = ui_runtime.presentation_id();
        self.lent.borrow_mut().push(Lent {
            address: PresentationAddress {
                ui_runtime_id: ui_runtime.id(),
                presentation_id: presentation,
            },
            urls: urls.clone(),
        });
        let mut first = FirstPanic::new();
        for (_, url) in urls {
            first.contain(|| {
                if let Err(error) = ui_runtime.open_url(presentation, &url) {
                    tracing::info!(refusal = error.reason(), "an opened URL was not opened");
                }
            });
        }
        first.resume();
    }

    /// The installation of `address` committed: the URLs lent to it are its.
    pub(crate) fn installed(&self, address: PresentationAddress) {
        self.lent
            .borrow_mut()
            .retain(|lent| lent.address != address);
    }

    /// The installation of `address` failed: the URLs lent to it come back to
    /// the hold, and those for its own window are refused.
    pub(crate) fn install_failed(&self, address: PresentationAddress) {
        let returned = {
            let mut lent = self.lent.borrow_mut();
            lent.iter()
                .position(|lent| lent.address == address)
                .map(|index| lent.remove(index))
        };
        if let Some(returned) = returned {
            self.give_back(returned);
        }
        self.settle();
    }

    /// Put lent URLs back in the hold; one it no longer takes is refused.
    fn give_back(&self, returned: Lent) {
        let mut refused = Vec::new();
        {
            let mut held = self.held.borrow_mut();
            for (target, url) in returned.urls.into_iter().rev() {
                if let Err(refusal) = held.give_back(target, url) {
                    refused.push(refusal);
                }
            }
        }
        for refusal in refused {
            refusal.report_urls(1);
        }
    }

    /// Settle what waits on installations that ended without a word to the
    /// router ([`reclaim_lent`](Self::reclaim_lent)), and refuse the URLs held
    /// for a window whose installation is not under way, or that closed.
    fn settle(&self) {
        self.reclaim_lent();
        let windows = self.held.borrow().window_targets();
        for target in windows {
            if let Destination::Refused(refusal) = destination(target, self.active.get()) {
                let count = self.held.borrow_mut().drop_target(target);
                refusal.report_urls(count);
            }
        }
    }

    /// URLs lent to an installation that neither runs nor committed (one
    /// that failed before it was submitted, as when the GPU refuses its
    /// window) come back to the hold; those of one that committed are its.
    fn reclaim_lent(&self) {
        let Some(host) = installed_host() else {
            return;
        };
        let ended: Vec<Lent> = {
            let mut lent = self.lent.borrow_mut();
            // An installation still under way keeps what it was lent; one
            // that ended registered committed, and what it was lent is its.
            lent.retain(|lent| {
                host.is_installing(lent.address) || !host.native().contains_address(lent.address)
            });
            let mut ended = Vec::new();
            let mut index = 0;
            while index < lent.len() {
                if host.is_installing(lent[index].address) {
                    index += 1;
                } else {
                    ended.push(lent.remove(index));
                }
            }
            ended
        };
        for returned in ended {
            self.give_back(returned);
        }
    }

    /// Open the URLs held for a presentation that joined a UI runtime built
    /// before they arrived, once it is installed.
    pub(crate) fn deliver_held(&self) {
        let mut first = FirstPanic::new();
        self.deliver_held_into(&mut first);
        first.resume();
    }

    /// Open every held URL whose target has a presentation now, target by
    /// target; a URL held for a window that closed meanwhile is refused.
    fn deliver_held_into(&self, first: &mut FirstPanic) {
        let targets = self.held.borrow().targets();
        for target in targets {
            match destination(target, self.active.get()) {
                Destination::Open(host, address) => {
                    let urls = self.held.borrow_mut().take(&[target]);
                    open_urls(&host, address, urls.into_iter().map(|(_, url)| url), first);
                }
                Destination::Pending => {}
                Destination::Refused(refusal) => {
                    let count = self.held.borrow_mut().drop_target(target);
                    refusal.report_urls(count);
                }
            }
        }
    }

    /// Keep `urls` until `target` has a presentation, by the hold's rule for
    /// the target ([`HeldUrls::hold`]); what the hold does not keep is
    /// refused.
    fn hold(&self, target: OpenTarget, urls: Vec<String>) {
        let mut refused = Vec::new();
        {
            let mut held = self.held.borrow_mut();
            for url in urls {
                if let Err(refusal) = held.hold(target, url) {
                    refused.push(refusal);
                }
            }
        }
        for refusal in refused {
            refusal.report_urls(1);
        }
    }

    /// Refuse every held and lent URL for `refusal`.
    fn refuse_held(&self, refusal: Refusal) {
        let lent: usize = self
            .lent
            .borrow_mut()
            .drain(..)
            .map(|lent| lent.urls.len())
            .sum();
        let held = self.held.borrow_mut().take_all();
        refusal.report_urls(held + lent);
    }
}

/// URLs still held when the run ends were accepted and never opened: their
/// window never came. They are refused here, in the log, not dropped
/// unseen; so are URLs lent to an installation the run never finished.
impl Drop for OpenRouting {
    fn drop(&mut self) {
        let lent: usize = self.lent.get_mut().iter().map(|lent| lent.urls.len()).sum();
        let held = self.held.get_mut().take_all();
        Refusal::RunEnded.report_urls(held + lent);
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
    application: Option<String>,
    windows: Vec<(OpenTarget, VecDeque<String>)>,
}

impl HeldUrls {
    /// Hold `url` for `target` by the target's rule.
    ///
    /// # Errors
    ///
    /// [`Refusal::Superseded`] when `url` replaced the application's held URL
    /// (the older one is the refused one), [`Refusal::HeldFull`] for `url`
    /// when its window holds as many as it may.
    fn hold(&mut self, target: OpenTarget, url: String) -> Result<(), Refusal> {
        if let OpenTarget::Window(_) = target {
            let urls = self.window_urls(target);
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

    /// Put back a URL lent to an installation that failed, ahead of the URLs
    /// its target holds: the application's only while it holds none newer.
    ///
    /// # Errors
    ///
    /// [`Refusal::Superseded`] when the application holds a newer URL,
    /// [`Refusal::HeldFull`] when the window holds as many as it may.
    fn give_back(&mut self, target: OpenTarget, url: String) -> Result<(), Refusal> {
        if let OpenTarget::Window(_) = target {
            let urls = self.window_urls(target);
            if urls.len() == MAX_HELD_PER_WINDOW {
                return Err(Refusal::HeldFull);
            }
            urls.push_front(url);
            return Ok(());
        }
        if self.application.is_some() {
            return Err(Refusal::Superseded);
        }
        self.application = Some(url);
        Ok(())
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
            .chain(self.window_targets())
            .collect()
    }

    /// The windows URLs are held for.
    fn window_targets(&self) -> Vec<OpenTarget> {
        self.windows.iter().map(|(target, _)| *target).collect()
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

    /// Drop what `target` holds; how many URLs that was.
    fn drop_target(&mut self, target: OpenTarget) -> usize {
        self.take(&[target]).len()
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
