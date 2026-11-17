//! Progress and a count on the dock's entry, through the Unity launcher
//! entry signal (`com.canonical.Unity.LauncherEntry.Update`).
//!
//! No portal carries it: the application broadcasts the entry's state on the
//! session bus, naming its desktop entry (`application://<id>.desktop`,
//! ADR-0351), and a dock that listens (KDE Plasma's task manager, Dash to
//! Dock, Ubuntu Dock) draws it. Nothing answers, so the handle reports the
//! entry as unconfirmed. GNOME's own dash does not listen.
//!
//! One task per UI runtime sends the signal ([`EntryFeed`]): it takes the
//! newest state each time it is free, so the dock ends on the last state set
//! however a burst of changes and a slow connection interleave.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use flui_platform::{Unsupported, UnsupportedReason};
use futures_util::future::select;
use zbus::Variant;

use super::{AppName, Bus, Portal, lock};
use crate::taskbar::{EntryState, Honoured, Progress, TaskbarBackend, TaskbarCapability};

const INTERFACE: &str = "com.canonical.Unity.LauncherEntry";

/// What one UI runtime's entry sends, shared by its handles and the one task
/// that sends: the newest state not sent yet, and the wake for that task.
#[derive(Default)]
pub(crate) struct EntryFeed {
    latest: Mutex<Option<EntryState>>,
    wake: zruntime::Event,
    /// Whether the sending task was started.
    sending: AtomicBool,
}

/// One UI runtime's [`EntryFeed`], which every handle of the runtime sends
/// through (a runtime-local value).
#[derive(Default)]
pub(crate) struct EntryFeedSlot(pub(crate) Arc<EntryFeed>);

/// [`TaskbarBackend`] over the launcher entry signal.
pub(crate) struct LauncherEntry {
    portal: Rc<Portal>,
    /// `application://<id>.desktop`.
    uri: Arc<str>,
    /// The object path the signal is sent from, derived from the id.
    path: Arc<str>,
    feed: Arc<EntryFeed>,
}

impl LauncherEntry {
    /// The entry of the application `app_id` names, sending through `feed`,
    /// its runtime's.
    ///
    /// # Errors
    ///
    /// [`UnsupportedReason::NotDeclared`] for an application with no id: the
    /// signal names the desktop entry by it.
    pub(crate) fn try_new(
        portal: Rc<Portal>,
        app_id: Option<AppName>,
        feed: Arc<EntryFeed>,
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
            feed,
        })
    }
}

impl TaskbarBackend for LauncherEntry {
    fn show(&self, state: EntryState) {
        if self.portal.start().is_err() {
            // No thread to send from: nothing a caller of a signal can do.
            return;
        }
        *lock(&self.feed.latest) = Some(state);
        self.feed.wake.notify(1);
        if !self.feed.sending.swap(true, Ordering::AcqRel) {
            let feed = Arc::clone(&self.feed);
            let uri = Arc::clone(&self.uri);
            let path = Arc::clone(&self.path);
            self.portal
                .spawn(move |bus| async move { send(&bus, &feed, &uri, &path).await });
        }
    }

    fn honours(&self) -> Honoured {
        Honoured::new(true, false, true, false)
    }
}

/// Send the newest state of `feed` each time there is one, until the UI
/// runtime retires: one sender, so no older state is sent after a newer one.
async fn send(bus: &Bus, feed: &EntryFeed, uri: &str, path: &str) {
    loop {
        // Listen before taking, so a state set meanwhile wakes the next turn.
        let woken = feed.wake.listen();
        let next = lock(&feed.latest).take();
        let Some(state) = next else {
            let woken = std::pin::pin!(woken);
            let retired = std::pin::pin!(bus.retired());
            if let futures_util::future::Either::Right(_) = select(woken, retired).await {
                return;
            }
            continue;
        };
        if let Ok(connection) = bus.connection().await {
            // A signal nobody answers; a failure to send has no one to tell.
            let _ = connection
                .emit_signal(
                    None::<&str>,
                    path,
                    INTERFACE,
                    "Update",
                    &(uri, properties(state)),
                )
                .await;
        }
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
