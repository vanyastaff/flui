### Changed

- **`flui-driver`**: the Android lane reads the window's place, the focus and the display's
  rotation from one `dumpsys window`, which the window manager dumps under its one lock, so they
  are of one moment. Before each press, key, text and scroll it reads them again: another window
  of the app's title answers `Error::NotForeground`, and the window it located moved, resized or
  turned answers `Error::Busy` until the lane locates it again, where the press went out at the
  old place.

### Removed

- **`flui-driver`**: `lane::android::device::FOCUS` and `parse_focus`: the lane reads the focus
  with the windows (`device::WINDOWS`), as `Windows::focus`, the window's `WindowName` (its
  `WindowHash` and its title).
