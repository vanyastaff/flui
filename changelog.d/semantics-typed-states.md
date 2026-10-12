### Added

- **Typed semantics states** (`flui-semantics`, ADR-0202): `States` and `Toggled` (`Off`, `On`,
  `Mixed`), read through `SemanticsConfiguration::states` and set with
  `SemanticsConfiguration::set_toggled`.
- **Interactive semantics roles** (`flui-protocol`, ADR-0203): `Button`, `CheckBox`,
  `RadioButton`, `Switch`, `Link`, `Slider`, `Image`, `Heading`, `TextInput`,
  `MultilineTextInput` and `PasswordInput`. `SemanticsRole::is_text_input` names the last three.
- **`Semantics::checkbox`, `radio`, `switch` and `toggled`** (`flui-widgets`): each sets the
  control's role and its state together.

### Changed

- **One role per node** (`flui-semantics`): `set_button`, `set_link`, `set_slider`,
  `set_image`, `set_header`, `set_text_field`, `set_obscured` and `set_multiline` set the node's
  role. Two configurations that each declare a role, or each declare the same state (a declared
  `false` included), now form separate nodes. `MergeSemantics` still forces them into one: the
  more specific role wins (structural, then checkable, then button, then link, text field or
  slider, then image or heading), and `Toggled` merges `Mixed` over `On` over `Off`.
- **A nested merger keeps its own node** (`flui-rendering`): a node that merges its descendants
  is no longer absorbed by an enclosing `MergeSemantics`.

### Removed

- **Semantics flags** (`flui-semantics`, `flui-widgets`): `SemanticsFlag`, `SemanticsFlags`,
  `SemanticsConfiguration::flags`, `has_flag`, `set_checked`/`is_checked`,
  `set_mixed`/`is_mixed`, `set_in_mutually_exclusive_group`/`is_in_mutually_exclusive_group`,
  `is_toggled`, and `Semantics::checked`, `mixed` and `in_mutually_exclusive_group`. Use the
  role setters, `set_toggled(Option<Toggled>)` and `Semantics::checkbox`/`radio`/`switch`;
  `Semantics::toggled` now takes a `Toggled`.
