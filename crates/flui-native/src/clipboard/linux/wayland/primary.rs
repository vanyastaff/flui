//! The seats' primary selection through
//! `zwp_primary_selection_device_manager_v1`: the text the user selected
//! last, which a middle click pastes (ADR-0152).
//!
//! It mirrors the clipboard beside it, on its own devices: one per seat,
//! keeping the offer of the seat's primary selection. Three things differ.
//!
//! - **The application has one selection, not one per seat.** A claim made
//!   above makes every earlier claim stale, wherever it was made, so
//!   offering on the seat of the last input releases the source on any
//!   other seat. The clipboard keeps each seat's copy instead, because a
//!   copy is written once and no claim replaces it.
//!
//! - **The application's selection is lazy.**
//!   [`offer_primary`](super::SeatClipboard::offer_primary) holds the
//!   application's closure and sets a source offering every form of
//!   [`TextForm::OFFERED`]; the closure is asked for the text only when the
//!   selection is read. A `send` from another client is recorded by the
//!   dispatch with its descriptor and answered at the loop's turn
//!   ([`deliver_primary`](super::SeatClipboard::deliver_primary)), with no
//!   lock of the loop or of this selection held, so the closure may read
//!   widget state and call back into the selection; the reader waits on its
//!   pipe meanwhile.
//! - **A read of the application's own selection** asks its closure at
//!   once, again with nothing held.

use std::os::fd::{AsFd, OwnedFd};
use std::rc::Rc;
use std::sync::{Arc, Weak};

use flui_platform::{ClipboardError, ClipboardRead, capability::Resolver};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};
use wayland_protocols::wp::primary_selection::zv1::client::{
    zwp_primary_selection_device_manager_v1::{self, ZwpPrimarySelectionDeviceManagerV1},
    zwp_primary_selection_device_v1::{self, ZwpPrimarySelectionDeviceV1},
    zwp_primary_selection_offer_v1::{self, ZwpPrimarySelectionOfferV1},
    zwp_primary_selection_source_v1::{self, ZwpPrimarySelectionSourceV1},
};
use web_time::Instant;

use super::{MAX_WRITERS, Shared, TRANSFER_TIMEOUT};
use crate::clipboard::linux::offered::{SelectionText, answer_with, ask, drop_retired};
use crate::clipboard::text::TextForm;
use crate::owner::linux::wayland::WaylandState;
use crate::transfer::linux::wayland::{OfferData, pipe};

/// The primary selection's part of the clipboard's state, reached on the
/// owner thread.
#[derive(Default)]
pub(super) struct Primary {
    /// The manager, by its global's name.
    manager: Option<(u32, ZwpPrimarySelectionDeviceManagerV1)>,
    queue: Option<QueueHandle<WaylandState>>,
    devices: Vec<PrimaryDevice>,
    /// The application's selection.
    owned: Option<Owned>,
    /// The application's closures the dispatch retired, dropped at the
    /// next turn: their captures' destructors are user code, which must not
    /// run inside the dispatch.
    retired: Vec<SelectionText>,
    /// The `send`s recorded since the last turn.
    sends: Vec<Request>,
}

struct PrimaryDevice {
    seat: WlSeat,
    device: ZwpPrimarySelectionDeviceV1,
    /// The offer of the seat's primary selection.
    selection: Option<ZwpPrimarySelectionOfferV1>,
}

impl PrimaryDevice {
    fn release(self) {
        if let Some(offer) = self.selection {
            offer.destroy();
        }
        self.device.destroy();
    }
}

/// The application's selection: what answers it, and the seat and source
/// the compositor asks for it under; `None` while no seat took it.
struct Owned {
    text: SelectionText,
    source: Option<(WlSeat, ZwpPrimarySelectionSourceV1)>,
}

impl Owned {
    fn holds(&self, seat: Option<&WlSeat>) -> bool {
        match &self.source {
            Some((owner, _)) => seat == Some(owner),
            None => true,
        }
    }

    /// Destroys the source, which unsets the selection it is, and hands
    /// back the closure for its owner to drop.
    fn release(self) -> SelectionText {
        if let Some((_, source)) = self.source {
            source.destroy();
        }
        self.text
    }
}

/// A `send` the dispatch recorded: the source asked, the form and the
/// descriptor to write it to.
struct Request {
    source: ZwpPrimarySelectionSourceV1,
    form: TextForm,
    fd: OwnedFd,
}

/// What a read does once the state is let go of.
enum Read {
    Now(ClipboardRead),
    Ask(SelectionText),
    Pipe(OwnedFd, TextForm),
}

impl Primary {
    /// Bind the manager if `interface` is its.
    pub(super) fn global(
        &mut self,
        registry: &WlRegistry,
        name: u32,
        interface: &str,
        queue: &QueueHandle<WaylandState>,
    ) {
        if interface != ZwpPrimarySelectionDeviceManagerV1::interface().name
            || self.manager.is_some()
        {
            return;
        }
        let manager = registry.bind::<ZwpPrimarySelectionDeviceManagerV1, (), WaylandState>(
            name,
            1,
            queue,
            (),
        );
        self.manager = Some((name, manager));
        self.queue = Some(queue.clone());
    }

    /// The devices to release when global `name` was the manager's.
    pub(super) fn global_removed(&mut self, name: u32) -> Vec<PrimaryDeviceGone> {
        if self.manager.as_ref().is_some_and(|(held, _)| *held == name) {
            self.manager = None;
            return std::mem::take(&mut self.devices)
                .into_iter()
                .map(PrimaryDeviceGone)
                .collect();
        }
        Vec::new()
    }

    /// Keep a device on each of `seats`; the devices of seats gone.
    pub(super) fn seats(
        &mut self,
        seats: &[WlSeat],
        shared: &Weak<Shared>,
    ) -> Vec<PrimaryDeviceGone> {
        let (kept, gone): (Vec<_>, Vec<_>) = std::mem::take(&mut self.devices)
            .into_iter()
            .partition(|device| seats.contains(&device.seat));
        self.devices = kept;
        if let (Some((_, manager)), Some(queue)) = (&self.manager, &self.queue) {
            for seat in seats {
                if self.devices.iter().any(|device| device.seat == *seat) {
                    continue;
                }
                let device = manager.get_device(
                    seat,
                    queue,
                    DeviceData {
                        shared: Weak::clone(shared),
                    },
                );
                self.devices.push(PrimaryDevice {
                    seat: seat.clone(),
                    device,
                    selection: None,
                });
            }
        }
        gone.into_iter().map(PrimaryDeviceGone).collect()
    }

    /// Everything to let go of as the loop ends.
    pub(super) fn take(&mut self) -> Released {
        Released {
            devices: std::mem::take(&mut self.devices),
            owned: self.owned.take(),
            sends: std::mem::take(&mut self.sends),
            retired: std::mem::take(&mut self.retired),
        }
    }

    pub(super) fn has_manager(&self) -> bool {
        self.manager.is_some()
    }
}

/// A device let go of outside the state's borrow.
pub(super) struct PrimaryDeviceGone(PrimaryDevice);

impl PrimaryDeviceGone {
    pub(super) fn release(self) {
        self.0.release();
    }
}

/// What the loop's end lets go of, outside the state's borrow: the
/// application's closure among it.
pub(super) struct Released {
    devices: Vec<PrimaryDevice>,
    owned: Option<Owned>,
    sends: Vec<Request>,
    retired: Vec<SelectionText>,
}

impl Released {
    /// The current closure drops first and the retired ones after it, in
    /// one protected sequence: once one destructor panics, the rest are still
    /// dropped, contained, and the first failure goes on (ADR-0281).
    pub(super) fn release(self) {
        self.devices.into_iter().for_each(PrimaryDevice::release);
        drop(self.sends);
        let current = self.owned.map(Owned::release);
        drop_retired(current.into_iter().chain(self.retired).collect());
    }
}

/// The primary selection of [`super::SeatClipboard`].
impl super::SeatClipboard {
    pub(super) fn offer_primary(&self, text: SelectionText) {
        let last = self.shared.last_input.latest();
        let previous = self.shared.primary.with(|primary| {
            let source = last.and_then(|input| {
                let (_, manager) = primary.manager.as_ref()?;
                let queue = primary.queue.as_ref()?;
                let device = primary
                    .devices
                    .iter()
                    .find(|device| device.seat == input.seat)?;
                let source = manager.create_source(
                    queue,
                    SourceData {
                        shared: Arc::downgrade(&self.shared),
                    },
                );
                for form in TextForm::OFFERED {
                    source.offer(form.name().to_owned());
                }
                device.device.set_selection(Some(&source), input.serial);
                Some((input.seat, source))
            });
            primary.owned.replace(Owned { text, source })
        });
        // The closure replaced is dropped with nothing held.
        if let Some(previous) = previous.flatten() {
            drop(previous.release());
        }
    }

    pub(super) fn withdraw_primary(&self) {
        let previous = self.shared.primary.with(|primary| primary.owned.take());
        // Destroying the source unsets the selection it is.
        if let Some(previous) = previous.flatten() {
            drop(previous.release());
        }
    }

    pub(super) fn owns_primary(&self) -> bool {
        self.shared
            .primary
            .with(|primary| primary.owned.is_some())
            .unwrap_or(false)
    }

    pub(super) fn read_primary(&self, answer: Resolver<ClipboardRead>) {
        let last_seat = self.shared.last_input.latest().map(|input| input.seat);
        let read = self
            .shared
            .primary
            .with(|primary| {
                let device = primary
                    .devices
                    .iter()
                    .find(|device| Some(&device.seat) == last_seat.as_ref())
                    .or_else(|| {
                        primary
                            .devices
                            .iter()
                            .find(|device| device.selection.is_some())
                    });
                if let Some(owned) = &primary.owned
                    && owned.holds(device.map(|device| &device.seat))
                {
                    return Read::Ask(Rc::clone(&owned.text));
                }
                let Some((offer, (form, mime))) = device
                    .and_then(|device| device.selection.as_ref())
                    .and_then(|offer| Some((offer, super::preferred(offer.data::<OfferData>()?)?)))
                else {
                    return Read::Now(Ok(None));
                };
                match rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC) {
                    Ok((reader, writer)) => {
                        offer.receive(mime, writer.as_fd());
                        drop(writer);
                        Read::Pipe(reader, form)
                    }
                    Err(error) => {
                        tracing::warn!(%error, "no pipe for a primary selection read");
                        Read::Now(Err(ClipboardError::Unavailable))
                    }
                }
            })
            .unwrap_or(Read::Now(Err(ClipboardError::Unavailable)));
        match read {
            // A retired owner has nobody left to answer.
            Read::Now(text) => drop(answer.resolve(text)),
            // Asked with nothing held: the closure may call back in.
            Read::Ask(text) => answer_with(text, answer),
            Read::Pipe(reader, form) => {
                let spawned = std::thread::Builder::new()
                    .name("flui-wayland-primary".to_owned())
                    .spawn(move || {
                        let text =
                            super::read_selection(reader).and_then(|bytes| form.answer(bytes));
                        let _ = answer.resolve(text);
                    });
                if let Err(error) = spawned {
                    tracing::warn!(%error, "no thread for a primary selection read");
                }
            }
        }
    }

    /// Answer the `send`s recorded since the last turn: each still asking
    /// the application's current source gets the closure's text now.
    pub(super) fn deliver_primary(&self) {
        // The closures the dispatch retired drop first, with nothing held and
        // nothing of the current selection taken yet: a destructor that
        // withdraws the current selection and then panics leaves no snapshot
        // of it for the unwind to drop.
        let retired = self
            .shared
            .primary
            .with(|primary| std::mem::take(&mut primary.retired))
            .unwrap_or_default();
        drop_retired(retired);
        let (sends, text) = self
            .shared
            .primary
            .with(|primary| {
                let sends = std::mem::take(&mut primary.sends);
                let current = primary.owned.as_ref().and_then(|owned| {
                    Some((owned.source.as_ref()?.1.clone(), Rc::clone(&owned.text)))
                });
                (sends, current)
            })
            .unwrap_or_default();
        if sends.is_empty() {
            return;
        }
        // A send to a source no longer current is answered with nothing:
        // dropping its descriptor ends the reader's read.
        let Some((current, text)) = text else {
            return;
        };
        // A place among the writers is taken before the selection is asked
        // or encoded, so requests past them cost nothing: their descriptors
        // close here, which ends their readers' reads.
        let sends: Vec<(Request, pipe::Permit)> = sends
            .into_iter()
            .filter(|send| send.source == current)
            .map_while(|send| Some((send, self.shared.writers.permit()?)))
            .collect();
        if sends.is_empty() {
            return;
        }
        // Asked once for every send of this turn, with nothing held.
        let Some(text) = ask(text) else {
            return;
        };
        let mut encoded: Vec<(TextForm, Arc<[u8]>)> = Vec::new();
        for (Request { form, fd, .. }, permit) in sends {
            let bytes = if let Some((_, bytes)) = encoded.iter().find(|(held, _)| *held == form) {
                Arc::clone(bytes)
            } else {
                let bytes: Arc<[u8]> = form.encode(&text).into();
                encoded.push((form, Arc::clone(&bytes)));
                bytes
            };
            let spawned = std::thread::Builder::new()
                .name("flui-wayland-primary".to_owned())
                .spawn(move || {
                    pipe::write(fd, &bytes, Instant::now() + TRANSFER_TIMEOUT);
                    drop(permit);
                });
            if let Err(error) = spawned {
                tracing::warn!(%error, "no thread for a primary selection write");
            }
        }
    }
}

// ---- The dispatch: each records what the compositor said, and runs no
// handler.

/// A primary selection device's user data.
pub(crate) struct DeviceData {
    shared: Weak<Shared>,
}

/// A primary selection source's user data.
pub(crate) struct SourceData {
    shared: Weak<Shared>,
}

impl Dispatch<ZwpPrimarySelectionDeviceManagerV1, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZwpPrimarySelectionDeviceManagerV1,
        _: zwp_primary_selection_device_manager_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The manager has no events.
    }
}

impl Dispatch<ZwpPrimarySelectionDeviceV1, DeviceData> for WaylandState {
    fn event(
        _: &mut Self,
        device: &ZwpPrimarySelectionDeviceV1,
        event: zwp_primary_selection_device_v1::Event,
        data: &DeviceData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let zwp_primary_selection_device_v1::Event::Selection { id } = event else {
            return;
        };
        let Some(shared) = data.shared.upgrade() else {
            return;
        };
        let foreign = id.is_some();
        let replaced = shared
            .primary
            .with(|primary| {
                let held = primary
                    .devices
                    .iter_mut()
                    .find(|held| held.device == *device)?;
                let replaced = std::mem::replace(&mut held.selection, id)
                    .filter(|offer| Some(offer) != held.selection.as_ref());
                // A selection held in this process with no source is the
                // selection until another client's arrives; its closure is
                // dropped at the next turn, outside the dispatch.
                if foreign
                    && primary
                        .owned
                        .as_ref()
                        .is_some_and(|owned| owned.source.is_none())
                    && let Some(owned) = primary.owned.take()
                {
                    primary.retired.push(owned.text);
                }
                replaced
            })
            .flatten();
        if let Some(offer) = replaced {
            offer.destroy();
        }
    }

    event_created_child!(WaylandState, ZwpPrimarySelectionDeviceV1, [
        zwp_primary_selection_device_v1::EVT_DATA_OFFER_OPCODE => (ZwpPrimarySelectionOfferV1, OfferData::new()),
    ]);
}

impl Dispatch<ZwpPrimarySelectionOfferV1, OfferData> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZwpPrimarySelectionOfferV1,
        event: zwp_primary_selection_offer_v1::Event,
        data: &OfferData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_primary_selection_offer_v1::Event::Offer { mime_type } = event {
            data.admit_type(mime_type);
        }
    }
}

impl Dispatch<ZwpPrimarySelectionSourceV1, SourceData> for WaylandState {
    fn event(
        _: &mut Self,
        source: &ZwpPrimarySelectionSourceV1,
        event: zwp_primary_selection_source_v1::Event,
        data: &SourceData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(shared) = data.shared.upgrade() else {
            if matches!(event, zwp_primary_selection_source_v1::Event::Cancelled) {
                source.destroy();
            }
            return;
        };
        match event {
            zwp_primary_selection_source_v1::Event::Send { mime_type, fd } => {
                // A form the source did not offer is answered with nothing.
                let Some(form) = TextForm::named(&mime_type) else {
                    return;
                };
                shared.primary.with(|primary| {
                    // Bounded as the writers are: a client asking faster
                    // than the loop turns is answered with nothing past it.
                    if primary.sends.len() < MAX_WRITERS {
                        primary.sends.push(Request {
                            source: source.clone(),
                            form,
                            fd,
                        });
                    }
                });
            }
            zwp_primary_selection_source_v1::Event::Cancelled => {
                shared.primary.with(|primary| {
                    if primary.owned.as_ref().is_some_and(|owned| {
                        owned
                            .source
                            .as_ref()
                            .is_some_and(|(_, held)| held == source)
                    }) && let Some(owned) = primary.owned.take()
                    {
                        // Dropped at the next turn, outside the dispatch.
                        primary.retired.push(owned.text);
                    }
                });
                // The source is the protocol's to destroy, once.
                source.destroy();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;

    fn failure_text(failure: &(dyn std::any::Any + Send)) -> Option<&str> {
        flui_foundation::panic::payload_text(failure)
    }

    /// Counts its drops, and fails each.
    struct Counted(Rc<std::cell::Cell<u32>>);

    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
            panic!("a capture's destructor failed");
        }
    }

    /// A closure holding a [`Counted`] capture over `drops`.
    fn counted(drops: &Rc<std::cell::Cell<u32>>) -> SelectionText {
        let capture = Counted(Rc::clone(drops));
        Rc::new(move || {
            let _held = &capture;
            None
        })
    }

    /// Runs `release`, which must fail once with the first capture's
    /// failure, having dropped both closures `drops` counts.
    fn fails_once(drops: &Rc<std::cell::Cell<u32>>, release: impl FnOnce()) {
        let failure = catch_unwind(AssertUnwindSafe(release)).expect_err("it failed");
        assert_eq!(
            failure_text(&*failure),
            Some("a capture's destructor failed")
        );
        assert_eq!(
            drops.get(),
            2,
            "the closures after the failure are still dropped"
        );
    }

    /// A selection that fails while it is asked is dropped with its
    /// captures, and the call's failure goes on, not the destructor's.
    #[test]
    fn a_failing_selection_is_dropped_and_its_failure_goes_on() {
        let drops = Rc::new(std::cell::Cell::new(0));
        let capture = Counted(Rc::clone(&drops));
        let text: SelectionText = Rc::new(move || {
            let _held = &capture;
            panic!("the selection failed")
        });
        let failure = catch_unwind(AssertUnwindSafe(|| ask(text))).expect_err("it failed");
        assert_eq!(failure_text(&*failure), Some("the selection failed"));
        assert_eq!(drops.get(), 1, "the closure is dropped after the call");
    }

    /// Retired closures after one whose capture fails to drop are still
    /// dropped, and the first failure goes on.
    #[test]
    fn retired_closures_after_a_failed_drop_are_dropped() {
        let drops = Rc::new(std::cell::Cell::new(0));
        let retired = vec![counted(&drops), counted(&drops)];
        fails_once(&drops, || drop_retired(retired));
    }

    /// At the loop's end the current selection drops in the same contained
    /// sequence as the retired ones.
    #[test]
    fn the_loops_end_drops_the_retired_after_the_current_fails() {
        let drops = Rc::new(std::cell::Cell::new(0));
        let released = Released {
            devices: Vec::new(),
            owned: Some(Owned {
                text: counted(&drops),
                source: None,
            }),
            sends: Vec::new(),
            retired: vec![counted(&drops)],
        };
        fails_once(&drops, || released.release());
    }
}
