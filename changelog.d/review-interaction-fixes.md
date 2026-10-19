### Fixed

- **Back from a nested navigator's first route pops the enclosing navigator**
  (`flui-widgets`): the nearest route's `BackIntent` binding no longer refuses
  the key without consulting the navigators around it, and a pop no navigator
  handles is reported unperformed, so the key reaches the host instead of being
  consumed. `DismissIntent` on a modal that does not pop is unperformed too.
- **`Dismissible::dismiss_threshold` for an axis applies to both of its sides**
  (`flui-widgets`): a threshold set for `DismissDirection::Horizontal` or
  `Vertical` was stored and never read, so a row configured as hard to dismiss
  went out at the default `0.4`.
- **Assistive-technology activation of a `GestureDetector` runs in the request's
  dispatch** (`flui-widgets`): a screen reader's Invoke on a `RawButton`, a
  gesture alternative's control or a context-menu row performed one frame late,
  after a rebuild and a post-frame hop through a locked queue, and was dropped
  where the context had no post-frame lane. It now runs the current `on_tap` or
  `on_long_press` inside the request's `EventCx`, in the same dispatch as Enter.

### Changed

- **`Draggable::new` and `DropTarget::new` take their commands' appearance,
  not `GestureAlternatives`** (`flui-widgets`): `Draggable::new(DragCommands,
  items, child)` and `DropTarget::new(DropCommands, accept, builder)` return a
  `Result` and build the alternatives themselves, pairing each
  `CommandAppearance` (label, keys, control content) with the intent the
  widget answers (`CopySelectionTextIntent::Cut`/`Copy`, `PasteTextIntent`),
  reached by an `AlternativeKind` (visible controls or a presented context
  menu). Alternatives whose commands performed an unrelated intent, which the
  keyboard and assistive-technology paths then could not complete, no longer
  type-check. Both widgets gain `target(name)` and `alternatives()`.

- **A package can build a multi-command or swiped behavioural widget on
  `flui-sdk`** (`flui-widgets`): `GestureCommand::handle` and
  `GestureCommand::performs` are public, so a widget whose gestures perform
  several commands reaches each through the action chain, as `SwipeActions`
  does. `ContactAdmission` is a public type (`ContactAdmission::new`,
  `ContactAdmission::outside_system_edges`) taken by the now public
  `RawGestureDetector::admission`, `GestureDetector::admission` and
  `AnchoredDrag::admission`, so a package row leaves the window's
  system-gesture strip to the operating system as the built-in rows do.

- **`GestureDetector::on_multi_tap` takes a `MultiTapCount`** (`flui-widgets`):
  a count below two is refused by `MultiTapCount::try_from` (with
  `MultiTapCountError`) instead of panicking inside the parent's `build`.
- **`Dismissible::dismiss_threshold` takes a `DismissThreshold`**
  (`flui-widgets`): a fraction in `0.0..=1.0` built by `DismissThreshold::new`
  (refusing NaN and out-of-range values with `DismissThresholdError`), with
  `DEFAULT` (`0.4`) and `NEVER` (`1.0`). `DismissUpdateDetails` is
  `#[non_exhaustive]`.
