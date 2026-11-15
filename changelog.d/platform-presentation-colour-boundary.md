### Changed

- **`flui-platform`**: `DisplayColor` is an enum, `Standard(ColorGamut)` or
  `High(ColorGamut, Result<Headroom, CapabilityAbsence>)`, so a standard-range display has no
  headroom to contradict its range: `DisplayColor::standard(gamut)` is `DisplayColor::Standard(gamut)`,
  `DisplayColor::high(..)` is `DisplayColor::High(..)`, and `color_space()` and `headroom()` read it
  (`Headroom::SDR` for a standard range).
  `DisplayParts::color` holds the `DisplayColor` itself; `ColorParts` and
  `DisplayColor::into_parts` are removed.
- **`flui-scheduler`**: `FrameClock::next_presentation_after` names the refresh after the newest
  report where that report lies after the reading, never the report itself: a report is of a frame
  already shown.

### Fixed

- **`flui-native`**: one presentation report stamped far ahead of the event clock (a rebase that
  saturated, a stamp left on the compositor's own epoch) no longer keeps every later report of the
  window from its frame requests. A report more than a second ahead of the clock when it arrives is
  dropped, on every host, and one ahead by less is held at the clock's reading, so a window whose
  every stamp leads the clock (a compositor that adds its display's latency) still reports on each
  frame request.
- **`flui-native`** (macOS, iOS): an `NSURLSession` request whose delegate panics while it handles
  the task's response, data or end now fails with `HttpError::Host` instead of waiting until its
  deadline, and a body that lost bytes that way ends in that error instead of reading as complete.
- **`flui-native`** (macOS, iOS): the Rust state of the Objective-C classes the host defines is
  destroyed inside the panic boundary, so a destructor that panics while UIKit or AppKit releases
  the object is logged instead of unwinding through the system's frames.
- **`flui-native`**: a native recording whose display row gives a standard-range display a headroom
  other than SDR white, or none, is refused where the row is read.
- **`flui-engine`**: a surface that reports a headroom that is not a finite number, or a peak or SDR
  white luminance that is not finite and positive, is treated as SDR; it was read as a display that
  reports nothing, and so as HDR-capable.
- **`flui-runtime`**: a frame's idle work ends at a refresh still ahead when the frame starts. A
  frame whose presentation report lay between its vsync and its start, or one the owner reached a
  refresh late, got a deadline already past and ran no idle work.
