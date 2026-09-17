# Flutter build-phase geometry access lesson

QUESTION: What should FLUI learn from Flutter issues where app code reads `RenderBox.size` / `localToGlobal` during build and later crashes after reparenting or layout invalidation?

ANSWER: FLUI already encodes the stronger shape those Flutter comments point toward: geometry/transform access is not a casual `BuildContext` read. `BuildContext::pipeline_owner()` is documented as a frame capability, acquisition is forbidden inside `build`/layout/paint by port-check trigger #22, and the sanctioned path is lifecycle acquisition plus post-frame or other later callback use after layout commits.

VERSIONS:
- Flutter issue/source read on 2026-09-13; local Flutter reference remains pinned at `3.44.0` for source comparisons where applicable.
- FLUI revision: workspace current on 2026-09-13.

SOURCES:
- Flutter issue https://github.com/flutter/flutter/issues/171425 is open, labeled `P2`, `framework`, `c: regression`, `a: error message`, and shows a production app pattern crashing with `RenderBox was not laid out`.
- The Flutter maintainer comments in #171425 identify the deeper contract: `RenderBox.hasSize` only proves the object was laid out before; it does not prove all ancestors have current paint transforms, especially during build or after GlobalKey reparenting. The thread points users toward post-frame callbacks or purpose-built layout/overlay APIs instead of synchronous build-phase geometry reads.
- `crates/flui-view/src/context/build_context.rs:367` through `:402` documents `BuildContext::pipeline_owner()` as the way to resolve a `RenderId` to geometry, but explicitly classifies it as a frame capability: acquire in `init_state` / `did_change_dependencies`, store it, and use it later from a callback, never directly inside `build`/layout/paint.
- `scripts/check-frame-capability-scope.sh:45` through `:58` names `pipeline_owner()` in trigger #22 and explains the mid-transaction reentry hazard.
- `scripts/check-frame-capability-scope.sh:97` through `:103` includes `pipeline_owner` in the guarded token list scanned inside `build`, `perform_layout`, `paint`, and related frame-phase bodies.
- `crates/flui-app/src/app/ui_realm.rs:5688` through `:5775` contains a production-path acceptance test proving an owner-local post-frame callback observes the same frame's committed layout.

VERIFICATION:
- `scripts/check-frame-capability-scope.sh --self-test` passed: the rejected fixture reports `pipeline_owner()` among the capability violations, and the accepted fixture passes.
- `scripts/check-frame-capability-scope.sh crates` passed with no violations.

OPEN:
- No FLUI issue filed. The Flutter thread reinforces an existing FLUI invariant rather than exposing a missing one.
- Future API design should avoid adding "maybe position from BuildContext" convenience helpers unless they are phase-scoped, return explicitly stale snapshots, or are backed by a post-frame/layout observer primitive. Otherwise FLUI would recreate the same social API trap Flutter now carries.

ANSWERED
