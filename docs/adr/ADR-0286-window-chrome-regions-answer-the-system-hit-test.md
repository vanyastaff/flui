# ADR-0286: Window chrome regions answer the system's hit test

- **Status:** Accepted (2026-11-02)
- **Date:** 2026-11-02
- **Amended:** 2026-11-02: §5 and §6, the press takers cut out of a move area and the widgets that
  mark the regions; §7, the Win32 frame. 2026-11-03: §8, the AppKit title bar; §9, the winit
  window; §10, the X11 window; §11, the Wayland window.
- **Retired:** 2026-11-03: §9's winit window left with the winit host (ADR-0151 §11). Its press
  rules (the resize band, the move threshold, the double-click and the window menu) stand as the
  rules §10 and §11 refer to.
- **Amends:** [ADR-0217](ADR-0217-interaction-module-layers-and-pointer-assembly.md) §5: input
  regions gain the chrome kinds the title-bar follow-up named, and their availability is
  answered per kind.
- **Related:** [ADR-0254](ADR-0254-window-contract.md) (the window contract and its per-frame
  verbs), [ADR-0098](ADR-0098-owned-f64-geometry-values.md) (logical and device geometry)

## Context and problem statement

`WindowFrame::AppTitleBar` lets an application draw its own title bar inside a frame the
system keeps. The system still moves, resizes, snaps and maximizes the window, so on every
press it asks the window what lies under the pointer: Win32 sends `WM_NCHITTEST`, AppKit asks
whether a view is draggable, Wayland and X11 wait for the client to start an interactive move
or resize. Win32 asks synchronously, inside the window procedure, and expects an answer within
that call. The answer depends on the application's widgets: a search field in the bar is
content, the gap beside it moves the window, a close button is a caption button with its own
snap layout menu on Windows 11. How does a backend answer from what the application painted,
and how does the application learn which of these a window honours?

## Decision drivers

- The frame path stays synchronous, and the system's callback runs no framework code: no
  build, no layout, no hit test inside `WM_NCHITTEST`.
- One source of truth: what a frame paints is what the system's answer reads.
- The answer follows paint order, so a widget painted over a move area wins without the
  application listing exclusions.
- Each host honours a different subset (gesture exclusion on mobile, the full chrome on Win32),
  and an application can ask which before it relies on one.
- Device pixels at the seam: the system asks in device pixels and the regions are projected
  once per frame (ADR-0217 §5).

## Considered options

1. Run the framework's hit test from the system's callback.
2. A dedicated title-bar description (`TitleBarLayout { height, buttons, … }`) the window holds.
3. Chrome kinds of input regions, resolved in paint order by one shared function, with
   availability per kind.

## Decision

Chosen option: 3, because the regions already travel from layout to every backend on every
composited frame, in device pixels, and a kind per chrome role keeps one path for all of them.

§1. **Chrome kinds.** `InputRegionKind` gains the kinds a frame drawn by the application needs:

```rust
#[non_exhaustive]
pub enum InputRegionKind {
    SystemGestureExclusion,
    WindowMove,
    WindowResize(ResizeEdge),
    CaptionButton(CaptionButton),
    WindowContent,
}

#[non_exhaustive]
pub enum ResizeEdge { Top, Bottom, Left, Right, TopLeft, TopRight, BottomLeft, BottomRight }

#[non_exhaustive]
pub enum CaptionButton { Minimize, Maximize, Close }
```

`WindowContent` is a cut-out: content painted over a move or resize area that gives the press
back to the application.

§2. **Paint order decides.** `PipelineOwner::input_regions` reports regions in paint order, a
node before its descendants and siblings in the order their parent last painted them, and
`set_input_regions` receives them in that order. A parent that paints its children other than first to last (a viewport whose first
sliver is on top, a sliver group whose pinned header covers its content) has the order its last
paint used recorded by the paint walk, and the region walk follows it. One function in
`flui-platform` answers the system's question for every backend:

```rust
#[non_exhaustive]
pub enum ChromeHit { Content, Move, Resize(ResizeEdge), CaptionButton(CaptionButton) }

pub fn chrome_hit(regions: &[DeviceInputRegion], point: DevicePoint) -> ChromeHit;
```

The last chrome region whose rectangle holds the point decides; a rectangle holds its minimum
edges and not its maximum ones; `SystemGestureExclusion` is not chrome and is passed over; a
point no chrome region holds is `Content`. A backend calls it from the system's callback on the
set the last composited frame handed it.

§3. **Availability per kind.** `InputCapabilities` answers per kind instead of with one
`input_regions` flag:

```rust
impl InputCapabilities {
    pub const fn input_region(&self, kind: InputRegionKind) -> Availability;
    pub const fn honours_input_regions(&self) -> bool;
}
impl InputCapabilitiesBuilder {
    pub const fn system_gesture_exclusion(self, availability: Availability) -> Self;
    pub const fn window_move(self, availability: Availability) -> Self;
    pub const fn window_resize(self, availability: Availability) -> Self;
    pub const fn caption_buttons(self, availability: Availability) -> Self;
}
```

`WindowContent` is honoured where move or resize regions are, since it only means something cut
out of them. `set_input_regions` is `Unsupported` exactly when `honours_input_regions` is false
(the ADR-0254 §3 rule, now read from the kinds), and otherwise keeps the regions of the kinds the
window honours and drops the rest. The headless host honours every kind unless a test says
otherwise, and filters as a native host does.

§4. **Recording format.** A recorded backend profile carries the four availabilities;
`flui-testing`'s recording format moves to version 3 and refuses version 2.

§5. **Press takers are cut out of a move area.** A render object says it takes presses with
`InputRegionSink::claim_presses` from `describe_input_regions`; `RenderListener` does when its
widget has a pointer-down handler or recognizers in the gesture arena, a hover-only listener does
not. While the walk is under a node that reported a `WindowMove` region, such a node adds a
`WindowContent` region over its box after its own regions, so a button or a field in the bar keeps
its presses and nobody marks it by hand. `InputRegionSink::claim_presses_over` takes presses on
part of the box only: `RenderParagraph` says it over the glyphs of each routed link, so a link in
the bar is pressed and the text around it moves the window. A caption button or resize region between the two keeps
its subtree's presses its own (the system answers them), and
`InputRegionSink::pass_presses_through` keeps a subtree moving the window until a move area below
it starts cutting out again: `RenderPressPassThrough` says it, and so does an ignoring
`RenderIgnorePointer` or `RenderSliverIgnorePointer`, whose subtree no press reaches. Inside a
cut-out such a node adds a `WindowMove` region over its box, so a handle inside a button still
moves the window.

§6. **Widgets.** `flui-widgets` marks the regions: `WindowTitleBar` (`WindowMove`),
`WindowCaptionButton::{minimize, maximize, close}` (`CaptionButton`, a `RawButton` named after
what it does now, acting through the window's `WindowHandle`), `WindowResizeArea::new(edge)`
(`WindowResize`, only while the window is in its normal mode) and `WindowMovePassThrough`. Each
reads the window's frame through a `WindowHandle` subscription and marks nothing while the system
draws the title bar, so one tree runs on every frame and host. Ordering the caption buttons after
the platform's convention belongs to the title bars the design packages build from these parts.

§7. **Win32.** A window with `AppTitleBar` keeps the overlapped style, so the system still
resizes, snaps, animates and offers its window menu. `WM_NCCALCSIZE` keeps the client the default
procedure computes but moves its top to the window's (a maximized window's to the monitor's), so
the caption and the top border go and the side and bottom borders stay. `WM_NCHITTEST` keeps the
default answer over those borders and otherwise answers from the regions the last composited frame
handed `set_input_regions`, through `chrome_hit`: a resize band of the sizing frame's thickness
along the top of a resizable window that is not maximized, then `HTCAPTION`, the resize edges and
`HTMINBUTTON`, `HTMAXBUTTON` (the snap layouts) and `HTCLOSE`. The non-client pointer messages over
a caption button reach the application's button as client ones and never the default procedure, so
the application's tap decides. A `WindowFrame::None` window answers from the regions without the
band. A fullscreen window answers as the system does.

§8. **AppKit.** A window with `AppTitleBar` keeps `NSWindowStyleMaskTitled` and adds
`NSWindowStyleMaskFullSizeContentView` with a transparent, untitled title bar, so the content
reaches the top, the system still resizes, tiles, zooms and enters full screen, and the traffic
lights stay the system's. `SystemControls` hides them or moves them by an offset, the bar growing
down to keep them inside it and the offset mirrored in a right-to-left layout. A left press on a
`Move` region calls `performWindowDragWithEvent:`, and a second click of it does what
`AppleActionOnDoubleClick` (or the older `AppleMiniaturizeOnDoubleClick`) names: zoom, minimize
or nothing. Every other press reaches the application. AppKit keeps the resize borders and the
caption buttons, so its input capabilities answer only `window_move`, and the other chrome kinds
stay `Unavailable`.

§9. **winit.** A window with `AppTitleBar` has no decorations: no window system draws a frame
around it, so the window answers each press itself, from the regions and from a resize band of
its own (6 logical pixels along every edge of a window that is resizable, not maximized and not
full screen), the band above any region. A primary press on the band or on a `WindowResize`
region calls `drag_resize_window`. One on a `Move` region calls `drag_window` once the pointer
has travelled 4 logical pixels while held, as the system's own title bars wait: a window manager
handed the move at the press holds the pointer, and the second click of a double-click would end
its move instead of reaching the window. A second press within 500 ms and 4 logical pixels of a
first that did not move the window toggles maximize, since winit reads no system setting for
either; a secondary press on the bar opens the window menu (`show_window_menu`). The loop takes
each such press and its release from the handler; the next press of the button ends a taken one
whose release the window system kept. A
`WindowFrame::None` window answers from its regions without the band. Caption buttons are the
application's own buttons, so winit's input capabilities answer `window_move` and
`window_resize` and leave `caption_buttons` `Unavailable`.

§10. **X11.** A window with `AppTitleBar` has no decorations (`_MOTIF_WM_HINTS`) and answers its
presses as §9 says, from the XInput 2 events the loop hands it before the input: a move or a
resize is a `_NET_WM_MOVERESIZE` request to the window manager, sent once the pointer's grab is
let go of, and a double-click enters or leaves maximized through `_NET_WM_STATE`. The window
manager's menu has no request every window manager takes, so a secondary press stays the
application's, as does a touch's emulated pointer. The window's capabilities take the title bar
only where the window manager lists `_NET_WM_MOVERESIZE`; elsewhere the window opens with the
window manager's frame. The input capabilities answer `window_move` and `window_resize`.

§11. **Wayland.** A window with `AppTitleBar` asks the compositor for no frame (`xdg-decoration`'s
client side, where it offers one) and keeps the margins of the frame it draws itself, which a
press resizes the window from (`xdg_toplevel.resize`); the title bar of that frame is gone, and
the window's geometry is its content. A press on the content is answered as §9 says, from the
`wl_pointer` events before they reach the input: a move is `xdg_toplevel.move` with the seat and
serial of the press, sent once the pointer travels; a double-click maximizes or restores the
window where the compositor maximizes it; a secondary press on the bar shows the compositor's
window menu (`show_window_menu`) where it offers one, and stays the application's elsewhere. No
resize band lies inside the content, as the margins outside it take that part. The window's
capabilities take the title bar where it can draw its own frame and the compositor has not
insisted on drawing one; elsewhere the window opens with the compositor's frame, and one the
compositor overrules reads back as the system's. The input capabilities answer `window_move` and
`window_resize`.

### Consequences

- Good, because the system's callback reads a slice and returns: no framework code, no lock
  held across application code, and an answer that matches the last frame the user saw.
- Good, because a widget painted over the bar takes its press back without the application
  maintaining an exclusion list.
- Good, because a capability per kind lets mobile keep gesture exclusion without claiming
  window chrome.
- Bad, because the answer can be one frame behind a layout change: a press during the frame
  that moves a button is resolved against the previous frame's regions. A frame is the
  granularity every other input path has too.
- Neutral, because every desktop host answers from the regions, yet a window that cannot take
  the title bar opens `WindowFrame::AppTitleBar` with the system's frame (`WindowFrame::at_open`):
  an X11 window where the window manager lists no `_NET_WM_MOVERESIZE`, a Wayland window that
  cannot draw a frame of its own.

### Confirmation

- `chrome_hit_takes_the_last_region_painted_over_the_point` and
  `a_chrome_region_holds_its_minimum_edges_and_not_its_maximum_ones` in `flui-platform` pin §2.
- `a_window_honours_the_region_kinds_its_input_capabilities_name` pins §3's per-kind answers.
- `headless_drops_the_kinds_its_os_does_not_honour` and
  `headless_without_any_kind_refuses_input_regions` in `flui-native` pin the filtering and the
  refusal; the window conformance check
  `per_frame_verbs_report_unsupported_exactly_when_unavailable` reads `honours_input_regions`.
- `harness_input_regions_cut_press_takers_out_of_a_move_area`, `harness_press_pass_through_disabled_cuts_out_again` and
  `harness_input_regions_follow_the_order_a_parent_paints_in` in `render_object_harness` pin §5
  and §2's paint order.
- The `window_chrome` table in `flui-widgets` pins §6 through the headless kit: the regions a bar
  with a button, a hover area, a pass-through and a close button publishes, nothing under the
  system's frame, a frame change, no resize border while maximized, and the caption buttons
  minimizing, maximizing, restoring and asking to close under the names they announce.
- The pure halves of `flui-native`'s Win32 window and input, on the Linux CI host
  (`the_hit_test_answers_from_the_regions_below_the_top_band`,
  `only_pointer_messages_over_a_caption_button_become_the_applications`,
  `the_client_reaches_the_top_and_a_maximized_one_stays_on_the_monitor`), and on Windows
  `an_app_title_bar_drops_the_caption_and_keeps_the_content` and
  `the_system_hit_test_answers_from_the_chrome_regions` in `native_window_commands`, pin §7.
- The pure halves of `flui-native`'s AppKit window and input, on the Linux CI host
  (`each_frame_asks_its_style_of_the_window`,
  `moved_traffic_lights_stay_inside_a_bar_that_grows_down`,
  `a_left_press_on_the_bar_moves_the_window_and_a_second_click_asks_the_setting`,
  `a_double_click_on_the_bar_does_what_system_settings_says`), and
  `the_desktop_hosts_honour_the_chrome_regions_they_answer` for the kinds each host honours, pin
  §8.
- `a_press_on_a_frameless_window_moves_resizes_or_reaches_the_app` and
  `a_second_press_on_the_bar_soon_and_near_toggles_maximize` in `flui-native`'s shared press
  model (`input::regions::presses`) pin §9's press rules, which §10 and §11 apply.
- `a_title_bar_of_the_applications_moves_and_resizes_through_the_window_manager` in
  `flui-native`'s `x11_input` suite, run on Xvfb under openbox, pins §10.
- `a_press_on_the_applications_bar_is_the_windows` in `flui-native`, and
  `a_title_bar_of_the_applications_moves_and_resizes_through_the_compositor` in its
  `wayland_frame` suite, run on Weston 13 with XTest, pin §11.

## Pros and cons of the options

### 1. The framework's hit test from the system's callback

- Good, because the answer is exact for the current tree.
- Bad, because `WM_NCHITTEST` runs inside the window procedure, possibly while the framework is
  mid-frame or re-entered: it would need the tree borrowed from a system callback.
- Bad, because the answer would differ from the regions other backends receive.

### 2. A dedicated title-bar description

- Good, because it is small and declarative.
- Bad, because it cannot express a field or a menu inside the bar, or a bar that is not a strip.
- Bad, because it is a second channel beside the input regions with its own diffing and scaling.

### 3. Chrome kinds of input regions in paint order

- Good, because it reuses the per-frame projection and diff ADR-0217 §5 already built.
- Good, because one shared resolver makes every backend give the same answer for the same set.
- Bad, because the kind enum grows with each chrome role a system adds; it is
  `#[non_exhaustive]` for that.

## More information

Win32 documents the `WM_NCHITTEST` answers (`HTCAPTION`, `HTTOP` … `HTBOTTOMRIGHT`,
`HTMINBUTTON`, `HTMAXBUTTON`, `HTCLOSE`); returning `HTMAXBUTTON` is what shows Windows 11's snap
layout menu over an application's maximize button. Wayland's `xdg_toplevel.move` and `resize`
and X11's `_NET_WM_MOVERESIZE` start the system's interaction from the client's own hit test,
which `chrome_hit` gives them.
