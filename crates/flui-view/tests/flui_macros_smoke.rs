//! Smoke test for the `flui-macros` dependency edge.
//!
//! Phase 1 §U5: the `flui-macros` crate ships as a skeleton (only a
//! placeholder `#[proc_macro_derive]` export). This test proves that:
//!
//! 1. `flui-view` depends on `flui-macros` (the `use flui_macros::...`
//!    line below would fail to compile otherwise).
//! 2. The placeholder derive applies to a local struct without error.
//!
//! Phase 3 §U23 replaces the placeholder with the real
//! `#[derive(StatelessView)]` / `#[derive(StatefulView)]` derives and
//! rewrites this test against the production surface.

use flui_macros::FluiMacrosPlaceholder;

// The placeholder emits an empty token stream, so applying the derive
// to a struct is a no-op. This compile-time check verifies that:
// - `flui-macros` is linked into `flui-view`'s dev-build.
// - The placeholder derive name resolves and is callable.
//
// The struct itself never participates in any view machinery; it is
// scaffolding for the linkage check only.
#[derive(FluiMacrosPlaceholder)]
struct LinkageProbe {
    _field: u8,
}

#[test]
fn flui_macros_crate_is_linked_into_flui_view() {
    // Construct the probe to ensure the derive expansion compiles
    // through to a usable type. The actual value is unused; the test
    // succeeds by virtue of compiling.
    let _probe = LinkageProbe { _field: 0 };
}
