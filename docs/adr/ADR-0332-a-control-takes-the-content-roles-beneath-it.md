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
the roles whose accessible name ARIA computes from content and whose children it treats as
presentational.

A **content role** is one of `Image`, `Heading`, `Paragraph`, `Caption`, `Code`, `Emphasis`,
`Strong`, `Insertion`, `Deletion`, `Term`, `Definition`, `Time`, `Math`, `Meter`,
`ProgressBar`: the roles that say what a piece of content is and offer no action. They are the
lowest rank of ADR-0206's role precedence, and the precedence reads the same list.

### §2. Absorbing

```rust
impl SemanticsConfiguration {
    /// Whether the node `self` forms can take `descendant`, which forms none of its own.
    pub fn can_absorb(&self, descendant: &SemanticsConfiguration) -> bool;
    /// Whether `first` and `second`, both beneath the node `self` forms, can both be in it.
    pub fn can_absorb_both(&self, first: &Self, second: &Self) -> bool;
}
```

Both apply the conflict rows of ADR-0206 decision 4 with one change to the role row:

- `can_absorb`: two declared roles conflict, unless `self` is a control named by its contents
  and `descendant` has a content role.
- `can_absorb_both`: two declared roles conflict, unless `self` is a control named by its
  contents and both have content roles (an icon and a heading in one button are one name).

Every other row is unchanged: a state both declare, a repeated action, competing values or
ranges still form separate nodes, so a button over a progress bar that carries a value absorbs
it only when the button has no value of its own. A configuration that scopes a route is never
content, whatever its role.

The rule is not symmetric: an image over a button is an image with a button in it. The
symmetric predicate (`is_compatible_with`) is removed; node formation asks the two questions
above, from the node's side.

### §3. What the node is

The absorbed configuration merges by the table of ADR-0206 decision 4. The role is settled by
its precedence, in which a control outranks content, so the node keeps the control's role. The
content's label and hint concatenate onto the control's in traversal order, which is what
names the control.

## Consequences

- Good, because a button, link, tab, menu item or checkable control over labelled content is
  one node with a name, on every platform and in the test and agent locators.
- Good, because content widgets say what they are everywhere: an icon can be an image in a
  paragraph and a name in a button without a parameter that chooses.
- Bad, because an image inside a button is no longer reachable as an image: a screen reader
  user cannot stop on it. This is ARIA's presentational-children rule and Flutter's behavior.
- Bad, because a heading inside a tab or a list option loses its place in heading navigation.
  An author who needs both wraps the heading in `Semantics::container(true)`, which forms a
  node no rule absorbs.
- Neutral, because Flutter treats two images beneath one button as a conflict (the same flag
  twice) and forms two nodes; here they are one name. A typed role has no "same flag" case to
  preserve, and two icons in one button are one button.
- Neutral, because a structural role (list, dialog, group, table cell) still keeps content as
  nodes beneath it: its name does not come from its contents.

## Confirmation

- `a_control_named_by_its_contents_takes_content_roles`
  (`crates/flui-semantics/src/configuration.rs`): every pair of `SemanticsRole::ALL` against two
  lists written out in the test, so a role added to either kind in the code without the test
  fails; the merged role and name; siblings; a route scope.
- `a_control_takes_the_content_roles_beneath_it_as_its_name` and
  `a_button_over_a_labelled_image_is_pressed_by_its_name`
  (`crates/flui-widgets/tests/semantics.rs`): the published AccessKit tree of a button over an
  image, over an image and a heading, of a list over an image, and the click on the named node.
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

- Revisit §1 when a role is added to `SemanticsRole`: the test's lists make the author decide
  its kind.
- `Slider`, `SpinButton`, the text inputs and `ComboBox` are named by a label beside them, not
  by their contents, and stay out of §1.
