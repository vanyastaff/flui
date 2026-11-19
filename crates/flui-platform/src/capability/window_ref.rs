//! The window a provider sees: its id, its scale, its raw handles, whether it
//! is still open, and how a desktop portal names it.

use std::{fmt, rc::Rc, rc::Weak, sync::Arc};

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

use super::{PortalParent, Resolver, WindowLifetime};
use crate::{PlatformWindow, SurfaceTarget, WindowId};

/// The acquiring widget's window, as a provider sees it: its id, its scale,
/// its raw window and display handles, its [`WindowLifetime`], and the
/// [`PortalParent`] a desktop portal names it by. No other method of the
/// window is reachable, so no window method becomes surface that capability
/// crates depend on (ADR-0152 §2, item 6; ADR-0365).
///
/// The raw handles are what owner-attached dialogs need; they are reached
/// through [`HasWindowHandle`] and [`HasDisplayHandle`], the upstream traits
/// ADR-0089 §2 allows in a stable signature.
///
/// Owner-thread (`!Send`), like the provider context it comes from.
#[derive(Clone)]
pub struct WindowRef {
    id: WindowId,
    scale: f64,
    target: Arc<dyn SurfaceTarget>,
    lifetime: WindowLifetime,
    /// Asked for the portal parent; the reference does not keep the window
    /// alive.
    window: Weak<dyn PlatformWindow>,
}

impl WindowRef {
    /// `window` as its providers see it, open for as long as `lifetime` says:
    /// its id, its scale now (`1.0` before it has metrics) and its surface's
    /// handles.
    #[must_use]
    pub fn new(window: &Rc<dyn PlatformWindow>, lifetime: WindowLifetime) -> Self {
        let scale = window
            .state()
            .metrics
            .map_or(1.0, |metrics| metrics.scale().get());
        Self {
            id: window.id(),
            scale,
            target: window.surface_target(),
            lifetime,
            window: Rc::downgrade(window),
        }
    }

    /// The window's id.
    #[must_use]
    pub fn id(&self) -> WindowId {
        self.id
    }

    /// Device pixels per logical pixel when the provider was asked.
    #[must_use]
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// Whether the window is still open, and the notice when it closes: what
    /// a provider that answers the window later ends its work by.
    #[must_use]
    pub fn lifetime(&self) -> &WindowLifetime {
        &self.lifetime
    }

    /// Ask how a desktop portal names this window, for one request: `answer`
    /// receives the [`PortalParent`] the request passes as `parent_window`
    /// and holds until it ends.
    ///
    /// A closed window, or one its host has let go, answers
    /// [`PortalParent::none`] at once: its id may name another window by
    /// then. An open window answers as its host does
    /// ([`PlatformWindow::portal_parent`]): an X11 window at once, a Wayland
    /// window once the compositor exported it. Owner thread only.
    pub fn portal_parent(&self, answer: Resolver<PortalParent>) {
        match self.window.upgrade() {
            Some(window) if !self.lifetime.is_closed() => window.portal_parent(answer),
            // The owner retiring first leaves nothing waiting for the answer.
            _ => {
                let _ = answer.resolve(PortalParent::none());
            }
        }
    }
}

impl HasWindowHandle for WindowRef {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.target.window_handle()
    }
}

impl HasDisplayHandle for WindowRef {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        self.target.display_handle()
    }
}

impl fmt::Debug for WindowRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowRef")
            .field("id", &self.id)
            .field("scale", &self.scale)
            .field("closed", &self.lifetime.is_closed())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    use super::WindowRef;
    use crate::PlatformWindow;
    use crate::capability::{
        CompletionSource, OwnerJob, OwnerPoster, OwnerScope, PendingCompletions, PortalParent,
        WindowCloser,
    };
    use crate::testing::MockWindow;

    /// A window whose host names it `x11:2a` whenever it is asked, closed
    /// or not: what the reference itself must refuse to hand on.
    fn named() -> Rc<dyn PlatformWindow> {
        Rc::new(
            MockWindow::new().with_portal_parent(|| {
                PortalParent::x11(NonZeroU32::new(0x2a).expect("a window id"))
            }),
        )
    }

    /// What `window` answers one ask, on an owner of the test's own.
    fn ask(window: &WindowRef) -> String {
        let pending = PendingCompletions::new();
        let posted: Arc<Mutex<Vec<OwnerJob>>> = Arc::default();
        let sink = Arc::clone(&posted);
        let poster = OwnerPoster::new(move |job| {
            sink.lock().expect("jobs").push(job);
            Ok(())
        });
        let (mut parent, answer) =
            CompletionSource::new(poster, pending.clone()).pending::<PortalParent>();
        window.portal_parent(answer);
        let jobs = std::mem::take(&mut *posted.lock().expect("jobs"));
        for job in jobs {
            job.run(&OwnerScope::new(&pending));
        }
        parent
            .poll()
            .expect("answered at once")
            .expect("not abandoned")
            .as_str()
            .to_owned()
    }

    #[test]
    fn a_closed_window_is_named_to_no_portal_whatever_its_host_answers() {
        let window = named();
        let (closer, lifetime) = WindowCloser::new();
        let reference = WindowRef::new(&window, lifetime);
        assert_eq!(ask(&reference), "x11:2a", "an open window is asked");
        closer.close();
        assert_eq!(
            ask(&reference),
            "",
            "a closed window's id may name another window by then"
        );
        drop(window);
        let (_closer, lifetime) = WindowCloser::new();
        let gone = WindowRef::new(&named(), lifetime);
        assert_eq!(ask(&gone), "", "a window its host let go is asked nothing");
    }
}
