### Added

- `flui-platform`: `WindowExtent`, a window, content or limit extent whose sides are finite and
  not negative, and `Eq`; `InvalidGeometry`, why numbers describe no window, carrying them (an
  extent or position that is not finite, a size or limit that encloses no area, a minimum above
  the maximum), equal to itself bit for bit, NaN included. `WindowGeometry::try_new`, `unplaced`,
  `at`, `extent` and `EMPTY`; `SizeConstraints::try_new`.
- `flui-platform`: `WindowCapability { Native, Emulated, Absent(CapabilityAbsence) }`, how a
  window has one capability or why it does not, with `is_available` and `NOT_ON_THIS_PLATFORM`;
  `WindowCapabilities::capability(&command)`; `CapabilityParts` is `Hash`.
- `flui-platform`: `WindowOptions::geometry`, the numbers a window opens at
  (`OpeningGeometry`), each checked as the command that sets it is, and `InvalidWindowOption`
  with `WindowOption`, which names the option refused and its numbers.
- `flui-native`: `OpenWindowError::InvalidOption`, an open refused for options a command would
  be refused for.
- `flui-view`: `WindowHandle::is_open`, whether the window is still open, asked without reading
  its state.
- `flui-testing`: the window conformance checks hold a host to
  `a_report_that_describes_no_window_publishes_nothing` (through the new
  `WindowFixture::report_no_window`): such a report publishes nothing of its geometry, and a title
  the window takes while it stands is published; and, in `a_closed_window_refuses`, to refusing
  the input regions of a closed window.

### Changed

- `flui-platform`: a `WindowState` cannot hold geometry that is not a window's. `WindowGeometry`
  has private fields, is built with `try_new` and read with `position()` and `size()`;
  `WindowState::content_size` and the bounds of `SizeConstraints` are `WindowExtent`s;
  `WindowState::new` takes a `WindowExtent`. `WindowGeometry::new` is gone.
- An invalid window size, size limit or position in the window options now refuses the open; it
  was ignored with a warning (a minimum above the maximum reached the window system). Every host
  refuses, before it takes an id, the numbers the command setting them would refuse (a size or
  position that is not finite, a size or limit that encloses no area, a minimum above the
  maximum) with `OpenWindowError::InvalidOption`, whose message names the option and its numbers;
  an application whose `AppConfig` asks for them does not start (`LaunchError::InitialWindow`).
  `WindowOptions::min_size` and `max_size` are `Option<WindowExtent>`, and so are `AppConfig`'s;
  `WindowOptions::constraints` and `content_extent` are replaced by `WindowOptions::geometry`.
- `flui-platform`: `AdmissionError::InvalidGeometry` carries the `InvalidGeometry` that refused
  the command.
- `flui-platform`: the fields of `WindowCapabilities` and of `CapabilityParts` that a command is
  admitted against, and the three reports, are `WindowCapability` values in place of an
  `Availability` with a reason kept apart (`CapabilityParts` held a
  `Result<Availability, CapabilityAbsence>`). The four observations
  (`observes_position`, `observes_minimized`, `places_at_open`, `frame_rate_hint`) stay
  `Availability`. The native recording codec refuses a window-state row whose geometry or limits
  describe no window.
- `flui-native`: a read of a window's state (`PlatformWindow::state`) no longer calls the
  window's handler. A revision the read publishes is announced with `state_changed` at the
  owner's next turn; a change the window system reports is still announced from the host's own
  callback. The web window now asks the page for that turn.
- `flui-native`: a window-system report whose geometry describes no window keeps the published
  geometry and still publishes the report's other facts (focus, visibility, mode, occlusion,
  execution state, a close), where it used to be dropped whole.

### Removed

- `flui-platform`: `WindowCapabilities::availability`, `refusal`, `presentation_feedback`,
  `keyboard_inset`, `system_gestures`, `without_presentation_feedback`, `without_keyboard_inset`,
  `without_system_gestures` and `without_app_title_bar`. Read or set the field, and match the
  `WindowCapability` it holds.

### Fixed

- Win32: a `WM_DPICHANGED` naming a DPI of zero published a window state with an infinite content
  size and outer rectangle, which then differed from itself on every read. The window keeps its
  scale, and no host publishes a report whose numbers describe no window.
- The headless window: reading its state discarded a snapshot its simulated window system had
  reported (`simulate_state`), so a replayed `window_state` recording never reached a reader.
- Win32, X11, Wayland, AppKit and Android: `set_input_regions` on a closed or retired window
  answered `Ok` (Android posted the regions to the view its successor owns); it answers
  `Unsupported` with `NoWindow`, as the UIKit and headless windows do.
- `flui-app`: looking a window up by its presentation read the window's state under the borrow of
  the window queues, where a handler run from that read could not register another window.
- `flui-app`: a restored placement whose frame is measured as no extent keeps the configured
  bounds a maximized window returns to, and says so, where they were dropped silently.
