//! The app's font registration door, and the notice every realm gets when
//! the app's fonts change (ADR-0092 §2, §7).
//!
//! A face registered here goes into the app's one `FontCollection`, which
//! every realm measures, paints and places carets with. The host's faces
//! reach the same collection from the feed the runtime started off the owner
//! thread. Either change raises the collection's generation, and every realm
//! the runtime hosts is told so on an owner turn ([`announce_font_change`]):
//! its next frame lays out again the text measured before the change.

use flui_painting::RegisterFontError;

use super::host::APP_RUNTIME;
use super::realm_dispatch::{RealmDispatcher, RealmTask, dispatch_platform_realm};

/// Why [`register_font`] refused a font.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FontRegistrationError {
    /// The bytes hold no face.
    #[error(transparent)]
    Font(#[from] RegisterFontError),
    /// The same bytes were registered before. A face is never added twice,
    /// so a repeated call changes nothing and lays nothing out again.
    #[error("these font bytes are already registered")]
    AlreadyRegistered,
    /// Called while the app's runtime was borrowed on this thread, from code
    /// the runtime itself was running (a drop or callback inside one of its
    /// own operations). Nothing was registered; call again from app code.
    #[error("the app runtime is busy on this thread")]
    RuntimeBusy,
}

/// Registers every face in `font_bytes` with the app, for measurement, paint
/// and carets alike.
///
/// Call on the thread that runs the app, before it starts or while it runs
/// (from a widget callback, for example). The face is added to the app's
/// font collection, which measures, paints and places carets in text, and
/// every realm lays out again, on its next frame, each piece of text it had
/// measured; text laid out later measures with the face from the start.
/// A realm is told on its own owner turn, so a call made from inside one
/// realm's callback reaches that realm after the callback returns.
///
/// Faces are never removed. Called before the app starts, it checks the
/// bytes and holds them; the first window registers them as it builds the
/// app's font collection, and measures and paints with them from its first
/// frame.
///
/// The app's fonts belong to the thread that runs it. Called on another
/// thread (a worker that downloaded the bytes, say), the bytes are held for
/// an app that thread never runs: neither paint nor measurement gains the
/// face, so the two never disagree, and no scan of the host's fonts runs
/// there. Send the bytes to the app's thread and register them there.
///
/// # Errors
///
/// - [`FontRegistrationError::Font`] if the bytes hold no face; nothing is
///   added.
/// - [`FontRegistrationError::AlreadyRegistered`] if the same bytes were
///   registered before; nothing changes.
/// - [`FontRegistrationError::RuntimeBusy`] if the runtime is borrowed on
///   this thread; nothing is added.
pub fn register_font(font_bytes: &[u8]) -> Result<(), FontRegistrationError> {
    APP_RUNTIME.with(|slot| {
        let runtime = slot
            .try_borrow()
            .map_err(|_| FontRegistrationError::RuntimeBusy)?;
        // Held for the first realm when the services are not resolved yet:
        // whichever realm comes first is built over the collection and
        // measures with the face from the start, and nothing is announced.
        runtime.register_font(font_bytes)
    })?;
    announce_font_change();
    Ok(())
}

/// Tells every realm the runtime hosts that the app's fonts changed, if they
/// did since the last notice: a registration, or the host feed landing.
///
/// Called after a registration and at the start of every top-level owner
/// turn, where the host feed's wake leads. Each realm gets
/// [`UiRealm::fonts_changed`](crate::app::ui_realm::UiRealm::fonts_changed)
/// on its own owner turn: a realm that is idle runs it at once, one that is
/// running (a registration from inside its own callback) after its task
/// returns. With the runtime borrowed, nothing is taken, and the next turn
/// announces the change.
pub(super) fn announce_font_change() {
    let realms = APP_RUNTIME.with(|slot| {
        let Ok(runtime) = slot.try_borrow() else {
            return Vec::new();
        };
        if !runtime.take_font_change() {
            return Vec::new();
        }
        // No owner thread means no realm installed: a realm built later is
        // built over the collection as it is then.
        let Some(owner_thread) = runtime.owner_thread else {
            return Vec::new();
        };
        runtime
            .realms
            .iter()
            .map(|(_, realm)| RealmDispatcher {
                owner_thread,
                address: realm.address,
            })
            .collect::<Vec<_>>()
    });
    for dispatcher in realms {
        // Outside the runtime borrow: a realm that is idle runs the notice
        // now, one that is checked out (the caller's own) gets it queued
        // behind the running turn.
        if let Err(error) = dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(crate::app::ui_realm::UiRealm::fonts_changed)),
        ) {
            // A realm closing or gone needs no layout.
            tracing::debug!(?dispatcher, ?error, "font change notice not delivered");
        }
    }
}
