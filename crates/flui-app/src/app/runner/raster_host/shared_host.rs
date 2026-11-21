//! The loop's handle on the host: [`SharedRasterHost`] runs work on the
//! [`RasterHost`] when it is free and keeps what met it busy until the turn
//! that holds it ends. Work deferred is never dropped, and its first panic is
//! held for a window's wake to resume.

use std::cell::RefCell;
use std::rc::Rc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use super::thread_link::ThreadLink;
use super::{RasterFactory, RasterHost};

use flui_engine::RasterPlacement;
use flui_foundation::PresentationAddress;
use flui_foundation::panic::FirstPanic;
#[cfg(not(target_arch = "wasm32"))]
use flui_platform::SurfaceTarget;

/// The loop's raster host, owner-local (ADR-0323 §1).
///
/// The host is busy while it turns the executor, which runs code that is not
/// ours (a tracing subscriber, a scene's or target's `Drop`). Work that
/// finds it busy and must not be lost (a window's detach, a memory trim) is
/// kept here and done when the turn that holds the host ends.
#[derive(Clone)]
pub(crate) struct SharedRasterHost {
    host: Rc<RefCell<RasterHost>>,
    deferred: Rc<Deferred>,
    /// Where the executor turns, read by each frame's sink for its in-flight
    /// limit.
    pub(super) placement: RasterPlacement,
    /// Scratch for the settlements a window's wake drains, kept between
    /// wakes.
    #[cfg_attr(
        all(
            any(target_os = "android", target_os = "ios", target_arch = "wasm32"),
            not(all(test, feature = "testing"))
        ),
        expect(dead_code, reason = "only the desktop loop turns a raster thread")
    )]
    pub(super) settled: Rc<RefCell<Vec<flui_runtime::sink::FrameSettlement>>>,
    /// The raster thread and its slots, shared with the host, so a window's
    /// detach that meets the host busy still waits for the thread before
    /// the window goes.
    #[cfg(not(target_arch = "wasm32"))]
    link: Rc<RefCell<ThreadLink>>,
}

impl Default for SharedRasterHost {
    /// The application's GPU context, turned inline: the host every runner
    /// but the desktop loop keeps.
    fn default() -> Self {
        Self::placed_on(RasterFactory::default(), RasterPlacement::default())
    }
}

/// Work that met a busy host.
#[derive(Default)]
pub(super) struct Deferred {
    detach: RefCell<Vec<PresentationAddress>>,
    /// Surfaces the platform released or gave back, in the order it did.
    surfaces: RefCell<Vec<(PresentationAddress, bool)>>,
    trim: std::cell::Cell<Option<flui_foundation::memory::MemoryPressureLevel>>,
    /// The first panic of that work, held until a window's wake resumes it
    /// ([`SharedRasterHost::take_deferred_panic`]) instead of in place of
    /// the answer of whatever ran before the work.
    panic: RefCell<FirstPanic>,
}

/// Resume `caught`, or, on a thread already unwinding (a window dropped by
/// another panic), say it before `FirstPanic` discards it.
pub(super) fn resume_unless_unwinding(caught: FirstPanic) {
    if std::thread::panicking() {
        if let Some(payload) = caught.into_payload() {
            tracing::error!(
                panic =
                    flui_foundation::panic::payload_text(&*payload).unwrap_or("<non-text payload>"),
                "raster host: a GPU panic met another panic's unwind"
            );
            flui_foundation::panic::discard_payload(payload);
        }
        return;
    }
    caught.resume();
}

impl Drop for Deferred {
    fn drop(&mut self) {
        // The run ended before a wake resumed it: said, not lost silently.
        if let Some(payload) = std::mem::take(self.panic.get_mut()).into_payload() {
            tracing::error!(
                panic =
                    flui_foundation::panic::payload_text(&*payload).unwrap_or("<non-text payload>"),
                "raster host: a closed window's GPU panic outlived the run"
            );
            flui_foundation::panic::discard_payload(payload);
        }
    }
}

impl SharedRasterHost {
    /// A host whose executor will be placed on `factory`'s backend, turned
    /// in `placement`.
    #[cfg_attr(
        all(
            any(target_os = "android", target_os = "ios", target_arch = "wasm32"),
            not(all(test, feature = "testing"))
        ),
        expect(
            dead_code,
            reason = "the mobile and web runners keep the default GPU host; only the desktop loop picks a factory"
        )
    )]
    pub(crate) fn new(factory: RasterFactory, placement: RasterPlacement) -> Self {
        Self::placed_on(factory, placement)
    }

    fn placed_on(factory: RasterFactory, placement: RasterPlacement) -> Self {
        let host = RasterHost::new(factory, placement);
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            link: Rc::clone(&host.link),
            host: Rc::new(RefCell::new(host)),
            deferred: Rc::default(),
            placement,
            settled: Rc::default(),
        }
    }

    /// Run `with` on the host, then the work that met it busy meanwhile;
    /// `None` while the host is turning.
    pub(crate) fn try_with<R>(&self, with: impl FnOnce(&mut RasterHost) -> R) -> Option<R> {
        let mut host = self.host.try_borrow_mut().ok()?;
        let answer = with(&mut host);
        self.run_deferred(&mut host);
        // A raster thread's detaches are waited for with the host released,
        // and so is the wake a death asks of every window; a panic either
        // meets is held as the deferred work's is.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let detaching = std::mem::take(&mut host.detaching);
            let death_wake = host.death_wake.take();
            drop(host);
            let mut caught = FirstPanic::new();
            let mut wedged = death_wake.is_some();
            for wait in detaching {
                wedged |= !wait.wait(&mut caught);
            }
            if wedged {
                self.keep_every_window(&mut caught);
            }
            if let Some(redraws) = death_wake {
                let _woken = caught.contain(|| redraws.wake());
            }
            self.hold_deferred(caught);
        }
        Some(answer)
    }

    /// Ask every attached window's host to keep its native window past its
    /// close: the raster thread is wedged (it did not answer a detach or an
    /// attach), and a wedged thread may
    /// still present over any surface it holds. Asked while the windows are
    /// open, so a close that comes later, by any path, already finds it
    /// (ADR-0305 §9). A host's panic is held in `caught`, and every other
    /// window is still asked.
    #[cfg(not(target_arch = "wasm32"))]
    fn keep_every_window(&self, caught: &mut FirstPanic) {
        // Read, then let go of the link before any host code runs.
        let Ok(link) = self.link.try_borrow() else {
            return;
        };
        let targets: Vec<Arc<dyn SurfaceTarget>> = link
            .slots
            .values()
            .map(|(_, target)| Arc::clone(target))
            .collect();
        drop(link);
        for target in targets {
            let _kept = caught.contain(|| target.keep_past_close());
        }
    }

    /// Read the host; `None` while it is turning.
    pub(crate) fn try_read<R>(&self, read: impl FnOnce(&RasterHost) -> R) -> Option<R> {
        let host = self.host.try_borrow().ok()?;
        Some(read(&host))
    }

    /// Release `address`'s slot now, or when the turn that holds the host
    /// ends.
    pub(crate) fn detach(&self, address: PresentationAddress) {
        if self.try_with(|host| host.detach(address)).is_none() {
            self.deferred.detach.borrow_mut().push(address);
            // A raster thread draws on concurrently, and the window goes when
            // this returns: its slot comes down first, and the host forgets it
            // when the turn that holds it ends.
            #[cfg(not(target_arch = "wasm32"))]
            {
                // Read, then let go of the link before the wait, which removes
                // the slot from it.
                let wait = self
                    .link
                    .try_borrow()
                    .ok()
                    .and_then(|link| link.detach_wait(address));
                if let Some(wait) = wait {
                    // The slot it took down is not the link's any more: the
                    // host's deferred detach then expects the thread to answer
                    // it stale, and still says so of any other slot.
                    let mut caught = FirstPanic::new();
                    if wait.wait(&mut caught) {
                        if let Ok(mut link) = self.link.try_borrow_mut() {
                            let _forgotten = link.slots.remove(&address);
                        }
                    } else {
                        self.keep_every_window(&mut caught);
                    }
                    self.hold_deferred(caught);
                }
            }
            return;
        }
        // A detach answers nothing, so the deferred work it ran resumes its
        // panic here: the window it closed may have been the last to wake.
        resume_unless_unwinding(self.take_deferred_panic());
    }

    /// The platform gave `address`'s window a surface (`available`) or is
    /// taking it away ([`RasterHost::set_surface`]), now, or when the turn
    /// that holds the host ends; a release then lands after the platform's
    /// callback returned, which only a platform callback re-entering the GPU
    /// work can cause. Answers whether the window's next frame repaints in
    /// full: a surface given back while the host is busy is taken as on its
    /// way.
    #[cfg_attr(
        not(any(target_os = "android", target_os = "ios")),
        expect(
            dead_code,
            reason = "only Android and iOS take a window's surface away; no desktop platform does"
        )
    )]
    pub(crate) fn set_surface(&self, address: PresentationAddress, available: bool) -> bool {
        if let Some(restoring) = self.try_with(|host| host.set_surface(address, available)) {
            // A surface change answers nothing the deferred work can take
            // the place of, as a detach does.
            resume_unless_unwinding(self.take_deferred_panic());
            return restoring;
        }
        tracing::warn!("raster host: a window's surface change met the host turning");
        self.deferred
            .surfaces
            .borrow_mut()
            .push((address, available));
        available
    }

    /// Trim the GPU memory behind every slot now, or when the turn that
    /// holds the host ends; the strongest of the trims asked meanwhile.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "no browser reports memory pressure to the page")
    )]
    pub(crate) fn reclaim(&self, level: flui_foundation::memory::MemoryPressureLevel) {
        if self.try_with(|host| host.reclaim(level)).is_none() {
            let strongest = self
                .deferred
                .trim
                .get()
                .map_or(level, |held| held.max(level));
            self.deferred.trim.set(Some(strongest));
            return;
        }
        // A trim answers nothing, as a detach does.
        resume_unless_unwinding(self.take_deferred_panic());
    }

    /// Take the first panic of the work that met the host busy, for a
    /// window's wake to resume once it has nothing left to answer.
    pub(crate) fn take_deferred_panic(&self) -> FirstPanic {
        self.deferred
            .panic
            .try_borrow_mut()
            .map_or_else(|_| FirstPanic::new(), |mut held| std::mem::take(&mut *held))
    }

    /// Run the work that met the host busy. Each item runs whatever an
    /// earlier one did; the first panic is held for
    /// [`take_deferred_panic`](Self::take_deferred_panic), so it does not
    /// take the place of the answer `try_with` returns.
    fn run_deferred(&self, host: &mut RasterHost) {
        let mut caught = FirstPanic::new();
        loop {
            let surfaces = std::mem::take(&mut *self.deferred.surfaces.borrow_mut());
            let detach = std::mem::take(&mut *self.deferred.detach.borrow_mut());
            let trim = self.deferred.trim.take();
            if surfaces.is_empty() && detach.is_empty() && trim.is_none() {
                break;
            }
            for (address, available) in surfaces {
                let _restoring = host.set_surface_held(address, available, &mut caught);
            }
            for address in detach {
                host.detach_held(address, &mut caught);
            }
            if let Some(level) = trim {
                host.reclaim_held(level, &mut caught);
            }
        }
        self.hold_deferred(caught);
    }

    /// Hold `caught` for [`take_deferred_panic`](Self::take_deferred_panic).
    fn hold_deferred(&self, caught: FirstPanic) {
        if let Ok(mut held) = self.deferred.panic.try_borrow_mut() {
            held.absorb(caught);
        } else {
            // Only a payload's own `Drop` runs while it is borrowed.
            resume_unless_unwinding(caught);
        }
    }
}
