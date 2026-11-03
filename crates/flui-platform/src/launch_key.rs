//! The keys a process's launch values are read under (ADR-0288 §7).
//!
//! What started the process hands it values it reads once, as it starts: the environment of a
//! desktop process, the variables `simctl launch` passes an iOS Simulator app, the string
//! extras of the intent that started an Android Activity. Each host reads a key in its own
//! spelling, so a value is asked for once, by one [`LaunchKey`], on every host.

use std::fmt;

/// The longest launch key, in bytes.
pub const MAX_LAUNCH_KEY_LEN: usize = 128;

/// The key a launch value is read under, in its dotted spelling (`flui.agent.token`).
///
/// An Android host reads the launching intent's string extra of this name; a host that reads
/// the environment reads the variable [`environment_variable`](Self::environment_variable)
/// spells (`FLUI_AGENT_TOKEN`). The rules keep both spellings valid names on every host:
/// lowercase ASCII letters, digits, `_` and `.`; starting with a letter; no `.` last or twice
/// in a row; at most [`MAX_LAUNCH_KEY_LEN`] bytes.
///
/// ```
/// use flui_platform::LaunchKey;
///
/// let key = LaunchKey::try_new("flui.agent.exit_on_disconnect").expect("a valid key");
/// assert_eq!(key.environment_variable(), "FLUI_AGENT_EXIT_ON_DISCONNECT");
/// assert!(LaunchKey::try_new("Flui.Agent").is_none());
/// assert!(LaunchKey::try_new("flui..agent").is_none());
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct LaunchKey(&'static str);

impl LaunchKey {
    /// `key`, when it keeps the rules on [`LaunchKey`]; `None` otherwise. A `const fn`, so a
    /// key named in a constant is checked as the crate compiles.
    #[must_use]
    pub const fn try_new(key: &'static str) -> Option<Self> {
        let bytes = key.as_bytes();
        if bytes.is_empty() || bytes.len() > MAX_LAUNCH_KEY_LEN || !bytes[0].is_ascii_lowercase() {
            return None;
        }
        let mut index = 0;
        while index < bytes.len() {
            let byte = bytes[index];
            let allowed = byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_';
            let dot_between = byte == b'.' && index + 1 < bytes.len() && bytes[index + 1] != b'.';
            if !allowed && !dot_between {
                return None;
            }
            index += 1;
        }
        Some(Self(key))
    }

    /// The key's dotted spelling, which an Android intent extra is named with.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }

    /// The environment variable the key is read from where a host reads the environment:
    /// uppercase, with each `.` an `_`.
    #[must_use]
    pub fn environment_variable(self) -> String {
        self.0
            .bytes()
            .map(|byte| match byte {
                b'.' => '_',
                other => char::from(other.to_ascii_uppercase()),
            })
            .collect::<String>()
    }
}

impl fmt::Debug for LaunchKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("LaunchKey").field(&self.0).finish()
    }
}

impl fmt::Display for LaunchKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}
