//! Progress and a count on the dock's entry, through the Unity launcher
//! entry signal (`com.canonical.Unity.LauncherEntry.Update`).
//!
//! No portal carries it: the application broadcasts the entry's state on the
//! session bus, naming its desktop entry (`application://<id>.desktop`,
//! ADR-0351), and a dock that listens (KDE Plasma's task manager, Dash to
//! Dock, Ubuntu Dock) draws it. Nothing answers, so the handle reports the
//! entry as unconfirmed. GNOME's own dash does not listen.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use flui_platform::{Unsupported, UnsupportedReason};
use zbus::Variant;

use super::{AppName, Portal, lock};
use crate::taskbar::{EntryState, Honoured, Progress, TaskbarBackend, TaskbarCapability};

const INTERFACE: &str = "com.canonical.Unity.LauncherEntry";

/// [`TaskbarBackend`] over the launcher entry signal.
pub(crate) struct LauncherEntry {
    portal: Rc<Portal>,
    /// `application://<id>.desktop`.
    uri: Arc<str>,
    /// The object path the signal is sent from, derived from the id.
    path: Arc<str>,
    /// The state the next send carries; taken by the send, so a burst of
    /// changes sends the latest once.
    latest: Arc<Mutex<Option<EntryState>>>,
}

impl LauncherEntry {
    /// The entry of the application `app_id` names.
    ///
    /// # Errors
    ///
    /// [`UnsupportedReason::NotDeclared`] for an application with no id: the
    /// signal names the desktop entry by it.
    pub(crate) fn try_new(
        portal: Rc<Portal>,
        app_id: Option<AppName>,
    ) -> Result<Self, Unsupported> {
        let app_id = app_id.ok_or(Unsupported::of::<TaskbarCapability>(
            UnsupportedReason::NotDeclared {
                what: "the application id ([app] id in flui.toml)",
            },
        ))?;
        let element = app_id
            .as_str()
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>();
        Ok(Self {
            portal,
            uri: format!("application://{}.desktop", app_id.as_str()).into(),
            path: format!("/com/canonical/unity/launcherentry/{element}").into(),
            latest: Arc::default(),
        })
    }
}

impl TaskbarBackend for LauncherEntry {
    fn show(&self, state: EntryState) {
        if self.portal.start().is_err() {
            // No thread to send from: nothing a caller of a signal can do.
            return;
        }
        *lock(&self.latest) = Some(state);
        let latest = Arc::clone(&self.latest);
        let uri = Arc::clone(&self.uri);
        let path = Arc::clone(&self.path);
        self.portal.spawn(move |bus| async move {
            let Some(state) = lock(&latest).take() else {
                // A send spawned later carried it already.
                return;
            };
            let Ok(connection) = bus.connection().await else {
                return;
            };
            let _ = connection
                .emit_signal(
                    None::<&str>,
                    &*path,
                    INTERFACE,
                    "Update",
                    &(&*uri, properties(state)),
                )
                .await;
        });
    }

    fn honours(&self) -> Honoured {
        Honoured::new(true, false, true, false)
    }
}

/// The signal's properties for `state`: every one, each time, so a dock that
/// starts listening late learns the whole state from the next change.
fn properties(state: EntryState) -> HashMap<&'static str, Variant<'static>> {
    let progress = match state.progress {
        Progress::Done(fraction) => Some(fraction.get()),
        // The signal has no indeterminate progress.
        Progress::Hidden | Progress::Indeterminate => None,
    };
    let count = state
        .count
        .map(|count| i64::try_from(count).unwrap_or(i64::MAX));
    HashMap::from([
        ("progress", Variant::from(progress.unwrap_or(0.0))),
        ("progress-visible", Variant::from(progress.is_some())),
        ("count", Variant::from(count.unwrap_or(0))),
        ("count-visible", Variant::from(count.is_some())),
    ])
}
