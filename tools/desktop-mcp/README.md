# flui-desktop-mcp

An [MCP](https://modelcontextprotocol.io) server that lets an agent (Claude Code, or any MCP
client) see and drive desktop applications the way a person with a screen reader does: list
and capture windows, read the accessibility tree, invoke element actions, and send real mouse
and keyboard input. It works on any application, not only FLUI ones, and it observes an app
strictly from the outside, through the OS accessibility, capture and input APIs.

It speaks MCP over stdio and is built on mature crates: [`rmcp`](https://crates.io/crates/rmcp)
(the official Rust MCP SDK), [`uiautomation`](https://crates.io/crates/uiautomation) (Windows UI
Automation), [`enigo`](https://crates.io/crates/enigo) (input) and
[`xcap`](https://crates.io/crates/xcap) (window listing and capture).

## Build and register

```bash
cargo build --release -p flui-desktop-mcp
# the binary: target/release/flui-desktop-mcp(.exe)
```

Register it with Claude Code (user scope, any project):

```bash
claude mcp add flui-desktop-mcp -- /absolute/path/to/target/release/flui-desktop-mcp
```

or for one project, in that project's `.mcp.json`:

```json
{
  "mcpServers": {
    "flui-desktop-mcp": {
      "command": "/absolute/path/to/target/release/flui-desktop-mcp",
      "args": []
    }
  }
}
```

Logs go to stderr; `RUST_LOG` filters them (default `info,xcap=off`: xcap logs an error for
every window whose process it may not open).

On macOS the server needs two permissions for the app that runs it (the terminal, or the MCP
client): Accessibility, for input (enigo asks for it at startup; input stays off until the
server restarts after it is granted), and Screen Recording, without which `list_windows` sees
almost no windows.

## Tools

Coordinates are screen coordinates everywhere (window rects, element rects, input): physical
pixels on Windows, where the server is per-monitor DPI aware, and points on macOS. A screenshot
reports how its pixels map back to them.

| Tool | Arguments | Does |
|------|-----------|------|
| `list_windows` | `title_contains?`, `pid?` | Top-level windows: `id`, `pid`, `app_name`, `title`, `rect`, `is_minimized`, `is_focused` (on macOS every window of the active app) |
| `launch` | `program`, `args?`, `cwd?`, `env?`, `wait_for_window_ms?` | Starts a process; returns its `pid`, and its first window, or its exit code if it exits first. Command line and environment at most 32 KiB |
| `kill` | `pid` | Ends a process this session launched (other pids are refused) |
| `screenshot` | `window_id?` \| `pid?` \| `monitor?`, `max_side?` | PNG of a window (captured even when covered, where the OS allows), a monitor, or the primary monitor, its longer side at most `max_side` (default 1920); plus size, the captured rect `source` and `scale_x`/`scale_y` |
| `accessibility_tree` | `window_id` \| `pid`, `max_depth?` | Nested nodes, `max_depth` levels (default 30, at most 200): `id`, `role`, `name`, `value`, `automation_id`, `class_name`, `rect`, `enabled`, `has_keyboard_focus`, `is_keyboard_focusable`, `toggle_state`, `patterns`, `children`, `omitted_children` |
| `find` | `window_id` \| `pid`, `name?`, `name_contains?`, `role?`, `automation_id?` | Matching nodes, flat |
| `wait_for` | as `find`, plus `timeout_ms?` | The first match once it exists, or a timeout error listing the last tree seen |
| `invoke` / `toggle` / `focus` / `select` | `element` | The Invoke / Toggle / focus / SelectionItem action, with no pointer involved |
| `set_value` | `element`, `value` | Value pattern (text, at most 100000 characters), or RangeValue (sliders, numeric `value`) |
| `click` | `element` \| `x`,`y`; `button?`; `double?`; `window_id?` \| `pid?` | A real click at the element's clickable point or a screen point; buttons are logical (a swapped mouse is honoured) |
| `move_mouse` | `x`, `y` | Moves the pointer; reports where it ended up |
| `drag` | `from`, `to`, `duration_ms?`, `window_id?` \| `pid?` | Left-button drag, at most 10 s |
| `scroll` | `x`, `y`, `dx?`, `dy?`, `window_id?` \| `pid?` | Wheel notches at a point, at most 100 each; positive is right/down |
| `type_text` | `text`, `window_id?` \| `pid?` | Types up to 10000 characters into the focused control; a newline is Enter, a tab is Tab |
| `key` | `combo`, `repeat?`, `window_id?` \| `pid?` | A key or combo: `enter`, `tab`, `f5`, `ctrl+shift+s`, `alt+f4`, `cmd+q`, `ctrl+plus`; on Windows a character that needs Shift or AltGr on the layout gets it |
| `activate_window` | `window_id` \| `pid` | Brings a window to the front; reports `became_foreground` (Windows only for now) |

Element ids (`e12`) are session handles issued by `accessibility_tree`, `find` and `wait_for`.
They are keyed by the element's UI Automation runtime id, so the same element keeps its id
across reads. A handle whose element, or whose process, is gone reports that it is stale, even
when UI Automation later gives its runtime id to a new element (which gets a new handle).

`screenshot` reports the captured screen rect (`source`) and `scale_x`/`scale_y`, image
pixels per screen unit measured from the image itself (2 on a Retina display, below 1 when
`max_side` shrank it): screen x = `source.x` + image x / `scale_x`. A window that moves, or no
longer belongs to the process it was listed for, is refused rather than returned with bounds or
pixels that do not match.

A read (`accessibility_tree`, `find`, one `wait_for` poll) fetches at most 5000 elements and
16 MiB of strings, cuts any one string at 4096 characters (ending in `…`), and stops 10 s after
it starts, so a huge, hostile or hung tree cannot hold the server; UI Automation calls also
time out on their own (5 s). A reply that left anything out (a budget, the depth, a provider
failing partway, a clipped or unreadable property) says `truncated: true`, and an empty `find`
is then no proof the element is absent. A process's reads include its popup menus and
drop-downs, which are windows of their own; an element that shows up under two windows (an
owned dialog) is reported once.

A failing call returns a tool error (`isError: true`) whose text names what went wrong and,
where there is one, the call that would succeed: the patterns an element does support, the
window that is in front instead, the tree seen before a timeout. That includes arguments that
do not parse. An action that stopped partway says how much of it already went out (characters
typed, presses sent, the first click of a double click); a pattern action whose call failed
after reaching the application, or whose element vanished during it, says it may have run; an
action that ran but could not be read back says so. Look before retrying any of them.

A request the client cancels while it waits for the desktop thread never runs, and at most 32
calls wait at once; past that a call is refused as busy.

## Safety rule for input

`click`, `drag`, `scroll`, `type_text` and `key` take an optional `window_id` or `pid`.
**Agents should always pass it.** When it is given, the server checks that the target owns
the foreground window and that at every coordinate the OS reports the target — a `window_id`
admits only that window at the point, a `pid` any of the process's windows (its own popups) —
with the deepest window there, the one that takes the click, in the same process (a preview
pane another process hosts is refused). The check runs again before **every** event of a
multi-event action — each repeat of `key`, each modifier press, each character of
`type_text`, each step of `drag` and its release, each click of a double click — so a window
that takes the foreground partway through receives none of the rest. Otherwise it refuses and
sends nothing, so keystrokes and clicks never land in another application.

- A window id is bound to the process that owned it when this session listed it, and to that
  process's start time; a pid to the start time of the process it named when listed or
  launched. OSes recycle both, and a target that now names another process is refused, before
  every event. A pid this session never handed out is refused, and so is any pid where the OS
  reports no start time (macOS, for now). Neither a later `list_windows` nor `activate_window`
  re-binds an id or pid; only `launch` binds the pid of the process it just started.
- Keys and text also require the window holding keyboard focus inside the target to belong
  to the target's process: an embedded browser or preview pane of another process with focus
  refuses them. Where the OS cannot say which window has focus (macOS, for now), keys and text
  with a target are refused.
- A drag that stops partway releases the button (the drop) at a point verified inside the
  target; if none verifies, the release still has to go out (a held button would drag on), and
  the error says where it happened. No other event is sent unverified.
- A button or key still held from a release that failed is released before the next input, or
  that input is refused; a call that panics mid-action releases everything it held.
- An element click requires the element itself (or a descendant) to be what UI Automation
  hit-tests at the point, the element's own top-level window to be under it and its
  application in front, target or not. A popup menu is a window of its own, never the
  foreground one, so target it with `pid`.
- Shell hotkeys reach the shell, not the window in front, so `key` refuses them when a target
  is given: on Windows the Windows key, `alt+tab`, `alt+esc`, `ctrl+esc`, `ctrl+shift+esc`,
  `ctrl+alt+delete`, the input-language switch (`alt+shift`, `ctrl+shift`) and Shift pressed five
  times; on macOS `cmd+tab`, `cmd+space`, `ctrl+space`, Force Quit, Lock Screen, Log Out, Mission
  Control and Spaces, the screenshot tool, the Dock and keyboard-navigation shortcuts. Other
  applications' own global hotkeys are not detected.
- The pointer position is read back before every press: a move the OS clamped, or a pointer
  someone else moved, refuses the press.

The check fails closed: where the OS cannot say which window is under a point or holds
keyboard focus (macOS, for now), input with a target is refused rather than sent unverified.
What it cannot close is the gap between the last check and the event itself, a few
milliseconds.

The pattern tools (`invoke`, `toggle`, `set_value`, `focus`, `select`) send no input at all
and work on covered windows; prefer them where the element supports the pattern.

Windows restricts which process may take the foreground. `activate_window` restores and
raises the window and falls back to UI Automation focus; check `became_foreground` before
sending input.

## Platform support

| | Windows | macOS | Linux |
|---|---|---|---|
| `list_windows`, `screenshot` | yes (xcap) | built (xcap); type-checked in CI (clippy), never run | not yet |
| Input (`click`, `key`, …) | yes (enigo; pointer moves via `SetCursorPos`) | built (enigo); with a target, refused for now; type-checked in CI, never run | not yet |
| Accessibility tools | yes (UI Automation) | "not supported on this OS yet (UIA only)" | same |
| `activate_window` | yes | not supported yet | not supported yet |
| Launched processes ended on server exit | yes, with everything they started, also on a hard kill (the server runs in a kill-on-close job; `launch` refuses without it) | on a clean exit, the launched processes only | same |

`kill` ends the launched process itself; what it started ends when the server exits (Windows).

On Linux the server builds, starts and lists its tools, and every desktop tool reports that the
OS is not supported yet: xcap links PipeWire and XCB there and enigo links libxkbcommon, system
libraries the workspace's Linux builds (CI included) do not install. Enabling them is a
manifest change plus those packages.

The accessibility layer is one trait (`AccessibilityBackend` in `src/a11y/mod.rs`); a macOS
backend over AX (`objc2-application-services`) or a Linux one over AT-SPI (`atspi`) is a new
module behind it.

All UI Automation and input work runs on one dedicated thread, in arrival order: UIA objects
belong to that thread's COM apartment, and synthesized input must not interleave.

## Tests

```bash
cargo nextest run -p flui-desktop-mcp
```

runs on any host without a desktop: key-combo parsing and shell-hotkey detection, the
keystrokes text becomes, the element-handle cache, argument validation, the input safety
checks, the binding refusals of a whole `Desktop`, cancellation on the desktop thread, process
bookkeeping, and `tests/protocol.rs`, which spawns the binary and speaks MCP over stdio: every
tool is listed with a schema, and malformed arguments come back as tool errors.

`tests/live_windows.rs` is an ignored test for an interactive Windows desktop. It launches the
repository's `a11y_probe` counter, lists and captures its window, reads the tree, finds and
invokes the Increment button, clicks it, and checks each press changed the count; then it
presses Tab and `ctrl+plus`, types, scrolls and drags, and checks that refused input is refused
for the reason given. Real input is the point, so when Windows will not give the probe the
foreground it fails as inconclusive instead of passing without it:

```bash
cargo build --release --example a11y_probe --features material,a11y
cargo nextest run -p flui-desktop-mcp --test live_windows --run-ignored only --no-capture
```

It moves the real pointer and sends real input to the probe window only, each step only after
the probe is confirmed in front.

## Relation to the other live checks

- `cargo xtask device windows-a11y` and `windows-input` are fixed pass/fail gates over the
  same `a11y_probe` (UI Automation, and `SendInput`); `cargo xtask device macos-*` are the
  Mac ones. A gate proves one scripted path; this server is for everything else, and
  `tests/live_windows.rs` exercises the server itself.
- `cargo xtask live-smoke` (`tools/live-smoke`) is the CI gate for real X11/Wayland input.
- This server is the interactive tool: an agent uses it to explore and test any app by hand,
  on the desktop in front of it.
