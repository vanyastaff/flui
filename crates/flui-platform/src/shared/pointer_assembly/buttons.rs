//! Held buttons and click counting for one contact.

use std::num::NonZeroU8;
use std::time::Duration;

use flui_platform_api::EventTime;
use flui_platform_api::pointer::{PointerButton, PointerButtons, PointerId, PointerKind};

use super::RawPoint;

/// What a button reading does to a contact's sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonTransition {
    /// The sequence starts.
    Down,
    /// Another button changed while the sequence continues, or a pen's barrel
    /// button changed while it hovers.
    Change,
    /// The sequence ends.
    Up,
    /// Nothing changed: a repeated press, a release of a button not held, or
    /// the release of a suppressed press.
    Ignored,
}

/// The buttons one contact holds, and the presses suppressed because they
/// arrived before the contact had a position.
///
/// A touch or pen sequence is its contact with the surface, the primary
/// button: barrel buttons change within it, or while the pen hovers, without
/// starting or ending it (the W3C `pointerdown` rule for pens). A mouse
/// sequence lasts while any button is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeldButtons {
    held: PointerButtons,
    suppressed: PointerButtons,
    contact_driven: bool,
}

impl HeldButtons {
    /// No button held, by the sequence rule of `kind`.
    #[must_use]
    pub const fn for_kind(kind: PointerKind) -> Self {
        Self {
            held: PointerButtons::NONE,
            suppressed: PointerButtons::NONE,
            contact_driven: matches!(kind, PointerKind::Touch | PointerKind::Pen { .. }),
        }
    }

    /// The buttons held now.
    #[must_use]
    pub const fn held(self) -> PointerButtons {
        self.held
    }

    /// Whether a sequence is in progress.
    #[must_use]
    pub fn in_sequence(self) -> bool {
        if self.contact_driven {
            self.held.contains(PointerButton::PRIMARY)
        } else {
            !self.held.is_empty()
        }
    }

    /// The transition `button` going down makes, without changing the set.
    #[must_use]
    pub fn press_transition(self, button: PointerButton) -> ButtonTransition {
        if self.held.contains(button) {
            ButtonTransition::Ignored
        } else if (self.contact_driven && button != PointerButton::PRIMARY) || self.in_sequence() {
            ButtonTransition::Change
        } else {
            ButtonTransition::Down
        }
    }

    /// The transition `button` coming up makes, given the platform's held set
    /// after it (`reported`), without changing the set.
    #[must_use]
    pub fn release_transition(
        self,
        button: PointerButton,
        reported: Option<PointerButtons>,
    ) -> ButtonTransition {
        if self.suppressed.contains(button) || !self.held.contains(button) {
            ButtonTransition::Ignored
        } else if self.contact_driven {
            if button == PointerButton::PRIMARY {
                ButtonTransition::Up
            } else {
                ButtonTransition::Change
            }
        } else if reported.unwrap_or(self.held).without(button).is_empty() {
            ButtonTransition::Up
        } else {
            ButtonTransition::Change
        }
    }

    /// Record `button` going down, with the platform's held set when it
    /// reports one.
    pub fn press(&mut self, button: PointerButton, reported: Option<PointerButtons>) {
        self.held = reported.unwrap_or(self.held).with(button);
    }

    /// Record `button` coming up. A suppressed press is forgotten instead.
    pub fn release(&mut self, button: PointerButton, reported: Option<PointerButtons>) {
        if self.suppressed.contains(button) {
            self.suppressed = self.suppressed.without(button);
            return;
        }
        self.held = reported.unwrap_or(self.held).without(button);
    }

    /// Record a press that could not be delivered: its release is dropped too.
    pub fn suppress(&mut self, button: PointerButton) {
        self.suppressed = self.suppressed.with(button);
    }

    /// Forget every held and suppressed button.
    pub fn clear(&mut self) {
        self.held = PointerButtons::NONE;
        self.suppressed = PointerButtons::NONE;
    }
}

/// The OS double-click thresholds: the interval and the full rectangle, in
/// device pixels, a following press must land within to count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClickSettings {
    /// The longest time between presses that still counts.
    pub interval: Duration,
    /// The full width and height of the rectangle centred on the previous press.
    pub area: (f64, f64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LastPress {
    pointer: PointerId,
    button: PointerButton,
    time: EventTime,
    position: RawPoint,
    count: NonZeroU8,
}

/// Counts consecutive presses of one button at one place, by the OS's
/// double-click settings.
///
/// Without settings it counts nothing, and the press carries no count, so the
/// framework's recognizers decide.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ClickCounter {
    settings: Option<ClickSettings>,
    last: Option<LastPress>,
}

impl ClickCounter {
    /// A counter using `settings`.
    #[must_use]
    pub const fn new(settings: Option<ClickSettings>) -> Self {
        Self {
            settings,
            last: None,
        }
    }

    /// Replace the thresholds, as the OS settings change. The run so far ends.
    pub fn set_settings(&mut self, settings: Option<ClickSettings>) {
        self.settings = settings;
        self.last = None;
    }

    /// The count of a press of `button` from `pointer` at `time` and
    /// `position`, and record it.
    pub fn press(
        &mut self,
        pointer: PointerId,
        button: PointerButton,
        time: EventTime,
        position: RawPoint,
    ) -> Option<NonZeroU8> {
        let settings = self.settings?;
        let continues = self.last.filter(|last| {
            last.pointer == pointer
                && last.button == button
                && time >= last.time
                && time.saturating_duration_since(last.time) <= settings.interval
                && (position.x - last.position.x).abs() <= settings.area.0 / 2.0
                && (position.y - last.position.y).abs() <= settings.area.1 / 2.0
        });
        let count = continues.map_or(NonZeroU8::MIN, |last| last.count.saturating_add(1));
        self.last = Some(LastPress {
            pointer,
            button,
            time,
            position,
            count,
        });
        Some(count)
    }
}
