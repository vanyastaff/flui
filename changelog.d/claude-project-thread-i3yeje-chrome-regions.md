### Added

- **Window chrome regions** (`flui-platform`): `InputRegionKind` gains `WindowMove`,
  `WindowResize(ResizeEdge)`, `CaptionButton(CaptionButton)` and `WindowContent`, the regions an
  application-drawn title bar marks. `chrome_hit` answers what a press at a device point does
  (`ChromeHit::{Content, Move, Resize, CaptionButton}`) from a window's regions in paint order:
  the last one painted over the point decides, and `WindowContent` cut out of a move area gives
  the press back to the application (ADR-0286). Only the headless host honours the chrome kinds
  so far.

### Changed

- **Input region availability per kind** (`flui-platform`, `flui-native`, `flui-testing`):
  `InputCapabilities::input_region(kind)` and `honours_input_regions()` replace
  `input_regions()`, and the builder's `system_gesture_exclusion`, `window_move`,
  `window_resize` and `caption_buttons` replace `input_regions`. `set_input_regions` is
  `Unsupported` exactly when no kind is honoured, and otherwise drops the regions of the kinds
  the window does not honour. `HeadlessWindowSystem::input_capabilities` replaces
  `input_regions`, and `WindowFixture::input_capabilities` replaces `input_regions`.
- **Recording format 3** (`flui-testing`): a recorded backend profile carries the four input
  region availabilities in place of `input_regions`; a format 2 recording is refused.
