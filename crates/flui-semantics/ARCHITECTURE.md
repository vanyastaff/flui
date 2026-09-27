# flui-semantics architecture

The fifth tree: the accessibility tree assembled from the render tree and
exported to the platform through AccessKit. Layer rules and the crate's place in
the workspace DAG live in [`docs/FOUNDATIONS.md`](../../docs/FOUNDATIONS.md) and
this crate's `[package.metadata.flui] layer`; this file
records the per-decision divergences from the Flutter reference that would
otherwise read as drift.

Cross-crate protocol decisions belong in an ADR (`docs/adr/`). What belongs here
is a decision local to this crate: a translation rule, a precedence, a payload
shape.

## Mapping decisions

### 1. Role resolution is a single-valued specificity cascade, and checkable state outranks the broad `IsButton`

**Rule:** [`AGENTS.md`](../../AGENTS.md) Design stance ("Flutter is a reference, not a spec") — the reference's
observable behavior is the floor; where a contract can be improved, improve it
and record what is better. The precedence below is a rule FLUI has to invent,
because the shape it exports into has no equivalent of the reference's.

**Oracle:** Flutter publishes a semantics node as a *set* of
`SemanticsFlag`s and lets each platform read the ones it wants. Its own corpus
asserts that set directly — for example
`test/material/radio_list_tile_test.dart` (tag `3.44.0`) asserts one node
carrying `isButton`, `hasCheckedState`, `hasEnabledState`, `isEnabled`,
`isInMutuallyExclusiveGroup`, `isFocusable` and `hasSelectedState` **together**,
with `[SemanticsAction.tap, SemanticsAction.focus]`. There is no single "role"
in the reference and therefore no precedence question to answer.

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
composition this precedence is measured to affect is the reference's own:
`MergeSemantics` over a `ListTile` carrying a `Radio`, which is what
`RadioListTile` builds (`material/radio_list_tile.dart`, tag `3.44.0`, wraps its
tile in `MergeSemantics` so the whole tile is "a single interactive entity").
Measured through `packages/flui-material/tests/list_tile.rs`'s fixture, that tree
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
tile under a default `MediaQueryData` for exactly this reason, and
`a_radio_inside_a_list_tile_announces_as_a_radio_button` is the test whose
absence let the wrong figure stand: it reddens to
`[GenericContainer, Button, CheckBox]` when the group flag is removed, which is
the only thing that changes the answer.

**The reference splits that node the same way; its one-node oracle is a merge.**
Flutter's corpus asserts the tile-and-radio composition as **one** node:
`test/material/radio_list_tile_test.dart`, `testWidgets('RadioListTile semantics')`
(tag `3.44.0`), one node carrying `isButton`, `hasCheckedState`, `hasEnabledState`,
`isEnabled`, `isInMutuallyExclusiveGroup`, `isFocusable` and `hasSelectedState`,
with `actions: [SemanticsAction.tap, SemanticsAction.focus]` and `label: 'Title'`
(the `isButton` there depends on the test passing `internalAddSemanticForOnTap:
true`). No flag bit lets the reference merge where FLUI splits. The
reference's `isCompatibleWith` rejects actions on the same raw bit intersection
FLUI uses, and rejects flags through `_flags.hasConflictingFlags(..)`, which for
every state the tile and the radio share tests the same "both carry the trait"
condition FLUI's bit intersection does: `isEnabled.hasConflict(..)` is
`both != Tristate.none`, and `hasEnabledState` *is* `isEnabled != none` — the
same for `isFocused`/`isFocusable` and `isChecked`/`hasCheckedState`. So the
reference's own predicate marks the radio as conflicting
(`rendering/object.dart`, `_marksConflictsInMergeGroup`), the radio forms its
own `SemanticsNode` (`shouldFormSemanticsNode`, via `_hasSiblingConflict`), and
what the oracle sees as one node is `MergeSemantics`: `getSemanticsData` folds
every descendant's flags and actions into the parent under
`mergeAllDescendantsIntoThisNode` (`semantics/semantics.dart`), and the tester
reports zero children for such a node. The bare `ListTile` FLUI measures above
is therefore *not* the oracle's composition; the like-for-like one is
`MergeSemantics::new().child(ListTile…Radio)`, and measured it exports **one**
node with role `RadioButton` — the same shape. A bare tile splits in both
frameworks — on the reference's side by its predicate as traced above; no
reference test mounts a bare `ListTile` with a `Radio`, so that half is an
inference at the tag, not an oracle. Provenance, since it is the one weak link: `hasConflictingFlags`
is defined on `SemanticsFlags` in the engine's `lib/ui`, which `.flutter` does
not carry; its body was read from the engine source bundled with a local SDK
(framework `3.44.8`, engine `0cd6107`), a different patch release from the
pinned tag. The tristate shape is not in doubt at the tag — the framework there
already drives it through `_tristateFromBoolOrNull` — but the exact member list
is read from that revision, not from `3.44.0`.

What the merged node still lacks against the oracle is named rather than left
to be discovered: it carries **no label** (`Text` → `RenderParagraph` publishes
no semantics — a deferral already recorded in
`crates/flui-objects/src/text/paragraph.rs`'s module doc, so the tile's `title`
cannot label it the way the reference's is labelled `'Title'`) and **no tap
action** (Flutter's `InkResponse` publishes `Semantics(onTap: ..)` itself;
FLUI's does not — see `packages/flui-material/ARCHITECTURE.md`). Role matches;
label and action are the recorded gaps.

**Consequences, named rather than left to be discovered:**

- **`IsButton` is now the weakest of the interactive roles, and that is a
  behaviour change for any node that carries a checkable flag beside it.** A
  widget that deliberately publishes both — a toggle styled as a button — now
  resolves to `Switch`/`CheckBox`. That is the correct reading of the flag set,
  and it is the reading the reference's corpus asserts, but it is a change to
  existing output rather than a new capability.
- **The cascade is still lossy, and cannot be made lossless here.** Any node
  carrying two unrelated roles in its flag set loses one. The reference does not
  have this problem because it does not have to choose. The mitigation is
  order-of-specificity, not completeness — a genuinely two-role node has no
  correct single answer, only a defensible one.
- **The order is load-bearing and is not obvious from any single flag's
  meaning**, so it is recorded here rather than left to the comment on the
  function: reordering the arms is a silent behavioural change that no
  type-checker and no single-flag test can catch.

**Replacement tests:**

- `crates/flui-semantics/src/accesskit_translation.rs` —
  `a_checkable_beside_is_button_still_resolves_to_the_checkable` is the pin, and it
  lives here because the rule does. It asserts every arm of the reordered cascade
  on a node carrying `IsButton` *beside* the more specific state, plus the
  `IsButton`-only case as the non-vacuous premise. Its sibling
  `a_checkable_in_a_mutually_exclusive_group_is_a_radio_button` sets only the
  checkable flags, so it passes under either order and cannot pin this.
  `is_link_and_is_text_field_lose_to_is_button_as_they_always_have` pins the
  *other* half of the cascade — the two arms this reorder deliberately left below
  `IsButton` (see mapping decision 2).
- `packages/flui-material/tests/radio.rs` —
  `a_mounted_radio_announces_as_a_radio_button` and
  `a_mounted_radio_does_not_announce_as_a_checkbox` pin the direct case;
  `a_radio_nested_under_an_annotated_ancestor_still_announces_as_a_radio_button`
  mounts the absorbed composition end-to-end and reddens when the cascade is
  reverted; `a_radio_without_a_tap_handler_still_announces_as_a_radio_button` pins
  that interactivity and kind are independent.
- `packages/flui-material/tests/list_tile.rs` —
  `a_radio_inside_a_list_tile_announces_as_a_radio_button` pins the composition a
  user actually writes, through this family's `MediaQuery`-wrapped `themed`
  fixture. It asserts **both** roles the composition exports (`Button` from the
  tile's own tap target, `RadioButton` from the radio's separate node), and it
  reddens to `[GenericContainer, Button, CheckBox]` when `Radio` stops publishing
  the group flag. It pins the *widget* half of the change rather than this
  precedence: the tile composition exports the same roles with the cascade
  reverted, so mounting it here would pin the wrong layer — which is exactly the
  wrong figure its absence let stand.
- `packages/flui-material/tests/checkbox.rs` keeps the other arm honest: a
  checkbox is a `CheckBox` and not a `RadioButton`, so the group flag is what
  distinguishes them rather than checked state alone. Both of its tests pass
  before *and* after the reorder, since a checkbox carries neither `IsButton` nor
  the group flag; they guard the group flag against leaking onto other
  checkables, which is a real regression to guard against and not evidence for
  the reorder.

### 2. `IsButton` outranks `IsLink` and `IsTextField`, and the reference's corpus is not silent about it

**Rule:** [`AGENTS.md`](../../AGENTS.md) Design stance ("Flutter is a reference, not a spec") — a divergence from the
reference is kept only by decision, recorded where a reader will find it, with a
test that replaces the reference's coverage.

**The divergence.** The cascade in mapping decision 1 leaves `IsLink`,
`IsTextField`, `IsSlider`, `IsImage` and `IsHeader` *below* `IsButton`, so a node
carrying `IsButton` beside one of them loses that other role. That is not an
absence of reference coverage, which an earlier draft of this file asserted: the
corpus pins the collision directly. Measured over
`.flutter/packages/flutter/test/**/*.dart` (tag `3.44.0`), counting every `flags:`
list literal — one per expectation node — 105 of 590 carry
`SemanticsFlag.isButton`, and among them:

| flags beside `isButton` | lists | files |
|---|---|---|
| `hasCheckedState` | 20 | `radio_list_tile_test.dart`, `switch_list_tile_test.dart`, `chip_test.dart`, `toggle_buttons_test.dart`, `custom_painter_test.dart`, `time_picker_test.dart` |
| `hasToggledState` | 1 | `switch_list_tile_test.dart` |
| `isTextField` | 1 | `dropdown_menu_test.dart` |
| `isLink` | 1 | `semantics_merge_test.dart` |
| `isSlider`, `isImage`, `isHeader`, `isKeyboardKey` | 0 | — |

**What the instrument cannot see, so this table is a floor and not a population:**
it matches the literal spelling `SemanticsFlag.x` inside a `flags: [...]` literal,
so a test that assembles its flag list from a local constant or a spread is not
counted at all. The two collisions that matter here are pinned by tests it does
see and were read directly; the counts beside them are context for "the corpus is
not silent", not an exhaustive census of it.

The two the earlier draft denied outright are the two that matter here, and both
were read directly rather than counted:

- `test/material/dropdown_menu_test.dart`,
  `testWidgets('ensure exclude semantics for trailing button')` — one node
  carrying `isTextField`, `isFocusable`, `hasEnabledState`, `isEnabled`,
  `isReadOnly`, `isButton` and `hasExpandedState`, with
  `actions: [SemanticsAction.focus, SemanticsAction.expand]`.
- `test/widgets/semantics_merge_test.dart`,
  `testWidgets('LinkUri from child is passed up to the parent when merging nodes')`
  — one node carrying `isButton`, `hasEnabledState`, `isEnabled`, `isFocusable`
  and `isLink`, plus a `linkUrl`.

So the corpus is a *floor* here and FLUI does not meet it. `Role` is
single-valued where the reference publishes a flag set and lets each platform
read what it wants, so a node FLUI reports as `Button` is one the reference
reports as a button **and** a link (or a text field).

**Choice:** keep `IsButton` above them. This is a **pre-existing divergence, not
a regression**: `IsButton` was already tested above `IsLink` and `IsTextField`
before this change, and the reorder above moved only the checkable states. The
behaviour is unchanged and deliberate; what this entry adds is the accounting.

**Why it is not merely theoretical.** `Semantics::new().button(true).link(true)`
and `Semantics::new().button(true).text_field(true)` both compile today and each
resolves `Role::Button`, which makes the `IsTextField` arm — the one carrying
`MultilineTextInput` / `PasswordInput` / `TextInput`, a distinction a screen
reader announces — unreachable for such a node. A widget that publishes both
intends both.

**Replacement test:** `crates/flui-semantics/src/accesskit_translation.rs` —
`is_link_and_is_text_field_lose_to_is_button_as_they_always_have` pins the
current precedence for exactly those two co-flag nodes, so a later reorder cannot
change the answer silently and the divergence reads as chosen rather than
overlooked. Its fixture sets the losing flag explicitly and shows each flag
winning on its own, so the pin cannot read as the `IsButton`-only case.

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

**Divergence.** Flutter's current `_RenderObjectSemantics` compiler is a nullable-per-node
state machine (`_SemanticsFragment`, `mergeUp`, sibling merge groups, geometry-dirty tracking).
FLUI keeps the observable `SemanticsNode` tree as the contract and implements incrementality as
anchor-scoped re-assembly over the classic merge model instead. Freshness is mark-driven, as in
Flutter's `flushSemantics`: geometry outside re-assembled subtrees keeps its last published
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

**Divergence.** Flutter's platform bridges expose a static text's label as its name and its
value separately. AccessKit's `Label` has one text slot on UIA and AT-SPI, so the two are
joined with the separator the reference uses to join merged labels (`_concatAttributedString`
in `packages/flutter/lib/src/semantics/semantics.dart`, tag `3.44.0`, `'\n'`).

**Test.** `static_text_is_named_by_its_text` reads each node's name through
`accesskit_consumer` the way the adapters do; `cargo xtask device windows-a11y` is the live
check that found the defect.

### 5. The ADR-0080 tools reach FLUI through AccessKit's Windows adapter: `set_value` is `SetText`, `expand`/`collapse` are `Tap`

**Rule.** `semantics_action_for` names every `accesskit::Action`. The tools an agent calls
(ADR-0080, `flui_protocol::ActionName`) arrive through accesskit_windows 0.35.0 as:
`invoke`, `toggle` and `select` → `Click` → `Tap`; `set_value` → `SetValue` → `SetText`;
`focus` → `Focus`; `scroll_into_view` → `ScrollIntoView` → `ShowOnScreen`; `expand` and
`collapse` → `Expand`/`Collapse` → `Tap`. A node with `HasExpandedState` and a tap handler
advertises `Expand` while collapsed and `Collapse` while expanded, never both.

**Why.** AccessKit does not count a node with an expanded state as invocable
(`accesskit_consumer` 0.39, `Node::is_invocable`), so the Windows adapter offers only the
`ExpandCollapse` pattern for it. With `Expand` and `Collapse` unrouted, an expandable FLUI node
could be neither invoked nor expanded by an agent or a screen reader. FLUI toggles an
expandable node through its tap handler, and the adapter refuses a transition to the state
the node already has (accesskit_windows 0.35.0 `node.rs`, the `ExpandCollapse` provider), so
routing both to `Tap` toggles in the requested direction while the adapter's copy of the tree
is current. No other shipped adapter (`accesskit_macos` 0.27, `accesskit_atspi_common` 0.20)
emits them.

**Known gap.** The adapter checks the expanded state in its own copy of the tree, which
changes only when FLUI publishes the next tree update, and the request is queued to the realm
without waiting for a frame. Two `Expand` requests before the next frame, or an `Expand`
right after a pointer tap that has not been published yet, each pass that check and each run
the tap handler, so the node can end collapsed after an expand. Nothing FLUI-side checks the
direction: the request reaching the realm is a plain `Tap`. The discrete actions below would
close this, because the handler would receive the requested direction instead of a toggle.

**Divergence.** Flutter's `dart:ui` has discrete `SemanticsAction.expand` (`1 << 24`) and
`collapse` (`1 << 25`) (`engine/src/flutter/lib/ui/semantics.dart`, checked on 2026-09-26).
FLUI has no such actions and keeps those bits reserved
(`flui_protocol::SemanticsAction::RESERVED_BITS`); adding the two actions at Flutter's bits
is the follow-up that would remove the `Tap` route. A numeric `set_value` (a slider through
UI Automation's `RangeValue`) arrives as `SetValue` with `ActionData::NumericValue`, which has
no FLUI argument shape: the handler receives `SetText` with no arguments. Nothing sends
`Increase`/`Decrease` for it.

**Test.** `every_wire_action_routes_to_a_semantics_action` (one row per `ActionName::ALL`),
`a_numeric_set_value_routes_set_text_without_its_number`,
`an_expandable_node_advertises_only_the_transition_its_state_allows` and
`every_inbound_routable_action_is_advertised_outbound_again` in `accesskit_translation.rs`;
`a_platform_expand_runs_the_tap_handler_of_a_collapsed_node` in
`crates/flui-widgets/tests/semantics.rs`. `flui_testing::a11y::invoke_semantics_action` has
no state guard: sending it the transition the node does not advertise toggles it anyway.

### 6. Every explicit role maps to an AccessKit role; `DragHandle` and `HotKey` stay generic

**Rule.** `explicit_role` gives every `SemanticsRole` except `None` an AccessKit role.
`DragHandle` and `HotKey`, which AccessKit has no counterpart for, become
`GenericContainer` rather than a role that would mislead a screen reader.

**Why.** `SemanticsRole` lives in `flui-protocol` and is `#[non_exhaustive]`, so the match in
this crate ends in a wildcard and the compiler no longer catches a forgotten arm. A role
without an arm would silently fall back to the flag cascade.

**Test.** `every_role_but_none_maps_to_an_accesskit_role` walks `SemanticsRole::ALL`, and
asserts that exactly `DragHandle` and `HotKey` map to `GenericContainer`; deleting the
`Form` arm makes it fail with `form maps to no AccessKit role`.
