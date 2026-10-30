//! The key a code-less key was pressed as, kept until its release.

use flui_platform::keyboard::{Code, Key, KeyEvent, KeyRelease, KeyRepeat, KeyState};

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
    /// code.
    pub(crate) fn settle(&mut self, id: Id, mut event: KeyEvent) -> KeyEvent {
        if event.code != Code::Unidentified || event.release() == KeyRelease::Never {
            return event;
        }
        let held = self.held.iter().position(|(held, _)| *held == id);
        match (event.state(), event.repeat(), held) {
            (KeyState::Down, KeyRepeat::First, Some(at)) => self.held[at].1 = event.key.clone(),
            (KeyState::Down, KeyRepeat::First, None) => {
                if self.held.len() == Self::MAX_HELD {
                    self.held.remove(0);
                }
                self.held.push((id, event.key.clone()));
            }
            (KeyState::Down, KeyRepeat::AutoRepeat, Some(at)) => {
                event.key = self.held[at].1.clone();
            }
            (KeyState::Up, _, Some(at)) => event.key = self.held.remove(at).1,
            (KeyState::Down, KeyRepeat::AutoRepeat, None) | (KeyState::Up, _, None) => {}
        }
        event
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::EventTime;
    use flui_platform::keyboard::{Code, Key, KeyEvent, KeyRelease, KeyRepeat, KeyState};

    use super::UnplacedKeys;

    fn event(state: KeyState, key: &str, code: Code) -> KeyEvent {
        KeyEvent::new(state, Key::character(key), code, EventTime::from_nanos(0))
    }

    #[test]
    fn a_code_less_key_is_released_as_it_was_pressed() {
        // (case, events in as (id, state, repeat, key, code), keys out)
        type Step = (u32, KeyState, KeyRepeat, &'static str, Code);
        type Row = (&'static str, &'static [Step], &'static [&'static str]);
        use Code::{KeyA, Unidentified};
        use KeyRepeat::{AutoRepeat, First};
        use KeyState::{Down, Up};
        let rows: &[Row] = &[
            (
                "shift pressed while held",
                &[
                    (7, Down, First, "x", Unidentified),
                    (7, Down, AutoRepeat, "X", Unidentified),
                    (7, Up, First, "X", Unidentified),
                ],
                &["x", "x", "x"],
            ),
            (
                "a key with a code passes through",
                &[(7, Down, First, "a", KeyA), (7, Up, First, "A", KeyA)],
                &["a", "A"],
            ),
            (
                "two keys apart",
                &[
                    (7, Down, First, "x", Unidentified),
                    (8, Down, First, "y", Unidentified),
                    (7, Up, First, "X", Unidentified),
                    (8, Up, First, "Y", Unidentified),
                ],
                &["x", "y", "x", "y"],
            ),
            (
                "a release with no press passes through",
                &[(7, Up, First, "X", Unidentified)],
                &["X"],
            ),
            (
                "a press again takes its new key",
                &[
                    (7, Down, First, "x", Unidentified),
                    (7, Down, First, "X", Unidentified),
                    (7, Up, First, "x", Unidentified),
                ],
                &["x", "X", "X"],
            ),
        ];
        for &(case, steps, keys) in rows {
            let mut unplaced = UnplacedKeys::default();
            let out: Vec<Key> = steps
                .iter()
                .map(|&(id, state, repeat, key, code)| {
                    unplaced
                        .settle(id, event(state, key, code).with_repeat(repeat))
                        .key
                })
                .collect();
            let want: Vec<Key> = keys.iter().map(|key| Key::character(*key)).collect();
            assert_eq!(out, want, "{case}");
        }
    }

    #[test]
    fn a_key_no_release_follows_is_not_held() {
        let mut unplaced = UnplacedKeys::default();
        let stray = event(KeyState::Down, "é", Code::Unidentified).with_release(KeyRelease::Never);
        let _ = unplaced.settle(1_u32, stray);
        let up = unplaced.settle(1, event(KeyState::Up, "E", Code::Unidentified));
        assert_eq!(up.key, Key::character("E"), "nothing was held for it");
    }

    #[test]
    fn the_oldest_held_key_is_let_go_past_the_bound() {
        let mut unplaced = UnplacedKeys::default();
        for id in 0..=16_u32 {
            let _ = unplaced.settle(id, event(KeyState::Down, "x", Code::Unidentified));
        }
        let first = unplaced.settle(0, event(KeyState::Up, "X", Code::Unidentified));
        assert_eq!(first.key, Key::character("X"), "the oldest was let go");
        let last = unplaced.settle(16, event(KeyState::Up, "X", Code::Unidentified));
        assert_eq!(last.key, Key::character("x"));
    }
}
