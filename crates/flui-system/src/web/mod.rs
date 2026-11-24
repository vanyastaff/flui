//! The web providers: what a page can do for the system services.
//!
//! Each provider reaches its page through the acquiring widget's window
//! (`page::Page`, from the canvas the window's raw handle names), never
//! through the browser's globals, so a widget in no window, or in a window
//! that is not a canvas, is refused with `NoWindow`.
//!
//! - The launcher opens URIs (`launcher`); the file dialogs pick and save
//!   files (`dialogs`); the message box is a modal `<dialog>` beside the
//!   canvas (`message_box`); sharing is the Web Share API (`share`); the
//!   launcher entry is the Badging API's count on an installed application's
//!   icon (`badge`).
//! - Autostart, global shortcuts, the user's folders, the jump list and the
//!   system information are not a page's: a page runs while it is open,
//!   hears keys only while it has focus, has no taskbar button of its own,
//!   and the browser keeps the user's folders and system from it. They
//!   answer `NotOnThisPlatform`.
//!
//! A browser admits a picker, a share and a new window only while the page
//! handles a user's gesture (transient activation). A call outside one is
//! refused with `UnsupportedReason::Unavailable` before the browser is
//! asked, as the page's vibration is, and the browser's own refusal of such
//! a call gets the same answer. A save outside a gesture downloads, which
//! needs none.
//!
//! What a page cannot decide without the DOM is in `rules`, tested on
//! every host.

#[cfg(target_arch = "wasm32")]
mod badge;
#[cfg(target_arch = "wasm32")]
mod dialogs;
#[cfg(target_arch = "wasm32")]
mod launcher;
#[cfg(target_arch = "wasm32")]
mod message_box;
#[cfg(target_arch = "wasm32")]
#[expect(unsafe_code, reason = "the canvas a web window's raw handle points at")]
mod page;
mod rules;
#[cfg(target_arch = "wasm32")]
mod share;
#[cfg(all(test, target_arch = "wasm32"))]
#[expect(
    unsafe_code,
    reason = "a test window's handle names a canvas of the page"
)]
mod tests;

#[cfg(target_arch = "wasm32")]
pub(crate) use services::providers;

#[cfg(target_arch = "wasm32")]
mod services {
    use std::rc::Rc;

    use flui_platform::capability::{Capability, ProviderContext};
    use flui_platform::launch::{Launcher, LauncherCapability};
    use flui_platform::{Unsupported, UnsupportedReason};

    use super::badge::WebBadge;
    use super::dialogs::WebDialogs;
    use super::launcher::WebLauncher;
    use super::message_box::WebMessageBoxes;
    use super::page::Page;
    use super::share::WebShare;
    use crate::dialog::{FileDialogCapability, FileDialogs, MessageBoxCapability, MessageBoxes};
    use crate::providers::{Providers, Provision};
    use crate::share::{Share, ShareCapability};
    use crate::taskbar::{TaskbarCapability, TaskbarEntry};

    /// The page of the acquiring widget's window, refused as `C` with
    /// `NoWindow` when there is none.
    fn page<C: Capability>(ctx: &ProviderContext<'_>) -> Result<Rc<Page>, Unsupported> {
        ctx.window()
            .as_ref()
            .and_then(Page::of)
            .map(Rc::new)
            .ok_or(Unsupported::of::<C>(UnsupportedReason::NoWindow))
    }

    /// The web providers of the services a page has, and the refusals of the
    /// others.
    pub(crate) fn providers() -> Providers {
        Providers {
            launcher: Provision::provided(|ctx: &ProviderContext<'_>| {
                Ok::<_, Unsupported>(Launcher::new(
                    WebLauncher::new(page::<LauncherCapability>(ctx)?),
                    ctx,
                ))
            }),
            file_dialogs: Provision::provided(|ctx: &ProviderContext<'_>| {
                FileDialogs::try_new(WebDialogs::new(page::<FileDialogCapability>(ctx)?), ctx)
            }),
            message_boxes: Provision::provided(|ctx: &ProviderContext<'_>| {
                MessageBoxes::try_new(
                    WebMessageBoxes::new(page::<MessageBoxCapability>(ctx)?),
                    ctx,
                )
            }),
            share: Provision::provided(|ctx: &ProviderContext<'_>| {
                Share::try_new(WebShare::try_new(page::<ShareCapability>(ctx)?)?, ctx)
            }),
            taskbar: Provision::provided(|ctx: &ProviderContext<'_>| {
                Ok::<_, Unsupported>(TaskbarEntry::new(
                    WebBadge::try_new(&page::<TaskbarCapability>(ctx)?)?,
                    ctx,
                ))
            }),
            // A page has no autostart, global shortcut, user's folder, jump
            // list or system information of its own.
            autostart: Provision::Refused(UnsupportedReason::NotOnThisPlatform),
            global_shortcuts: Provision::Refused(UnsupportedReason::NotOnThisPlatform),
            user_dirs: Provision::Refused(UnsupportedReason::NotOnThisPlatform),
            jump_list: Provision::Refused(UnsupportedReason::NotOnThisPlatform),
            os: Provision::Refused(UnsupportedReason::NotOnThisPlatform),
            // A browser's lasting access is a `FileSystemHandle` kept in the
            // page's IndexedDB, an object and not bytes; no provider keeps one.
            grants: Provision::Refused(UnsupportedReason::NotYetOnThisHost),
        }
    }
}
