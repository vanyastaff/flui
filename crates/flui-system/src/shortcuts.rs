//! Shortcuts that reach the application while another one has the focus.
//!
//! A widget acquires [`GlobalShortcuts`] through
//! `ctx.capability::<GlobalShortcutCapability>()` and binds a set of
//! [`Shortcut`]s. The application names each shortcut and may prefer a
//! [`Trigger`]; the system decides the trigger, may ask the user, and tells
//! the binding which triggers it chose ([`ShortcutEvent::Bound`]). The
//! binding then hears each press and release while it is held; dropping it
//! unbinds the shortcuts.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::{any::Any, fmt, rc::Rc};

use flui_platform::capability::{Capability, Completion, CompletionSource, ProviderContext, Resolver};
use flui_platform::keyboard::{Key, Modifiers, NamedKey};
use flui_platform::{SystemFailure, Unsupported};

/// The global-shortcut capability:
/// `ctx.capability::<GlobalShortcutCapability>()` hands out
/// [`GlobalShortcuts`].
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a capability marker has no values; it is only ever named"
)]
pub enum GlobalShortcutCapability {}

impl Capability for GlobalShortcutCapability {
    type Handle = GlobalShortcuts;
    const NAME: &'static str = "global-shortcuts";
}

/// The application's name for one shortcut: what each press reports, unique
/// within a binding and stable across runs, since a system remembers the
/// trigger the user chose by it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShortcutId(Box<str>);

impl ShortcutId {
    /// The longest id, in bytes.
    pub const MAX_LEN: usize = 64;

    /// `id`, when it is 1 to [`Self::MAX_LEN`] bytes of ASCII letters,
    /// digits, `-`, `_` and `.`; `None` otherwise.
    #[must_use]
    pub fn try_new(id: &str) -> Option<Self> {
        let valid = !id.is_empty()
            && id.len() <= Self::MAX_LEN
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
        valid.then(|| Self(id.into()))
    }

    /// The id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ShortcutId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A chord a shortcut prefers: modifiers and one key. The system may give
/// the shortcut another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trigger {
    modifiers: Modifiers,
    key: TriggerKey,
}

/// The keys a trigger takes: those every system names alike.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TriggerKey {
    /// An ASCII letter (kept in lower case) or digit.
    Character(char),
    /// A function, editing or navigation key.
    Named(NamedKey),
}

impl Trigger {
    /// The modifiers a trigger may hold.
    pub const MODIFIERS: Modifiers = Modifiers::CONTROL
        .union(Modifiers::ALT)
        .union(Modifiers::SHIFT)
        .union(Modifiers::META);

    /// `key` with `modifiers`, when the key is an ASCII letter or digit, a
    /// function key (F1 to F24), Enter, Escape, Tab, Backspace, Insert,
    /// Delete, Home, End, Page Up, Page Down or an arrow, and the modifiers
    /// are among [`Self::MODIFIERS`]; `None` otherwise.
    #[must_use]
    pub fn try_new(modifiers: Modifiers, key: &Key) -> Option<Self> {
        if !Self::MODIFIERS.contains(modifiers) {
            return None;
        }
        let key = match key {
            Key::Character(text) => {
                let mut characters = text.as_str().chars();
                match (characters.next(), characters.next()) {
                    (Some(character), None) if character.is_ascii_alphanumeric() => {
                        TriggerKey::Character(character.to_ascii_lowercase())
                    }
                    _ => return None,
                }
            }
            Key::Named(named) if keysym(*named).is_some() => TriggerKey::Named(*named),
            _ => return None,
        };
        Some(Self { modifiers, key })
    }

    /// The modifiers.
    #[must_use]
    pub fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// The trigger as the XDG shortcuts specification writes it
    /// (`CTRL+SHIFT+a`): each modifier, then the key's X keysym name.
    #[must_use]
    pub fn to_xdg(&self) -> String {
        let mut written = String::new();
        for (modifier, name) in [
            (Modifiers::CONTROL, "CTRL+"),
            (Modifiers::ALT, "ALT+"),
            (Modifiers::SHIFT, "SHIFT+"),
            (Modifiers::META, "LOGO+"),
        ] {
            if self.modifiers.contains(modifier) {
                written.push_str(name);
            }
        }
        match self.key {
            TriggerKey::Character(character) => written.push(character),
            TriggerKey::Named(named) => {
                written.push_str(&keysym(named).expect("BUG: a trigger holds a named key with a keysym"));
            }
        }
        written
    }
}

/// The X keysym name of a named key a trigger takes.
fn keysym(key: NamedKey) -> Option<String> {
    let name = match key {
        NamedKey::Enter => "Return",
        NamedKey::Escape => "Escape",
        NamedKey::Tab => "Tab",
        NamedKey::Backspace => "BackSpace",
        NamedKey::Insert => "Insert",
        NamedKey::Delete => "Delete",
        NamedKey::Home => "Home",
        NamedKey::End => "End",
        NamedKey::PageUp => "Page_Up",
        NamedKey::PageDown => "Page_Down",
        NamedKey::ArrowLeft => "Left",
        NamedKey::ArrowRight => "Right",
        NamedKey::ArrowUp => "Up",
        NamedKey::ArrowDown => "Down",
        _ => return function_key(key).map(|number| format!("F{number}")),
    };
    Some(name.to_owned())
}

/// The number of a function key, F1 to F24.
fn function_key(key: NamedKey) -> Option<u8> {
    const KEYS: [NamedKey; 24] = [
        NamedKey::F1,
        NamedKey::F2,
        NamedKey::F3,
        NamedKey::F4,
        NamedKey::F5,
        NamedKey::F6,
        NamedKey::F7,
        NamedKey::F8,
        NamedKey::F9,
        NamedKey::F10,
        NamedKey::F11,
        NamedKey::F12,
        NamedKey::F13,
        NamedKey::F14,
        NamedKey::F15,
        NamedKey::F16,
        NamedKey::F17,
        NamedKey::F18,
        NamedKey::F19,
        NamedKey::F20,
        NamedKey::F21,
        NamedKey::F22,
        NamedKey::F23,
        NamedKey::F24,
    ];
    let place = KEYS.iter().position(|candidate| *candidate == key)?;
    u8::try_from(place + 1).ok()
}

/// One shortcut a binding asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Shortcut {
    /// Its name, which presses report.
    pub id: ShortcutId,
    /// What it does, as the system shows the user: one line.
    pub description: String,
    /// The trigger it prefers, if any.
    pub preferred: Option<Trigger>,
}

impl Shortcut {
    /// The longest description, in bytes.
    pub const MAX_DESCRIPTION_LEN: usize = 256;

    /// The shortcut `id`, described as `description`, with no preferred
    /// trigger.
    #[must_use]
    pub fn new(id: ShortcutId, description: impl Into<String>) -> Self {
        Self {
            id,
            description: description.into(),
            preferred: None,
        }
    }

    /// Prefer `trigger`.
    #[must_use]
    pub fn preferred(mut self, trigger: Trigger) -> Self {
        self.preferred = Some(trigger);
        self
    }
}

/// What is wrong with a binding's shortcuts, which the handle refuses before
/// the system is asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MalformedShortcut {
    /// No shortcut, or more than [`GlobalShortcuts::MAX_PER_BINDING`].
    #[error("a binding holds no shortcut or too many")]
    Count,
    /// Two shortcuts share an id.
    #[error("two shortcuts share an id")]
    DuplicateId,
    /// A description is blank, longer than
    /// [`Shortcut::MAX_DESCRIPTION_LEN`], or holds a control character.
    #[error("a description is not one line of text")]
    Description,
}

/// A shortcut as the system bound it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BoundShortcut {
    /// The shortcut's id.
    pub id: ShortcutId,
    /// The trigger as the system shows it to the user (`Ctrl+Shift+A`), or
    /// `None` when the system bound it to no trigger yet.
    pub trigger: Option<String>,
}

impl BoundShortcut {
    /// The shortcut `id`, shown to the user as `trigger`.
    #[must_use]
    pub fn new(id: ShortcutId, trigger: Option<String>) -> Self {
        Self { id, trigger }
    }
}

/// Why a binding ended, or never started.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ShortcutError {
    /// The shortcuts are malformed, as the [`MalformedShortcut`] says; the
    /// system was not asked.
    #[error("the shortcuts are malformed: {0}")]
    InvalidRequest(#[from] MalformedShortcut),
    /// The user, or the system's policy, refused the shortcuts.
    #[error("the user or the system refused")]
    Denied,
    /// The system ended the binding: the user revoked it, or the service
    /// closed it.
    #[error("the system ended the binding")]
    Revoked,
    /// The service is not here (no shortcuts portal, or one too old), as the
    /// [`Unsupported`] says.
    #[error("global shortcuts are not available: {0}")]
    Unsupported(#[from] Unsupported),
    /// The system failed in a way the handle does not name.
    #[error("the system failed: {0}")]
    Failed(#[from] SystemFailure),
}

/// What a binding hears.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ShortcutEvent {
    /// The system bound the shortcuts it lists, each with its trigger; one
    /// the system left out is not bound. It comes again after the system's
    /// service restarted and the binding was renewed.
    Bound(Vec<BoundShortcut>),
    /// The trigger of `id` was pressed.
    Pressed(ShortcutId),
    /// The trigger of `id` was released.
    Released(ShortcutId),
    /// The user changed the triggers; the list is the whole binding now.
    Changed(Vec<BoundShortcut>),
    /// The binding ended: no shortcut fires any more, and every later
    /// [`ShortcutBinding::next`] answers this again.
    Ended(ShortcutError),
}

/// The events a binding has heard and not handed out yet, and who waits for
/// the next: what a backend sends through from any thread.
struct FeedState {
    queued: VecDeque<ShortcutEvent>,
    waiting: VecDeque<Resolver<ShortcutEvent>>,
    /// The end, once sent: what every event after the queue drains is.
    ended: Option<ShortcutError>,
    /// How many presses and releases were dropped because nobody took them.
    dropped: u64,
}

/// Where a [`ShortcutBackend`] sends what the system tells one binding.
///
/// `Send` and cheap to clone; the system's events arrive on whichever thread
/// the backend hears them. The binding takes them in order with
/// [`ShortcutBinding::next`]. At most [`Self::MAX_QUEUED`] wait untaken; a
/// press or release beyond that drops the oldest press or release, never a
/// `Bound`, a `Changed` or the end.
#[derive(Clone)]
pub struct ShortcutFeed(Arc<Mutex<FeedState>>);

impl ShortcutFeed {
    /// The most events a binding holds that nobody took.
    pub const MAX_QUEUED: usize = 256;

    fn new() -> Self {
        Self(Arc::new(Mutex::new(FeedState {
            queued: VecDeque::new(),
            waiting: VecDeque::new(),
            ended: None,
            dropped: 0,
        })))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FeedState> {
        // A panic while the lock is held leaves a queue that is whole.
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Send `event`; one sent after the end is dropped. An
    /// [`Ended`](ShortcutEvent::Ended) ends the feed.
    pub fn send(&self, event: ShortcutEvent) {
        let waiting = {
            let mut state = self.lock();
            if state.ended.is_some() {
                return;
            }
            if let ShortcutEvent::Ended(error) = &event {
                state.ended = Some(*error);
            }
            match state.waiting.pop_front() {
                Some(waiting) => Some((waiting, event)),
                None => {
                    if state.queued.len() >= Self::MAX_QUEUED {
                        let input = state.queued.iter().position(|queued| {
                            matches!(queued, ShortcutEvent::Pressed(_) | ShortcutEvent::Released(_))
                        });
                        match input {
                            Some(oldest) => {
                                drop(state.queued.remove(oldest));
                                state.dropped += 1;
                            }
                            // Only bindings and changes queued: this press
                            // or release is the one that goes.
                            None if matches!(
                                event,
                                ShortcutEvent::Pressed(_) | ShortcutEvent::Released(_)
                            ) =>
                            {
                                state.dropped += 1;
                                return;
                            }
                            None => {}
                        }
                    }
                    state.queued.push_back(event);
                    None
                }
            }
        };
        // Resolved outside the lock: settling posts to the owner.
        if let Some((waiting, event)) = waiting {
            let _ = waiting.resolve(event);
        }
    }

    /// The next event, for `completions`' owner.
    fn next(&self, completions: &CompletionSource) -> Completion<ShortcutEvent> {
        let (completion, answer) = completions.pending();
        let ready = {
            let mut state = self.lock();
            match state.queued.pop_front() {
                Some(event) => Some((answer, event)),
                None => match state.ended {
                    Some(error) => Some((answer, ShortcutEvent::Ended(error))),
                    None => {
                        state.waiting.push_back(answer);
                        None
                    }
                },
            }
        };
        if let Some((answer, event)) = ready {
            // The owner retiring first leaves nothing waiting.
            let _ = answer.resolve(event);
        }
        completion
    }

    /// How many presses and releases were dropped untaken.
    fn dropped(&self) -> u64 {
        self.lock().dropped
    }
}

impl fmt::Debug for ShortcutFeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.lock();
        f.debug_struct("ShortcutFeed")
            .field("queued", &state.queued.len())
            .field("ended", &state.ended)
            .finish_non_exhaustive()
    }
}

/// What a [`ShortcutBackend`] gives for each binding: the system's binding
/// lasts while the handle holds it, and its drop unbinds the shortcuts.
#[must_use = "dropping the hold unbinds the shortcuts"]
pub struct ShortcutHold(#[expect(dead_code, reason = "held for its drop")] Box<dyn Any>);

impl ShortcutHold {
    /// A hold whose drop drops `keep`, the value whose `Drop` unbinds.
    pub fn new(keep: impl Any) -> Self {
        Self(Box::new(keep))
    }
}

impl fmt::Debug for ShortcutHold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShortcutHold").finish_non_exhaustive()
    }
}

/// The system's global shortcuts as [`GlobalShortcuts`] reaches them.
///
/// Owner-local. The shortcuts it receives are well formed. It sends what the
/// system tells the binding through `feed`, ending it with
/// [`ShortcutEvent::Ended`] when the binding ends on the system's side; the
/// hold it returns unbinds when dropped.
pub trait ShortcutBackend: 'static {
    /// Bind `shortcuts`.
    fn bind(&self, shortcuts: Vec<Shortcut>, feed: ShortcutFeed) -> ShortcutHold;
}

/// Widget-facing global shortcuts.
///
/// Owner-thread (`!Send`), cheap to clone.
#[derive(Clone)]
pub struct GlobalShortcuts {
    backend: Rc<dyn ShortcutBackend>,
    completions: CompletionSource,
}

impl GlobalShortcuts {
    /// The most shortcuts one binding holds.
    pub const MAX_PER_BINDING: usize = 64;

    /// A handle over `backend` for the acquisition `ctx`.
    pub fn new(backend: impl ShortcutBackend, ctx: &ProviderContext<'_>) -> Self {
        Self {
            backend: Rc::new(backend),
            completions: ctx.completions().clone(),
        }
    }

    /// Bind `shortcuts`; the binding hears the system's answer and every
    /// press while it is held. Malformed shortcuts end the binding at once
    /// with [`ShortcutError::InvalidRequest`], and the system is not asked.
    pub fn bind(&self, shortcuts: Vec<Shortcut>) -> ShortcutBinding {
        let feed = ShortcutFeed::new();
        let hold = match check(&shortcuts) {
            Ok(()) => Some(self.backend.bind(shortcuts, feed.clone())),
            Err(malformed) => {
                feed.send(ShortcutEvent::Ended(malformed.into()));
                None
            }
        };
        ShortcutBinding {
            feed,
            completions: self.completions.clone(),
            hold,
        }
    }
}

/// The first thing wrong with `shortcuts`.
fn check(shortcuts: &[Shortcut]) -> Result<(), MalformedShortcut> {
    if shortcuts.is_empty() || shortcuts.len() > GlobalShortcuts::MAX_PER_BINDING {
        return Err(MalformedShortcut::Count);
    }
    for (index, shortcut) in shortcuts.iter().enumerate() {
        let description = &shortcut.description;
        if description.len() > Shortcut::MAX_DESCRIPTION_LEN
            || description.trim().is_empty()
            || description.chars().any(char::is_control)
        {
            return Err(MalformedShortcut::Description);
        }
        if shortcuts[..index]
            .iter()
            .any(|earlier| earlier.id == shortcut.id)
        {
            return Err(MalformedShortcut::DuplicateId);
        }
    }
    Ok(())
}

impl fmt::Debug for GlobalShortcuts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GlobalShortcuts").finish_non_exhaustive()
    }
}

/// A set of shortcuts the system has bound, or is asked to bind.
///
/// Owner-thread (`!Send`). The shortcuts stay bound while it is held;
/// dropping it unbinds them, and a completion still waiting from
/// [`next`](Self::next) ends `ProviderDropped`.
#[must_use = "dropping a binding unbinds its shortcuts"]
pub struct ShortcutBinding {
    feed: ShortcutFeed,
    completions: CompletionSource,
    /// `None` for a binding refused before the system was asked.
    hold: Option<ShortcutHold>,
}

impl ShortcutBinding {
    /// The next event the binding hears, in order; it settles at an owner
    /// turn. After [`ShortcutEvent::Ended`], every call answers it again.
    pub fn next(&self) -> Completion<ShortcutEvent> {
        self.feed.next(&self.completions)
    }

    /// How many presses and releases were dropped because more than
    /// [`ShortcutFeed::MAX_QUEUED`] waited untaken.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.feed.dropped()
    }
}

impl Drop for ShortcutBinding {
    fn drop(&mut self) {
        // Unbind first, then let the completions still waiting end.
        drop(self.hold.take());
        let waiting = std::mem::take(&mut self.feed.lock().waiting);
        drop(waiting);
    }
}

impl fmt::Debug for ShortcutBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShortcutBinding")
            .field("feed", &self.feed)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::keyboard::{Key, Modifiers, NamedKey};

    use super::{ShortcutId, Trigger};

    #[test]
    fn a_trigger_is_written_as_the_xdg_shortcuts_specification_has_it() {
        let ctrl_shift = Modifiers::CONTROL.union(Modifiers::SHIFT);
        let cases: [(Modifiers, Key, Option<&str>); 7] = [
            (ctrl_shift, Key::character("A"), Some("CTRL+SHIFT+a")),
            (Modifiers::META, Key::character("7"), Some("LOGO+7")),
            (Modifiers::ALT, Key::Named(NamedKey::F12), Some("ALT+F12")),
            (Modifiers::NONE, Key::Named(NamedKey::PageUp), Some("Page_Up")),
            // A character no system names alike, a modifier no trigger
            // takes, a key with no keysym.
            (Modifiers::CONTROL, Key::character("é"), None),
            (Modifiers::CAPS_LOCK, Key::character("a"), None),
            (Modifiers::NONE, Key::Named(NamedKey::MediaPlay), None),
        ];
        for (modifiers, key, written) in cases {
            assert_eq!(
                Trigger::try_new(modifiers, &key).map(|trigger| trigger.to_xdg()),
                written.map(str::to_owned),
                "{modifiers:?} {key:?}"
            );
        }
        assert!(ShortcutId::try_new("toggle-recording").is_some());
        assert!(ShortcutId::try_new("").is_none());
        assert!(ShortcutId::try_new("with space").is_none());
    }
}
