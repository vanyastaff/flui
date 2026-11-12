//! The owner's side of the executor: it mints slot ids, sends commands and
//! drains each slot's record (ADR-0305 §3, §4).

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicUsize, Ordering},
    },
};

use crossbeam_channel::{Sender, TrySendError};
use flui_foundation::{SurfaceGeneration, geometry::DeviceSize, memory::MemoryPressureLevel};

use super::{
    SlotId,
    command::{
        ExecutorHealth, RasterCommand, RasterSendError, SendReceipt, SlotChange, SlotConfig,
        SlotSettlement, SlotTarget,
    },
    inbox::{SlotFailure, SlotInbox, SlotPhase, SlotRecord},
    slot::SlotMint,
};
use crate::{UnconfiguredSurface, error::EngineResult};

/// A lifecycle command as it travels the queue: the owner's command plus the
/// inbox it shares with the executor.
pub(crate) enum Lifecycle {
    Attach {
        slot: SlotId,
        target: SlotTarget,
        config: SlotConfig,
        /// The generation the attach configured, minted at send.
        generation: SurfaceGeneration,
        inbox: Arc<SlotInbox>,
    },
    Release {
        slot: SlotId,
    },
    Restore {
        slot: SlotId,
        surface: UnconfiguredSurface,
        /// The generation the restore minted at send: frames stamped before
        /// it are refused.
        generation: SurfaceGeneration,
    },
    Detach {
        slot: SlotId,
        inbox: Arc<SlotInbox>,
    },
}

impl Lifecycle {
    fn into_command(self) -> RasterCommand {
        match self {
            Self::Attach {
                slot,
                target,
                config,
                inbox,
                ..
            } => RasterCommand::Attach {
                slot,
                address: inbox.address,
                target,
                config,
            },
            Self::Release { slot } => RasterCommand::Release { slot },
            Self::Restore { slot, surface, .. } => RasterCommand::Restore { slot, surface },
            Self::Detach { slot, .. } => RasterCommand::Detach { slot },
        }
    }
}

/// What the owner and the executor share: the lifecycle queue, the coalesced
/// trim and the executor's health. No lock: the queue is a
/// bounded channel and the rest are atomics.
pub(crate) struct Shared {
    lifecycle: Sender<Lifecycle>,
    health: AtomicU8,
    /// Lifecycle sends under way: each saw the executor running, and its
    /// message is queued once it leaves.
    sending: AtomicUsize,
    /// The strongest trim asked for since the last turn: 0 for none.
    trim: AtomicU8,
}

const RUNNING: u8 = 0;
const SHUT_DOWN: u8 = 1;

impl Shared {
    pub(crate) fn new(lifecycle: Sender<Lifecycle>) -> Self {
        Self {
            lifecycle,
            health: AtomicU8::new(RUNNING),
            sending: AtomicUsize::new(0),
            trim: AtomicU8::new(0),
        }
    }

    pub(crate) fn health(&self) -> ExecutorHealth {
        match self.health.load(Ordering::Acquire) {
            RUNNING => ExecutorHealth::Running,
            _ => ExecutorHealth::ShutDown,
        }
    }

    /// Stop taking lifecycle commands. Returns once every send that saw the
    /// executor running has queued its message, so the queue the stop then
    /// drains holds every command a send accepted, and every later send
    /// answers `Gone` (ADR-0305 §9). A send runs no user code, so the wait
    /// ends whatever thread the sends are on.
    pub(crate) fn shut_down(&self) {
        // Sequentially consistent with the send's increment and health read:
        // either the send sees the stop, or the stop sees the send.
        self.health.store(SHUT_DOWN, Ordering::SeqCst);
        while self.sending.load(Ordering::SeqCst) != 0 {
            std::hint::spin_loop();
        }
    }

    /// Queue `message` while the executor runs, or hand its command back:
    /// `Full` while the queue is full, `Gone` once the executor stopped.
    pub(crate) fn send_lifecycle(&self, message: Lifecycle) -> Result<(), RasterSendError> {
        self.sending.fetch_add(1, Ordering::SeqCst);
        let sent = if self.health.load(Ordering::SeqCst) == RUNNING {
            self.lifecycle.try_send(message)
        } else {
            Err(TrySendError::Disconnected(message))
        };
        self.sending.fetch_sub(1, Ordering::SeqCst);
        sent.map_err(|refused| {
            let full = matches!(refused, TrySendError::Full(_));
            let command = refused.into_inner().into_command();
            if full {
                RasterSendError::full(command)
            } else {
                RasterSendError::gone(command)
            }
        })
    }

    fn ask_trim(&self, level: MemoryPressureLevel) {
        let rank = match level {
            MemoryPressureLevel::Moderate => 1,
            // A level this crate does not know yet is taken as the strongest.
            MemoryPressureLevel::Critical | _ => 2,
        };
        self.trim.fetch_max(rank, Ordering::AcqRel);
    }

    pub(crate) fn take_trim(&self) -> Option<MemoryPressureLevel> {
        match self.trim.swap(0, Ordering::AcqRel) {
            0 => None,
            1 => Some(MemoryPressureLevel::Moderate),
            _ => Some(MemoryPressureLevel::Critical),
        }
    }
}

/// One slot as the owner knows it.
struct OwnerSlot {
    id: SlotId,
    inbox: Arc<SlotInbox>,
    /// A detach was sent: the slot takes no more commands, and its index is
    /// freed once its record reports it detached.
    detaching: bool,
}

#[derive(Default)]
struct OwnerSlots {
    mint: SlotMint,
    entries: Vec<Option<OwnerSlot>>,
}

impl OwnerSlots {
    fn live(&self, id: SlotId) -> Option<&OwnerSlot> {
        self.entries
            .get(id.index() as usize)?
            .as_ref()
            .filter(|slot| slot.id == id)
    }

    fn live_mut(&mut self, id: SlotId) -> Option<&mut OwnerSlot> {
        self.entries
            .get_mut(id.index() as usize)?
            .as_mut()
            .filter(|slot| slot.id == id)
    }

    /// Free the index of every slot whose detach the executor completed, and
    /// hand the slots back: their inboxes may hold the last reference to a
    /// panic payload, which the caller drops once this table is released.
    fn collect_detached(&mut self) -> Vec<OwnerSlot> {
        let mut retired = Vec::new();
        for entry in &mut self.entries {
            let done = entry.as_ref().is_some_and(|slot| {
                slot.detaching && slot.inbox.record().phase == SlotPhase::Detached
            });
            if done && let Some(slot) = entry.take() {
                self.mint.release(slot.id);
                retired.push(slot);
            }
        }
        retired
    }
}

/// The owner's handle on a raster executor: mints slot ids, sends
/// [`RasterCommand`]s and reads each slot's record.
///
/// Owner-local (`!Send`): clones share one slot table on the owner thread, so
/// a destructor the owner runs can send again.
#[derive(Clone)]
pub struct RasterSender {
    shared: Arc<Shared>,
    slots: Rc<RefCell<OwnerSlots>>,
}

impl std::fmt::Debug for RasterSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RasterSender")
            .field("health", &self.shared.health())
            .finish_non_exhaustive()
    }
}

impl RasterSender {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            slots: Rc::new(RefCell::new(OwnerSlots::default())),
        }
    }

    /// A fresh slot id, for the [`RasterCommand::Attach`] that gives it a
    /// target. Indexes of slots whose detach completed are reused first,
    /// under a new generation.
    ///
    /// # Errors
    /// [`SlotIdentityExhausted`](crate::EngineError::SlotIdentityExhausted)
    /// when no index is left.
    pub fn mint_slot(&self) -> EngineResult<SlotId> {
        let (retired, minted) = {
            let mut slots = self.slots.borrow_mut();
            let retired = slots.collect_detached();
            (retired, slots.mint.mint())
        };
        // A retired inbox's unclaimed failure is discarded here, with the
        // slot table released (`InboxState`'s drop).
        drop(retired);
        minted
    }

    /// Whether the executor still takes commands.
    #[must_use]
    pub fn health(&self) -> ExecutorHealth {
        self.shared.health()
    }

    /// Send `command`.
    ///
    /// # Errors
    /// The command, handed back: [`RasterSendError::Full`] while the
    /// lifecycle queue is full, [`RasterSendError::StaleSlot`] for a slot that
    /// is not attached under that id (or a frame stamped for another
    /// presentation), [`RasterSendError::Gone`] once the executor stopped.
    pub fn send(&self, command: RasterCommand) -> Result<SendReceipt, RasterSendError> {
        if self.shared.health() != ExecutorHealth::Running {
            return Err(RasterSendError::gone(command));
        }
        match command {
            RasterCommand::Attach {
                slot,
                address,
                target,
                config,
            } => self.attach(slot, address, target, config),
            RasterCommand::Release { slot } => self.release(slot),
            RasterCommand::Restore { slot, surface } => self.restore(slot, surface),
            RasterCommand::Detach { slot } => self.detach(slot),
            RasterCommand::Frame { slot, snapshot } => self.frame(slot, snapshot),
            RasterCommand::Configure { slot, change } => self.configure(slot, change),
            RasterCommand::Trim(level) => {
                self.shared.ask_trim(level);
                Ok(SendReceipt::Accepted)
            }
        }
    }

    /// The record of `slot`; `None` for an id the owner does not hold.
    #[must_use]
    pub fn record(&self, slot: SlotId) -> Option<SlotRecord> {
        let inbox = self.inbox(slot)?;
        Some(inbox.record())
    }

    /// Move `slot`'s undrained settlements, oldest first, into `into`, and
    /// answer how many moved.
    pub fn drain_settlements(&self, slot: SlotId, into: &mut Vec<SlotSettlement>) -> usize {
        let Some(inbox) = self.inbox(slot) else {
            return 0;
        };
        let mut state = inbox.lock();
        let drained = state.settlements.len();
        into.extend(state.settlements.drain(..));
        drained
    }

    /// The first failure of `slot` since the last take.
    #[must_use]
    pub fn take_failure(&self, slot: SlotId) -> Option<SlotFailure> {
        let inbox = self.inbox(slot)?;
        inbox.lock().failure.take()
    }

    fn inbox(&self, slot: SlotId) -> Option<Arc<SlotInbox>> {
        let slots = self.slots.borrow();
        slots.live(slot).map(|owned| Arc::clone(&owned.inbox))
    }

    /// The inbox of a slot that takes commands: live and not detaching.
    fn commanded(&self, slot: SlotId) -> Option<Arc<SlotInbox>> {
        let slots = self.slots.borrow();
        slots
            .live(slot)
            .filter(|owned| !owned.detaching)
            .map(|owned| Arc::clone(&owned.inbox))
    }

    fn attach(
        &self,
        slot: SlotId,
        address: flui_foundation::PresentationAddress,
        target: SlotTarget,
        config: SlotConfig,
    ) -> Result<SendReceipt, RasterSendError> {
        let (generation, receipt) = if target.size().is_some_and(has_area) {
            let generation = SurfaceGeneration::ZERO.next();
            (generation, SendReceipt::Configured(generation))
        } else {
            (SurfaceGeneration::ZERO, SendReceipt::Unconfigured)
        };
        let inbox = {
            let mut slots = self.slots.borrow_mut();
            let index = slot.index() as usize;
            if !slots.mint.is_issued(slot) || slots.entries.get(index).is_some_and(Option::is_some)
            {
                drop(slots);
                return Err(RasterSendError::stale_slot(RasterCommand::Attach {
                    slot,
                    address,
                    target,
                    config,
                }));
            }
            if slots.entries.len() <= index {
                slots.entries.resize_with(index + 1, || None);
            }
            let inbox = Arc::new(SlotInbox::new(address, generation));
            slots.entries[index] = Some(OwnerSlot {
                id: slot,
                inbox: Arc::clone(&inbox),
                detaching: false,
            });
            inbox
        };
        let message = Lifecycle::Attach {
            slot,
            target,
            config,
            generation,
            inbox,
        };
        let sent = self.shared.send_lifecycle(message);
        if sent.is_err() {
            // The slot was never sent: the owner holds its id again.
            if let Some(entry) = self
                .slots
                .borrow_mut()
                .entries
                .get_mut(slot.index() as usize)
            {
                *entry = None;
            }
        }
        sent.map(|()| receipt)
    }

    fn release(&self, slot: SlotId) -> Result<SendReceipt, RasterSendError> {
        let Some(inbox) = self.commanded(slot) else {
            return Err(RasterSendError::stale_slot(RasterCommand::Release { slot }));
        };
        if !inbox.lock().phase.takes_commands() {
            return Err(self.refusal(RasterCommand::Release { slot }));
        }
        self.shared
            .send_lifecycle(Lifecycle::Release { slot })
            .map(|()| SendReceipt::Accepted)
    }

    fn restore(
        &self,
        slot: SlotId,
        surface: UnconfiguredSurface,
    ) -> Result<SendReceipt, RasterSendError> {
        let Some(inbox) = self.commanded(slot) else {
            return Err(RasterSendError::stale_slot(RasterCommand::Restore {
                slot,
                surface,
            }));
        };
        // Minted before the send, so the owner stamps its next frame with it,
        // and taken back if the send fails, unless something minted since.
        let (previous, generation) = {
            let mut state = inbox.lock();
            if !state.phase.takes_commands() {
                drop(state);
                return Err(self.refusal(RasterCommand::Restore { slot, surface }));
            }
            let previous = state.generation;
            if previous != SurfaceGeneration::ZERO {
                state.generation = previous.next();
            }
            (previous, state.generation)
        };
        let sent = self.shared.send_lifecycle(Lifecycle::Restore {
            slot,
            surface,
            generation,
        });
        match sent {
            Ok(()) if generation == SurfaceGeneration::ZERO => Ok(SendReceipt::Unconfigured),
            Ok(()) => Ok(SendReceipt::Configured(generation)),
            Err(error) => {
                let mut state = inbox.lock();
                if state.generation == generation {
                    state.generation = previous;
                }
                Err(error)
            }
        }
    }

    /// The refusal of a command to a slot that takes none: refused or
    /// detached while the executor runs, gone once it stopped.
    fn refusal(&self, command: RasterCommand) -> RasterSendError {
        if self.shared.health() == ExecutorHealth::Running {
            RasterSendError::stale_slot(command)
        } else {
            RasterSendError::gone(command)
        }
    }
    fn detach(&self, slot: SlotId) -> Result<SendReceipt, RasterSendError> {
        let inbox = {
            let mut slots = self.slots.borrow_mut();
            let Some(owned) = slots.live_mut(slot).filter(|owned| !owned.detaching) else {
                drop(slots);
                return Err(RasterSendError::stale_slot(RasterCommand::Detach { slot }));
            };
            owned.detaching = true;
            Arc::clone(&owned.inbox)
        };
        let sent = self
            .shared
            .send_lifecycle(Lifecycle::Detach { slot, inbox });
        if sent.is_err()
            && let Some(owned) = self.slots.borrow_mut().live_mut(slot)
        {
            owned.detaching = false;
        }
        sent.map(|()| SendReceipt::Accepted)
    }

    fn frame(
        &self,
        slot: SlotId,
        mut snapshot: flui_layer::SceneSnapshot,
    ) -> Result<SendReceipt, RasterSendError> {
        let inbox = match self.commanded(slot) {
            Some(inbox) if inbox.address == snapshot.stamp.address => inbox,
            Some(_) | None => {
                return Err(RasterSendError::stale_slot(RasterCommand::Frame {
                    slot,
                    snapshot,
                }));
            }
        };
        let superseded = {
            let mut state = inbox.lock();
            match state.phase {
                SlotPhase::Attaching | SlotPhase::Attached => {}
                SlotPhase::Refused | SlotPhase::Detached => {
                    drop(state);
                    let command = RasterCommand::Frame { slot, snapshot };
                    return Err(if self.shared.health() == ExecutorHealth::Running {
                        RasterSendError::stale_slot(command)
                    } else {
                        RasterSendError::gone(command)
                    });
                }
            }
            // The superseded frame never renders, but its damage is real:
            // the producer compared the new frame against it, not against
            // what the target shows.
            if let Some(pending) = state.frame.as_ref() {
                snapshot.damage = snapshot.damage.union(pending.damage);
            } else {
                state.in_flight += 1;
            }
            state.frame.replace(snapshot)
        };
        // After the lock: a scene's destructor can be user code, and it may
        // send to this slot again.
        drop(superseded);
        Ok(SendReceipt::Accepted)
    }

    fn configure(&self, slot: SlotId, change: SlotChange) -> Result<SendReceipt, RasterSendError> {
        let Some(inbox) = self.commanded(slot) else {
            return Err(RasterSendError::stale_slot(RasterCommand::Configure {
                slot,
                change,
            }));
        };
        let mut state = inbox.lock();
        if matches!(state.phase, SlotPhase::Refused | SlotPhase::Detached) {
            drop(state);
            let command = RasterCommand::Configure { slot, change };
            return Err(if self.shared.health() == ExecutorHealth::Running {
                RasterSendError::stale_slot(command)
            } else {
                RasterSendError::gone(command)
            });
        }
        Ok(match change {
            SlotChange::Size(size) if has_area(size) => {
                state.generation = state.generation.next();
                state.size = Some(size);
                SendReceipt::Configured(state.generation)
            }
            SlotChange::Size(_) => SendReceipt::Unconfigured,
            SlotChange::Color(color) => {
                state.color = Some(color);
                SendReceipt::Accepted
            }
            SlotChange::Latency(latency) => {
                state.latency = Some(latency);
                SendReceipt::Accepted
            }
            SlotChange::Shown(visibility) => {
                state.visibility = Some(visibility);
                SendReceipt::Accepted
            }
        })
    }
}

/// Whether both sides of `size` are positive.
fn has_area(size: DeviceSize) -> bool {
    size.width > 0 && size.height > 0
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use crossbeam_channel::bounded;

    use super::{Lifecycle, Shared};
    use crate::raster_executor::{LIFECYCLE_CAPACITY, SlotMint};

    /// A stop that races a lifecycle send on another thread answers it: a
    /// send that saw the executor running has its command in the queue
    /// before the stop drains it, and a send after the stop gets its command
    /// back as `Gone`. None lands in a queue nobody drains, where an attach
    /// would wait `Attaching` for good (ADR-0305 §9).
    ///
    /// Red-check: drop the wait for sends under way from `Shared::shut_down`
    /// and the stop returns while the send is still between its health read
    /// and its queueing.
    #[test]
    fn a_send_racing_the_stop_is_drained_or_handed_back() {
        let slot = SlotMint::default().mint().expect("an index is left");
        let (queue, drain) = bounded(LIFECYCLE_CAPACITY);
        let shared = Arc::new(Shared::new(queue));
        // A send on another thread has read the health as running and not
        // queued its command yet.
        shared.sending.fetch_add(1, Ordering::SeqCst);
        assert_eq!(shared.health.load(Ordering::SeqCst), super::RUNNING);
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = {
            let shared = Arc::clone(&shared);
            let stopped = Arc::clone(&stopped);
            std::thread::spawn(move || {
                shared.shut_down();
                stopped.store(true, Ordering::SeqCst);
            })
        };
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(
            !stopped.load(Ordering::SeqCst),
            "the stop waits for the send under way"
        );
        assert!(
            shared
                .lifecycle
                .try_send(Lifecycle::Release { slot })
                .is_ok()
        );
        shared.sending.fetch_sub(1, Ordering::SeqCst);
        stop.join().expect("the stop does not panic");
        assert_eq!(
            drain.try_iter().count(),
            1,
            "the accepted command is in the queue the stop drains"
        );
        assert!(
            matches!(
                shared.send_lifecycle(Lifecycle::Release { slot }),
                Err(crate::raster_executor::RasterSendError::Gone(_))
            ),
            "a send after the stop gets its command back"
        );
    }
}
