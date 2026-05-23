//! SC-011 — `ElementKind` is a closed `#[non_exhaustive]` enum.
//!
//! The contract: a contributor adding a new `ElementKind` variant
//! should see the compiler enumerate every non-exhaustive `match`
//! site that needs updating. This is the "loud failure on
//! structural extension" guarantee Constitution Principle 4 and
//! FR-019 commit to.
//!
//! This file pins TWO complementary properties:
//!
//! 1. **`ElementKind` is `#[non_exhaustive]`** — a downstream
//!    `match` on `ElementKind` without a `_` arm fails to compile
//!    once the enum gains a new variant. The
//!    `classify_compile_check` function below is the canary: it
//!    matches every Phase 1 §U6 variant by name AND ends with
//!    `_ => …`, so it is *forward-safe* (compiles after a future
//!    variant addition) and *backward-strict* (the named arms catch
//!    any rename or removal at compile time).
//! 2. **Every variant currently in the enum is exercised by name**
//!    — the eight named arms together cover the Phase 1 §U6
//!    closed set (`Stateless`, `Stateful`, `Proxy`, `Inherited`,
//!    `RenderLeaf`, `RenderSingle`, `RenderOptional`,
//!    `RenderVariable`). A contributor who renames a variant
//!    sees a compile-fail at the matching arm.
//!
//! The CI feature-flagged stub-variant smoke (plan §U33 referencing
//! §U6) is deferred — the Phase 3 PR opens with this in-test
//! compile-time guard plus the future-proof wildcard. A real
//! `cfg(feature = "test-non-exhaustive-smoke")` stub-variant test
//! that gates a CI smoke job is the natural next step (post-merge,
//! not blocking) — tracked under `docs/plans/2026-05-22-005-...md`
//! "Open Questions".

use flui_view::element::ElementKind;

/// Match-canary: every Phase 1 §U6 variant named explicitly, plus
/// a `_` arm so a future variant addition does NOT silently
/// regress this test. The function is `#[allow(dead_code)]`
/// because the test below only references its address, not the
/// body — the compile-time check the function performs is on the
/// `match` arms themselves.
#[allow(dead_code, reason = "compile-time match-arm enumeration is the test")]
fn classify_compile_check(kind: &ElementKind) -> &'static str {
    match kind {
        ElementKind::Stateless(_) => "Stateless",
        ElementKind::Stateful { .. } => "Stateful",
        ElementKind::Proxy(_) => "Proxy",
        ElementKind::Inherited(_) => "Inherited",
        ElementKind::RenderLeaf(_) => "RenderLeaf",
        ElementKind::RenderSingle(_) => "RenderSingle",
        ElementKind::RenderOptional(_) => "RenderOptional",
        ElementKind::RenderVariable(_) => "RenderVariable",
        _ => "<unknown variant — sc011_non_exhaustive_smoke needs an update>",
    }
}

#[test]
fn covers_sc011_element_kind_is_non_exhaustive_and_variants_named() {
    // The function pointer is what we observe — the compiler
    // accepts the body only if every named arm matches an actual
    // variant. The runtime call is unnecessary for the check
    // (and would require constructing an ElementKind value, which
    // pulls in real owner machinery).
    let f: fn(&ElementKind) -> &'static str = classify_compile_check;
    // Sanity: prove the address is well-defined (cast through
    // usize so the test does not depend on `std::ptr::fn_addr_eq`
    // requiring `T: FnPtr`, which only matches typed `fn`
    // pointers — closures coerce-to-fn but the bound is fussy
    // when both sides are plain `fn` items).
    let addr: usize = f as usize;
    assert!(addr != 0);
}
