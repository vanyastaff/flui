//! The rule over hand-built rustdoc JSON: one shape per case.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Value as Json, json};

use super::{Exception, Finding, Mode, Rule, check};

/// A rustdoc JSON document with no items.
fn empty() -> Json {
    json!({ "index": {}, "paths": {}, "external_crates": {} })
}

/// Declares the upstream crate `name` and its item `path`; returns the item's id.
fn upstream(doc: &mut Json, crate_id: u64, path: &[&str]) -> u64 {
    let id = 1000 + doc["paths"].as_object().map_or(0, serde_json::Map::len) as u64;
    doc["external_crates"][crate_id.to_string()] = json!({ "name": path[0] });
    doc["paths"][id.to_string()] = json!({ "crate_id": crate_id, "path": path, "kind": "struct" });
    id
}

/// Adds an item of the package.
fn item(doc: &mut Json, id: u64, name: Option<&str>, inner: Json) {
    doc["index"][id.to_string()] = json!({
        "id": id,
        "crate_id": 0,
        "name": name,
        "visibility": "public",
        "span": { "filename": "src/lib.rs", "begin": [id, 0], "end": [id, 0] },
        "inner": inner,
    });
}

fn returning(id: u64) -> Json {
    json!({ "function": { "sig": { "inputs": [], "output": { "resolved_path": { "path": "T", "id": id, "args": null } } } } })
}

fn rule(mode: Mode, exceptions: Vec<Exception>) -> Rule {
    Rule {
        package: "pkg".to_owned(),
        lib: "pkg".to_owned(),
        mode,
        exceptions,
    }
}

/// `item -> named` for each finding, and `unused <crate>` for an unused exception.
fn found(doc: &Json, rule: &Rule) -> Vec<String> {
    let workspace = BTreeSet::from(["flui_foundation".to_owned()]);
    check(doc, rule, &workspace, Path::new(""))
        .expect("well-formed document")
        .into_iter()
        .map(|finding| match finding {
            Finding::Names { item, named, .. } => format!("{item} -> {named}"),
            Finding::UnusedException { krate, .. } => format!("unused {krate}"),
        })
        .collect()
}

#[test]
fn a_stable_signature_names_only_what_adr_0089_allows() {
    let mut doc = empty();
    let ui = upstream(&mut doc, 1, &["ui_events", "PointerEvent"]);
    let raw = upstream(&mut doc, 2, &["raw_window_handle", "RawWindowHandle"]);
    let handle = upstream(&mut doc, 2, &["raw_window_handle", "WindowHandle"]);
    let cursor = upstream(&mut doc, 3, &["cursor_icon", "CursorIcon"]);
    let own = upstream(&mut doc, 4, &["flui_foundation", "Size"]);
    let std = upstream(&mut doc, 5, &["core", "option", "Option"]);
    for (id, target) in [
        (1, ui),
        (2, raw),
        (3, handle),
        (4, cursor),
        (5, own),
        (6, std),
    ] {
        item(&mut doc, id, Some(&format!("f{id}")), returning(target));
    }
    assert_eq!(
        found(&doc, &rule(Mode::Stable, Vec::new())),
        [
            "f1 -> ui_events::PointerEvent",
            "f2 -> raw_window_handle::RawWindowHandle"
        ]
    );
}

#[test]
fn a_blanket_or_synthetic_impl_is_not_the_packages_signature() {
    let mut doc = empty();
    let instrument = upstream(&mut doc, 1, &["tracing", "instrument", "Instrument"]);
    let implementation = |blanket: Json, synthetic: bool| {
        json!({ "impl": {
            "trait": { "path": "Instrument", "id": instrument, "args": null },
            "for": { "generic": "T" },
            "items": [],
            "blanket_impl": blanket,
            "is_synthetic": synthetic,
        } })
    };
    item(
        &mut doc,
        1,
        None,
        implementation(json!({ "generic": "T" }), false),
    );
    item(&mut doc, 2, None, implementation(Json::Null, true));
    assert_eq!(
        found(&doc, &rule(Mode::Stable, Vec::new())),
        Vec::<String>::new()
    );
    item(&mut doc, 3, None, implementation(Json::Null, false));
    assert_eq!(
        found(&doc, &rule(Mode::Stable, Vec::new())),
        ["impl -> tracing::instrument::Instrument"]
    );
}

#[test]
fn serde_is_allowed_only_inside_an_implementation_of_its_own_traits() {
    let mut doc = empty();
    let serialize = upstream(&mut doc, 1, &["serde", "ser", "Serialize"]);
    let serializer = upstream(&mut doc, 1, &["serde", "ser", "Serializer"]);
    item(
        &mut doc,
        1,
        None,
        json!({ "impl": {
            "trait": { "path": "Serialize", "id": serialize, "args": null },
            "for": { "resolved_path": { "path": "Own", "id": 99, "args": null } },
            "items": [2],
            "blanket_impl": null,
            "is_synthetic": false,
        } }),
    );
    item(&mut doc, 2, Some("serialize"), returning(serializer));
    assert_eq!(
        found(&doc, &rule(Mode::Stable, Vec::new())),
        Vec::<String>::new()
    );
    item(&mut doc, 3, Some("free"), returning(serializer));
    assert_eq!(
        found(&doc, &rule(Mode::Stable, Vec::new())),
        ["free -> serde::ser::Serializer"]
    );
}

#[test]
fn a_public_reexport_of_an_upstream_item_is_a_finding() {
    let mut doc = empty();
    let role = upstream(&mut doc, 1, &["accesskit", "Role"]);
    item(
        &mut doc,
        1,
        Some("Role"),
        json!({ "use": { "source": "accesskit::Role", "name": "Role", "id": role, "is_glob": false } }),
    );
    assert_eq!(
        found(&doc, &rule(Mode::Stable, Vec::new())),
        ["use accesskit::Role -> accesskit::Role"]
    );
}

#[test]
fn an_exception_admits_its_crate_until_nothing_needs_it() {
    let mut doc = empty();
    let ui = upstream(&mut doc, 1, &["ui_events", "PointerEvent"]);
    let exception = |krate: &str| Exception {
        krate: krate.to_owned(),
        exit: "ADR-0143".to_owned(),
    };
    let both = rule(
        Mode::Stable,
        vec![exception("ui_events"), exception("keyboard_types")],
    );
    assert_eq!(
        found(&doc, &both),
        ["unused keyboard_types", "unused ui_events"]
    );
    item(&mut doc, 1, Some("f"), returning(ui));
    assert_eq!(found(&doc, &both), ["unused keyboard_types"]);
}

#[test]
fn a_deny_list_reports_only_the_crates_it_names() {
    let mut doc = empty();
    let hwnd = upstream(&mut doc, 1, &["windows", "Win32", "Foundation", "HWND"]);
    let span = upstream(&mut doc, 2, &["tracing", "Span"]);
    item(&mut doc, 1, Some("hwnd"), returning(hwnd));
    item(&mut doc, 2, Some("span"), returning(span));
    let deny = rule(
        Mode::Deny(BTreeSet::from(["windows".to_owned()])),
        Vec::new(),
    );
    assert_eq!(
        found(&doc, &deny),
        ["hwnd -> windows::Win32::Foundation::HWND"]
    );
}
