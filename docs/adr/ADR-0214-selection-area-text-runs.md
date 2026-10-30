# ADR-0214: Static text inside a `SelectionArea` publishes text runs

- **Status:** Accepted (2026-10-13); implemented (2026-10-30): the semantics model, `RenderParagraph::set_selectable` and the assistive path of `SelectionArea`, with the rows below in `crates/flui-widgets/tests/selection_area.rs`. Pointer, keyboard and handles follow in docs/plans/specs/flui-interaction/completeness/tasks.md (T34). The native `TextPattern`, VoiceOver and Orca rows are still to run on a host. A paragraph with routed links keeps its link nodes and publishes no runs yet (row `links_in_a_selection_area_keep_their_own_nodes`); links carrying runs of their own are tracked in T34. Until pointer selection lands, the area orders its texts by place in the tree, the order the semantics assembly publishes; geometric reading order then replaces it for the area and the assembly together, so nothing should depend on tree order.
- **Date:** 2026-10-13
- **Amends:** [ADR-0204](ADR-0204-synthetic-semantics-nodes.md) §2, whose "only for text
  inputs; static text keeps its value until its cost is measured" now has one exception.
- **Related:** [ADR-0203](ADR-0203-semantics-vocabulary.md) (`SetTextSelection`, `Document`),
  [ADR-0208](ADR-0208-gesture-ownership-table-and-composition.md) (selection handles are
  `Sequenced`), [ADR-0090](ADR-0090-ime-pull-text-store-contract.md); the
  [semantics-model spec](../plans/specs/semantics-model/design.md), (h); the
  [interaction completeness spec](../plans/specs/flui-interaction/completeness/design.md) §13, §14.

## Context

`SelectionArea` makes static text selectable by pointer, keyboard and assistive technology
together. A screen reader selects by sending `SetTextSelection` with two (run, character)
positions, and AccessKit can only address characters through `TextRun` nodes: there is no lazy
request for runs. ADR-0204 §2 publishes runs only for text inputs, so the AT path of a
`SelectionArea` has nothing to address: the user can read the text but not select it.

`accesskit_consumer` 0.39.1 resolves text ranges only on a text input or a `Label`, `Document`
or `Terminal` node that has runs (`supports_text_ranges`, `text.rs:1402-1406`), and gathers
runs through intermediate nodes (`text_node_filter`, `text.rs:932-938`). So runs may sit under
each paragraph while one ancestor holds a selection spanning several paragraphs.

## Decision

1. **Scope.** Text that a `SelectionArea` registers as selectable publishes runs. Nothing else
   changes for static text. Such text is registered as a nested merger (it merges its own
   descendants), which the assembly rule of ADR-0206 decision 4 (`assembly_decisions` in
   `flui-rendering`'s semantics owner) makes a node of its own even inside an outer merge; a plain
   semantics boundary would not, because a boundary under a merging ancestor is absorbed. So
   the paragraph is never merged into an ancestor's label, and its runs (ADR-0204 synthetic
   children) always have a node of their own, even under a merging ancestor.
2. **Runs.** Each such paragraph's node publishes runs as its children, built exactly as
   ADR-0204 §2 builds a field's: split at line ends, direction changes and every 256
   characters; grapheme clusters and word starts from `flui-painting::text_boundaries`;
   positions and widths from the laid-out text; runs of one visual line linked to each other.
   Runs are synthetic children (ADR-0204), and the semantics assembly keeps them as nodes beneath
   their paragraph: `merge_child_fragments` in `flui-rendering`'s semantics owner excludes
   synthetic run children from absorption, so a nested-merger paragraph keeps its run nodes. This
   is an assembly-rule change made with this record; row
   `nested_merger_paragraph_keeps_its_run_nodes`.
   Run ids come from the same owner allocator with the same identity and exhaustion rules
   (ADR-0204 §1). Runs are rebuilt when the paragraph's text or layout changes, never per frame,
   and only while the semantics tree is enabled.
3. **Selection.** The `SelectionArea` itself is a merge boundary, not a merger. That is a distinct, typed input to the assembly: `SemanticsConfiguration` gains a `merge_boundary` property (`is_merge_boundary`/`set_merge_boundary`), separate from `is_semantics_boundary` (which an outer merger absorbs) and from `is_merging_semantics_of_descendants` (which would merge the area's controls); the area sets it, `assembly_decisions` reads it, and no role or widget identity is consulted. Toward an outer merging ancestor the assembly treats a merge boundary as it treats a nested merger (the area keeps its own node and its subtree is not absorbed upward), but it sets no `merge_into_ancestor` on its descendants, so a button or link inside the area keeps its own addressable node beside the paragraph and run hierarchy. This is the second assembly-rule change made with this record (in `assembly_decisions`); rows `nested_merger_paragraph_keeps_its_run_nodes` and `a_control_inside_a_selection_area_under_an_outer_merger_keeps_its_node`. Its own node takes the `Document` role unless its author
   declared another one that supports text ranges, and publishes the area's current selection
   as `text_selection` in (run, character) positions. A selection inside one paragraph is
   published the same way; the paragraph node also supports ranges and reads by itself.
4. **Inbound `SetTextSelection`.** Accepted on the area's node and on any paragraph node under
   it, routed to the area. Both ends must name current runs inside that area (owner node and
   id generation) or the request is refused before conversion, as ADR-0204 §3 refuses a stale
   end; a position inside a split cluster snaps to the cluster boundary. The positions are
   converted to the area's selection model and applied through the path pointer and keyboard
   selection use, with the same change notifications. `ReplaceSelectedText` and `SetValue` are
   not offered: the text is not editable.
5. **Unchanged outside a `SelectionArea`.** Static text publishes its value as today: no runs,
   no `text_selection`, no `SetTextSelection`, and no synthetic ids drawn from the owner's
   range.

## Consequences and verification

- Narrator, VoiceOver and Orca can read selectable text by character and word and move its
  selection; static text elsewhere costs exactly what it costs now.
- A `SelectionArea` over long text multiplies semantics nodes by about one per 256 characters
  per line; the cost is measured, not assumed.
- Tests through public API: `selection_area_text_publishes_runs` (several paragraphs, an RTL
  line, a line longer than 256 characters read as one line);
  `selection_area_accepts_assistive_selection` (a range inside one paragraph and one spanning
  two, through `accesskit_consumer`); `selection_area_refuses_stale_or_foreign_positions` (an end
  in another area, a mix of current and stale ends);
  `selectable_text_under_a_merging_ancestor_keeps_its_runs` (runs exposed and
  `SetTextSelection` accepted); `static_text_outside_selection_area_publishes_no_runs`.
- Benchmark: a `selection_area_runs` row in the semantics benchmark
  (`crates/flui-semantics/benches/publish_cost.rs`, group `semantics_publish`) with the existing
  sizes and idle, one-dirty and all-dirty cases; the existing rows, which have no runs, are the
  baseline that must not move.
- CI executes the tests through `accesskit_consumer` and runs the benchmark build. A screen
  reader reading and selecting the text runs on a native host only: a `TextPattern` row in
  [`cargo xtask device windows-a11y`](https://github.com/vanyastaff/flui/blob/e8a42673ad23a2e88614940a266cd01224dcb25f/tools/xtask/src/device.rs), VoiceOver and Orca by hand, each recorded in the PR.
- Native Orca row: a `SelectionArea` with the `Document` role must not switch Orca into a browse
  mode that takes arrow keys away from the controls inside it. If it does, the area wraps text
  only and keeps controls outside its `Document` node.

## Alternatives rejected

- **Runs for all static text.** AccessKit cannot fetch them lazily, so every paragraph would pay
  for a capability only selectable text can use.
- **Runs in an AccessKit subtree.** `accesskit_consumer` resolves a `TextPosition` in the node's
  own tree and panics on a run in a grafted subtree (ADR-0204, Context).
- **Selection only within one paragraph.** A sighted user can select across paragraphs; an AT
  user must be able to as well.
