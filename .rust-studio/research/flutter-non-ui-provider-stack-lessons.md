QUESTION: Does Flutter #73734 reveal an additional FLUI architecture issue around non-UI Provider/Inherited layers?

ANSWER: Flutter #73734 is the upstream architectural version of #85026: roughly 160 nested Provider widgets consumed stack frames during recursive element mounting, and commenters asked why non-UI dependency widgets deepen the call stack at all. FLUI appears to avoid the stack-frame failure class through dirty-heap child scheduling and O(1) inherited lookup maps. The concrete FLUI inherited problem is therefore not provider lookup depth or call-stack depth; it is provider-granularity invalidation, already filed as #1090. No new issue was opened.

VERSIONS:
- Flutter issue state checked on 2026-09-13: flutter/flutter#73734 is open, labeled `c: crash`, `framework`, `P3`, `team-framework`, `triaged-framework`.
- FLUI workspace at local checkout on 2026-09-13.

SOURCES:
- GitHub issue: https://github.com/flutter/flutter/issues/73734
- Flutter #73734 body says the original stack-overflow app had about 160 nested Provider widgets above the app tree and proposes reducing stack frames used by non-UI widgets like `InheritedWidget` and `Provider`.
- Flutter #73734 comment https://github.com/flutter/flutter/issues/73734#issuecomment-883757760 asks why the call has to nest so deeply and whether calls can move away from the stack.
- Flutter #73734 comment https://github.com/flutter/flutter/issues/73734#issuecomment-884053704 identifies recursive `Element.mount` shape as the likely root cause.
- Flutter #73734 comment https://github.com/flutter/flutter/issues/73734#issuecomment-1428930138 states that providers at the same level are always nested because they use `InheritedWidget`, making many providers structurally expensive.
- `crates/flui-view/src/tree/id_reconcile.rs:394-403` documents FLUI's contrasting build model: children rebuild as their own dirty-heap drain entries rather than through recursive parent `perform_build`.
- `crates/flui-view/src/view/inherited.rs:1-7` documents FLUI inherited lookup as a per-node inherited map keyed by provider view `TypeId`, so `ctx.depend_on::<T>()` is a hash lookup rather than an ancestor walk.
- `crates/flui-view/src/tree/element_tree.rs:153-180` computes inherited scopes by sharing the parent's `Arc<HashMap>` for non-provider nodes and cloning/inserting only at provider nodes.
- `crates/flui-view/src/context/element_build_context.rs:140-154` reads the resolved inherited scope instead of walking ancestors.
- `crates/flui-view/src/element/behavior.rs:1339-1374` shows the remaining provider-granularity issue: when `update_should_notify` is true, every registered dependent is scheduled. This is the problem filed in https://github.com/vanyastaff/flui/issues/1090.

VERIFICATION:
- `gh issue list --repo vanyastaff/flui --state all --limit 200 --search 'provider dependency injection inherited non-ui widget tree depth 73734 build context'` found no duplicate stack-depth issue.
- `cargo nextest run -p flui-view -p flui-rendering -E 'test(finalize_tree_survives_deep_chain) or test(eager_remove_survives_deep_chain) or test(test_collect_all_elements_deep_chain) or test(deep_tree_stack::)' --no-fail-fast`
  - Result: 7 passed, 1684 skipped.
- `cargo nextest run -p flui-view --test view_it -E 'test(live_build_context_reports_authoritative_tree_depth)' --no-fail-fast`
  - Result: 1 passed, 285 skipped.

OPEN:
- New Provider-like APIs should document cost explicitly: each provider is still an element and a map entry, even if it does not consume call stack per depth. The direct stack-safety regression test gap from `.rust-studio/research/flutter-deep-widget-stack-lessons.md` remains open.
- #1090 is the active engineering issue for inherited dependency granularity. This note does not create a second issue for the same mechanism.

NOTE: .rust-studio/research/flutter-non-ui-provider-stack-lessons.md

ANSWERED
