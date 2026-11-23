### Fixed

- A `MergeSemantics` node runs each action on the part whose state the action changes: a click
  on a merged tile and radio, switch or tab runs the control the node announces, a disabled or
  blocked control is no longer clicked through a sibling's handler, and a scrollable at its end
  offers no further step from another part (ADR-0392).
- A merged node's `toggled`, `selected` and `enabled` come from the part that owns the click,
  and a forced merge whose click toggles or selects drops a click-only `expanded`.

### Removed

- `SemanticsAction::is_scroll_action`, `is_text_action` and `is_focus_action`, which no code
  read and which disagreed with the subjects ADR-0392 gives the actions.
