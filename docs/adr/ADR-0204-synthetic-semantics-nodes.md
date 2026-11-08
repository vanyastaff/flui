# ADR-0204: Synthetic semantics nodes use tagged generational ids; text fields publish runs

- **Status:** Accepted (2026-10-12)
- **Superseded-by:** [ADR-0223](ADR-0223-authored-semantics-regions.md), §1 identity authority and authored-region action routing; [ADR-0314](ADR-0314-synthetic-semantics-ids-use-a-tagged-generational-slot.md), §1 id encoding and release policy
- **Date:** 2026-10-12
- **Amended by:** [ADR-0214](ADR-0214-selection-area-text-runs.md) (§2: text runs for static text
  inside a `SelectionArea`); amended 2026-10-31 (§5: route changes)
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
`AccessibilityNodeId` is 64-bit at the adapter boundary. ADR-0314 reserves its top bit for
synthetic nodes and packs a 31-bit generation and 32-bit slot below it; render-backed nodes keep
the same layout with a zero domain bit.

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

1. **Id range.** Synthetic nodes use tagged generational slots under ADR-0314. The pipeline's
   presentation-local source allocates them; the top published bit distinguishes them from
   render-backed nodes. A run's identity is its owning field and ordinal. Text, starting byte and
   geometry are payload: a same-count edit keeps every run id, including when an insertion moves
   later runs. Removing an ordinal releases its slot; a later run at that ordinal receives the
   recycled slot only under a new generation, and a terminal slot closes. When no slot is
   available, the affected synthetic content is withheld and the owner reports `tracing::error!`
   once, queued with the publication and emitted after the owner releases its borrows (a
   subscriber is user code and may re-enter). An inbound action addressed to a synthetic id
   resolves to its owner node through the reverse map; an unknown or stale synthetic id is refused
   like a stale identity. An outstanding `TextPosition` is decoded only while the current runs and
   the last delivered runs have the same text and UTF-8 character lengths. An edit that has not
   been delivered therefore refuses the old position; successful delivery makes the stable id and
   its new coordinates current.
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
   and IME rules (ADR-0090). Both selection ends must name current runs of the targeted field
   (owner and generation), and the text coordinates must agree with the last delivery, or the
   request is refused before conversion; a position inside a split cluster snaps to the cluster's
   boundary.
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
5. **Route changes.** A route is a node that scopes one (`scopes_route`) or a modal `Dialog` or
   `AlertDialog`; the innermost is the last such rooted node in reading order. Its name is the
   label of the first node in its scope that names the route (`names_route`), else, for a modal
   dialog, its own label or the labels of the nodes it is labelled by. `SemanticsOwner::flush`
   raises one `RouteChange` (route, name, `RouteChangeKind::Pushed`, `Popped` or `Replaced`)
   when the innermost route's identity differs from the one the last delivered update showed:
   never for the first route a tree publishes, nor for a republish of the same route, and once
   for several changes inside one flush. An adapter states its `RouteChangeSupport`. Where it is
   `Native` (the platform has a notice of a new screen: iOS
   `UIAccessibilityScreenChangedNotification` with the name as the string it speaks, Android `TYPE_WINDOW_STATE_CHANGED`
   with the name as its text), the composition root installs a sink that calls
   `PlatformAccessibility::route_changed` after the update showing the route is delivered; a
   panic in it is contained and nothing is spoken in its place. Where it is `Announced` (AT-SPI,
   UI Automation, NSAccessibility and the Web have no such notice), the polite announcer (§4)
   speaks the name in the update that shows the route, and in that update only; a route nothing
   names says nothing. The owner does not move focus into the route: that is the focus system's.
   An undelivered update leaves the route change to its retry, which tells the route the tree
   then shows. Only published nodes count: a route under a node with no identity is not told, and
   an unpublished dialog title names nothing.

## Consequences

- Narrator, VoiceOver and Orca can read a field by character, word and line and hear the
  selection; assistive edits cannot bypass the editor's rules.
- Run geometry is computed when a field's text or layout changes, not per frame; the cost is
  measured in the semantics assembly benchmark.
- A screen reader hears a pushed, popped or replaced page and a modal dialog by name, on platforms with a native notice through it and elsewhere as a polite announcement.
- Synthetic slots recycle only under a new generation; per-slot terminal exhaustion is observable
  rather than a silent collision, and ordinary churn has bounded slot consumption.
- Published word starts are the editor's word stops: `flui_painting::text_boundaries::word_starts`,
  which the controller steps between for Ctrl+Arrow (`next_word_start`, `previous_word_start`)
  and `RenderEditable` publishes, so a screen reader's word stops match the editor's.

## Verification

`tagged_synthetic_ids_cannot_alias_render_ids`, `released_slots_advance_and_stale_releases_do_nothing`
and `synthetic_identity_exhaustion_never_wraps_or_partially_reserves` (allocator unit tests,
including the terminal boundary); `text_runs_follow_edits_incrementally` and
`text_positions_wait_for_their_text_to_be_delivered` through the owner's public API;
`a_fields_runs_keep_their_identities_through_text_edits`, `a_long_cluster_and_a_long_line_read_whole`,
`a_password_field_publishes_only_its_mask`, `a_screen_reader_moves_by_word_where_the_caret_steps`
and `a_screen_reader_types_into_an_empty_field_and_selects_in_it` in the widget suite; a `TextPattern` fixture in [`cargo xtask device windows-a11y`](https://github.com/vanyastaff/flui/blob/e8a42673ad23a2e88614940a266cd01224dcb25f/tools/xtask/src/device.rs) reading a field by
character and word and moving the selection; `announcements_reach_the_live_region_value`; `each_change_of_the_innermost_route_is_told_once`, `without_a_sink_a_named_route_is_announced`, `a_failed_sink_or_delivery_loses_no_change_and_speaks_once` and `a_new_adapter_hears_changes_from_the_route_it_was_handed` through the owner's public API; `a_named_page_is_announced_when_pushed_and_when_uncovered` in the widget suite; `a_route_change_is_heard_by_its_name` in `linux_atspi`, a real AT-SPI client hearing a push and a pop; and `each_native_adapter_says_how_a_route_change_is_told`, which pins each adapter's `RouteChangeSupport`.
