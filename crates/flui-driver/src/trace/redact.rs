//! Redaction (ADR-0215 §5, ADR-0241 §4): unless the session keeps text, a trace replaces typed
//! text and every text-bearing field the app reports with
//! [`REDACTED`].
//!
//! The rule is by field name, over the JSON a row carries: every string under one of
//! [`TEXT_FIELDS`], however deep, is replaced. A command's [`TextMatch`] (an object with a
//! `match` field) is the caller's own selector or expectation, as written in the test, and is
//! kept, so a replay re-resolves it. The protocol's schema test holds every string field of the
//! wire to one side: a new one that carries text fails it until it joins the list.
//!
//! [`TextMatch`]: flui_protocol::locator::TextMatch

use flui_protocol::input_recording::REDACTED;
use serde_json::Value;

/// The fields whose strings a trace redacts: the names, values, descriptions and titles the app
/// shows, the text typed into it (a key's character, an input method's commit, as ADR-0215 §5
/// redacts them), its announcements, the outline lines that quote them, the token a session is
/// admitted with, and the source file a build report names (absolute for a file outside the
/// workspace, so it can name the author's home directory).
pub const TEXT_FIELDS: &[&str] = &[
    "candidates",
    "character",
    "commit",
    "covered_by",
    "description",
    "file",
    "label",
    "message",
    "name",
    "outline",
    "text",
    "title",
    "token",
    "value",
];

/// Redact what an app answered or sent: see the [module docs](self).
pub(crate) fn output(value: &mut Value) {
    walk(value, false, false);
}

/// Redact a command's parameters, keeping its text matches: see the [module docs](self).
pub(crate) fn params(value: &mut Value) {
    walk(value, false, true);
}

fn walk(value: &mut Value, in_text: bool, keep_matches: bool) {
    match value {
        Value::Object(fields) => {
            if keep_matches && fields.contains_key("match") {
                return;
            }
            for (key, child) in fields.iter_mut() {
                walk(
                    child,
                    in_text || TEXT_FIELDS.contains(&key.as_str()),
                    keep_matches,
                );
            }
        }
        Value::Array(items) => {
            for item in items {
                walk(item, in_text, keep_matches);
            }
        }
        Value::String(text) if in_text => REDACTED.clone_into(text),
        _ => {}
    }
}
