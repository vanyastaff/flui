//! A slot's surface, display, visibility and size: the surface's restore,
//! the changes sent ahead of a frame and the deadlines they owe.

use std::sync::Arc;

use web_time::Instant;

use super::shared_host::resume_unless_unwinding;
use super::{BeforeFrame, Placed, RasterHost, RestorePace, device_retry};

use flui_engine::{
    EngineError, RasterCommand, SendReceipt, SlotChange, SlotSize, SlotVisibility, SurfaceHold,
};
use flui_foundation::geometry::DeviceSize;
use flui_foundation::panic::FirstPanic;
use flui_foundation::{PresentationAddress, SurfaceGeneration};
use flui_platform::DisplayColor;

impl RasterHost {
    /// Follow the display `address`'s window is on: a changed colour profile
    /// sets its surface up again before the next frame. Answers whether it
    /// changed, so the caller repaints in full.
    pub(crate) fn follow_display(
        &mut self,
        address: PresentationAddress,
        display: Option<DisplayColor>,
    ) -> bool {
        let Some(placed) = self.placed.as_mut() else {
            return false;
        };
        let Some(slot) = placed.slots.get_mut(&address) else {
            return false;
        };
        let Some(color) = display else {
            return false;
        };
        if slot.display == Some(color) {
            return false;
        }
        // Recorded whether or not it is taken: a send is refused only for a
        // slot that is gone or an executor that died, which asking again
        // cannot change.
        slot.display = Some(color);
        match placed.sender.send(RasterCommand::Configure {
            slot: slot.slot,
            change: SlotChange::Color(color),
        }) {
            Ok(_) => true,
            Err(error) => {
                tracing::warn!(%error, "raster host: a display colour change was refused");
                false
            }
        }
    }

    /// Tell the executor whether `address`'s window can be seen: a slot
    /// that cannot is not drawn, and draws in full when it shows again.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "a hidden tab runs no frames, so the page's slot stays shown"
        )
    )]
    pub(crate) fn set_visibility(
        &mut self,
        address: PresentationAddress,
        visibility: SlotVisibility,
    ) {
        let Some(placed) = self.placed.as_mut() else {
            return;
        };
        let Some(slot) = placed.slots.get_mut(&address) else {
            return;
        };
        if slot.visibility == visibility {
            return;
        }
        slot.visibility = visibility;
        // A window shown again tries for a surface it is owed at once, not
        // at the end of a backoff run up while it could not be seen.
        if visibility == SlotVisibility::Shown {
            slot.restore = RestorePace::default();
        }
        let command = RasterCommand::Configure {
            slot: slot.slot,
            change: SlotChange::Shown(visibility),
        };
        // A configuration is folded into the slot's inbox, never queued, so
        // no full queue refuses it; an executor that stopped, or a slot it
        // no longer holds, draws nothing whatever the window's visibility.
        if let Err(error) = placed.sender.send(command) {
            tracing::warn!(%error, "raster host: a visibility change was refused");
        }
    }

    /// Bring `address`'s slot to `size`, and a surface if the executor
    /// dropped its own, and answer the configuration generation its next
    /// frame is stamped with: `None` while the slot has no size the executor
    /// accepted or no surface.
    ///
    /// The generation is the executor's, read after the size is sent: a
    /// device rebuild or a lost surface moves it without the owner asking,
    /// so a cached one would stamp every later frame stale.
    pub(super) fn configure(
        placed: &mut Placed,
        address: PresentationAddress,
        size: (u32, u32),
    ) -> Option<SurfaceGeneration> {
        let slot = placed.slots.get(&address)?;
        let record = placed.sender.record(slot.slot)?;
        // A size past `i32` is one no device configures: unconfigured.
        let (Ok(width), Ok(height)) = (i32::try_from(size.0), i32::try_from(size.1)) else {
            return None;
        };
        let size = DeviceSize::new(width, height);
        placed
            .slots
            .get_mut(&address)
            .expect("BUG: the slot was found above")
            .wanted = Some(size);
        let slot = placed.slots.get(&address)?;
        // Nothing is drawn at a size the device refused, and asking again
        // is refused the same way: the window waits for another size, or
        // for a rebuilt device, which the record shows pending again.
        if record.surface.size == SlotSize::Refused(size) {
            return None;
        }
        // A surface released by the platform waits for it; one on its way is
        // not asked for again; another attempt waits out its pace.
        if record.surface.hold == SurfaceHold::Held {
            placed
                .slots
                .get_mut(&address)
                .expect("BUG: the slot was found above")
                .restore = RestorePace::default();
        } else if slot.needs_surface(record.surface.hold) {
            if !Self::restore_surface(placed, address, web_time::Instant::now()) {
                return None;
            }
        } else if record.surface.hold != SurfaceHold::Restoring {
            // Released, and the platform has not given it back.
            return None;
        }
        let slot = placed.slots.get(&address)?;
        if record.surface.size.newest() != Some(size) {
            match placed.sender.send(RasterCommand::Configure {
                slot: slot.slot,
                change: SlotChange::Size(size),
            }) {
                Ok(SendReceipt::Configured(_)) => {}
                Ok(
                    SendReceipt::Unconfigured | SendReceipt::Accepted | SendReceipt::Replaced(_),
                ) => {
                    return None;
                }
                Err(error) => {
                    tracing::warn!(%error, "raster host: a window's size was refused");
                    return None;
                }
            }
        }
        let generation = placed.sender.record(slot.slot)?.required_generation;
        (generation != SurfaceGeneration::ZERO).then_some(generation)
    }

    /// Make `address`'s window a new surface and send it to its slot, when
    /// the window's pace allows an attempt at `now`: answers whether one is
    /// on its way. A window whose native surface is not there yet (an
    /// Android `Resume` before its `InitWindow`) is taken as released by the
    /// platform until it reports one again.
    fn restore_surface(placed: &mut Placed, address: PresentationAddress, now: Instant) -> bool {
        // A dead executor restores nothing, whoever asks (the restore pace, a
        // surface the platform gives back, a lent frame): a surface made for
        // it would only be refused and dropped again.
        if placed.sender.health() == flui_engine::ExecutorHealth::Died {
            return false;
        }
        let Some(slot) = placed.slots.get_mut(&address) else {
            return false;
        };
        if !slot.restore.due(now) {
            return false;
        }
        slot.restore.attempted(now);
        let surface = match placed.surfaces.create(Arc::clone(&slot.surface_target)) {
            Ok(surface) => surface,
            Err(EngineError::SurfaceTargetUnavailable { .. }) => {
                slot.available = false;
                slot.restore = RestorePace::default();
                tracing::trace!("raster host: the window has no native surface yet");
                return false;
            }
            Err(error) => {
                tracing::warn!(%error, "raster host: a window's surface could not be made again");
                return false;
            }
        };
        match placed.sender.send(RasterCommand::Restore {
            slot: slot.slot,
            surface,
        }) {
            Ok(_) => true,
            Err(error) => {
                tracing::warn!(%error, "raster host: a window's new surface was refused");
                false
            }
        }
    }

    /// The platform gave `address`'s window a surface (`available`) or is
    /// taking it away. A release is applied before this returns, since the
    /// native window behind the surface may go as soon as the platform's
    /// callback does; a surface given back is made and sent at once, and a
    /// device the executor gave up on is asked to rebuild, the window's
    /// return being the moment to try again. Answers whether the window has a
    /// surface on its way, so its next frame repaints in full.
    #[cfg_attr(
        not(any(
            target_os = "android",
            target_os = "ios",
            all(test, feature = "testing")
        )),
        expect(
            dead_code,
            reason = "only Android and iOS take a window's surface away; no desktop platform does"
        )
    )]
    pub(crate) fn set_surface(&mut self, address: PresentationAddress, available: bool) -> bool {
        let mut caught = FirstPanic::new();
        let restored = self.set_surface_held(address, available, &mut caught);
        // Reached from a platform callback, which may run during an unwind.
        resume_unless_unwinding(caught);
        restored
    }

    pub(super) fn set_surface_held(
        &mut self,
        address: PresentationAddress,
        available: bool,
        caught: &mut FirstPanic,
    ) -> bool {
        let Some(placed) = self.placed.as_mut() else {
            return false;
        };
        let Some(slot) = placed.slots.get_mut(&address) else {
            return false;
        };
        slot.available = available;
        slot.restore = RestorePace::default();
        let id = slot.slot;
        let restoring = if available {
            // A given-up device is replaced before the surface is made: a
            // restore configured on the given-up device would be refused, and
            // the surface is made with the factory of the device's instance.
            let retry = device_retry::retry_given_up(&self.factory, placed, |placed, command| {
                Self::send_with_room(placed, address, command, caught)
            });
            if retry == device_retry::Retry::Sent {
                let _turned = caught.contain(|| Self::turn_for(placed, address));
            }
            Self::restore_surface(placed, address, Instant::now())
        } else {
            // The native window behind the surface goes when the platform's
            // callback returns. An executor that stopped draws nothing more,
            // and a slot it no longer holds has no surface to release.
            let sent =
                Self::send_with_room(placed, address, RasterCommand::Release { slot: id }, caught);
            if let Err(error) = sent {
                tracing::warn!(%error, "raster host: a window's surface release was refused");
            }
            false
        };
        let _turned = caught.contain(|| Self::turn_for(placed, address));
        restoring
            && placed
                .sender
                .record(id)
                .is_some_and(|record| record.surface.hold == SurfaceHold::Held)
    }

    /// Draw `scene`, lent for this call alone, into `address`'s window at
    /// `size`, outside its frames: the development reload plugin's scene.
    /// Answers whether it was shown. The window's next frame draws in full.
    #[cfg_attr(
        all(target_arch = "wasm32", not(all(test, feature = "testing"))),
        expect(dead_code, reason = "the web runner drives no development reload hook")
    )]
    pub(crate) fn render_lent(
        &mut self,
        address: PresentationAddress,
        size: (u32, u32),
        scene: &flui_layer::Scene,
        reset_fonts: bool,
    ) -> bool {
        let Some(placed) = self.placed.as_mut() else {
            return false;
        };
        if Self::configure(placed, address, size).is_none() {
            return false;
        }
        // The size the configuration sent is applied before the scene is.
        let _settled = Self::turn_for(placed, address);
        let Some(slot) = placed.slots.get_mut(&address) else {
            return false;
        };
        // The screen shows a scene no publication described.
        slot.base.forget();
        let id = slot.slot;
        // A panic of the render is the slot's, held for the window's report.
        let mut caught = FirstPanic::new();
        // A lent scene is drawn within this call, which only an inline
        // executor can do; `RasterSlot::lends_scenes` keeps a window on a
        // raster thread from asking.
        let Some(executor) = placed.executor.inline_mut() else {
            tracing::warn!("raster host: a lent scene met a raster thread, which cannot draw it");
            return false;
        };
        let rendered = caught.contain(|| executor.render_lent(id, scene, reset_fonts));
        slot.panic.absorb(caught);
        match rendered {
            Some(Ok(shown)) => shown == flui_engine::PresentDisposition::Presented,
            Some(Err(error)) => {
                tracing::error!(%error, "raster host: the plugin's scene failed to render");
                false
            }
            None => false,
        }
    }

    /// Make `address`'s window the surface it is owed once its pace is due at
    /// `now`, with no frame of its own: the deadline
    /// [`window_deadline`](Self::window_deadline) reports is served by the
    /// wake it schedules, and the attempt moves it on, so a window the
    /// backend keeps refusing does not hand the loop the same past instant
    /// again. Answers whether the window holds a surface again.
    fn restore_due(&mut self, address: PresentationAddress, now: Instant) -> bool {
        let Some(placed) = self.placed.as_mut() else {
            return false;
        };
        let Some(slot) = placed.slots.get(&address) else {
            return false;
        };
        let id = slot.slot;
        let owed = placed
            .sender
            .record(id)
            .is_some_and(|record| slot.surface_on_pace(&record.surface));
        if !owed || !Self::restore_surface(placed, address, now) {
            return false;
        }
        let _settled = Self::turn_for(placed, address);
        placed
            .sender
            .record(id)
            .is_some_and(|record| record.surface.hold == SurfaceHold::Held)
    }

    /// What `address`'s window does before its frame at `now`, with no
    /// frame of its own: a retry of a given-up device the application
    /// asked for (`retry`), a surface it is owed whose pace is due, the
    /// colour of the display it is on, and a turn of the executor, which
    /// asks for a lost device that is due and installs one whose request
    /// answered. Answers what came of it ([`BeforeFrame`]).
    pub(super) fn before_frame(
        &mut self,
        address: PresentationAddress,
        now: Instant,
        display: Option<DisplayColor>,
        retry: bool,
    ) -> BeforeFrame {
        let retry_failed = retry && self.retry_device(address) == device_retry::Retry::Failed;
        let restored = self.restore_due(address, now);
        let changed = self.follow_display(address, display);
        self.service(address);
        let painter_reset = self.follow_painter(address);
        BeforeFrame {
            repaint: changed | restored | painter_reset,
            retry_failed,
        }
    }

    /// Whether the executor let its painter go since `address`'s window last
    /// asked (a device rebuilt, or a render of any window that panicked,
    /// ADR-0305 §7): the recordings the window's last scene was drawn from
    /// went with it, so its frame repaints in full rather than republish a
    /// version it holds for a retry (ADR-0304 §7). Read after the turn, which
    /// may rebuild the device.
    fn follow_painter(&mut self, address: PresentationAddress) -> bool {
        let Some(placed) = self.placed.as_mut() else {
            return false;
        };
        let now = placed.sender.painter_resets();
        placed
            .slots
            .get_mut(&address)
            .is_some_and(|slot| std::mem::replace(&mut slot.painters, now) != now)
    }

    /// The loop's next wake for raster work: the earliest
    /// [`window_deadline`](Self::window_deadline) of the windows whose frames
    /// run (`frames_run`). A window whose frames are off is left out: only
    /// its own frame serves its surface, so its deadline would wake the loop
    /// for a past instant no wake moves; the device's next rebuild is still
    /// counted through every window whose frames run.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "the browser's animation frames wake the page; no deadline does"
        )
    )]
    pub(crate) fn wake_deadline(
        &self,
        frames_run: impl Fn(PresentationAddress) -> bool,
    ) -> Option<Instant> {
        let placed = self.placed.as_ref()?;
        placed
            .slots
            .keys()
            .copied()
            .filter(|&address| frames_run(address))
            .filter_map(|address| self.window_deadline(address))
            .min()
    }

    /// When `address`'s own window may try for the surface it is owed: the
    /// deadline only that window's frame serves.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "the browser's animation frames wake the page; no deadline does"
        )
    )]
    fn surface_retry_at_for(&self, address: PresentationAddress) -> Option<Instant> {
        let placed = self.placed.as_ref()?;
        let slot = placed.slots.get(&address)?;
        let record = placed.sender.record(slot.slot)?;
        slot.surface_on_pace(&record.surface)
            .then_some(slot.restore.next)
            .flatten()
    }

    /// The deadline `address`'s own frame serves, which its frame's dirty
    /// gate counts and the loop wakes for while its frames run
    /// ([`wake_deadline`](Self::wake_deadline)): the device's next turn,
    /// which any window's turn runs, and the next attempt at the surface
    /// this window is owed. Another window's surface deadline is that
    /// window's: counted here, it would render this window on every wake
    /// while the other's frames are off, and never move.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "the browser's animation frames wake the page; no deadline does"
        )
    )]
    pub(crate) fn window_deadline(&self, address: PresentationAddress) -> Option<Instant> {
        // A dead executor rebuilds nothing and restores nothing: a deadline it
        // left in its record would wake the loop at a past instant for good.
        if self.executor_died() {
            return None;
        }
        super::super::host::merge_wake_deadlines(
            self.device_retry_at(),
            self.surface_retry_at_for(address),
        )
    }

    /// Whether the device refused `surface`, the size `address`'s window is
    /// at, and nothing has asked it again since.
    pub(super) fn size_refused(
        placed: &Placed,
        address: PresentationAddress,
        surface: (u32, u32),
    ) -> bool {
        let (Ok(width), Ok(height)) = (i32::try_from(surface.0), i32::try_from(surface.1)) else {
            return false;
        };
        placed
            .slots
            .get(&address)
            .and_then(|slot| placed.sender.record(slot.slot))
            .is_some_and(|record| {
                record.surface.size == SlotSize::Refused(DeviceSize::new(width, height))
            })
    }
}
