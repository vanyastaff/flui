QUESTION: What should FLUI learn from Flutter #85026 ("StackOverflowError when building too many widgets"), especially from the comments about provider/dependency layers?

ANSWER: Flutter's issue is not just "too many visual widgets": comments identify dependency/provider-style non-UI widgets as a source of accidental tree depth, and a `LayoutBuilder` workaround works because it splits build across a later framework phase. FLUI's current element build/reconcile path is intentionally more stack-stable: child builds are scheduled through the dirty heap instead of recursively performed by the parent, inactive unmount collection uses an explicit heap work-stack, and render deep-tree stack tests cover layout/paint/hit/intrinsic/compositing/disposal. I did not file an issue because the audited paths already address the concrete failure class, but FLUI should keep a direct "thousands of proxy/stateless layers built through build_scope" regression test before provider/inherited APIs become heavy production patterns.

VERSIONS:
- Flutter issue state checked on 2026-09-13: flutter/flutter#85026 is open, labeled `c: crash`, `framework`, `customer: crowd`, `P2`, `has reproducible steps`, `team-framework`, `triaged-framework`.
- FLUI workspace at local checkout on 2026-09-13.

SOURCES:
- GitHub issue: https://github.com/flutter/flutter/issues/85026
- Flutter #85026 comments:
  - https://github.com/flutter/flutter/issues/85026#issuecomment-884060759 says the depth limit affects very large enterprise applications and points at related architecture work.
  - https://github.com/flutter/flutter/issues/85026#issuecomment-885428067 reports that inserting `LayoutBuilder` resets effective call-stack depth because the rest of the tree builds during layout.
  - https://github.com/flutter/flutter/issues/85026#issuecomment-2062020298 argues widgets are not free and very deep widget counts should trigger app-architecture review.
  - https://github.com/flutter/flutter/issues/85026#issuecomment-2063325826 says roughly half the widgets in one app come from API clients, repositories, cubits, services, and Provider layers.
  - https://github.com/flutter/flutter/issues/85026#issuecomment-2132183927 reports similar Flutter web behavior with `flutter_bloc`, mitigated by `LayoutBuilder`.
  - https://github.com/flutter/flutter/issues/85026#issuecomment-2194332092 states the architectural concern directly: non-UI dependency/provider widgets should not increase stack size enough to crash.
- `crates/flui-view/src/owner/build_owner.rs:1139-1167` exposes `BuildOwner::build_scope`, which drains scheduled build work rather than recursively walking the whole subtree from a root call.
- `crates/flui-view/src/owner/build_owner.rs:1592-1635` reconciles returned child views after a node build and relies on the reconciler to schedule newly inserted children for the same drain loop.
- `crates/flui-view/src/tree/id_reconcile.rs:394-403` documents the key invariant: each child rebuilds as its own build-scope drain entry, not via recursive `perform_build` from its parent.
- `crates/flui-view/src/owner/build_owner.rs:2114-2137` collects inactive elements with an explicit `Vec<ElementId>` work stack and documents constant call-stack usage.
- `crates/flui-rendering/tests/deep_tree_stack.rs:1-17` documents the render-tree stack overflow class and why deep stack-safety tests exist.
- `crates/flui-rendering/tests/deep_tree_stack.rs:27-120` covers 2,500-deep layout/paint/hit/intrinsic paths and a 20,000-deep compositing-bits walk.

VERIFICATION:
- `cargo nextest run -p flui-view -p flui-rendering -E 'test(finalize_tree_survives_deep_chain) or test(eager_remove_survives_deep_chain) or test(test_collect_all_elements_deep_chain) or test(deep_tree_stack::)' --no-fail-fast`
  - Result: 7 passed, 1684 skipped.
- `cargo nextest run -p flui-view --test view_it -E 'test(live_build_context_reports_authoritative_tree_depth)' --no-fail-fast`
  - Result: 1 passed, 285 skipped.

OPEN:
- No direct FLUI regression test currently proves that a real, declarative chain of thousands of `StatelessView`/proxy/provider-like layers can be mounted and fully built through ordinary `build_scope` without exhausting the Rust call stack. Existing evidence strongly suggests the architecture supports it, but the exact user-facing failure mode from Flutter #85026 should be pinned before dependency-injection/provider APIs grow.

NOTE: .rust-studio/research/flutter-deep-widget-stack-lessons.md

ANSWERED
