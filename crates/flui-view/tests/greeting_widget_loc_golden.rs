//! The canonical `Greeting { name: String }` stateless
//! widget compiles in ≤ 7 lines of widget-author code.
//!
//! This is framed as a parity claim with Flutter:
//! `class Greeting extends StatelessWidget { final String name;
//! const Greeting({super.key, required this.name}); @override
//! Widget build(BuildContext c) => Text(name); }` is 6-7 lines
//! depending on `@override` placement. The FLUI minimum is **7**
//! after rustfmt-canonical formatting because rustfmt forces
//! opening braces on their own line for `impl` blocks (Dart does
//! not).
//!
//! What the source MUST NOT contain (also asserted below):
//!   - `Box::new(`  — no manual boxing at the build call site.
//!   - `impl View for Greeting`  — the derive emits this.
//!   - `impl_stateless_view!(`  — the declarative macro is deleted.
//!   - `.into_view()`  — the framework normalizes; the author
//!     never types it.
//!
//! The fixture lives in `tests/fixtures/greeting.rs` so a future
//! `rustfmt` configuration change re-canonicalizes the same
//! source. The line-count assertion is against the raw byte count
//! (`\n`-separated) which `rustfmt` controls; an intentional
//! diagnostic-comment change to the fixture that pushes it past
//! 7 lines fails this test loudly — by design.
//!
//! The fixture itself is NOT compiled as a separate crate target;
//! it is read as a source-text artifact. Including it under
//! `tests/fixtures/` rather than `tests/` keeps `cargo test` from
//! picking it up as a test binary (cargo treats `tests/*.rs` —
//! not `tests/fixtures/*.rs` — as integration targets).

const GREETING_FIXTURE: &str = include_str!("fixtures/greeting.rs");

#[test]
fn greeting_is_at_most_seven_lines() {
    let line_count = GREETING_FIXTURE.lines().count();
    assert!(
        line_count <= 7,
        "Greeting widget fixture is {line_count} lines (max 7). \
         If a recent edit pushed the fixture past 7 lines, audit whether \
         the extra line is essential authoring code (in which case revisit \
         the ≤7-line bound) or accidental — most likely an extra \
         comment, blank line, or imports that should consolidate. The \
         fixture lives at `crates/flui-view/tests/fixtures/greeting.rs`."
    );
}

#[test]
fn greeting_uses_no_forbidden_syntax() {
    // The "no Box::new / no impl View / no impl_stateless_view! /
    // no .into_view()" requirements are spec-level. Assert each
    // explicitly so a regression that re-introduces the verbose
    // pattern surfaces with a clear failure.
    let forbidden = [
        ("Box::new(", "manual boxing at the build call site"),
        (
            "impl View for Greeting",
            "explicit View impl block (derive emits it)",
        ),
        ("impl_stateless_view!(", "deleted declarative macro"),
        (
            ".into_view()",
            "framework normalizes — author never types this",
        ),
        (
            "BoxedView::new(",
            "manual BoxedView construction (use .boxed() on recursion edge)",
        ),
    ];
    for (needle, why) in forbidden {
        assert!(
            !GREETING_FIXTURE.contains(needle),
            "Forbidden syntax `{needle}` found in Greeting fixture ({why}). \
             Audit `crates/flui-view/tests/fixtures/greeting.rs`."
        );
    }
}
