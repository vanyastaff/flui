### Changed

- **`flui-interaction`, `flui-testing`**: the scripted clipboard and primary selection live beside
  the contracts they double, in `flui-interaction`'s `testing` kit
  ([ADR-0264](/docs/adr/ADR-0264-platform-test-doubles-live-with-their-contract.md)):
  `flui_testing::clipboard::{ScriptedClipboard, ClipboardRequest, WriteRequest,
  ClipboardWriteHold}` are `flui_interaction::testing::{ScriptedClipboard, ClipboardRequest,
  ClipboardWriteRequest, ClipboardWriteHold}`, and
  `flui_testing::primary_selection::{ScriptedPrimarySelection, PrimarySelectionRequest}` are
  `flui_interaction::testing::{ScriptedPrimarySelection, PrimarySelectionRequest}`. The
  `flui_testing::clipboard` and `flui_testing::primary_selection` modules are gone;
  `flui_testing::host` re-exports all six beside `HeadlessHost::clipboard` and
  `HeadlessHost::primary_selection`. `flui-interaction`'s `testing` feature now turns on
  `flui-platform`'s `scripted`.
