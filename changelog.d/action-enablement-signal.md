### Added
- `Action::enablement_changes` returns the `ChangeNotifier` an action's enablement changes through, with no rebuild; `None`, the default, means it never changes after the action is bound. `PageView`, `AnchoredDrag` and the tab bar's step action provide one.
- `Semantics::custom_action_when` offers a custom action only while a gate holds, and `Semantics::refresh_on` describes the node again, without a rebuild, when a `ChangeNotifier` fires. `CustomActionId::at` and `SemanticsConfiguration::add_custom_action_as` let a widget keep an action's id while the others around it come and go.

### Changed
- A `GestureAlternatives` command is offered to assistive technology as a custom action only while the action bound to its intent is enabled, and a request for one that has since closed is refused at delivery as well as by the semantics owner. A command with no action bound is still offered, with its warning.
