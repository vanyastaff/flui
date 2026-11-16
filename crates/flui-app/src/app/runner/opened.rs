//! What the operating system opens the application with, routed to the
//! application (ADR-0330 §5), on every host and the headless one: files to
//! its [`on_open_files`](crate::Application::on_open_files) callback, URLs it
//! declares to the presentation they address, where the UI runtime opens
//! each one in that presentation's `Router` (`UiRuntime::open_url`).
//!
//! The presentation is the window's own for [`OpenTarget::Window`] (an iOS
//! scene, an Android activity), and for the application as a whole the one
//! whose window was focused last, else the window opened first. A URL is
//! admitted here first, so one the application does not open is refused
//! before it is held or can make a window open. URLs that arrive before their
//! presentation exists (a link that launched the application, one that came
//! while it had no window) are held per target, in order and bounded, and
//! queued for it as it is installed, ahead of its first frame, so its first
//! `Router` opens on the link instead of its initial page; for the
//! application as a whole the run opens its main window for them
//! ([`OpenRouting::waits_for_a_window`]).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use flui_foundation::PresentationAddress;
use flui_foundation::panic::FirstPanic;
use flui_platform::{
    MAX_OPENED_ITEMS, MAX_OPENED_URL_LEN, OpenTarget, Opened, OpenedFile, PlatformWindow,
};
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
/// runtime, held for a presentation still to come (bounded, see
/// [`HeldUrls`]), or refused with a [`Refusal`] that is logged by its reason.
/// Held URLs are opened when their presentation is installed and refused
/// when the bootstrap fails ([`Launching`]) or the run ends.
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
    /// presentation, behind the URLs held for it, or held until it exists,
    /// or refused when its window closed or the held URLs fill their bound;
    /// a URL the application does not open is refused as it arrives. Each
    /// piece of user code (the callback, each URL's navigation) runs
    /// contained, and the first panic resumes once every URL had its turn.
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
            match destination(target, self.active.get()) {
                Destination::Open(host, address) => {
                    self.deliver_held_into(&mut first);
                    open_urls(&host, address, urls, &mut first);
                }
                Destination::Pending => self.hold(target, urls),
                Destination::Closed => Refusal::WindowClosed.report_urls(urls.len()),
            }
        }
        first.resume();
    }

    /// Whether URLs are held for the application as a whole while it has no
    /// presentation: the run then opens its main window for them, as a Dock
    /// click does, and that window's first Router opens on them. A URL held
    /// for one window waits for that window, which its host opens.
    pub(crate) fn waits_for_a_window(&self) -> bool {
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

    /// The URLs of `urls` the application opens: by its declarations, and on
    /// the web its page's origin, as each UI runtime admits them
    /// (`UiRuntime::open_url`). The rest are refused here, by reason, before
    /// any is held or can make a window open.
    fn admitted(&self, urls: Vec<String>) -> Vec<String> {
        if urls.is_empty() {
            return urls;
        }
        let declarations = self.declarations();
        let mut refusals: Vec<&'static str> = Vec::new();
        // Cloned, so no borrow is held while parsing logs to a subscriber.
        let origin = self
            .page_origin
            .borrow()
            .as_ref()
            .and_then(|origin| origin.parsed.clone());
        let admitted: Vec<String> = urls
            .into_iter()
            .filter(|url| {
                let admission = OpenedUrl::parse(url).and_then(|parsed| {
                    if origin
                        .as_ref()
                        .is_some_and(|origin| origin.same_origin(&parsed))
                    {
                        Ok(())
                    } else {
                        declarations.admit(&parsed)
                    }
                });
                admission
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
    /// opens on them (`UrlOwnerSlot::open_at`'s cold start).
    pub(crate) fn open_held_in(&self, ui_runtime: &UiRuntime, window: &dyn PlatformWindow) {
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
        let presentation = ui_runtime.presentation_id();
        let mut first = FirstPanic::new();
        for url in urls {
            first.contain(|| {
                if let Err(error) = ui_runtime.open_url(presentation, &url) {
                    tracing::info!(refusal = error.reason(), "an opened URL was not opened");
                }
            });
        }
        first.resume();
    }

    /// Open the URLs held for a presentation that joined a UI runtime built
    /// before they arrived, once it is installed.
    pub(crate) fn deliver_held(&self) {
        let mut first = FirstPanic::new();
        self.deliver_held_into(&mut first);
        first.resume();
    }

    /// Open every held URL whose target has a presentation now, target by
    /// target in the order of their first URL; a URL held for a window that
    /// closed meanwhile is refused.
    fn deliver_held_into(&self, first: &mut FirstPanic) {
        let targets = self.held.borrow().targets();
        for target in targets {
            match destination(target, self.active.get()) {
                Destination::Open(host, address) => {
                    let urls = self.held.borrow_mut().take(&[target]);
                    open_urls(&host, address, urls, first);
                }
                Destination::Pending => {}
                Destination::Closed => {
                    let urls = self.held.borrow_mut().take(&[target]);
                    Refusal::WindowClosed.report_urls(urls.len());
                }
            }
        }
    }

    /// Keep `urls` until `target` has a presentation, behind the URLs held
    /// before them; those past the bound are refused.
    fn hold(&self, target: OpenTarget, urls: Vec<String>) {
        let mut over = 0_usize;
        {
            let mut held = self.held.borrow_mut();
            for url in urls {
                if held.hold(target, url).is_err() {
                    over += 1;
                }
            }
        }
        Refusal::HeldFull.report_urls(over);
    }

    /// Refuse every held URL for `refusal`.
    fn refuse_held(&self, refusal: Refusal) {
        let count = self.held.borrow_mut().take_all();
        refusal.report_urls(count);
    }
}

/// URLs still held when the run ends were accepted and never opened: their
/// window never came. They are refused here, in the log, not dropped
/// unseen.
impl Drop for OpenRouting {
    fn drop(&mut self) {
        let count = self.held.get_mut().take_all();
        Refusal::RunEnded.report_urls(count);
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

/// The most bytes of URL text the runner holds for presentations that do
/// not exist yet, every target together: eight URLs of the longest an
/// [`Opened`] carries, or [`MAX_OPENED_ITEMS`] ordinary ones.
const MAX_HELD_URL_BYTES: usize = 8 * MAX_OPENED_URL_LEN;

/// Why an opened URL or file was not opened, logged by its
/// [`reason`](Self::reason) and never with the URL, which may carry a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    /// The application does not open the URL: its
    /// [`OpenUrlRefused::reason`](flui_view::navigation::OpenUrlRefused::reason).
    NotAdmitted(&'static str),
    /// The URLs held for windows still to open fill their bound.
    HeldFull,
    /// The window the URL was addressed to has closed.
    WindowClosed,
    /// The application's bootstrap failed or panicked.
    NotStarted,
    /// The run ended before a window opened for the URL.
    RunEnded,
}

impl Refusal {
    fn reason(self) -> &'static str {
        match self {
            Self::NotAdmitted(reason) => reason,
            Self::HeldFull => "too many URLs wait for a window",
            Self::WindowClosed => "its window closed",
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

/// URLs admitted for presentations that do not exist yet, in the order they
/// arrived, each with the target it was addressed to; at most
/// [`MAX_OPENED_ITEMS`] URLs and [`MAX_HELD_URL_BYTES`] bytes in all.
#[derive(Debug, Default)]
struct HeldUrls {
    entries: VecDeque<(OpenTarget, String)>,
    bytes: usize,
}

impl HeldUrls {
    /// Hold `url` for `target`, behind every URL held before it, or refuse it
    /// when the bound is reached: the URLs held first keep their place.
    fn hold(&mut self, target: OpenTarget, url: String) -> Result<(), Refusal> {
        // Each URL is at most `MAX_OPENED_URL_LEN` bytes (`Opened::new`,
        // `OpenedUrl::parse`) and `bytes` at most `MAX_HELD_URL_BYTES`.
        let bytes = self.bytes + url.len();
        if self.entries.len() == MAX_OPENED_ITEMS || bytes > MAX_HELD_URL_BYTES {
            return Err(Refusal::HeldFull);
        }
        self.bytes = bytes;
        self.entries.push_back((target, url));
        Ok(())
    }

    /// The targets URLs are held for, each once, in the order of their first
    /// URL.
    fn targets(&self) -> Vec<OpenTarget> {
        let mut targets = Vec::new();
        for (target, _) in &self.entries {
            if !targets.contains(target) {
                targets.push(*target);
            }
        }
        targets
    }

    /// Whether a URL is held for the application as a whole.
    fn holds_for_application(&self) -> bool {
        self.entries
            .iter()
            .any(|(target, _)| *target == OpenTarget::Application)
    }

    /// Take the URLs held for any of `targets`, in the order they arrived.
    fn take(&mut self, targets: &[OpenTarget]) -> Vec<String> {
        let mut taken = Vec::new();
        self.entries.retain_mut(|(target, url)| {
            if targets.contains(target) {
                taken.push(std::mem::take(url));
                false
            } else {
                true
            }
        });
        self.bytes -= taken.iter().map(String::len).sum::<usize>();
        taken
    }

    /// Take every URL held; how many there were.
    fn take_all(&mut self) -> usize {
        self.bytes = 0;
        self.entries.drain(..).count()
    }
}

/// Where URLs for a target go now.
enum Destination {
    /// The installed host and the presentation that takes them.
    Open(super::InstalledHost, PresentationAddress),
    /// No presentation yet, or the loop's state is busy: they wait.
    Pending,
    /// The window they were addressed to has closed.
    Closed,
}

/// Where what the operating system opened for `target` goes now
/// ([`WindowRegistry::open_target`]).
fn destination(target: OpenTarget, active: Option<PresentationAddress>) -> Destination {
    let Some(host) = APP_RUNTIME.with(|slot| {
        slot.try_borrow()
            .ok()
            .map(|state| state.installed_host.clone())
    }) else {
        return Destination::Pending;
    };
    match host.native().open_target(target, active) {
        OpenDestination::Presentation(address) => Destination::Open(host, address),
        OpenDestination::Pending => Destination::Pending,
        OpenDestination::Closed => Destination::Closed,
    }
}

/// Open each of `urls` in `address`, in the operating system's order, each
/// contained, so a panic in one URL's navigation does not lose the next.
fn open_urls(
    host: &super::InstalledHost,
    address: PresentationAddress,
    urls: Vec<String>,
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
