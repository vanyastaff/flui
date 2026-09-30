//! The app's font registration door (ADR-0092 §2, §10 step 3b).
//!
//! A face registered here goes into the app's one `FontCollection`, which
//! loads it into the process font system paint shapes with, and every realm
//! the runtime hosts is told so on its owner turn: its next frame lays out
//! again the text measured before the face existed.

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
/// font collection, which measures and paints text, and to the process font
/// system carets are laid out in, and every realm lays out again, on its next frame, each piece of text it
/// had measured; text laid out later measures with the face from the start.
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
    let realms = APP_RUNTIME.with(|slot| {
        let runtime = slot
            .try_borrow()
            .map_err(|_| FontRegistrationError::RuntimeBusy)?;
        // Held for the first realm, or no owner thread (no realm installed
        // yet): whichever realm comes first is built over the collection
        // and measures with the face from the start.
        let registered_now = runtime.register_font(font_bytes)?;
        let (true, Some(owner_thread)) = (registered_now, runtime.owner_thread) else {
            return Ok(Vec::new());
        };
        Ok::<_, FontRegistrationError>(
            runtime
                .realms
                .iter()
                .map(|(_, realm)| RealmDispatcher {
                    owner_thread,
                    address: realm.address,
                })
                .collect::<Vec<_>>(),
        )
    })?;
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
    Ok(())
}
