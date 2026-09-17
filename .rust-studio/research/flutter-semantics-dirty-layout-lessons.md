# Flutter dirty-layout semantics traversal lesson

QUESTION: Does FLUI repeat Flutter flutter/flutter#191188, where `flushSemantics` can traverse into a render object that still needs layout?

ANSWER: Not for the main reported class. Flutter 3.44.0 filters layout-dirty semantics roots, but the child merge walk still asserts when a dirty child is reachable through `visitChildrenForSemantics`. FLUI's semantics assembly instead follows the same placement gate used by paint/hit-testing: a child is visited only when it was stamped as laid out by the parent in the parent's current layout generation. The normal frame pipeline also runs layout before semantics.

VERSIONS:
- Flutter reference: local `.flutter` clone verified at tag `3.44.0`.
- FLUI revision: workspace current on 2026-09-13.

SOURCES:
- Flutter issue: https://github.com/flutter/flutter/issues/191188 is open, labeled `P2`, `framework`, `a: accessibility`, `c: crash`, and contains two reproducible paths: a child no longer laid out but still returned by `visitChildrenForSemantics`, and a newly admitted semantics subtree that is still layout-dirty.
- Flutter maintainer comment in the same issue confirms the report "from source" and routes it to the accessibility team.
- `.flutter/packages/flutter/lib/src/rendering/object.dart:1451` starts `PipelineOwner.flushSemantics`; lines `1468` and `1512` filter queued semantics roots with `!object._needsLayout`, while line `5994` asserts `!childSemantics.renderObject._needsLayout` inside `_collectChildMergeUpAndSiblingGroup`.
- `crates/flui-rendering/src/pipeline/owner/construction.rs:203` through `:223` run layout, compositing, paint, then semantics in that order.
- `crates/flui-rendering/src/pipeline/owner/semantics.rs:544` through `:584` compute `parent_generation = node.layout_generation()` and skip children for which `!child.was_placed_by(id, parent_generation)` before recursing.
- `crates/flui-rendering/src/storage/state/offset.rs:116` through `:189` define the layout-generation stamp and compare both parent identity and generation.
- `crates/flui-rendering/src/pipeline/owner/paint.rs:641` through `:647` also refuses to paint a node that still needs layout, keeping visual traversal and semantics traversal aligned around clean layout.
- `cargo nextest run -p flui-rendering --test rendering_it -E 'test(semantics_assembly::) or test(placed_generation_gate::)' --no-fail-fast` passed 39/39 selected tests, 305 skipped.

OPEN:
- I did not file a FLUI issue from this Flutter case. The architecture already has the key missing invariant: traversal uses a current-layout placement stamp, not only structural child membership.
- There is still a narrow test-hardening opportunity: the selected suite proves semantics assembly and placed-generation behavior separately, while paint/hit-test have explicit tests for a child dropped from a later layout pass. A direct semantics regression test for "dropped child is not announced" would be useful, but absent current source evidence that FLUI publishes stale semantics, this is not issue-worthy by itself.

ANSWERED
