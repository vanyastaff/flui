# ADR-0392: A merged node's action belongs to the contributor whose state it changes

- **Status:** Accepted (2026-11-23)
- **Date:** 2026-11-23
- **Supersedes:** [ADR-0206](ADR-0206-typed-semantics-attributes.md), in part: decision 3's
  `Mixed > On > Off` precedence for `Toggled`, and decision 4's forced merge of a standard
  action's handler and of `toggled`, `selected` and `enabled`
- **Related:** [ADR-0206](ADR-0206-typed-semantics-attributes.md) (the merge table this record
  completes for a forced merge's standard actions, `toggled`, `selected` and `enabled`),
  [ADR-0332](ADR-0332-a-control-takes-the-content-roles-beneath-it.md) (role precedence in a
  merge), [ADR-0223](ADR-0223-authored-semantics-regions.md) (handlers pinned before a fold)
- **Sources:** `flutter/flutter@3.44.0` `packages/flutter/lib/src/semantics/semantics.dart:5040`
  (`_getSemanticsActionHandlerForId`), `:6746` (`SemanticsConfiguration.absorb`), `:3830`
  (merged flags); `packages/flutter/lib/src/widgets/basic.dart:8121` (the `MergeSemantics`
  contract)

## Context and problem statement

Assembly folds every configuration a node takes into one `SemanticsConfiguration`
(`merge_child_fragments` in `crates/flui-rendering/src/pipeline/owner/semantics.rs`): the
receiver first, then the fragments in reading order (sort key, then tree order), each of which
may itself be the fold of its own descendants. `absorb` keeps each group of attributes by the
merge table of ADR-0206 decision 4, but inserts every standard action's handler
unconditionally, so the last contributor's handler wins. An ordinary merge never shows this: two
handlers for one action, two declarations of one state or two scroll groups conflict and form
separate nodes (`shares_a_node_with`). Under `MergeSemantics` the conflicts are suppressed and
the node can announce one control while running another's handler:

| Composition under `MergeSemantics` | The node announces | Click, Expand or a scroll runs |
|---|---|---|
| two buttons A, B | one button | B |
| a tile's tap, then a radio | the radio and its state | last wins: the radio; first wins: the tile |
| a button, then a click-only disclosure header | the header's `expanded` | first wins: the button, so "collapsed" never changes |
| a tap, then a selectable tab | the tab and `selected` | first wins: the tap, so the tab is never selected |
| scrollable A at its end, then scrollable B | A's position and extents | ScrollDown: B's, until A leaves its end |

First wins and last wins each choose which composition breaks, because the state a node
announces and the action that changes it come from two merge rules. The question is which
contributor's handler a forced merge's standard action runs, and which contributor's `enabled`
the node announces beside it.

Flutter documents the first handler in tree order (`basic.dart:8121`), and resolves an action
on a merged node by its own handler, else the first descendant node in tree order that can
perform it (`semantics.dart:5040`). A descendant that forms no node of its own is folded into
its parent's configuration last wins (`_actions.addAll`, `:6759`), so two plain buttons under
one `MergeSemantics` run the second, against the documented contract; the merged node's flags
are the union of its descendants' (`:3830`). Either way the handler is chosen by tree order and
the state by union, so Flutter can announce a radio's checked state and run the tile's tap. Its
material list tiles avoid the case by giving the tile and the control one callback; an
application composing its own tile does not know to.

## Decision drivers

- An action a screen reader requests changes the state the node announces: activating a node
  that announces "radio button, not selected" selects that radio, and a node offers an action
  only while it announces the control that action belongs to as enabled.
- Composition stays Flutter's: `MergeSemantics` over a tile with a control is one node, and no
  widget is written differently to get the right action.
- The result depends on the set of contributors and their reading order only through the merge
  table's own precedences, never on which contributor happens to be folded before a later one
  changes the node's role or groups.
- An ordinary merge publishes what it publishes today.
- The frame path stays allocation-free for a node that merges nothing; a forced merge adds no
  allocation per contributor.
- A new action or a new state cannot be added without deciding where its handler comes from.

## Considered options

1. **First wins per action:** the receiver's handler, else the first contributor's in reading
   order.
2. **Last wins per action** (the code before this record).
3. **Flutter's form:** keep the merged descendants as hidden nodes and resolve the first eligible
   one when the request arrives.
4. **The action follows its state's owner, decided while folding:** each contributor's handlers
   are kept or replaced as it is absorbed, by who owns the action's group so far.
5. **The action follows its state's owner, planned over the whole merge:** the forced merge first
   reads every contributor to decide, per group, which one the node publishes; then each handler
   is taken only from its group's owner.
6. **Refuse the split:** a forced merge whose state and action would come from two contributors
   forms separate nodes, or drops the state.
7. **Keep the displaced handler as a custom action:** the owner's handler takes the standard
   action, and a non-owner's handler for it is published as a custom action of the node.

## Decision

Chosen option: 5, because it is the only one under which every row of the table above runs the
handler of the control the node announces, in both orders and whatever a later contributor
changes, while `MergeSemantics` keeps producing one node.

### §1 Each standard action has a subject

`flui-semantics` maps every `SemanticsAction` to the group of attributes it changes on a node,
`ActionSubject::classify(action, click, has_range)`, where `click` is the subject the merged node gives `Click` and `has_range` whether it publishes a numeric range:

```rust
/// The group of a node's attributes a standard action changes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionSubject {
    /// Nothing the node publishes: the first handler in reading order runs.
    Activation,
    /// `toggled`, which the role gives its meaning (ADR-0206 decision 3).
    Toggle,
    /// `selected`.
    Selection,
    /// `expanded`.
    Disclosure,
    /// The numeric range.
    Range,
    /// The scroll group (position, axis, extents).
    Scroll,
    /// The value.
    Value,
    /// The text content.
    Text,
    /// `focusable` and `focused`, one group.
    Focus,
    /// The tooltip.
    Tooltip,
}
```

| Action | Subject on the merged node |
|---|---|
| `Click` | the first of `Toggle`, `Selection`, `Disclosure` whose state the node publishes and its published role announces (`applicability::applicable`; a node without a role announces every state), where `Disclosure` counts only when its owner registers neither `Expand` nor `Collapse` (the click-only disclosure); else `Activation` |
| `Expand`, `Collapse` | `Disclosure` |
| `Increment`, `Decrement`, `SetNumericValue` | `Range` when the node publishes a numeric range, else `Scroll` |
| `ScrollUp`, `ScrollDown`, `ScrollLeft`, `ScrollRight`, `SetScrollOffset`, `ScrollIntoView` | `Scroll`: reveal moves the scroll groups of the node and its ancestors (`resolve_action`), so its handler is the scroll group's owner's |
| `SetValue` | `Value` |
| `SetTextSelection`, `ReplaceSelectedText` | `Text` |
| `Focus`, `Blur` | `Focus` |
| `ShowTooltip`, `HideTooltip` | `Tooltip` |
| `ShowContextMenu` | `Activation` |
| `CustomAction` | none: each custom action keeps its own handler (ADR-0206 decision 4) |

`SemanticsAction` is `#[non_exhaustive]` and belongs to `flui-protocol`, so the `match` ends in
an arm a new action falls into; `every_action_names_its_subject` lists every action of
`SemanticsAction::ALL` with its subject and fails until a new one is placed. The subject table is
the one classification of actions: the unused `is_scroll_action`, `is_text_action` and
`is_focus_action` predicates of `flui-protocol` are removed.

A forced merge whose `Click` belongs to `Toggle` or `Selection` while a click-only disclosure
was merged into it drops that `expanded`: the click changes the toggle or the selection, so the
node does not also announce a disclosure the click would route `Expand` or `Collapse` to
(`tap_disclosure_transition`, unchanged for every other node). A switch merged with a click-only
header announces the switch and offers no Expand.

### §2 A forced merge plans its owners before it takes a handler

When `merge_child_fragments` folds with conflicts suppressed, it opens the fold with
`SemanticsConfiguration::forced_merge(parts)`, where `parts` are exactly the configurations the
fold will absorb, in fold order (a second text keeps its own node, ADR-0214 §2, and one
predicate decides that for the plan and for the fold). It reads the receiver and every part and
builds an `ActionPlan` (a fixed array over the subjects, on the stack), then returns a
`ForcedMerge` that absorbs each part by the plan and settles the result when it is finished or
dropped. Its owners are those of the merged result:

- the owner of a subject is the configuration whose declaration of that group the node
  publishes: the first in fold order that declares it, except for `Toggle` and `Selection`,
  whose owner is the configuration whose role the node finally publishes when that one declares
  the state, else the first that declares it. A forced merge publishes the owner's `toggled` and
  `selected`, not the most informative one, so `Toggled` merges receiver first like every other
  state and ADR-0206 decision 3's `Mixed > On > Off` is gone;
- an owner **acts on** its subject when it registers one of the subject's actions (a blocked one
  included), declares `enabled(false)`, or is itself a merge whose owner of that subject acted
  (§3);
- an acting owner supplies every handler of its subject, and only it: an action of the subject it
  does not offer (a scrollable at its end, a disabled or blocked control) is offered by nobody;
- a subject whose owner does not act (a `Semantics` that only declares `expanded` or `toggled`
  over the detector that handles the tap), and a subject nobody declares, takes each action from
  the first configuration in fold order that registers it, as `Activation` does;
- the node's `enabled` is the owner's of `Click`'s subject when that owner declares one, else
  ADR-0206's (the receiver's, else the first contributor's), so a disabled radio in an enabled
  tile is announced disabled and offers no click. A node announced disabled refuses every
  request at `resolve_action`, so the merged node offers what it announces: a disabled owner of
  `Click` disables the whole node, as a disabled tile always has.

`absorb` itself keeps ADR-0206's table for an ordinary merge, where the conflict rules already
leave each group and each action one supplier; only `ForcedMerge::absorb` reads the plan.

### §3 A merged configuration keeps what an outer merge needs

A configuration that is the result of a forced merge is one contributor to an outer merge.
Blocking drops a handler before the fold takes it, so such a configuration records, in a
`SubjectSet` of two bytes, the subjects whose owner acted but whose handlers it offers none of;
the outer plan reads that set as acting. Nothing else is kept per contributor once it is folded,
and no allocation is added. The handlers the fold drops are pinned before it runs (ADR-0223), as
the overwritten ones were.

### Consequences

- Good, because every composition of the table above runs the announced control's handler in
  both orders, and a later contributor that changes the role or adds a group changes the owner
  before any handler is taken.
- Good, because two plain buttons merged run the first one, which is the `MergeSemantics`
  contract Flutter documents (`basic.dart:8121`) and its own fold does not keep.
- Good, because an ordinary merge is unchanged: `absorb`'s table and the conflict rules are as
  before.
- Good, because a new `SemanticsAction` fails a test until its subject is chosen.
- Bad, because a tile's own tap is no longer reachable through the merged node when a control in
  it owns `Click`. In the usual form, a radio, check box or switch list tile, tapping the tile and
  selecting its control are one act with one callback, so nothing is lost; only a tile whose tap
  does something other than its control loses that tap under assistive technology, and such a tile
  announces a control whose state the tap would not change. The author's remedy is a custom
  action on the tile for that other act, or not merging.
- Bad, because a forced merge of two checkable controls announces the owner's state rather than
  the most informative one.
- Bad, because a plan covers one fold level: a fragment its descendants already folded brings
  the owners its own plan chose, and the outer plan reads it as one contributor with its withheld
  subjects (§3), not as the parts inside it. A control nested two folds deep beside one at the
  outer level is ordered by the outer level's fold order alone.
- Bad, because a new state in `States` does not by itself fail anything: whether it needs a
  subject is a review question, which the table's test makes visible beside the action list.
- Neutral, because the forced fold reads its contributors twice; both passes are over the
  fragments already gathered, with no allocation.

### Confirmation

- `every_action_names_its_subject` in `crates/flui-semantics/src/action_subject.rs`: every
  action of `SemanticsAction::ALL` with its subject.
- `a_forced_merge_plans_each_action_by_its_owner` in `configuration.rs`: a row per subject in
  both orders, the non-acting wrapper, a disabled and a blocked owner, a later contributor that
  changes the role or adds a range, a nested merge whose blocked owner acted, a switch beside a
  click-only header (its `expanded` dropped, no Expand), a toggle button over a check box whose
  role announces the toggle, and `enabled` from the owner; `state_merge_precedence` there keeps
  the receiver's `toggled`.
- `a_merged_action_runs_the_control_whose_state_it_announces` in
  `crates/flui-widgets/tests/semantics.rs`, through the platform request path: two buttons, a
  click-only disclosure and a button in both orders, a switch and a tap in both orders, a
  selectable tab and a tap, two scrollables each at one extent.
- `an_ordinary_merge_keeps_its_actions` in `configuration.rs`: a wrapper that declares a state
  over a detector, and a `value` declarer beside a `SetValue` registrant, publish what they
  published before.
- `merge_semantics_over_a_tile_and_radio_clicks_the_radio` in
  `packages/flui-material/tests/list_tile.rs`, with an unselected radio and a tile tap that
  records its own call, then one shared callback: Click runs the radio's once, and the tile's
  own tap does not run.

## Pros and cons of the options

### 1. First wins per action

- Good, because it is one line and matches Flutter for two plain buttons.
- Bad, because a tile followed by its radio runs the tile under a node announcing the radio, and
  a button followed by a disclosure never opens it.

### 2. Last wins per action

- Good, because nothing changes.
- Bad, because two merged buttons run the last, against the documented contract, and each row of
  the table breaks in the mirrored order.

### 3. Flutter's form

- Good, because it is the reference's shape.
- Bad, because it resolves by tree order alone, so it has option 1's defects.
- Bad, because it keeps a hidden node per merged descendant, a second tree shape beside the folded
  configuration, which costs memory per control and a walk per request.

### 4. Owner decided while folding

- Good, because it keeps `absorb`'s one pass.
- Bad, because the role that decides `toggled`'s owner and the groups that decide `Click`'s and
  `Increment`'s subject can change at a later contributor, after the handler the final owner
  needs has been replaced or refused: the result depends on fold order beyond the table's
  precedences.
- Bad, because a blocked owner's handlers are gone by the time a later contributor asks whether
  it acted.

### 5. Owner planned over the whole merge

- Good, because each owner is decided from the final groups, so no handler is taken before its
  owner is known.
- Good, because the plan is a fixed array and the extra pass reads fragments already gathered.
- Bad, because a forced fold and an ordinary one take actions by two entry points; the ordinary
  one is the special case the conflict rules make trivial.

### 6. Refuse the split

- Good, because the published node can never be inconsistent.
- Bad, because `MergeSemantics` over a tile and its control would form two stops, which is what
  the widget exists to prevent; dropping the state instead hides the control's state.

### 7. Displaced handler as a custom action

- Good, because both handlers stay reachable.
- Bad, because a custom action needs a label and the displaced handler has none of its own: the
  tile's label is already the merged node's name, so the action would be announced by a
  framework-made string.
- Bad, because it adds a custom action, and its allocation, to every merged tile whose tap and
  control share one callback, the usual form, where nothing is displaced.

## More information

- [Issue 1070](https://github.com/vanyastaff/flui/issues/1070) (the first-in-tree-order
  contract) and [issue 2881](https://github.com/vanyastaff/flui/issues/2881) (state and action
  from different contributors, with the scroll-group comment).
- A standard-action request carries no ticket: it is matched to the delivered update by its
  action bit and resolves against the node as it is when it arrives. When a contributor joins a
  merge between an update and a request, the request can run the new owner's handler, a control
  the update did not announce. This window exists under every option here. Revisit with a
  delivered-handler check per offered standard action, like custom actions' (ADR-0206 decision 4),
  when a test or a report shows a request running a control its update did not announce.
