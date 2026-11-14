//! What the operating system opens the application with, routed to the
//! application (activation spec, AC5): files to its
//! [`on_open_files`](crate::Application::on_open_files) callback, URLs to
//! the presentation they address, where the UI runtime parses and admits
//! each one by the application's declarations and opens it in that
//! presentation's `Router` (`UiRuntime::open_url`, ADR-0330 §5).
//!
//! The presentation is the window's own for
//! [`OpenTarget::Window`] (an iOS scene, an
//! Android activity), and for the application as a whole the one whose
//! window was focused last, else the window opened first. URLs that arrive
//! before their presentation exists (a link that launched the application)
//! are held, the newest target's alone and bounded, and queued for it as it
//! is installed, ahead of its first frame, so its first `Router` opens on the
//! link instead of its initial page.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::PresentationAddress;
use flui_foundation::panic::FirstPanic;
use flui_platform::{MAX_OPENED_ITEMS, OpenTarget, Opened, OpenedFile, PlatformWindow};
use flui_runtime::ui_runtime::UiRuntime;
use flui_view::navigation::LinkDeclarations;

use super::host::APP_RUNTIME;
use crate::app::window_registry::WindowRegistry;

/// The callback an application hears its opened files with.
pub(crate) type OpenFilesCallback = Rc<dyn Fn(&[OpenedFile])>;

/// What an [`Application`](crate::Application) set about what the operating
/// system opens it with.
#[derive(Clone, Default)]
pub(crate) struct OpenSettings {
    /// The URL schemes and domains it opens.
    pub(crate) declarations: Option<LinkDeclarations>,
    /// Where its opened files go.
    pub(crate) on_open_files: Option<OpenFilesCallback>,
    /// Whether the application runs as one instance; `None` leaves it to
    /// the declarations.
    pub(crate) single_instance: Option<bool>,
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
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    pub(crate) fn single_instance_id(&self, identity: &flui_log::AppIdentity) -> Option<String> {
        let declared = self
            .declarations
            .is_some_and(|declarations| !declarations.declared_url_schemes().is_empty());
        // The Linux and Windows hosts keep one instance; macOS, iOS and
        // Android keep one themselves, and each page on the Web is an
        // application of its own.
        let kept_here = cfg!(any(target_os = "linux", windows));
        if !kept_here || !self.single_instance.unwrap_or(declared) {
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
}

impl std::fmt::Debug for OpenSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenSettings")
            .field("declarations", &self.declarations)
            .field("on_open_files", &self.on_open_files.is_some())
            .field("single_instance", &self.single_instance)
            .finish()
    }
}

/// One run's routing of what the operating system opened: the application's
/// settings, the presentation focused last and the URLs held for a
/// presentation that does not exist yet. Owned by the run's application
/// handler, so a run never sees another's.
pub(crate) struct OpenRouting {
    settings: OpenSettings,
    /// The presentation whose window was focused last.
    active: Cell<Option<PresentationAddress>>,
    /// URLs whose presentation did not exist when they arrived, with the
    /// target they were addressed to.
    held: RefCell<Option<(OpenTarget, Vec<String>)>>,
    /// The origin of the page the run is in on the web, whose URLs every UI
    /// runtime of the run admits beside the declarations.
    page_origin: RefCell<Option<String>>,
}

impl OpenRouting {
    pub(crate) fn new(settings: OpenSettings) -> Self {
        Self {
            settings,
            active: Cell::new(None),
            held: RefCell::new(None),
            page_origin: RefCell::new(None),
        }
    }

    /// The URL schemes and domains the application opens, which every UI
    /// runtime of the run is given before its first presentation opens.
    pub(crate) fn declarations(&self) -> LinkDeclarations {
        self.settings.declarations.unwrap_or_default()
    }

    /// The run is a page of `origin` on the web: the platform said so before
    /// the bootstrap opened its first presentation.
    pub(crate) fn set_page_origin(&self, origin: String) {
        *self.page_origin.borrow_mut() = Some(origin);
    }

    /// Give `ui_runtime` what it admits: the declarations, and on the web
    /// the page's origin. Before its first presentation can hear an opened
    /// URL.
    pub(crate) fn admit_in(&self, ui_runtime: &UiRuntime) {
        ui_runtime.set_link_declarations(self.declarations());
        // Cloned, so no borrow is held while the log line reaches its subscriber.
        let origin = self.page_origin.borrow().clone();
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
    /// application's callback, its URLs to their presentation, or held until
    /// it exists. Each piece of user code (the callback, each URL's
    /// navigation) runs contained, and the first panic resumes once every
    /// URL had its turn.
    pub(crate) fn deliver(&self, opened: Opened) {
        let Opened {
            urls,
            files,
            target,
            ..
        } = opened;
        let mut first = FirstPanic::new();
        self.files_to_callback(files, &mut first);
        if !urls.is_empty() {
            match open_target(target, self.active.get()) {
                Some((host, address)) => open_urls(&host, address, urls, &mut first),
                None => self.hold(target, urls),
            }
        }
        first.resume();
    }

    /// Hold the URLs that launched the application for the presentation
    /// they address, which the bootstrap is about to open, and hand back its
    /// files, which the application hears once the bootstrap returns.
    pub(crate) fn hold_launch(&self, launch: Opened) -> Vec<OpenedFile> {
        let Opened {
            urls,
            files,
            target,
            ..
        } = launch;
        if !urls.is_empty() {
            self.hold(target, urls);
        }
        files
    }

    /// Hand `files` to the application's callback, if it has one.
    pub(crate) fn open_files(&self, files: Vec<OpenedFile>) {
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

    /// Open the URLs held for the presentation `ui_runtime` is built for in
    /// `window`, in that runtime before its root mounts, so its first Router
    /// opens on them (`UrlOwnerSlot::open_at`'s cold start).
    pub(crate) fn open_held_in(&self, ui_runtime: &UiRuntime, window: &dyn PlatformWindow) {
        let Some(target) = self.held.borrow().as_ref().map(|(target, _)| *target) else {
            return;
        };
        // The window is platform code: asked with nothing borrowed.
        if !WindowRegistry::opens_in(target, window) {
            return;
        }
        let Some((_, urls)) = self.held.borrow_mut().take() else {
            return;
        };
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
        let Some(target) = self.held.borrow().as_ref().map(|(target, _)| *target) else {
            return;
        };
        let Some((host, address)) = open_target(target, self.active.get()) else {
            return;
        };
        let Some((_, urls)) = self.held.borrow_mut().take() else {
            return;
        };
        let mut first = FirstPanic::new();
        open_urls(&host, address, urls, &mut first);
        first.resume();
    }

    /// Keep `urls` until `target` has a presentation. URLs for the same
    /// target join the ones held, up to [`MAX_OPENED_ITEMS`]; a new target
    /// replaces the old one's, whose window is not coming.
    fn hold(&self, target: OpenTarget, urls: Vec<String>) {
        let mut held = self.held.borrow_mut();
        let dropped = match held.as_mut() {
            Some((held_target, held_urls)) if *held_target == target => {
                let room = MAX_OPENED_ITEMS.saturating_sub(held_urls.len());
                let over = urls.len().saturating_sub(room);
                held_urls.extend(urls.into_iter().take(room));
                over
            }
            _ => held
                .replace((target, urls))
                .map_or(0, |(_, replaced)| replaced.len()),
        };
        drop(held);
        if dropped > 0 {
            tracing::warn!(
                dropped,
                "opened URLs waiting for their window were dropped: too many, or another target"
            );
        }
    }
}

/// URLs still held when the run ends were accepted and never opened: their
/// window never came. They are refused here, in the log, not dropped
/// unseen.
impl Drop for OpenRouting {
    fn drop(&mut self) {
        if let Some((_, urls)) = self.held.get_mut().take() {
            tracing::warn!(
                urls = urls.len(),
                "opened URLs were never opened: the run ended before their window opened"
            );
        }
    }
}

/// The installed host and the presentation `target` names, or `None` while
/// it has none (or the loop's state is busy).
fn open_target(
    target: OpenTarget,
    active: Option<PresentationAddress>,
) -> Option<(super::InstalledHost, PresentationAddress)> {
    let host = APP_RUNTIME.with(|slot| {
        slot.try_borrow()
            .ok()
            .map(|state| state.installed_host.clone())
    })?;
    let address = host.native().open_target(target, active)?;
    Some((host, address))
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

#[cfg(all(test, any(target_os = "linux", windows)))]
mod tests {
    use flui_log::{AppId, AppIdentity};
    use flui_view::navigation::LinkDeclarations;

    use super::OpenSettings;

    /// One instance is kept under the application id when the application
    /// declares a URL scheme or asks for it, and not when it turns it off,
    /// declares only web domains, or has no id.
    #[test]
    fn one_instance_follows_the_declarations_unless_the_application_says() {
        let named = AppIdentity::try_new("Notes")
            .expect("a display name")
            .with_app_id(AppId::try_new("com.example.Notes").expect("an id"));
        let unnamed = AppIdentity::try_new("Notes").expect("a display name");
        let settings =
            |declarations: Option<LinkDeclarations>, single: Option<bool>| OpenSettings {
                declarations,
                single_instance: single,
                ..OpenSettings::default()
            };
        let scheme = Some(LinkDeclarations::new().url_schemes(&["notes"]));
        let domain = Some(LinkDeclarations::new().associated_domains(&["example.com"]));
        let id = Some("com.example.Notes".to_owned());
        let cases = [
            (
                "a declared scheme",
                settings(scheme, None),
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
