//! The key a held key was pressed as, kept until its release.

use flui_platform::input::keyboard::{Code, Key, KeyEvent, KeyPress, KeyState};

/// The keys whose repeats and release must name the key their key-down did,
/// while they are held, by the host's own name for the key (`Id`: a key code,
/// a scan code). Two kinds of key are held here, on every host that reads its
/// keys from a layout:
///
/// - a code-less key, known by its key alone, so a modifier that changed its
///   meaning while it was held must not change the key its release names
///   (`KeyEvent`'s contract);
/// - a dead key, whose release reports the accent its press did
///   ([`Key::Dead`]'s contract), even if Shift, the layout or a compose
///   sequence changed what the key means now.
///
/// A first press of a key already held replaces what it was pressed as (its
/// release went to another window), or forgets it when the new press is of
/// neither kind. Any other key passes through.
#[derive(Debug)]
pub(crate) struct PressedAs<Id> {
    held: Vec<(Id, Key)>,
}

impl<Id> Default for PressedAs<Id> {
    fn default() -> Self {
        Self { held: Vec::new() }
    }
}

impl<Id: PartialEq> PressedAs<Id> {
    /// The keys held at once: past it, the oldest is let go, so keys whose
    /// releases went to another window cannot grow the record.
    const MAX_HELD: usize = 16;

    /// `event` of the host's key `id`, its key the key-down's while the key is
    /// held here. `None` is a key the host has no name for (a synthetic event's
    /// zero key code): it passes through, since keys sharing no name cannot be
    /// told apart.
    pub(crate) fn settle(&mut self, id: Option<Id>, mut event: KeyEvent) -> KeyEvent {
        let Some(id) = id else {
            return event;
        };
        let held = self.held.iter().position(|(held, _)| *held == id);
        match (event.state, held) {
            (KeyState::Down(KeyPress::First), held) => {
                let kept = event.code == Code::Unidentified || matches!(event.key, Key::Dead(_));
                match (kept, held) {
                    (true, Some(at)) => self.held[at].1 = event.key.clone(),
                    (true, None) => {
                        if self.held.len() == Self::MAX_HELD {
                            self.held.remove(0);
                        }
                        self.held.push((id, event.key.clone()));
                    }
                    (false, Some(at)) => {
                        self.held.remove(at);
                    }
                    (false, None) => {}
                }
            }
            (KeyState::Down(KeyPress::AutoRepeat), Some(at)) => {
                event.key = self.held[at].1.clone();
            }
            (KeyState::Up, Some(at)) => event.key = self.held.remove(at).1,
            (KeyState::Down(KeyPress::AutoRepeat) | KeyState::Up, None)
            | (KeyState::Down(KeyPress::Unpaired), _) => {}
        }
        event
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::EventTime;
    use flui_platform::input::keyboard::{Code, Key, KeyEvent, KeyPress, KeyState};

    use super::PressedAs;

    /// What one host's key reads as at each step: its press, a repeat, its
    /// release, each with the key the layout makes of it now.
    type Step = (Option<u32>, KeyState, Key, Code);

    /// The same script, (case, steps, keys out), on any host's ledger.
    fn run(rows: &[(&str, Vec<Step>, Vec<Key>)]) {
        for (case, steps, keys) in rows {
            let mut pressed = PressedAs::default();
            let out: Vec<Key> = steps
                .iter()
                .map(|(id, state, key, code)| {
                    let event = KeyEvent::new(*state, key.clone(), *code, EventTime::from_nanos(0));
                    pressed.settle(*id, event).key
                })
                .collect();
            assert_eq!(&out, keys, "{case}");
        }
    }

    #[test]
    fn a_held_key_is_released_as_it_was_pressed() {
        use Code::{KeyA, Quote, Unidentified};
        use KeyPress::{AutoRepeat, First};
        use KeyState::{Down, Up};
        let c = |text: &str| Key::character(text);
        let acute = Key::Dead(Some('\u{b4}'));
        let grave = Key::Dead(Some('`'));
        run(&[
            (
                "shift pressed while a code-less key is held",
                vec![
                    (Some(7), Down(First), c("x"), Unidentified),
                    (Some(7), Down(AutoRepeat), c("X"), Unidentified),
                    (Some(7), Up, c("X"), Unidentified),
                ],
                vec![c("x"), c("x"), c("x")],
            ),
            (
                "a key with a code passes through",
                vec![
                    (Some(7), Down(First), c("a"), KeyA),
                    (Some(7), Up, c("A"), KeyA),
                ],
                vec![c("a"), c("A")],
            ),
            (
                "two code-less keys apart",
                vec![
                    (Some(7), Down(First), c("x"), Unidentified),
                    (Some(8), Down(First), c("y"), Unidentified),
                    (Some(7), Up, c("X"), Unidentified),
                    (Some(8), Up, c("Y"), Unidentified),
                ],
                vec![c("x"), c("y"), c("x"), c("y")],
            ),
            (
                "a release with no press passes through",
                vec![(Some(7), Up, c("X"), Unidentified)],
                vec![c("X")],
            ),
            (
                "keys the host has no name for pass through",
                vec![
                    (None, Down(First), c("a"), Unidentified),
                    (None, Up, c("b"), Unidentified),
                ],
                vec![c("a"), c("b")],
            ),
            (
                "a dead key released with Shift held reports its press's accent",
                vec![
                    (Some(40), Down(First), acute.clone(), Quote),
                    (Some(40), Up, grave.clone(), Quote),
                ],
                vec![acute.clone(), acute.clone()],
            ),
            (
                "a dead key released after the layout or compose moved on",
                vec![
                    (Some(40), Down(First), Key::Dead(None), Quote),
                    (Some(40), Up, c("\u{e9}"), Quote),
                ],
                vec![Key::Dead(None), Key::Dead(None)],
            ),
            (
                "a dead key pressed again replaces what it was pressed as",
                vec![
                    (Some(40), Down(First), acute.clone(), Quote),
                    (Some(40), Down(First), grave.clone(), Quote),
                    (Some(40), Up, acute.clone(), Quote),
                ],
                vec![acute.clone(), grave.clone(), grave],
            ),
            (
                "the same key pressed as a letter forgets the dead press",
                vec![
                    (Some(40), Down(First), acute.clone(), Quote),
                    (Some(40), Down(First), c("'"), Quote),
                    (Some(40), Up, c("'"), Quote),
                ],
                vec![acute, c("'"), c("'")],
            ),
        ]);
    }

    #[test]
    fn a_key_no_release_follows_is_not_held() {
        let mut pressed = PressedAs::default();
        let stray = KeyEvent::new(
            KeyState::Down(KeyPress::Unpaired),
            Key::character("\u{e9}"),
            Code::Unidentified,
            EventTime::from_nanos(0),
        );
        let _ = pressed.settle(Some(1_u32), stray);
        let up = KeyEvent::new(
            KeyState::Up,
            Key::character("E"),
            Code::Unidentified,
            EventTime::from_nanos(0),
        );
        assert_eq!(
            pressed.settle(Some(1), up).key,
            Key::character("E"),
            "nothing was held for it"
        );
    }

    #[test]
    fn the_oldest_held_key_is_let_go_past_the_bound() {
        let key = |state, text: &str| {
            KeyEvent::new(
                state,
                Key::character(text),
                Code::Unidentified,
                EventTime::from_nanos(0),
            )
        };
        let mut pressed = PressedAs::default();
        for id in 0..=16_u32 {
            let _ = pressed.settle(Some(id), key(KeyState::Down(KeyPress::First), "x"));
        }
        assert_eq!(
            pressed.settle(Some(0), key(KeyState::Up, "X")).key,
            Key::character("X"),
            "the oldest was let go"
        );
        assert_eq!(
            pressed.settle(Some(16), key(KeyState::Up, "X")).key,
            Key::character("x")
        );
    }
}
