# ADR-0354: A window shows the cursor its region asks for, holds the pointer only while focused, and says when the user resizes it

- **Status:** Accepted (2026-11-16)
- **Date:** 2026-11-16
- **Supersedes:** [ADR-0343](ADR-0343-the-owner-promises-turn-deadlines-and-the-window-states-user-placement.md) decision 2 only for its set of interactions and the rule a consumer counts the user's change by
- **Related:** [ADR-0254](ADR-0254-window-contract.md) §1 (one source per window fact), §3 (verbs
  and commands); [ADR-0349](ADR-0349-the-window-contract-states-each-fact-in-one-shape-and-every-window-says-what-it-does.md)
  (each fact in one shape, `PlatformFailure`, no default bodies);
  [ADR-0343](ADR-0343-the-owner-promises-turn-deadlines-and-the-window-states-user-placement.md) (only the
  user's change is recorded); [ADR-0143](ADR-0143-flui-owned-input-event-vocabulary.md) (the pointer
  vocabulary); [ADR-0342](ADR-0342-a-value-added-to-a-closed-union-breaks-the-agent-protocol.md) (a new variant of
  a closed union is a break)

## Context and problem statement

The window contract had one cursor verb, `PlatformWindow::set_cursor(CursorIcon)`. It had no way to
hide the cursor, show an application's image, hold the pointer inside the window or in place (the
web's pointer lock, a 3D view that turns with the mouse), no window level (always on top or below),
no resize steps or proportion for the user's resize, and no fact that the user is resizing the
window by its frame. winit, SDL3, Rune and Raven carry some of each; `docs/gaps.md` named none.

The question: what shape does each take in a contract whose facts live in one snapshot, whose
changes are admitted commands, and whose per-frame requests are verbs?

## Decision drivers

- A fact has one source (ADR-0254 §1) and one shape (ADR-0349); a state no window has is not
  representable.
- What a host cannot do is a typed answer: a capability `Absent` with its reason, never a no-op.
- A grab is the desktop's (a Win32 cursor clip, an X11 pointer grab, a browser's pointer lock):
  left behind, it captures the user's pointer outside the application.
- Accepted work is delivered or refused on every exit (AGENTS.md, "Before each push").

## Considered options

For hiding and images:

1. A window-wide visibility command beside the cursor verb (winit's `set_cursor_visible`).
2. The cursor verb takes a `Cursor` value: an icon, an image or hidden, shown over the region that
   asked for it (CSS `cursor: none`, Rune's `Cursor::Hidden`).

For the grab:

- (a) A command that holds until released, re-taken by the host when the focus comes back (SDL).
- (b) A command honoured only while the window has the keyboard focus, ended with it, and a fact of
  the snapshot (`cursor_grab`); the application asks again.

For the live resize:

- (i) A new handler method or a `resizing: bool` snapshot fact.
- (ii) A variant of the existing `WindowInteraction`, which already says the user is placing the
  window.

## Decision

Chosen: 2, (b) and (ii).

### 1. The cursor is a value of the region that asked for it

```rust
pub enum Cursor { Icon(CursorIcon), Image(CursorImage), Hidden }
fn set_cursor(&self, cursor: &Cursor) -> Result<(), CursorError>; // PlatformWindow
```

A window-wide visibility switch beside a per-region cursor is two sources for one fact (which one
wins when the hovered region asks for another cursor?) and on AppKit and Win32 a process-wide
counter that leaks across windows. Hidden as a cursor is per window and per region by
construction, and comes back with the next cursor. `CursorImage` is straight-alpha RGBA8 of device
pixels, at most `CursorImage::MAX_SIDE` a side, checked before its pixels are read, and shares its
pixels so a host compares the last image it made a native cursor of cheaply. An image is a verb's
argument, so its capability is a plain `Availability`, `WindowCapabilities::cursor_image`, which
`set_cursor` answers `Unsupported` for exactly when it is `Unavailable` (as the frame-rate hint).

### 2. A grab holds only while the window has the keyboard focus

```rust
pub enum CursorGrab { None, Confined, Locked }
WindowCommand::SetCursorGrab(CursorGrab)          // slots: confines_cursor, locks_cursor
pub cursor_grab: CursorGrab                        // WindowState
PointerEvent::RawMotion(RawMotion)                 // only while Locked, instead of moves
```

Every window system ends a grab when the user switches away (the browser ends a pointer lock on
Escape and blur and refuses to re-take it without a gesture), so re-taking it on focus (a) is not
possible everywhere and would hold the pointer of a user who did not ask. `WindowState::successor`
publishes `CursorGrab::None` for a window without the keyboard focus; every host lets the desktop's
grab go on focus loss, hide and close. A host commits what it holds and publishes on every exit of
a grab or level command, a refusal included, so the snapshot says what is held, never what was
asked. A lock keeps its point relative to the content: when the window moves, resizes or changes
scale, the host holds the pointer at that point of the content (inside it, if the content shrank;
on the part a display shows, if the content went partly off its displays), never at an old screen
point over another window or at a point no display shows. A window none of whose content a display
shows holds no grab on Windows, where the grab is the desktop's cursor clip: the host lets it go
and publishes `CursorGrab::None`.
`SetCursorGrab(None)` has no slot, as the normal mode has
none. A locked pointer is not shown, and its motion is a `RawMotion` built by the one registry
function every host calls (`ContactRegistry::raw_motion`), carrying a checked `MotionDelta` and
whether it is before or after the system's acceleration (`MotionAcceleration`).

### 3. A level is a fact; steps and a proportion are requests

```rust
pub enum WindowLevel { Normal, AlwaysOnTop, AlwaysOnBottom }
WindowCommand::SetLevel(WindowLevel)                       // slots: keeps_on_top, keeps_on_bottom
pub level: WindowLevel                                     // WindowState
WindowCommand::SetResizeIncrements(Option<WindowExtent>)   // slot: sets_resize_increments
WindowCommand::SetAspectRatio(Option<AspectRatio>)         // slot: sets_aspect_ratio
```

The user or another program changes a level (a window manager's "Always on Top", a Win32
`WS_EX_TOPMOST` another process clears), so it is a snapshot fact. Steps and a proportion only
the application sets and the window system holds during the user's resize; like the material
they are requests, not facts. `AspectRatio` is two `NonZeroU32` terms in lowest terms. Fullscreen
wins over a level while it lasts: a window kept above or below the others is shown on top of its
display while fullscreen, keeps its level in the snapshot, and is stacked by that level again
when it leaves fullscreen, on every host that has both. A host
without them natively builds them (`Emulated`: Win32 holds the `WM_SIZING` rectangle).

### 4. A live resize is the user's interaction

`WindowInteraction::UserResizing` beside `UserPlacing`, and `WindowInteraction::is_user` reading
either. A separate boolean would let `interaction` say "the user is placing" while `resizing` said
"resizing": two shapes of one fact (ADR-0349). The placement recorder (ADR-0343) records a change
whose report `is_user`. Win32 states it from the first `WM_SIZING` of its size-move loop to the
loop's end, AppKit between `windowWillStartLiveResize:` and `windowDidEndLiveResize:`, Wayland
while the toplevel's configure carries `resizing`; X11, UIKit, Android and the web read no signal
for it and state what they stated before.

### Recordings

The native recording mirrors the new capabilities, `cursor_grab`, `level` and `user_resizing` as
optional fields; the input recording gains `raw_motion`, a new variant of a union its reader
refuses whole, so its format version moves and protocol 0.24 is a break relative to 0.23 (ADR-0342).

## Consequences

- Good: an application hides the cursor, shows its own, locks or confines the pointer, keeps a
  window on top, steps or proportions the user's resize, and throttles work during a live resize,
  on every host that can, through `WindowHandle` and the snapshot; every other host says why not.
- Good: a grab cannot outlive the focus in the snapshot, and the conformance check
  `a_grab_ends_with_the_focus` holds each host's window system to it.
- Bad: a widget cannot yet ask for an image or a hidden cursor (the framework's cursor request
  carries an icon); `docs/gaps.md` records it with the consumer that brings it.

## Compliance

- `crates/flui-platform/tests/window_command_validate.rs`:
  `admit_refuses_exactly_when_the_capability_is_unavailable`,
  `restoring_normal_needs_no_capability`, `a_grab_is_published_only_for_a_focused_window`,
  `a_cursor_image_holds_only_what_a_window_system_shows`, `an_aspect_ratio_is_held_in_lowest_terms`.
- `flui_testing::window_conformance` check `a_grab_ends_with_the_focus` and the fault
  `KeepsTheGrabWithoutFocus` it catches (`a_grab_kept_after_the_focus_went_is_caught`).
- Win32: `a_grab_is_held_while_focused_and_let_go_with_the_focus`,
  `a_resize_by_the_frame_is_a_live_resize_until_the_loop_ends`,
  `a_frame_drag_keeps_the_steps_and_the_proportion`, `a_level_is_what_win32_holds`
  (`crates/flui-native/tests/windows_window_commands.rs`).
- Each host's capability table names every new field (`a_wayland_window_has_the_capabilities_it_names`,
  `an_x11_window_has_the_capabilities_it_names`, `crates/flui-native/src/window/capabilities.rs`).
