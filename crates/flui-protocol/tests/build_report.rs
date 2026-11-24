//! `build.report`'s diagnostics (ADR-0395 §2): a failed build holds at most the errors a window
//! shows, built or decoded.

#![cfg(feature = "serde")]

use flui_protocol::build::{BuildState, Diagnostic, Diagnostics, MAX_DIAGNOSTICS, SourceLocation};
use serde_json::{Value, json};

fn error(line: u32) -> Diagnostic {
    Diagnostic::new(
        format!("error {line}"),
        Some(SourceLocation::new("src/lib.rs".to_owned(), line, 1)),
    )
}

fn wire(line: u32) -> Value {
    json!({ "message": format!("error {line}"), "location": {
        "file": "src/lib.rs", "line": line, "column": 1
    } })
}

#[test]
fn a_failed_build_keeps_the_errors_a_window_shows_and_counts_the_rest() {
    let reported = u32::try_from(MAX_DIAGNOSTICS).expect("a small bound") + 3;
    let mut errors = Diagnostics::new();
    for line in 1..=reported {
        errors.push(error(line));
    }
    assert_eq!(
        errors.kept(),
        (1..=reported)
            .take(MAX_DIAGNOSTICS)
            .map(error)
            .collect::<Vec<Diagnostic>>(),
        "the first errors, in the compiler's order"
    );
    assert_eq!(errors.omitted(), 3);
    assert_eq!(
        serde_json::to_value(BuildState::Failed(errors)).expect("json"),
        json!({ "failed": {
            "diagnostics": (1..=reported).take(MAX_DIAGNOSTICS).map(wire).collect::<Vec<Value>>(),
            "omitted": 3,
        } })
    );
}

#[test]
fn a_report_past_the_bound_is_refused_before_its_next_diagnostic_is_read() {
    let bound = u32::try_from(MAX_DIAGNOSTICS).expect("a small bound");
    let full: Vec<Value> = (1..=bound).map(wire).collect();
    let decoded: BuildState =
        serde_json::from_value(json!({ "failed": { "diagnostics": full, "omitted": 4 } }))
            .expect("a report at the bound decodes");
    let BuildState::Failed(errors) = decoded else {
        panic!("a failed report decodes as one: {decoded:?}");
    };
    assert_eq!(errors.kept().len(), MAX_DIAGNOSTICS);
    assert_eq!(errors.omitted(), 4);

    // The element past the bound is not a diagnostic at all: the refusal names the bound, not
    // the element's type, so decoding stopped before it read that element as one.
    let mut over = full;
    over.push(json!(42));
    let refused = serde_json::from_value::<BuildState>(
        json!({ "failed": { "diagnostics": over, "omitted": 0 } }),
    )
    .expect_err("a report past the bound is refused");
    let refused = refused.to_string();
    assert!(
        refused.contains(&format!("more than {MAX_DIAGNOSTICS} diagnostics")),
        "{refused}"
    );
}
