//! The escape hatch from a widget (ADR-0254 §8): the capability that hands a
//! widget its window as the core host hands it out, so it reaches the
//! window's per-system view (`win32()`, `appkit()`).

use std::rc::{Rc, Weak};

use flui_native::HostWindow;
use flui_platform::capability::{Capability, ProviderContext};
use flui_platform::{Unsupported, UnsupportedReason, WindowId};

/// The window a widget is presented in, as the core host hands it out:
/// `ctx.capability::<NativeWindowCapability>()` hands out a
/// [`NativeWindow`].
///
/// Acquire it in `create_state` and keep the handle in the state; a command
/// run through it is the application's, from a callback or a lifecycle hook,
/// never from `build`. An application's test stands in for the built-in
/// provider with `Application::capability` and a handle on a window of its
/// own ([`NativeWindow::new`]).
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

/// A widget's handle on the window it is presented in.
///
/// Owner-thread (`!Send`), cheap to clone, and weak: it does not keep the
/// window alive. [`with`](Self::with) runs a closure on the window, which
/// narrows it to its system's view.
#[derive(Debug, Clone)]
pub struct NativeWindow {
    window: Target,
}

/// The window a [`NativeWindow`] reaches.
#[derive(Debug, Clone)]
enum Target {
    /// The window of this id while a presentation of the running
    /// application is drawn in it: the built-in provider's.
    Presented(WindowId),
    /// A window the handle was given, while something else keeps it: a
    /// stand-in provider's.
    Given(Weak<dyn HostWindow>),
}

impl NativeWindow {
    /// A handle on `window`, which it reaches while something else keeps it
    /// alive: what a provider that stands in for the built-in one hands out
    /// (`Application::capability`), in an application's test with a window
    /// of a host it opened.
    #[must_use]
    pub fn new(window: &Rc<dyn HostWindow>) -> Self {
        Self {
            window: Target::Given(Rc::downgrade(window)),
        }
    }

    /// Run `f` on the window, as its host hands it out, and return what `f`
    /// returns.
    ///
    /// `f` runs on the calling thread, outside every borrow of the runtime,
    /// so it may open or close windows. `None`, and `f` does not run, once
    /// the window has closed or no presentation is drawn in it any more; for
    /// a handle made by [`new`](Self::new), once nothing keeps the window.
    pub fn with<R>(&self, f: impl FnOnce(&dyn HostWindow) -> R) -> Option<R> {
        let window = match &self.window {
            Target::Presented(id) => crate::app::runner::native_window_by_id(*id)?,
            Target::Given(window) => window.upgrade()?,
        };
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
        window: Target::Presented(window.id()),
    })
}
