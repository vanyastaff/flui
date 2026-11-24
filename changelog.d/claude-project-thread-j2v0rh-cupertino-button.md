### Added

- **`CupertinoButton` takes keyboard focus** (`flui-cupertino`): an enabled button is reached
  with Tab, draws the focus ring while that focus is shown, and Enter, Space or Select presses
  it. A disabled button takes no focus.

### Fixed

- **`CupertinoButton`'s fade follows the press** (`flui-cupertino`): the button fades out when the
  press goes down, stays faded while it is held, and fades back in when it lifts or is cancelled,
  including when the pointer moves past the tap slop. It used to flash only after the tap was
  recognized.
- **`CupertinoButton` keeps its colour's alpha** (`flui-cupertino`): a plain or filled button
  paints a dynamic colour with the alpha of the variant the brightness resolves, so
  `CupertinoColors::SEPARATOR` under a dark theme paints at alpha 153 instead of the light
  variant's 73.
