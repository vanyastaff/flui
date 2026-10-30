# ADR-0204: Synthetic semantics nodes take owner-allocated ids below 2^32; text fields publish runs

- **Status:** Accepted (2026-10-12)
- **Superseded-by:** [ADR-0223](ADR-0223-authored-semantics-regions.md), §1 identity authority and authored-region action routing
- **Date:** 2026-10-12
- **Amended by:** [ADR-0214](ADR-0214-selection-area-text-runs.md) (§2: text runs for static text
  inside a `SelectionArea`)
- **Related:** [ADR-0206](ADR-0206-typed-semantics-attributes.md),
  [ADR-0203](ADR-0203-semantics-vocabulary.md),
  [ADR-0090](ADR-0090-ime-pull-text-store-contract.md) (the editing path assistive edits share),
  [ADR-0205](ADR-0205-semantics-relations-by-anchor.md); the
  [semantics-model spec](../plans/specs/semantics-model/design.md), (h) and (j).

## Context

Implementation note: this document records the text-run and announcement
contracts. Its earlier implementation status was too broad: the node model
still derived all accessible identities from render objects. ADR-0223 adds a
presentation-owned synthetic identity source for authored regions; it does not
establish completion of the text-run consumer path below. The announcer (§4) is
implemented: `Announcements` and `Announcer` in flui-semantics, kept by the pipeline and handed
to widgets through `LifecycleContext::announcer`; flui-semantics ARCHITECTURE.md records its rules.
The text-run model and its publication are implemented (`TextContent`, `TextRun`, identities
from the pipeline, the owner decoding run positions); a field that lays out its text declares it.

Two kinds of published nodes have no render object: the runs of a text field's text, which a
screen reader needs to read by character, word and line and to follow the selection, and the
nodes that carry announcements, since AccessKit 0.25 has no announcement API. An
`AccessibilityNodeId` is a packed `RenderId`, `(generation << 32) | index` with
`generation ≥ 1`, so it uses all 64 bits for render-backed nodes, but no render object ever
receives a value in `1..2^32`.

AccessKit's text model: the field node holds `text_selection`, whose positions name a
`TextRun` node and a character index; each run carries its text, per-character UTF-8 lengths
(a character is what the user perceives), positions, widths and word starts. `word_starts` is
`u8`, so a run holds at most 256 characters. AccessKit cannot request runs lazily.

Checked against the locked versions: AccessKit subtrees (`tree_id`) give a separate id space
and work on the three desktop adapters, but `TextPosition` names a run by a bare `NodeId`, which
`accesskit_consumer` 0.39 resolves in the field's own tree and unwraps
(`text.rs:22-41`, `:1448-1480`); runs in a grafted subtree would panic the consumer. Every
framework checked keeps runs as children of the field in the same tree and reserves an id range
for synthetic nodes (Chromium a negative range, Flutter ids from `1 << 16`, Slint high bits with
an index that is never reused).

## Decision

1. **Id range.** Synthetic nodes take ids in `1..2^32`, allocated by the `SemanticsOwner` that
   publishes them. A run keeps its id while it is the same run: same owner node, same
   ordinal, same starting character in the field and same text. Any other run takes the next
   value, so an edit re-mints the edited run and every later run whose start moved, and an
   outstanding `TextPosition` into a changed run is refused as stale rather than resolving to
   other text. A released value is never reissued during the owner's life. When the range is exhausted, the
   owner refuses permanently: no further synthetic node is published, and it reports
   `tracing::error!` once, queued with the publication and emitted after the owner releases its
   borrows (a subscriber is user code and may re-enter). An inbound action addressed to a synthetic id resolves to its owner
   node through the reverse map; an unknown synthetic id is refused like a stale identity.
2. **Text runs.** The render object that owns a field's layout fills its `TextContent`: runs
   split at line ends, direction changes and every 256 characters; characters are grapheme
   clusters from `flui-painting::text_boundaries`, the same segmentation the editor's caret
   and word navigation use; positions and widths come from the laid-out text; a hard line break
   is the last character of its line's run. AccessKit's per-character length is a `u8`: a
   cluster longer than 255 UTF-8 bytes is published as several characters split at `char`
   boundaries (each at most 255 bytes), so the lengths still sum to the run's byte length and
   only the published positions inside that cluster are coarser than the editor's. An obscured field publishes the mask, so its runs
   reveal only the number of characters. Runs are published as children of the field node, in
   the same tree, and only for text inputs; static text keeps its value until its cost is
   measured. An empty field publishes one empty run, so its caret has a run to name.
3. **Selection and assistive edits.** The field's selection is published as
   `text_selection` in (run, character) positions. Inbound `SetTextSelection`,
   `ReplaceSelectedText` and `SetValue` are translated to the editor's byte offsets and applied
   through the editing path the keyboard uses, with the same validation, change notifications
   and IME rules (ADR-0090). Both ends must name current runs of the targeted field (owner and
   generation), or the request is refused before conversion; a position inside a split cluster
   snaps to the cluster's boundary.
4. **Announcer.** The owner keeps two synthetic nodes, polite and assertive live regions of role
   `Label`, as the last children of the window root. `SemanticsOwner::announce(message,
   urgency)` writes the message to the matching node's value, which is what every adapter
   reacts to. An accepted message is queued before the presentation is asked for a frame, and stays
   queued until a frame publishes it, so an idle presentation still speaks; messages within one frame are coalesced, and a repeated message alternates an
   invisible separator so the adapter sees a change. Declarative live regions remain the primary
   mechanism. UI Automation notifications (`UiaRaiseNotificationEvent`, what Chromium maps
   `ariaNotify` to) are not reachable through AccessKit 0.25; that is a recorded limit, not a
   bridge FLUI builds. An announcement's urgency (`Polite`, `Assertive`) is its own type, apart
   from a node's live setting, which also has `Off`: an absent live setting inherits the
   parent's, so a `Timer` inside a polite region publishes `Off` explicitly.

## Consequences

- Narrator, VoiceOver and Orca can read a field by character, word and line and hear the
  selection; assistive edits cannot bypass the editor's rules.
- Run geometry is computed when a field's text or layout changes, not per frame; the cost is
  measured in the semantics assembly benchmark.
- The id range is a fixed 4 294 967 295 synthetic nodes per owner lifetime; exhaustion is
  terminal and observable rather than a silent collision.
- Published word starts are the editor's word stops: `flui_painting::text_boundaries::word_starts`,
  which the controller steps between for Ctrl+Arrow (`next_word_start`, `previous_word_start`)
  and `RenderEditable` publishes, so a screen reader's word stops match the editor's.

## Verification

`synthetic_ids_are_stable_never_reissued_and_refuse_at_exhaustion` (owner unit tests, including
the terminal boundary); `text_runs_follow_edits_incrementally` (including an insertion before
existing runs and an overlong grapheme cluster) and
`obscured_fields_publish_only_the_mask` through the owner's public API;
`published_word_starts_match_editor_word_stops`; `assistive_edits_use_the_keyboard_path` in the
widget suite; a `TextPattern` fixture in [`cargo xtask device windows-a11y`](https://github.com/vanyastaff/flui/blob/e8a42673ad23a2e88614940a266cd01224dcb25f/tools/xtask/src/device.rs) reading a field by
character and word and moving the selection; `announcements_reach_the_live_region_value`.
