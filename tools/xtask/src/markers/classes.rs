//! The marker classes: one pattern over prose, and for some a second over the
//! snake-case pieces of an identifier.
//!
//! Each pattern is narrowed to what the tree uses as a process label, and
//! away from the domain words that share its shape: lowercase `cycle`, numbered
//! algorithm phases, font weights, function keys, oracle line citations,
//! license ids, `PR #N` history links, `--slice N/M` flags and the horizon
//! names of the roadmap in Markdown. A false positive is fixed by narrowing a
//! pattern here, never by a marker in the source.

use std::sync::LazyLock;

use regex::Regex;

/// One marker class.
pub(super) struct Class {
    /// Its name in reports and in the allowlist.
    pub(super) name: &'static str,
    /// Matched against comments, strings and Markdown text.
    prose: Regex,
    /// In a Markdown file, used instead of `prose`.
    markdown: Option<Regex>,
    /// Matched against identifiers.
    ident: Option<Regex>,
    /// A match preceded by `--` is a command-line flag, not a marker.
    not_after_dashes: bool,
}

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("BUG: static regex")
}

/// Every class, in report order.
pub(super) static CLASSES: LazyLock<Vec<Class>> = LazyLock::new(|| {
    vec![
        Class {
            name: "cycle",
            prose: regex(r"\bCycle[ -]?\d+\b"),
            markdown: None,
            ident: Some(regex(r"(?:^|_)cycle\d+(?:_|$)")),
            not_after_dashes: false,
        },
        Class {
            name: "phase-letter",
            prose: regex(r"\bPhase[ -][A-Z]\b"),
            markdown: None,
            ident: None,
            not_after_dashes: false,
        },
        Class {
            name: "wave",
            prose: regex(
                r"\b[Ww]ave[ -]?\d+(?:\.\d+)*[a-z]?\b|\bW\d+(?:\.\d+)+[a-z]?(?:-\d+)?\b|\bW\d+-(?:[A-Z]{1,2}\d+[a-z]?|\d+)\b",
            ),
            markdown: None,
            ident: Some(regex(r"(?:^|_)wave\d+(?:_|$)")),
            not_after_dashes: false,
        },
        Class {
            name: "slice",
            prose: regex(r"\b[Ss]lice[ -]?(?:\d+|[A-Z])\b"),
            markdown: None,
            ident: Some(regex(r"(?:^|_)slice\d+(?:_|$)")),
            not_after_dashes: true,
        },
        Class {
            name: "pr-label",
            prose: regex(r"\bPR-?\d+\b|\bPR #\d+ (?:[A-Z][a-z]+ )?(?:review|comment)\b"),
            markdown: None,
            ident: Some(regex(r"(?:^|_)pr\d+(?:_|$)")),
            not_after_dashes: false,
        },
        Class {
            name: "spec-task",
            prose: regex(r"\bT\d{3}\b|\bUS\d+\b"),
            markdown: None,
            ident: Some(regex(r"(?:^|_)(?:t\d{3}|us\d+)(?:_|$)")),
            not_after_dashes: false,
        },
        Class {
            name: "tracker-h",
            prose: regex(r"\bH\d+\b"),
            // the roadmap's horizons are named with the low numbers
            markdown: Some(regex(r"\bH(?:[5-9]|\d{2,})\b")),
            ident: None,
            not_after_dashes: false,
        },
    ]
});

/// Whether `name` is a class.
pub(super) fn known(name: &str) -> bool {
    CLASSES.iter().any(|class| class.name == name)
}

/// What a chunk of text is, for choosing a pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reading {
    /// A comment (doc comments included), a string, or text in a file that is
    /// not Markdown.
    Prose,
    /// Text of a Markdown file.
    Markdown,
    /// An identifier.
    Ident,
}

/// Every `(byte range, class name)` in `text`.
pub(super) fn matches(text: &str, reading: Reading) -> Vec<(std::ops::Range<usize>, &'static str)> {
    let mut found = Vec::new();
    for class in CLASSES.iter() {
        let pattern = match reading {
            Reading::Prose => &class.prose,
            Reading::Markdown => class.markdown.as_ref().unwrap_or(&class.prose),
            Reading::Ident => match &class.ident {
                Some(pattern) => pattern,
                None => continue,
            },
        };
        for m in pattern.find_iter(text) {
            if class.not_after_dashes && text[..m.start()].ends_with("--") {
                continue;
            }
            found.push((m.range(), class.name));
        }
    }
    found
}
