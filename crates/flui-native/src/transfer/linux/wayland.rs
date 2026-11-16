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
//!   device keeps the last, which a read of the clipboard asks.
//! - **A drag** over one of the client's surfaces reaches the seat's device as
//!   `enter` with its offer, then `motion`, and `leave` or `drop`. The
//!   [`DragListener`] hears each, and the offer of an `enter` is its own
//!   from then on; with no listener the offer is let go of at `leave` or
//!   `drop`.
//! - **A drag the client starts** goes from a seat's device under the
//!   serial of the pointer grab its origin surface holds
//!   ([`DataDevices::start_drag`]).
//!
//! An offer is an object the compositor makes the client hold: a device
//! introduces it (`data_offer`) before the event that says what it is. Each
//! device keeps the one it introduced last until a `selection` or an `enter`
//! names it, and destroys it when it introduces the next or goes
//! ([`DeviceOffers`]), so a compositor introducing offers it never names
//! leaves one a device. An offer an event names on a device already let go
//! of is destroyed as it comes.
//!
//! The dispatch records what the compositor said and runs no handler; a
//! listener hears it with nothing of the devices held.

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

/// The offers one device holds: the one it introduced last that no event
/// named yet, the seat's selection, and a drag's. Every offer a device
/// introduces is in one of the three or handed back to be destroyed, so the
/// device holds three at most whatever the compositor sends.
#[derive(Debug)]
pub(crate) struct DeviceOffers<Offer> {
    /// The offer introduced last (`data_offer`) and named by no event yet.
    introduced: Option<Offer>,
    /// The offer of the seat's selection; `None` when it holds none.
    selection: Option<Offer>,
    /// The offer of a drag over one of the client's surfaces, held here only
    /// while no [`DragListener`] takes it.
    dragged: Option<Offer>,
}

impl<Offer> Default for DeviceOffers<Offer> {
    fn default() -> Self {
        Self {
            introduced: None,
            selection: None,
            dragged: None,
        }
    }
}

impl<Offer: PartialEq> DeviceOffers<Offer> {
    /// The device introduced `offer`. Hands back the one it introduced
    /// before, which no event named and none will: to be destroyed.
    #[must_use]
    pub(crate) fn introduce(&mut self, offer: Offer) -> Option<Offer> {
        self.introduced.replace(offer)
    }

    /// An event named `offer`, which is something from now on: no longer
    /// the one waiting for its name.
    pub(crate) fn name(&mut self, offer: &Offer) {
        if self.introduced.as_ref() == Some(offer) {
            self.introduced = None;
        }
    }

    /// The seat's selection is `offer` now. Hands back the selection it
    /// replaces, to be destroyed, unless it is still held.
    #[must_use]
    pub(crate) fn select(&mut self, offer: Option<Offer>) -> Option<Offer> {
        if let Some(offer) = &offer {
            self.name(offer);
        }
        let replaced = std::mem::replace(&mut self.selection, offer);
        self.unless_held(replaced)
    }

    /// A drag entered offering `offer`, which no listener takes. Hands back
    /// the drag's offer it replaces, to be destroyed, unless it is still
    /// held.
    #[must_use]
    pub(crate) fn drag(&mut self, offer: Option<Offer>) -> Option<Offer> {
        if let Some(offer) = &offer {
            self.name(offer);
        }
        let replaced = std::mem::replace(&mut self.dragged, offer);
        self.unless_held(replaced)
    }

    /// The drag left or dropped. Hands back its offer, to be destroyed,
    /// unless it is the selection's too.
    #[must_use]
    pub(crate) fn end_drag(&mut self) -> Option<Offer> {
        let ended = self.dragged.take();
        self.unless_held(ended)
    }

    /// The offer of the seat's selection; `None` when it holds none.
    pub(crate) const fn selection(&self) -> Option<&Offer> {
        self.selection.as_ref()
    }

    /// Every offer held, each once, as the device goes: to be destroyed.
    pub(crate) fn into_held(self) -> impl Iterator<Item = Offer> {
        let Self {
            introduced,
            selection,
            dragged,
        } = self;
        let dragged = dragged.filter(|offer| Some(offer) != selection.as_ref());
        [introduced, selection, dragged].into_iter().flatten()
    }

    /// `replaced` unless an event named it again as what is held now.
    fn unless_held(&self, replaced: Option<Offer>) -> Option<Offer> {
        replaced.filter(|offer| {
            Some(offer) != self.selection.as_ref() && Some(offer) != self.dragged.as_ref()
        })
    }
}

/// One seat's data device and the offers it holds.
struct SeatDevice {
    seat: WlSeat,
    device: WlDataDevice,
    offers: DeviceOffers<WlDataOffer>,
}

impl SeatDevice {
    fn release(self) {
        for offer in self.offers.into_held() {
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
                drags: OwnerCell::new(),
            }),
        }
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
                            offers: DeviceOffers::default(),
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
                            .find(|device| device.offers.selection().is_some())
                    })?;
                Some(SeatSelection {
                    seat: device.seat.clone(),
                    offer: device.offers.selection().cloned(),
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
                    .offers
                    .selection()
                    .cloned()
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

    /// Start a drag of `source` from `origin` on `seat`, under the serial of
    /// the grab `origin` holds; `false` when the seat has no device.
    pub(crate) fn start_drag(
        &self,
        seat: &WlSeat,
        source: &WlDataSource,
        origin: &WlSurface,
        serial: u32,
    ) -> bool {
        self.shared
            .state
            .with(|state| {
                let device = state.devices.iter().find(|device| device.seat == *seat)?;
                device.device.start_drag(Some(source), origin, None, serial);
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
    told: OwnerCell<OfferTold>,
}

/// What the compositor told of one offer.
#[derive(Default)]
struct OfferTold {
    /// Each type offered, once, in the order first offered: at most
    /// [`MAX_TYPES`], none longer than [`MAX_TYPE_LEN`], however often the
    /// owner repeats an `offer` event.
    types: Vec<String>,
    /// The actions a drag's source permits (`source_actions`, version 3);
    /// `None` before the compositor told them.
    source_actions: Option<DndAction>,
    /// The action the compositor chose for a drag (`action`, version 3);
    /// `None` before it chose one.
    action: Option<DndAction>,
}

impl OfferData {
    /// An offer's data, made by the dispatch on the owner thread.
    pub(crate) fn new() -> Self {
        Self {
            told: OwnerCell::new(),
        }
    }

    /// Record an `offer` event: a type not offered before, while the offer
    /// holds fewer than [`MAX_TYPES`].
    pub(crate) fn offer(&self, mime_type: String) {
        if mime_type.len() > MAX_TYPE_LEN {
            return;
        }
        self.told.with(|told| {
            if told.types.len() < MAX_TYPES && !told.types.contains(&mime_type) {
                told.types.push(mime_type);
            }
        });
    }

    /// Run `f` with the types offered, in the order first offered; `None`
    /// off the owner thread.
    pub(crate) fn types<R>(&self, f: impl FnOnce(&[String]) -> R) -> Option<R> {
        self.told.with(|told| f(&told.types))
    }

    /// The actions a drag's source permits; `None` before the compositor
    /// told them, as below version 3 it never does.
    pub(crate) fn source_actions(&self) -> Option<DndAction> {
        self.told.with(|told| told.source_actions).flatten()
    }

    /// The action the compositor chose for a drag; `None` before it chose
    /// one, as below version 3 it never does.
    pub(crate) fn action(&self) -> Option<DndAction> {
        self.told.with(|told| told.action).flatten()
    }
}

/// The actions `bits` name; bits no action has are dropped.
fn actions(bits: u32) -> DndAction {
    DndAction::from_bits_truncate(bits)
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
                        let held = state
                            .devices
                            .iter_mut()
                            .find(|held| held.device == *device)?;
                        // The listener's from here: the device waits on no
                        // name for it.
                        if let DragWire::Enter {
                            offer: Some(offer), ..
                        } = &wire
                        {
                            held.offers.name(offer);
                        }
                        Some(held.seat.clone())
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
        let unheld = shared
            .state
            .with(|state| {
                let Some(held) = state.devices.iter_mut().find(|held| held.device == *device)
                else {
                    // A device let go of holds nothing: the offer its event
                    // brings goes as it comes.
                    return match event {
                        wl_data_device::Event::DataOffer { id } => Some(id),
                        wl_data_device::Event::Selection { id }
                        | wl_data_device::Event::Enter { id, .. } => id,
                        _ => None,
                    };
                };
                match event {
                    wl_data_device::Event::DataOffer { id } => held.offers.introduce(id),
                    wl_data_device::Event::Selection { id } => held.offers.select(id),
                    wl_data_device::Event::Enter { id, .. } => held.offers.drag(id),
                    wl_data_device::Event::Leave | wl_data_device::Event::Drop => {
                        held.offers.end_drag()
                    }
                    _ => None,
                }
            })
            .flatten();
        if let Some(offer) = unheld {
            offer.destroy();
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
            wl_data_offer::Event::SourceActions { source_actions } => {
                data.told.with(|told| {
                    told.source_actions = Some(actions(u32::from(source_actions)));
                });
            }
            wl_data_offer::Event::Action { dnd_action } => {
                data.told
                    .with(|told| told.action = Some(actions(u32::from(dnd_action))));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A compositor that introduces offer after offer and names none leaves
    /// a device one: each is handed back for its destruction as the next
    /// comes. An offer an event names is held as what the event says, and
    /// handed back when the next of its kind replaces it; one named again is
    /// the one held; and the device's end hands back all it holds, each
    /// once.
    #[test]
    fn a_device_holds_one_offer_no_event_named() {
        let mut offers = DeviceOffers::default();
        let mut destroyed = Vec::new();
        for offer in 0..10_000_u32 {
            destroyed.extend(offers.introduce(offer));
        }
        assert_eq!(destroyed, (0..9_999).collect::<Vec<u32>>());

        assert_eq!(offers.select(Some(9_999)), None, "the first selection");
        assert_eq!(offers.selection(), Some(&9_999));
        assert_eq!(
            offers.introduce(10_000),
            None,
            "a named offer waits no more"
        );
        assert_eq!(offers.select(Some(10_000)), Some(9_999), "the one replaced");
        assert_eq!(offers.select(Some(10_000)), None, "named again");

        assert_eq!(offers.introduce(10_001), None);
        assert_eq!(offers.drag(Some(10_001)), None);
        assert_eq!(offers.drag(Some(10_000)), Some(10_001), "a second enter");
        assert_eq!(offers.end_drag(), None, "the selection's offer stays");
        assert_eq!(offers.select(None), Some(10_000), "the selection cleared");

        assert_eq!(offers.introduce(1), None);
        assert_eq!(offers.introduce(2), Some(1));
        assert_eq!(offers.select(Some(3)), None);
        assert_eq!(offers.drag(Some(3)), None);
        assert_eq!(offers.into_held().collect::<Vec<u32>>(), [2, 3]);
    }

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
