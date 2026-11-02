# ADR-0286: Window chrome regions answer the system's hit test

- **Status:** Accepted (2026-11-02)
- **Date:** 2026-11-02
- **Amended:** 2026-11-02: §5 and §6, the press takers cut out of a move area and the widgets that mark the regions.
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
its presses and nobody marks it by hand. A caption button or resize region between the two keeps
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
- Neutral, because the native hosts do not answer from the regions yet; until they do, their
  capabilities leave the chrome kinds `Unavailable` and their windows open
  `WindowFrame::AppTitleBar` with the system's frame (`WindowFrame::at_open`).

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
