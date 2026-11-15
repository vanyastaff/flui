//! The escape hatch from a widget (ADR-0254 §8): the capability that hands a
//! widget its window as the core host hands it out, so it reaches the
//! window's per-system view (`win32()`, `appkit()`).

use std::marker::PhantomData;
use std::rc::Rc;

use flui_native::HostWindow;
use flui_platform::capability::{Capability, ProviderContext};
use flui_platform::{Unsupported, UnsupportedReason, WindowId};

/// The window a widget is presented in, as the core host hands it out:
/// `ctx.capability::<NativeWindowCapability>()` hands out a
/// [`NativeWindow`].
///
/// Acquire it in `create_state` and keep the handle in the state; a command
/// run through it is the application's, from a callback or a lifecycle hook,
/// never from `build`. There is no stand-in for it: the views it lends exist
/// only on their operating system, and an application's test on the headless
/// platform meets the absence an application meets wherever no extension
/// applies (the window's `win32()` and `appkit()` answer `None`).
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a capability marker has no values; it is only ever named"
)]
pub enum NativeWindowCapability {}

impl Capability for NativeWindowCapability {
    type Handle = NativeWindow;
    const NAME: &'static str = "native-window";
}

/// A widget's handle on the window it is presented in, by the window's id.
///
/// Owner-thread (`!Send`), cheap to clone, and weak: it does not keep the
/// window alive. [`with`](Self::with) runs a closure on the window, which
/// narrows it to its system's view.
#[derive(Debug, Clone)]
pub struct NativeWindow {
    window: WindowId,
    owner_thread: PhantomData<Rc<()>>,
}

impl NativeWindow {
    /// Run `f` on the window, as its host hands it out, and return what `f`
    /// returns.
    ///
    /// `f` runs on the calling thread, outside every borrow of the runtime,
    /// so it may open or close windows. `None`, and `f` does not run, once
    /// the window has closed or no presentation is drawn in it any more.
    pub fn with<R>(&self, f: impl FnOnce(&dyn HostWindow) -> R) -> Option<R> {
        let window = crate::app::runner::native_window_by_id(self.window)?;
        Some(f(window.as_ref()))
    }
}

/// The built-in provider: the handle names the acquiring widget's window.
///
/// # Errors
/// [`Unsupported`] with [`UnsupportedReason::NoWindow`] for a widget in no
/// window.
pub(crate) fn provide(ctx: &ProviderContext<'_>) -> Result<NativeWindow, Unsupported> {
    let window = ctx
        .window()
        .ok_or(Unsupported::of::<NativeWindowCapability>(
            UnsupportedReason::NoWindow,
        ))?;
    Ok(NativeWindow {
        window: window.id(),
        owner_thread: PhantomData,
    })
}
