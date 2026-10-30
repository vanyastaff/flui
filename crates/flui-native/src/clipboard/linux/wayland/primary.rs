//! The seats' primary selection through
//! `zwp_primary_selection_device_manager_v1`: the text the user selected
//! last, which a middle click pastes (ADR-0152).
//!
//! It mirrors the clipboard beside it, on its own devices: one per seat,
//! keeping the offer of the seat's primary selection. Two things differ.
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

use flui_platform::capability::Resolver;
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

use super::{MAX_WRITERS, OfferData, Shared, TRANSFER_TIMEOUT, transfer};
use crate::clipboard::linux::text::TextForm;
use crate::owner::linux::wayland::WaylandState;

/// What the application offers as its selection.
type SelectionText = Rc<dyn Fn() -> Option<String>>;

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

    fn release(self) {
        if let Some((_, source)) = self.source {
            source.destroy();
        }
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
    Now(Option<String>),
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
    pub(super) fn release(self) {
        self.devices.into_iter().for_each(PrimaryDevice::release);
        if let Some(owned) = self.owned {
            owned.release();
        }
        drop(self.sends);
        drop_retired(self.retired);
    }
}

/// Drop the closures the dispatch retired, in order. Once a destructor
/// panics the rest are retained, not dropped during the unwind, and the
/// first failure goes on (ADR-0127).
fn drop_retired(retired: Vec<SelectionText>) {
    let mut retired = retired.into_iter();
    for text in retired.by_ref() {
        if let Err(failure) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(text)))
        {
            retired.for_each(std::mem::forget);
            std::panic::resume_unwind(failure);
        }
    }
}

/// Ask the application's closure for its text. A closure that panics is
/// retained rather than dropped during the unwind, which may be its last
/// owner: a capture's destructor that panicked then would abort (ADR-0127).
fn ask(text: SelectionText) -> Option<String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| text())) {
        Ok(answer) => answer,
        Err(failure) => {
            std::mem::forget(text);
            std::panic::resume_unwind(failure)
        }
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
            previous.release();
        }
    }

    pub(super) fn withdraw_primary(&self) {
        let previous = self.shared.primary.with(|primary| primary.owned.take());
        // Destroying the source unsets the selection it is.
        if let Some(previous) = previous.flatten() {
            previous.release();
        }
    }

    pub(super) fn owns_primary(&self) -> bool {
        self.shared
            .primary
            .with(|primary| primary.owned.is_some())
            .unwrap_or(false)
    }

    pub(super) fn read_primary(&self, answer: Resolver<Option<String>>) {
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
                    .and_then(|offer| Some((offer, offer.data::<OfferData>()?.preferred()?)))
                else {
                    return Read::Now(None);
                };
                match rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC) {
                    Ok((reader, writer)) => {
                        offer.receive(mime, writer.as_fd());
                        drop(writer);
                        Read::Pipe(reader, form)
                    }
                    Err(error) => {
                        tracing::warn!(%error, "no pipe for a primary selection read");
                        Read::Now(None)
                    }
                }
            })
            .unwrap_or(Read::Now(None));
        match read {
            // A retired owner has nobody left to answer.
            Read::Now(text) => drop(answer.resolve(text)),
            // Asked with nothing held: the closure may call back in.
            Read::Ask(text) => drop(answer.resolve(ask(text))),
            Read::Pipe(reader, form) => {
                let spawned = std::thread::Builder::new()
                    .name("flui-wayland-primary".to_owned())
                    .spawn(move || {
                        let text = transfer::read(reader, Instant::now() + TRANSFER_TIMEOUT)
                            .map(|bytes| form.decode(bytes));
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
        let sends: Vec<(Request, transfer::Permit)> = sends
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
                    transfer::write(fd, &bytes, Instant::now() + TRANSFER_TIMEOUT);
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
            data.offer(mime_type);
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

    /// Set in the child process a retention case runs in.
    const CHILD: &str = "FLUI_PRIMARY_RETENTION_CHILD";

    /// A capture whose destructor fails.
    struct FailsOnDrop;

    impl Drop for FailsOnDrop {
        fn drop(&mut self) {
            panic!("a capture's destructor failed");
        }
    }

    /// Run `case` in a child of this test binary: a value dropped during an
    /// unwind whose destructor fails aborts the process, which only a child
    /// can report.
    fn in_child(test: &str, case: impl FnOnce()) {
        if std::env::var_os(CHILD).is_some() {
            case();
            return;
        }
        let path = module_path!()
            .split_once("::")
            .map_or(module_path!(), |(_, path)| path);
        let output = std::process::Command::new(std::env::current_exe().expect("the test binary"))
            .args(["--exact", &format!("{path}::{test}"), "--test-threads=1"])
            .env(CHILD, "1")
            .output()
            .expect("the child runs");
        assert!(
            output.status.success(),
            "the child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn failure_text(failure: &(dyn std::any::Any + Send)) -> Option<&str> {
        flui_foundation::panic::payload_text(failure)
    }

    /// A selection that fails while it is asked is retained with its
    /// captures, and its failure goes on.
    #[test]
    fn a_failing_selection_is_retained_with_its_captures() {
        in_child("a_failing_selection_is_retained_with_its_captures", || {
            let capture = FailsOnDrop;
            let text: SelectionText = Rc::new(move || {
                let _held = &capture;
                panic!("the selection failed")
            });
            let failure = catch_unwind(AssertUnwindSafe(|| ask(text))).expect_err("it failed");
            assert_eq!(failure_text(&*failure), Some("the selection failed"));
        });
    }

    /// Retired closures after one whose capture fails to drop are retained,
    /// and the first failure goes on.
    #[test]
    fn retired_closures_after_a_failed_drop_are_retained() {
        /// Counts its drops, and fails each.
        struct Counted(Rc<std::cell::Cell<u32>>);

        impl Drop for Counted {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
                panic!("a capture's destructor failed");
            }
        }

        in_child("retired_closures_after_a_failed_drop_are_retained", || {
            let drops = Rc::new(std::cell::Cell::new(0));
            let retired: Vec<SelectionText> = (0..2)
                .map(|_| {
                    let capture = Counted(Rc::clone(&drops));
                    Rc::new(move || {
                        let _held = &capture;
                        None
                    }) as SelectionText
                })
                .collect();
            let failure =
                catch_unwind(AssertUnwindSafe(|| drop_retired(retired))).expect_err("it failed");
            assert_eq!(
                failure_text(&*failure),
                Some("a capture's destructor failed")
            );
            assert_eq!(drops.get(), 1, "the closure after the failure is retained");
        });
    }
}
