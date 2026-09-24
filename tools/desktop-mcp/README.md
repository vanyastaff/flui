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
      "command": "D:\\flui\\target\\release\\flui-desktop-mcp.exe",
      "args": []
    }
  }
}
```

Logs go to stderr; `RUST_LOG` filters them (default `info,xcap=off`: xcap logs an error for
every window whose process it may not open).

## Tools

Coordinates are physical screen pixels everywhere: the server is per-monitor DPI aware, so
window rects, element rects, screenshots (at `scale` 1) and input all share one space.

| Tool | Arguments | Does |
|------|-----------|------|
| `list_windows` | `title_contains?`, `pid?` | Top-level windows: `id`, `pid`, `app_name`, `title`, `rect`, `is_minimized`, `is_focused` |
| `launch` | `program`, `args?`, `cwd?`, `env?`, `wait_for_window_ms?` | Starts a process; returns its `pid` (and first window). Every launched process is killed when the server exits |
| `kill` | `pid` | Ends a process this session launched (other pids are refused) |
| `screenshot` | `window_id?` \| `pid?` \| `monitor?`, `max_side?` | PNG of a window (captured even when covered, where the OS allows), a monitor, or the primary monitor; plus size, captured rect and `scale` |
| `accessibility_tree` | `window_id` \| `pid`, `max_depth?` | Nested nodes: `id`, `role`, `name`, `value`, `automation_id`, `class_name`, `rect`, `enabled`, `has_keyboard_focus`, `is_keyboard_focusable`, `toggle_state`, `patterns`, `children` |
| `find` | `window_id` \| `pid`, `name?`, `name_contains?`, `role?`, `automation_id?` | Matching nodes, flat |
| `wait_for` | as `find`, plus `timeout_ms?` | The first match once it exists, or a timeout error listing the last tree seen |
| `invoke` / `toggle` / `focus` / `select` | `element` | The Invoke / Toggle / focus / SelectionItem action, with no pointer involved |
| `set_value` | `element`, `value` | Value pattern (text), or RangeValue (sliders, numeric `value`) |
| `click` | `element` \| `x`,`y`; `button?`; `double?`; `window_id?` \| `pid?` | A real click at the element's clickable point or a screen point |
| `move_mouse` | `x`, `y` | Moves the pointer; reports where it ended up |
| `drag` | `from`, `to`, `duration_ms?`, `window_id?` \| `pid?` | Left-button drag |
| `scroll` | `x`, `y`, `dx?`, `dy?`, `window_id?` \| `pid?` | Wheel notches at a point; positive is right/down |
| `type_text` | `text`, `window_id?` \| `pid?` | Types characters into the focused control |
| `key` | `combo`, `repeat?`, `window_id?` \| `pid?` | A key or combo: `enter`, `tab`, `f5`, `ctrl+shift+s`, `alt+f4`, `cmd+q` |
| `activate_window` | `window_id` \| `pid` | Brings a window to the front; reports `became_foreground` |

Element ids (`e12`) are session handles issued by `accessibility_tree`, `find` and `wait_for`.
They are keyed by the element's UI Automation runtime id, so the same element keeps its id
across reads. A handle whose element the application has removed reports that it is stale.

A failing call returns a tool error (`isError: true`) whose text names what went wrong and,
where there is one, the call that would succeed: the patterns an element does support, the
window that is in front instead, the tree seen before a timeout.

## Safety rule for input

`click`, `drag`, `scroll`, `type_text` and `key` take an optional `window_id` or `pid`.
**Agents should always pass it.** When it is given, the server checks, immediately before
sending the input, that the target owns the foreground window and that every coordinate lies
inside that window and is not covered there by another application's window. Otherwise it
refuses and sends nothing, so keystrokes and clicks never land in another application. An
element click always requires the element's own window to be in front, target or not.

The pattern tools (`invoke`, `toggle`, `set_value`, `focus`, `select`) send no input at all
and work on covered windows; prefer them where the element supports the pattern.

Windows restricts which process may take the foreground. `activate_window` restores and
raises the window and falls back to UI Automation focus; check `became_foreground` before
sending input.

## Platform support

| | Windows | macOS | Linux |
|---|---|---|---|
| `list_windows`, `screenshot` | yes (xcap) | built (xcap), never run or type-checked here | not yet |
| Input (`click`, `key`, …) | yes (enigo; pointer moves via `SetCursorPos`) | built (enigo), never run or type-checked here | not yet |
| Accessibility tools | yes (UI Automation) | "not supported on this OS yet (UIA only)" | same |
| `activate_window` | yes | not supported yet | not supported yet |
| Children killed on server exit | yes, also on a hard kill (job object) | on a clean exit | on a clean exit |

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

runs on any host without a desktop: key-combo parsing, the element-handle cache, argument
validation, the input safety check, and `tests/protocol.rs`, which spawns the binary and does
an MCP `initialize` plus `tools/list` over stdio, checking every tool is listed with a schema.

`tests/live_windows.rs` is an ignored test for an interactive Windows desktop. It launches the
repository's `a11y_probe` counter, lists and captures its window, reads the tree, finds and
invokes the Increment button, then clicks it, and checks each press changed the count; it also
checks that refused input is refused:

```bash
cargo build --release --example a11y_probe --features material,a11y
cargo nextest run -p flui-desktop-mcp --test live_windows --run-ignored only --no-capture
```

It moves the real pointer, and clicks, presses Tab, types one character, scrolls a notch and
drags inside the probe window, each only after the probe is confirmed in front.

## Relation to the other live checks

- `cargo xtask device windows-a11y` and `windows-input` are fixed pass/fail gates over the
  same `a11y_probe` (UI Automation, and `SendInput`); `cargo xtask device macos-*` are the
  Mac ones (`tools/device-checks`). A gate proves one scripted path; this server is for
  everything else, and `tests/live_windows.rs` exercises the server itself.
- `cargo xtask live-smoke` (`tools/live-smoke`) is the CI gate for real X11/Wayland input.
- This server is the interactive tool: an agent uses it to explore and test any app by hand,
  on the desktop in front of it.
