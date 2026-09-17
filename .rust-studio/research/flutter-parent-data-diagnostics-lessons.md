QUESTION: What architectural lesson should FLUI take from Flutter issue #108186 around ParentDataWidget diagnostics?

ANSWER: Parent-data compatibility is a user-facing widget/element contract, not an internal render-protocol invariant. FLUI currently allows mismatched parent data such as Expanded-under-Stack or Positioned-under-Row to reach layout, where it panics from BoxLayoutCtx::from_erased with TypeId details instead of reporting which ParentDataView requires which render ancestor.

VERSIONS: FLUI workspace at 2026-09-14; Flutter reference clone pinned at 3.44.0.

SOURCES:
- Flutter `ParentDataWidget.debugIsValidRenderObject`, ancestor descriptions, ownership-chain diagnostics, and `_updateParentData`: `.flutter/packages/flutter/lib/src/widgets/framework.dart:1595`, `.flutter/packages/flutter/lib/src/widgets/framework.dart:1644`, `.flutter/packages/flutter/lib/src/widgets/framework.dart:6876`.
- Flutter issue #108186 comments: users reported debug visuals seeming fine while release/profile failed; framework maintainers traced the core problem to parent-data errors being hidden or surfacing inconsistently.
- FLUI `ElementTree::apply_ancestor_parent_data` applies the nearest parent-data config without validating compatibility against the render parent contract before layout: `crates/flui-view/src/tree/element_tree.rs:1134`.
- FLUI `ParentDataBehavior::apply_parent_data_config` treats a mismatch as a `BUG:` downcast panic: `crates/flui-view/src/element/behavior.rs:578`.
- FLUI `BoxLayoutCtx::from_erased` has a debug assertion for parent-data TypeId mismatch at render layout context construction: `crates/flui-rendering/src/protocol/box_protocol.rs:573`.
- Temporary probes showed `Expanded` under `Stack` and `Positioned` under `Row` panic during layout through `BoxLayoutCtx::from_erased`, then the test bootstrap reports a secondary `InvalidGeometry` panic.

OPEN: Exact API shape for typed expected-ancestor metadata is undecided; possible solutions include explicit ParentDataView metadata, render-parent parent-data acceptance metadata, or a checked compatibility query in the element/render attach seam.

NOTE: .rust-studio/research/flutter-parent-data-diagnostics-lessons.md

ANSWERED
