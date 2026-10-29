# flui-semantics architecture

The fifth tree: the accessibility tree assembled from the render tree and
exported to the platform through AccessKit. Layer rules and the crate's place in
the workspace DAG live in [`docs/architecture.md`](../../docs/architecture.md) and
this crate's `[package.metadata.flui] layer`; this file
records the per-decision rules that would otherwise read as drift.

Cross-crate protocol decisions belong in an ADR (`docs/adr/`). What belongs here
is a decision local to this crate: a translation rule, a precedence, a payload
shape.

## Mapping decisions

### Rejected drafts retain executable ownership rather than inert metadata

`SemanticsConfiguration::retire` lends each action handler to the caller's
existing `RecoveryScope`. Configuration attributes and storage buffers are
ordinary destruction; opaque handler ownership follows ADR-0127. Rendering
uses this operation after a draft validation failure, whose typed error stays
authoritative. `harness_rejected_semantics_drafts_preserve_the_error_and_callback_custody`
in flui-objects checks single and competing capture risks, forced merge, overwritten
receiver actions, blocked/custom actions, unchanged publication and subsequent
retry through the real frame pipeline. `registered_action_handlers` lends all raw
handler ownership to assembly, independent of dispatch authorization. Rendering
pins those handles before any fold can overwrite or filter them, through the
complete pass verdict and across a graft fallback. Healthy temporary pins retire
after publication and phase completion; arbitrary description before return and
old published-tree destruction retain their existing boundaries (ADR-0223).
The public
`configuration_capture_retirement_preserves_first_failure_and_healthy_destruction`
also exercises normal and custom action captures on the healthy path, preserves
the first destructor failure and retires a subsequent healthy configuration.
`harness_accepted_semantics_drafts_retire_filtered_callbacks` verifies that a
successful assembly releases filtered callbacks normally and keeps published
callbacks only until their owner retires.

### Authored regions have independent accessibility identities

[ADR-0223](../../docs/adr/ADR-0223-authored-semantics-regions.md) distinguishes
render-boundary origins from allocated region origins. `SemanticsNode` encodes
that choice as one private enum. A presentation-owned opaque source reserves
synthetic IDs below nonzero render generations; it never rewinds or reissues
released values. `synthetic_identity_exhaustion_never_wraps_or_partially_reserves`
pins the terminal boundary using a private seam. Region actions use the same
rooted-node and delivered-offer validation as render boundaries.

### Scroll ranges retain their actual axis

[ADR-0157](../../docs/adr/ADR-0157-native-scroll-range-actions.md) carries the
foundation `Axis` beside offset and bounds through configuration and export. AccessKit emits X for horizontal sources and Y for vertical sources.
Axis-neutral authored scalar metadata retains the vertical fallback; merging a
child's range takes its axis together with the values. The mounted widget row
`scrollable_accessibility_ranges_follow_the_actual_axis` checks both axes,
absence of orthogonal metadata and later offset republication.

### System settings belong to the host

Semantics collection and platform delivery remain presentation-owned. System
preferences are observations from the host's `SystemPreferences` source
(ADR-0172), not a constant-backed flag set in this crate. Consumer fallback
and policy belong to the runtime and widgets that apply each observation.

### Descendant reveal snapshots retain current ownership and geometry

[ADR-0168](../../docs/adr/ADR-0168-descendant-reveal-through-scroll-ancestors.md)
defines the render/semantics/Scrollable contract. Automatic reveal uses actual
ancestor handlers with separate unclipped root-logical geometry and a published
scroll-position basis. Detached descendant-reveal invocations validate weak per-tree membership
for the target and its full ancestor path; clones carry no authority, and
replacement, reparenting or mutable geometry access revokes stale snapshots.

Invocation and intentional snapshot destruction retire each callback separately
outside pipeline borrows. The first failure remains authoritative; remaining
user ownership is retained, live ancestor delivery continues, and healthy
destruction still runs. Public family
`descendant_reveal_snapshots_preserve_identity_failure_and_recovery` checks stale
paths, single/competing body failure, recovery and healthy/failed capture
retirement, including an uninvoked last-owner snapshot.

### Failed incremental publication keeps delivery pending

The published-node mirror and published focus advance only after the platform
callback returns successfully. A callback panic propagates to the caller; dirty
nodes remain pending, so retrying the same input delivers its changed payload
and focus. Removed identities count as absent while the update is diffed, but
leave the mirror only once it is delivered: a rejected removal leaves the
adapter holding the child, and the dirty parent's changed child list still
differs from the delivered mirror and therefore retries. Focus claimant bookkeeping describes current
tree state rather than delivered state and may likewise advance before delivery.
The callback may have accepted an update before panicking, so retries can repeat
delivery; this boundary promises progress, not exactly-once delivery.
`failed_incremental_delivery_preserves_retry_and_progress` checks failed label,
focus, and removal updates, a retry without further mutation, an idle flush,
and a subsequent independent change.

### Action resolution admits what the current tree serves and the delivered update offered

`SemanticsOwner::resolve_action` admits a request only when two things agree:
the current rooted tree can serve it, and the node as the last delivered update
showed it (the published mirror) offered it. Neither alone is enough. Only the
current tree holds handlers, so the mirror cannot decide by itself: running the
handler of a node that has since changed would act on captures the widget has
retired. The current tree alone admits what the adapter was never shown: after
a callback panic leaves an update undelivered, a request could reach an action
no assistive technology offered, or be read under a meaning the adapter never
presented. So every check is applied to both. The requested action must be one
the delivered node advertised (`accesskit_translation::offered_as` names the
AccessKit action a request answers: a numeric write arrives as `SetValue`, a
disclosure transition as itself), which also carries the delivered expanded
state, since a node advertises only the transition its state allows. A
numeric value must lie in the current range and in the delivered minimum and
maximum. A `SetValue` text goes to the numeric handler of a range with no text
handler, and to the text handler otherwise; the delivered node decided which
one the platform meant, so a text is refused while the current node would route
it to the other kind, in either direction. Which handler a write reaches is not
in the AccessKit payload, so the mirror records it beside each node's custom
actions, once that node is delivered. A reveal is refused while either node is
hidden, whether ancestors or the node's own handler would serve it; a custom
action must be the one delivered
under its id, compared by identity (mapping decision 1). A node an update on
record did not carry is `NodeNotFound`, since the adapter cannot have offered
anything on it. With no update on record (nothing published yet, or
`schedule_full_publish` forgot what a reconnecting adapter holds) the current
tree alone decides, as there is nothing to contradict. A replacement callback
that panics on its first update installs nothing, so the previous adapter's
mirror stays in force rather than becoming "no update on record". The refusal lasts until
the next successful publication advances the mirror. The in-process wire read
projects the current tree, so across a failed publication an agent can be
refused an action it read; that is the safe direction, and it clears with the
same publication.

`a_failed_publication_admits_only_what_both_nodes_offer` (`tests/incremental_flush.rs`)
rejects an incremental update for a disclosure whose state changed, a button
replaced by a paragraph, a moved numeric range, a text field turned into a
numeric-only range, an added child and a reveal target whose visibility
changed, then checks that an action only the rejected node offers and one only
the delivered node offered are both refused, and that the next accepted
delivery admits what it carried. Its other rows cover a rejected removal whose
child is restored before the retry (still actionable), a range whose only value
handler changed kind either way (a text write reaches neither), and a
replacement callback that rejects its first update (the installed adapter's
mirror still decides).

### Parent-specific detachment preserves both sides of a link

`SemanticsTree::remove_child` detaches only a child whose current parent is the
live parent supplied by the caller. A stale or different parent is a no-op;
clearing the child's actual parent in that case would leave its old parent's
children list pointing at it and defeat `add_child`'s automatic reparenting.
`detaching_checks_the_actual_parent_and_preserves_reparenting` exercises the
owner wrapper, refused detachment, reparenting, published parent child lists,
and a successful detach followed by reattachment.

### 1. One role per node; a forced merge keeps the more specific one

**Rule.** [ADR-0206](../../docs/adr/ADR-0206-typed-semantics-attributes.md): a node has one
`SemanticsRole`, interactive kinds included (`Button`, `CheckBox`, `RadioButton`, `Switch`,
`Link`, `Slider`, `Image`, `Heading`, the text-input kinds), and typed `States`. Two
configurations that each declare a role, or each declare the same state, form separate nodes
(`is_compatible_with`); a state declared `false` counts, so a node's shape does not depend on
its current values. A forced merge (`MergeSemantics`) cannot separate them and takes the role
by `more_specific_role`: an authored structural role (tab, list item, dialog, …) over any
interactive kind, then a checkable role, then `Button`, then link, text field, slider or other
value control, then content (image, heading, a text-level role, or a read-only value such as
`Meter` or `ProgressBar`); on a tie the receiver keeps its own. States merge by their type's
rule: `Mixed` over `On` over `Off`, and every other state (a yes/no state, `Invalid`,
`Current`, `HasPopup`, `AutoComplete`, an orientation) as the receiver declares it, else as
the contributor does, so a disabled tile is not enabled by a part that declares itself enabled
and a field that passed validation is not failed by a part. `Invalid::False` is a declaration:
two fields that each validated form two nodes (`a_passed_validation_counts_as_declared`).
The roles `Generic` and `Presentation` declare a node to mean nothing: it publishes as a
container, and its label does not make it static text as a node without a role would.

Three groups describe one thing each and never mix: a scrollable's metrics (position, axis,
extents, scroll index), an entry's place in a set (index, child count) and a field's lengths
(current, maximum). Either side declaring any member of a group conflicts with the other
declaring any member; a forced merge keeps the receiver's group whole or takes the
contributor's whole. A node offers any number of custom actions, each added with its label and
its own handler, and two sets of them share a node. Their `CustomActionId` is the action's place
in the node's list: a merge appends the contributor's after the receiver's and so renumbers them,
but a handler always receives the id its own configuration gave the action, so a widget that
registered several actions behind one route still knows which ran. A request runs only while its
id still names the action the last delivered update offered there, compared by identity — the
same handler allocation, held weakly, the same own id and the same label — never by label alone,
so two same-labelled actions that trade places are told apart; an id whose place now holds
another action, or none, is refused. The record holds what the delivered update offered: a
blocked node offered none, and a full update records only its rooted, addressable nodes. One
`Semantics` widget routes all its actions through one handler, so within it a label names one
action: a repeated label replaces the earlier action in place. Removing `CustomAction` removes
every custom action.

**Why.** One role per node is AccessKit's, Chromium's and ARIA's model: the role sent to the OS
is the one the node declares, and a node that is two things at once is not representable. The
precedence settles the merges a widget cannot avoid: `MergeSemantics` over a `ListTile`
carrying a `Radio` is one radio button, not a button, and a structural role (a navigation tab
that is also tappable) stays structural. Outside a forced merge two roles are two nodes: a
button configuration over a `Radio` publishes the button and the radio, so neither is lost. A read-only value ranks as content because it carries no action:
a meter merged with a button would otherwise publish the button's actions under a role that
announces none. A group mixed from two sources would publish a position from one scrollable
against another's extents, an index in one set against another's count.

A node that merges its own descendants keeps its node inside an outer merge
(`nested_merge_semantics_keeps_its_own_node`): a card merging its content still lets a screen
reader stop on the tile inside it.

**Test.** `role_precedence_settles_forced_merges`, `state_merge_precedence`,
`a_forced_merge_keeps_one_scroll_group`, `a_forced_merge_keeps_one_collection_and_text_group`
and `a_forced_merge_keeps_custom_actions_with_their_handler` in `configuration.rs`;
`custom_actions_publish_with_their_ids` in `accesskit_translation.rs`;
`custom_actions_run_their_own_handler_until_stale` in `owner.rs`;
`merged_custom_actions_each_reach_their_own_handler` in `crates/flui-widgets/tests/semantics.rs`;
`merge_semantics_over_a_tile_and_radio_announces_as_one_radio_button` in
`packages/flui-material/tests/list_tile.rs`; `a_mounted_radio_announces_as_a_radio_button` in
`packages/flui-material/tests/radio.rs`; `compatibility_rules_for_modeled_fields` for the
conflict rows; `nested_merge_semantics_keeps_its_own_node` in
`crates/flui-rendering/tests/semantics_assembly.rs`.

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
- Geometry composes each parent edge through `RenderNode::apply_paint_transform`,
  using its committed child offset and current paint transform. Full assembly
  and graft ancestor reconstruction use the same operation. Node bounds,
  unclipped reveal bounds and parent clips are projected into root coordinates
  before clipping; there is no second source of node geometry.
- A pass re-assembles only the subtrees that can observe the frame's semantics marks, grafting
  each into the persistent semantics arena under its **anchor** — the nearest unmarked ancestor
  that formed a node last pass. A formed node absorbs every pending fragment below it and its
  own forming decision does not depend on its descendants, so re-assembling the anchor's subtree
  reproduces everything a mark could influence. Anything the graft preconditions cannot prove
  falls back to the whole-tree rebuild, which is the correctness baseline.
- `accesskit_translation.rs` maps the owner's tree to AccessKit updates with stable
  `AccessibilityNodeId`s; `flui-native` hosts the AccessKit adapters per backend.
- `platform.rs` holds `PlatformAccessibility`, the capability those adapters implement and the
  UI runtime holds per window (ADR-0082 §2). It names only AccessKit types, never a semantics type:
  what crosses it is already translated.

**Incrementality.** The observable `SemanticsNode` tree is the contract; incrementality is
anchor-scoped re-assembly over the classic merge model, not a per-node fragment state machine.
Freshness is mark-driven: geometry outside re-assembled subtrees keeps its last published
value.

**Publication is a tree or nothing.** AccessKit assumes a tree, requires one node per id, and
rejects a full update with a node no parent lists; the public node APIs can build a cycle, a
child under two parents, an unattached node, a node with no identity or two nodes with one
identity. The published set is the root's walk over addressable nodes: a node with no stable
identity is not published and neither is its subtree, since its parent lists none of its
children. A full update built from that walk refuses a node reached twice and two of its nodes
sharing an identity; an unattached node is simply left out, even when it shares a published
node's identity. An incremental flush carries content changes only. A change that reshapes the
tree (a node the adapter does not hold, or changed children) escalates to the validated full
publish, and so does every flush while any identity is aliased, since a diff could carry the
unattached alias's payload under the published id. Content changes stay O(dirty); a reshaping
change pays one full publish. Everything an update names stays inside the set it carries:
focus is resolved among published nodes, a relation names only published targets, and a clean
relation source the adapter does not hold is not re-translated.

**Actions resolve over the set publication carries.** `resolve_action` walks the same rooted,
addressable nodes of the current tree, so a request naming a node that left that set
(unattached, or moved under a node with no identity) is `NodeNotFound` and reaches no handler.
Two rooted nodes with the requested identity are `AmbiguousNode`, even while the adapter still
shows the last update that held one of them: the arena is rebuilt every pass, so nothing ties
the published identity back to either node, and running one would be a guess. The refusal
lasts until the alias clears and the next flush publishes again.

**Test.** `tests/incremental_flush.rs`: `a_child_under_two_parents_is_not_published`,
`a_cyclic_tree_is_not_published`, `a_cycle_after_publication_is_not_published`,
`a_full_update_leaves_out_unattached_nodes`, `an_unaddressable_node_leaves_out_its_subtree`,
`an_aliased_identity_publishes_nothing_until_it_clears`,
`an_unattached_alias_does_not_withhold_the_tree`, `an_unattached_focus_claim_does_not_steal_focus`,
`a_relation_to_an_unattached_target_names_nothing`,
`an_unattached_relation_source_stays_unpublished` and
`an_action_under_an_unaddressable_node_is_not_found`.

**Modal blocking is structural, not paint-ordered.** No render object blocks "previously
painted siblings". A modal route's overlay entry tells `RenderTheater` how many entries below it
to skip for semantics (`flui-widgets` overlay, "Modal entries"), and the dialog's own node carries
the `modal` state, published as AccessKit's `modal`. Test:
`a_shown_dialog_hides_the_page_beneath_from_assistive_technology` in
`packages/flui-material/tests/show_dialog.rs`;
`harness_theater_semantics_blocked_entries_stay_laid_out_but_unannounced` in `flui-objects`.

**Not implemented.** Sibling merge groups beyond configuration-conflict marking.

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
text's name through `accesskit_consumer` the way the adapters do. For the joined label and value:
**Unasserted:** no test pins this.

### 5. The ADR-0080 tools reach FLUI through AccessKit's Windows adapter: `expand`/`collapse` are discrete, with a click fallback

**Rule.** [ADR-0203](../../docs/adr/ADR-0203-semantics-vocabulary.md): `SemanticsAction` follows
`accesskit::Action`, and `semantics_action_for` names every AccessKit action. A handler receives
an `ActionCall`, the action fused with exactly its payload; `semantics_action_request_for`
decodes a platform request into one or refuses it with `ActionDecodeError` (a missing or
mismatched payload, an unrouted action, an id FLUI never publishes) before any handler runs.
The tools an agent calls (ADR-0080, `flui_protocol::ActionName`) arrive through
accesskit_windows 0.35.0 as: `invoke`, `toggle` and `select` → `Click`; `set_value` →
`SetValue` with its text, or `SetNumericValue` with its number when the request carries
`ActionData::NumericValue` (a slider through UI Automation's `RangeValue`) or, on the wire,
when the node publishes a numeric value; `focus` → `Focus`; `scroll_into_view` →
`ScrollIntoView`; `expand` and `collapse` → `Expand`/`Collapse` (ADR-0124).

A node that declares an expanded state (`States::expanded` is `Some`, as `Semantics::expanded`
sets it) advertises `Expand` while collapsed and `Collapse` while
expanded, never both. It advertises the transition when it registers the matching discrete
action; when it registers neither discrete action but has a click handler (the shape
`Semantics::new().expanded(false).on_click(..)` builds), it advertises the transition too, and
`SemanticsOwner::resolve_action` routes that request to the click handler as `Click`. The owner
admits an `Expand` or `Collapse` request, to a discrete handler or the click fallback alike,
only in the direction the node's current state allows (`disclosure_transition` in
`src/action.rs`); `tap_disclosure_transition` there is the one fallback predicate both
directions use.

**Why.** AccessKit does not count a node with an expanded state as invocable
(`accesskit_consumer` 0.39, `Node::is_invocable`), so the Windows adapter offers only the
`ExpandCollapse` pattern for it: without the fallback a click-only expandable node could be
neither invoked nor expanded by an agent or a screen reader. A discrete handler receives the
requested direction and sets that state, so repeated requests are idempotent. No other shipped
adapter (`accesskit_macos` 0.27, `accesskit_atspi_common` 0.20) emits `Expand`/`Collapse`.

**Limit.** The click fallback carries no direction to the handler. The adapter checks the
expanded state in its own copy of the tree and the owner checks the committed tree, and both
change only when the next frame publishes. Two `Expand` requests before then, or an `Expand`
right after an unpublished pointer tap, each run the click handler, so a click-only node can end
collapsed after an expand. A node that needs the direction registers its state and both handlers together with
`Semantics::expandable`.

**Value writes.** A value write's payload picks its handler: a string reaches `SetValue`, a
number `SetNumericValue`. A node advertises `SetValue`, and a numeric range is published
writable, when some write a platform can send it has a handler (`value_writable` in
`accesskit_translation.rs`): the numeric handler on a range, whatever text the range also
shows, and the text handler on a node without a range or on a range that shows text. The
adapters send both payloads independently: `accesskit_windows` 0.35 offers UIA's `Value`
pattern on a node with a text value and `RangeValue` on one with a numeric value
(its `node.rs`, `is_value_pattern_supported`, `is_range_value_pattern_supported`), and their
`SetValue` methods send `ActionData::Value` and `ActionData::NumericValue`; `accesskit_macos`
0.27 sends an `NSString` or an `NSNumber` from `setAccessibilityValue:`;
`accesskit_atspi_common` 0.21 sends `Value.CurrentValue` as a number. A range none of whose
writes has a handler is marked read-only, because AccessKit otherwise reports a slider writable
(`accesskit_consumer` 0.39, `Node::is_read_only`).

A string written to a range that has no text handler is offered to its numeric handler when
it parses as a finite number (`SemanticsOwner::resolve_action`), so a `50%` slider with only a
numeric setter takes a `Value`-pattern write of `40`, from a screen reader or from the desktop
server, which writes through `Value` ahead of `RangeValue`.

**Limit.** AccessKit has one read-only flag, which `accesskit_windows` reports as both
patterns' `IsReadOnly`, so a range that shows text and registers only one of the two handlers
is published writable through both. On a numeric-only range a `Value` string that is not a
number is refused; on a text-only range every `RangeValue` number is. Either refusal comes
after the adapter reported the write accepted. Marking the node read-only would hide the write
that works, which is the worse failure.

**Test.** `every_wire_action_routes_to_a_semantics_action` (one row per `ActionName::ALL`) in
`src/agent/tests.rs`; `every_action_round_trips_through_accesskit` and `platform_requests_decode_to_their_typed_call` in
`accesskit_translation.rs`; `disclosure_requests_follow_the_expanded_state` in
`src/agent/tests.rs`, whose rows drive explicit and click-only nodes in both states through the
wire and platform paths to the handler that runs; `set_value_reaches_the_handler_with_its_payload`
for text and numeric `set_value`; `set_value_follows_the_value_pattern_precedence` in
`src/agent/tests.rs` for the handler each value shape runs;
`a_numeric_range_is_writable_only_through_the_handler_a_write_reaches`
in `accesskit_translation.rs` for range writability.

### 6. Every role maps to its own AccessKit role

**Rule.** [ADR-0203](../../docs/adr/ADR-0203-semantics-vocabulary.md): `SemanticsRole` holds
every non-abstract WAI-ARIA 1.2 role with an AccessKit counterpart, the editable-text kinds
and the desktop roles, and admits nothing else; `explicit_role` gives each, except `None`, an
AccessKit role of its own. ARIA's `separator` is AccessKit's `Splitter`, as in Chromium; an
indeterminate progress bar is a `ProgressBar` without a value.

**Why.** `SemanticsRole` lives in `flui-protocol` and is `#[non_exhaustive]`, so the match in
this crate ends in a wildcard and the compiler no longer catches a forgotten arm. A role
without an arm would silently publish as a label or a generic container, and two roles
sharing one AccessKit role would be indistinguishable to every assistive technology.

**Test.** `every_role_maps_to_accesskit` walks `SemanticsRole::ALL`: deleting the `Form` arm
makes it fail with `form maps to no AccessKit role`.

### Relations resolve through anchors at publication

**Rule.** [ADR-0205](../../docs/adr/ADR-0205-semantics-relations-by-anchor.md): a target declares
a `SemanticsAnchor`, a source holds `(SemanticsRelation, anchor)`, and both merge as unions. The
full path indexes the whole tree; the owner's `RelationIndex` keeps anchored nodes and relation
sources from dirty nodes, like the focus claimants, and an incremental flush re-translates every
clean relation source against the fresh index. Targets are published sorted, so an unchanged
relation diffs as unchanged.

**Why the clean sources are re-translated.** A mark-scoped graft can remount a target under a new
identity and leave its source's subtree untouched; diffing only dirty nodes would keep the old id.
Sources are few, and the mirror diff discards the ones that did not change; an incremental flush is
O(dirty + anchors + relation sources), and the scaling oracle counts all three
(`an_incremental_flush_counts_the_relation_work_it_does`).

**Test.** `relations_resolve_through_anchors_across_publication` in `tests/incremental_flush.rs`,
red with the clean-source loop removed.

### A live region's politeness is explicit, else implied by its role

**Rule.** `set_live_region(Some(politeness))` records the node's `Assertiveness`, and
AccessKit's `live` follows it. A node with no explicit politeness takes the one ARIA gives its
role: `Alert` assertive, `Status` and `Log` polite, `Timer` and `Marquee` off.
An explicit politeness wins over the role's.

**Why.** Live regions are how a change reaches a screen reader without the user moving to it:
AccessKit has no imperative announcement, so `SemanticsHost::announce` has no platform path and
a widget that must speak (a snack bar, a validation error) does it by being a live region.
A yes/no live flag could only say polite, and an `Alert` that stayed silent until visited is
not an alert. `Timer` and `Marquee` publish `off` rather than nothing: an absent setting would
inherit an enclosing region's politeness and announce every tick.

**Test.** `live_regions_announce_at_their_politeness` in `accesskit_translation.rs`;
`a_shown_snack_bar_is_announced_as_a_polite_status` in
`packages/flui-material/tests/snack_bar.rs`.

### A header publishes AccessKit's `Heading` at its level

**Rule.** `SemanticsRole::Heading` publishes `Role::Heading`, and its `HeadingLevel` (`H1`–`H6`,
one-based as `aria-level` is) is published as AccessKit's zero-based `level`. A heading with no
level publishes none, and a level on a node of another role is not published.

**Why.** AccessKit's `Header` is HTML's `<header>` landmark: `accesskit_windows` 0.35 reports it
as a UIA `Group` with the `banner` landmark type (`node.rs`), so a title published as one never
reached a screen reader's heading navigation. `Heading` is UIA's `Text` with the `heading` ARIA
role, VoiceOver's `AXHeading` and AT-SPI's `Heading`. The level is an enum rather than an
integer because the six levels are the ones every platform names and a zero or out-of-range
level is not a state to validate at run time.

**Limit.** Measured through UI Automation on an `H2`: `accesskit_windows`
0.35 reports `AriaRole` `heading` and UIA `Level` 2, but `AriaProperties` `level=1` (AccessKit's
zero-based value written raw) and `HeadingLevel` `HeadingLevel_None`. FLUI publishes AccessKit's
documented zero-based level rather than compensating for one adapter; a client reading
`AriaProperties` sees the level one lower until the adapter is fixed.

**Test.** `a_header_publishes_a_heading_at_its_level` in `accesskit_translation.rs`;
`the_title_announces_as_a_level_one_heading` in `packages/flui-material/tests/app_bar.rs` for
the mounted path; `every_interactive_role_reads_as_the_role_uia_reports` for the wire fold.

### Where a node sits in a set, a tree and a table

**Rule.** A node's place in a set (its position, and the size of the set it contains), in a tree
(its level), in a table (the table's shape, a cell's row, column and spans, a header's sort) and
a container's selection rule are one group, `Collection` (`src/collection.rs`). It merges per
field: two nodes that each declare the same one cannot be one node, and agreeing does not help.
Two lists of three rows are two sets whose positions each restart at zero, which one container
could not say, so equal sizes are not an identity; slivers a caller composes into one set keep
their own containers, each with the whole set's size and the positions it continues from. A
member still merges with its container.

**Position on the item, size on the container.** AccessKit reads `size_of_set` from the nearest
ancestor the consumer keeps (`size_of_set_from_container`), never from the item; a node can carry
both, a tree item holding a nested list. The consumer drops a `GenericContainer`, so a size on a
node with no declared role would be read by nobody: such a node publishes as a `List`, and is
held to what ARIA gives a list, so a table shape or a selection rule declared beside its size is
dropped as it would be on a declared one. A set whose
size is `SetSize::Unknown` publishes no size at all, and so does one too large for the adapters to
convert (past `i32::MAX`), which every reader therefore sees as unknown from the moment it is
stored, since AccessKit has no way to say "unknown"
and a number would claim the set ends where the built members do.

**A position counts from zero; a level, a row and a column from one.** A position is
zero-based as AccessKit's is (the adapters add one). A level, a row and a column are the numbers
ARIA's authors write, `1` for the first, and are `NonZero` types so that a zero-based first row
cannot be passed by mistake; the translation subtracts the one AccessKit does not have. Spans of
one are not published.

**Checked by role.** A level, a table's shape, a cell's position, a row's index, a sort and a
selection rule are authored, so the ARIA roles that support them are listed in
`applicability.rs`; a row takes an index of its own, not a cell's column and spans. Position and
size are not checked by role: the framework derives them, whatever role the node declares. The
assembly checks them against each other once the tree is built (see flui-rendering's
architecture): a position past its own known set's size is not published, and the totals of the
known sets above an unknown set, or above a member numbered for another set, are withheld
(ADR-0227), since the consumer would read "item 8 of 3" or "item 12 of 5". A lazy host's
`SemanticsCollection` and a `size_of_set` a caller declares are both sets to that check.
Anything past what the adapters convert to an `i32` (a position, a level, a count, a row, a
column, a span) is not published either.

**Reach.** `accesskit_windows` 0.35.1 reads the position and size, a level and a sort into UI
Automation (`PositionInSet`, `SizeOfSet`, `Level`, `AriaProperties`), and not a table's counts,
a cell's indexes and spans or a selection rule (upstream marks tables TODO). Those are published
and pinned through `accesskit_consumer`; a UI Automation fixture for them waits for the adapter.

**Test.** The collection rows of `every_published_attribute_reads_through_the_consumer` and
`an_inapplicable_attribute_is_not_published`, `a_member_reads_its_size_from_its_container`
in `src/accesskit_translation/coverage_matrix.rs`; `collection_places_conflict_one_by_one` in
`src/configuration.rs`; the lists, tables and trees of `collection_semantics` in
`crates/flui-widgets/tests`.

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
headers, a switch, a heading) and its `IsPassword`/`IsDialog` refinements. `native_role` keeps the AccessKit
name. `resolve_wire_action` refuses an action the node does not advertise *now*, so `expand` on
an expanded node is `action_unsupported`.

**Why.** One derivation for both backends: the desktop server reads the same AccessKit node
through UI Automation, so a second derivation from FLUI's roles would drift from what the OS
reports. The fold targets UIA rather than AccessKit's full role set because the wire
vocabulary is the desktop server's, and ADR-0095 §4 compares the two backends after
normalization. The direction check reads the last committed tree, which lags the request as
the adapter's copy does; that lag matters only for a click-only expandable node (mapping
decision 5, Limit).

**Geometry and dialog roles.** The rectangles are
surface-relative physical pixels, reported as `surface_rect` with no screen `rect`: the UI runtime
knows no window position, and a client that reads `rect` as screen pixels must find none rather
than a misplaced one. `AlertDialog` and `Dialog` read as `dialog` (UIA's `Window` with `IsDialog`),
and a `Keyboard` key or `TabPanel` as `pane`, as the desktop server would report them.

**Test.** `src/agent/tests.rs`: `every_role_but_none_reads_as_a_wire_role`
(ADR-0095 "Mapping pinned"; `none` is the documented exception),
`every_interactive_role_reads_as_the_role_uia_reports`,
`wire_role_matches_the_windows_adapter_for_every_role_flui_publishes` (transcribed from the
adapter and desktop sources, and checked to cover every role FLUI publishes),
`the_role_fold_was_transcribed_from_the_locked_windows_adapter` (fails when `Cargo.lock` moves
`accesskit_windows` off the release the fold was transcribed from),
`generic_containers_are_lifted_and_hidden_subtrees_dropped`,
`advertised_actions_follow_the_uia_patterns`, `disclosure_requests_follow_the_expanded_state`,
`set_value_reaches_the_handler_with_its_payload`, `a_disabled_node_refuses_with_disabled`,
`read_honours_max_depth_and_max_nodes_and_says_truncated` and
`a_read_tree_round_trips_through_json`. `every_wire_action_routes_to_a_semantics_action` pins
`semantics_action_for_wire` to the Windows adapter's route row by row.


### The coverage matrix reads every attribute back through `accesskit_consumer`

**Rule.** Each role and each attribute FLUI models is published on a node under a window root
and read back through `accesskit_consumer`, the tree model every adapter reads: the role passes
`common_filter` under its own AccessKit role and is named by its label, and each attribute
reaches the consumer as its AccessKit property (`coverage_matrix.rs`). An attribute or state
added to the model does not compile until the matrix gives it a row or the reason it is not
published (framework bookkeeping, or a property AccessKit does not have). The matrix
is pinned to the AccessKit releases it was read against: bumping `accesskit` or
`accesskit_consumer` fails until its rows are re-read, as bumping `accesskit_windows` does for
the wire fold.

**Why.** Comparing the fields translation wrote proves only that translation wrote them; a
property the consumer reads under another accessor, filters out or names differently passes
such a test and still never reaches a screen reader.

**Naming.** The consumer names a node from its own label, then from its `labelled_by` targets,
and a button, link, check box, radio or menu item with neither from its descendants' text (AccName
1.2's name from content, as `accesskit_consumer` implements it). The matrix pins the first and
the last; `a_labelled_by_target_names_its_source_through_the_consumer` the second.

**Set position.** The position is the item's and the size is its container's; the matrix pins
each, and `a_member_reads_its_size_from_its_container` reads the pair the way an adapter does
(see "Where a node sits in a set, a tree and a table").

**Test.** `every_published_attribute_reads_through_the_consumer`,
`every_role_is_kept_and_named_through_the_consumer`,
`a_control_without_a_label_is_named_by_its_content` and
`the_matrix_was_checked_against_the_locked_accesskit` in `src/accesskit_translation/coverage_matrix.rs`.

### An attribute the declared role does not support is not published

**Rule.** `applicability::applicable(role, attribute)` encodes WAI-ARIA 1.2's supported states
and properties per role for the role-dependent ones an author declares: checked or pressed,
selected, expanded, read-only, required, modal, the numeric range, the placeholder and a
heading's level. FLUI's text-input kinds read as `textbox`. One table, `FIELDS`, ties each of
them to the fields that hold it; translation reads `retain_applicable`'s view of a node (borrowed
when nothing is left out), and the owner's report and its action resolution read the same view, so
a state left out is neither published nor routes a request; the two cannot
disagree. Two cases are checked against the node as well as its role: a toggle's mixed state,
which ARIA forbids on a radio button, a switch and a radio menu item, and a separator's range,
which ARIA defines only for a focusable (interactive) separator. A row's or a header's expanded, read-only and required states, and a grid
cell's expanded state, depend on the table around it (a tree grid, a grid), which the role-only table cannot see, so they are left
out. A node with no declared role (`SemanticsRole::None`) is not checked: it names no ARIA
role whose table could reject anything. A set's position and size are not checked: the framework
derives them from the collection a node sits in, whatever role the node itself declares.

The owner queues what a publication left out, once per node and attribute within it, and reports
nothing itself: a flush runs inside the pipeline's checkout and the widgets binding's lock, and a
`tracing` subscriber is user code that may reach either. After each flush the pipeline moves the
queue onto itself, so disabling semantics or replacing the owner does not drop it. Every frame
driver reports it once its locks are released (`PipelineCell::report_semantics_diagnostics`: the
runtime's frame, a failed one included, once layout availability reflects it;
`RenderingBinding::draw_frame`; the headless harness's frames): the findings move out under one
brief checkout, so a subscriber that replaces the owner cannot retire another's finding, and a
subscriber that panics puts back what it left unreported, its own finding included, ahead of
anything queued since. The runtime contains a reporting panic, so the frame's outcome or error
stays the one returned; the runtime also schedules another frame to retry the report.
`RenderingBinding::draw_frame` lets the panic propagate and first gives the painted tree back to
the pipeline, with no call out, so the next draw delivers the frame. A finding still queued and the same finding from a
newer change are two reports. `flui-testing` fails every harness frame (the widget host's and
`HeadlessBinding`'s) that left anything out, from a record the pipeline keeps apart from the
report queue (`PipelineOwner::take_unchecked_semantics_diagnostics`, with the `testing` feature),
so a reporting subscriber cannot hide a finding and a widget's author error surfaces in its own
tests.

**Why.** ARIA 1.2 requires a user agent to ignore a non-global state or property on a role that
does not support it; publishing one anyway hands
each adapter a combination it handles differently. A widget declares a state where its role
carries it: a filter chip, a toggle button, publishes its selection as pressed; a list tile
marked selectable is an option (`ListBoxOption`) that always declares whether it is selected, so
its role never changes with selection, and any other tile declares no selection.

**Test.** `applicability_follows_aria`, `every_role_attribute_has_one_field_row` and
`the_view_drops_what_the_report_names`, `a_mixed_toggle_is_supported_only_where_aria_allows_it`
and `a_range_is_supported_on_a_separator_only_when_focusable` in `src/applicability.rs`;
`an_inapplicable_attribute_is_not_published` in `src/accesskit_translation/coverage_matrix.rs`;
`an_inapplicable_attribute_is_reported_once_per_change` and
`a_queued_diagnostic_and_a_newer_change_are_two_reports` and
`a_state_the_role_does_not_support_routes_no_action` in `owner.rs`;
`a_diagnostic_is_reported_outside_the_pipeline_checkout` in
`crates/flui-widgets/tests/semantics.rs`; `a_selectable_tile_is_an_option_whether_or_not_selected`
and `a_filter_chip_announces_its_selection_as_pressed` in `packages/flui-material/tests/`;
`draw_frame_reports_semantics_diagnostics` and `a_panicking_report_leaves_the_frame_for_the_next_draw` in
`crates/flui-runtime/src/renderer_binding.rs`; `a_panicking_semantics_report_schedules_its_retry` in
`crates/flui-runtime/src/ui_runtime/tests/frame_failure_containment.rs`;
`a_headless_frame_fails_on_an_attribute_its_role_does_not_support` in
`crates/flui-testing/tests/a11y_query.rs`.

### A role ARIA forbids a name carries its label as its description

**Rule.** WAI-ARIA 1.2 forbids a name on caption, code, deletion, emphasis, insertion, paragraph
and strong (`applicability::prohibits_name`). A node of one of those roles publishes no label;
its label becomes the description, ahead of the hint and joined to it by a newline. A node with no
role and a label still publishes as static text named by its value (mapping decision 4).

Relations follow the same rule. The nodes labelling such a node name it no more than its label
does: their text, joined by spaces, leads its description, since no shipped adapter reads
`described_by` (ADR-0205). A target's text is what an adapter names from it: a static text's
published value (label, then value), anything else's label, never a control's value (it names
nothing, and a password's would leave its node's protection). Conversely, an adapter names a node
from its labelling nodes' own names and finds none on such a role, so a node labelled by one is
named here, from every labelling node's text, unless it is named directly. The relation index
carries each target's text, so a caption that changes under a clean source republishes it.

**Why.** Dropping the label would lose the text for the user; publishing it as the name hands the
adapter a combination ARIA rules out ([Name From: prohibited](https://www.w3.org/TR/wai-aria-1.2/#namefromprohibited)
covers `aria-label` and `aria-labelledby` alike). The description keeps the text reachable.

**Test.** The `label of a role ARIA forbids a name` row of
`every_published_attribute_reads_through_the_consumer` and
`every_role_is_kept_and_named_through_the_consumer` in `src/accesskit_translation/coverage_matrix.rs`;
`a_labelled_by_target_names_its_source_through_the_consumer` in `tests/incremental_flush.rs`.

### Publication reads attributes in place

[ADR-0206](../../docs/adr/ADR-0206-typed-semantics-attributes.md): a configuration's
payload is one `SemanticsAttributes`, and publication reads it through a borrowed
`PublishView` (identity, effective actions, bounds, addressable children) rather than a
copied payload, so there is no second shape to keep in step with it; the platform's
cross-thread handoff is the AccessKit `TreeUpdate`. `SemanticsAttributes` is the crate's:
consumers read and write a node through `SemanticsConfiguration`'s accessors. A selection-only annotation still reaches the
platform (`a_selection_only_annotation_reaches_the_platform`).

### The identifier names only the node its own configuration forms

**Rule.** [ADR-0240](../../docs/adr/ADR-0240-semantics-identifier.md)'s identifier is published as
AccessKit `author_id` and as the wire `Node.identifier`, and never as text. Setting it neither marks
the configuration annotated nor makes it a boundary, and `absorb` never takes it: a configuration
with only an identifier contributes nothing, and one absorbed into an ancestor or a
`MergeSemantics` loses it while the receiver keeps its own. A widget names its node where the node
is formed: a container `Semantics`, `MergeSemantics::identifier`, or a control whose own
configuration is the outermost of its parts.

**Why.** An identifier that annotated or split nodes would change the tree, and so what a screen
reader says, by adding a test id; one carried upward would name a node with another node's id, and
a merged parent would collect its children's.

**Test.** `the_identifier_is_exported_unannounced_unmerged_and_no_boundary` in
`tests/identifier.rs`; the `identifier` row of `every_published_attribute_reads_through_the_consumer`.

### Siblings sharing an identifier are reported, not repaired

**Rule.** An identifier is unique among its siblings (ADR-0240 §4), and nothing enforces it:
translation publishes every identifier as its author wrote it. Siblings are the children an
agent or OS client sees: `accesskit_consumer::common_filter`'s, where an unlabelled container's
children join its parent's. `read_wire` lists a `TreeDiagnostic::DuplicateIdentifier` for each
clash, naming the parent and the siblings in tree order, and checks the children of every node
it reaches, including those its budget leaves out. In debug builds, a publish that changes an
identifier checks the published tree and queues a `SemanticsDiagnostic::DuplicateIdentifier`
for each clash it has not reported in that shape; the frame driver logs it with no lock held,
like every owner diagnostic, and the testing kit fails a frame that leaves it unchecked. The
same identifier on nodes that are not siblings is no clash.

**Why.** Dropping or rewriting a duplicate would change what the author wrote, and a locator
matching the written value would then miss it; reporting the clash as data lets a test or agent
assert on it, where a log alone would not. Logging inside `flush` would run a subscriber, which
is user code, under the pipeline's borrows.

**Test.** `siblings_sharing_an_identifier_are_reported_and_published_as_written` and
`debug_builds_queue_a_published_clash_once_per_shape` in `tests/identifier.rs`.

### Locators and actionability are pure reads of the published tree

**Rule.** A `Locator` resolves over the filtered tree the adapters see, reading the same wire
`Node` as `semantics.read`, so a locator matches what the outline shows. `within` and `has` are
scopes over that tree: an unlabelled container that is lifted away is no ancestor. A strict
locator lists every match in `locate_wire`; `resolve_wire_locator` is where one element is needed
and answers `Ambiguous` (the first ten outline lines) or `NotFound` (the ten nearest by held
criteria, then name distance). Actionability is two calls with the caller's hit-test between them:
`actionability_point` says where a pointer would land, `actionability_wire` takes the render
objects the hit-test found there, most specific first. A path maps to the first render object
whose node the filtered tree shows, because a published node's id is its render object's id.
`SemanticsOwner::revision` grows with each publication that delivered something, and matches and
facts carry it.

**Why.** The hit-test is render-tree code and user code (a render object's hit test), so it must
not run under a semantics borrow; splitting the check keeps the owner free of callbacks. Naming
the node by its render object's id means there is no index to go stale, and a handle's generation
makes an old handle detached after its slot is reused. `visible` leans on the clip the semantics
assembly already applies and adds only the window region: a second clip computation here would be
a second place to be wrong.

**Test.** `locate.rs` and `actionability.rs` in `tests/`.

## Numeric and directional input

ADR-0124 distinguishes expand/collapse from activation and numeric setters from
text edits (mapping decision 5). `NumericRange` admits only finite inclusive
values and positive finite steps, and `set_numeric_range` marks the node as a
slider. Whole-request platform translation preserves numeric payloads, and
`SemanticsOwner::resolve_action` refuses a numeric setter whose value is
missing, non-finite or outside the node's current range
(`numeric_setters_are_checked_against_the_current_range`), or outside the range
the last delivered update showed (mapping decisions, "Action resolution admits
what the current tree serves and the delivered update offered").
