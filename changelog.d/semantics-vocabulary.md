### Added

- **`ActionCall`** (`flui-semantics`, ADR-0203): a semantics action fused with exactly its
  payload, which every `SemanticsActionHandler` now receives. `Reveal` and `TextSelection`
  carry the reveal geometry and a selection.
- **`ActionDecodeError`** (`flui-semantics`): why a platform request did not decode — an id
  FLUI never publishes, an unrouted action, or a missing or mismatched payload.
- **`ReplaceSelectedText`, `ShowTooltip` and `HideTooltip` actions** (`flui-protocol`), routed
  from and to AccessKit.
- **`ScrollUnit`** (`flui-semantics`): a directional scroll carries the unit a platform asked
  for (`Item` or `Page`); `Scrollable` moves one wheel line for an item and most of a viewport
  for a page, and `Semantics::on_scroll_{up,down,left,right}` hand it to their handler.
- **`Semantics::on_set_scroll_offset`, `on_scroll_into_view`** (`flui-widgets`), with typed
  payloads.
- **The ARIA role vocabulary** (`flui-protocol`, ADR-0203): `SemanticsRole` gains every
  non-abstract WAI-ARIA 1.2 role with an AccessKit counterpart (`Paragraph`, `Group`, `Grid`,
  `Tree`, `ListBox`, `Toolbar`, `Banner`, `Log`, `Meter`, …), the text-field kinds AccessKit
  distinguishes (`SearchInput`, `EmailInput`, `NumberInput`, `PhoneNumberInput`, `UrlInput`,
  `DateInput`, `TimeInput`) and the desktop roles `Window` and `Pane`. Each publishes an
  AccessKit role of its own.
- **The `heading` wire role** (`flui-protocol`, agent protocol 0.3): an agent reads a heading as
  `heading` from both backends; the desktop server restores it from UI Automation's ARIA role.

### Changed

- **`SemanticsAction` follows AccessKit** (`flui-protocol`, ADR-0203): `Tap` is `Click`,
  `LongPress` is `ShowContextMenu`, `Increase`/`Decrease` are `Increment`/`Decrement`,
  `ShowOnScreen` is `ScrollIntoView`, `SetSelection` is `SetTextSelection`, `SetText` is
  `SetValue`, `ScrollToOffset` is `SetScrollOffset`, `DidLoseAccessibilityFocus` is `Blur`.
  Bits are dense from zero.
- **`SemanticsActionHandler`** is `Fn(ActionCall)`, and `SemanticsActionRequest` holds a
  `call` instead of `action` and `arguments`.
- **`semantics_action_request_for`** returns `Result<_, ActionDecodeError>`: a request whose
  payload does not fit its action is refused before any handler runs.
- **`Semantics` builders** (`flui-widgets`) are named after their actions: `on_click`,
  `on_show_context_menu`, `on_increment`, `on_decrement`, `on_set_value`; `on_action` takes
  `Fn(&mut EventCx, ActionCall)`.
- **`flui_testing::a11y::InvokeActionError`**: `Decode` replaces `UnroutablePlatformAction`
  and `MalformedNodeIdentity`.
- A node with blocked user actions keeps only `Blur`.
- **`SemanticsRole` values are dense in declaration order**, which groups the roles by kind;
  `TabBar` is `TabList`, ARIA's name.

### Removed

- **Actions with no platform path** (`flui-protocol`): the four cursor moves, `Copy`, `Cut`,
  `Paste`, `Dismiss`, `DidGainAccessibilityFocus`, and `SemanticsAction::RESERVED_BITS`.
- **Roles with no AccessKit counterpart** (`flui-protocol`): `DragHandle` and `HotKey`, which
  published as a plain container, and `LoadingSpinner`; an indeterminate progress indicator is
  a `ProgressBar` without a value.
- **`ActionArgs` and `semantics_action_args_for`** (`flui-semantics`); `ActionCall` carries the
  payload. `Semantics::on_show_on_screen`, `on_set_text` and `on_scroll_to_offset` are
  `on_scroll_into_view`, `on_set_value` and `on_set_scroll_offset`.
