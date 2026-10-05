# flui-semantics architecture

The fifth tree: the accessibility tree assembled from the render tree and
exported to the platform through AccessKit. Layer rules and the crate's place in
the workspace DAG live in [`docs/FOUNDATIONS.md`](../../docs/FOUNDATIONS.md) and
this crate's `[package.metadata.flui] layer`; this file
records the per-decision rules that would otherwise read as drift.

Cross-crate protocol decisions belong in an ADR (`docs/adr/`). What belongs here
is a decision local to this crate: a translation rule, a precedence, a payload
shape.

## Mapping decisions

### Failed incremental publication keeps delivery pending

The published-node mirror and published focus advance only after the platform
callback returns successfully. A callback panic propagates to the caller; dirty
nodes remain pending, so retrying the same input delivers its changed payload
and focus. Removal bookkeeping can prune absent identities before delivery:
the dirty parent's changed child list remains different from the delivered
mirror and therefore retries. Focus claimant bookkeeping describes current
tree state rather than delivered state and may likewise advance before delivery.
The callback may have accepted an update before panicking, so retries can repeat
delivery; this boundary promises progress, not exactly-once delivery.
`failed_incremental_delivery_preserves_retry_and_progress` checks failed label,
focus, and removal updates, a retry without further mutation, an idle flush,
and a subsequent independent change.

### Parent-specific detachment preserves both sides of a link

`SemanticsTree::remove_child` detaches only a child whose current parent is the
live parent supplied by the caller. A stale or different parent is a no-op;
clearing the child's actual parent in that case would leave its old parent's
children list pointing at it and defeat `add_child`'s automatic reparenting.
`detaching_checks_the_actual_parent_and_preserves_reparenting` exercises the
owner wrapper, refused detachment, reparenting, published parent child lists,
and a successful detach followed by reattachment.

### 1. Role resolution is a single-valued specificity cascade, and checkable state outranks the broad `IsButton`

**Rule:** a semantics node carries a *set* of flags, and a radio tile, for example,
carries `IsButton`, `HasCheckedState`, `HasEnabledState`, `IsInMutuallyExclusiveGroup`,
`IsFocusable` and `HasSelectedState` together. The shape FLUI exports into is
single-valued, so the precedence below is a rule FLUI has to define.

**Choice:** AccessKit's `Node::role` is one `Role`, so `resolve_role` picks
exactly one from the union of flags the node carries, in this order:

```
HasToggledState                        → Role::Switch
HasCheckedState + IsInMutuallyExclusiveGroup → Role::RadioButton
HasCheckedState                        → Role::CheckBox
IsButton                               → Role::Button
IsLink, IsSlider, IsTextField, …       → their own roles
IsImage, IsHeader                      → their own roles
otherwise                              → Role::GenericContainer
```

**Why the checkable states are tested first.** They are strictly more specific
than `IsButton`: a control that reports a checked or toggled state *is* a
checkable, whatever else it also is. This is not a stylistic preference, because
of how flags arrive:

**Flags from an annotated, non-boundary ancestor can absorb into the
descendant's node**, so one node can arrive here carrying both an ancestor's
`IsButton` and a descendant's checkable flags. Measured, the shape
`Semantics::new().button(true).child(Radio::new(..))` is exactly that: its node
resolves `Button` under the old order and `RadioButton` under this one, with the
same node id either way. That composition is reachable through the public API —
the `Semantics` widget and its `button` builder are both public. The other
composition this precedence is measured to affect is
`MergeSemantics` over a `ListTile` carrying a `Radio`, the shape of a radio list
tile whose whole tile is a single interactive entity. Measured through `packages/flui-material/tests/list_tile.rs`'s fixture, that tree
exports one merged node, and it resolves `Button` under the old order and
`RadioButton` under this one.

**The merge is conditional, and the condition is the part a reader gets wrong.**
`is_compatible_with` treats *any* overlapping flag bit as a conflict, and
`mark_configuration_conflicts` then forces that descendant into a node of its
own. Both `ListTile` and `Radio` set `HasEnabledState` unconditionally
(`ListTile::build` publishes `.button(on_tap.is_some()).selected(..).enabled(..)`,
`Radio::build` publishes `.enabled(interactive)`), so the two configurations
conflict and a `Radio` inside a real `ListTile` does **not** merge into the
tile's node. Measured through `packages/flui-material/tests/list_tile.rs`'s own
`MediaQuery`-wrapped fixture, that composition exports
`[GenericContainer, Button, RadioButton]`: because the two do **not** merge, the
radio keeps a node of its own, and that node **announces as a radio**. A `Radio`
in a real `ListTile` therefore does announce as one.

**That fix is the widget's, not this crate's.** The radio's own node carries no
`IsButton`, so it resolves `RadioButton` under either arm order — measured, the
composition exports the same roles with this cascade reverted — and the
precedence above is not load-bearing for it. What makes the node announce as a
radio is `Radio` publishing `.in_mutually_exclusive_group(true)`, the
`flui-material` half of the same change.

**The figure this entry used to carry was an artifact of a mount that panicked.**
An earlier draft recorded the tile composition as `[GenericContainer, Button]`
"with no `RadioButton` node at all", and read that as the tile losing the radio.
It was produced by mounting a `ListTile` with no ambient `MediaQuery`:
`ListTile::build` composes `SafeArea`, which reads `MediaQuery::of` and **panics**
when no ancestor provides one, and the widget-layer harness installs none. The
panic is contained, the test still **passes**, the tile degrades, and the `Radio`
below `SafeArea` never mounts — so the roles that came back belonged to the live
tile and to nothing else. `packages/flui-material/tests/list_tile.rs` mounts every
tile under a default `MediaQueryData` for exactly this reason. Removing the
group flag, which turns the radio's node into a `CheckBox`, is the only thing
that changes the answer.

A bare `ListTile` with a `Radio` splits into separate nodes; the like-for-like
composition of one interactive entity is
`MergeSemantics::new().child(ListTile…Radio)`, which exports **one** node with
role `RadioButton`.

What the merged node still lacks is named rather than left to be discovered: it
carries **no label** (`Text` → `RenderParagraph` publishes no semantics — a
deferral already recorded in `crates/flui-objects/src/text/paragraph.rs`'s module
doc, so the tile's `title` cannot label it) and **no tap action** (FLUI's ink
response does not publish `Semantics(onTap: ..)` itself — see
`packages/flui-material/ARCHITECTURE.md`). Role matches; label and action are the
recorded gaps.

**Consequences, named rather than left to be discovered:**

- **`IsButton` is now the weakest of the interactive roles, and that is a
  behaviour change for any node that carries a checkable flag beside it.** A
  widget that deliberately publishes both — a toggle styled as a button — now
  resolves to `Switch`/`CheckBox`. That is the correct reading of the flag set,
  but it is a change to existing output rather than a new capability.
- **The cascade is still lossy, and cannot be made lossless here.** Any node
  carrying two unrelated roles in its flag set loses one, because AccessKit's
  role is single-valued. The mitigation is
  order-of-specificity, not completeness — a genuinely two-role node has no
  correct single answer, only a defensible one.
- **The order is load-bearing and is not obvious from any single flag's
  meaning**, so it is recorded here rather than left to the comment on the
  function: reordering the arms is a silent behavioural change that no
  type-checker and no single-flag test can catch.

**Replacement tests:**

- `packages/flui-material/tests/list_tile.rs` —
  `merge_semantics_over_a_tile_and_radio_announces_as_one_radio_button` is the pin
  for the `RadioButton` arm: its merged node carries the tile's `IsButton` beside
  the radio's checkable flags, and it resolves `Button` once the checkable arms
  move back below `IsButton`. `crates/flui-semantics/src/agent/tests.rs` —
  `every_role_bearing_flag_reads_as_the_role_uia_reports` sets only the checkable
  flags, so it passes under either order and cannot pin this. The `CheckBox` and
  `Switch` arms beside `IsButton`, and the *other* half of the cascade — the two
  arms this reorder deliberately left below `IsButton` (see mapping decision 2):
  **Unasserted:** no test pins this.
- `packages/flui-material/tests/radio.rs` —
  `a_mounted_radio_announces_as_a_radio_button` pins the direct case, and with it
  the *widget* half of the change: without `Radio`'s group flag the node resolves
  `CheckBox`. The composition absorbed under an annotated ancestor
  (`Semantics::new().button(true).child(Radio::new(..))`), a radio without a tap
  handler, and the bare-tile composition a user actually writes (`Button` from the
  tile's own tap target, `RadioButton` from the radio's separate node):
  **Unasserted:** no test pins this.
- The group flag staying off other checkables (a checkbox is a `CheckBox` and
  not a `RadioButton`): **Unasserted:** no test pins this. At the flag level,
  `every_role_bearing_flag_reads_as_the_role_uia_reports` resolves
  `HasCheckedState` alone to `CheckBox` and beside the group flag to
  `RadioButton`, so the group flag is what distinguishes them rather than checked
  state alone.

### 2. `IsButton` outranks `IsLink` and `IsTextField`

**Rule:** the cascade in mapping decision 1 leaves `IsLink`, `IsTextField`, `IsSlider`,
`IsImage` and `IsHeader` *below* `IsButton`, so a node carrying `IsButton` beside one of them
loses that other role. The collision is real: a dropdown's text field with a trailing button
carries `IsTextField` and `IsButton` together, and a linked button carries `IsButton` and
`IsLink`. `Role` is single-valued where the flag set is not, so a node reported as `Button`
may also be a link or a text field.

**Choice:** keep `IsButton` above them. This precedence predates the checkable reorder in
decision 1, which moved only the checkable states; the behaviour is unchanged and deliberate,
and this entry records it.

**Why it is not merely theoretical.** `Semantics::new().button(true).link(true)`
and `Semantics::new().button(true).text_field(true)` both compile today and each
resolves `Role::Button`, which makes the `IsTextField` arm — the one carrying
`MultilineTextInput` / `PasswordInput` / `TextInput`, a distinction a screen
reader announces — unreachable for such a node. A widget that publishes both
intends both.

**Unasserted:** no test pins this.

### 3. Semantics assembly is mark-scoped with a whole-tree fallback, and the tree reaches the OS through AccessKit

**Rule.** The `SemanticsOwner` lives on `PipelineOwner` as `Option<SemanticsOwner>`, created
when semantics is enabled and disposed when it is disabled (firing the owner-created/disposed
notifier hooks). Assembly runs in the existing post-paint semantics phase (`run_semantics`,
`crates/flui-rendering/src/pipeline/owner/semantics.rs`):

- Each render object describes itself through `describe_semantics_configuration`; non-boundary
  configuration merges up into the nearest node-forming ancestor with
  `SemanticsConfiguration::absorb`; `is_merging_semantics_of_descendants` collapses a subtree
  into one node; `excludes_semantics_subtree` and a per-child `visits_child_for_semantics`
  drop children from the walk.
- Geometry reuses the paint walk's inputs — the committed `RenderNode::offset()` and
  `paint_transform()` — plus the parent's semantics clip (`describe_semantics_clip`), so there
  is no second source of node geometry.
- A pass re-assembles only the subtrees that can observe the frame's semantics marks, grafting
  each into the persistent semantics arena under its **anchor** — the nearest unmarked ancestor
  that formed a node last pass. A formed node absorbs every pending fragment below it and its
  own forming decision does not depend on its descendants, so re-assembling the anchor's subtree
  reproduces everything a mark could influence. Anything the graft preconditions cannot prove
  falls back to the whole-tree rebuild, which is the correctness baseline.
- `accesskit_translation.rs` maps the owner's tree to AccessKit updates with stable
  `AccessibilityNodeId`s; `flui-platform` hosts the AccessKit adapters per backend.
- `platform.rs` holds `PlatformAccessibility`, the capability those adapters implement and the
  realm holds per window (ADR-0082 §2). It names only AccessKit types, never a semantics type:
  what crosses it is already translated.

**Incrementality.** The observable `SemanticsNode` tree is the contract; incrementality is
anchor-scoped re-assembly over the classic merge model, not a per-node fragment state machine.
Freshness is mark-driven: geometry outside re-assembled subtrees keeps its last published
value.

**Not implemented.** `RenderBlockSemantics` / blocking of previously painted siblings; sibling
merge groups beyond configuration-conflict marking.

### 4. Static text carries its text as its AccessKit value, and a value it also has joins it

**Rule.** A node that resolves to `Role::Label` (mapping decision 1's static text) publishes
its label as the AccessKit label *and* as the value; a node that also carries a FLUI value
publishes `"{label}\n{value}"` as the value. Every other role publishes label and value as
they are.

**Why.** AccessKit's contract for `Label` is that its text is its value
(`accesskit_consumer::Node::label_comes_from_value`): the UI Automation and AT-SPI adapters
take a label node's name from the value alone, so a text published only as a label had an
empty name for Narrator and Orca. The AppKit adapter falls back to the label, which is why
VoiceOver read it and the macOS check passed. Keeping the label as well leaves queries by
label (`flui::testing::a11y`, the harnesses) unchanged.

AccessKit's `Label` has one text slot on UIA and AT-SPI, so the label and the value are
joined with a newline, the same separator used when merged labels are concatenated.

**Test.** `advertised_actions_follow_the_uia_patterns` in `src/agent/tests.rs` reads a static
text's name through `accesskit_consumer` the way the adapters do; `cargo xtask device
windows-a11y` is the live check that found the defect. For the joined label and value:
**Unasserted:** no test pins this.

### 5. The ADR-0080 tools reach FLUI through AccessKit's Windows adapter: `expand`/`collapse` are discrete, with a tap fallback

**Rule.** `semantics_action_for` names every `accesskit::Action`. The tools an agent calls
(ADR-0080, `flui_protocol::ActionName`) arrive through accesskit_windows 0.35.0 as:
`invoke`, `toggle` and `select` → `Click` → `Tap`; `set_value` → `SetValue` → `SetText`, or
`SetNumericValue` with its number when the request carries `ActionData::NumericValue` (a
slider through UI Automation's `RangeValue`; `semantics_action_request_for`) or, on the wire,
when the node publishes a numeric value; `focus` → `Focus`; `scroll_into_view` →
`ScrollIntoView` → `ShowOnScreen`; `expand` and `collapse` → `Expand`/`Collapse` →
`SemanticsAction::Expand`/`Collapse` (ADR-0124).

A node with `HasExpandedState` advertises `Expand` while collapsed and `Collapse` while
expanded, never both. It advertises the transition when it registers the matching discrete
action; when it registers neither discrete action but has a tap handler (the shape
`Semantics::new().expanded(false).on_tap(..)` builds), it advertises the transition too, and
`SemanticsOwner::resolve_action` routes that request to the tap handler as `Tap`, only in the
direction the node's current state allows. `tap_disclosure_transition` in `src/action.rs` is
the one predicate both directions use.

**Why.** AccessKit does not count a node with an expanded state as invocable
(`accesskit_consumer` 0.39, `Node::is_invocable`), so the Windows adapter offers only the
`ExpandCollapse` pattern for it: without the fallback a tap-only expandable node could be
neither invoked nor expanded by an agent or a screen reader. A discrete handler receives the
requested direction and sets that state, so repeated requests are idempotent. No other shipped
adapter (`accesskit_macos` 0.27, `accesskit_atspi_common` 0.20) emits `Expand`/`Collapse`.

**Limit.** The tap fallback carries no direction to the handler. The adapter checks the
expanded state in its own copy of the tree and the owner checks the committed tree, and both
change only when the next frame publishes. Two `Expand` requests before then, or an `Expand`
right after an unpublished pointer tap, each run the tap handler, so a tap-only node can end
collapsed after an expand. A node that needs the direction registers `on_expand`/`on_collapse`.

**Test.** `every_wire_action_routes_to_a_semantics_action` (one row per `ActionName::ALL`) in
`src/agent/tests.rs`; `every_inbound_routable_action_is_advertised_outbound_again` in
`accesskit_translation.rs`; `disclosure_requests_follow_the_expanded_state` in
`src/agent/tests.rs`, whose rows drive explicit and tap-only nodes in both states through the
wire and platform paths to the handler that runs; `set_value_reaches_set_text_with_its_text`
for text and numeric `set_value`.

### 6. Every explicit role maps to an AccessKit role; `DragHandle` and `HotKey` stay generic

**Rule.** `explicit_role` gives every `SemanticsRole` except `None` an AccessKit role.
`DragHandle` and `HotKey`, which AccessKit has no counterpart for, become
`GenericContainer` rather than a role that would mislead a screen reader.

**Why.** `SemanticsRole` lives in `flui-protocol` and is `#[non_exhaustive]`, so the match in
this crate ends in a wildcard and the compiler no longer catches a forgotten arm. A role
without an arm would silently fall back to the flag cascade.

**Test.** `roles_and_checkbox_states_translate_to_accesskit` walks `SemanticsRole::ALL`, and
asserts that exactly `DragHandle` and `HotKey` map to `GenericContainer`; deleting the
`Form` arm makes it fail with `form maps to no AccessKit role`.

### 7. The in-process wire read projects the published AccessKit node; roles fold to what UIA reports

**Rule.** `SemanticsOwner::read_wire` (`src/agent.rs`) answers an agent with ADR-0080 wire
nodes projected from `to_accesskit_tree_update`, the update the platform adapter is handed,
read through `accesskit_consumer`: `common_filter` decides which nodes appear (a hidden node
drops its subtree, a `GenericContainer` is lifted into its parent, a focused node is kept), a
static text is named by its value, and the actions follow UI Automation's pattern predicates
(`is_invocable`; `Toggle` for a toggled node that is not a selection item; `SelectionItem` as
`accesskit_windows` 0.35 offers it). `set_value`, `expand`, `collapse`, `scroll_into_view` and
`focus` are listed only when the node carries the AccessKit action that performs them, and a
password field's value is never read. `wire_role` folds each AccessKit role to the wire role
the desktop server reads for it on Windows: the control type `accesskit_windows` 0.35 gives it,
through the desktop server's control-type table, with its ARIA restorations (cells, rows,
headers, a switch) and its `IsPassword`/`IsDialog` refinements. `native_role` keeps the AccessKit
name. `resolve_wire_action` refuses an action the node does not advertise *now*, so `expand` on
an expanded node is `action_unsupported`.

**Why.** One derivation for both backends: the desktop server reads the same AccessKit node
through UI Automation, so a second cascade over FLUI's flags would drift from what the OS
reports. The fold targets UIA rather than AccessKit's full role set because the wire
vocabulary is the desktop server's, and ADR-0095 §4 compares the two backends after
normalization. The direction check reads the last committed tree, which lags the request as
the adapter's copy does; that lag matters only for a tap-only expandable node (mapping
decision 5, Limit).

**Geometry and dialog roles.** The rectangles are
surface-relative physical pixels, reported as `surface_rect` with no screen `rect`: the realm
knows no window position, and a client that reads `rect` as screen pixels must find none rather
than a misplaced one. `AlertDialog` and `Dialog` read as `dialog` (UIA's `Window` with `IsDialog`),
and a `Keyboard` key or `TabPanel` as `pane`, as the desktop server would report them.

**Test.** `src/agent/tests.rs`: `every_role_but_the_documented_ones_reads_as_a_wire_role`
(ADR-0095 "Mapping pinned"; `none`, `drag_handle` and `hot_key` are the documented exceptions),
`every_role_bearing_flag_reads_as_the_role_uia_reports`,
`wire_role_matches_the_windows_adapter_for_every_role_flui_publishes` (transcribed from the
adapter and desktop sources, and checked to cover every role FLUI publishes),
`the_role_fold_was_transcribed_from_the_locked_windows_adapter` (fails when `Cargo.lock` moves
`accesskit_windows` off the release the fold was transcribed from),
`generic_containers_are_lifted_and_hidden_subtrees_dropped`,
`advertised_actions_follow_the_uia_patterns`, `disclosure_requests_follow_the_expanded_state`,
`set_value_reaches_set_text_with_its_text`, `a_disabled_node_refuses_with_disabled`,
`read_honours_max_depth_and_max_nodes_and_says_truncated` and
`a_read_tree_round_trips_through_json`. `every_wire_action_routes_to_a_semantics_action` pins
`semantics_action_for_wire` to the Windows adapter's route row by row.


### Property presence includes every supplied annotation

`SemanticsProperties::is_empty` means that no optional field is supplied and both
the tag and custom-action collections are empty. `Some(false)`, a supplied empty
string and supplied empty hint overrides remain present; this query concerns
whether an annotation was supplied, not its truth or text content. The exhaustive
field destructure makes a newly added property require a presence decision at
compile time. Public family
`semantics_property_presence_includes_every_public_annotation` covers each field
and a selection-only annotation published through a consumer's empty-annotation
filter and `SemanticsOwner`.

## Numeric and directional input

ADR-0124 distinguishes expand/collapse from activation and numeric setters from
text edits (mapping decision 5). `NumericRange` admits only finite inclusive
values and positive finite steps, and `set_numeric_range` marks the node as a
slider. Whole-request platform translation preserves numeric payloads, and
`SemanticsOwner::resolve_action` refuses a numeric setter whose value is
missing, non-finite or outside the node's current range
(`numeric_setters_are_checked_against_the_current_range`).
