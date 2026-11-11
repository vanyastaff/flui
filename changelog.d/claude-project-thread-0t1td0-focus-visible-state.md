### Added

- `WidgetState::FocusVisible`: the widget holds focus and the focus is shown (moved or last used
  by a key, traversal or assistive technology).
- `RawButton::states_controller`: the button writes `Pressed`, `Focused`, `FocusVisible` and
  `Disabled` to the given `WidgetStatesController` and clears them when it unmounts or is given
  another one.

### Changed

- `WidgetState::Focused` means "holds focus, by any cause". Material's `InkWell` sets it on a
  pointer press too and sets `FocusVisible` for the focus a user can see; state layers, halos and
  the stock overlay ramps resolve on `FocusVisible`. A `WidgetStateProperty` resolver that shows
  focus reads `FocusVisible`.
- `WidgetStates` is backed by `u16`.
