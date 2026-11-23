# ADR-0392: A merged node's action belongs to the contributor whose state it changes

- **Status:** Proposed (2026-11-23)
- **Date:** 2026-11-23
- **Related:** [ADR-0206](ADR-0206-typed-semantics-attributes.md) (the merge table this record
  completes for standard actions, and decision 3's `Toggled` precedence),
  [ADR-0332](ADR-0332-a-control-takes-the-content-roles-beneath-it.md) (role precedence in a
  merge), [ADR-0223](ADR-0223-authored-semantics-regions.md) (handlers pinned before a fold)
- **Sources:** `flutter/flutter@3.44.0` `packages/flutter/lib/src/semantics/semantics.dart:5040`
  (`_getSemanticsActionHandlerForId`), `:6746` (`SemanticsConfiguration.absorb`), `:3830`
  (merged flags); `packages/flutter/lib/src/widgets/basic.dart:8121` (the `MergeSemantics`
  contract)

## Context and problem statement

Assembly folds every configuration a node takes into one `SemanticsConfiguration`
(`merge_child_fragments` in `crates/flui-rendering/src/pipeline/owner/semantics.rs`, receiver
first, then the fragments in reading order: sort key, then tree order). `absorb` keeps each group
of attributes by the merge table of ADR-0206 decision 4, but inserts every standard action's
handler unconditionally, so the last contributor's handler wins. Outside a forced merge this is
never observed: two handlers for one action, two declarations of one state or two scroll groups
conflict and form separate nodes (`shares_a_node_with`). Under `MergeSemantics` the conflicts are
suppressed and the split is published:

| Composition under `MergeSemantics` | The node announces | Click/Expand/Scroll runs |
|---|---|---|
| two buttons A, B | one button | B (Flutter runs A) |
| `ListTile(on_tap)` then `Radio` | a radio button, the radio's state | last wins: the radio; first wins: the tile |
| button then click-only disclosure header | the header's `expanded` | first wins: the button, so "collapsed" never changes |
| scrollable A at its max extent, then scrollable B | A's position and extents | ScrollDown and Increment: B's, until A leaves its extent |

First wins and last wins each choose which composition breaks:
the state a node announces and the action that changes it come from two merge rules. The question
is which contributor's handler a merged node's standard action runs.

Flutter keeps a merged node's descendants as hidden nodes and resolves an action when it arrives:
the merged node's own handler, else the first descendant in tree order that can perform it
(`semantics.dart:5040`). Its configuration fold is last wins (`_actions.addAll`, `:6759`), and
the merged node's flags are the union of its descendants' (`:3830`). So Flutter announces the
radio's checked state and runs the tile's tap for the second row; its material list tiles avoid
the case by giving the tile and the control one callback, which an application composing its own
tile does not know to do.

## Decision drivers

- An action a screen reader requests changes the state the node announces: activating a node
  that announces "radio button, not selected" selects that radio.
- Composition stays Flutter's: `MergeSemantics` over a tile with a control is one node, and no
  widget is written differently to get the right action.
- One rule for forced and ordinary merges; outside a forced merge nothing observable changes.
- Ordering independence: the result does not depend on whether the control comes before or
  after the tappable part in reading order.
- The frame path stays allocation-free for a node that merges nothing, and no per-node record
  is added (the control-cost gate, ADR-0322).
- A new action or state cannot be added without deciding where its handler comes from.

## Considered options

1. **First wins per action:** the receiver's handler, else the first contributor's in reading
   order.
2. **Last wins per action** (the code before this record).
3. **Flutter's form:** keep the merged descendants as hidden nodes and resolve the first eligible
   one when the request arrives.
4. **The action follows its state's owner:** each standard action is bound to the group of
   attributes it changes; the contributor that supplies that group to the node supplies its
   handlers, and an action bound to no published group goes to the first in reading order.
5. **Refuse the split:** a forced merge whose state and action would come from two contributors
   forms separate nodes, or drops the state.

## Decision

Chosen option: 4, because it is the only one under which every row of the table above runs the
handler of the control the node announces, in both orders, while `MergeSemantics` keeps
producing one node.

### §1 Each standard action has a subject

`flui-semantics` maps every `SemanticsAction` to the group of attributes it changes, by an
exhaustive `match` (`SemanticsAction` is not `#[non_exhaustive]`, so a new action does not
compile until it names its subject):

```rust
/// The group of a node's attributes a standard action changes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionSubject {
    /// Nothing the node publishes: the first handler in reading order runs.
    Activation,
    /// `toggled`, which the role gives its meaning (ADR-0206 decision 3).
    Toggle,
    /// `expanded`.
    Disclosure,
    /// The numeric range.
    Range,
    /// The scroll group (position, axis, extents).
    Scroll,
    /// The text content, else the value.
    Text,
    /// `focusable` and `focused`.
    Focus,
    /// The tooltip.
    Tooltip,
}
```

| Action | Subject |
|---|---|
| `Click` | `Toggle` when the node publishes `toggled`; else `Disclosure` when it publishes `expanded` and its disclosure owner registers neither `Expand` nor `Collapse` (the click-only disclosure `tap_disclosure_transition` routes); else `Activation` |
| `Expand`, `Collapse` | `Disclosure` |
| `Increment`, `Decrement`, `SetNumericValue` | `Range` when the node publishes a numeric range, else `Scroll` |
| `ScrollUp`, `ScrollDown`, `ScrollLeft`, `ScrollRight`, `SetScrollOffset` | `Scroll` |
| `SetValue`, `SetTextSelection`, `ReplaceSelectedText` | `Text` |
| `Focus`, `Blur` | `Focus` |
| `ShowTooltip`, `HideTooltip` | `Tooltip` |
| `ScrollIntoView`, `ShowContextMenu` | `Activation` |
| `CustomAction` | none: each custom action keeps its own handler (ADR-0206 decision 4, unchanged) |

### §2 A subject's owner supplies its handlers

The owner of a subject is the configuration whose declaration of that group the node publishes:
for every group but `Toggle` the first in fold order that declares it (the merge table already
keeps those first wins); for `Toggle` the configuration whose role the node publishes when that
one declares `toggled`, else the first that declares `toggled`. A forced merge therefore takes
`toggled` from its owner, not by `Mixed > On > Off`: the published state is one control's, and
two radios merged into one node announce the first one's selection and select that one.

An owner **acts on** its subject when it registers any of the subject's actions, a blocked one
included, or declares `enabled(false)`. Then:

- the subject's actions are the owner's, and only the owner's: an action of the subject the owner
  does not offer (a scrollable at its extent, a disabled control, a blocked part) is not taken from
  another contributor, and one an earlier contributor supplied is removed when the owner arrives;
- an owner that does not act on its subject (a `Semantics` that only declares `expanded` or
  `toggled` over the detector that handles the tap, the documented wrapper form) leaves the
  subject's actions to the first contributor in reading order that registers them;
- `Activation` has no owner: the receiver's handler, else the first contributor's in reading
  order.

Eligibility is filtered before ownership, as before: a contributor that blocks user actions
offers none of the blocked ones, but its registered handlers still make it act on its subject,
so a blocked radio in a tappable tile publishes no click rather than the tile's.

### §3 The fold stays one pass over the configurations

The owner of every subject is decided by what the receiver already holds and what the
contributor declares, so `absorb` keeps its signature and decides each action as it folds one
contributor: an owner that arrives later than an action's first supplier replaces that handler,
and nothing is kept per contributor once it is folded. A configuration that is itself the result
of a fold is one contributor whose groups and handlers already agree. No allocation is added; the
handlers a fold drops are pinned before it runs (ADR-0223), as the overwritten ones were.

### Consequences

- Good, because every composition of the table above runs the announced control's handler in
  both orders: the radio in a tile, the disclosure beside a button, the scrollable whose metrics
  are published.
- Good, because two plain buttons merged run the first one, which is the `MergeSemantics`
  contract Flutter documents (`basic.dart:8121`).
- Good, because nothing changes outside a forced merge: there a group and an action have one
  supplier already, and the wrapper form keeps working through the non-acting owner.
- Good, because a new `SemanticsAction` does not compile until its subject is chosen.
- Bad, because a tile's own tap is no longer reachable through the merged node when a control in
  it owns `Click`; the tile's handler is the one assistive technology cannot reach. That is the
  same trade Flutter's list tiles make by giving both one callback.
- Bad, because a forced merge of two checkable controls no longer announces the more informative
  state; it announces the owner's.
- Neutral, because a merged node's `enabled` still follows ADR-0206 (receiver first), so a node
  whose owner is disabled may publish no action under an enabled state; the owner rule removes
  the action, and the role's own validation is unchanged.

### Confirmation

- The exhaustive `match` of §1 in `flui-semantics`: a new action is a compile error until it is
  bound.
- Behaviour tests through the public API in `crates/flui-widgets/tests/semantics.rs`, invoking
  each action through the platform request path and asserting which handler ran and the state
  the node announces afterwards: two buttons (the first runs), the receiver's own handler over a
  descendant's, a blocked first part; a plain button and a click-only disclosure header in both
  orders (Expand opens the header); a tile with a tap and a switch in both orders (Click toggles
  the switch); two scrollables with the first at each extent (ScrollDown and Increment run the
  first's or are not offered).
- Unit rows in `configuration.rs` for the owner of each subject and for the non-acting wrapper
  form, and `state_merge_precedence` restated for `Toggled` under its owner.
- `merge_semantics_over_a_tile_and_radio_announces_as_one_radio_button` in
  `packages/flui-material/tests/list_tile.rs` extended to invoke Click.

## Pros and cons of the options

### 1. First wins per action

- Good, because it is one line and matches Flutter for two plain buttons.
- Bad, because a tile followed by its radio runs the tile, under a node announcing the radio, and
  a button followed by a disclosure never opens the disclosure.
- Bad, because a scrollable at its extent hands its missing scroll action to the next one.

### 2. Last wins per action

- Good, because nothing changes.
- Bad, because two merged buttons run the last, against the documented contract, and each
  row of the table breaks in the mirrored order.

### 3. Flutter's form

- Good, because it is the reference's shape.
- Bad, because it resolves by tree order alone, so it has option 1's defects; Flutter announces
  the union of the descendants' flags and runs the first descendant's tap.
- Bad, because it keeps a hidden node per merged descendant, a second tree shape beside the folded
  configuration, which costs memory per control and a walk per request.

### 4. The action follows its state's owner

- Good, because the action and the state it changes have one supplier, whatever the order.
- Good, because it needs no per-node record and keeps `absorb`'s one pass.
- Bad, because the subject table is one more rule to keep with the action vocabulary; the
  exhaustive `match` holds it.

### 5. Refuse the split

- Good, because the published node can never be inconsistent.
- Bad, because `MergeSemantics` over a tile and its control would form two stops, which is what
  the widget exists to prevent; dropping the state instead hides the control's state.

## More information

- [Issue 1070](https://github.com/vanyastaff/flui/issues/1070) (the first-in-tree-order contract) and
  [issue 2881](https://github.com/vanyastaff/flui/issues/2881) (state and action from different contributors,
  with the scroll-group comment).
- A request carries no ticket for a standard action: it resolves against the node as it is when
  it arrives. Because the owner rule makes the handler a function of the published groups, a
  request reaches the control whose state the last update announced unless that state's owner
  changed in between, which is the same window a rebuilt control's new closure has. Revisit with a
  delivered-handler check like custom actions' (ADR-0206 decision 4) if a test shows a request
  running a control the update did not announce.
