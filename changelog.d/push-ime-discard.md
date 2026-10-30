### Added

- `PlatformTextInput::discard_composition`, a required method: the platform forgets its in-progress composition while the store commits its own copy in place.

### Fixed

- Completing a composition on a push platform (winit, iOS) no longer leaves the input method holding the preedit, which it committed again on the next commit, doubling the text.
