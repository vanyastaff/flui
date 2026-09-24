# flui-desktop-mcp

An [MCP](https://modelcontextprotocol.io) server that lets an agent (Claude Code, Codex, or
any MCP client) see and drive desktop applications the way a person with a screen reader
does: list and capture windows, read the accessibility tree, perform element actions, and
send real mouse and keyboard input. It works on any application, not only FLUI ones, and it
observes an app strictly from the outside, through the OS accessibility, capture and input
APIs.

Its wire contract (handles, vocabulary, replies, errors) is the agent protocol of
[ADR-0080](../../docs/adr/ADR-0080-agent-protocol-desktop-contract.md): backend-neutral, so
the in-process FLUI backend planned for `flui mcp` speaks the same one.

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

One server per desktop: it shares the one pointer and keyboard with the person at it, and a
call the client timed out on still runs (Codex's default tool timeout is 60 s; `wait_for`
and `wait_for_window` accept up to 120 s).

On macOS the server needs two permissions for the app that runs it (the terminal, or the MCP
client): Accessibility, for input (enigo asks for it at startup; input stays off until the
server restarts after it is granted), and Screen Recording, without which `list_windows` sees
almost no windows.

## Tools

Coordinates are screen coordinates everywhere (window rects, element rects, input): physical
pixels on Windows, where the server is per-monitor DPI aware, and points on macOS. A
screenshot reports how its pixels map back to them, and its id can be passed with image
pixels instead.

| Tool | Arguments | Does |
|------|-----------|------|
| `list_windows` | `title_contains?`, `pid?` | Top-level windows, front to back: `id` (`w3`), `pid`, `app_name`, `title`, `rect`, `is_minimized`, `is_focused`, `targetable` and `untargetable_reason` |
| `launch` | `program`, `args?`, `cwd?`, `env?` | Starts a process; returns its `pid` and whether it is `bound` as a target. Command line and environment at most 32 KiB; a program still starting after 30 s is given up on (and ended if it starts) |
| `wait_for_window` | `pid`, `title_contains?`, `timeout_ms?` | The process's first matching window, with its handle; `gone` if the process exits first (Windows only) |
| `kill` | `pid` | Ends a process this session launched (other pids are refused); `already_exited`, `exited.code` |
| `screenshot` | `window?` \| `pid?` \| `monitor?`, `max_side?` | PNG of a window (captured even when covered, where the OS allows), a monitor, or the primary monitor, its longer side at most `max_side` (default 1920, at most 4096); plus, as text, `id` (`s2`), size, the captured rect `source` and `scale_x`/`scale_y` |
| `accessibility_tree` | `window` \| `pid` \| `root`, `max_depth?`, `max_nodes?`, `format?` | The element trees, as an outline (default) or as nodes (`format: json`); `count`, `truncated` |
| `find` | `window` \| `pid` \| `root`, `name?`, `name_contains?`, `role?`, `automation_id?`, `limit?`, `format?` | Matching elements, flat; `count`, `total`, `truncated` |
| `wait_for` | as `find`, plus `element?`, `state?`, `gone?`, `timeout_ms?` | The first element matching the criteria and in `state`, or with `gone` the moment none matches; a timeout error otherwise |
| `invoke` / `toggle` / `select` / `focus` / `expand` / `collapse` / `scroll_into_view` | `element` | The accessibility action, with no pointer involved; returns the element afterwards, or `readback_failed` |
| `set_value` | `element`, `value` | The text (at most 100000 characters), or a number for a slider |
| `click` | `element` \| `x`,`y` \| `screenshot`,`x`,`y`; `button?`; `double?`; `window` \| `pid` | A real click; buttons are logical (a swapped mouse is honoured) |
| `move_mouse` | `element` \| `x`,`y` \| `screenshot`,`x`,`y` | Moves the pointer; reports where it ended up |
| `drag` | `from`, `to` (each a location object), `duration_ms?`, `window` \| `pid` | Left-button drag, at most 10 s |
| `scroll` | a location, `dx?`, `dy?`, `window` \| `pid` | Wheel notches at a point, at most 100 each; positive is right/down |
| `type_text` | `text`, `window` \| `pid` | Types up to 10000 characters into the focused control; a newline is Enter, a tab is Tab |
| `key` | `combo`, `repeat?`, `window` \| `pid` | A key or combo: `enter`, `tab`, `f5`, `ctrl+shift+s`, `alt+f4`, `cmd+q`, `ctrl+plus`; on Windows a character that needs Shift or AltGr on the layout gets it |
| `activate_window` | `window` \| `pid` | Brings a window to the front; reports `became_foreground` and the `foreground` window (Windows only for now) |

Every tool but `screenshot` publishes an output schema and replies with structured content
(the same JSON goes in a text block). `screenshot` replies with an image and a text block
only: the clients that receive structured content drop the image. Every tool carries a
title and annotations; the six readers (`list_windows`, `wait_for_window`, `screenshot`,
`accessibility_tree`, `find`, `wait_for`) are `readOnlyHint`.

### Handles

Windows (`w3`), elements (`e12`) and screenshots (`s2`) are session handles. The same
window or element keeps its handle across reads; a handle whose window, element or process
is gone answers `gone` and is never re-bound, even when the OS reuses the native id (which
then gets a fresh handle). A pid is the OS number, bound to the start time of the process it
named when this session first handed it out (listed or launched), and refused once that
process is gone. At most 20000 element handles stay resolvable; the last 8 screenshots.

### Elements

A node: `id`, `role`, `native_role`, `name`, `value`, `automation_id`, `class_name`,
`rect`, `disabled`, `focused`, `focusable`, `checked` (`true`/`false`/`"mixed"`),
`expanded`, `selected`, `actions`, `window` (on roots), `children`, `omitted_children`,
`gone`. A flag at its default is left out. `role` is an AccessKit role name (`button`,
`check_box`, `text_input`, `list_item`, `menu_item`, `tab`, `window`, ...; `unknown` when
the vocabulary has no name) and `native_role` what the OS calls it; `find`'s `role` matches
either. `actions` lists the tools that act on the element.

The outline is one line per element:

```text
- window "Counter" [ref=e1] [window=w3] [focused]
  - label "0" [ref=e2]
  - button "Increment" [ref=e3] [actions=invoke]
  - check_box "Bold" [ref=e4] [checked] [actions=toggle,focus]
  - … 12 more children not read
```

### Reads

A read (`accessibility_tree`, `find`, one `wait_for` poll) fetches at most 5000 elements
(500 reported by `accessibility_tree` unless `max_nodes` says more; read a subtree with
`root`) and 16 MiB of strings, cuts any one string at 4096 characters (ending in `…`), and
starts no provider call more than 10 s after it began (a call in flight can take up to 5 s
more), so a huge, hostile or hung tree cannot hold the server. A reply that left anything
out (a budget, the depth, a provider failing partway, a clipped or unreadable property)
says `truncated: true`, and an empty `find` is then no proof the element is absent. A
process's reads include its popup menus and drop-downs, which are windows of their own,
each root marked with its window handle; an element that shows up under two windows (an
owned dialog) is reported once.

### Errors

A failing call returns a tool error (`isError: true`) whose structured content is
`{"error": {"code", "message", "retry", "effect"?, ...}}`:

- `code`, fixed: `invalid_argument`, `not_supported`, `busy`, `not_found`,
  `unknown_handle` (with `handle`, `kind`), `gone` (`handle`, `kind`, `why`), `disabled`,
  `action_unsupported` (`supported`, `unread`), `not_foreground` (`target`, `foreground`),
  `focus_elsewhere` (`holder`), `outside_target` (`x`, `y`, `reason`, `covered_by`),
  `timeout` (`timeout_ms`, `what`, `summary`), `input_held`, `shutting_down`, `platform`.
- `retry`: `never`, `soon` (something passing got in the way: a full queue, a window that
  moved during a capture, a layout that changed under a key) or `when_appears` (nothing
  matches now). `wait_for` and `wait_for_window` keep polling through `soon`.
- `effect`, when part of the action already went out: `partial` (`sent` of `total`
  `unit`: characters, presses, clicks, scroll axes, drag steps), `may_have_run` (the
  action reached the application and then failed or timed out), `ran` (the action went
  out; what followed failed), `incidental` (something else went out: modifiers tapped on
  their own, input held from an earlier call released now). Look before retrying any of
  them.

Arguments that do not parse are `invalid_argument` too, naming the field. An action that
ran but whose element could not be read back afterwards is a success with
`readback_failed`, so a retry does not repeat it.

## Safety rule for input

`click`, `drag`, `scroll`, `type_text` and `key` require a `window` or `pid`. The server
checks that the target owns the foreground window and that at every coordinate the OS
reports the target — a `window` admits only that window at the point, a `pid` any of the
process's windows (its own popups) — with the deepest window there, the one that takes the
click, in the same process (a preview pane another process hosts is refused). The check runs
again before **every** event of a multi-event action — each repeat of `key`, each modifier
press, each character of `type_text`, each step of `drag` and its release, each click of a
double click — so a window that takes the foreground partway through receives none of the
rest, apart from what must go out regardless: the release of what was already held, and an
inert masking key before modifiers come up on their own. Otherwise it refuses and sends
nothing.

- A window handle is bound to the window it was issued for: its native id, its owner, the
  owner's start time and the window's class; a pid to the start time of the process it named
  when listed or launched. OSes recycle native ids and pids, and a target that now names
  something else is `gone`, before every event. A pid this session never handed out is
  `unknown_handle`; a process whose start time the OS cannot report (macOS for now; on
  Windows a protected process) is not a safety target, and `list_windows` says so with
  `targetable: false`.
- Keys and text also require the window holding keyboard focus inside the target to belong
  to the target's process: an embedded browser or preview pane of another process with focus
  refuses them (`focus_elsewhere`), with advice to move focus to one of the target's own
  controls. When such a panel fills the window (an app that is one web view), keys cannot
  reach it with a safety target. Where the OS cannot say which window has focus (macOS, for
  now), keys and text are refused.
- A drag's start, end and every point between are checked before the button goes down, and
  each step again before it is reached. A drag that stops partway releases the button (the
  drop) at the last point verified inside the target; if none verifies, the release still
  has to go out (a held button would drag on), and the error says where it happened.
- A button or key still held from a release that failed is released before the next input,
  or that input is refused (`input_held`); a call that panics mid-action releases everything
  it held.
- An element click requires the element itself (or a descendant) to be what UI Automation
  hit-tests at the point, the element's own top-level window to be under it and its
  application in front, target or not. A popup menu is a window of its own, never the
  foreground one, so target it with `pid`.
- Shell hotkeys reach the shell, not the window in front, so `key` refuses them: on Windows
  the Windows key, `alt+tab`, `alt+esc`, `ctrl+esc`, `ctrl+shift+esc`, `ctrl+alt+delete`,
  the input-language switch (`alt+shift`, `ctrl+shift`), Caps Lock (a state every window
  shares) and a lone Shift (five taps, counted across calls, open Sticky Keys); on macOS
  `cmd+tab`, `cmd+space`, `ctrl+space`, Force Quit, Lock Screen, Log Out, Mission Control and
  Spaces, the screenshot tool, the Dock and keyboard-navigation shortcuts, a lone Shift and
  a lone Option (Mouse Keys). Other applications' own global hotkeys are not detected.
- The pointer position is read back before every press: a move the OS clamped, or a pointer
  someone else moved, refuses the press (`busy`: a retry can succeed).

The check fails closed: where the OS cannot say which window is under a point or holds
keyboard focus (macOS, for now), input with a target is refused rather than sent unverified.
What it cannot close is the gap between the last check and the event itself, a few
milliseconds, and the person at the desk typing or moving the mouse at the same time.

The element actions (`invoke`, `toggle`, `set_value`, `select`, `focus`, `expand`,
`collapse`, `scroll_into_view`) send no input at all and work on covered windows; prefer
them where the element offers the action.

Windows restricts which process may take the foreground. `activate_window` restores and
raises the window and falls back to UI Automation focus; check `became_foreground` before
sending input.

## Processes

The server kills every process it launched when it exits: on Windows also everything those
processes start directly, even when the server itself is killed (they run in a kill-on-close
job; `launch` refuses without it); work handed to an already running instance, the shell or
a COM server escapes it. Elsewhere only the launched processes, on a clean exit. `kill` ends
the launched process itself. A launch the client cancels before the reply is built ends its
process, since the pid is never delivered.

## Platform support

| | Windows | macOS | Linux |
|---|---|---|---|
| `list_windows`, `screenshot` | yes (xcap) | built (xcap); type-checked in CI (clippy), never run | not yet |
| Input (`click`, `key`, …) | yes (enigo; pointer moves via `SetCursorPos`) | built (enigo); refused for now with a target, which every input tool requires; type-checked in CI, never run | not yet |
| Accessibility tools | yes (UI Automation) | "not supported on this OS yet (UIA only)" | same |
| `activate_window`, `wait_for_window` | yes | not supported yet | not supported yet |

On Linux the server builds, starts and lists its tools, and every desktop tool reports that the
OS is not supported yet: xcap links PipeWire and XCB there and enigo links libxkbcommon, system
libraries the workspace's Linux builds (CI included) do not install. Enabling them is a
manifest change plus those packages.

The accessibility layer is one trait (`AccessibilityBackend` in `src/a11y/mod.rs`) and one
vocabulary (`src/a11y/role.rs`); a macOS backend over AX (`objc2-application-services`) or
a Linux one over AT-SPI (`atspi`) is a new module behind it, mapping its roles onto the
vocabulary.

All UI Automation and input work runs on one dedicated thread, in arrival order: UIA objects
belong to that thread's COM apartment, and synthesized input must not interleave. At most 32
calls wait for it at once (`busy` past that).

## Tests

```bash
cargo nextest run -p flui-desktop-mcp
```

runs on any host without a desktop: key-combo parsing and shell-hotkey detection, the
keystrokes text becomes, the element-handle cache, argument validation, the input safety
checks, the binding refusals of a whole `Desktop`, the error envelope and output schemas,
cancellation on the desktop thread, process bookkeeping, and `tests/protocol.rs`, which
spawns the binary and speaks MCP over stdio: every tool is listed with its schemas, title
and annotations, and refusals come back as tool errors with a code.

`tests/live_windows.rs` is an ignored test for an interactive Windows desktop. It launches the
repository's `a11y_probe` counter, waits for its window, lists and captures it, reads the
tree, finds and invokes the Increment button, clicks it by element and by screenshot pixel,
and checks each press changed the count; then it presses Tab and `ctrl+plus`, types, scrolls
and drags, checks that refused input is refused with the right code, kills the probe and
waits for its window to be gone. Real input is the point, so when Windows will not give the
probe the foreground it fails as inconclusive instead of passing without it:

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
