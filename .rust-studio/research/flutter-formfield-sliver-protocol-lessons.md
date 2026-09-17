QUESTION: Does Flutter #173912 ("FormField with Sliver causes exception") expose a comparable FLUI protocol-boundary problem?

ANSWER: Flutter #173912 is a useful protocol-boundary lesson: a generic `FormField` added a box-only `Semantics` wrapper around builder output, so a sliver-returning field crashed with a RenderBox/RenderSliver mismatch. FLUI currently avoids the exact shape because `Semantics` is explicitly a `BoxProtocol` widget, sliver wrappers are separate types, and the render layer has protocol mismatch tests. No new FLUI issue was opened. The architectural rule is to add protocol-specific wrappers (`SliverSemantics`, `SliverFormField`, etc.) rather than flags on a generic box wrapper.

VERSIONS:
- Flutter issue state checked on 2026-09-13: flutter/flutter#173912 is open, labeled `c: regression`, `framework`, `a: error message`, `has reproducible steps`, `P2`, `team-framework`, `triaged-framework`, `found in release: 3.33`, `found in release: 3.35`.
- FLUI workspace at local checkout on 2026-09-13.

SOURCES:
- GitHub issue: https://github.com/flutter/flutter/issues/173912
- Flutter #173912 body reports that `FormField.builder` returning `SliverList` crashes because the framework-added `Semantics` wrapper expects a `RenderBox` child and receives a `RenderSliverList`.
- Flutter #173912 comment https://github.com/flutter/flutter/issues/173912#issuecomment-3249228073 distinguishes the `Form`-as-sliver migration guide from the `FormField` case and points to the `FormField` semantics wrapper as the root.
- Flutter #173912 comment https://github.com/flutter/flutter/issues/173912#issuecomment-4284731132 proposes two directions: a separate `SliverFormField` using `SliverSemantics`, or an opt-in flag causing `FormField` to wrap in `SliverSemantics`; the commenter prefers the separate widget because the framework already has many protocol-specific parallels.
- `crates/flui-widgets/src/semantics/mod.rs:43-50` defines `Semantics` as a normal single-child widget.
- `crates/flui-widgets/src/semantics/mod.rs:305-343` implements `RenderView for Semantics` with `type Protocol = BoxProtocol`, so it is not protocol-polymorphic.
- `crates/flui-widgets/src/scroll/sliver_to_box_adapter.rs:1-13` documents the box-to-sliver adapter boundary explicitly.
- `crates/flui-widgets/src/scroll/sliver_to_box_adapter.rs:33-56` implements `SliverToBoxAdapter` as a `SliverProtocol` `RenderView`, separate from box wrappers.
- `crates/flui-view/src/view/render.rs:445-451` requires every `RenderView` to name its `Protocol` and a `RenderObject<Self::Protocol>`, which is the type-level boundary Flutter lacks in Dart.
- `crates/flui-rendering/tests/cross_protocol_layout.rs:277-341` covers the structural failure path when sliver layout is requested for a box child: the result is bounded and explicit rather than an unbounded retry.

VERIFICATION:
- Duplicate search: `gh issue list --repo vanyastaff/flui --state all --limit 200 --search 'FormField Sliver RenderObject parent data box sliver widget element protocol'` returned no existing FLUI issue.
- `cargo nextest run -p flui-rendering --test rendering_it -E 'test(cross_protocol_layout::)' --no-fail-fast`
  - Result: 3 passed, 341 skipped.

OPEN:
- FLUI has no shipped `FormField` in the searched widget/material sources, so there is no local user-facing FormField bug to reproduce.
- If FLUI adds form-field-like builders whose builder can return arbitrary `IntoView`, do not silently wrap output in a box-protocol widget. Either constrain the builder protocol or add a protocol-specific sliver counterpart.

NOTE: .rust-studio/research/flutter-formfield-sliver-protocol-lessons.md

ANSWERED
