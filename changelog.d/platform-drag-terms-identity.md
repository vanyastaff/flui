### Added

- **`flui-platform`**: `TransferFormat::essence` and `Mime::essence` name the media type a format is
  offered under, without parameters (`text/plain` for `Text`, the MIME's `type/subtype` for an
  image or a custom format).

### Changed

- **`flui-platform`**: `OsDragTerms::try_new` refuses two serialized representations under one
  media type (`DragRequestError::DuplicateFormat`), and refuses the identity's media type whichever
  variant carries it: `TransferFormat::is_drag_identity` now holds for an `Image` of that type too.
- **`flui-interaction`**: a drag whose items share a media type (two items with text) offers the
  OS the first representation of each type, and serves each from the item it came from; before,
  every host dropped the repeats its own way (first kept on Windows, X11 and Wayland, last on the
  web and Android).

### Fixed

- **Windows**: a drag offering an image under the identity's media type no longer takes the
  identity's place in the data object, where a target read image bytes as the session identity.
- **Headless**: the headless platform's drag source refuses a request begun by a press its window no
  longer holds, as a Win32 window does.
