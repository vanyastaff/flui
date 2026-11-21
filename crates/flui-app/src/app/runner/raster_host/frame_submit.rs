//! A frame's submission to a slot and the settlement of what the executor
//! answers, inline or from a raster thread.

use std::num::NonZeroUsize;

use super::{PendingFrame, Placed, RasterHost, SlotTurn};

use flui_engine::{RasterCommand, RasterVerdict, SendReceipt, SlotSettlement, SurfaceHold};
use flui_foundation::panic::FirstPanic;
use flui_foundation::{FrameEpoch, FrameStamp, PresentationAddress};
use flui_layer::{DamageRegion, RasterBase, ScenePublication, SceneSnapshot, SurfaceKey};
use flui_runtime::sink::SubmitVerdict;

impl RasterHost {
    /// Send `publication`, the frame `epoch` names in its UI runtime's
    /// sequence, to `address`'s slot, measured on `surface`, turn
    /// the executor and answer what became of the frame. A panic of the
    /// frame is held for the window to report to its UI runtime after the
    /// frame ([`RasterSlot::report_frame_panic`]); the frame is refused
    /// (`SubmitVerdict::Refused`), and the window's next change draws in full.
    /// A size past the device's limit refuses the frame too, until the
    /// platform announces another size.
    pub(super) fn submit(
        &mut self,
        address: PresentationAddress,
        surface: (u32, u32),
        epoch: FrameEpoch,
        publication: ScenePublication,
    ) -> SubmitVerdict {
        let Some(placed) = self.placed.as_mut() else {
            tracing::error!("BUG: raster host: a frame for a window with no executor");
            return SubmitVerdict::Failed;
        };
        // A dead executor takes no frame (ADR-0305 §9): the frame is finished
        // as far as the window can tell, and commits, so input and the agent
        // keep reading the tree while the window draws nothing. Its texture
        // updates are taken into the slot's carry, newest per id, which a
        // replacing executor receives: left in the registry they would keep
        // the window's texture frame owed, a frame every period.
        if placed.sender.health() == flui_engine::ExecutorHealth::Died {
            if let Some(slot) = placed.slots.get_mut(&address)
                && let Some(textures) = &slot.textures
            {
                textures.take_updates_into(&mut slot.carried);
            }
            return SubmitVerdict::NoPresent;
        }
        let Some(generation) = Self::configure(placed, address, surface) else {
            // Past the device's limit, a retry is refused the same way.
            // Otherwise (a zero size, no surface yet) nothing is drawn until
            // the next resize.
            return if Self::size_refused(placed, address, surface) {
                SubmitVerdict::Refused
            } else {
                SubmitVerdict::NotShown
            };
        };
        let slot = placed
            .slots
            .get_mut(&address)
            .expect("BUG: `configure` found the slot");
        let key = SurfaceKey {
            size: surface,
            generation,
        };
        let stamp = FrameStamp::new(address, epoch, generation);
        // The images a replaced context kept go first, so the registry's
        // newer frames replace them, folded into the carried buffer. The
        // buffer rides the snapshot to the raster thread, so a texture frame
        // costs its list here; a frame with no update allocates nothing.
        let mut updates = std::mem::take(&mut slot.carried);
        if let Some(textures) = &slot.textures {
            textures.take_updates_into(&mut updates);
        }
        let mut damage = slot.base.damage_for(&publication, key);
        // A new image or a retirement changes the pixels of the texture's
        // users and of nothing else, whatever the patch damaged (ADR-0304 §9).
        // Damage already full has nothing to add; the union is held to the
        // threshold the patch's damage was.
        if !matches!(damage, DamageRegion::Full(_)) && !updates.is_empty() {
            let textures = updates.frames.iter().map(|(id, _)| *id);
            let users = publication.scene.texture_damage(
                textures.chain(updates.retired.iter().copied()),
                key.size,
                &mut placed.texture_damage,
            );
            damage = slot.base.thresholded(damage.union(users), key.size);
        }
        let shown = publication.scene.id();
        let mut snapshot =
            SceneSnapshot::of_version(stamp, publication.base, damage, publication.scene);
        if !updates.is_empty() {
            snapshot = snapshot.textures(updates);
        }
        let id = slot.slot;
        let receipt = match placed
            .sender
            .send(RasterCommand::Frame { slot: id, snapshot })
        {
            Ok(receipt) => receipt,
            Err(error) => {
                tracing::error!(%error, "raster host: a frame was refused");
                // The texture frames it took from the registry and the carry
                // reached no backend: the window's next frame takes them.
                if let RasterCommand::Frame { snapshot, .. } = error.into_command() {
                    slot.carried = snapshot.textures;
                }
                return SubmitVerdict::Failed;
            }
        };
        let frame = PendingFrame {
            epoch,
            shown,
            key,
            tainted: false,
        };
        if placed.executor.inline().is_none() {
            return Self::queue(placed, address, frame, receipt);
        }
        // The turn runs code that is not ours; its settlements are drained
        // whatever it did, so a panic leaves none for the next frame to trip
        // over. A panic of the turn itself (a retired scene's `Drop`) is held
        // for the window's report and does not change what the frame became.
        let turned = Self::turn_for(placed, address);
        placed.settlements.clear();
        placed.sender.drain_settlements(id, &mut placed.settlements);
        let settled = placed
            .settlements
            .iter()
            .find(|settlement| settlement.epoch == epoch)
            .map(|settlement| settlement.verdict);
        let Some(verdict) = Self::settle_one(placed, address, frame, settled, turned) else {
            let slot = placed
                .slots
                .get_mut(&address)
                .expect("BUG: the window an inline frame settled for holds its slot");
            let _logged = slot.panic.contain(move || {
                tracing::error!(?epoch, "raster host: an inline frame settled nothing");
            });
            return SubmitVerdict::Failed;
        };
        verdict
    }

    /// Hold `frame`, which a raster thread took with `receipt`, until it
    /// settles: answered [`SubmitVerdict::Queued`], or
    /// [`SubmitVerdict::QueuedReplacing`] naming the frame it took the place
    /// of, which never settles and is no longer held (ADR-0305 §4).
    fn queue(
        placed: &mut Placed,
        address: PresentationAddress,
        mut frame: PendingFrame,
        receipt: SendReceipt,
    ) -> SubmitVerdict {
        let slot = placed
            .slots
            .get_mut(&address)
            .expect("BUG: `configure` found the slot");
        let verdict = match receipt {
            SendReceipt::Replaced(replaced) => {
                // The replacement inherits the replaced frame's taint, as it
                // inherits its damage.
                if slot
                    .pending
                    .iter()
                    .any(|held| held.epoch == replaced && held.tainted)
                {
                    frame.tainted = true;
                }
                slot.pending.retain(|held| held.epoch != replaced);
                SubmitVerdict::QueuedReplacing(replaced)
            }
            SendReceipt::Accepted | SendReceipt::Configured(_) | SendReceipt::Unconfigured => {
                SubmitVerdict::Queued
            }
        };
        slot.pending.push(frame);
        verdict
    }

    /// What `frame` of `address`'s window became, once the executor settled
    /// it `settled` in a turn that `turned`, with the slot's base moved to
    /// what the screen now holds; `None` when nothing settled it and nothing
    /// refuses it.
    pub(super) fn settle_one(
        placed: &mut Placed,
        address: PresentationAddress,
        frame: PendingFrame,
        settled: Option<RasterVerdict>,
        turned: SlotTurn,
    ) -> Option<SubmitVerdict> {
        let PendingFrame {
            shown,
            key,
            tainted,
            ..
        } = frame;
        let id = placed.slots.get(&address)?.slot;
        // No surface the GPU takes, or no size it draws at: the frame is
        // refused, and nothing until the window's next change (or a rebuilt
        // device) asks again. Retried, the frame would meet the same refusal.
        let stuck = settled != Some(RasterVerdict::Presented)
            && (placed
                .sender
                .record(id)
                .is_some_and(|record| record.surface.hold == SurfaceHold::Refused)
                || Self::size_refused(placed, address, key.size));
        let slot = placed.slots.get_mut(&address)?;
        if turned == SlotTurn::Panicked {
            let verdict = match settled {
                // The render itself panicked, which the executor settles as
                // a retry. Refused: a renderer that panics on this scene
                // panics on its retry too, and the window's next change
                // draws again.
                Some(RasterVerdict::Retry) | None => SubmitVerdict::Refused,
                // A command before the frame panicked (a resize, an upload)
                // and the frame settled otherwise: what it became stands, so
                // a stale surface is retried and applies what is still
                // pending. Once per surface: another frame on it that meets
                // the panic again (the retry, or one queued behind the first)
                // is the size's or the driver's failure, not the moment's,
                // and the window's next change tries it again.
                Some(RasterVerdict::SurfaceStale) if slot.stale_panic == Some(key) => {
                    SubmitVerdict::Refused
                }
                Some(verdict) => {
                    let verdict = Self::verdict(&mut slot.base, verdict, shown, key);
                    slot.stale_panic = (verdict == SubmitVerdict::SurfaceStale).then_some(key);
                    verdict
                }
            };
            // The image may be torn or miss an upload: the next frame is
            // drawn in full.
            slot.base.forget();
            return Some(verdict);
        }
        match settled {
            // Nothing settled says nothing of the surface.
            None => {}
            // A frame held when its slot's panic was taken may have met the
            // panic and lost its mark: settled stale, it counts as the
            // panicked frame does.
            Some(RasterVerdict::SurfaceStale) if tainted => {
                if slot.stale_panic == Some(key) {
                    slot.base.forget();
                    return Some(SubmitVerdict::Refused);
                }
                slot.stale_panic = Some(key);
            }
            // Any other settlement ends the surface's retry.
            Some(_) => slot.stale_panic = None,
        }
        if stuck {
            slot.base.forget();
            return Some(SubmitVerdict::Refused);
        }
        Some(Self::verdict(&mut slot.base, settled?, shown, key))
    }

    /// Drain what a raster thread settled of `address`'s frames into
    /// `into`, each with the verdict its UI runtime settles it with
    /// (`UiRuntime::settle_frame`). A frame whose turn panicked (its
    /// settlement's mark) settles as a panicked one, its panic held for the window's
    /// report; a frame settled with no record of it held (one the
    /// owner forgot on a detach) is dropped. An inline executor settles each
    /// frame in its own submit and leaves nothing here.
    #[cfg_attr(
        all(
            any(target_os = "android", target_os = "ios", target_arch = "wasm32"),
            not(all(test, feature = "testing"))
        ),
        expect(dead_code, reason = "only the desktop loop turns a raster thread")
    )]
    pub(crate) fn drain_settled(
        &mut self,
        address: PresentationAddress,
        into: &mut Vec<flui_runtime::sink::FrameSettlement>,
    ) {
        let Some(placed) = self.placed.as_mut() else {
            return;
        };
        if placed.executor.inline().is_some() {
            return;
        }
        let Some(id) = placed.slots.get(&address).map(|slot| slot.slot) else {
            return;
        };
        let mut settlements = std::mem::take(&mut placed.settlements);
        settlements.clear();
        // The slot's failure is taken with its settlements, under one lock,
        // for its payload, which the window reports. Which frame a panic came
        // with is each settlement's own mark; taking the panic ends the mark,
        // so a frame held now and not settled in this drain may settle
        // unmarked though the panic came with its turn (or with a command
        // before it): it is tainted, its settlement does not move the base,
        // and settled stale it counts toward the surface's retry.
        let failure = placed
            .sender
            .drain_settlements_and_failure(id, &mut settlements);
        // Read after the drain, which settles what the death left in the
        // inboxes: a death the drain saw is seen here.
        let died = placed.sender.health() == flui_engine::ExecutorHealth::Died;
        let slot = placed
            .slots
            .get_mut(&address)
            .expect("BUG: the window whose slot drained still holds it");
        let panicked = Self::hold_slot_failure(
            slot,
            failure,
            FirstPanic::new(),
            placed.sender.painter_resets(),
        ) == SlotTurn::Panicked;
        if panicked {
            for held in &mut slot.pending {
                held.tainted |= !settlements
                    .iter()
                    .any(|settlement| settlement.epoch == held.epoch);
            }
        }
        let turned = |settlement: &SlotSettlement| {
            if settlement.panicked {
                SlotTurn::Panicked
            } else {
                SlotTurn::Clean
            }
        };
        // A frame drawn after a panicked one of this drain was drawn over its
        // image, which may be torn or miss an upload: it does not move the
        // base either (after a panicked render, whose next frame the executor
        // draws in full anyway, that costs one more full frame).
        let mut torn = false;
        for settlement in &settlements {
            torn |= settlement.panicked;
            let Some(slot) = placed.slots.get_mut(&address) else {
                break;
            };
            let Some(at) = slot
                .pending
                .iter()
                .position(|held| held.epoch == settlement.epoch)
            else {
                let epoch = settlement.epoch;
                let _logged = slot.panic.contain(move || {
                    tracing::warn!(
                        ?epoch,
                        "raster host: a settlement for a frame the window does not hold"
                    );
                });
                continue;
            };
            let frame = slot.pending.remove(at);
            let forget = frame.tainted || torn;
            let verdict = Self::settle_one(
                placed,
                address,
                frame,
                Some(settlement.verdict),
                turned(settlement),
            )
            .unwrap_or(SubmitVerdict::Failed);
            if forget && let Some(slot) = placed.slots.get_mut(&address) {
                slot.base.forget();
            }
            // After the death no later frame shows the tree: one that will not
            // be shown (the executor's own `Failed` at the death, a stale or
            // withheld one) is finished with nothing to present, so it commits
            // and its held input replays (ADR-0305 §9).
            let shown = matches!(verdict, SubmitVerdict::Presented | SubmitVerdict::NoPresent);
            let verdict = if died && !shown {
                SubmitVerdict::NoPresent
            } else {
                verdict
            };
            into.push(flui_runtime::sink::FrameSettlement::new(
                address,
                settlement.epoch,
                verdict,
            ));
        }
        // A dead executor's frames still held never settle, or settle only
        // if a wedged render returns: the window settles them now, as
        // finished with nothing to present, so its UI runtime commits the
        // tree they were painted from and the input held for that commit
        // replays. The window is quarantined, not closed (ADR-0305 §9): its
        // frames go on against the current tree while it draws nothing. A
        // settlement the thread still sends for one of them is a frame the
        // window no longer holds.
        if died
            && let Some(slot) = placed.slots.get_mut(&address)
            && !slot.pending.is_empty()
        {
            slot.base.forget();
            into.extend(slot.pending.drain(..).map(|frame| {
                flui_runtime::sink::FrameSettlement::new(
                    address,
                    frame.epoch,
                    SubmitVerdict::NoPresent,
                )
            }));
        }
        // After a panic the next frame the window asks for draws in full,
        // whatever settled clean in this drain.
        if panicked && let Some(slot) = placed.slots.get_mut(&address) {
            slot.base.forget();
        }
        placed.settlements = settlements;
    }

    /// Move the GPU times the executor holds for `address`'s slot into
    /// `into` (ADR-0388); a window with no slot has none.
    pub(super) fn drain_gpu_times(
        &self,
        address: PresentationAddress,
        into: &mut Vec<flui_foundation::GpuFrameTime>,
    ) {
        let Some(placed) = self.placed.as_ref() else {
            return;
        };
        if let Some(slot) = placed.slots.get(&address) {
            let _drained = placed.sender.drain_gpu_times(slot.slot, into);
        }
    }

    /// How many frames of `address`'s window its sink may hold unsettled
    /// ([`in_flight_limit`]); one for a window with no slot, whose frames
    /// are refused.
    pub(super) fn in_flight_limit(&self, address: PresentationAddress) -> NonZeroUsize {
        self.placed
            .as_ref()
            .and_then(|placed| placed.slots.get(&address))
            .map_or(NonZeroUsize::MIN, |slot| slot.in_flight_limit)
    }

    /// Whether the in-flight floor of `address`'s presentation still guards
    /// a settlement (ADR-0305 §5): `false` only once a raster thread has
    /// settled every frame it holds of the window and its wake has returned
    /// after them, so the window was asked for the frame that drains them,
    /// which its host delivers once the window can take one: woken at the
    /// floor's pace before then, the loop would only ask the other windows
    /// for frames they do not owe. Inline, with no slot, with a frame still
    /// on the thread, or with a settlement whose wake has not returned (it
    /// has not run yet, or a requester panicked), the floor stands.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "the browser's animation frames wake the page; no deadline does"
        )
    )]
    pub(crate) fn floor_owed(&self, address: PresentationAddress) -> bool {
        let Some(placed) = self.placed.as_ref() else {
            return true;
        };
        if placed.executor.inline().is_some() {
            return true;
        }
        let Some(slot) = placed.slots.get(&address) else {
            return true;
        };
        slot.pending.is_empty()
            || placed.sender.settlements_announced(slot.slot) < slot.pending.len()
    }

    /// Whether `address`'s window holds frames a raster thread has not
    /// settled yet.
    #[cfg_attr(
        all(
            any(target_os = "android", target_os = "ios", target_arch = "wasm32"),
            not(all(test, feature = "testing"))
        ),
        expect(dead_code, reason = "only the desktop loop turns a raster thread")
    )]
    pub(crate) fn has_pending(&self, address: PresentationAddress) -> bool {
        self.placed
            .as_ref()
            .and_then(|placed| placed.slots.get(&address))
            .is_some_and(|slot| !slot.pending.is_empty())
    }

    /// What a settled frame of `version`, drawn on `surface`, became, with
    /// the slot's base moved to what the screen now holds.
    pub(super) fn verdict(
        base: &mut RasterBase,
        verdict: RasterVerdict,
        version: flui_layer::SceneVersionId,
        surface: SurfaceKey,
    ) -> SubmitVerdict {
        match verdict {
            RasterVerdict::Presented => {
                base.shown(version, surface);
                SubmitVerdict::Presented
            }
            RasterVerdict::NoPresent => {
                base.shown(version, surface);
                SubmitVerdict::NoPresent
            }
            RasterVerdict::NotShown => SubmitVerdict::NotShown,
            RasterVerdict::Retry => SubmitVerdict::Retry,
            RasterVerdict::SurfaceStale => {
                // The surface the image was on is gone.
                base.forget();
                SubmitVerdict::SurfaceStale
            }
            RasterVerdict::DeviceLost => {
                // The image went with the device: the next frame is drawn in full.
                base.forget();
                SubmitVerdict::DeviceLost
            }
            RasterVerdict::Failed => SubmitVerdict::Failed,
        }
    }
}
