//! Integration tests for `#[derive(Diagnosticable)]`.
//!
//! The derive lives in a `proc-macro = true` crate, which cannot host
//! `#[test]` functions that *use* its own derives — the macro is not
//! available to the crate that defines it. These integration tests are a
//! separate compilation unit and so can exercise the generated code.

use flui_foundation::{Diagnosticable, DiagnosticsProperty};
use flui_macros::Diagnosticable;

#[test]
fn diagnosticable_derive_basic() {
    #[derive(Debug, Diagnosticable)]
    struct TestWidget {
        width: f32,
        height: f32,
        // Skipped from diagnostics, hence never read.
        #[diagnostic(skip)]
        #[allow(dead_code)]
        internal_id: u64,
    }

    let widget = TestWidget {
        width: 100.0,
        height: 50.0,
        internal_id: 42,
    };

    let node = widget.to_diagnostics_node();
    assert_eq!(node.name(), Some("TestWidget"));

    let props = node.properties();
    assert_eq!(props.len(), 2, "internal_id must be skipped");

    let names: Vec<&str> = props.iter().map(DiagnosticsProperty::name).collect();
    assert!(names.contains(&"width"));
    assert!(names.contains(&"height"));
    assert!(
        !names.contains(&"internal_id"),
        "skipped field must be absent"
    );
}

#[test]
fn diagnosticable_derive_empty_struct() {
    #[derive(Debug, Diagnosticable)]
    struct Empty {}

    let node = Empty {}.to_diagnostics_node();
    assert_eq!(node.name(), Some("Empty"));
    assert_eq!(node.properties().len(), 0);
}

#[test]
fn diagnosticable_derive_all_skipped() {
    #[derive(Debug, Diagnosticable)]
    #[allow(dead_code)]
    struct AllSkipped {
        #[diagnostic(skip)]
        a: u32,
        #[diagnostic(skip)]
        b: u32,
    }

    let node = AllSkipped { a: 1, b: 2 }.to_diagnostics_node();
    assert_eq!(node.properties().len(), 0);
}

#[test]
fn diagnosticable_derive_generic() {
    #[derive(Debug, Diagnosticable)]
    struct Wrap<T: std::fmt::Debug> {
        inner: T,
    }

    let node = Wrap { inner: 7u32 }.to_diagnostics_node();
    // The node name is produced by the trait-default `to_diagnostics_node`,
    // which uses `type_name::<Self>()` — for a monomorphized generic that
    // includes the type arguments (`Wrap<u32>`). The derive itself only
    // generates `debug_fill_properties`, so it does not influence the name.
    assert_eq!(node.name(), Some("Wrap<u32>"));
    let props = node.properties();
    assert_eq!(props.len(), 1);
    assert_eq!(props[0].name(), "inner");
    assert_eq!(props[0].value(), "7");
}

#[test]
fn diagnosticable_derive_lifetime_generic() {
    #[derive(Debug, Diagnosticable)]
    struct Borrowed<'a> {
        label: &'a str,
    }

    let s = String::from("hi");
    let node = Borrowed { label: &s }.to_diagnostics_node();
    // Trait-default name via `type_name` includes the elided lifetime.
    assert_eq!(node.name(), Some("Borrowed<'_>"));
    assert_eq!(node.properties().len(), 1);
    assert_eq!(node.properties()[0].name(), "label");
}

#[test]
fn diagnosticable_derive_raw_identifier() {
    #[derive(Debug, Diagnosticable)]
    struct Raw {
        r#type: u32,
    }

    let node = Raw { r#type: 9 }.to_diagnostics_node();
    let props = node.properties();
    assert_eq!(props.len(), 1);
    // The diagnostic name must strip the `r#` raw prefix.
    assert_eq!(props[0].name(), "type");
}
