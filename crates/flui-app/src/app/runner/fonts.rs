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

/// Registers every face in `font_bytes` with the app, for measurement and
/// paint alike.
///
/// Call on the thread that runs the app, before it starts or while it runs
/// (from a widget callback, for example). The face is added to the app's
/// font collection and to the process font system glyphs are painted from,
/// and every realm lays out again, on its next frame, each piece of text it
/// had measured; text laid out later measures with the face from the start.
/// A realm is told on its own owner turn, so a call made from inside one
/// realm's callback reaches that realm after the callback returns.
///
/// Faces are never removed. Called before the app starts, it builds the
/// app's font collection then, and with it the scan of the host's fonts the
/// first window would otherwise pay for.
///
/// On any other thread it registers with a runtime of that thread's own,
/// which no window of the app reads: the face reaches paint and not the
/// app's measurement.
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
        runtime.register_font(font_bytes)?;
        // No owner thread means no realm was installed yet: the first one is
        // built over the collection and measures with the face from the
        // start.
        let Some(owner_thread) = runtime.owner_thread else {
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
