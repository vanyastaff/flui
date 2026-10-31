//! A window's OS drag session over XDND: the window as the source.
//!
//! The session starts when the runtime's drag leaves its window, with a
//! button still held, so the press's implicit grab keeps the pointer's
//! XInput 2 events coming to the window wherever it goes. The window owns
//! `XdndSelection` and lists its types in `XdndTypeList`. Each movement finds
//! the `XdndAware` window under the pointer, FLUI's own included, entering
//! and leaving targets as the pointer crosses them. A position goes out once
//! the last one is answered, the newest held meanwhile; a release while one
//! waits sends the held one first. The release of the drag's own button drops
//! on a target that accepted with an action the runtime allows and leaves one
//! that did not. Escape leaves and cancels. A drop ends at the target's
//! `XdndFinished` with an allowed action, or as cancelled when none comes
//! within [`FINISH_TIMEOUT`]. Another client taking `XdndSelection` ends the
//! session as cancelled.
//!
//! The session's pointer events are its alone while it runs, as an OS
//! session's are on every desktop: the window's input never sees them, and
//! ends the press the session took at the loop's next turn.
//!
//! The session lives in an owner-thread cell. Its provider and end hook are
//! the runtime's code: each is taken out of the cell before it runs, so
//! either may start, serve or end a session again.

use std::time::Duration;

use flui_platform::data_transfer::{RepresentationIndex, TransferActions};
use flui_platform::drag::{
    DragError, OsDragEnd, OsDragEndHook, OsDragProvider, OsDragRequest, OsDragStart,
    PlatformDragSource,
};
use flui_platform::{Unsupported, UnsupportedReason};
use web_time::Instant;
use x11rb::connection::{Connection as _, RequestConnection as _};
use x11rb::protocol::Event;
use x11rb::protocol::xinput;
use x11rb::protocol::xproto::{
    self, AtomEnum, ConnectionExt as _, EventMask, KeyButMask, PropMode, SELECTION_NOTIFY_EVENT,
    SelectionNotifyEvent, SelectionRequestEvent,
};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{CURRENT_TIME, NONE};

use super::model::{self, Reply};
use super::{Atoms, X11Drag};
use crate::owner::linux::x11::WindowRef;
use crate::owner::panic_boundary::contain_owner_callback;

/// How long a target has to finish a drop before the session ends as
/// cancelled, and to answer the last position before a release decides.
pub(super) const FINISH_TIMEOUT: Duration = Duration::from_secs(5);

/// The keysym of Escape.
const ESCAPE: xproto::Keysym = 0xff1b;

/// How deep the search for the target under the pointer descends.
const MAX_DEPTH: usize = 64;

/// A session under way.
pub(super) struct Session {
    /// The window the drag started in, which owns `XdndSelection`.
    window: xproto::Window,
    /// Each offered type and the representation it serves.
    types: Vec<(xproto::Atom, RepresentationIndex)>,
    /// The action proposed to every target.
    action: xproto::Atom,
    /// The actions the runtime allows: a target's answer outside them is
    /// a refusal.
    allowed: TransferActions,
    /// The button the drag holds, whose release ends it; another button's,
    /// a wheel notch's among them, does not.
    button: u32,
    /// Whether the window still owns `XdndSelection`; another client may
    /// take it.
    owns: bool,
    /// `None` while it serves a request.
    provider: Option<OsDragProvider>,
    /// `None` once the session ended.
    on_end: Option<OsDragEndHook>,
    /// The keycodes Escape is on.
    escape: Vec<xproto::Keycode>,
    over: Option<Over>,
    phase: Phase,
}

/// The target under the pointer.
struct Over {
    target: xproto::Window,
    version: u32,
    /// The action it accepts, from its last status.
    accepted: Option<xproto::Atom>,
    /// Whether a position waits for its status.
    asked: bool,
    /// The newest position held while one waits.
    held: Option<((i16, i16), xproto::Timestamp)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Dragging,
    /// Released at `time` while a position waited: its status decides.
    Released {
        time: xproto::Timestamp,
        deadline: Instant,
    },
    /// Dropped, waiting for `XdndFinished`.
    Dropped {
        deadline: Instant,
    },
}

/// What a step of the session leaves to run with the cell let go.
enum Then {
    Nothing,
    /// The session ended: its hook hears how.
    End(OsDragEndHook, OsDragEnd),
}

impl X11Drag {
    /// Start `request`'s session from `window`.
    pub(super) fn start_session(
        &self,
        window: xproto::Window,
        request: OsDragRequest,
    ) -> Result<(), DragError> {
        let refused = |reason| DragError::from(Unsupported::new(reason));
        let busy = self
            .session
            .with(|session| session.is_some())
            .ok_or_else(|| refused(UnsupportedReason::Unavailable))?;
        // One session at a time; and a session no button holds would never
        // hear its release.
        let (button, root) = (!busy)
            .then(|| self.button_held())
            .flatten()
            .ok_or_else(|| refused(UnsupportedReason::Unavailable))?;
        let OsDragRequest {
            representations,
            allowed,
            provider,
            on_end,
            ..
        } = request;
        let mut types = Vec::new();
        for (index, descriptor) in representations.iter().enumerate() {
            let Ok(index) = u16::try_from(index) else {
                break;
            };
            for name in model::names_of(&descriptor.format) {
                if let Some(atom) = self.intern(&name)
                    && types.iter().all(|(held, _)| *held != atom)
                {
                    types.push((atom, RepresentationIndex(index)));
                }
            }
        }
        let action = self.atoms.actions.atom_of(proposed(allowed));
        let time = self.time();
        let xcb = self.connection.xcb();
        let atoms: Vec<xproto::Atom> = types.iter().map(|(atom, _)| *atom).collect();
        let owned = xcb
            .change_property32(
                PropMode::REPLACE,
                window,
                self.atoms.type_list,
                AtomEnum::ATOM,
                &atoms,
            )
            .map(drop)
            .and_then(|()| {
                xcb.set_selection_owner(window, self.atoms.selection, time)
                    .map(drop)
            });
        if let Err(error) = owned {
            tracing::debug!(%error, "cannot take XdndSelection for a drag");
            return Err(refused(UnsupportedReason::Unavailable));
        }
        // The server ignores a claim older than the selection's last change,
        // and says nothing: a session starts only on the selection it owns,
        // or another client would answer its drop.
        let owner = xcb
            .get_selection_owner(self.atoms.selection)
            .map_err(x11rb::errors::ReplyError::from)
            .and_then(x11rb::cookie::Cookie::reply);
        match owner {
            Ok(reply) if reply.owner == window => {}
            Ok(_) => {
                tracing::debug!("another client holds XdndSelection past the drag's time");
                return Err(refused(UnsupportedReason::Unavailable));
            }
            Err(error) => {
                tracing::debug!(%error, "cannot ask who holds XdndSelection");
                return Err(refused(UnsupportedReason::Unavailable));
            }
        }
        let session = Session {
            window,
            types,
            action,
            allowed,
            button,
            owns: true,
            provider: Some(provider),
            on_end: Some(on_end),
            escape: self.escape_keys(),
            over: None,
            phase: Phase::Dragging,
        };
        self.session.with(|slot| *slot = Some(session));
        // The press the window's input holds gets no release now: the loop
        // has the input end it, as the compositor's leave does on Wayland.
        self.taken.with(|taken| taken.push(window));
        self.flush();
        // The pointer may already be over a target: it hears the session
        // there, before any movement, so a release in place can drop.
        self.moved(root, self.time());
        Ok(())
    }

    /// The button held down now, the lowest of the three a drag can hold
    /// (buttons 4 and 5 are a wheel's notches), and where the pointer is on
    /// the root window.
    fn button_held(&self) -> Option<(u32, (i16, i16))> {
        let pointer = self
            .connection
            .xcb()
            .query_pointer(self.connection.root())
            .ok()?
            .reply()
            .ok()?;
        [
            (KeyButMask::BUTTON1, 1),
            (KeyButMask::BUTTON2, 2),
            (KeyButMask::BUTTON3, 3),
        ]
        .into_iter()
        .find(|(held, _)| pointer.mask.contains(*held))
        .map(|(_, button)| (button, (pointer.root_x, pointer.root_y)))
    }

    /// The windows whose press a session took since the last call.
    pub(super) fn taken_pointers(&self) -> Vec<xproto::Window> {
        self.taken.with(std::mem::take).unwrap_or_default()
    }

    /// The atom named `name`.
    fn intern(&self, name: &str) -> Option<xproto::Atom> {
        let reply = self
            .connection
            .xcb()
            .intern_atom(false, name.as_bytes())
            .ok()?
            .reply()
            .ok()?;
        Some(reply.atom)
    }

    /// The keycodes Escape is on now.
    fn escape_keys(&self) -> Vec<xproto::Keycode> {
        let setup = self.connection.xcb().setup();
        let (first, last) = (setup.min_keycode, setup.max_keycode);
        let Some(mapping) = self
            .connection
            .xcb()
            .get_keyboard_mapping(first, last.saturating_sub(first).saturating_add(1))
            .ok()
            .and_then(|cookie| cookie.reply().ok())
        else {
            return Vec::new();
        };
        let per = usize::from(mapping.keysyms_per_keycode).max(1);
        mapping
            .keysyms
            .chunks(per)
            .zip(first..=last)
            .filter(|(keysyms, _)| keysyms.contains(&ESCAPE))
            .map(|(_, keycode)| keycode)
            .collect()
    }

    /// The session's part of `event`; `true` when it was the session's alone.
    pub(super) fn session_event(&self, event: &Event) -> bool {
        if let Some(time) = time_of(event) {
            // The time a session takes the selection at, as ICCCM asks.
            self.saw(time);
        }
        let Some((window, button)) = self
            .session
            .with(|session| {
                session
                    .as_ref()
                    .map(|session| (session.window, session.button))
            })
            .flatten()
        else {
            return false;
        };
        match event {
            Event::XinputMotion(motion) if motion.event == window => {
                self.moved(root_of(motion.root_x, motion.root_y), motion.time);
                true
            }
            Event::XinputButtonPress(press) if press.event == window => true,
            Event::XinputButtonRelease(release) if release.event == window => {
                // Another button, or a wheel's notch, goes by.
                if release.detail == button {
                    self.released(release.time);
                }
                true
            }
            Event::XinputKeyPress(key) if key.event == window => {
                let escape = u8::try_from(key.detail).is_ok_and(|keycode| {
                    self.session
                        .with(|session| {
                            session
                                .as_ref()
                                .is_some_and(|session| session.escape.contains(&keycode))
                        })
                        .unwrap_or(false)
                });
                if escape {
                    self.cancel();
                }
                escape
            }
            Event::ClientMessage(message) if message.format == 32 && message.window == window => {
                let version = self
                    .session
                    .with(|session| {
                        session
                            .as_ref()
                            .and_then(|session| session.over.as_ref())
                            .map_or(model::VERSION, |over| over.version)
                    })
                    .unwrap_or(model::VERSION);
                let Some(reply) = Reply::read(
                    message.type_,
                    message.data.as_data32(),
                    version,
                    &self.atoms.replies(),
                ) else {
                    return false;
                };
                self.replied(reply);
                true
            }
            Event::SelectionRequest(request)
                if request.owner == window && request.selection == self.atoms.selection =>
            {
                self.serve(request);
                true
            }
            // Unmapped, the window loses the grab, and the release may never
            // come to it.
            Event::UnmapNotify(unmap) if unmap.window == window => {
                self.cancel();
                false
            }
            Event::SelectionClear(clear)
                if clear.owner == window && clear.selection == self.atoms.selection =>
            {
                self.selection_cleared();
                true
            }
            _ => false,
        }
    }

    /// The window was told it lost `XdndSelection`. A clear the session's own
    /// release of an earlier ownership left is stale: the server's owner says
    /// which it is. Lost, the session can serve nothing more: it leaves its
    /// target and ends, letting go of no selection, which is another's now.
    fn selection_cleared(&self) {
        let Some(owner) = self
            .connection
            .xcb()
            .get_selection_owner(self.atoms.selection)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| reply.owner)
        else {
            return;
        };
        let Some(mut session) = self.take_session() else {
            return;
        };
        let then = if owner == session.window {
            Then::Nothing
        } else {
            session.owns = false;
            // A target that took the drop has had its leave.
            let dropped = matches!(session.phase, Phase::Dropped { .. });
            if let Some(over) = session.over.take().filter(|_| !dropped) {
                self.send(
                    over.target,
                    self.atoms.messages.leave,
                    model::leave(session.window),
                );
            }
            self.end(&mut session, OsDragEnd::Cancelled)
        };
        self.finish_step(session, then);
    }

    /// The pointer is at `root`.
    fn moved(&self, root: (i16, i16), time: xproto::Timestamp) {
        let found = self.target_at(root);
        let Some(mut session) = self.take_session() else {
            return;
        };
        if session.phase == Phase::Dragging {
            let left = session
                .over
                .as_ref()
                .is_some_and(|over| Some(over.target) != found.map(|(target, _)| target));
            if left && let Some(over) = session.over.take() {
                self.send(
                    over.target,
                    self.atoms.messages.leave,
                    model::leave(session.window),
                );
            }
            if session.over.is_none()
                && let Some((target, version)) = found
            {
                let types: Vec<xproto::Atom> =
                    session.types.iter().map(|(atom, _)| *atom).collect();
                self.send(
                    target,
                    self.atoms.messages.enter,
                    model::enter(session.window, version, &types),
                );
                session.over = Some(Over {
                    target,
                    version,
                    accepted: None,
                    asked: false,
                    held: None,
                });
            }
            let (source, action) = (session.window, session.action);
            if let Some(over) = session.over.as_mut() {
                if over.asked {
                    over.held = Some((root, time));
                } else {
                    over.asked = true;
                    self.send(
                        over.target,
                        self.atoms.messages.position,
                        model::position(source, root, time, action),
                    );
                }
            }
            self.flush();
        }
        self.put_session(session);
    }

    /// The button was let go at `time`.
    fn released(&self, time: xproto::Timestamp) {
        let Some(mut session) = self.take_session() else {
            return;
        };
        if session.phase != Phase::Dragging {
            self.put_session(session);
            return;
        }
        let then = match session.over.as_ref() {
            Some(over) if over.asked => {
                session.phase = Phase::Released {
                    time,
                    deadline: Instant::now() + FINISH_TIMEOUT,
                };
                Then::Nothing
            }
            _ => self.decide(&mut session, time),
        };
        self.finish_step(session, then);
    }

    /// Drop on a target that accepted at the release, at `time`, or leave
    /// it.
    fn decide(&self, session: &mut Session, time: xproto::Timestamp) -> Then {
        // A target accepting with an action the runtime did not allow
        // refuses. A private action, which FLUI has no name for, is left to
        // the finish, whose action must be allowed.
        let allowed = |atom: xproto::Atom| {
            let actions = self.atoms.actions.actions_of(atom);
            actions.is_empty() || !(actions & session.allowed).is_empty()
        };
        match session.over.as_ref() {
            Some(over) if over.accepted.is_some_and(allowed) => {
                self.send(
                    over.target,
                    self.atoms.messages.drop,
                    model::drop_at(session.window, time),
                );
                session.phase = Phase::Dropped {
                    deadline: Instant::now() + FINISH_TIMEOUT,
                };
                self.flush();
                Then::Nothing
            }
            _ => {
                if let Some(over) = session.over.take() {
                    self.send(
                        over.target,
                        self.atoms.messages.leave,
                        model::leave(session.window),
                    );
                }
                self.end(session, OsDragEnd::Cancelled)
            }
        }
    }

    /// A target answered.
    fn replied(&self, reply: Reply) {
        let Some(mut session) = self.take_session() else {
            return;
        };
        let source = session.window;
        let action = session.action;
        let then = match reply {
            Reply::Status {
                target,
                action: accepted,
            } => {
                let mut then = Then::Nothing;
                if let Some(over) = session.over.as_mut().filter(|over| over.target == target) {
                    over.accepted = accepted;
                    over.asked = false;
                    match session.phase {
                        Phase::Dragging => {
                            if let Some((root, time)) = over.held.take() {
                                over.asked = true;
                                self.send(
                                    target,
                                    self.atoms.messages.position,
                                    model::position(source, root, time, action),
                                );
                                self.flush();
                            }
                        }
                        Phase::Released { time, .. } => {
                            // The pointer moved after the position this
                            // answers: the drop waits for the answer to the
                            // last one, where the button was let go.
                            if let Some((root, moved)) = over.held.take() {
                                over.asked = true;
                                self.send(
                                    target,
                                    self.atoms.messages.position,
                                    model::position(source, root, moved, action),
                                );
                                self.flush();
                            } else {
                                then = self.decide(&mut session, time);
                            }
                        }
                        Phase::Dropped { .. } => {}
                    }
                }
                then
            }
            Reply::Finished {
                target,
                action: done,
            } => {
                let dropped = matches!(session.phase, Phase::Dropped { .. })
                    && session
                        .over
                        .as_ref()
                        .is_some_and(|over| over.target == target);
                if dropped {
                    let accepted = session.over.as_ref().and_then(|over| over.accepted);
                    let end = match done {
                        Some(atom) => {
                            let named = self.atoms.actions.actions_of(atom);
                            let resolved = if named.is_empty() {
                                accepted.map(|atom| self.atoms.actions.actions_of(atom))
                            } else {
                                Some(named)
                            };
                            // An action the runtime did not allow is the
                            // target's to claim, not the drop's outcome.
                            resolved
                                .map(|actions| actions & session.allowed)
                                .filter(|actions| !actions.is_empty())
                                .map_or(OsDragEnd::Cancelled, OsDragEnd::Dropped)
                        }
                        None => OsDragEnd::Cancelled,
                    };
                    self.end(&mut session, end)
                } else {
                    Then::Nothing
                }
            }
        };
        self.finish_step(session, then);
    }

    /// Escape, or the window unmapped: leave the target and end; a drop is
    /// the target's already.
    fn cancel(&self) {
        let Some(mut session) = self.take_session() else {
            return;
        };
        let then = if matches!(session.phase, Phase::Dropped { .. }) {
            // The drop is the target's now.
            Then::Nothing
        } else {
            if let Some(over) = session.over.take() {
                self.send(
                    over.target,
                    self.atoms.messages.leave,
                    model::leave(session.window),
                );
            }
            self.end(&mut session, OsDragEnd::Cancelled)
        };
        self.finish_step(session, then);
    }

    /// End a session whose window closed.
    pub(super) fn session_window_closed(&self, window: xproto::Window) {
        let Some(mut session) = self.take_session() else {
            return;
        };
        let then = if session.window == window {
            if let Some(over) = session.over.take() {
                self.send(over.target, self.atoms.messages.leave, model::leave(window));
            }
            self.end(&mut session, OsDragEnd::Cancelled)
        } else {
            Then::Nothing
        };
        self.finish_step(session, then);
    }

    /// When the session gives up on its target.
    pub(super) fn session_deadline(&self) -> Option<Instant> {
        self.session
            .with(|session| match session.as_ref()?.phase {
                Phase::Dragging => None,
                Phase::Released { deadline, .. } | Phase::Dropped { deadline } => Some(deadline),
            })
            .flatten()
    }

    /// End a session whose target did not answer by `now`.
    pub(super) fn session_expire(&self, now: Instant) {
        if self
            .session_deadline()
            .is_none_or(|deadline| deadline > now)
        {
            return;
        }
        let Some(mut session) = self.take_session() else {
            return;
        };
        if let (Phase::Released { .. }, Some(over)) = (session.phase, session.over.take()) {
            self.send(
                over.target,
                self.atoms.messages.leave,
                model::leave(session.window),
            );
        }
        let then = self.end(&mut session, OsDragEnd::Cancelled);
        self.finish_step(session, then);
    }

    /// End `session` with `outcome`: it lets go of `XdndSelection`, while it
    /// still owns it, and of its type list, and its hook is to hear
    /// `outcome`.
    fn end(&self, session: &mut Session, outcome: OsDragEnd) -> Then {
        let xcb = self.connection.xcb();
        let released = if session.owns {
            xcb.set_selection_owner(NONE, self.atoms.selection, self.time())
                .map(drop)
        } else {
            Ok(())
        }
        .and_then(|()| {
            xcb.delete_property(session.window, self.atoms.type_list)
                .map(drop)
        });
        if let Err(error) = released {
            tracing::debug!(%error, "cannot let go of a drag's selection");
        }
        self.flush();
        session.over = None;
        match session.on_end.take() {
            Some(on_end) => Then::End(on_end, outcome),
            None => Then::Nothing,
        }
    }

    /// Put `session` back unless it ended, then run what the step left.
    fn finish_step(&self, session: Session, then: Then) {
        if session.on_end.is_some() {
            self.put_session(session);
        } else {
            // The provider and the rest are the runtime's: dropped with no
            // cell borrowed, and contained, so a capture's failing
            // destructor leaves the hook its outcome (ADR-0127 §2).
            contain_owner_callback(|| drop(session));
        }
        if let Then::End(on_end, outcome) = then {
            contain_owner_callback(|| on_end(outcome));
        }
    }

    /// Answer a target's conversion of `XdndSelection`.
    fn serve(&self, request: &SelectionRequestEvent) {
        let property = if request.property == NONE {
            request.target
        } else {
            request.property
        };
        let answered = if request.target == self.atoms.targets {
            let atoms: Vec<xproto::Atom> = self
                .session
                .with(|session| {
                    session
                        .as_ref()
                        .map(|session| session.types.iter().map(|(atom, _)| *atom).collect())
                })
                .flatten()
                .unwrap_or_default();
            let mut atoms = atoms;
            atoms.push(self.atoms.targets);
            self.connection
                .xcb()
                .change_property32(
                    PropMode::REPLACE,
                    request.requestor,
                    property,
                    AtomEnum::ATOM,
                    &atoms,
                )
                .is_ok()
        } else if !self.dropped_on_client_of(request.requestor) {
            // The data is the drop's: no window hovered over, and no client
            // but the target's, reads it before the target took the drop.
            // The types stay listed, as `XdndTypeList` lists them anyway.
            false
        } else {
            self.provide(request.target).is_some_and(|bytes| {
                // An answer past one request would need `INCR`, which the
                // source does not write yet.
                let room = self
                    .connection
                    .xcb()
                    .maximum_request_bytes()
                    .saturating_sub(64);
                if bytes.len() > room {
                    tracing::debug!(len = bytes.len(), "a drag's payload is past one request");
                    return false;
                }
                self.connection
                    .xcb()
                    .change_property8(
                        PropMode::REPLACE,
                        request.requestor,
                        property,
                        request.target,
                        &bytes,
                    )
                    .is_ok()
            })
        };
        let notify = SelectionNotifyEvent {
            response_type: SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: request.time,
            requestor: request.requestor,
            selection: request.selection,
            target: request.target,
            property: if answered { property } else { NONE },
        };
        if let Err(error) =
            self.connection
                .xcb()
                .send_event(false, request.requestor, EventMask::NO_EVENT, notify)
        {
            tracing::debug!(%error, "cannot answer a drag's conversion");
        }
        self.flush();
    }

    /// Whether the session dropped on a target of the client that made
    /// `requestor`: a client's windows share the base of their X ids.
    fn dropped_on_client_of(&self, requestor: xproto::Window) -> bool {
        let mask = self.connection.xcb().setup().resource_id_mask;
        self.session
            .with(|session| {
                session.as_ref().is_some_and(|session| {
                    matches!(session.phase, Phase::Dropped { .. })
                        && session
                            .over
                            .as_ref()
                            .is_some_and(|over| over.target & !mask == requestor & !mask)
                })
            })
            .unwrap_or(false)
    }

    /// The bytes the session's provider serves for `target`, asked with no
    /// cell borrowed.
    fn provide(&self, target: xproto::Atom) -> Option<Vec<u8>> {
        let (index, mut provider) = self
            .session
            .with(|session| {
                let session = session.as_mut()?;
                let (_, index) = session.types.iter().find(|(atom, _)| *atom == target)?;
                Some((*index, session.provider.take()?))
            })
            .flatten()?;
        let mut served = None;
        contain_owner_callback(|| served = Some(provider(index)));
        // The provider goes back to the session it came from, if that one
        // still runs; a later session's is its own.
        let provider = self
            .session
            .with(|session| match session.as_mut() {
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
        match served? {
            Ok(payload) => Some(model::bytes_of(payload)),
            Err(error) => {
                tracing::debug!(%error, "a drag's provider refused a conversion");
                None
            }
        }
    }

    /// The `XdndAware` window under `root` and the version both speak, the
    /// topmost the search finds descending from the root window.
    fn target_at(&self, root: (i16, i16)) -> Option<(xproto::Window, u32)> {
        let xcb = self.connection.xcb();
        let top = self.connection.root();
        let mut window = top;
        for _ in 0..MAX_DEPTH {
            let child = xcb
                .translate_coordinates(top, window, root.0, root.1)
                .ok()?
                .reply()
                .ok()?
                .child;
            if child == NONE {
                return None;
            }
            window = child;
            let aware = xcb
                .get_property(false, window, self.atoms.aware, AtomEnum::ATOM, 0, 1)
                .ok()?
                .reply()
                .ok()?
                .value32()
                .and_then(|mut words| words.next());
            if let Some(version) = aware {
                return (version >= model::OLDEST).then(|| (window, version.min(model::VERSION)));
            }
        }
        None
    }

    fn take_session(&self) -> Option<Session> {
        self.session.with(Option::take).flatten()
    }

    fn put_session(&self, session: Session) {
        self.session.with(|slot| *slot = Some(session));
    }

    /// Note the server time of a pointer or key event.
    fn saw(&self, time: xproto::Timestamp) {
        self.last_time
            .store(time, std::sync::atomic::Ordering::Relaxed);
    }

    /// The last server time the transfer heard, or `CurrentTime` before any.
    fn time(&self) -> xproto::Timestamp {
        match self.last_time.load(std::sync::atomic::Ordering::Relaxed) {
            0 => CURRENT_TIME,
            time => time,
        }
    }
}

/// The action proposed for `allowed`: a copy where it may, then a move, then
/// a link.
fn proposed(allowed: TransferActions) -> TransferActions {
    [
        TransferActions::COPY,
        TransferActions::MOVE,
        TransferActions::LINK,
    ]
    .into_iter()
    .find(|action| allowed.contains(*action))
    .unwrap_or(TransferActions::COPY)
}

/// A root position in XInput 2's fixed point, as XDND's 16-bit pixels.
fn root_of(x: xinput::Fp1616, y: xinput::Fp1616) -> (i16, i16) {
    let pixel = |value: xinput::Fp1616| {
        i16::try_from(value >> 16).unwrap_or(if value < 0 { i16::MIN } else { i16::MAX })
    };
    (pixel(x), pixel(y))
}

/// A window's drag source: its sessions run on the transfer of its loop.
pub(crate) struct X11DragSource {
    drag: std::sync::Arc<X11Drag>,
    window: WindowRef,
}

impl X11DragSource {
    pub(super) fn new(drag: std::sync::Arc<X11Drag>, window: WindowRef) -> Self {
        Self { drag, window }
    }
}

impl PlatformDragSource for X11DragSource {
    fn start_timing(&self) -> OsDragStart {
        crate::input::availability::DESKTOP_DRAG_START
    }

    fn start(&self, request: OsDragRequest) -> Result<(), DragError> {
        self.drag.start_session(self.window.xid, request)
    }
}

impl Atoms {
    pub(super) fn replies(&self) -> model::ReplyAtoms {
        model::ReplyAtoms {
            status: self.status,
            finished: self.finished,
        }
    }
}

/// The server time of a pointer or key event.
fn time_of(event: &Event) -> Option<xproto::Timestamp> {
    match event {
        Event::XinputMotion(pointer)
        | Event::XinputButtonPress(pointer)
        | Event::XinputButtonRelease(pointer) => Some(pointer.time),
        Event::XinputKeyPress(key) | Event::XinputKeyRelease(key) => Some(key.time),
        _ => None,
    }
}
