//! [`Gesture`]: the builder for multi-pointer, keyed and timed input, which
//! [`Tester::perform`](super::Tester::perform) sends (ADR-0400 §2).
//!
//! A gesture is data: an [`ActionSequence`] of the protocol, one source a device, aligned by
//! tick, which [`flui_protocol::input::plan`] lays on one timeline for every path that plays it.
//! It names points, not elements: a verb aimed at an element (`tap`, `long_press`, `drag`) is
//! the agent's to aim, and a gesture is what those verbs cannot say, such as two contacts at
//! once, a press held for a stated time, or a stroke of a stated length. The counterpart in
//! Flutter 3.50.1's `flutter_test` is `TestGesture` (`startGesture`, `moveBy`, `up`, `cancel`),
//! with the test's `pump(duration)` between its steps where a gesture names a
//! [`hold`](Pointer::hold).
//!
//! [`Gesture::try_build`] refuses a pointer press the gesture does not release, and a release
//! of a press it did not make: a gesture is one whole stroke, and a contact it left down would
//! stay with the window's input lease into the next step. Keys may stay down, as a modifier held
//! across steps does; the lease keeps them, and a chord that names a key it holds sends no second
//! press. A sequence built by hand and sent with `perform` may hold a contact across steps, as
//! the protocol's `input.inject` does: the lease keeps it until a later sequence lifts it, and its
//! release at the end of the test cancels it.
//!
//! ```
//! use std::time::Duration;
//! use flui_protocol::geometry::LogicalPoint;
//! use flui_testing::scenario::gesture::{Gesture, GestureError, Pointer};
//!
//! let press = Gesture::new()
//!     .pointer(
//!         Pointer::touch(LogicalPoint::new(20.0, 15.0))
//!             .down()
//!             .hold(Duration::from_millis(300))
//!             .up(),
//!     )
//!     .try_build()?;
//! assert_eq!(press.sources.len(), 1);
//!
//! let left_down = Gesture::new()
//!     .pointer(Pointer::touch(LogicalPoint::new(20.0, 15.0)).down())
//!     .try_build();
//! assert!(matches!(left_down, Err(GestureError::LeftDown { .. })));
//! # Ok::<(), GestureError>(())
//! ```

use std::fmt;
use std::time::Duration;

use flui_protocol::geometry::LogicalPoint;
use flui_protocol::input::{
    ActionSequence, InputSource, KeyAction, PlanError, PointerAction, PointerButton, PointerDevice,
    plan,
};

/// Input from several devices, aligned by tick: tick `n` of every source plays together, and
/// starts when the longest action of tick `n - 1` has ended.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Gesture {
    pointers: Vec<Pointer>,
    keys: Option<Keys>,
    text: Option<String>,
}

impl Gesture {
    /// A gesture with no device yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// With `pointer`, a contact or cursor of its own beside the pointers added before.
    #[must_use]
    pub fn pointer(mut self, pointer: Pointer) -> Self {
        self.pointers.push(pointer);
        self
    }

    /// With `keys`, the keyboard's actions, in place of keys added before.
    #[must_use]
    pub fn keys(mut self, keys: Keys) -> Self {
        self.keys = Some(keys);
        self
    }

    /// With `text` committed at the first tick, as an input method commits it.
    #[must_use]
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// The sequence the gesture is.
    ///
    /// # Panics
    ///
    /// As [`Self::try_build`] fails.
    #[track_caller]
    #[must_use]
    pub fn build(self) -> ActionSequence {
        match self.try_build() {
            Ok(sequence) => sequence,
            Err(error) => panic!("the gesture cannot be built (see `Gesture::try_build`): {error}"),
        }
    }

    /// The sequence the gesture is, checked as every path plays it.
    ///
    /// # Errors
    ///
    /// [`GestureError::LeftDown`] for a press the gesture does not release,
    /// [`GestureError::NotPressed`] for a release of a press it did not make, and
    /// [`GestureError::Plan`] for a sequence the protocol's plan refuses (too long, too many
    /// steps, a point or a pressure out of range).
    pub fn try_build(self) -> Result<ActionSequence, GestureError> {
        for (index, pointer) in self.pointers.iter().enumerate() {
            pointer.check(index)?;
        }
        let mut sources: Vec<InputSource> = self
            .pointers
            .into_iter()
            .enumerate()
            .map(|(index, pointer)| InputSource::Pointer {
                id: format!("gesture-pointer-{index}"),
                device: pointer.device,
                actions: pointer.actions,
            })
            .collect();
        if let Some(keys) = self.keys {
            sources.push(InputSource::Keys {
                id: "gesture-keys".to_owned(),
                actions: keys.actions,
            });
        }
        if let Some(text) = self.text {
            sources.push(InputSource::Text { text });
        }
        let sequence = ActionSequence::new(sources);
        plan(&sequence).map_err(GestureError::Plan)?;
        Ok(sequence)
    }
}

/// One pointer of a [`Gesture`]: a touch contact, a mouse, a pen or a virtual pointer, which
/// starts where [`new`](Self::new) puts it and takes one action a tick.
#[derive(Debug, Clone, PartialEq)]
pub struct Pointer {
    device: PointerDevice,
    /// Where the last action left it.
    at: LogicalPoint,
    actions: Vec<PointerAction>,
}

impl Pointer {
    /// A `device` pointer that moves to `at` at the gesture's first tick.
    #[must_use]
    pub fn new(device: PointerDevice, at: LogicalPoint) -> Self {
        Self {
            device,
            at,
            actions: vec![PointerAction::Move {
                to: at,
                duration_ms: 0,
                pressure: None,
            }],
        }
    }

    /// A touch contact at `at`, as `flutter_test`'s `startGesture` starts one.
    #[must_use]
    pub fn touch(at: LogicalPoint) -> Self {
        Self::new(PointerDevice::Touch, at)
    }

    /// A mouse at `at`.
    #[must_use]
    pub fn mouse(at: LogicalPoint) -> Self {
        Self::new(PointerDevice::Mouse, at)
    }

    /// Press the primary button, or put the contact down, where the pointer is.
    #[must_use]
    pub fn down(self) -> Self {
        self.press(PointerButton::Primary)
    }

    /// Press `button` where the pointer is.
    #[must_use]
    pub fn press(mut self, button: PointerButton) -> Self {
        self.actions.push(PointerAction::Down {
            button,
            pressure: None,
        });
        self
    }

    /// Release the primary button, or lift the contact.
    #[must_use]
    pub fn up(self) -> Self {
        self.release(PointerButton::Primary)
    }

    /// Release `button`.
    #[must_use]
    pub fn release(mut self, button: PointerButton) -> Self {
        self.actions.push(PointerAction::Up { button });
        self
    }

    /// Move to `to` over `over`, in steps of a frame.
    #[must_use]
    pub fn move_to(mut self, to: LogicalPoint, over: Duration) -> Self {
        self.at = to;
        self.actions.push(PointerAction::Move {
            to,
            duration_ms: millis(over),
            pressure: None,
        });
        self
    }

    /// Move by `dx`, `dy` logical pixels from where the pointer is, over `over`.
    #[must_use]
    pub fn move_by(self, dx: f64, dy: f64, over: Duration) -> Self {
        let to = LogicalPoint::new(self.at.x + dx, self.at.y + dy);
        self.move_to(to, over)
    }

    /// Hold still for `duration`: a press held this long, as a long press is.
    #[must_use]
    pub fn hold(mut self, duration: Duration) -> Self {
        self.actions.push(PointerAction::Pause {
            duration_ms: millis(duration),
        });
        self
    }

    /// The platform takes the contact away: every button the pointer holds is let go without a
    /// release.
    #[must_use]
    pub fn cancel(mut self) -> Self {
        self.actions.push(PointerAction::Cancel);
        self
    }

    /// Every press released and no release unpressed, `index` naming the pointer.
    fn check(&self, index: usize) -> Result<(), GestureError> {
        let mut pressed: Vec<PointerButton> = Vec::new();
        for action in &self.actions {
            match action {
                PointerAction::Down { button, .. } => pressed.push(*button),
                PointerAction::Up { button } => {
                    let Some(at) = pressed.iter().position(|held| held == button) else {
                        return Err(GestureError::NotPressed {
                            pointer: index,
                            button: *button,
                        });
                    };
                    pressed.swap_remove(at);
                }
                PointerAction::Cancel => pressed.clear(),
                _ => {}
            }
        }
        match pressed.first() {
            Some(button) => Err(GestureError::LeftDown {
                pointer: index,
                button: *button,
            }),
            None => Ok(()),
        }
    }
}

/// The keyboard of a [`Gesture`], one action a tick, each key by its W3C name (`"Shift"`,
/// `"Enter"`) or its text (`"a"`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keys {
    actions: Vec<KeyAction>,
}

impl Keys {
    /// A keyboard with no action yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Press `key`; it stays down past the gesture until a later step releases it.
    #[must_use]
    pub fn down(mut self, key: impl Into<String>) -> Self {
        self.actions.push(KeyAction::Down { key: key.into() });
        self
    }

    /// Release `key`.
    #[must_use]
    pub fn up(mut self, key: impl Into<String>) -> Self {
        self.actions.push(KeyAction::Up { key: key.into() });
        self
    }

    /// Hold still for `duration`.
    #[must_use]
    pub fn hold(mut self, duration: Duration) -> Self {
        self.actions.push(KeyAction::Pause {
            duration_ms: millis(duration),
        });
        self
    }
}

/// Why a [`Gesture`] cannot be built.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum GestureError {
    /// The pointer pressed `button` and the gesture ends with it down.
    #[non_exhaustive]
    LeftDown {
        /// The pointer's place among the gesture's pointers, from 0.
        pointer: usize,
        /// The button.
        button: PointerButton,
    },
    /// The pointer releases `button`, which it did not press.
    #[non_exhaustive]
    NotPressed {
        /// The pointer's place among the gesture's pointers, from 0.
        pointer: usize,
        /// The button.
        button: PointerButton,
    },
    /// The protocol's plan refuses the sequence.
    Plan(PlanError),
}

impl fmt::Display for GestureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LeftDown { pointer, button } => write!(
                f,
                "pointer {pointer} presses {button:?} and the gesture ends with it down"
            ),
            Self::NotPressed { pointer, button } => {
                write!(
                    f,
                    "pointer {pointer} releases {button:?}, which it did not press"
                )
            }
            Self::Plan(error) => write!(f, "the protocol's plan refuses it: {error}"),
        }
    }
}

impl std::error::Error for GestureError {}

/// `duration` in whole milliseconds, the protocol's unit, saturating.
fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
