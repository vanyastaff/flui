//! The seats' data devices: `wl_data_device_manager` and one
//! `wl_data_device` on each seat the input holds, shared by every kind of
//! transfer that goes through them (the clipboard's selection, a drag).
//!
//! A client gets a seat's selection and its drags on one data device per
//! seat: Weston tells a drag only to the first device a client made on the
//! seat. So the devices belong to no feature: the loop binds the manager as
//! the compositor announces it, keeps a device on each seat the input holds
//! and lets them go as it ends, and the features reach the devices through
//! [`DataDevices`].
//!
//! - **A seat's selection** reaches its device as a data offer named by
//!   `selection`, sent before the keyboard enters one of the client's
//!   surfaces and whenever it changes while the keyboard is in one. The
//!   device keeps the last, and every [`DeviceListener`] hears that one
//!   arrived.
//! - **A drag** over one of the client's surfaces reaches the seat's device as
//!   `enter` with its offer, then `motion`, and `leave` or `drop`. The
//!   [`DragListener`] hears each, and the offer of an `enter` is its own
//!   from then on; with no listener the offer is let go of at `leave` or
//!   `drop`.
//!
//! The dispatch records what the compositor said and runs no handler; a
//! listener hears it with nothing of the devices held.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Weak};

use wayland_client::protocol::wl_data_device::{self, WlDataDevice};
use wayland_client::protocol::wl_data_device_manager::{self, DndAction, WlDataDeviceManager};
use wayland_client::protocol::wl_data_offer::{self, WlDataOffer};
use wayland_client::protocol::wl_data_source::WlDataSource;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};

use crate::owner::linux::wayland::WaylandState;
use crate::owner::slot::OwnerCell;

pub(crate) mod pipe;

/// The newest `wl_data_device_manager`: version 3 adds the drag actions,
/// version 2 lets a device be released.
const MANAGER_VERSION: u32 = 3;

/// The most MIME types an offer keeps: a source offering more offers
/// nothing past them.
pub(crate) const MAX_TYPES: usize = 100;

/// The longest MIME type kept: a media type's type and subtype are at most
/// 127 characters each (RFC 6838), so a longer name names nothing.
pub(crate) const MAX_TYPE_LEN: usize = 255;

/// What a feature hears of the devices.
pub(crate) trait DeviceListener: Send + Sync {
    /// A seat's device was told a selection offer: another client's, or the
    /// application's own as the compositor holds it.
    fn selection_arrived(&self);
}

/// What a seat's device was told of a drag over one of the client's
/// surfaces.
#[derive(Debug)]
pub(crate) enum DragWire {
    /// The drag entered `surface` at (`x`, `y`), surface-local, under
    /// `serial`, offering `offer`: the listener's from now on, `None` for a
    /// drag that offers nothing.
    Enter {
        serial: u32,
        surface: WlSurface,
        x: f64,
        y: f64,
        offer: Option<WlDataOffer>,
    },
    /// The pointer moved to (`x`, `y`) on the surface entered.
    Motion { x: f64, y: f64 },
    /// The drag left the surface, or its source gave up.
    Leave,
    /// The user dropped on the surface entered.
    Drop,
}

/// What the drag feature hears of the devices: called by a dispatch on the
/// owner thread, with nothing of the devices held. It records what it hears
/// and runs no handler.
pub(crate) trait DragListener: Send + Sync {
    /// `seat`'s device was told `wire`.
    fn dragged(&self, seat: &WlSeat, wire: DragWire);
}

/// The seats' data devices, as the loop and the features reach them. Cloned
/// handles reach the same devices. See the module docs.
#[derive(Clone)]
pub(crate) struct DataDevices {
    shared: Arc<Shared>,
}

/// What the devices hold, reached on the owner thread.
struct Shared {
    state: OwnerCell<State>,
    listeners: OwnerCell<Vec<Weak<dyn DeviceListener>>>,
    /// The drag feature, which takes the offers of the drags.
    drags: OwnerCell<Option<Weak<dyn DragListener>>>,
}

#[derive(Default)]
struct State {
    /// `wl_data_device_manager`, by its global's name.
    manager: Option<(u32, WlDataDeviceManager)>,
    /// The queue the devices, their offers and the features' sources report
    /// to.
    queue: Option<QueueHandle<WaylandState>>,
    devices: Vec<SeatDevice>,
}

/// One seat's data device and the offers it holds.
struct SeatDevice {
    seat: WlSeat,
    device: WlDataDevice,
    /// The offer of the seat's selection; `None` when it holds none.
    selection: Option<WlDataOffer>,
    /// The offer of a drag over one of the client's surfaces, held here only
    /// while no [`DragListener`] takes it.
    dragged: Option<WlDataOffer>,
}

impl SeatDevice {
    fn release(self) {
        for offer in [self.selection, self.dragged].into_iter().flatten() {
            offer.destroy();
        }
        // `release` exists from version 2; below it the proxy is just
        // dropped.
        if self.device.version() >= 2 {
            self.device.release();
        }
    }
}

/// A seat's selection as a feature reads it.
pub(crate) struct SeatSelection {
    /// The seat.
    pub(crate) seat: WlSeat,
    /// The offer of its selection; `None` when it holds none.
    pub(crate) offer: Option<WlDataOffer>,
}

impl DataDevices {
    /// The devices of a loop on the calling thread, with no global bound
    /// yet.
    pub(crate) fn new() -> Self {
        Self {
            shared: Arc::new(Shared {
                state: OwnerCell::new(),
                listeners: OwnerCell::new(),
                drags: OwnerCell::new(),
            }),
        }
    }

    /// `listener` hears what the devices are told from now on, while it
    /// lives.
    pub(crate) fn listen(&self, listener: Weak<dyn DeviceListener>) {
        self.shared
            .listeners
            .with(|listeners| listeners.push(listener));
    }

    /// `listener` hears the drags over the client's surfaces from now on,
    /// while it lives, and takes their offers.
    pub(crate) fn listen_drags(&self, listener: Weak<dyn DragListener>) {
        self.shared.drags.with(|drags| *drags = Some(listener));
    }

    /// The compositor announced global `name`: bind it if it is the data
    /// device manager.
    pub(crate) fn global(
        &self,
        registry: &WlRegistry,
        name: u32,
        interface: &str,
        version: u32,
        queue: &QueueHandle<WaylandState>,
    ) {
        if interface != WlDataDeviceManager::interface().name {
            return;
        }
        self.shared.state.with(|state| {
            // A second announcement would bind an object nothing destroys.
            if state.manager.is_some() {
                return;
            }
            let manager = registry.bind::<WlDataDeviceManager, (), WaylandState>(
                name,
                version.min(MANAGER_VERSION),
                queue,
                (),
            );
            state.manager = Some((name, manager));
            state.queue = Some(queue.clone());
        });
    }

    /// The compositor removed global `name`: the devices go with the
    /// manager.
    pub(crate) fn global_removed(&self, name: u32) {
        let gone = self
            .shared
            .state
            .with(|state| {
                if state
                    .manager
                    .as_ref()
                    .is_some_and(|(held, _)| *held == name)
                {
                    state.manager = None;
                    std::mem::take(&mut state.devices)
                } else {
                    Vec::new()
                }
            })
            .unwrap_or_default();
        gone.into_iter().for_each(SeatDevice::release);
    }

    /// The seats the input holds now: each has a data device, and a seat
    /// gone lets go of its own.
    pub(crate) fn seats(&self, seats: &[WlSeat]) {
        let gone = self
            .shared
            .state
            .with(|state| {
                let (kept, gone) = std::mem::take(&mut state.devices)
                    .into_iter()
                    .partition(|device| seats.contains(&device.seat));
                state.devices = kept;
                if let (Some((_, manager)), Some(queue)) = (&state.manager, &state.queue) {
                    for seat in seats {
                        if state.devices.iter().any(|device| device.seat == *seat) {
                            continue;
                        }
                        let device = manager.get_data_device(
                            seat,
                            queue,
                            DeviceData {
                                shared: Arc::downgrade(&self.shared),
                            },
                        );
                        state.devices.push(SeatDevice {
                            seat: seat.clone(),
                            device,
                            selection: None,
                            dragged: None,
                        });
                    }
                }
                gone
            })
            .unwrap_or_default();
        gone.into_iter().for_each(SeatDevice::release);
    }

    /// The loop ends: let go of the devices and the offers they hold.
    pub(crate) fn release(&self) {
        if let Some(State { devices, .. }) = self.shared.state.with(std::mem::take) {
            devices.into_iter().for_each(SeatDevice::release);
        }
    }

    /// The selection a read reads: `seat`'s, or else the first seat's that
    /// holds one; `None` with no device.
    pub(crate) fn selection(&self, seat: Option<&WlSeat>) -> Option<SeatSelection> {
        self.shared
            .state
            .with(|state| {
                let device = state
                    .devices
                    .iter()
                    .find(|device| Some(&device.seat) == seat)
                    .or_else(|| {
                        state
                            .devices
                            .iter()
                            .find(|device| device.selection.is_some())
                    })?;
                Some(SeatSelection {
                    seat: device.seat.clone(),
                    offer: device.selection.clone(),
                })
            })
            .flatten()
    }

    /// The offer of `seat`'s selection; `None` when it holds none. For a
    /// test.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn selection_of(&self, seat: &WlSeat) -> Option<WlDataOffer> {
        self.shared
            .state
            .with(|state| {
                state
                    .devices
                    .iter()
                    .find(|device| device.seat == *seat)?
                    .selection
                    .clone()
            })
            .flatten()
    }

    /// A data source reporting to the loop's queue with `data`; `None`
    /// before the manager is bound.
    pub(crate) fn create_source<U>(&self, data: U) -> Option<WlDataSource>
    where
        U: Send + Sync + 'static,
        WaylandState: Dispatch<WlDataSource, U>,
    {
        self.shared
            .state
            .with(|state| {
                let (_, manager) = state.manager.as_ref()?;
                Some(manager.create_data_source(state.queue.as_ref()?, data))
            })
            .flatten()
    }

    /// Set `source` as `seat`'s selection under the input `serial`; `false`
    /// when the seat has no device.
    pub(crate) fn set_selection(&self, seat: &WlSeat, source: &WlDataSource, serial: u32) -> bool {
        self.shared
            .state
            .with(|state| {
                let device = state.devices.iter().find(|device| device.seat == *seat)?;
                device.device.set_selection(Some(source), serial);
                Some(())
            })
            .flatten()
            .is_some()
    }

    /// The queue the devices report to, once the manager is bound.
    pub(crate) fn queue(&self) -> Option<QueueHandle<WaylandState>> {
        self.shared
            .state
            .with(|state| state.queue.clone())
            .flatten()
    }
}

// ---- The dispatch of the devices' objects on the owner's queue: each
// records what the compositor said, and runs no handler.

/// A data device's user data.
pub(crate) struct DeviceData {
    shared: Weak<Shared>,
}

/// A data offer's user data: the MIME types it offers, as its `offer`
/// events name them, read on the owner thread alone.
pub(crate) struct OfferData {
    /// Each type offered, once, in the order first offered: at most
    /// [`MAX_TYPES`], none longer than [`MAX_TYPE_LEN`], however often the
    /// owner repeats an `offer` event.
    types: OwnerCell<Vec<String>>,
    /// The actions a drag's source permits (`source_actions`, version 3),
    /// [`UNTOLD`] before the compositor told them.
    source_actions: AtomicU32,
    /// The action the compositor chose for a drag (`action`, version 3),
    /// [`UNTOLD`] before it chose one.
    action: AtomicU32,
}

/// An action set the compositor has not told: no `DndAction` has every bit.
const UNTOLD: u32 = u32::MAX;

impl OfferData {
    /// An offer's data, made by the dispatch on the owner thread.
    pub(crate) fn new() -> Self {
        Self {
            types: OwnerCell::new(),
            source_actions: AtomicU32::new(UNTOLD),
            action: AtomicU32::new(UNTOLD),
        }
    }

    /// Record an `offer` event: a type not offered before, while the offer
    /// holds fewer than [`MAX_TYPES`].
    pub(crate) fn offer(&self, mime_type: String) {
        if mime_type.len() > MAX_TYPE_LEN {
            return;
        }
        self.types.with(|types| {
            if types.len() < MAX_TYPES && !types.contains(&mime_type) {
                types.push(mime_type);
            }
        });
    }

    /// Run `f` with the types offered, in the order first offered; `None`
    /// off the owner thread.
    pub(crate) fn types<R>(&self, f: impl FnOnce(&[String]) -> R) -> Option<R> {
        self.types.with(|types| f(types))
    }

    /// The actions a drag's source permits; `None` before the compositor
    /// told them, as below version 3 it never does.
    pub(crate) fn source_actions(&self) -> Option<DndAction> {
        told(self.source_actions.load(Ordering::Acquire))
    }

    /// The action the compositor chose for a drag; `None` before it chose
    /// one, as below version 3 it never does.
    pub(crate) fn action(&self) -> Option<DndAction> {
        told(self.action.load(Ordering::Acquire))
    }
}

/// The actions `bits` name, unless [`UNTOLD`]; bits no action has are
/// dropped.
fn told(bits: u32) -> Option<DndAction> {
    (bits != UNTOLD).then(|| DndAction::from_bits_truncate(bits))
}

impl Dispatch<WlDataDeviceManager, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlDataDeviceManager,
        _: wl_data_device_manager::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The manager has no events.
    }
}

impl Dispatch<WlDataDevice, DeviceData> for WaylandState {
    fn event(
        _: &mut Self,
        device: &WlDataDevice,
        event: wl_data_device::Event,
        data: &DeviceData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(shared) = data.shared.upgrade() else {
            return;
        };
        let drags = shared
            .drags
            .with(|drags| drags.as_ref().and_then(Weak::upgrade))
            .flatten();
        let wire = match (&drags, event) {
            (
                Some(_),
                wl_data_device::Event::Enter {
                    serial,
                    surface,
                    x,
                    y,
                    id,
                },
            ) => Ok(DragWire::Enter {
                serial,
                surface,
                x,
                y,
                offer: id,
            }),
            (Some(_), wl_data_device::Event::Motion { x, y, .. }) => Ok(DragWire::Motion { x, y }),
            (Some(_), wl_data_device::Event::Leave) => Ok(DragWire::Leave),
            (Some(_), wl_data_device::Event::Drop) => Ok(DragWire::Drop),
            (_, event) => Err(event),
        };
        let event = match wire {
            Ok(wire) => {
                let seat = shared
                    .state
                    .with(|state| {
                        state
                            .devices
                            .iter()
                            .find(|held| held.device == *device)
                            .map(|held| held.seat.clone())
                    })
                    .flatten();
                match (drags, seat) {
                    // Heard with nothing of the devices held.
                    (Some(drags), Some(seat)) => drags.dragged(&seat, wire),
                    // A device let go of: its drag's offer goes with it.
                    (_, _) => {
                        if let DragWire::Enter {
                            offer: Some(offer), ..
                        } = wire
                        {
                            offer.destroy();
                        }
                    }
                }
                return;
            }
            Err(event) => event,
        };
        let selection = matches!(&event, wl_data_device::Event::Selection { id: Some(_) });
        let replaced = shared
            .state
            .with(|state| {
                let held = state
                    .devices
                    .iter_mut()
                    .find(|held| held.device == *device)?;
                let replaced = match event {
                    wl_data_device::Event::Selection { id } => {
                        std::mem::replace(&mut held.selection, id)
                    }
                    wl_data_device::Event::Enter { id, .. } => {
                        std::mem::replace(&mut held.dragged, id)
                    }
                    wl_data_device::Event::Leave | wl_data_device::Event::Drop => {
                        held.dragged.take()
                    }
                    _ => None,
                };
                // An offer named again is the one already held.
                replaced.filter(|offer| {
                    Some(offer) != held.selection.as_ref() && Some(offer) != held.dragged.as_ref()
                })
            })
            .flatten();
        if let Some(offer) = replaced {
            offer.destroy();
        }
        if selection {
            // Heard with nothing of the devices held: a listener may read
            // them.
            let listeners = shared
                .listeners
                .with(|listeners| {
                    listeners.retain(|listener| listener.strong_count() > 0);
                    listeners.clone()
                })
                .unwrap_or_default();
            for listener in listeners.iter().filter_map(Weak::upgrade) {
                listener.selection_arrived();
            }
        }
    }

    event_created_child!(WaylandState, WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, OfferData::new()),
    ]);
}

impl Dispatch<WlDataOffer, OfferData> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlDataOffer,
        event: wl_data_offer::Event,
        data: &OfferData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_offer::Event::Offer { mime_type } => data.offer(mime_type),
            wl_data_offer::Event::SourceActions { source_actions } => data
                .source_actions
                .store(u32::from(source_actions), Ordering::Release),
            wl_data_offer::Event::Action { dnd_action } => {
                data.action.store(u32::from(dnd_action), Ordering::Release);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An owner that repeats its `offer` events grows nothing past one entry
    /// per type, keeps the order first offered, and keeps nothing past
    /// [`MAX_TYPES`] or longer than [`MAX_TYPE_LEN`].
    #[test]
    fn an_offer_keeps_each_type_once_and_no_more_than_its_bound() {
        let data = OfferData::new();
        for _ in 0..10_000 {
            data.offer("text/plain;charset=UTF-8".to_owned());
            data.offer("UTF8_STRING".to_owned());
        }
        data.offer("x".repeat(MAX_TYPE_LEN + 1));
        assert_eq!(
            data.types(<[String]>::to_vec),
            Some(vec![
                "text/plain;charset=UTF-8".to_owned(),
                "UTF8_STRING".to_owned()
            ])
        );
        for n in 0..10_000 {
            data.offer(format!("application/x-{n}"));
        }
        assert_eq!(data.types(<[String]>::len), Some(MAX_TYPES));
    }
}
