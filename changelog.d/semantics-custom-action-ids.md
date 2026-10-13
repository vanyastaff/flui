### Added

- **`Semantics::custom_action(label, handler)`** (`flui-widgets`): offers an operation assistive
  technology lists by its label, such as a swipe action or "Move up"; each call adds one action
  with its own handler.
- **`CustomActionId`** and **`DeclaredCustomAction`** (`flui-semantics`): a custom action's id is
  its place in the node's list, and its handler receives the id its own configuration gave it.

### Changed

- **`SemanticsConfiguration::add_custom_action(label, handler)`** takes the action's handler and
  returns its id; `ActionCall::CustomAction` carries a `CustomActionId`. A node's custom actions
  each run their own handler, two sets of them share a node, and a request whose id no longer
  names the action last published there (by handler, own id and label, not label alone) is
  refused. `remove_action(CustomAction)` removes every custom action, and within one `Semantics`
  a repeated label replaces the earlier action.

### Removed

- **`CustomSemanticsAction`**: a custom action is a label and a handler. Its unpublished hint is
  gone, and a bare `CustomAction` handler registered with `add_action` names nothing to run.
