//! The share sheet over `DataTransferManager`, reached for a desktop
//! window through `IDataTransferManagerInterop` (Windows 10 and later).
//!
//! On the owner thread, which owns the window: the manager is the window's,
//! the `DataRequested` handler fills the package when the sheet asks for
//! it, and `ShowShareUIForWindow` shows the sheet. A share with files first
//! checks, off the owner thread, that each exists, so a missing file is
//! [`ShareError::NotFound`] rather than an empty sheet; the sheet then
//! takes the files as `StorageFile`s, which the handler resolves on a
//! thread of its own under the request's deferral. One registration per
//! window, replaced by the next share and removed when the window closes.

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

use super::threads::Apartment;
use super::{Owner, failure};
use crate::share::{ShareBackend, ShareCapability, ShareError, ShareRequest, ShareResult};

/// What a share hands the sheet, owned by its handler.
struct Payload {
    title: String,
    text: Option<String>,
    uri: Option<(String, bool)>,
    files: Vec<PathBuf>,
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

/// [`ShareBackend`] over the share sheet.
pub(crate) struct WindowsShare {
    registrations: Rc<ShareRegistrations>,
    completions: CompletionSource,
}

impl WindowsShare {
    pub(crate) fn new(ctx: &ProviderContext<'_>) -> Self {
        Self {
            registrations: ctx.runtime_local::<ShareRegistrations>(),
            completions: ctx.completions().clone(),
        }
    }
}

impl ShareBackend for WindowsShare {
    fn share(&self, window: &WindowRef, request: ShareRequest, answer: Resolver<ShareResult>) {
        if request.files.is_empty() {
            let _ = answer.resolve(show(&self.registrations, window, &request));
            return;
        }
        // Checked off the owner thread; shown back on it.
        let (checked, check) = self.completions.pending::<ShareResult>();
        let files = request.files.clone();
        let spawned = std::thread::Builder::new()
            .name("flui-share".to_owned())
            .spawn(move || {
                let missing = files.iter().any(|file| !file.is_file());
                let _ = check.resolve(if missing {
                    Err(ShareError::NotFound)
                } else {
                    Ok(())
                });
            });
        if spawned.is_err() {
            let unavailable = Unsupported::of::<ShareCapability>(UnsupportedReason::Unavailable);
            let _ = answer.resolve(Err(ShareError::Unsupported(unavailable)));
            return;
        }
        let registrations = Rc::downgrade(&self.registrations);
        let window = window.clone();
        let mut pending = Some((answer, request));
        checked.on_complete(move |outcome| {
            let Some((answer, request)) = pending.take() else {
                return;
            };
            let result = match (outcome, registrations.upgrade()) {
                (Ok(Ok(())), Some(registrations)) if !window.lifetime().is_closed() => {
                    show(&registrations, &window, &request)
                }
                (Ok(Ok(())), Some(_)) => Err(ShareError::WindowClosed),
                (Ok(Err(error)), _) => Err(error),
                // The check's thread or the runtime went: nothing to show.
                (Err(_), _) | (Ok(Ok(())), None) => return,
            };
            let _ = answer.resolve(result);
        });
    }
}

/// Register `request`'s content with `window`'s manager and show the sheet.
fn show(
    registrations: &Rc<ShareRegistrations>,
    window: &WindowRef,
    request: &ShareRequest,
) -> ShareResult {
    let unsupported = |reason| ShareError::Unsupported(Unsupported::of::<ShareCapability>(reason));
    let Some(owner) = Owner::of(window) else {
        return Err(unsupported(UnsupportedReason::NoWindow));
    };
    let interop = windows::core::factory::<DataTransferManager, IDataTransferManagerInterop>()
        .map_err(|_| unsupported(UnsupportedReason::NotOnThisDevice))?;
    // SAFETY: the window is this thread's, alive (its lifetime is open), and
    // the interop asks for the manager interface it returns.
    let manager: DataTransferManager = unsafe { interop.GetForWindow(owner.hwnd()) }
        .map_err(|error| ShareError::Failed(failure(&error)))?;
    let payload = Arc::new(Payload {
        title: sheet_title(request),
        text: request.text.clone(),
        uri: request.uri.as_ref().map(|uri| {
            let scheme = uri.scheme();
            let web = scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http");
            (uri.as_str().to_owned(), web)
        }),
        files: request.files.clone(),
    });
    let handler =
        TypedEventHandler::<DataTransferManager, DataRequestedEventArgs>::new(move |_, args| {
            if let Some(args) = args.as_ref() {
                fill(&payload, args);
            }
            Ok(())
        });
    let token = manager
        .DataRequested(&handler)
        .map_err(|error| ShareError::Failed(failure(&error)))?;
    let id = window.id();
    let closing: Weak<ShareRegistrations> = Rc::downgrade(registrations);
    let watch = window.lifetime().on_close(move || {
        if let Some(registrations) = closing.upgrade() {
            registrations.remove(id);
        }
    });
    registrations.replace(
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

/// Fill the package the sheet asks for. Text and a link go in at once;
/// files are resolved on a thread of their own under the request's
/// deferral, since a `StorageFile` is found asynchronously.
fn fill(payload: &Arc<Payload>, args: &DataRequestedEventArgs) {
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
    let payload = Arc::clone(payload);
    let spawned = std::thread::Builder::new()
        .name("flui-share-files".to_owned())
        .spawn(move || {
            let _apartment = Apartment::enter();
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
        });
    // A thread that could not start drops the deferral uncompleted, which
    // ends the request with no files shared.
    drop(spawned);
}

#[cfg(test)]
mod tests {
    use flui_platform::launch::{ExternalUri, Scheme};

    use super::sheet_title;
    use crate::share::ShareRequest;

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
}
