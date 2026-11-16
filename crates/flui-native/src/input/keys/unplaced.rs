//! The key a code-less key was pressed as, kept until its release.

use flui_platform::keyboard::{Code, Key, KeyEvent, KeyPress, KeyState};

/// The keys a host reported without a code while they are held, each with the
/// key its key-down named, by the host's own name for the key (`Id`: a key
/// code, a virtual key). A code-less key is known by its key alone, so its
/// repeats and release carry the key its key-down did, whatever a modifier
/// made of it since (`KeyEvent`'s contract); a key with a code passes through.
#[derive(Debug)]
pub(crate) struct UnplacedKeys<Id> {
    held: Vec<(Id, Key)>,
}

impl<Id> Default for UnplacedKeys<Id> {
    fn default() -> Self {
        Self { held: Vec::new() }
    }
}

impl<Id: PartialEq> UnplacedKeys<Id> {
    /// The code-less keys held at once: past it, the oldest is let go, so keys
    /// whose releases went to another window cannot grow the record.
    const MAX_HELD: usize = 16;

    /// `event` of the host's key `id`, its key the key-down's when it has no
    /// code. `None` is a key the host has no name for either (a synthetic
    /// event's zero key code): it passes through, since keys sharing no name
    /// cannot be told apart.
    pub(crate) fn settle(&mut self, id: Option<Id>, mut event: KeyEvent) -> KeyEvent {
        let Some(id) = id else {
            return event;
        };
        if event.code != Code::Unidentified || event.state == KeyState::Down(KeyPress::Unpaired) {
            return event;
        }
        let held = self.held.iter().position(|(held, _)| *held == id);
        match (event.state, held) {
            (KeyState::Down(KeyPress::First), Some(at)) => self.held[at].1 = event.key.clone(),
            (KeyState::Down(KeyPress::First), None) => {
                if self.held.len() == Self::MAX_HELD {
                    self.held.remove(0);
                }
                self.held.push((id, event.key.clone()));
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
    use flui_platform::keyboard::{Code, Key, KeyEvent, KeyPress, KeyState};

    use super::UnplacedKeys;

    fn event(state: KeyState, key: &str, code: Code) -> KeyEvent {
        KeyEvent::new(state, Key::character(key), code, EventTime::from_nanos(0))
    }

    #[test]
    fn a_code_less_key_is_released_as_it_was_pressed() {
        // (case, events in as (id, state, key, code), keys out)
        type Step = (Option<u32>, KeyState, &'static str, Code);
        type Row = (&'static str, &'static [Step], &'static [&'static str]);
        use Code::{KeyA, Unidentified};
        use KeyPress::{AutoRepeat, First};
        use KeyState::{Down, Up};
        let rows: &[Row] = &[
            (
                "shift pressed while held",
                &[
                    (Some(7), Down(First), "x", Unidentified),
                    (Some(7), Down(AutoRepeat), "X", Unidentified),
                    (Some(7), Up, "X", Unidentified),
                ],
                &["x", "x", "x"],
            ),
            (
                "a key with a code passes through",
                &[(Some(7), Down(First), "a", KeyA), (Some(7), Up, "A", KeyA)],
                &["a", "A"],
            ),
            (
                "two keys apart",
                &[
                    (Some(7), Down(First), "x", Unidentified),
                    (Some(8), Down(First), "y", Unidentified),
                    (Some(7), Up, "X", Unidentified),
                    (Some(8), Up, "Y", Unidentified),
                ],
                &["x", "y", "x", "y"],
            ),
            (
                "a release with no press passes through",
                &[(Some(7), Up, "X", Unidentified)],
                &["X"],
            ),
            (
                "keys the host has no name for pass through",
                &[
                    (None, Down(First), "a", Unidentified),
                    (None, Down(First), "b", Unidentified),
                    (None, Up, "a", Unidentified),
                    (None, Up, "b", Unidentified),
                ],
                &["a", "b", "a", "b"],
            ),
            (
                "a press again takes its new key",
                &[
                    (Some(7), Down(First), "x", Unidentified),
                    (Some(7), Down(First), "X", Unidentified),
                    (Some(7), Up, "x", Unidentified),
                ],
                &["x", "X", "X"],
            ),
        ];
        for &(case, steps, keys) in rows {
            let mut unplaced = UnplacedKeys::default();
            let out: Vec<Key> = steps
                .iter()
                .map(|&(id, state, key, code)| unplaced.settle(id, event(state, key, code)).key)
                .collect();
            let want: Vec<Key> = keys.iter().map(|key| Key::character(*key)).collect();
            assert_eq!(out, want, "{case}");
        }
    }

    #[test]
    fn a_key_no_release_follows_is_not_held() {
        let mut unplaced = UnplacedKeys::default();
        let stray = event(KeyState::Down(KeyPress::Unpaired), "é", Code::Unidentified);
        let _ = unplaced.settle(Some(1_u32), stray);
        let up = unplaced.settle(Some(1), event(KeyState::Up, "E", Code::Unidentified));
        assert_eq!(up.key, Key::character("E"), "nothing was held for it");
    }

    #[test]
    fn the_oldest_held_key_is_let_go_past_the_bound() {
        let mut unplaced = UnplacedKeys::default();
        for id in 0..=16_u32 {
            let _ = unplaced.settle(
                Some(id),
                event(KeyState::Down(KeyPress::First), "x", Code::Unidentified),
            );
        }
        let first = unplaced.settle(Some(0), event(KeyState::Up, "X", Code::Unidentified));
        assert_eq!(first.key, Key::character("X"), "the oldest was let go");
        let last = unplaced.settle(Some(16), event(KeyState::Up, "X", Code::Unidentified));
        assert_eq!(last.key, Key::character("x"));
    }
}
