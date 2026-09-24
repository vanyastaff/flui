//! The one error type every tool reports, and the envelope it goes out in.
//!
//! An agent branches on [`ToolError::code`] and [`ToolError::retry`], not on
//! the wording: the codes and the fields each carries are the contract; the
//! message is for reading, and names the argument, element or window
//! involved and, where there is one, the next step that would succeed. An
//! error raised after part of an action reached the OS or the application
//! carries an [`Effect`], so a retry is a decision, not a guess.

use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;

/// What a session handle names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HandleKind {
    /// An element id (`e12`).
    Element,
    /// A window id (`w3`).
    Window,
    /// A screenshot id (`s2`).
    Screenshot,
    /// A pid this session handed out.
    Process,
}

impl std::fmt::Display for HandleKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Element => "element",
            Self::Window => "window",
            Self::Screenshot => "screenshot",
            Self::Process => "process",
        })
    }
}

/// What an action left behind before it failed: what a retry must know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// `sent` of `total` `unit` (characters, presses, clicks, scroll axes)
    /// went out; a retry from the start repeats them.
    Partial {
        /// How many went out.
        sent: usize,
        /// How many the call asked for.
        total: usize,
        /// What is counted.
        unit: &'static str,
    },
    /// The action reached the application and may have run.
    MayHaveRun,
    /// The action went out; what came after it (reading it back, a release)
    /// failed.
    Ran,
    /// Not the action, but something else went out: modifiers tapped on
    /// their own, input held from an earlier call released now.
    Incidental,
}

impl Effect {
    fn kind(&self) -> &'static str {
        match self {
            Self::Partial { .. } => "partial",
            Self::MayHaveRun => "may_have_run",
            Self::Ran => "ran",
            Self::Incidental => "incidental",
        }
    }
}

/// A window named in an error: its session handle when this session issued
/// one, and what the OS says about it either way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct WindowRef {
    /// The window's handle (`w3`), when this session has issued one for it.
    pub window: Option<String>,
    /// The owning process.
    pub pid: u32,
    /// The title, when readable.
    pub title: Option<String>,
}

impl std::fmt::Display for WindowRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.window {
            Some(w) => write!(f, "window {w} of process {}", self.pid)?,
            None => write!(f, "an unlisted window of process {}", self.pid)?,
        }
        if let Some(t) = &self.title {
            write!(f, " {t:?}")?;
        }
        Ok(())
    }
}

fn window_or_none(w: Option<&WindowRef>) -> String {
    w.map_or_else(|| "none".to_owned(), ToString::to_string)
}

/// Whether the same call can succeed later with nothing changed on the
/// agent's side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Retry {
    /// Final: change the arguments, read again, or give up.
    Never,
    /// Something passing got in the way; the same call can succeed in a
    /// moment.
    Soon,
    /// Nothing matches now; the same call succeeds once something does.
    WhenAppears,
}

/// Why a tool call did not do what it was asked.
#[derive(Debug, Error)]
pub enum ToolError {
    /// The arguments are malformed or contradict each other.
    #[error("invalid arguments: {0}")]
    InvalidArgument(String),

    /// The feature has no backend on this OS.
    #[error("{0}")]
    NotSupported(String),

    /// Nothing matches the target now: a process with no window yet, a
    /// monitor index past the last one.
    #[error("{0}")]
    NotFound(String),

    /// The handle was never issued in this session.
    #[error("unknown {kind} `{handle}`: {}", kind.source())]
    UnknownHandle {
        /// The handle as given.
        handle: String,
        /// What it would name.
        kind: HandleKind,
    },

    /// The handle was issued, but what it named is gone or has changed
    /// hands: a removed element, a closed window, an exited process. A
    /// handle is never re-bound, so this is final.
    #[error("{kind} `{handle}` is gone: {why}; {}", kind.again())]
    Gone {
        /// The handle as given.
        handle: String,
        /// What it named.
        kind: HandleKind,
        /// What happened to it.
        why: String,
    },

    /// The element is disabled, so the action was not performed.
    #[error("element `{element}` is disabled; {action} was not performed")]
    Disabled {
        /// The element handle.
        element: String,
        /// The action asked for.
        action: &'static str,
    },

    /// The element does not offer the action.
    #[error(
        "element `{element}` does not support {action}; its actions: [{}]{}",
        supported.join(", "),
        if unread.is_empty() { String::new() } else { format!(" (could not read: {})", unread.join(", ")) }
    )]
    ActionUnsupported {
        /// The element handle.
        element: String,
        /// The action asked for, as the tool is named.
        action: &'static str,
        /// The actions it does offer, as the tools are named.
        supported: Vec<&'static str>,
        /// The actions whose availability could not be read.
        unread: Vec<&'static str>,
    },

    /// Input was refused because the target window is not in front.
    #[error(
        "refused: the target {target} is not the foreground window (foreground: {}); call activate_window first",
        window_or_none(foreground.as_ref())
    )]
    NotForeground {
        /// The window the caller wanted the input to reach.
        target: String,
        /// The window that would actually have received it, if any.
        foreground: Option<WindowRef>,
    },

    /// The target is in front, but keyboard focus inside it is in another
    /// process's window (an embedded browser or preview pane), where keys
    /// would go. Activating the window again does not move it.
    #[error(
        "refused: keyboard focus in {target} is in a window of process {holder} (an embedded panel), so keys would go there; move focus to a control of the target's own outside that panel (focus or click one); if the panel fills the window, keys cannot be sent to it with a safety target"
    )]
    FocusElsewhere {
        /// The window the caller wanted the input to reach.
        target: String,
        /// The process whose window holds keyboard focus.
        holder: u32,
    },

    /// A coordinate input would not land on the target.
    #[error("refused: point ({x}, {y}) is not on the target: {reason}; nothing was sent there")]
    OutsideTarget {
        /// Point x in screen coordinates.
        x: i32,
        /// Point y in screen coordinates.
        y: i32,
        /// What is there instead.
        reason: String,
        /// The window covering the point, when one does.
        covered_by: Option<WindowRef>,
    },

    /// A wait ran out before the condition held.
    #[error("timed out after {timeout_ms} ms waiting for {what}; last tree seen:\n{summary}")]
    Timeout {
        /// The wait's budget.
        timeout_ms: u64,
        /// The awaited condition, formatted.
        what: String,
        /// A short summary of the last tree the wait read.
        summary: String,
    },

    /// An action failed after part of it reached the OS or the application:
    /// `effect` says what, and `detail` how, so a retry does not repeat it.
    #[error("{cause}; {detail}")]
    Interrupted {
        /// Why it stopped.
        cause: Box<ToolError>,
        /// What had already happened.
        effect: Effect,
        /// The particulars: where a drop landed, what may still be held.
        detail: String,
    },

    /// Something passing got in the way (a full queue, a window that moved
    /// during the call); the same call can succeed moments later.
    #[error("{0}; retry shortly")]
    Busy(String),

    /// A key or button from an earlier call is still down and could not be
    /// released; nothing is sent until it is. The next call tries again.
    #[error("{0} still held from an earlier failed release; no input is sent until released")]
    InputHeld(String),

    /// The server is shutting down: the call was skipped, or the action in
    /// progress stopped at its next event.
    #[error("the server is shutting down; the call was not run, or stopped at its next event")]
    ShuttingDown,

    /// The client cancelled the request before it ran; nothing was done.
    /// Never seen by the client: it dropped the reply.
    #[error("cancelled by the client before it ran; nothing was done")]
    Cancelled,

    /// An OS API call failed.
    #[error("{context}: {message}")]
    Platform {
        /// What the server was doing.
        context: String,
        /// The OS or library message.
        message: String,
    },
}

impl HandleKind {
    /// Where handles of this kind come from.
    fn source(self) -> &'static str {
        match self {
            Self::Element => {
                "element ids come from accessibility_tree, find or wait_for in this session"
            }
            Self::Window => "window ids come from list_windows or wait_for_window in this session",
            Self::Screenshot => "screenshot ids come from screenshot in this session",
            Self::Process => "pids come from list_windows or launch in this session",
        }
    }

    /// How to get a fresh handle of this kind.
    fn again(self) -> &'static str {
        match self {
            Self::Element => "read the tree again for a fresh id",
            Self::Window => "list windows again for a fresh id",
            Self::Screenshot => "take a new screenshot",
            Self::Process => {
                "list windows again, or launch again, for the process that is there now"
            }
        }
    }
}

impl ToolError {
    /// An OS or library failure while doing `context`.
    pub fn platform(context: impl Into<String>, err: impl std::fmt::Display) -> Self {
        Self::Platform {
            context: context.into(),
            message: err.to_string(),
        }
    }

    /// An element handle whose element is gone.
    pub fn gone_element(handle: &str, why: impl Into<String>) -> Self {
        Self::Gone {
            handle: handle.to_owned(),
            kind: HandleKind::Element,
            why: why.into(),
        }
    }

    /// This error, after `effect` already happened. An error that already
    /// carries an effect keeps it (what happened first is what a retry must
    /// know) and gains the new detail.
    #[must_use]
    pub fn after(self, effect: Effect, detail: impl Into<String>) -> Self {
        match self {
            Self::Interrupted {
                cause,
                effect: first,
                detail: earlier,
            } => Self::Interrupted {
                cause,
                effect: first,
                detail: format!("{earlier}; {}", detail.into()),
            },
            cause => Self::Interrupted {
                cause: Box::new(cause),
                effect,
                detail: detail.into(),
            },
        }
    }

    /// The code an agent branches on: fixed, one per variant, with the
    /// cause's for an interrupted action.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidArgument(_) => "invalid_argument",
            Self::NotSupported(_) => "not_supported",
            Self::Busy(_) => "busy",
            Self::NotFound(_) => "not_found",
            Self::UnknownHandle { .. } => "unknown_handle",
            Self::Gone { .. } => "gone",
            Self::Disabled { .. } => "disabled",
            Self::ActionUnsupported { .. } => "action_unsupported",
            Self::NotForeground { .. } => "not_foreground",
            Self::FocusElsewhere { .. } => "focus_elsewhere",
            Self::OutsideTarget { .. } => "outside_target",
            Self::Timeout { .. } => "timeout",
            Self::InputHeld(_) => "input_held",
            Self::ShuttingDown => "shutting_down",
            Self::Cancelled => "cancelled",
            Self::Platform { .. } => "platform",
            Self::Interrupted { cause, .. } => cause.code(),
        }
    }

    /// Whether the same call can succeed later. An interrupted action is
    /// never retried blindly: part of it went out.
    pub fn retry(&self) -> Retry {
        match self {
            Self::Busy(_) | Self::InputHeld(_) => Retry::Soon,
            Self::NotFound(_) => Retry::WhenAppears,
            _ => Retry::Never,
        }
    }

    /// The structured content of the failed call: `{"error": {...}}` with
    /// the code, the message, the retry policy, the variant's fields and
    /// the effect of an interrupted action.
    pub fn payload(&self) -> Value {
        let (inner, effect) = match self {
            Self::Interrupted {
                cause,
                effect,
                detail,
            } => {
                let mut effect_json = json!({ "kind": effect.kind(), "detail": detail });
                if let Effect::Partial { sent, total, unit } = effect {
                    effect_json["sent"] = json!(sent);
                    effect_json["total"] = json!(total);
                    effect_json["unit"] = json!(unit);
                }
                (cause.as_ref(), Some(effect_json))
            }
            other => (other, None),
        };
        let mut error = inner.fields();
        error["code"] = json!(inner.code());
        error["message"] = json!(self.to_string());
        error["retry"] = json!(self.retry());
        if let Some(effect) = effect {
            error["effect"] = effect;
        }
        json!({ "error": error })
    }

    /// The variant's own fields, as data.
    fn fields(&self) -> Value {
        match self {
            Self::UnknownHandle { handle, kind } => json!({ "handle": handle, "kind": kind }),
            Self::Gone { handle, kind, why } => {
                json!({ "handle": handle, "kind": kind, "why": why })
            }
            Self::Disabled { element, action } => json!({ "element": element, "action": action }),
            Self::ActionUnsupported {
                element,
                action,
                supported,
                unread,
            } => json!({
                "element": element,
                "action": action,
                "supported": supported,
                "unread": unread,
            }),
            Self::NotForeground { target, foreground } => {
                json!({ "target": target, "foreground": foreground })
            }
            Self::FocusElsewhere { target, holder } => {
                json!({ "target": target, "holder": holder })
            }
            Self::OutsideTarget {
                x,
                y,
                reason,
                covered_by,
            } => json!({ "x": x, "y": y, "reason": reason, "covered_by": covered_by }),
            Self::Timeout {
                timeout_ms,
                what,
                summary,
            } => json!({ "timeout_ms": timeout_ms, "what": what, "summary": summary }),
            Self::Platform { context, message } => {
                json!({ "context": context, "detail": message })
            }
            _ => json!({}),
        }
    }
}

/// The result every backend operation returns.
pub type ToolResult<T> = Result<T, ToolError>;

#[cfg(test)]
mod tests {
    use super::*;

    /// The code is the variant's, fixed; an interrupted action reports its
    /// cause's code and its effect as data.
    #[test]
    fn codes_are_fixed_and_effects_are_data() {
        let err = ToolError::NotForeground {
            target: "window w3".into(),
            foreground: Some(WindowRef {
                window: Some("w9".into()),
                pid: 42,
                title: Some("Other".into()),
            }),
        };
        assert_eq!(err.code(), "not_foreground");
        assert_eq!(err.retry(), Retry::Never);
        let payload = err.payload();
        assert_eq!(payload["error"]["code"], "not_foreground");
        assert_eq!(payload["error"]["retry"], "never");
        assert_eq!(payload["error"]["foreground"]["window"], "w9");
        assert_eq!(payload["error"]["foreground"]["pid"], 42);
        assert!(err.to_string().contains("window w9 of process 42"), "{err}");
        assert!(payload["error"].get("effect").is_none());

        let partial = err.after(
            Effect::Partial {
                sent: 3,
                total: 10,
                unit: "characters",
            },
            "so a retry repeats them",
        );
        assert_eq!(partial.code(), "not_foreground");
        assert_eq!(partial.retry(), Retry::Never, "part of it went out");
        let payload = partial.payload();
        assert_eq!(payload["error"]["effect"]["kind"], "partial");
        assert_eq!(payload["error"]["effect"]["sent"], 3);
        assert_eq!(payload["error"]["effect"]["unit"], "characters");
        assert!(partial.to_string().contains("retry repeats"), "{partial}");
    }

    /// An effect already recorded is kept: what happened first is what a
    /// retry must know.
    #[test]
    fn the_first_effect_wins() {
        let err = ToolError::Busy("x".into())
            .after(Effect::MayHaveRun, "first")
            .after(Effect::Ran, "second");
        assert!(matches!(
            &err,
            ToolError::Interrupted {
                effect: Effect::MayHaveRun,
                detail,
                ..
            } if detail == "first; second"
        ));
    }

    #[test]
    fn retry_policy_follows_the_code() {
        assert_eq!(ToolError::Busy("q".into()).retry(), Retry::Soon);
        assert_eq!(ToolError::InputHeld("Shift".into()).retry(), Retry::Soon);
        assert_eq!(
            ToolError::NotFound("no window".into()).retry(),
            Retry::WhenAppears
        );
        assert_eq!(
            ToolError::gone_element("e1", "removed").retry(),
            Retry::Never
        );
        assert_eq!(ToolError::ShuttingDown.retry(), Retry::Never);
    }

    #[test]
    fn handle_errors_name_their_kind() {
        let unknown = ToolError::UnknownHandle {
            handle: "w7".into(),
            kind: HandleKind::Window,
        };
        assert!(unknown.to_string().contains("list_windows"), "{unknown}");
        assert_eq!(unknown.payload()["error"]["kind"], "window");
        let gone = ToolError::gone_element("e1", "the application removed it");
        assert!(gone.to_string().contains("read the tree again"), "{gone}");
    }
}
