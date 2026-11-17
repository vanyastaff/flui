//! Global shortcuts: a key chord the application hears while another
//! application has the focus.
//!
//! A widget acquires [`GlobalShortcuts`] through
//! `ctx.capability::<GlobalShortcutCapability>()` and registers a
//! [`Shortcut`] with a listener; the shortcut stays registered while its
//! [`ShortcutRegistration`] is held.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use flui_platform::capability::{
    Capability, Completion, CompletionSource, ProviderContext, Resolver,
};
use flui_platform::keyboard::{Code, Modifiers};
use flui_platform::{SystemFailure, Unsupported};

/// The global shortcut capability:
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
    const NAME: &'static str = "global-shortcut";
}

/// A key chord: modifiers held and the key pressed, named by its position
/// ([`Code`]), so a shortcut stays on the same key whatever the keyboard
/// layout prints on it.
///
/// A global shortcut takes its chord from every other application, so it
/// holds Control, Alt or Meta, unless its key is one no application types
/// with (F13 to F24, the media and volume keys). Caps Lock, Num Lock and the
/// other lock and level keys are not modifiers of a shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Shortcut {
    modifiers: Modifiers,
    code: Code,
}

impl Shortcut {
    /// The modifiers a shortcut may hold.
    pub const MODIFIERS: Modifiers = Modifiers::CONTROL
        .union(Modifiers::ALT)
        .union(Modifiers::SHIFT)
        .union(Modifiers::META);

    /// The chord `modifiers` + `code`.
    ///
    /// # Errors
    ///
    /// [`InvalidShortcut::Modifier`] when `modifiers` holds one outside
    /// [`Shortcut::MODIFIERS`]; [`InvalidShortcut::NeedsModifier`] when it
    /// holds neither Control, Alt nor Meta and `code` is not F13 to F24 or
    /// a media or volume key.
    pub fn try_new(modifiers: Modifiers, code: Code) -> Result<Self, InvalidShortcut> {
        if !Self::MODIFIERS.contains(modifiers) {
            return Err(InvalidShortcut::Modifier);
        }
        let commanding = [Modifiers::CONTROL, Modifiers::ALT, Modifiers::META]
            .into_iter()
            .any(|modifier| modifiers.contains(modifier));
        if !commanding && !is_untyped_key(code) {
            return Err(InvalidShortcut::NeedsModifier);
        }
        Ok(Self { modifiers, code })
    }

    /// The modifiers held.
    #[must_use]
    pub const fn modifiers(self) -> Modifiers {
        self.modifiers
    }

    /// The key pressed.
    #[must_use]
    pub const fn code(self) -> Code {
        self.code
    }
}

/// Whether `code` is a key no application types with: F13 to F24, and the
/// media and volume keys.
fn is_untyped_key(code: Code) -> bool {
    matches!(
        code,
        Code::F13
            | Code::F14
            | Code::F15
            | Code::F16
            | Code::F17
            | Code::F18
            | Code::F19
            | Code::F20
            | Code::F21
            | Code::F22
            | Code::F23
            | Code::F24
            | Code::MediaPlayPause
            | Code::MediaStop
            | Code::MediaTrackNext
            | Code::MediaTrackPrevious
            | Code::AudioVolumeDown
            | Code::AudioVolumeMute
            | Code::AudioVolumeUp
    )
}

/// Why a chord is not a [`Shortcut`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidShortcut {
    /// It holds a modifier outside Control, Alt, Shift and Meta.
    #[error("a shortcut holds only Control, Alt, Shift and Meta")]
    Modifier,
    /// It holds neither Control, Alt nor Meta, and its key is one other
    /// applications type with.
    #[error("a global shortcut needs Control, Alt or Meta")]
    NeedsModifier,
}

/// Why a shortcut was not registered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ShortcutError {
    /// Another application, or another registration of this one, holds the
    /// chord.
    #[error("the shortcut is taken")]
    Taken,
    /// The system cannot register a chord on this key.
    #[error("the system cannot register a shortcut on this key")]
    KeyNotSupported,
    /// This system has no global shortcuts, or not for this application
    /// (a sandbox without the portal); the [`Unsupported`] says why.
    #[error("global shortcuts cannot do this: {0}")]
    Unsupported(#[from] Unsupported),
    /// The system failed in a way the handle does not name.
    #[error("the system failed: {0}")]
    Failed(#[from] SystemFailure),
}

/// What a registration request answers.
pub type ShortcutResult = Result<ShortcutRegistration, ShortcutError>;

/// What a backend gives for a shortcut it registers: the shortcut stays
/// registered while the handle holds it, and dropping it unregisters.
///
/// Dropped on the owner thread, with none of the handle's state borrowed.
#[must_use = "dropping the hold unregisters the shortcut"]
pub struct ShortcutHold(#[expect(dead_code, reason = "held for its drop")] Box<dyn Any>);

impl ShortcutHold {
    /// A hold whose drop drops `keep`, the value whose `Drop` unregisters.
    pub fn new(keep: impl Any) -> Self {
        Self(Box::new(keep))
    }
}

impl fmt::Debug for ShortcutHold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShortcutHold").finish_non_exhaustive()
    }
}

/// Where a backend reports the presses of one registered shortcut.
///
/// Owner-thread (`!Send`). Each [`press`](Self::press) runs the listener
/// once, at the owner's next turn, never inside the call: a backend reports
/// from wherever the system tells it (a window procedure, a bus signal
/// handed to the owner) with nothing of its own borrowed by the listener.
/// A press after the registration ended runs nothing.
#[derive(Clone)]
pub struct ShortcutPresses {
    listener: Rc<RefCell<dyn FnMut()>>,
    active: Rc<Cell<bool>>,
    completions: CompletionSource,
}

impl ShortcutPresses {
    /// Report one press: the listener runs at the owner's next turn.
    pub fn press(&self) {
        if !self.active.get() {
            return;
        }
        let (pressed, answer) = self.completions.pending::<()>();
        if answer.resolve(()).is_err() {
            // The owner retired: nothing runs any more.
            return;
        }
        let listener = Rc::clone(&self.listener);
        let active = Rc::clone(&self.active);
        pressed.on_complete(move |_| {
            // A registration dropped between the press and this turn hears
            // nothing; a listener cannot be running here, since presses
            // never run it inline.
            if active.get()
                && let Ok(mut listener) = listener.try_borrow_mut()
            {
                listener();
            }
        });
    }
}

impl fmt::Debug for ShortcutPresses {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShortcutPresses")
            .field("active", &self.active.get())
            .finish_non_exhaustive()
    }
}

/// The system's global shortcuts as [`GlobalShortcuts`] reaches them.
///
/// Owner-local. A backend answers each registration through its
/// [`Resolver`]: `Ok(())` once the system holds the chord for this
/// application, or why not. From then until the returned hold drops, it
/// reports each press to `presses`.
pub trait GlobalShortcutBackend: 'static {
    /// Register `shortcut`.
    fn register(
        &self,
        shortcut: Shortcut,
        presses: ShortcutPresses,
        answer: Resolver<Result<(), ShortcutError>>,
    ) -> ShortcutHold;
}

/// Widget-facing global shortcuts.
///
/// Owner-thread (`!Send`), cheap to clone.
#[derive(Clone)]
pub struct GlobalShortcuts {
    backend: Rc<dyn GlobalShortcutBackend>,
    completions: CompletionSource,
}

impl GlobalShortcuts {
    /// A handle over `backend` for the acquisition `ctx`.
    #[must_use]
    pub fn new(backend: impl GlobalShortcutBackend, ctx: &ProviderContext<'_>) -> Self {
        Self {
            backend: Rc::new(backend),
            completions: ctx.completions().clone(),
        }
    }

    /// Register `shortcut`; `on_press` runs at an owner turn after each
    /// press, while the [`ShortcutRegistration`] the completion hands out is
    /// held.
    ///
    /// Dropping the completion before it settles unregisters the shortcut
    /// once the system answers.
    pub fn register(
        &self,
        shortcut: Shortcut,
        on_press: impl FnMut() + 'static,
    ) -> Completion<ShortcutResult> {
        let (completion, caller) = self.completions.pending();
        let active = Rc::new(Cell::new(true));
        let presses = ShortcutPresses {
            listener: Rc::new(RefCell::new(on_press)),
            active: Rc::clone(&active),
            completions: self.completions.clone(),
        };
        let (registered, answer) = self.completions.pending();
        let hold = self.backend.register(shortcut, presses, answer);
        let mut ending = Some((caller, hold));
        registered.on_complete(move |outcome| {
            let Some((caller, hold)) = ending.take() else {
                return;
            };
            match outcome {
                Ok(Ok(())) => {
                    let registration = ShortcutRegistration {
                        shortcut,
                        active: Rc::clone(&active),
                        hold: Some(hold),
                    };
                    // A caller that left drops the registration with the
                    // answer, which unregisters.
                    let _ = caller.resolve(Ok(registration));
                }
                Ok(Err(error)) => {
                    active.set(false);
                    drop(hold);
                    let _ = caller.resolve(Err(error));
                }
                // The backend dropped its answer: the caller's completion
                // ends as the backend's did.
                Err(_) => {
                    active.set(false);
                    drop(hold);
                    drop(caller);
                }
            }
        });
        completion
    }
}

impl fmt::Debug for GlobalShortcuts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GlobalShortcuts").finish_non_exhaustive()
    }
}

/// A registered global shortcut: registered while this value is held,
/// unregistered when it drops. A press reported before the drop and not yet
/// delivered runs nothing.
#[must_use = "dropping the registration unregisters the shortcut"]
pub struct ShortcutRegistration {
    shortcut: Shortcut,
    active: Rc<Cell<bool>>,
    hold: Option<ShortcutHold>,
}

impl ShortcutRegistration {
    /// The chord it registered.
    #[must_use]
    pub fn shortcut(&self) -> Shortcut {
        self.shortcut
    }
}

impl Drop for ShortcutRegistration {
    fn drop(&mut self) {
        self.active.set(false);
        drop(self.hold.take());
    }
}

impl fmt::Debug for ShortcutRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShortcutRegistration")
            .field("shortcut", &self.shortcut)
            .finish_non_exhaustive()
    }
}
