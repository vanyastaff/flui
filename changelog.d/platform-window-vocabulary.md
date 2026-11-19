### Changed

- A window's snapshot states its theme as `WindowState::brightness: Brightness`; `WindowAppearance`, whose vibrant values no host produced, is removed (what the window system draws behind the content is its `WindowMaterial`).
- A window's focus and activation are one `WindowState::focus: WindowFocus` (`Inactive`, `Active`, `Focused`) in place of the `focused` and `active` booleans, so a window holding the keyboard is always the active one, and a hidden window is published `Inactive`. A host whose window system reports the two apart reads them through `WindowFocus::reported`, where the focus wins.
- A cursor update or preference query the window system refuses is `CursorError::Platform(PlatformFailure)` or `PreferenceQueryError::Platform(PlatformFailure)`, the shape `ApplyError::Platform` already has, in place of `CursorError::Backend(String)` and `PreferenceQueryError::Native { message }`.
- `PlatformWindow::gesture_geometry`, `window_at` and `reveal_after_first_frame` have no default body: every window implements them.
- The headless window's `simulate_appearance` is `simulate_brightness`.

### Removed

- `flui_platform::testing` (`NoNativeSurface` and the contract doubles) exists only with the `testing` feature.
