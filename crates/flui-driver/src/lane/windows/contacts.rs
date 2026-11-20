//! Touch and pen contacts as the frames `InjectSyntheticPointerInput` takes.
//!
//! A synthetic pointer device injects frames: each frame lists every contact the device has in
//! contact (or, for a pen, in range), the one that changed with its phase and the others as
//! updates where they are. A touch contact exists from its press to its release; its id is the
//! lowest one free, as a digitizer numbers fingers. A pen is one contact that hovers in range
//! before and after it touches, and leaves range when the lane releases everything. A move of a
//! touch contact that is not down only places it: touch has no hover.
//!
//! The contacts are what the OS holds: each change is handed to an injection and recorded only
//! once it succeeds, so a press Windows refused is not lifted later and a release it refused is
//! lifted again by [`Contacts::release_all`].
//!
//! The primary contact follows Windows' rule (Microsoft Learn, "Pointer Flags",
//! `POINTER_FLAG_PRIMARY`): the first contact to come down while no other is down is primary for
//! its whole life; while it is down no other contact is, and once it lifts no remaining contact
//! becomes primary: the next primary is the first contact down after every contact has lifted. A
//! pen is its device's only contact, so it is always primary.

use crate::coordinates::DevicePoint;
use crate::error::Result;
use crate::lane::windows::safety::Refusal;

/// The most touch contacts the lane's synthetic touch device carries at once.
pub const MAX_TOUCH_CONTACTS: u32 = 10;

/// Which synthetic device a frame goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PointerKind {
    /// A touch digitizer.
    Touch,
    /// A pen.
    Pen,
}

/// What a contact does in a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Phase {
    /// It comes into contact.
    Down,
    /// It stays in contact, here.
    Update,
    /// It leaves contact.
    Up,
    /// A pen in range above the surface, not touching it.
    Hover,
    /// A pen leaves range.
    Leave,
}

/// One contact of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ContactFrame {
    /// The pointer id the device reports.
    pub id: u32,
    /// Where, in screen device pixels.
    pub at: DevicePoint,
    /// What it does.
    pub phase: Phase,
    /// Whether it is the primary contact (`POINTER_FLAG_PRIMARY`).
    pub primary: bool,
}

/// One injection: every contact of one device.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Frame {
    /// The device.
    pub kind: PointerKind,
    /// Its contacts, the one that changed among them.
    pub contacts: Vec<ContactFrame>,
}

#[derive(Debug, Clone)]
struct Contact {
    pointer: String,
    id: u32,
    at: DevicePoint,
    primary: bool,
}

#[derive(Debug, Clone)]
struct Pen {
    pointer: String,
    at: DevicePoint,
    down: bool,
}

/// The touch and pen contacts the OS holds for the lane: see the [module docs](self).
#[derive(Debug, Clone, Default)]
pub struct Contacts {
    touch: Vec<Contact>,
    /// Touch sources placed by a move and not yet pressed.
    placed: Vec<(String, DevicePoint)>,
    pen: Option<Pen>,
}

fn unsent(what: &str, pointer: &str) -> crate::error::Error {
    Refusal::UnsentInput {
        what: format!("{what} ({pointer})"),
    }
    .into_error()
}

impl Contacts {
    /// No contact at all.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Where `pointer` of `kind` is: down, placed by a move, or in range.
    #[must_use]
    pub fn position(&self, pointer: &str, kind: PointerKind) -> Option<DevicePoint> {
        match kind {
            PointerKind::Touch => self
                .touch
                .iter()
                .find(|c| c.pointer == pointer)
                .map(|c| c.at)
                .or_else(|| {
                    self.placed
                        .iter()
                        .find(|(placed, _)| placed == pointer)
                        .map(|(_, at)| *at)
                }),
            PointerKind::Pen => self
                .pen
                .as_ref()
                .filter(|pen| pen.pointer == pointer)
                .map(|pen| pen.at),
        }
    }

    /// Move `pointer` of `kind` to `at`, injecting the frame through `inject` when the contact is
    /// down or a pen; a touch contact that is not down is only placed.
    ///
    /// # Errors
    ///
    /// The injection's failure; the contact keeps its place then.
    pub fn move_to(
        &mut self,
        pointer: &str,
        kind: PointerKind,
        at: DevicePoint,
        inject: impl FnOnce(&Frame) -> Result<()>,
    ) -> Result<()> {
        match kind {
            PointerKind::Touch => {
                let Some(index) = self.touch.iter().position(|c| c.pointer == pointer) else {
                    self.placed.retain(|(placed, _)| placed != pointer);
                    self.placed.push((pointer.to_owned(), at));
                    return Ok(());
                };
                let mut next = self.touch.clone();
                next[index].at = at;
                inject(&touch_frame(&next, pointer, Phase::Update))?;
                self.touch = next;
                Ok(())
            }
            PointerKind::Pen => {
                let down = self
                    .pen
                    .as_ref()
                    .is_some_and(|pen| pen.pointer == pointer && pen.down);
                let phase = if down { Phase::Update } else { Phase::Hover };
                inject(&pen_frame(at, phase))?;
                match &mut self.pen {
                    Some(pen) if pen.pointer == pointer => pen.at = at,
                    _ => {
                        self.pen = Some(Pen {
                            pointer: pointer.to_owned(),
                            at,
                            down: false,
                        });
                    }
                }
                Ok(())
            }
        }
    }

    /// Put `pointer` of `kind` down where it was moved to, through `inject`.
    ///
    /// # Errors
    ///
    /// `not_supported` for a contact never moved anywhere, one already down, or an eleventh touch
    /// contact; the injection's failure, after which the contact is not down.
    pub fn down(
        &mut self,
        pointer: &str,
        kind: PointerKind,
        inject: impl FnOnce(&Frame) -> Result<()>,
    ) -> Result<()> {
        match kind {
            PointerKind::Touch => {
                if self.touch.iter().any(|c| c.pointer == pointer) {
                    return Err(unsent("a touch press of a contact already down", pointer));
                }
                let at = self
                    .placed
                    .iter()
                    .find(|(placed, _)| placed == pointer)
                    .map(|(_, at)| *at)
                    .ok_or_else(|| unsent("a touch press with no position", pointer))?;
                let id = (0..MAX_TOUCH_CONTACTS)
                    .find(|id| self.touch.iter().all(|c| c.id != *id))
                    .ok_or_else(|| unsent("more than ten touch contacts at once", pointer))?;
                let mut next = self.touch.clone();
                next.push(Contact {
                    pointer: pointer.to_owned(),
                    id,
                    at,
                    primary: self.touch.is_empty(),
                });
                inject(&touch_frame(&next, pointer, Phase::Down))?;
                self.touch = next;
                self.placed.retain(|(placed, _)| placed != pointer);
                Ok(())
            }
            PointerKind::Pen => {
                let pen = self
                    .pen
                    .as_ref()
                    .filter(|pen| pen.pointer == pointer)
                    .ok_or_else(|| unsent("a pen press with no position", pointer))?;
                if pen.down {
                    return Err(unsent("a pen press while the pen is down", pointer));
                }
                inject(&pen_frame(pen.at, Phase::Down))?;
                if let Some(pen) = &mut self.pen {
                    pen.down = true;
                }
                Ok(())
            }
        }
    }

    /// Lift `pointer` of `kind` through `inject`.
    ///
    /// # Errors
    ///
    /// `not_supported` for a contact that is not down; the injection's failure, after which the
    /// contact is still down.
    pub fn up(
        &mut self,
        pointer: &str,
        kind: PointerKind,
        inject: impl FnOnce(&Frame) -> Result<()>,
    ) -> Result<()> {
        let not_down = || unsent("a release of a contact that is not down", pointer);
        match kind {
            PointerKind::Touch => {
                if !self.touch.iter().any(|c| c.pointer == pointer) {
                    return Err(not_down());
                }
                inject(&touch_frame(&self.touch, pointer, Phase::Up))?;
                self.touch.retain(|c| c.pointer != pointer);
                Ok(())
            }
            PointerKind::Pen => {
                let pen = self
                    .pen
                    .as_ref()
                    .filter(|pen| pen.pointer == pointer && pen.down)
                    .ok_or_else(not_down)?;
                inject(&pen_frame(pen.at, Phase::Up))?;
                if let Some(pen) = &mut self.pen {
                    pen.down = false;
                }
                Ok(())
            }
        }
    }

    /// Lift every touch contact the OS holds and take the pen out of range, through `inject`;
    /// what an injection failed to lift stays held, and placements are forgotten.
    ///
    /// # Errors
    ///
    /// The first injection that failed, after every frame was tried.
    pub fn release_all(&mut self, mut inject: impl FnMut(&Frame) -> Result<()>) -> Result<()> {
        let mut first = None;
        self.placed.clear();
        if !self.touch.is_empty() {
            let frame = Frame {
                kind: PointerKind::Touch,
                contacts: self
                    .touch
                    .iter()
                    .map(|c| ContactFrame {
                        id: c.id,
                        at: c.at,
                        phase: Phase::Up,
                        primary: c.primary,
                    })
                    .collect(),
            };
            match inject(&frame) {
                Ok(()) => self.touch.clear(),
                Err(error) => first = Some(error),
            }
        }
        if let Some(pen) = self.pen.clone() {
            let mut lifted = !pen.down;
            if pen.down {
                match inject(&pen_frame(pen.at, Phase::Up)) {
                    Ok(()) => {
                        lifted = true;
                        if let Some(pen) = &mut self.pen {
                            pen.down = false;
                        }
                    }
                    Err(error) => {
                        first.get_or_insert(error);
                    }
                }
            }
            if lifted {
                match inject(&pen_frame(pen.at, Phase::Leave)) {
                    Ok(()) => self.pen = None,
                    Err(error) => {
                        first.get_or_insert(error);
                    }
                }
            }
        }
        first.map_or(Ok(()), Err)
    }
}

/// A touch frame of `contacts`: `pointer` with `phase`, every other contact as an update.
fn touch_frame(contacts: &[Contact], pointer: &str, phase: Phase) -> Frame {
    Frame {
        kind: PointerKind::Touch,
        contacts: contacts
            .iter()
            .map(|c| ContactFrame {
                id: c.id,
                at: c.at,
                phase: if c.pointer == pointer {
                    phase
                } else {
                    Phase::Update
                },
                primary: c.primary,
            })
            .collect(),
    }
}

fn pen_frame(at: DevicePoint, phase: Phase) -> Frame {
    Frame {
        kind: PointerKind::Pen,
        contacts: vec![ContactFrame {
            id: 0,
            at,
            phase,
            primary: true,
        }],
    }
}
