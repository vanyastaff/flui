//! The RemoteDesktop portal (`org.freedesktop.portal.RemoteDesktop`, version 2 and later), asked
//! for a session with a keyboard and a pointer and for the EIS socket of that session.
//!
//! Each step is a request whose answer comes as the `Response` signal on the request's object,
//! which is subscribed to before the call, so an answer cannot arrive unheard. A refused or ended
//! step is a lane the host cannot run ([`mapping::portal_response`]). The portal's session lives
//! as long as the connection that made it, which the lane keeps for as long as it sends.

use std::collections::HashMap;
use std::os::fd::OwnedFd;

use atspi::zbus;
use futures_lite::StreamExt as _;
use futures_lite::future::block_on;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use super::mapping::{self, LANE};
use crate::error::{Error, Result};

const DESTINATION: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const INTERFACE: &str = "org.freedesktop.portal.RemoteDesktop";

/// The device types a session asks for: the keyboard and the pointer.
const DEVICES: u32 = 1 | 2;

/// How long a step waits for the portal's answer, which may wait on a dialog a person answers.
const ANSWER_WITHIN: std::time::Duration = std::time::Duration::from_secs(60);

fn unavailable(message: String) -> Error {
    Error::Lane {
        lane: LANE.to_owned(),
        unavailable: true,
        message,
    }
}

/// A RemoteDesktop session and its EIS socket.
pub(super) struct PortalSession {
    /// The connection the session lives as long as.
    _connection: zbus::Connection,
    /// The session's EIS socket.
    pub(super) eis: OwnedFd,
}

/// Ask the portal on the session bus at `bus` (the process's own when `None`) for a session with
/// a keyboard and a pointer, started, and its EIS socket.
///
/// # Errors
///
/// [`Error::Lane`] with `unavailable` when there is no portal, it is older than version 2, or a
/// step was refused or ended.
pub(super) fn open(bus: Option<&str>) -> Result<PortalSession> {
    block_on(async {
        let builder = match bus {
            Some(address) => zbus::connection::Builder::address(address),
            None => zbus::connection::Builder::session(),
        }
        .map_err(|error| unavailable(format!("no session bus: {error}")))?;
        let connection = builder
            .build()
            .await
            .map_err(|error| unavailable(format!("no session bus: {error}")))?;
        let sender = connection
            .unique_name()
            .map(|name| name.trim_start_matches(':').replace('.', "_"))
            .ok_or_else(|| unavailable("the session bus gave no name".to_owned()))?;
        let mut tokens = 0_u32;
        let mut token = || {
            tokens += 1;
            format!("flui_driver_{}_{tokens}", std::process::id())
        };

        let create = token();
        let results = request(
            &connection,
            &sender,
            &create,
            "creating a session",
            |options| {
                let session_token = format!("{create}_session");
                options.insert("session_handle_token", Value::from(session_token));
                ("CreateSession", None)
            },
        )
        .await?;
        let session: OwnedObjectPath = results
            .get("session_handle")
            .and_then(|value| String::try_from(value.clone()).ok())
            .and_then(|path| OwnedObjectPath::try_from(path).ok())
            .ok_or_else(|| unavailable("the portal named no session".to_owned()))?;

        let select = token();
        request(
            &connection,
            &sender,
            &select,
            "choosing devices",
            |options| {
                options.insert("types", Value::from(DEVICES));
                ("SelectDevices", Some(session.clone()))
            },
        )
        .await?;

        let start = token();
        request(&connection, &sender, &start, "start", |_| {
            ("Start", Some(session.clone()))
        })
        .await?;

        let options: HashMap<&str, Value<'_>> = HashMap::new();
        let reply = connection
            .call_method(
                Some(DESTINATION),
                PATH,
                Some(INTERFACE),
                "ConnectToEIS",
                &(&session, options),
            )
            .await
            .map_err(|error| {
                unavailable(format!(
                    "the RemoteDesktop portal gave no EIS socket (version 2 has it): {error}"
                ))
            })?;
        let eis: zbus::zvariant::OwnedFd = reply
            .body()
            .deserialize()
            .map_err(|error| unavailable(format!("the portal's EIS socket: {error}")))?;
        Ok(PortalSession {
            _connection: connection,
            eis: OwnedFd::from(eis),
        })
    })
}

/// Make the request `call` names (its method, and the session it is for), with `handle_token`
/// set to `token` and the options `call` adds, and wait for its `Response`: the results, or why
/// the step `step` did not go through.
async fn request(
    connection: &zbus::Connection,
    sender: &str,
    token: &str,
    step: &str,
    call: impl FnOnce(
        &mut HashMap<&'static str, Value<'static>>,
    ) -> (&'static str, Option<OwnedObjectPath>),
) -> Result<HashMap<String, OwnedValue>> {
    let handle = format!("{PATH}/request/{sender}/{token}");
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.portal.Request")
        .and_then(|rule| rule.member("Response"))
        .and_then(|rule| rule.path(handle.as_str()))
        .map_err(|error| unavailable(format!("a portal request: {error}")))?
        .build();
    let mut responses = zbus::MessageStream::for_match_rule(rule, connection, None)
        .await
        .map_err(|error| unavailable(format!("a portal request: {error}")))?;
    let mut options: HashMap<&'static str, Value<'static>> = HashMap::new();
    options.insert("handle_token", Value::from(token.to_owned()));
    let (method, session) = call(&mut options);
    let sent = match session {
        Some(session) if method == "Start" => {
            connection
                .call_method(
                    Some(DESTINATION),
                    PATH,
                    Some(INTERFACE),
                    method,
                    &(&session, "", options),
                )
                .await
        }
        Some(session) => {
            connection
                .call_method(
                    Some(DESTINATION),
                    PATH,
                    Some(INTERFACE),
                    method,
                    &(&session, options),
                )
                .await
        }
        None => {
            connection
                .call_method(
                    Some(DESTINATION),
                    PATH,
                    Some(INTERFACE),
                    method,
                    &(options,),
                )
                .await
        }
    };
    sent.map_err(|error| {
        unavailable(format!(
            "no RemoteDesktop portal answers {method} on the session bus: {error}"
        ))
    })?;
    let answered = futures_lite::future::or(async { responses.next().await.map(Some) }, async {
        async_io::Timer::after(ANSWER_WITHIN).await;
        Some(None)
    })
    .await;
    let message = match answered {
        Some(Some(message)) => message,
        Some(None) => {
            return Err(unavailable(format!(
                "the portal did not answer {method} within {} s: its dialog may wait for a \
                 person",
                ANSWER_WITHIN.as_secs()
            )));
        }
        None => return Err(unavailable(format!("the portal never answered {method}"))),
    }
    .map_err(|error| unavailable(format!("the portal's answer to {method}: {error}")))?;
    let (response, results): (u32, HashMap<String, OwnedValue>) = message
        .body()
        .deserialize()
        .map_err(|error| unavailable(format!("the portal's answer to {method}: {error}")))?;
    mapping::portal_response(response, step)?;
    Ok(results)
}
