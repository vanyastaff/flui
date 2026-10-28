//! The Apple windows: the AppKit window (`macos`) and the UIKit scene's
//! window (`ios`), over the pure rules both ask ([`geometry`]).
//!
//! The macOS window keeps its AppKit features beyond the cross-platform window
//! (`MacOSWindowExt`) and the material behind its content beside it.
//!
//! Both windows hand the thread that presents their frames a surface target
//! that keeps the window's content view alive ([`MainRetainedView`]): a GPU
//! surface built from the view may outlive the window, and the view must
//! outlive the surface.

// The view a surface target keeps is retained and released through
// Objective-C, behind the safe `MainRetainedView`.
#![cfg_attr(any(target_os = "macos", target_os = "ios"), expect(unsafe_code))]

pub(crate) mod geometry;
#[cfg(target_os = "ios")]
pub(crate) mod ios;
#[cfg(target_os = "macos")]
pub(crate) mod macos;

/// Where a value of the main thread is let go of when the thread that drops
/// it may be another one: on the main thread, now; elsewhere, in a turn of
/// the main queue.
pub(crate) mod main_release {
    #![forbid(unsafe_code)]

    /// The main thread, as the release asks it.
    pub(crate) trait MainThread {
        /// Whether the calling thread is the main thread.
        fn is_main(&self) -> bool;

        /// Run `release` in a later turn of the main thread.
        fn post(&self, release: Box<dyn FnOnce() + Send>);
    }

    /// Drop `value` on the main thread: at once when the caller is on it,
    /// otherwise in a turn `main` runs there.
    pub(crate) fn release_on_main<T: Send + 'static>(value: T, main: &impl MainThread) {
        if main.is_main() {
            drop(value);
        } else {
            main.post(Box::new(move || drop(value)));
        }
    }

    #[cfg(test)]
    mod tests {
        use std::cell::{Cell, RefCell};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        use super::{MainThread, release_on_main};

        /// A value that counts the times it was dropped.
        struct Counted(Arc<AtomicUsize>);

        impl Drop for Counted {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        /// A main thread whose posted turns queue until the test runs them.
        struct QueuedMain {
            main: bool,
            posted: RefCell<Vec<Box<dyn FnOnce() + Send>>>,
            posts: Cell<usize>,
        }

        impl MainThread for QueuedMain {
            fn is_main(&self) -> bool {
                self.main
            }

            fn post(&self, release: Box<dyn FnOnce() + Send>) {
                self.posts.set(self.posts.get() + 1);
                self.posted.borrow_mut().push(release);
            }
        }

        fn main_thread(main: bool) -> QueuedMain {
            QueuedMain {
                main,
                posted: RefCell::new(Vec::new()),
                posts: Cell::new(0),
            }
        }

        #[test]
        fn a_value_dropped_on_the_main_thread_is_released_at_once() {
            let drops = Arc::new(AtomicUsize::new(0));
            let main = main_thread(true);
            release_on_main(Counted(Arc::clone(&drops)), &main);
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            assert_eq!(
                main.posts.get(),
                0,
                "nothing is posted from the main thread"
            );
        }

        #[test]
        fn a_value_dropped_elsewhere_is_released_in_a_main_turn() {
            let drops = Arc::new(AtomicUsize::new(0));
            let main = main_thread(false);
            release_on_main(Counted(Arc::clone(&drops)), &main);
            assert_eq!(
                drops.load(Ordering::SeqCst),
                0,
                "a value dropped off the main thread is not released there"
            );
            assert_eq!(main.posts.get(), 1);
            for turn in main.posted.borrow_mut().drain(..) {
                turn();
            }
            assert_eq!(drops.load(Ordering::SeqCst), 1, "the main turn releases it");
        }
    }
}

/// How a window is taken apart around the view it owns: each part hooked to
/// the view is detached, then the window is released, then the view goes,
/// each a cleanup boundary of its own. A view a detach failed on is kept for
/// the rest of the process instead: the part may still be hooked to it (the
/// accessibility bridge's dynamic subclass stays on the view until it is
/// unhooked), and the view's `dealloc`, or VoiceOver, would reach what the
/// failed detach left behind.
#[cfg(any(target_os = "macos", test))]
pub(crate) mod teardown {
    #![forbid(unsafe_code)]

    /// Detach each of `hooked`, release the window, then let `view` go if
    /// every detach completed. `guarded` runs one step as a cleanup boundary
    /// and answers whether it completed.
    pub(crate) fn take_apart<V>(
        view: V,
        hooked: &mut [&mut dyn FnMut()],
        release_window: &mut dyn FnMut(),
        guarded: impl Fn(&mut dyn FnMut()) -> bool,
    ) {
        let mut detached = true;
        for detach in hooked.iter_mut() {
            detached &= guarded(&mut **detach);
        }
        guarded(release_window);
        let mut view = Some(view);
        if detached {
            guarded(&mut || drop(view.take()));
        } else {
            std::mem::forget(view);
        }
    }

    #[cfg(test)]
    mod tests {
        use std::cell::RefCell;
        use std::panic::{AssertUnwindSafe, catch_unwind};

        use super::take_apart;

        /// A view that logs its release.
        struct View<'log>(&'log RefCell<Vec<&'static str>>);

        impl Drop for View<'_> {
            fn drop(&mut self) {
                self.0.borrow_mut().push("view");
            }
        }

        fn guarded(step: &mut dyn FnMut()) -> bool {
            catch_unwind(AssertUnwindSafe(step))
                .map_err(std::mem::forget)
                .is_ok()
        }

        fn take_apart_with(log: &RefCell<Vec<&'static str>>, accessibility_fails: bool) {
            take_apart(
                View(log),
                &mut [
                    &mut || {
                        log.borrow_mut().push("accessibility");
                        assert!(!accessibility_fails, "the accessibility detach failed");
                    },
                    &mut || log.borrow_mut().push("text"),
                ],
                &mut || log.borrow_mut().push("window"),
                guarded,
            );
        }

        #[test]
        fn a_view_goes_after_its_window_once_every_part_is_detached() {
            let log = RefCell::new(Vec::new());
            take_apart_with(&log, false);
            assert_eq!(*log.borrow(), ["accessibility", "text", "window", "view"]);
        }

        #[test]
        fn a_view_a_detach_failed_on_is_kept() {
            let log = RefCell::new(Vec::new());
            take_apart_with(&log, true);
            assert_eq!(
                *log.borrow(),
                ["accessibility", "text", "window"],
                "every other step still ran, and the view was not released"
            );
        }
    }
}

/// A content view a surface target keeps alive: retained on the main thread
/// when the target is made, and released on the main thread whichever thread
/// drops the target ([`main_release`]). Its pointer is read on the main
/// thread only.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) struct MainRetainedView(Option<KeptView>);

/// The retained view, moved to the main thread to be released there.
#[cfg(any(target_os = "macos", target_os = "ios"))]
struct KeptView(objc2::rc::Retained<objc2::runtime::AnyObject>);

// SAFETY: a `KeptView` is a +1 reference to a view of the main thread. Nothing
// messages the view through it: it leaves its thread only to be released, and
// `MainRetainedView::drop` routes that release to the main thread
// (`release_on_main`), the only thread a view may be released on.
#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe impl Send for KeptView {}

// SAFETY: a shared `MainRetainedView` hands out its view's address on the main
// thread only (`MainRetainedView::view` takes a `MainThreadMarker`), and
// releases the view on the main thread only (see `KeptView`): no thread but the
// main thread reaches the view through it.
#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe impl Sync for MainRetainedView {}

#[cfg(any(target_os = "macos", target_os = "ios"))]
impl MainRetainedView {
    /// Keep `view` alive. `None` inside when `view` is nil or the caller is
    /// not on the main thread, where a view is retained.
    ///
    /// # Safety
    ///
    /// `view` is nil or a live Objective-C object.
    pub(crate) unsafe fn retain(view: *mut objc2::runtime::AnyObject) -> Self {
        if objc2::MainThreadMarker::new().is_none() {
            return Self(None);
        }
        // SAFETY: the caller guarantees `view` is nil or live; the retain
        // runs on the main thread.
        Self(unsafe { objc2::rc::Retained::retain(view) }.map(KeptView))
    }

    /// The kept view, on the main thread: `None` when no view was kept.
    pub(crate) fn view(
        &self,
        _main: objc2::MainThreadMarker,
    ) -> Option<std::ptr::NonNull<std::ffi::c_void>> {
        self.0
            .as_ref()
            .map(|kept| std::ptr::NonNull::from(&*kept.0).cast())
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
impl Drop for MainRetainedView {
    fn drop(&mut self) {
        if let Some(kept) = self.0.take() {
            main_release::release_on_main(kept, &AppleMain);
        }
    }
}

/// The main thread of an Apple process and its main queue.
#[cfg(any(target_os = "macos", target_os = "ios"))]
struct AppleMain;

#[cfg(any(target_os = "macos", target_os = "ios"))]
impl main_release::MainThread for AppleMain {
    fn is_main(&self) -> bool {
        objc2::MainThreadMarker::new().is_some()
    }

    fn post(&self, release: Box<dyn FnOnce() + Send>) {
        use crate::owner::panic_boundary::{AfterPanic, os_entry};

        dispatch2::DispatchQueue::main().exec_async(move || {
            os_entry(
                "a surface target's view release",
                (),
                AfterPanic::Answer(|()| ()),
                release,
            );
        });
    }
}
