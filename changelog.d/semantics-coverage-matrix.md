### Fixed

- **A list item's position reaches screen readers as published** (`flui-semantics`): AccessKit's
  `position_in_set` is zero-based and the adapters add one, so the framework index is now
  published as is; it was published one-based, and UI Automation reported the third item as the
  fourth.
