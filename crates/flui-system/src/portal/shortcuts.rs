//! Global shortcuts over the `GlobalShortcuts` portal.
//!
//! A binding is a portal `Session`: `CreateSession`, then `BindShortcuts`
//! attached to the acquiring widget's window, which the portal answers with
//! the triggers it chose; then the session's `Activated`, `Deactivated` and
//! `ShortcutsChanged` signals until the binding is dropped (the session is
//! closed), the portal closes the session (`Closed`), or the UI runtime
//! retires. Every object path is the one the portal will give, so each
//! subscription is in place before the call that could answer it.
//!
//! The portal is the only path inside a sandbox, and the only one a Wayland
//! desktop offers outside it; an X11 key grab would be a second path with
//! other rules (no user consent, no conflict resolution), so there is none.
//!
//! A portal that leaves the bus takes the session with it. The binding is
//! renewed on the portal that takes its place, which the bus starts on
//! demand, and hears `Bound` again. A failure that can clear by itself (the
//! portal not up yet, a bus timeout) is retried on [`RETRY`], at most
//! [`RETRIES`] times in a row, and then ends the binding with that failure;
//! nothing is polled.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use flui_platform::capability::{CompletionSource, PortalParent, ProviderContext, WindowRef};
use flui_platform::{Backoff, SystemFailure, UnsupportedReason};
use futures_util::StreamExt;
use futures_util::future::{Either, select};
use zbus::{MatchRule, MessageStream, OwnedVariant, Variant};

use super::request::{self, Response};
use super::{Bus, Desktop, Dismissal, Interface, Portal, PortalError, with_parent};
use crate::shortcuts::{
    BoundShortcut, Shortcut, ShortcutBackend, ShortcutError, ShortcutEvent, ShortcutFeed,
    ShortcutHold, ShortcutId,
};

const GLOBAL_SHORTCUTS: Interface = Interface::new("org.freedesktop.portal.GlobalShortcuts", 1);
const SESSION: &str = "org.freedesktop.portal.Session";

/// The waits before renewing a binding a failure that can clear ended:
/// 1 s, doubling to 16 s.
const RETRY: Backoff = Backoff::new(Duration::from_secs(1), Duration::from_secs(16));

/// The renewals in a row a binding tries before its failure ends it.
const RETRIES: u32 = 5;

/// [`ShortcutBackend`] over the desktop portal.
pub(crate) struct PortalShortcuts {
    portal: Rc<Portal>,
    /// The window the portal's question is attached to.
    window: Option<WindowRef>,
    completions: CompletionSource,
}

impl PortalShortcuts {
    pub(crate) fn new(portal: Rc<Portal>, ctx: &ProviderContext<'_>) -> Self {
        Self {
            portal,
            window: ctx.window(),
            completions: ctx.completions().clone(),
        }
    }
}

impl ShortcutBackend for PortalShortcuts {
    fn bind(&self, shortcuts: Vec<Shortcut>, feed: ShortcutFeed) -> ShortcutHold {
        if let Err(error) = self.portal.start() {
            feed.send(ShortcutEvent::Ended(shortcut_error(error)));
            return ShortcutHold::new(());
        }
        let (hold, dismissal) = Dismissal::pair();
        let portal = Rc::clone(&self.portal);
        with_parent(self.window.as_ref(), &self.completions, move |parent| {
            if dismissal.is_dismissed() {
                return;
            }
            let tokens = Tokens::new(portal.token());
            portal.spawn(move |bus| watch(bus, shortcuts, parent, feed, dismissal, tokens));
        });
        ShortcutHold::new(hold)
    }
}

/// The handle tokens of one binding's objects: its own token, then a count,
/// so a renewed session's objects are new paths.
struct Tokens {
    base: String,
    next: u64,
}

impl Tokens {
    fn new(base: String) -> Self {
        Self { base, next: 0 }
    }

    fn next(&mut self) -> String {
        // A u64 counted up once per call does not wrap.
        self.next += 1;
        format!("{}_{}", self.base, self.next)
    }
}

/// How one session of a binding ended.
enum SessionEnd {
    /// The binding was let go, or the UI runtime retired; the session is
    /// closed.
    Dismissed,
    /// The portal left the bus: the session is gone with it.
    PortalLeft,
    /// A failure that may clear by itself.
    Transient(ShortcutError),
    /// The binding is over.
    Final(ShortcutError),
}

impl From<PortalError> for SessionEnd {
    fn from(error: PortalError) -> Self {
        match error {
            PortalError::Unsupported(unsupported)
                if unsupported.reason == UnsupportedReason::Unavailable =>
            {
                Self::Transient(unsupported.into())
            }
            PortalError::Failed(SystemFailure::Bus) => Self::Transient(SystemFailure::Bus.into()),
            other => Self::Final(shortcut_error(other)),
        }
    }
}

/// Keep `shortcuts` bound until the binding is let go, renewing the session
/// when the portal restarts or a transient failure ended it.
async fn watch(
    bus: std::sync::Arc<Bus>,
    shortcuts: Vec<Shortcut>,
    parent: PortalParent,
    feed: ShortcutFeed,
    dismissal: Dismissal,
    mut tokens: Tokens,
) {
    // Only the first session asks with the window: a renewal answers no
    // question the user did not already answer.
    let mut parent = Some(parent);
    let mut misses = 0;
    loop {
        let end = session(&bus, &shortcuts, parent.take(), &feed, &dismissal, &mut tokens).await;
        match end {
            Ok(()) => misses = 0,
            Err(SessionEnd::Dismissed) => return,
            Err(SessionEnd::Final(error)) => {
                feed.send(ShortcutEvent::Ended(error));
                return;
            }
            Err(SessionEnd::PortalLeft) => misses = 0,
            Err(SessionEnd::Transient(error)) => {
                if misses >= RETRIES {
                    feed.send(ShortcutEvent::Ended(error));
                    return;
                }
                let waited = std::pin::pin!(bus.sleep(RETRY.delay(misses)));
                let ended = std::pin::pin!(ended(&dismissal, &bus));
                if let Either::Right(_) = select(waited, ended).await {
                    return;
                }
                misses += 1;
            }
        }
    }
}

/// Completes when the binding is let go or the UI runtime retires.
async fn ended(dismissal: &Dismissal, bus: &Bus) {
    let caller = std::pin::pin!(dismissal.dismissed());
    let owner = std::pin::pin!(bus.retired());
    let _ = select(caller, owner).await;
}

/// One session: create it, bind the shortcuts, and carry its signals into
/// `feed` until it ends. `Ok` is a session that was bound and then lost to
/// a departed portal, so the next one starts afresh.
async fn session(
    bus: &Bus,
    shortcuts: &[Shortcut],
    parent: Option<PortalParent>,
    feed: &ShortcutFeed,
    dismissal: &Dismissal,
    tokens: &mut Tokens,
) -> Result<(), SessionEnd> {
    let desktop = bus.desktop().await?;
    let connection = &desktop.connection;
    let session_token = tokens.next();
    let session_path = request::object_path(connection, "session", &session_token)?;
    // Subscribed before the session exists, so nothing it says is missed.
    let mut signals = subscribe(&desktop, GLOBAL_SHORTCUTS.name, None, request::PATH).await?;
    let mut closed = subscribe(&desktop, SESSION, Some("Closed"), &session_path).await?;
    let mut departures = request::subscribe_departures(connection).await?;

    let create = tokens.next();
    let options = HashMap::from([
        ("handle_token", Variant::from(create.clone())),
        ("session_handle_token", Variant::from(session_token)),
    ]);
    match request::call(
        &desktop,
        GLOBAL_SHORTCUTS,
        "CreateSession",
        &create,
        &(options,),
        Some(dismissal),
    )
    .await?
    {
        Response::Answered(results) => {
            // The handle is the path asked for; a portal that names another
            // is outside its contract, and its signals would not be heard.
            let handle = results
                .get("session_handle")
                .and_then(|handle| String::try_from(handle.clone()).ok());
            if handle.as_deref() != Some(session_path.as_str()) {
                close(&desktop, &session_path).await;
                return Err(SessionEnd::Final(SystemFailure::MalformedAnswer.into()));
            }
        }
        Response::Cancelled => return Err(SessionEnd::Final(ShortcutError::Denied)),
        Response::Ended(code) => {
            return Err(SessionEnd::Final(SystemFailure::PortalResponse(code).into()));
        }
        Response::Dismissed => return Err(SessionEnd::Dismissed),
    }

    let bind = tokens.next();
    let parent_window = parent.as_ref().map_or("", PortalParent::as_str).to_owned();
    let session_object =
        zbus::ObjectPath::try_from(session_path.as_str())
            .map_err(|error| PortalError::from(zbus::Error::from(error)))?;
    let body = (
        session_object,
        requested(shortcuts),
        parent_window,
        HashMap::from([("handle_token", Variant::from(bind.clone()))]),
    );
    let bound = request::call(
        &desktop,
        GLOBAL_SHORTCUTS,
        "BindShortcuts",
        &bind,
        &body,
        Some(dismissal),
    )
    .await;
    // The window's export lasts until the request that names it ended.
    drop(parent);
    let failure = match bound {
        Ok(Response::Answered(results)) => match listed(results.get("shortcuts")) {
            Ok(list) => {
                feed.send(ShortcutEvent::Bound(list));
                None
            }
            Err(failure) => Some(SessionEnd::Final(failure.into())),
        },
        Ok(Response::Cancelled) => Some(SessionEnd::Final(ShortcutError::Denied)),
        Ok(Response::Ended(code)) => Some(SessionEnd::Final(
            SystemFailure::PortalResponse(code).into(),
        )),
        Ok(Response::Dismissed) => Some(SessionEnd::Dismissed),
        Err(error) => Some(SessionEnd::from(error)),
    };
    if let Some(failure) = failure {
        close(&desktop, &session_path).await;
        return Err(failure);
    }

    loop {
        let signal = signals.next();
        let session_closed = closed.next();
        let departed = std::pin::pin!(request::departure(&mut departures, desktop.owner.as_str()));
        let let_go = std::pin::pin!(ended(dismissal, bus));
        let heard = select(select(signal, session_closed), select(departed, let_go)).await;
        match heard {
            Either::Left((Either::Left((Some(Ok(message)), _)), _)) => {
                if let Some(event) = event(&message, &session_path) {
                    feed.send(event);
                }
            }
            // A stream ends only with its connection.
            Either::Left((Either::Left((_, _)), _)) => return Ok(()),
            Either::Left((Either::Right((Some(Ok(message)), _)), _))
                if message.header().sender().map(|sender| sender.as_str())
                    == Some(desktop.owner.as_str()) =>
            {
                return Err(SessionEnd::Final(ShortcutError::Revoked));
            }
            Either::Left((Either::Right(_), _)) => {}
            Either::Right((Either::Left(_), _)) => return Err(SessionEnd::PortalLeft),
            Either::Right((Either::Right(_), _)) => {
                close(&desktop, &session_path).await;
                return Err(SessionEnd::Dismissed);
            }
        }
    }
}

/// The signals `owner` sends of `interface` (one `member`, or every one) on
/// `path`. The sender is checked by the bus's rule and again by each reader.
async fn subscribe(
    desktop: &Desktop,
    interface: &'static str,
    member: Option<&'static str>,
    path: &str,
) -> Result<MessageStream, PortalError> {
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(desktop.owner.as_str())
        .interface(interface)
        .path(path);
    let rule = match member {
        Some(member) => rule.member(member),
        None => rule,
    };
    Ok(MessageStream::for_match_rule(rule.build()?, &desktop.connection, None).await?)
}

/// Close the session at `path`: the portal unbinds its shortcuts. A session
/// the portal closed meanwhile answers an error nobody needs.
async fn close(desktop: &Desktop, path: &str) {
    let _ = desktop
        .connection
        .call_method(Some(desktop.owner.as_str()), path, Some(SESSION), "Close", &())
        .await;
}

/// `shortcuts` as `BindShortcuts` takes them (`a(sa{sv})`).
fn requested(shortcuts: &[Shortcut]) -> Vec<(String, HashMap<&'static str, Variant<'static>>)> {
    shortcuts
        .iter()
        .map(|shortcut| {
            let mut options =
                HashMap::from([("description", Variant::from(shortcut.description.clone()))]);
            if let Some(trigger) = &shortcut.preferred {
                options.insert("preferred_trigger", Variant::from(trigger.to_xdg()));
            }
            (shortcut.id.as_str().to_owned(), options)
        })
        .collect()
}

/// The shortcuts a result lists (`a(sa{sv})`), each with the trigger the
/// portal shows; an entry whose id is not one is left out, as no request
/// named it.
fn listed(shortcuts: Option<&OwnedVariant>) -> Result<Vec<BoundShortcut>, SystemFailure> {
    let Some(shortcuts) = shortcuts else {
        return Ok(Vec::new());
    };
    let entries =
        Vec::<Entry>::try_from(shortcuts.clone()).map_err(|_| SystemFailure::MalformedAnswer)?;
    Ok(bound(entries))
}

/// One shortcut as the portal lists it: its id and its details.
type Entry = (String, HashMap<String, OwnedVariant>);

/// The shortcuts `entries` list, each with the trigger the portal shows.
fn bound(entries: Vec<Entry>) -> Vec<BoundShortcut> {
    entries
        .into_iter()
        .filter_map(|(id, details)| {
            let id = ShortcutId::try_new(&id)?;
            let trigger = details
                .get("trigger_description")
                .and_then(|trigger| String::try_from(trigger.clone()).ok())
                .filter(|trigger| !trigger.is_empty());
            Some(BoundShortcut::new(id, trigger))
        })
        .collect()
}

/// What a `GlobalShortcuts` signal of the session at `session` says: a press,
/// a release or new triggers; `None` for another session's, a malformed one,
/// or one that names no shortcut id.
fn event(message: &zbus::Message, session: &str) -> Option<ShortcutEvent> {
    let header = message.header();
    let member = header.member()?.as_str().to_owned();
    let body = message.body();
    match member.as_str() {
        "Activated" | "Deactivated" => {
            let (handle, id, _timestamp, _options) = body
                .deserialize::<(zbus::ObjectPath<'_>, &str, u64, HashMap<&str, OwnedVariant>)>()
                .ok()?;
            if handle.as_str() != session {
                return None;
            }
            let id = ShortcutId::try_new(id)?;
            Some(if member == "Activated" {
                ShortcutEvent::Pressed(id)
            } else {
                ShortcutEvent::Released(id)
            })
        }
        "ShortcutsChanged" => {
            let (handle, entries) = body
                .deserialize::<(zbus::ObjectPath<'_>, Vec<Entry>)>()
                .ok()?;
            (handle.as_str() == session).then(|| ShortcutEvent::Changed(bound(entries)))
        }
        _ => None,
    }
}

fn shortcut_error(error: PortalError) -> ShortcutError {
    match error {
        PortalError::Unsupported(unsupported) => ShortcutError::Unsupported(unsupported),
        PortalError::Failed(failure) => ShortcutError::Failed(failure),
    }
}
