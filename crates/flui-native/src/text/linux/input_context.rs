//! The rules of an input method's D-Bus input context, as the X11 host
//! speaks to IBus and Fcitx5: which daemon to ask, how a key waits for its
//! answer, and the protocols' values for what the field states.
//!
//! An X11 client hands each key to the input method and waits for it to say
//! whether it took the key. Nothing on the owner thread may block on the
//! bus, so a key asked about is held ([`KeyHold`]): keys behind it wait in
//! order, a key the input method did not take is delivered as typed, and a
//! key whose answer does not come within [`ANSWER_TIMEOUT`] is delivered as
//! not taken, its late answer dropped by its [`Ticket`]. A key is never
//! dropped and never overtakes one before it.
//!
//! Pure, so its rules run on every host; `text::linux::x11` binds it to the
//! bus and the X11 loop.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;

use web_time::Instant;

use super::text_input::ContentType;

#[cfg(test)]
mod tests;

/// How long a key waits for the input method's answer before it is
/// delivered as not taken. GTK waits about 100 ms where it waits at all; an
/// engine that converts on a key (a kana-kanji lookup) may take longer, and
/// typing does not feel a fifth of a second held behind it.
pub(crate) const ANSWER_TIMEOUT: Duration = Duration::from_millis(200);

/// The most keys held at once. Past it the oldest key still waiting is
/// delivered as not taken, so an input method that never answers holds
/// neither the keys nor memory.
pub(crate) const MAX_HELD_KEYS: usize = 256;

/// Names one key asked of the input method, and its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Ticket(u64);

/// Where a held key is in its wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fate {
    /// Asked, no answer yet.
    Asked { ticket: Ticket, deadline: Instant },
    /// Delivered in its turn: not asked, not taken, or not answered in time.
    Deliver,
    /// The input method took it: it goes in its turn, undelivered.
    Taken,
}

/// The keys between the X server and the windows while the input method
/// decides on them, in the order the server sent them.
#[derive(Debug)]
pub(crate) struct KeyHold<K> {
    queue: VecDeque<(K, Fate)>,
    next: u64,
}

impl<K> Default for KeyHold<K> {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            next: 0,
        }
    }
}

impl<K> KeyHold<K> {
    /// `key` waits for the input method's answer from `now`: the ticket its
    /// answer names. Past [`MAX_HELD_KEYS`] the oldest key still waiting
    /// stops waiting first, told to `evicted`: as with a key overdue, the
    /// input method that has it may still act on it.
    pub(crate) fn ask(&mut self, key: K, now: Instant, evicted: impl FnOnce(&K)) -> Ticket {
        if self.queue.len() >= MAX_HELD_KEYS
            && let Some((oldest, fate)) = self
                .queue
                .iter_mut()
                .find(|(_, fate)| matches!(fate, Fate::Asked { .. }))
        {
            *fate = Fate::Deliver;
            evicted(oldest);
        }
        let ticket = Ticket(self.next);
        self.next = self
            .next
            .checked_add(1)
            .expect("BUG: a process asks fewer than 2^64 keys");
        self.queue.push_back((
            key,
            Fate::Asked {
                ticket,
                deadline: now + ANSWER_TIMEOUT,
            },
        ));
        ticket
    }

    /// `key` is not asked: it is delivered in its turn, behind the keys held.
    pub(crate) fn pass(&mut self, key: K) {
        self.queue.push_back((key, Fate::Deliver));
    }

    /// The input method's answer for `ticket`: whether it took the key.
    /// `false` when no key waits on `ticket` (it timed out, or its context
    /// went), and the answer is dropped.
    pub(crate) fn answer(&mut self, ticket: Ticket, taken: bool) -> bool {
        let Some((_, fate)) = self
            .queue
            .iter_mut()
            .find(|(_, fate)| matches!(fate, Fate::Asked { ticket: held, .. } if *held == ticket))
        else {
            return false;
        };
        *fate = if taken { Fate::Taken } else { Fate::Deliver };
        true
    }

    /// Every key whose answer is due by `now` is delivered as not taken,
    /// each told to `overdue`: the input method that left it unanswered may
    /// still act on it.
    pub(crate) fn expire(&mut self, now: Instant, mut overdue: impl FnMut(&K)) {
        for (key, fate) in &mut self.queue {
            if matches!(fate, Fate::Asked { deadline, .. } if *deadline <= now) {
                *fate = Fate::Deliver;
                overdue(key);
            }
        }
    }

    /// Every key `which` names stops waiting and is delivered as not taken:
    /// the input context it was asked of went, or its window lost the focus.
    pub(crate) fn release_where(&mut self, which: impl Fn(&K) -> bool) {
        for (key, fate) in &mut self.queue {
            if matches!(fate, Fate::Asked { .. }) && which(key) {
                *fate = Fate::Deliver;
            }
        }
    }

    /// The keys whose turn came, in order: the front of the queue up to the
    /// first key still waiting, those the input method took left out.
    pub(crate) fn ready(&mut self) -> Vec<K> {
        let mut ready = Vec::new();
        while let Some((_, fate)) = self.queue.front() {
            match fate {
                Fate::Asked { .. } => break,
                Fate::Deliver => {
                    let (key, _) = self
                        .queue
                        .pop_front()
                        .expect("BUG: the front was just read");
                    ready.push(key);
                }
                Fate::Taken => drop(self.queue.pop_front()),
            }
        }
        ready
    }

    /// When the first answer still awaited is due.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.queue
            .iter()
            .filter_map(|(_, fate)| match fate {
                Fate::Asked { deadline, .. } => Some(*deadline),
                _ => None,
            })
            .min()
    }
}

// ============================================================================
// Which input method
// ============================================================================

/// An input method daemon an X11 client reaches over D-Bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Daemon {
    /// IBus, on its own bus.
    IBus,
    /// Fcitx5, on the session bus.
    Fcitx5,
}

/// Where the input method is asked for an input context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Endpoint {
    pub(crate) daemon: Daemon,
    /// Through the daemon's portal on the session bus: inside a sandbox,
    /// where the daemon's own bus and name are out of reach.
    pub(crate) portal: bool,
}

/// The endpoints to try, in order, as the environment names them: the input
/// method `XMODIFIERS` names (`@im=fcitx`, `@im=ibus`), else the one
/// `GTK_IM_MODULE` or `QT_IM_MODULE` names, else IBus first. Inside a
/// sandbox (`sandboxed`) each is reached through its portal.
pub(crate) fn endpoints(var: impl Fn(&str) -> Option<String>, sandboxed: bool) -> [Endpoint; 2] {
    let named = |value: &str| {
        let value = value.to_ascii_lowercase();
        if value.contains("fcitx") {
            Some(Daemon::Fcitx5)
        } else if value.contains("ibus") {
            Some(Daemon::IBus)
        } else {
            None
        }
    };
    let first = var("XMODIFIERS")
        .and_then(|modifiers| {
            modifiers
                .split('@')
                .find_map(|part| part.strip_prefix("im=").map(str::to_owned))
        })
        .and_then(|name| named(&name))
        .or_else(|| var("GTK_IM_MODULE").and_then(|name| named(&name)))
        .or_else(|| var("QT_IM_MODULE").and_then(|name| named(&name)))
        .unwrap_or(Daemon::IBus);
    let second = match first {
        Daemon::IBus => Daemon::Fcitx5,
        Daemon::Fcitx5 => Daemon::IBus,
    };
    [first, second].map(|daemon| Endpoint {
        daemon,
        portal: sandboxed,
    })
}

/// The file IBus writes its bus's address to, as `ibus_get_socket_path`
/// finds it for an X11 client: `IBUS_ADDRESS_FILE`, else
/// `<config>/ibus/bus/<machine id>-<host>-<display number>`, the host and
/// number read from `DISPLAY` (`unix` and `0` where it names none).
pub(crate) fn ibus_address_file(
    var: impl Fn(&str) -> Option<String>,
    machine_id: &str,
    config_dir: &Path,
) -> PathBuf {
    if let Some(file) = var("IBUS_ADDRESS_FILE") {
        return PathBuf::from(file);
    }
    let display = var("DISPLAY").unwrap_or_default();
    let (host, rest) = display.split_once(':').unwrap_or((display.as_str(), ""));
    let number = rest.split('.').next().filter(|number| !number.is_empty());
    let host = if host.is_empty() { "unix" } else { host };
    config_dir.join("ibus").join("bus").join(format!(
        "{}-{host}-{}",
        machine_id.trim(),
        number.unwrap_or("0")
    ))
}

/// The bus address an IBus address file names (`IBUS_ADDRESS=` line).
pub(crate) fn ibus_address(file: &str) -> Option<String> {
    file.lines()
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| line.strip_prefix("IBUS_ADDRESS="))
        .map(str::trim)
        .filter(|address| !address.is_empty())
        .map(str::to_owned)
}

// ============================================================================
// What the field states, in each protocol's terms
// ============================================================================

/// `IBUS_CAP_PREEDIT_TEXT | IBUS_CAP_FOCUS | IBUS_CAP_SURROUNDING_TEXT`: the
/// client shows the preedit in the field, has the focus, and states the text
/// around the caret.
#[cfg(x11_host)]
pub(crate) const IBUS_CAPABILITIES: u32 = (1 << 0) | (1 << 3) | (1 << 5);

/// `IBUS_RELEASE_MASK`: the key is released.
#[cfg(x11_host)]
pub(crate) const IBUS_RELEASE: u32 = 1 << 30;

/// Fcitx5's `Preedit | FormattedPreedit | SurroundingText`.
const FCITX_BASE: u64 = (1 << 1) | (1 << 4) | (1 << 6);

/// The X server's key codes are the kernel's plus 8; IBus takes the
/// kernel's.
#[cfg(x11_host)]
pub(crate) const X_KEYCODE_OFFSET: u32 = 8;

/// IBus's `ContentType`, purpose and hints, of `content`.
pub(crate) fn ibus_content(content: ContentType) -> (u32, u32) {
    // IBusInputPurpose: FREE_FORM 0, NUMBER 3, PHONE 4, URL 5, EMAIL 6,
    // PASSWORD 8.
    let purpose = match content.purpose {
        ContentType::NUMBER => 3,
        ContentType::PHONE => 4,
        ContentType::URL => 5,
        ContentType::EMAIL => 6,
        ContentType::PASSWORD => 8,
        _ => 0,
    };
    // IBusInputHints: PRIVATE 1 << 11, HIDDEN_TEXT 1 << 12, MULTILINE 1 << 14.
    let mut hints = 0;
    if content.hint & ContentType::HINT_SENSITIVE_DATA != 0 {
        hints |= 1 << 11;
    }
    if content.hint & ContentType::HINT_HIDDEN_TEXT != 0 {
        hints |= 1 << 12;
    }
    if content.hint & ContentType::HINT_MULTILINE != 0 {
        hints |= 1 << 14;
    }
    (purpose, hints)
}

/// Fcitx5's capability flags of a context serving a field of `content`:
/// the client's own capabilities with the field's purpose and hints.
pub(crate) fn fcitx_capability(content: ContentType) -> u64 {
    // fcitx::CapabilityFlag: Password 1 << 3, Email 1 << 7, Url 1 << 12,
    // Dialable 1 << 13, Number 1 << 14, Multiline 1 << 35, Sensitive 1 << 36.
    let mut flags = FCITX_BASE
        | match content.purpose {
            ContentType::PASSWORD => 1 << 3,
            ContentType::EMAIL => 1 << 7,
            ContentType::URL => 1 << 12,
            ContentType::PHONE => 1 << 13,
            ContentType::NUMBER => 1 << 14,
            _ => 0,
        };
    if content.hint & ContentType::HINT_HIDDEN_TEXT != 0 {
        flags |= 1 << 3;
    }
    if content.hint & ContentType::HINT_MULTILINE != 0 {
        flags |= 1 << 35;
    }
    if content.hint & ContentType::HINT_SENSITIVE_DATA != 0 {
        flags |= 1 << 36;
    }
    flags
}

/// The characters of `text` before byte `byte`, which both daemons count
/// the surrounding text's cursor in. A byte inside a character counts it.
pub(crate) fn chars_before(text: &str, byte: usize) -> u32 {
    let count = text
        .char_indices()
        .take_while(|(at, _)| *at < byte.min(text.len()))
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// The byte where character `chars` of `text` starts; the text's end past
/// its last.
pub(crate) fn byte_of_char(text: &str, chars: u32) -> usize {
    usize::try_from(chars)
        .ok()
        .and_then(|chars| text.char_indices().nth(chars))
        .map_or(text.len(), |(at, _)| at)
}

/// The surrounding text the input method was last told: its text, and the
/// cursor and anchor as bytes of it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Told {
    pub(crate) text: String,
    pub(crate) cursor: usize,
    pub(crate) anchor: usize,
}

/// A deletion as both daemons send it, `chars` characters from `offset`
/// characters past the cursor (negative before it), as the bytes before and
/// after the selection the text input client deletes. `None` where the span
/// does not reach the selection on both sides (it lies wholly before or
/// after it), which the client's deletion cannot state.
pub(crate) fn deletion(told: &Told, offset: i32, chars: u32) -> Option<(u32, u32)> {
    let cursor = i64::from(chars_before(&told.text, told.cursor));
    let start = cursor.checked_add(i64::from(offset))?;
    let end = start.checked_add(i64::from(chars))?;
    let (low, high) = (told.cursor.min(told.anchor), told.cursor.max(told.anchor));
    let (low_chars, high_chars) = (
        i64::from(chars_before(&told.text, low)),
        i64::from(chars_before(&told.text, high)),
    );
    if start < 0 || start > low_chars || end < high_chars {
        return None;
    }
    let at = |chars: i64| byte_of_char(&told.text, u32::try_from(chars).unwrap_or(u32::MAX));
    let before = low - at(start);
    let after = at(end).saturating_sub(high);
    Some((
        u32::try_from(before).unwrap_or(u32::MAX),
        u32::try_from(after).unwrap_or(u32::MAX),
    ))
}

/// A preedit's caret, in bytes of its text, from a count of characters
/// (IBus's `cursor_pos`).
pub(crate) fn preedit_caret(text: &str, chars: u32) -> i32 {
    i32::try_from(byte_of_char(text, chars)).unwrap_or(i32::MAX)
}
