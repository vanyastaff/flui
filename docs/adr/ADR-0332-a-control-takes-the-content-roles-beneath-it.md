# ADR-0332: A control named by its contents takes the content roles beneath it

- **Status:** Accepted (2026-11-13)
- **Date:** 2026-11-13
- **Supersedes:** [ADR-0206](ADR-0206-typed-semantics-attributes.md), in part: decision 4's
  statement that two declared roles always form separate nodes outside a forced merge.
- **Related:** [ADR-0206](ADR-0206-typed-semantics-attributes.md) (the typed attribute model,
  the merge table and the role precedence this decision reuses);
  [ADR-0203](ADR-0203-semantics-vocabulary.md) (the roles);
  [ADR-0322](ADR-0322-a-control-mounts-one-element-for-its-behaviours.md) (a control is one node, which makes
  its semantics one configuration over its child)
- **Sources:** Flutter 3.27.0 `SemanticsConfiguration.isCompatibleWith`
  (<https://github.com/flutter/flutter/blob/3.27.0/packages/flutter/lib/src/semantics/semantics.dart>):
  two configurations conflict when they set the same flag or handle the same action, so
  `isButton` over `isImage` is one node; WAI-ARIA 1.2 §5.2.8 "Presentational children"
  (<https://www.w3.org/TR/wai-aria-1.2/#childrenArePresentational>) and the accessible name
  computation's "name from content" (<https://www.w3.org/TR/accname-1.2/#step2F>).

## Context and problem statement

A semantics node has one role (ADR-0206). A configuration that forms no node of its own is
absorbed by the node above it, unless the two conflict; a conflict gives each its own node.
ADR-0206 decision 4 counted every pair of declared roles as a conflict.

That is right for two controls, where neither may be lost: a button over a radio is a button
and a radio. It is wrong for a control over content. A button whose child is an image with a
label published a button with no name and an image node beneath it: a screen reader announced
"button", then "Add, image" as a second stop, and a test or an agent that asks for the button
named "Add" found none. Each catalog control that holds an icon met this, and the workaround
was to stop the content from saying what it is (an icon without the image role) or to wrap the
control in `MergeSemantics`.

Flutter does not have the defect: its roles are flags, a button and an image are different
flags, and the two merge into one node announced "Add, button".

## Decision drivers

- A control an author writes as `Button(child: Icon(label))` is one named stop for assistive
  technology, as in Flutter, without the author knowing about merging.
- One role per node stays: AccessKit, UI Automation and ARIA publish one.
- Two controls are still two nodes; nothing that can be acted on is lost in a merge.
- The rule is in the semantics model, once, not in each widget.

## Considered options

1. A control named by its contents takes content roles beneath it; the node keeps the
   control's role.
2. Keep ADR-0206's rule; content widgets inside controls do not declare a role.
3. Every control wraps its child in a forced merge (`MergeSemantics`).
4. Any two roles merge, the more specific one winning, as under a forced merge.

## Decision

Chosen option: 1, because it is the one that gives the author Flutter's result from the widget
tree Flutter's author writes, keeps one role per node, and leaves two controls as two nodes.

### §1. The two kinds of role

A **control named by its contents** is one of `Button`, `Link`, `CheckBox`, `RadioButton`,
`Switch`, `MenuItem`, `MenuItemCheckbox`, `MenuItemRadio`, `Tab`, `ListBoxOption`, `TreeItem`:
the roles whose accessible name ARIA computes from content.

A **naming role** is one of `Image`, `Heading`, `Paragraph`, `Caption`, `Code`, `Emphasis`,
`Strong`, `Insertion`, `Deletion`, `Term`, `Definition`, `Time`, `Math`: the roles that say
what a piece of content is and carry nothing but its name. They are the content rank of
ADR-0206's role precedence without its two read-only values: a `Meter` or a `ProgressBar`
carries a numeric range, which a control's node cannot publish (the applicability table drops
a range from a button), so it stays a node of its own beneath the control.

### §2. The scope of a control

The rule is decided by the control whose node the content ends in, not by the configuration
directly above the content: a hint or a tooltip between a button and its icon forms
no node and must not undo the rule. The assembly carries one fact down the tree:

```rust
impl SemanticsConfiguration {
    /// Whether what is absorbed beneath this configuration names a control.
    pub fn names_a_control(&self, forms_node: bool, enclosing: bool) -> bool;
    /// Whether this configuration can take `descendant`, which forms no node of its own.
    pub fn can_absorb(&self, descendant: &Self, names_a_control: bool) -> bool;
    /// Whether this configuration and `sibling`, absorbed by one above them, can share its node.
    pub fn can_join(&self, sibling: &Self, names_a_control: bool) -> bool;
    /// Whether this configuration, absorbed, can stay beside `node`, a sibling that was refused.
    pub fn can_stay_beside(&self, node: &Self, names_a_control: bool) -> bool;
}
```

- A control named by its contents starts the scope.
- A configuration that forms no node and has no role, or a naming role, passes on the scope
  it is in (`enclosing`): what it absorbs travels up to the same control.
- Every other configuration ends it: a node of another kind, a role that is neither, a route
  scope whatever its role.

`SemanticsAssemblyContext` in `flui-rendering` holds the answer of the configuration above;
`mark_configuration_conflicts` hands a configuration's own answer to the predicates.

### §3. The role row inside a scope

Outside a scope the predicates apply the conflict rows of ADR-0206 decision 4 unchanged.
Inside one, the role row reads:

- `can_absorb`: a descendant with a naming role is taken, whatever role the configuration
  taking it has (a button over an image; a heading over an image inside a button).
- `can_join`: two naming roles do not compete. An icon and a heading in one button are one
  name.
- `can_stay_beside`: a fragment its parent refuses is a node already, so a naming role does
  not compete with its role: a check box directly beneath a button, beside an icon, does not
  keep the icon from naming the button. Its other rows are compared as between any siblings.

Every other row is unchanged: a state both declare, a repeated action, competing values or
ranges still form separate nodes, and a configuration without a role still takes a control
beneath it, so a tap target over a check box inside a button is still one pressable check box.
A configuration that scopes a route never has a naming role, whatever role it declares, and
ends the scope.

The rule is not symmetric: an image over a button is an image with a button in it. The
symmetric predicate (`is_compatible_with`) is removed.

### §4. What the node is

The absorbed configuration merges by the table of ADR-0206 decision 4. The role is settled by
its precedence, in which a control outranks content, so the node keeps the control's role. The
content's label and hint concatenate onto the control's in traversal order, which is what
names the control. An identifier never crosses into another node (ADR-0240), so the
identifier of absorbed content is not published; content a test must find by identifier forms
its own node with `Semantics::container(true)`.

## Consequences

- Good, because a button, link, tab, menu item or checkable control over labelled content is
  one node with a name, on every platform and in the test and agent locators.
- Good, because content widgets say what they are everywhere: an icon can be an image in a
  paragraph and a name in a button without a parameter that chooses.
- Bad, because an image inside a button is no longer reachable as an image: a screen reader
  user cannot stop on it. This is Flutter's behavior, and ARIA's for the roles whose children
  are presentational (button, checkbox, radio, switch, tab, option, the checkable menu items).
- Bad, because a heading inside a tab or a list option loses its place in heading navigation.
  An author who needs both wraps the heading in `Semantics::container(true)`, which forms a
  node no rule absorbs.
- Bad, because a configuration without a role absorbs a check box instead of refusing it, so
  beneath it an icon
  beside the check box still competes with it on role and both form nodes: a hint over the two
  inside a button leaves the button without a name, as before this decision.
- Bad, because a configuration between a control and its content that repeats the control's
  action (a tap target inside a button) still cannot join the control: it forms a node, takes
  the content with it, and the control stays without a name, as before this decision. The
  scope is decided on the way down, before conflicts are known, so such a
  configuration that absorbs an image and a heading forms a node with the first one's
  role and both names. For the same reason a configuration that took content it would not
  have taken outside the scope can be refused further up where it was absorbed before: content
  that repeats the control's action carries the wrapper's value into its own node.
- Neutral, because Flutter treats two images beneath one button as a conflict (the same flag
  twice) and forms two nodes; here they are one name. A typed role has no "same flag" case to
  preserve, and two icons in one button are one button.
- Neutral, because a structural role (list, dialog, group, table cell) still keeps content as
  nodes beneath it: its name does not come from its contents.
- Neutral, because a progress bar or an activity indicator inside a button is still a node
  beneath it, as before this decision.

## Confirmation

- `a_control_named_by_its_contents_takes_content_roles`
  (`crates/flui-semantics/src/configuration.rs`): every pair of `SemanticsRole::ALL` through
  both predicates, inside a scope and outside, and every role through `names_a_control`,
  against two lists written out in the test; the merged role and name; a route scope. A role
  moved between kinds in the code without the test fails; a role added to `SemanticsRole` is
  in neither kind until its author lists it.
- `a_control_takes_the_content_roles_beneath_it_as_its_name` and
  `a_button_over_a_labelled_image_is_pressed_by_its_name`
  (`crates/flui-widgets/tests/semantics.rs`): the published AccessKit tree of a button over an
  image; over an image and a heading; over a hint over both; over a heading over an image;
  over an image beside a check box; over a progress bar; over a tap target over a check box;
  of a list over an image; and the
  click on the named node.
- `compatibility_rules_for_modeled_fields` keeps the rows that did not change.

## Pros and cons of the options

### 1. A control named by its contents takes content roles

- Good, because it is Flutter's result and ARIA's rule, stated once in the model.
- Good, because the role precedence already ranks content lowest; the rule adds no second
  ordering.
- Bad, because node formation is no longer one symmetric predicate: the node's side matters.

### 2. Content inside a control declares no role

- Good, because the model does not change.
- Bad, because each content widget needs to know where it stands, or each control needs to
  strip its child: the rule is repeated per widget and forgotten in the next one.
- Bad, because an author's own `Semantics::role(Image)` inside a button still splits.

### 3. Every control forces a merge

- Good, because it uses a mechanism that exists.
- Bad, because a forced merge also swallows controls: a list tile with a switch in it becomes
  one node and the switch is lost, which is the case decision 4 of ADR-0206 protects.
- Bad, because it is an element and a render object per control (ADR-0322 removed them).

### 4. Any two roles merge by precedence

- Good, because it is one rule with no lists.
- Bad, because a button over a radio publishes a radio and loses the button's action name, or
  merges two handlers of one action; two controls are two things to act on.

## More information

- A role added to `SemanticsRole` is in neither kind of §1 until its author lists it; the
  vocabulary change decides its kind.
- `Slider`, `SpinButton`, the text inputs and `ComboBox` are named by a label beside them, not
  by their contents, and stay out of §1.
