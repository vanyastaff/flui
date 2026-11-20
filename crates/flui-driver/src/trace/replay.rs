//! Replaying a trace (ADR-0241 §6): each command is sent again, through the method table, and
//! its result compared with the recorded one modulo what varies between runs.
//!
//! - **Handles** are renumbered by first appearance: the first element or window handle a result
//!   names stands for the one the replay's result names in its place, and a later command's
//!   parameters are sent with the replay's handle. Only the fields of the protocol that hold a
//!   handle ([`HANDLE_FIELDS`]) are read as handles: a text or a locator's name that looks like
//!   one (`e1`) is text. Locators are sent as written, so the replay resolves them again.
//! - **Redacted text** is never sent: a command of a redacted trace whose parameters hold the
//!   redaction mark ends the replay there, refused (ADR-0215 §5, ADR-0241 §4); the trace does not
//!   hold what was typed. A trace recorded with `keep_text` replays it.
//! - **Times and frames** (`t_ms`, `frame`, `frames`) are left out of the comparison.
//! - **Positions** (`rect`, `surface_rect`, `point`, `at`, `x`, `y`) are compared only when the
//!   header's fidelity is exact.
//! - **Text** in a redacted trace is compared redacted: the replay's result is redacted the same
//!   way first.
//! - A failure compares by its wire code.
//!
//! The first result that differs ends the replay with a [`Divergence`]. A command whose method is
//! not in the protocol's table (a pixel check, whose row joins the table with the front ends) is
//! listed as skipped, never compared.

use std::collections::BTreeMap;

use flui_protocol::method;
use serde_json::Value;

use super::format::{Positions, Privacy, TraceError};
use super::reader::Trace;
use super::redact;
use crate::driver::Driver;
use crate::error::{Error, Result};
use crate::scenario::WindowSpec;

/// Fields a replay never compares: the times and frame counts of a run.
const VOLATILE: &[&str] = &["t_ms", "frame", "frames"];

/// Fields a replay compares only at exact fidelity: where things were.
const POSITIONS: &[&str] = &["rect", "surface_rect", "point", "at", "x", "y"];

/// The fields of the protocol that hold a window or element handle (`w3`, `e12`, `w3/e12`), alone
/// or in a list: the only strings a replay renumbers. The protocol's schema test holds this list
/// to every field whose schema is a handle.
pub const HANDLE_FIELDS: &[&str] = &[
    "element", "elements", "gone", "id", "node", "parent", "root", "window", "windows",
];

/// What a replay came to.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct ReplayReport {
    /// The commands sent again.
    pub replayed: u64,
    /// The commands not sent, each with why.
    pub skipped: Vec<Skipped>,
    /// The command the replay refused to send, which ended it: one that would send redacted
    /// text.
    pub refused: Option<Skipped>,
    /// The first result that differed; none when every compared result matched.
    pub divergence: Option<Divergence>,
}

impl ReplayReport {
    /// Whether every command was sent and matched.
    #[must_use]
    pub fn matched(&self) -> bool {
        self.divergence.is_none() && self.skipped.is_empty() && self.refused.is_none()
    }
}

/// A command a replay did not send.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Skipped {
    /// Its number.
    pub seq: u64,
    /// Its method.
    pub method: String,
    /// Why.
    pub reason: String,
}

/// Where a replay's result first differed from the recorded one.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Divergence {
    /// The command's number.
    pub seq: u64,
    /// Its method.
    pub method: String,
    /// Where in the result they differ, as a JSON pointer.
    pub at: String,
    /// What the trace holds there.
    pub expected: Value,
    /// What the replay answered there.
    pub actual: Value,
}

/// Replay `trace` against `driver`'s target: see the [module docs](self).
///
/// # Errors
///
/// [`Error::InvalidArgument`] naming both windows when the target's main window is not the size
/// the trace was recorded at; the target's failure to read its main window.
pub fn replay(trace: &Trace, driver: &mut Driver) -> Result<ReplayReport> {
    let header = trace.header();
    if let Some(window) = header.window {
        let placement = driver.main_window()?.placement()?;
        WindowSpec::new(window.width, window.height, window.scale).check(&placement)?;
    }
    let mut handles = Handles::default();
    let mut report = ReplayReport::default();
    let positions = header.fidelity.positions == Positions::Exact;
    let redacted = header.privacy == Privacy::Redact;
    for step in trace.steps() {
        let command = step.command;
        let Some(row) = method::find(&command.method) else {
            report.skipped.push(Skipped {
                seq: command.seq,
                method: command.method.clone(),
                reason: "not a method of the protocol's table".into(),
            });
            continue;
        };
        if row.name.starts_with("session.") {
            report.skipped.push(Skipped {
                seq: command.seq,
                method: command.method.clone(),
                reason: "it begins or ends a session, which the replay holds".into(),
            });
            continue;
        }
        if redacted && holds_redaction(&command.params) {
            report.refused = Some(Skipped {
                seq: command.seq,
                method: command.method.clone(),
                reason:
                    "its parameters were redacted: the trace does not hold what was typed, and \
                         a replay never sends the redaction mark in its place; record with \
                         `keep_text` to replay it"
                        .into(),
            });
            break;
        }
        let mut params = command.params.clone();
        handles.substitute(&mut params, false);
        let answered = driver.call_value(row.name, params);
        report.replayed += 1;
        let Some(recorded) = step.result else {
            // The trace ends before this command answered: there is nothing to compare with.
            break;
        };
        let compare = Compare {
            positions,
            handles: &mut handles,
        };
        let divergence = match (&recorded.ok, &recorded.error, answered) {
            (Some(expected), _, Ok(mut actual)) => {
                if redacted {
                    redact::output(&mut actual);
                }
                compare.values(expected, &actual)
            }
            (_, Some(expected), Err(actual)) => {
                (expected.code != actual.code()).then(|| Mismatch {
                    at: "/error/code".into(),
                    expected: serde_json::to_value(expected.code).unwrap_or(Value::Null),
                    actual: serde_json::to_value(actual.code()).unwrap_or(Value::Null),
                })
            }
            (Some(expected), _, Err(actual)) => Some(Mismatch {
                at: String::new(),
                expected: expected.clone(),
                actual: error_value(&actual, redacted),
            }),
            (_, Some(expected), Ok(actual)) => Some(Mismatch {
                at: String::new(),
                expected: serde_json::to_value(expected).unwrap_or(Value::Null),
                actual,
            }),
            (None, None, _) => None,
        };
        if let Some(mismatch) = divergence {
            report.divergence = Some(Divergence {
                seq: command.seq,
                method: command.method.clone(),
                at: mismatch.at,
                expected: mismatch.expected,
                actual: mismatch.actual,
            });
            break;
        }
    }
    Ok(report)
}

/// `error` as a trace would keep it.
fn error_value(error: &Error, redacted: bool) -> Value {
    let kept = TraceError {
        code: error.code(),
        message: if redacted {
            flui_protocol::input_recording::REDACTED.to_owned()
        } else {
            error.to_string()
        },
    };
    serde_json::json!({ "error": kept })
}

/// A recorded handle and the replay's in its place, each way, so two recorded handles never
/// stand for one of the replay's.
#[derive(Debug, Default)]
struct Handles {
    recorded_to_actual: BTreeMap<String, String>,
    actual_to_recorded: BTreeMap<String, String>,
}

impl Handles {
    /// Pair `recorded` with `actual`; whether that agrees with every earlier pairing.
    fn pair(&mut self, recorded: &str, actual: &str) -> bool {
        match (
            self.recorded_to_actual.get(recorded),
            self.actual_to_recorded.get(actual),
        ) {
            (Some(paired), _) => paired == actual,
            (None, Some(_)) => false,
            (None, None) => {
                self.recorded_to_actual
                    .insert(recorded.to_owned(), actual.to_owned());
                self.actual_to_recorded
                    .insert(actual.to_owned(), recorded.to_owned());
                true
            }
        }
    }

    /// Replace each recorded handle in `params` with the replay's: the strings of
    /// [`HANDLE_FIELDS`] only, `in_handle` saying whether `params` is the value of one.
    fn substitute(&self, params: &mut Value, in_handle: bool) {
        match params {
            Value::String(text) if in_handle && is_handle(text) => {
                if let Some(actual) = self.recorded_to_actual.get(text.as_str()) {
                    actual.clone_into(text);
                } else if let Some((window, element)) = text.split_once('/') {
                    let window = self
                        .recorded_to_actual
                        .get(window)
                        .map_or(window, String::as_str);
                    *text = format!("{window}/{element}");
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.substitute(item, in_handle);
                }
            }
            Value::Object(fields) => {
                for (key, value) in fields.iter_mut() {
                    self.substitute(value, HANDLE_FIELDS.contains(&key.as_str()));
                }
            }
            _ => {}
        }
    }
}

/// Whether `params` holds the redaction mark anywhere: text the trace replaced.
fn holds_redaction(params: &Value) -> bool {
    match params {
        Value::String(text) => text == flui_protocol::input_recording::REDACTED,
        Value::Array(items) => items.iter().any(holds_redaction),
        Value::Object(fields) => fields.values().any(holds_redaction),
        _ => false,
    }
}

/// Whether `text` is a handle the app minted: `w3`, `e12` or `w3/e12`.
fn is_handle(text: &str) -> bool {
    let numbered = |part: &str, prefix: char| {
        part.strip_prefix(prefix).is_some_and(|digits| {
            !digits.is_empty()
                && !digits.starts_with('0')
                && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
    };
    match text.split_once('/') {
        Some((window, element)) => numbered(window, 'w') && numbered(element, 'e'),
        None => numbered(text, 'w') || numbered(text, 'e'),
    }
}

struct Mismatch {
    at: String,
    expected: Value,
    actual: Value,
}

struct Compare<'a> {
    positions: bool,
    handles: &'a mut Handles,
}

impl Compare<'_> {
    fn values(mut self, expected: &Value, actual: &Value) -> Option<Mismatch> {
        self.walk(String::new(), expected, actual, false)
    }

    /// The first place `expected` and `actual` differ under `at`; `in_handle` says whether they
    /// are the value of one of [`HANDLE_FIELDS`], where handles pair.
    fn walk(
        &mut self,
        at: String,
        expected: &Value,
        actual: &Value,
        in_handle: bool,
    ) -> Option<Mismatch> {
        let differ = |at: String| Mismatch {
            at,
            expected: expected.clone(),
            actual: actual.clone(),
        };
        match (expected, actual) {
            (Value::Object(left), Value::Object(right)) => {
                let keys: std::collections::BTreeSet<&String> =
                    left.keys().chain(right.keys()).collect();
                for key in keys {
                    if VOLATILE.contains(&key.as_str())
                        || (!self.positions && POSITIONS.contains(&key.as_str()))
                    {
                        continue;
                    }
                    let child = format!("{at}/{}", key.replace('~', "~0").replace('/', "~1"));
                    match (left.get(key), right.get(key)) {
                        (Some(left), Some(right)) => {
                            let handle = HANDLE_FIELDS.contains(&key.as_str());
                            if let Some(mismatch) = self.walk(child, left, right, handle) {
                                return Some(mismatch);
                            }
                        }
                        (left, right) => {
                            return Some(Mismatch {
                                at: child,
                                expected: left.cloned().unwrap_or(Value::Null),
                                actual: right.cloned().unwrap_or(Value::Null),
                            });
                        }
                    }
                }
                None
            }
            (Value::Array(left), Value::Array(right)) => {
                if left.len() != right.len() {
                    return Some(differ(at));
                }
                left.iter()
                    .zip(right)
                    .enumerate()
                    .find_map(|(index, (left, right))| {
                        self.walk(format!("{at}/{index}"), left, right, in_handle)
                    })
            }
            (Value::String(left), Value::String(right))
                if in_handle && is_handle(left) && is_handle(right) =>
            {
                let same_kind = left.contains('/') == right.contains('/')
                    && left.chars().next() == right.chars().next();
                (!same_kind || !self.handles.pair(left, right)).then(|| differ(at))
            }
            (left, right) => (left != right).then(|| differ(at)),
        }
    }
}
