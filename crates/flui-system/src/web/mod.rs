//! The web providers: what a page can do for the system services.
//!
//! Each provider reaches its page through the acquiring widget's window
//! (`page::Page`, from the canvas the window's raw handle names), never
//! through the browser's globals, so a widget in no window, or in a window
//! that is not a canvas, is refused with `NoWindow`.
//!
//! - The launcher opens URIs (`launcher`); the file dialogs pick and save
//!   files (`dialogs`); the launcher entry is the Badging API's count on an
//!   installed application's icon (`badge`).
//! - Autostart, global shortcuts and the user's folders are not a page's: a
//!   page runs while it is open, hears keys only while it has focus, and the
//!   browser keeps the user's folders from it. They answer
//!   `NotOnThisPlatform`.
//!
//! A browser admits a picker and a new window only while the page handles
//! a user's gesture (transient activation). A call outside one is refused
//! with `UnsupportedReason::Unavailable` before the browser is asked, as the
//! page's vibration is: trying again from a gesture may succeed. A save
//! outside a gesture downloads, which needs none.
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
#[expect(unsafe_code, reason = "the canvas a web window's raw handle points at")]
mod page;
mod rules;
#[cfg(all(test, target_arch = "wasm32"))]
#[expect(
    unsafe_code,
    reason = "a test window's handle names a canvas of the page"
)]
mod tests;

#[cfg(target_arch = "wasm32")]
pub(crate) use registration::register;

#[cfg(target_arch = "wasm32")]
mod registration {
    use std::rc::Rc;

    use flui_platform::capability::{Capability, CapabilityRegistrar, ProviderContext};
    use flui_platform::launch::{Launcher, LauncherCapability};
    use flui_platform::{Unsupported, UnsupportedReason};

    use super::badge::WebBadge;
    use super::dialogs::{PagePicker, WebDialogs};
    use super::launcher::WebLauncher;
    use super::page::Page;
    use crate::autostart::AutostartCapability;
    use crate::dialog::{FileDialogCapability, FileDialogs};
    use crate::shortcuts::GlobalShortcutCapability;
    use crate::taskbar::{TaskbarCapability, TaskbarEntry};
    use crate::user_dirs::UserDirCapability;

    /// The page of the acquiring widget's window, refused as `C` with
    /// `NoWindow` when there is none.
    fn page<C: Capability>(ctx: &ProviderContext<'_>) -> Result<Rc<Page>, Unsupported> {
        ctx.window()
            .as_ref()
            .and_then(Page::of)
            .map(Rc::new)
            .ok_or(Unsupported::of::<C>(UnsupportedReason::NoWindow))
    }

    /// Provide `C` as a refusal: a page has no such thing.
    fn not_a_page<C: Capability>(registrar: &mut CapabilityRegistrar) {
        registrar.provide::<C>(|_: &ProviderContext<'_>| {
            Err::<C::Handle, _>(Unsupported::of::<C>(UnsupportedReason::NotOnThisPlatform))
        });
    }

    /// Register the web providers of every service.
    pub(crate) fn register(registrar: &mut CapabilityRegistrar) {
        registrar.provide::<LauncherCapability>(|ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(Launcher::new(
                WebLauncher::new(page::<LauncherCapability>(ctx)?),
                ctx,
            ))
        });
        registrar.provide::<FileDialogCapability>(|ctx: &ProviderContext<'_>| {
            FileDialogs::try_new(
                WebDialogs::new(
                    page::<FileDialogCapability>(ctx)?,
                    ctx.runtime_local::<PagePicker>(),
                ),
                ctx,
            )
        });
        registrar.provide::<TaskbarCapability>(|ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(TaskbarEntry::new(
                WebBadge::try_new(&page::<TaskbarCapability>(ctx)?)?,
                ctx,
            ))
        });
        not_a_page::<AutostartCapability>(registrar);
        not_a_page::<GlobalShortcutCapability>(registrar);
        not_a_page::<UserDirCapability>(registrar);
    }
}
