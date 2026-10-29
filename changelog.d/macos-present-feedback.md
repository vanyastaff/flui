### Added

- **`flui-native`**: an AppKit window reports when its frames reached the screen
  (`FrameRequest::presented`, `WindowCapabilities::reports_presentation` is `Native`): its view is
  backed by its own `CAMetalLayer`, whose drawables carry a presented handler.

### Changed

- **`flui-native`**: the AppKit view draws through `updateLayer` instead of `drawRect:`; its
  backing layer is the `CAMetalLayer` wgpu presents into, with no sublayer between them.
