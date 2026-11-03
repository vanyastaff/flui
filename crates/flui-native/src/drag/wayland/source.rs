//! Drag sessions started from the Wayland host's windows (`wl_data_source`,
//! version 3).
//!
//! A session starts from a window whose surface holds a seat's pointer grab:
//! the compositor takes a drag only under the serial of the press that began
//! the grab on its origin surface (`LastInput::grab_on`). The source offers
//! each representation under the type names it is read as
//! (`offers::names_of`), each once, with the actions the session allows, and
//! the seat's device starts the drag; the compositor then moves it, shows no
//! icon and ends it on Escape.
//!
//! The source's events reach [`Sources`] through its dispatch, which records
//! them, and [`super::SeatDrag::deliver`] handles them on the loop's turn,
//! with no cell borrowed:
//!
//! - **`send`** asks for a type into a pipe: the provider serves the
//!   representation it names, and a thread of its own writes it, at most
//!   [`MAX_WRITERS`] at once, each held to [`super::TRANSFER_TIMEOUT`].
//! - **`action`** is what the compositor chose from the source's and the
//!   target's actions; the last one told is the session's.
//! - **`dnd_drop_performed`** says the user dropped; the target reads on.
//! - **`dnd_finished`** ends the session as dropped with the last action;
//!   **`cancelled`** ends it as cancelled, before or after a drop.
//!
//! A session whose window closes, or whose loop ends, ends cancelled; its
//! source is destroyed, which ends the drag in the compositor too. So does
//! one whose grab a release ended at the window: a compositor ignores a drag
//! started under a grab that ended before it heard the start, and tells the
//! source nothing, while a drag it takes leaves the window before the
//! release.

use std::collections::VecDeque;
use std::os::fd::OwnedFd;
use std::sync::Weak;

use flui_platform::data_transfer::{RepresentationIndex, TransferActions};
use flui_platform::drag::{
    DragError, OsDragEnd, OsDragEndHook, OsDragProvider, OsDragRequest, OsDragStart,
    PlatformDragSource,
};
use flui_platform::{Unsupported, UnsupportedReason, WindowId};
use wayland_client::protocol::wl_data_device_manager::DndAction;
use wayland_client::protocol::wl_data_source::{self, WlDataSource};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, Proxy as _, QueueHandle};
use web_time::Instant;

use super::{ACTIONS_VERSION, SeatDrag, Shared, TRANSFER_TIMEOUT, transfer_actions};
use crate::drag::offers::{bytes_of, names_of};
use crate::owner::linux::wayland::WaylandState;
use crate::owner::panic_boundary::contain_owner_callback;
use crate::transfer::linux::wayland::pipe;

/// The writes of a session's data at once; a `send` past them is answered
/// with nothing.
pub(super) const MAX_WRITERS: usize = 4;

/// The session a window started, on the owner thread.
struct Session {
    source: WlDataSource,
    window: WindowId,
    /// Each type name offered and the representation it serves.
    types: Vec<(String, RepresentationIndex)>,
    /// Taken while it serves, so it runs with no cell borrowed.
    provider: Option<OsDragProvider>,
    on_end: Option<OsDragEndHook>,
    /// The action the compositor told last.
    action: TransferActions,
    /// The seat and the press serial of the grab it started under.
    grab: (WlSeat, u32),
}

/// What a source's dispatch recorded, handled on the loop's turn.
enum SourceWire {
    Send { mime: String, fd: OwnedFd },
    Action(DndAction),
    Dropped,
    Finished,
    Cancelled,
}

/// The sessions' state, on the owner thread.
#[derive(Default)]
pub(super) struct Sources {
    session: Option<Session>,
    wires: VecDeque<(WlDataSource, SourceWire)>,
}

/// A source's user data.
pub(super) struct SourceData {
    shared: Weak<Shared>,
}

impl SeatDrag {
    /// Start `request`'s session from `window`, refused when the window is
    /// closed, holds no pointer grab, or a session runs already.
    fn start_session(&self, window: WindowId, request: OsDragRequest) -> Result<(), DragError> {
        let refused = || DragError::from(Unsupported::new(UnsupportedReason::Unavailable));
        let busy = self
            .shared
            .sources
            .with(|sources| sources.session.is_some())
            .ok_or_else(refused)?;
        if busy {
            return Err(refused());
        }
        let surface = {
            let state = self.shared.state.lock();
            state
                .windows
                .iter()
                .find(|held| held.id == window)
                .map(|held| held.surface.clone())
        }
        .ok_or_else(refused)?;
        let grab = self
            .shared
            .last_input
            .grab_on(&surface)
            .ok_or_else(refused)?;
        let data = SourceData {
            shared: std::sync::Arc::downgrade(&self.shared),
        };
        let source = self
            .shared
            .devices
            .create_source(data)
            .ok_or_else(refused)?;
        // A version 1 or 2 source hears no drop or finish, so its session
        // could not end: none starts.
        if source.version() < ACTIONS_VERSION {
            source.destroy();
            return Err(refused());
        }
        let OsDragRequest {
            representations,
            allowed,
            provider,
            on_end,
            ..
        } = request;
        let mut types: Vec<(String, RepresentationIndex)> = Vec::new();
        for (index, descriptor) in representations.iter().enumerate() {
            let Ok(index) = u16::try_from(index) else {
                break;
            };
            for name in names_of(&descriptor.format) {
                if types.iter().all(|(held, _)| *held != name) {
                    source.offer(name.clone());
                    types.push((name, RepresentationIndex(index)));
                }
            }
        }
        source.set_actions(offered(allowed));
        if !self
            .shared
            .devices
            .start_drag(&grab.seat, &source, &grab.surface, grab.serial)
        {
            source.destroy();
            return Err(refused());
        }
        let session = Session {
            source,
            window,
            types,
            provider: Some(provider),
            on_end: Some(on_end),
            action: TransferActions::empty(),
            grab: (grab.seat.clone(), grab.serial),
        };
        self.shared
            .sources
            .with(|sources| sources.session = Some(session));
        Ok(())
    }

    /// Handle what the sources' dispatch recorded.
    pub(super) fn deliver_sources(&self) {
        while let Some((source, wire)) = self
            .shared
            .sources
            .with(|sources| sources.wires.pop_front())
            .flatten()
        {
            self.source_wire(&source, wire);
        }
        // A release reached the window under the grab the session started
        // under: the compositor took no drag from it, and says nothing.
        let ignored = self
            .shared
            .sources
            .with(|sources| {
                let (seat, serial) = &sources.session.as_ref()?.grab;
                Some(self.shared.last_input.released_grab(seat, *serial))
            })
            .flatten()
            .unwrap_or(false);
        if ignored {
            self.end_session(OsDragEnd::Cancelled);
        }
    }

    fn source_wire(&self, source: &WlDataSource, wire: SourceWire) {
        let current = self
            .shared
            .sources
            .with(|sources| {
                sources
                    .session
                    .as_ref()
                    .is_some_and(|session| session.source == *source)
            })
            .unwrap_or(false);
        if !current {
            // A source a session no longer holds was destroyed with it; a
            // pipe it was asked to fill closes unwritten.
            return;
        }
        match wire {
            SourceWire::Send { mime, fd } => self.serve(&mime, fd),
            SourceWire::Action(action) => {
                self.shared.sources.with(|sources| {
                    if let Some(session) = sources.session.as_mut() {
                        session.action = transfer_actions(action);
                    }
                });
            }
            SourceWire::Dropped => {}
            SourceWire::Finished => {
                let action = self
                    .shared
                    .sources
                    .with(|sources| sources.session.as_ref().map(|session| session.action))
                    .flatten()
                    .unwrap_or_else(TransferActions::empty);
                let outcome = if action.is_empty() {
                    OsDragEnd::Cancelled
                } else {
                    OsDragEnd::Dropped(action)
                };
                self.end_session(outcome);
            }
            SourceWire::Cancelled => self.end_session(OsDragEnd::Cancelled),
        }
    }

    /// Write the representation `mime` names into `fd`, from a thread of
    /// its own; the provider is asked with no cell borrowed.
    fn serve(&self, mime: &str, fd: OwnedFd) {
        let taken = self
            .shared
            .sources
            .with(|sources| {
                let session = sources.session.as_mut()?;
                let (_, index) = session.types.iter().find(|(name, _)| name == mime)?;
                Some((*index, session.provider.take()?))
            })
            .flatten();
        // A type the source did not offer is answered with nothing: dropping
        // the descriptor ends the reader's read.
        let Some((index, mut provider)) = taken else {
            return;
        };
        let mut served = None;
        contain_owner_callback(|| served = Some(provider(index)));
        // The provider goes back to its session if that one still runs.
        let provider = self
            .shared
            .sources
            .with(|sources| match sources.session.as_mut() {
                Some(session) if session.provider.is_none() => {
                    session.provider = Some(provider);
                    None
                }
                _ => Some(provider),
            })
            .flatten();
        if let Some(provider) = provider {
            contain_owner_callback(|| drop(provider));
        }
        let bytes = match served {
            Some(Ok(payload)) => bytes_of(payload),
            Some(Err(error)) => {
                tracing::debug!(%error, "a drag's provider refused a type");
                return;
            }
            None => return,
        };
        let Some(permit) = self.shared.writers.permit() else {
            tracing::debug!("a drag's write past {MAX_WRITERS} at once was refused");
            return;
        };
        let spawned = std::thread::Builder::new()
            .name("flui-wayland-drag".to_owned())
            .spawn(move || {
                pipe::write(fd, &bytes, Instant::now() + TRANSFER_TIMEOUT);
                drop(permit);
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "no thread for a drag's write");
        }
    }

    /// End the session with `outcome`: its source is destroyed, and its
    /// hook told, with no cell borrowed.
    pub(super) fn end_session(&self, outcome: OsDragEnd) {
        let Some(mut session) = self
            .shared
            .sources
            .with(|sources| sources.session.take())
            .flatten()
        else {
            return;
        };
        session.source.destroy();
        let on_end = session.on_end.take();
        // The provider and the rest are the runtime's: dropped contained, so
        // a capture's failing destructor leaves the hook its outcome
        // (ADR-0127 §2).
        contain_owner_callback(|| drop(session));
        if let Some(on_end) = on_end {
            contain_owner_callback(|| on_end(outcome));
        }
    }

    /// The loop ends: a session still running ends cancelled, and what its
    /// source heard goes unread.
    pub(super) fn release_session(&self) {
        self.shared.sources.with(|sources| sources.wires.clear());
        self.end_session(OsDragEnd::Cancelled);
    }

    /// `window` closes: a session it started ends on the loop's turn.
    pub(super) fn window_closed(&self, window: WindowId) {
        self.shared.sources.with(|sources| {
            let source = sources
                .session
                .as_ref()
                .filter(|session| session.window == window)
                .map(|session| session.source.clone());
            if let Some(source) = source {
                sources.wires.push_back((source, SourceWire::Cancelled));
            }
        });
    }
}

/// The actions a source offers for `allowed`: copy and move, as Wayland
/// names them.
fn offered(allowed: TransferActions) -> DndAction {
    let mut actions = DndAction::empty();
    if allowed.contains(TransferActions::COPY) {
        actions |= DndAction::Copy;
    }
    if allowed.contains(TransferActions::MOVE) {
        actions |= DndAction::Move;
    }
    actions
}

/// A window's drag source: its sessions run on the host's drag.
pub(super) struct WindowDragSource {
    pub(super) drag: std::sync::Arc<SeatDrag>,
    pub(super) window: WindowId,
}

impl PlatformDragSource for WindowDragSource {
    fn start_timing(&self) -> OsDragStart {
        crate::input::availability::DESKTOP_DRAG_START
    }

    fn start(&self, request: OsDragRequest) -> Result<(), DragError> {
        self.drag.start_session(self.window, request)
    }
}

// ---- The dispatch of a session's source on the owner's queue: it records
// what the compositor said, and runs no handler.

impl Dispatch<WlDataSource, SourceData> for WaylandState {
    fn event(
        _: &mut Self,
        source: &WlDataSource,
        event: wl_data_source::Event,
        data: &SourceData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let wire = match event {
            wl_data_source::Event::Send { mime_type, fd } => SourceWire::Send {
                mime: mime_type,
                fd,
            },
            wl_data_source::Event::Action { dnd_action } => {
                SourceWire::Action(dnd_action.into_result().unwrap_or(DndAction::empty()))
            }
            wl_data_source::Event::DndDropPerformed => SourceWire::Dropped,
            wl_data_source::Event::DndFinished => SourceWire::Finished,
            wl_data_source::Event::Cancelled => SourceWire::Cancelled,
            _ => return,
        };
        let Some(shared) = data.shared.upgrade() else {
            // After the drag's release the source is still the protocol's
            // to destroy.
            if matches!(wire, SourceWire::Cancelled) {
                source.destroy();
            }
            return;
        };
        shared
            .sources
            .with(|sources| sources.wires.push_back((source.clone(), wire)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A session offers copy and move as the actions it allows, and none it
    /// does not.
    #[test]
    fn a_source_offers_the_actions_it_allows() {
        assert_eq!(offered(TransferActions::COPY), DndAction::Copy);
        assert_eq!(
            offered(TransferActions::COPY | TransferActions::MOVE),
            DndAction::Copy | DndAction::Move
        );
        assert_eq!(offered(TransferActions::empty()), DndAction::empty());
    }
}
