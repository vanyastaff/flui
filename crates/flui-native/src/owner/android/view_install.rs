//! `FluiView`'s installation in the Activity's window, as a state machine
//! over the JNI calls it makes ([`ViewHost`]), so its failure exits run in the
//! Linux host's tests as they do on Android.
//!
//! The view names its bridge by a handle, which Java passes back to every
//! native method; the handle may be freed only once no view in the window
//! names it. So every exit that cannot tell whether Java took the handle
//! (a refused installation, one that panicked half way, a `detach` that
//! failed) asks the window for the view that names it ([`ViewHost::withdraw`])
//! and frees the handle only when that answered; otherwise the handle is
//! kept, a small leak, rather than freed under a view that may call with it.
//! Every step runs on the Java main thread; the owner only reads.

use std::fmt;

use parking_lot::Mutex;

/// The JNI calls of the installation. Java main thread only.
pub(crate) trait ViewHost {
    /// A reference to an installed view.
    type View: Clone;
    /// What the view names its bridge by.
    type Handle: Copy;
    /// A failed JNI call.
    type Error: fmt::Display;

    /// A new handle for the bridge.
    fn mint(&self) -> Self::Handle;

    /// Free `handle`, which no view in the window names.
    fn free(&self, handle: Self::Handle);

    /// `FluiView.install`: a view in the window naming `handle`.
    fn install(&self, handle: Self::Handle) -> Result<Self::View, Self::Error>;

    /// `FluiView.detach`: the view names no handle and leaves the window.
    fn detach(&self, view: &Self::View) -> Result<(), Self::Error>;

    /// Detach the view in the window that names `handle`: whether one did.
    fn withdraw(&self, handle: Self::Handle) -> Result<bool, Self::Error>;
}

/// Where the view is in its life.
enum Stage<V, H> {
    /// No view is in the window: the installation has not run, or it
    /// failed and the backend runs without the view.
    Absent,
    /// Java has been handed, or is about to be handed, `H`.
    Installing(H),
    /// The view is in the window and names `handle`.
    Ready { view: V, handle: H },
    /// The platform ended; nothing installs any more.
    Detached,
}

/// What [`ViewInstall::install`] did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Installed<E> {
    /// The view is in the window.
    Ready,
    /// Java refused it; the handle was kept when the window could not say
    /// whether a view names it.
    Refused { error: E, handle_kept: bool },
    /// The platform ended before the installation ran.
    Detached,
}

/// What an exit that let go of a handle did with it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Released {
    /// No view names it any more, and it was freed.
    Freed,
    /// A view may still name it, so it was kept.
    Kept,
}

/// The installation's state; the owner reads it, the Java main thread moves
/// it.
pub(crate) struct ViewInstall<V, H> {
    stage: Mutex<Stage<V, H>>,
}

impl<V: Clone, H: Copy> ViewInstall<V, H> {
    /// No view yet.
    pub(crate) fn new() -> Self {
        Self {
            stage: Mutex::new(Stage::Absent),
        }
    }

    /// The installed view, cloned out of the lock so no JNI call runs under
    /// it.
    pub(crate) fn view(&self) -> Option<V> {
        match &*self.stage.lock() {
            Stage::Ready { view, .. } => Some(view.clone()),
            _ => None,
        }
    }

    /// Install the view, unless the platform ended first. The state is
    /// committed before this returns, so the caller tells the owner and logs
    /// after it.
    pub(crate) fn install<J>(&self, host: &J) -> Installed<J::Error>
    where
        J: ViewHost<View = V, Handle = H>,
    {
        let handle = {
            let mut stage = self.stage.lock();
            if !matches!(*stage, Stage::Absent) {
                return Installed::Detached;
            }
            let handle = host.mint();
            *stage = Stage::Installing(handle);
            handle
        };
        match host.install(handle) {
            Ok(view) => {
                *self.stage.lock() = Stage::Ready { view, handle };
                Installed::Ready
            }
            Err(error) => {
                // Java may have put a view in the window before it threw.
                let released = release(host, handle);
                *self.stage.lock() = Stage::Absent;
                Installed::Refused {
                    error,
                    handle_kept: released == Released::Kept,
                }
            }
        }
    }

    /// The installation stopped short (it panicked, or its work was dropped):
    /// a view Java may have put in the window is taken out and the stage is
    /// `Absent` again. `None` when it had not stopped short: it finished, or
    /// never began.
    pub(crate) fn stopped_short<J>(&self, host: &J) -> Option<Released>
    where
        J: ViewHost<View = V, Handle = H>,
    {
        let handle = {
            let mut stage = self.stage.lock();
            let Stage::Installing(handle) = *stage else {
                return None;
            };
            *stage = Stage::Absent;
            handle
        };
        Some(release(host, handle))
    }

    /// The platform ended: the view leaves the window, and its handle is
    /// freed once it names it no more. `None` when no view was in the window.
    pub(crate) fn detach<J>(&self, host: &J) -> Option<Released>
    where
        J: ViewHost<View = V, Handle = H>,
    {
        let previous = std::mem::replace(&mut *self.stage.lock(), Stage::Detached);
        match previous {
            Stage::Ready { view, handle } => Some(if host.detach(&view).is_ok() {
                host.free(handle);
                Released::Freed
            } else {
                Released::Kept
            }),
            Stage::Installing(handle) => Some(release(host, handle)),
            Stage::Absent | Stage::Detached => None,
        }
    }
}

/// Take out whatever view names `handle` and free it, or keep it when the
/// window could not say.
fn release<J: ViewHost>(host: &J, handle: J::Handle) -> Released {
    match host.withdraw(handle) {
        Ok(_) => {
            host.free(handle);
            Released::Freed
        }
        Err(_) => Released::Kept,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::{Installed, Released, ViewHost, ViewInstall};
    use crate::owner::android::main_thread::{MainThreadWork, PostFailure};

    /// How the stand-in's `FluiView.install` behaves.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Java {
        Installs,
        /// Puts the view in the window, then throws.
        ThrowsAfterAdding,
        /// Throws before any view.
        Refuses,
        /// Puts the view in the window, then the call panics on the way back.
        PanicsAfterAdding,
    }

    /// The Activity's window as the installation sees it.
    #[derive(Default)]
    struct Window {
        java: Cell<Option<Java>>,
        /// The handles the views in the window name.
        views: RefCell<Vec<u32>>,
        freed: RefCell<Vec<u32>>,
        next: Cell<u32>,
        /// Whether the window can be read to withdraw a view.
        readable: Cell<bool>,
        detach_fails: Cell<bool>,
    }

    impl Window {
        fn new(java: Java) -> Rc<Self> {
            let window = Self::default();
            window.java.set(Some(java));
            window.readable.set(true);
            Rc::new(window)
        }
    }

    impl ViewHost for Window {
        type View = u32;
        type Handle = u32;
        type Error = &'static str;

        fn mint(&self) -> u32 {
            self.next.set(self.next.get() + 1);
            self.next.get()
        }

        fn free(&self, handle: u32) {
            assert!(
                !self.views.borrow().contains(&handle),
                "a handle freed while a view names it"
            );
            self.freed.borrow_mut().push(handle);
        }

        fn install(&self, handle: u32) -> Result<u32, &'static str> {
            match self.java.get().expect("a scripted install") {
                Java::Installs => {
                    self.views.borrow_mut().push(handle);
                    Ok(handle)
                }
                Java::ThrowsAfterAdding => {
                    self.views.borrow_mut().push(handle);
                    Err("addView threw")
                }
                Java::Refuses => Err("the class did not load"),
                Java::PanicsAfterAdding => {
                    self.views.borrow_mut().push(handle);
                    panic!("the call panicked after Java added the view");
                }
            }
        }

        fn detach(&self, view: &u32) -> Result<(), &'static str> {
            if self.detach_fails.get() {
                return Err("detach threw");
            }
            self.views.borrow_mut().retain(|named| named != view);
            Ok(())
        }

        fn withdraw(&self, handle: u32) -> Result<bool, &'static str> {
            if !self.readable.get() {
                return Err("the window could not be read");
            }
            let mut views = self.views.borrow_mut();
            let before = views.len();
            views.retain(|named| *named != handle);
            Ok(views.len() != before)
        }
    }

    /// The installation as the Android host posts it: the body installs,
    /// the report takes out what a stopped installation left.
    fn posted(install: &Rc<ViewInstall<u32, u32>>, window: &Rc<Window>) -> Vec<Option<Released>> {
        let reports = Rc::new(RefCell::new(Vec::new()));
        let (body_install, body_window) = (Rc::clone(install), Rc::clone(window));
        let (report_install, report_window, seen) =
            (Rc::clone(install), Rc::clone(window), Rc::clone(&reports));
        MainThreadWork::new(
            "FluiView.install",
            move || {
                let _ = body_install.install(&*body_window);
                Ok(())
            },
            move |_: PostFailure| {
                seen.borrow_mut()
                    .push(report_install.stopped_short(&*report_window));
            },
        )
        .run();
        Rc::try_unwrap(reports)
            .expect("the work let go of its report")
            .into_inner()
    }

    /// Every exit of the installation leaves no view in the window naming a
    /// freed handle, and the view of a stopped installation is taken out.
    #[test]
    fn every_exit_of_the_installation_frees_only_handles_no_view_names() {
        let window = Window::new(Java::Installs);
        let install = Rc::new(ViewInstall::new());
        assert!(posted(&install, &window).is_empty(), "installed: no report");
        assert_eq!(install.view(), Some(1));
        assert_eq!(install.detach(&*window), Some(Released::Freed));
        assert!(window.views.borrow().is_empty());
        assert_eq!(*window.freed.borrow(), [1]);

        let window = Window::new(Java::PanicsAfterAdding);
        let install = Rc::new(ViewInstall::new());
        assert_eq!(
            posted(&install, &window),
            [Some(Released::Freed)],
            "a panic after Java added the view: the report takes it out"
        );
        assert!(
            window.views.borrow().is_empty(),
            "no view left in the window"
        );
        assert_eq!(install.view(), None);
        assert_eq!(install.detach(&*window), None, "nothing left to detach");

        let window = Window::new(Java::PanicsAfterAdding);
        window.readable.set(false);
        let install = Rc::new(ViewInstall::new());
        assert_eq!(
            posted(&install, &window),
            [Some(Released::Kept)],
            "a window that cannot be read keeps the handle"
        );
        assert!(window.freed.borrow().is_empty());

        let window = Window::new(Java::ThrowsAfterAdding);
        let install = ViewInstall::new();
        assert_eq!(
            install.install(&*window),
            Installed::Refused {
                error: "addView threw",
                handle_kept: false,
            }
        );
        assert!(
            window.views.borrow().is_empty(),
            "the refused view is taken out"
        );

        let window = Window::new(Java::Refuses);
        let install = ViewInstall::new();
        assert!(matches!(
            install.install(&*window),
            Installed::Refused {
                handle_kept: false,
                ..
            }
        ));
        assert_eq!(*window.freed.borrow(), [1]);
        assert_eq!(install.stopped_short(&*window), None, "a refusal finished");
    }

    /// A `detach` that fails keeps the handle the view still names, and an
    /// installation after the platform ended installs nothing.
    #[test]
    fn a_failed_detach_keeps_the_handle_and_a_late_installation_installs_nothing() {
        let window = Window::new(Java::Installs);
        let install = ViewInstall::new();
        assert_eq!(install.install(&*window), Installed::Ready);
        window.detach_fails.set(true);
        assert_eq!(install.detach(&*window), Some(Released::Kept));
        assert!(window.freed.borrow().is_empty());

        let window = Window::new(Java::Installs);
        let install = ViewInstall::<u32, u32>::new();
        assert_eq!(install.detach(&*window), None);
        assert_eq!(install.install(&*window), Installed::Detached);
        assert!(window.views.borrow().is_empty());
    }
}
