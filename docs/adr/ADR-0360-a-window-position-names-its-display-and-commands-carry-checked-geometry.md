# ADR-0360: A window position names its display, a display's bounds are its own logical pixels, and commands carry checked geometry

- **Status:** Accepted (2026-11-17)
- **Date:** 2026-11-17
- **Supersedes:** [ADR-0237](ADR-0237-window-state-and-host-notices.md) decision 2's global position
  and decision 6's display option only; [ADR-0236](ADR-0236-display-snapshot-and-refresh-period.md)
  decision 1's bounds only
- **Related:** [ADR-0254](ADR-0254-window-contract.md) (the snapshot's geometry is valid by its
  types; this record extends that to the position, the commands and the options, and says what a
  position is measured in),
  [ADR-0237](ADR-0237-window-state-and-host-notices.md) (decision 6, the placement record, which
  keeps its format and measures from a display's usable area),
  [ADR-0236](ADR-0236-display-snapshot-and-refresh-period.md) (the display snapshot and its ids,
  which a position now names),
  [ADR-0098](ADR-0098-owned-f64-geometry-values.md) (logical and device geometry do not mix),
  [ADR-0354](ADR-0354-a-window-shows-the-cursor-its-region-asks-for-holds-the-pointer-only-while-focused-and-says-when-the-user-resizes-it.md)
  (resize steps carry a `WindowExtent` and admission checks its area: the form this record
  extends)

## Context and problem statement

A window's place had no fixed meaning on a desktop of mixed scales.

| Where | What it said | What it did |
|---|---|---|
| `WindowState::outer`, `WindowCommand::SetPosition` | "global logical pixels" | Win32 divided the physical corner by the *window's* scale and multiplied a command by it, so a window on a 2x monitor right of a 1x one read x 960 at physical 1920, and one `SetPosition` landed on different monitors depending on which monitor the window was on |
| `WindowOptions::position`, `normal_geometry` | logical pixels of `WindowOptions::display`, from its usable area | one type, `WindowGeometry`, carried two spaces depending on where it was stored |
| `DisplayInfo::bounds` | device pixels, logical on Wayland | macOS published points times each screen's own scale, so two screens could overlap in that "device" space; placement read it as logical by dividing by each display's scale |
| `WindowCommand::SetSize`, `WindowOptions::size`, `SetPosition` | raw `Size<f64>`, `Point<f64>` | a second pass checked what the snapshot's types already hold, and a host received unchecked numbers from an admitted command |

winit answers the first problem by publishing physical positions only; SDL3 keeps screen
coordinates beside a per-display scale; GPUI publishes one global logical space; Raven converts
by the window's own scale, as FLUI did. The question is what a position, a display's bounds, a
size and a command are measured in, so that every host converts them one way and a value read
back is the value that was asked for.

## Decision drivers

- Logical and device numbers do not mix (ADR-0098); a unit is a type, not a sentence in a doc.
- A value the snapshot publishes, a command asks for and an opening opens at is one type with one
  check, refused at the seam with an error that names the numbers (ADR-0254 §1, §2).
- A position read back from the window converts back to the same pixel, on every host that has
  positions, whatever display the window is on when it is asked.
- Every intermediate step is checked: no saturating cast, no overflow, nothing non-finite
  published.
- Hosts that are not told where a window is (Wayland, phones, the web canvas) say so through
  their capabilities, as today.
- The placement record keeps its stored format: a placement written by the previous build reads.

## Considered options

1. A position is a point of one display, named by `DisplayId`, in that display's logical pixels
   from its top-left corner; a display publishes its bounds in its own logical pixels and its
   arrangement as an opaque desktop origin; commands and options carry `WindowPosition` and
   `WindowExtent`.
2. Physical positions only, as winit: a `DevicePoint` in the window system's desktop.
3. One global logical space for every display, as GPUI.
4. Keep `Point<f64>` and measure it from the window's current display, leaving the display out of
   the value.

## Decision

Chosen option: 1, because it is the only one whose value carries its own space: a host converts it
through the display it names, never through the window's state, and a consumer fits a window to a
display without knowing the platform's unit.

### §1 A position names its display

`WindowPosition { display: DisplayId, at: Point<f64> }`, built by `WindowPosition::try_new`,
which refuses a coordinate that is not finite with `InvalidGeometry::Position`. It is the outer
top-left corner of a window, in the logical pixels of `display`, from that display's top-left
corner.

- `WindowGeometry` holds `Option<WindowPosition>`; `WindowState::outer`,
  `WindowState::normal_geometry`, `WindowOptions::position`, the position of
  `WindowOptions::normal_geometry`, `WindowCommand::SetPosition` and
  `ResolvedPlacement::position` are that type.
- A snapshot's outer position is measured on the window's display, the one holding the largest
  part of it (the nearest when it touches none). `WindowState::successor` sets
  `WindowState::display` to the display a sample's position names: the two are one fact. A window
  on no display has no position.
- A window that spans displays or hangs past its display's edge has coordinates outside that
  display's extent, negative ones included, still in that display's logical pixels.
- A display left of or above the primary moves no window's coordinates.
- `WindowOptions::display` is gone: the position names its display.
- A host converts a position through its display's own origin and scale. Win32 and X11 share
  `DisplayFrame` (`flui-native`'s `window::desktop`): `origin + snap(at × scale)`, refused past
  the coordinate range (Win32's `DEVICE_COORDINATE_LIMIT`, X11's 16 bits) as
  `PlatformFailure::OutOfRange`, and back as `(pixel − origin) / scale`, in which every
  difference of two `i32`s is exact. AppKit adds the screen's corner in points, its desktop's
  unit. The headless host keeps a window's position on its display.
- A host not told where its windows are publishes no position and refuses `SetPosition`
  (`observes_position`, `sets_position`, `places_at_open`), unchanged.
- A position whose display is not in the host's published snapshot is no place: an opening is
  placed by the window system, and `SetPosition` answers `ApplyError::DisplayGone(id)`. A
  snapshot keeps naming the display its window was on until the window system reports where the
  window went.
- `flui-app`'s `AppConfig::position(x, y)` takes whole logical pixels of the primary display (the
  first where none is primary); the runner turns it into a `WindowPosition` when it opens the
  window, so no configured position refuses a start.

### §2 A display's bounds are its own logical pixels

`DisplayInfo` replaces `bounds: Bounds<i32>` and `usable_bounds` with:

- `bounds: DisplayBounds`: the display's extent (finite, positive) and its usable area (finite,
  inside the extent), in its logical pixels from its corner, built by `DisplayBounds::try_new`,
  `whole` or `from_device`, which refuse anything else with `InvalidDisplayBounds`;
- `origin: DesktopOrigin`: where its corner sits in the window system's desktop, in that window
  system's unit (device pixels on Windows, X11 and Android; points on macOS and iOS; the
  compositor's logical units on Wayland; the browser's screen coordinates on the web). It orders
  the displays and is never a logical position; no arithmetic mixes it with one.
- `DisplayInfo::from_device` builds a display of a device-pixel desktop.

### §3 Commands and options carry checked geometry

`WindowCommand::SetSize(WindowExtent)` and `WindowOptions::size: WindowExtent`, as resize steps
already are: a size that is not finite cannot be put in a command, and admission refuses one with
no area (`InvalidGeometry::Empty`). `WindowCommand::validate` and `WindowOptions::geometry` check
only what the types do not say (an area, the limits' order); `WindowOption::Position` is gone.
`WindowPlacement` holds its size as a `WindowExtent` and its scale as a `DevicePixelRatio`; its
stored format is unchanged.

### §4 The wire carries the display

The native recording writes a position as `{ display, at }`, the display by its label, and a
display as `origin`, `extent` and `usable`; `RecordedWindowState::into_parts` resolves each label
to the replay's id. Protocol 0.25 is a break (`BREAKING`).

### Consequences

- Good, because the same `SetPosition` lands on the same pixel whichever display the window is on,
  and the position a window publishes converts back to its corner exactly.
- Good, because a consumer fits a window to a display (placement, a menu, a dialog) in that
  display's logical pixels, with no platform's unit to know.
- Good, because a host receives only numbers a window can take, with no second check to forget.
- Bad, because an application cannot name a position before it has a display id; `AppConfig`
  measures on the primary display for that reason.
- Bad, because the desktop arrangement is opaque: a consumer can order displays but not lay one
  out in another's pixels.
- Neutral, because a recording or agent client of protocol 0.24 reads no 0.25 recording.

### Confirmation

- The compiler: `SetPosition`, `SetSize`, `WindowOptions::{size, position}` and
  `WindowGeometry::position` take the checked types; `WindowOptions::display` and the raw
  `DisplayInfo` bounds no longer exist.
- `flui-platform`'s `a_position_names_the_display_the_window_is_on`,
  `position_must_be_finite`, `size_must_be_finite_and_positive`,
  `an_opening_is_refused_for_what_a_command_is_refused_for`,
  `display_bounds_are_logical_pixels_of_the_display`,
  `a_panel_along_the_top_moves_the_offset_not_the_position`.
- `flui-native`'s `a_position_converts_through_its_own_display`,
  `a_window_spanning_two_displays_has_one_pixel_and_two_names`,
  `a_rectangle_is_on_the_display_holding_most_of_it` (the shared conversion),
  `open_geometry` (Win32 opening), `a_position_set_on_a_normal_window_moves_it` and
  `a_position_on_a_display_the_host_does_not_hold_is_refused` (Win32, real windows),
  `open_plan_places_by_display_origin_and_scale` and `a_window_is_on_the_display_it_overlaps_most`
  (X11), `a_position_on_a_display_becomes_a_global_one` (AppKit),
  `a_screen_is_published_in_its_own_points_from_the_top_left`.
- `flui-protocol`'s `wire_schema` with the 0.25 entry of `BREAKING`.
