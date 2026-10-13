### Removed

- **`Semantics` boolean role builders** (`flui-widgets`): `button`, `link`, `slider`, `header`,
  `image`, `text_field`, `obscured` and `multiline`. Declare the role with
  `.role(SemanticsRole::…)` (`Button`, `Link`, `Slider`, `Image`, `TextInput`, `PasswordInput`,
  `MultilineTextInput`), or `.heading_level(level)` for a heading.
- **`SemanticsConfiguration` boolean role setters and getters** (`flui-semantics`):
  `set_button`/`is_button` and the `link`, `slider`, `image`, `header`, `text_field`, `obscured`
  and `multiline` pairs. Use `set_role` and `role()`.
- **`SemanticsAttributes`** and `SemanticsConfiguration::attributes` are no longer public: a node
  is read and written through `SemanticsConfiguration`'s accessors.

### Changed

- **A forced merge keeps the receiver's yes/no states** (`flui-semantics`): when both parts of a
  `MergeSemantics` node declare `enabled`, `focused`, `selected` or another yes/no state, the
  merged node takes the receiver's value instead of `true`, so a disabled tile is not enabled
  by a part that declares itself enabled.
- **`ActionDecodeError` names no AccessKit type** (`flui-semantics`): `UnknownTarget` carries the
  raw `u64` id and `Unrouted` a new `UnroutedAction` (`ScrollToPoint`,
  `SetSequentialFocusNavigationStartingPoint`).
