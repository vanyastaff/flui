# Flutter Cacheable Hot-Path Lessons for FLUI

Date: 2026-09-13

Scope: Flutter #190187 and local FLUI inherited/rendering source. Goal: learn
from Flutter's active framework performance debt without assuming FLUI has the
same implementation problem.

## Sources Read

- Flutter #190187, open P2 framework performance proposal:
  `BoxDecoration` shadow paint allocation and `InheritedModel.inheritFrom`
  repeated work on hot paths.
- FLUI source:
  - `crates/flui-view/src/context/element_build_context.rs`
  - `crates/flui-view/src/element/behavior.rs`
  - `crates/flui-material/src/theme.rs`
  - `crates/flui-material/src/theme_data.rs`
  - `crates/flui-widgets/src/app/media_query.rs`
  - `docs/adr/ADR-0008-flui-view-leapfrog-buildcontext-inherited-element.md`
- FLUI tests:
  - `crates/flui-material/tests/rebuild_exactness.rs`

## Findings

### Inherited Lookup

Flutter #190187 names `InheritedModel.inheritFrom` as hot-path work. FLUI does
not repeat the ancestor-walk problem: `ElementBuildContext::find_inherited_provider`
uses the building element's resolved inherited map, and the code documents this
as O(1). This is the correct foundation.

The remaining FLUI problem is dependency granularity after lookup:

- `InheritedBehavior` stores `HashMap<ElementId, usize>` where the value is the
  dependent's depth.
- On provider update, `InheritedBehavior::on_view_updated` schedules every
  dependent when `update_should_notify` is true.
- `Theme::of` and `MediaQuery::of` clone whole data snapshots.
- `Theme` / `MediaQuery` update notifications compare the whole data object.

So FLUI has provider-scoped invalidation but not field-scoped invalidation.

Executed:

```text
cargo nextest run -p flui-material --test rebuild_exactness --no-fail-fast
1 test run: 1 passed, 0 skipped
```

This proves non-dependents are not rebuilt by `Theme`, but it does not prove
that two different `Theme` consumers can subscribe to different fields. Current
source shows they cannot.

Filed:

- <https://github.com/vanyastaff/flui/issues/1090> — `view: add
  field-granular inherited dependencies before Theme and MediaQuery APIs ossify`

### Paint Shadow Caching

Flutter #190187 also names `BoxDecoration` shadow-paint allocation. FLUI's
pipeline is not a direct match: `BoxDecoration` emits display-list
`DrawShadow` commands, and the engine batches shadow instances rather than
creating Flutter `Paint` + native `MaskFilter` objects. I did not file a FLUI
issue from the Flutter shadow item in this pass because the implementation
shape is materially different and no local hot-path allocation proof was
gathered.

Relevant local files inspected:

- `crates/flui-painting/src/decoration.rs`
- `crates/flui-painting/src/display_list/command.rs`
- `crates/flui-engine/src/wgpu/effects.rs`
- `crates/flui-engine/src/wgpu/batches/paths.rs`
- `crates/flui-engine/src/wgpu/replay/flush.rs`

## Lesson

External issues are most useful when translated into FLUI's actual architecture.
Here the lesson is not “copy Flutter's `InheritedModel` optimization” and not
“cache Dart `Paint` objects.” The durable rule is:

> Hot build/paint paths need stable representations of reusable work and
> precise dependency registration before catalog APIs make broad reads normal.

For FLUI, the inherited half is actionable now because ADR-0008 already defined
the field-mask design and current `Theme`/`MediaQuery` APIs are still young.

Status: ANSWERED for Flutter #190187 mapping; shadow hot-path allocation remains
unfiled pending local measurement.
