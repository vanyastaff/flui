//! Touch and pen contacts as the frames `InjectSyntheticPointerInput` takes.
//!
//! A synthetic pointer device injects frames: each frame lists every contact the device has in
//! contact (or, for a pen, in range), the one that changed with its phase and the others as
//! updates where they are. A touch contact exists from its press to its release; its id is the
//! lowest one free, as a digitizer numbers fingers. A pen is one contact that hovers in range
//! before and after it touches, and leaves range when the lane releases everything. A move of a
//! touch contact that is not down only places it: touch has no hover.

use crate::coordinates::DevicePoint;
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
    down: bool,
}

/// The touch and pen contacts the lane holds: see the [module docs](self).
#[derive(Debug, Clone, Default)]
pub struct Contacts {
    touch: Vec<Contact>,
    /// Touch sources placed by a move and not yet pressed.
    placed: Vec<(String, DevicePoint)>,
    pen: Option<Contact>,
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

    /// Move `pointer` of `kind` to `at`: the frame that injects it, or `None` for a touch contact
    /// that is not down, which only places it.
    pub fn move_to(&mut self, pointer: &str, kind: PointerKind, at: DevicePoint) -> Option<Frame> {
        match kind {
            PointerKind::Touch => {
                let Some(contact) = self.touch.iter_mut().find(|c| c.pointer == pointer) else {
                    self.placed.retain(|(placed, _)| placed != pointer);
                    self.placed.push((pointer.to_owned(), at));
                    return None;
                };
                contact.at = at;
                Some(self.touch_frame(pointer, Phase::Update))
            }
            PointerKind::Pen => {
                let pen = self.pen.get_or_insert_with(|| Contact {
                    pointer: pointer.to_owned(),
                    id: 0,
                    at,
                    down: false,
                });
                pen.at = at;
                let phase = if pen.down {
                    Phase::Update
                } else {
                    Phase::Hover
                };
                Some(Frame {
                    kind: PointerKind::Pen,
                    contacts: vec![ContactFrame {
                        id: pen.id,
                        at,
                        phase,
                    }],
                })
            }
        }
    }

    /// Put `pointer` of `kind` down where it was moved to.
    ///
    /// # Errors
    ///
    /// [`Refusal::UnsentInput`] for a contact never moved anywhere, one already down, or an
    /// eleventh touch contact.
    pub fn down(&mut self, pointer: &str, kind: PointerKind) -> Result<Frame, Refusal> {
        let unsent = |what: &str| Refusal::UnsentInput {
            what: format!("{what} ({pointer})"),
        };
        match kind {
            PointerKind::Touch => {
                if self.touch.iter().any(|c| c.pointer == pointer) {
                    return Err(unsent("a touch press of a contact already down"));
                }
                let at = self
                    .placed
                    .iter()
                    .position(|(placed, _)| placed == pointer)
                    .map(|index| self.placed.remove(index).1)
                    .ok_or_else(|| unsent("a touch press with no position"))?;
                let id = (0..MAX_TOUCH_CONTACTS)
                    .find(|id| self.touch.iter().all(|c| c.id != *id))
                    .ok_or_else(|| unsent("more than ten touch contacts at once"))?;
                self.touch.push(Contact {
                    pointer: pointer.to_owned(),
                    id,
                    at,
                    down: true,
                });
                Ok(self.touch_frame(pointer, Phase::Down))
            }
            PointerKind::Pen => {
                let pen = self
                    .pen
                    .as_mut()
                    .filter(|pen| pen.pointer == pointer)
                    .ok_or_else(|| unsent("a pen press with no position"))?;
                if pen.down {
                    return Err(unsent("a pen press while the pen is down"));
                }
                pen.down = true;
                Ok(Frame {
                    kind: PointerKind::Pen,
                    contacts: vec![ContactFrame {
                        id: pen.id,
                        at: pen.at,
                        phase: Phase::Down,
                    }],
                })
            }
        }
    }

    /// Lift `pointer` of `kind`.
    ///
    /// # Errors
    ///
    /// [`Refusal::UnsentInput`] for a contact that is not down.
    pub fn up(&mut self, pointer: &str, kind: PointerKind) -> Result<Frame, Refusal> {
        let unsent = || Refusal::UnsentInput {
            what: format!("a release of a contact that is not down ({pointer})"),
        };
        match kind {
            PointerKind::Touch => {
                if !self.touch.iter().any(|c| c.pointer == pointer) {
                    return Err(unsent());
                }
                let frame = self.touch_frame(pointer, Phase::Up);
                self.touch.retain(|c| c.pointer != pointer);
                Ok(frame)
            }
            PointerKind::Pen => {
                let pen = self
                    .pen
                    .as_mut()
                    .filter(|pen| pen.pointer == pointer && pen.down)
                    .ok_or_else(unsent)?;
                pen.down = false;
                Ok(Frame {
                    kind: PointerKind::Pen,
                    contacts: vec![ContactFrame {
                        id: pen.id,
                        at: pen.at,
                        phase: Phase::Up,
                    }],
                })
            }
        }
    }

    /// The frames that lift every touch contact and take the pen out of range, and forget them.
    pub fn release_all(&mut self) -> Vec<Frame> {
        let mut frames = Vec::new();
        if !self.touch.is_empty() {
            frames.push(Frame {
                kind: PointerKind::Touch,
                contacts: self
                    .touch
                    .iter()
                    .map(|c| ContactFrame {
                        id: c.id,
                        at: c.at,
                        phase: Phase::Up,
                    })
                    .collect(),
            });
        }
        if let Some(pen) = self.pen.take() {
            if pen.down {
                frames.push(Frame {
                    kind: PointerKind::Pen,
                    contacts: vec![ContactFrame {
                        id: pen.id,
                        at: pen.at,
                        phase: Phase::Up,
                    }],
                });
            }
            frames.push(Frame {
                kind: PointerKind::Pen,
                contacts: vec![ContactFrame {
                    id: pen.id,
                    at: pen.at,
                    phase: Phase::Leave,
                }],
            });
        }
        self.touch.clear();
        self.placed.clear();
        frames
    }

    /// A touch frame: `pointer` with `phase`, every other contact down as an update.
    fn touch_frame(&self, pointer: &str, phase: Phase) -> Frame {
        Frame {
            kind: PointerKind::Touch,
            contacts: self
                .touch
                .iter()
                .filter(|c| c.down)
                .map(|c| ContactFrame {
                    id: c.id,
                    at: c.at,
                    phase: if c.pointer == pointer {
                        phase
                    } else {
                        Phase::Update
                    },
                })
                .collect(),
        }
    }
}
