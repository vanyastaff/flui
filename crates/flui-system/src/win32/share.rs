//! The share sheet over `DataTransferManager`, reached for a desktop
//! window through `IDataTransferManagerInterop` (Windows 10 and later).
//!
//! On the owner thread, which owns the window: the manager is the window's,
//! the `DataRequested` handler fills the package when the sheet asks for
//! it, and `ShowShareUIForWindow` shows the sheet. A share with files first
//! checks, on a shell thread, that each exists, so a missing file is
//! [`ShareError::NotFound`] rather than an empty sheet; the sheet then
//! takes the files as `StorageFile`s, which the handler resolves on a shell
//! thread of the same runtime's count, in the multithreaded apartment, under
//! the request's deferral. One registration per window, replaced by the next
//! share and removed when the window closes.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use flui_platform::capability::{
    CompletionSource, ProviderContext, Resolver, WindowCloseWatch, WindowRef,
};
use flui_platform::{Unsupported, UnsupportedReason, WindowId};
use rustc_hash::FxHashMap;
use windows::ApplicationModel::DataTransfer::{DataRequestedEventArgs, DataTransferManager};
use windows::Foundation::{TypedEventHandler, Uri};
use windows::Storage::{IStorageItem, StorageFile};
use windows::Win32::UI::Shell::IDataTransferManagerInterop;
use windows::core::{HSTRING, Interface as _};
use windows_collections::IIterable;

use super::threads::{ApartmentKind, ShellPool};
use super::{Owner, ShellThreads, failure};
use crate::share::{ShareBackend, ShareCapability, ShareError, ShareRequest, ShareResult};
use flui_foundation::panic::{AfterPanic, os_entry};

/// What a share hands the sheet, owned by its handler.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Payload {
    pub(crate) title: String,
    pub(crate) text: Option<String>,
    /// The link, and whether it is a web link (`http`, `https`) rather than
    /// an application's.
    pub(crate) uri: Option<(String, bool)>,
    pub(crate) files: Vec<PathBuf>,
}

impl Payload {
    fn of(request: &ShareRequest) -> Self {
        Self {
            title: sheet_title(request),
            text: request.text.clone(),
            uri: request.uri.as_ref().map(|uri| {
                let scheme = uri.scheme();
                let web =
                    scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http");
                (uri.as_str().to_owned(), web)
            }),
            files: request.files.clone(),
        }
    }
}

/// The title the sheet shows: the request's, or what it shares, since the
/// sheet refuses a package without one.
pub(super) fn sheet_title(request: &ShareRequest) -> String {
    request
        .title
        .clone()
        .or_else(|| request.uri.as_ref().map(|uri| uri.as_str().to_owned()))
        .or_else(|| {
            request
                .files
                .first()
                .and_then(|file| file.file_name())
                .map(|name| name.to_string_lossy().into_owned())
        })
        .or_else(|| {
            request
                .text
                .as_ref()
                .map(|text| text.chars().take(80).collect::<String>())
        })
        .unwrap_or_default()
}

/// The share sheet as the provider reaches it, on the owner thread: show
/// `payload` for `window`, whose handle `owner` names.
pub(crate) trait ShareSystem: 'static {
    fn show(&self, window: &WindowRef, owner: Owner, payload: Payload) -> ShareResult;
}

/// [`ShareBackend`] over the share sheet.
pub(crate) struct WindowsShare {
    system: Rc<dyn ShareSystem>,
    threads: Rc<ShellThreads>,
    completions: CompletionSource,
}

impl WindowsShare {
    pub(crate) fn new(ctx: &ProviderContext<'_>) -> Self {
        let threads = ctx.runtime_local::<ShellThreads>();
        let sheet = Sheet {
            registrations: ctx.runtime_local::<ShareRegistrations>(),
            pool: threads.pool(),
        };
        Self::over(Rc::new(sheet), threads, ctx)
    }

    pub(crate) fn over(
        system: Rc<dyn ShareSystem>,
        threads: Rc<ShellThreads>,
        ctx: &ProviderContext<'_>,
    ) -> Self {
        Self {
            system,
            threads,
            completions: ctx.completions().clone(),
        }
    }
}

impl ShareBackend for WindowsShare {
    fn share(&self, window: &WindowRef, request: ShareRequest, answer: Resolver<ShareResult>) {
        let Some(owner) = Owner::of(window) else {
            let no_window = Unsupported::of::<ShareCapability>(UnsupportedReason::NoWindow);
            let _ = answer.resolve(Err(ShareError::Unsupported(no_window)));
            return;
        };
        if request.files.is_empty() {
            let _ = answer.resolve(self.system.show(window, owner, Payload::of(&request)));
            return;
        }
        // Checked on a shell thread; shown back on the owner thread.
        let (checked, check) = self.completions.pending::<ShareResult>();
        let files = request.files.clone();
        self.threads.answer(
            "flui-share",
            check,
            || {
                Err(ShareError::Unsupported(Unsupported::of::<ShareCapability>(
                    UnsupportedReason::Unavailable,
                )))
            },
            move || {
                if files.iter().all(|file| file.is_file()) {
                    Ok(())
                } else {
                    Err(ShareError::NotFound)
                }
            },
        );
        let system = Rc::clone(&self.system);
        let window = window.clone();
        let mut pending = Some((answer, request));
        checked.on_complete(move |outcome| {
            let Some((answer, request)) = pending.take() else {
                return;
            };
            let result = match outcome {
                Ok(Ok(())) if window.lifetime().is_closed() => Err(ShareError::WindowClosed),
                Ok(Ok(())) => system.show(&window, owner, Payload::of(&request)),
                Ok(Err(error)) => Err(error),
                // The check's thread went without answering: neither does
                // the share, and its completion ends as the check's did.
                Err(_) => return,
            };
            let _ = answer.resolve(result);
        });
    }
}

/// One window's registration with its manager.
struct Registration {
    manager: DataTransferManager,
    token: i64,
    _watch: Option<WindowCloseWatch>,
}

impl Drop for Registration {
    fn drop(&mut self) {
        // A manager whose window went refuses; nothing is left to remove.
        let _ = self.manager.RemoveDataRequested(self.token);
    }
}

/// The registrations of one UI runtime's windows.
#[derive(Default)]
pub(crate) struct ShareRegistrations(RefCell<FxHashMap<WindowId, Registration>>);

impl ShareRegistrations {
    /// Keep `registration` for `window`, dropping the one before it after
    /// the borrow ends.
    fn replace(&self, window: WindowId, registration: Registration) {
        let replaced = self.0.borrow_mut().insert(window, registration);
        drop(replaced);
    }

    fn remove(&self, window: WindowId) {
        let removed = self.0.borrow_mut().remove(&window);
        drop(removed);
    }
}

/// The real share sheet.
struct Sheet {
    registrations: Rc<ShareRegistrations>,
    /// The runtime's shell thread count, which resolving the files takes a
    /// place in.
    pool: Arc<ShellPool>,
}

impl ShareSystem for Sheet {
    fn show(&self, window: &WindowRef, owner: Owner, payload: Payload) -> ShareResult {
        let unsupported =
            |reason| ShareError::Unsupported(Unsupported::of::<ShareCapability>(reason));
        let interop = windows::core::factory::<DataTransferManager, IDataTransferManagerInterop>()
            .map_err(|_| unsupported(UnsupportedReason::NotOnThisDevice))?;
        // SAFETY: the window is this thread's, alive (its lifetime is open),
        // and the interop asks for the manager interface it returns.
        let manager: DataTransferManager = unsafe { interop.GetForWindow(owner.hwnd()) }
            .map_err(|error| ShareError::Failed(failure(&error)))?;
        let payload = Arc::new(payload);
        let pool = Arc::clone(&self.pool);
        let handler = TypedEventHandler::<DataTransferManager, DataRequestedEventArgs>::new(
            move |_, args| {
                os_entry(
                    "the share sheet's data request",
                    (),
                    AfterPanic::Answer(|()| Ok(())),
                    || {
                        if let Some(args) = args.as_ref() {
                            fill(&payload, &pool, args);
                        }
                        Ok(())
                    },
                )
            },
        );
        let token = manager
            .DataRequested(&handler)
            .map_err(|error| ShareError::Failed(failure(&error)))?;
        let id = window.id();
        let closing: Weak<ShareRegistrations> = Rc::downgrade(&self.registrations);
        let watch = window.lifetime().on_close(move || {
            if let Some(registrations) = closing.upgrade() {
                registrations.remove(id);
            }
        });
        self.registrations.replace(
            id,
            Registration {
                manager,
                token,
                _watch: Some(watch),
            },
        );
        // SAFETY: as above; the call shows the sheet for this thread's window.
        unsafe { interop.ShowShareUIForWindow(owner.hwnd()) }
            .map_err(|error| ShareError::Failed(failure(&error)))
    }
}

/// Fill the package the sheet asks for. Text and a link go in at once;
/// files are resolved on a shell thread under the request's deferral, since
/// a `StorageFile` is found asynchronously.
fn fill(payload: &Arc<Payload>, pool: &Arc<ShellPool>, args: &DataRequestedEventArgs) {
    let Ok(request) = args.Request() else {
        return;
    };
    let Ok(data) = request.Data() else {
        return;
    };
    if let Ok(properties) = data.Properties() {
        let _ = properties.SetTitle(&HSTRING::from(payload.title.as_str()));
    }
    if let Some(text) = &payload.text {
        let _ = data.SetText(&HSTRING::from(text.as_str()));
    }
    if let Some((uri, web)) = &payload.uri
        && let Ok(uri) = Uri::CreateUri(&HSTRING::from(uri.as_str()))
    {
        let _ = if *web {
            data.SetWebLink(&uri)
        } else {
            data.SetApplicationLink(&uri)
        };
    }
    if payload.files.is_empty() {
        return;
    }
    let Ok(deferral) = request.GetDeferral() else {
        return;
    };
    let failing = request.clone();
    let payload = Arc::clone(payload);
    // The thread only waits on WinRT's asynchronous calls and pumps no
    // messages: the multithreaded apartment.
    let spawned = pool.spawn(
        "flui-share-files",
        ApartmentKind::MultiThreaded,
        move || {
            let items = payload
                .files
                .iter()
                .map(|path| {
                    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))?
                        .join()?;
                    file.cast::<IStorageItem>().map(Some)
                })
                .collect::<windows::core::Result<Vec<Option<IStorageItem>>>>();
            match items {
                Ok(items) => {
                    let _ = data.SetStorageItems(&IIterable::<IStorageItem>::from(items), true);
                }
                Err(_) => {
                    let _ = request
                        .FailWithDisplayText(&HSTRING::from("A shared file could not be read."));
                }
            }
            let _ = deferral.Complete();
        },
    );
    if spawned.is_err() {
        // Every shell thread of the runtime is busy: the sheet says so
        // rather than sharing nothing in silence.
        let _ = failing.FailWithDisplayText(&HSTRING::from(
            "The files could not be shared now; try again.",
        ));
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use flui_platform::UnsupportedReason;
    use flui_platform::capability::{Abandoned, WindowRef};
    use flui_platform::launch::{ExternalUri, Scheme};

    use super::{Payload, ShareSystem, WindowsShare, sheet_title};
    use crate::share::{Share, ShareError, ShareRequest, ShareResult};
    use crate::testing::win32::{TestOwner, win32_window, window};
    use crate::win32::{Owner, ShellThreads};

    #[test]
    fn the_sheet_always_has_a_title_taken_from_what_is_shared() {
        let uri = ExternalUri::try_new("https://example.com/a", &[Scheme::HTTPS]).expect("a link");
        let file = std::env::temp_dir().join("report.pdf");
        let cases = [
            (ShareRequest::new().title("Notes").text("x"), "Notes"),
            (
                ShareRequest::new().uri(uri).text("x"),
                "https://example.com/a",
            ),
            (ShareRequest::new().file(file).text("x"), "report.pdf"),
            (ShareRequest::new().text("Hello"), "Hello"),
        ];
        for (request, title) in cases {
            assert_eq!(sheet_title(&request), title, "{request:?}");
        }
    }

    /// A share sheet that records what it was shown.
    #[derive(Default)]
    struct ShownSheets(RefCell<Vec<Payload>>);

    impl ShareSystem for Rc<ShownSheets> {
        fn show(&self, _: &WindowRef, _: Owner, payload: Payload) -> ShareResult {
            self.0.borrow_mut().push(payload);
            Ok(())
        }
    }

    fn shared(owner: &TestOwner, share: &Share, request: ShareRequest) -> Option<ShareResult> {
        let landed: Rc<RefCell<Option<Result<ShareResult, Abandoned>>>> = Rc::default();
        let into = Rc::clone(&landed);
        share
            .share(request)
            .on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
        owner.turn_until(|| landed.borrow().is_some());
        landed.borrow_mut().take().expect("landed").ok()
    }

    #[test]
    fn a_share_reaches_the_sheet_once_its_files_are_found_off_the_owner() {
        let owner = TestOwner::new();
        let (win, _closer) = win32_window(0x51);
        let sheets = Rc::new(ShownSheets::default());
        let ctx = owner.ctx(Some(win));
        let share = Share::try_new(
            WindowsShare::over(
                Rc::new(Rc::clone(&sheets)),
                Rc::new(ShellThreads::default()),
                &ctx,
            ),
            &ctx,
        )
        .expect("a window");
        let link = ExternalUri::try_new("mailto:a@example.com", &[Scheme::MAILTO]).expect("a link");
        assert_eq!(
            shared(&owner, &share, ShareRequest::new().text("Hi").uri(link)),
            Some(Ok(()))
        );
        assert_eq!(
            sheets.0.borrow()[0],
            Payload {
                title: "mailto:a@example.com".to_owned(),
                text: Some("Hi".to_owned()),
                uri: Some(("mailto:a@example.com".to_owned(), false)),
                files: Vec::new(),
            },
            "a mail link is an application's link, not a web link"
        );

        let missing = std::env::temp_dir().join("flui-system-share-missing.pdf");
        assert_eq!(
            shared(&owner, &share, ShareRequest::new().file(&missing)),
            Some(Err(ShareError::NotFound))
        );
        assert_eq!(sheets.0.borrow().len(), 1, "a missing file shows no sheet");

        let present =
            std::env::temp_dir().join(format!("flui-system-share-{}.txt", std::process::id()));
        std::fs::write(&present, b"x").expect("a file to share");
        assert_eq!(
            shared(&owner, &share, ShareRequest::new().file(&present)),
            Some(Ok(()))
        );
        assert_eq!(sheets.0.borrow()[1].files, std::slice::from_ref(&present));
        let _ = std::fs::remove_file(&present);
    }

    #[test]
    fn a_window_without_a_win32_handle_has_no_sheet_to_attach_to() {
        let owner = TestOwner::new();
        let (win, _closer) = window();
        let ctx = owner.ctx(Some(win));
        let share = Share::try_new(
            WindowsShare::over(
                Rc::new(Rc::new(ShownSheets::default())),
                Rc::new(ShellThreads::default()),
                &ctx,
            ),
            &ctx,
        )
        .expect("a window");
        let refused = shared(&owner, &share, ShareRequest::new().text("Hi"));
        assert!(matches!(
            refused,
            Some(Err(ShareError::Unsupported(unsupported)))
                if unsupported.reason == UnsupportedReason::NoWindow
        ));
    }
}
