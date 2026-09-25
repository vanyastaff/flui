# Desktop MCP safety audit

Reviewed PR [#1287](https://github.com/vanyastaff/flui/pull/1287) at
`d7e728daa6dca68b162cbf825f597dc2ff10fcf7`, including 287 inline review threads
(four unresolved at that head). The review covered input sequencing and OS guards,
accessibility traversal and actions, handle lifetimes, capture coordinates, worker
cancellation, process cleanup, MCP validation and wait deadlines. Changes concern
the desktop tool, this report and inheritance of the unchanged `core-graphics` version
by `flui-platform`; no framework behavior, CI workflow or publishing changes.

## Repaired failure scenarios

Text-only MCP clients receive the same JSON error envelope, including retry policy and
partial effects, as structured-content clients. Handle-only queries can match a verified
node despite unrelated incomplete search properties; unread state predicates still fail.
Subtree traversal reserves part of the request for final root validation so a normal
budget-limited read can return its truncated result. A provider that consumes this reserve
still produces a validation timeout rather than unchecked data.

Tree roots use window identities recorded before provider traversal and checked again
after enrichment. Screenshot handles are permanently retired after an observed source
identity or geometry change, while unreadable observations remain transient. Unix clean
shutdown waits for in-flight launches to settle; it can wait indefinitely for a stuck OS
spawn. A regression creates a real child and holds its spawn return beyond the cleanup
deadline, then verifies cleanup waited and reaped it. Windows retains bounded waiting
backed by its creation-time Job containment.
`cargo xtask check-changed --base 854c89d1bf76dfa7ec233b1db6f06b2394cee19c`
passed: 152 tests, six skipped, formatting, strict rustdoc and Windows/macOS clippy.
`cargo nextest run -p flui-desktop-mcp --test native_windows --run-ignored only
--no-capture` passed all three entries. The blocked-spawn regression runs the Unix
waiting policy with a real Windows child on this host; a real blocked Unix exec was
not run. Subtree deadline reservation and identity races use deterministic helpers,
not a deliberately delayed live UIA provider.

Subtree reads revalidate their original root after traversal, including truncated
results. A confirmed replacement retires handles touched by that walk; an expired
deadline refuses the result instead of starting another provider call. Nodes whose
held identity cannot support actions no longer advertise them. Unknown enabledness
omits `disabled` while retaining the unread-state marker used by wait predicates.
`flui-platform` inherits the existing workspace `core-graphics` requirement without
changing its version; `cargo metadata --locked --no-deps --format-version 1` passed.
The native fixture now checks a checkbox click through a deliberately downscaled
200-pixel screenshot and records the received main virtual key and modifier state.
`native_controls_through_mcp` passed before mutation. Removing screenshot scale division
made the checkbox assertion fail; separately removing implied modifiers produced
`vk=187 modifiers=2` instead of `vk=187 modifiers=3` for `ctrl+plus`, and failed the
target-side assertion. Both production mutations were restored. The fixture derives
its expected shifted punctuation from its own keyboard layout; this live run used
the plus mapping. These checks exercise the actual MCP input path.
`cargo xtask check-changed --base a40ccbc37210b5c2527c8272ba659c80e092046b`
passed the platform-dependent scope: 7,645 tests, six skipped, clippy, strict rustdoc,
doctests, desktop Windows/macOS checks and both 53-configuration feature passes.
Linux's platform suite, iOS and wasm remain CI coverage on this Windows host.
After restoring both mutations, `cargo nextest run -p flui-desktop-mcp --test
native_windows --run-ignored only --no-capture` passed all three entries, including
the strengthened controls scenario and the external drag-release scenario.

Monitor screenshots retain the native display identifier from the captured monitor,
alongside its geometry. Pixel-coordinate validation looks up that identifier rather
than reselecting the current primary monitor or enumeration index. Native display IDs
are not a hardware lifetime guarantee: unobserved reuse of the same native identifier
and geometry remains outside this check. Direct element focus requires a readable live
focusable flag before dispatch; unsupported elements receive a clean refusal without
an uncertain action effect.
`cargo xtask check-changed --base 2b0db0919728f82ded1cbd9daaa86dcffaf16a1e`
passed: 142 tests, six skipped, formatting, strict rustdoc and Windows/macOS clippy.
`cargo nextest run -p flui-desktop-mcp --test native_windows --run-ignored only
--no-capture` passed all three entries. The native static landmark rejects direct focus
with `action_unsupported`, no effect, and the edit retains focus. Display replacement
and enumeration reordering are injected tests; no physical display hotplug was run.

PID activation and capture retain the selected window's adopted class rather than
reading it again after adoption; they also retain the original process start time.
This prevents a replacement window from being accepted under a retired identity when
the intervening class lookup would have failed. Click accounting counts only completed
press/release pairs: a failed release preserves the current click's uncertainty without
adding it to the completed count. These failure paths use injected regression checks;
ordinary native smoke does not force OS release failures or native-handle reuse.
`cargo xtask check-changed --base 5d17eb044e29a73a4872216ff07b1041e8997d60`
passed: 139 tests, six skipped, formatting, strict rustdoc, and Windows/macOS clippy.
The native Windows suite above was rerun with `--run-ignored only --no-capture`;
all three entries passed, including ordinary controls and drag interruption.

Action readback now checks process identity after the last provider call, including
failed readback. Drag guards require the owned button to remain down; only initial
press delivery receives a bounded settling period. A PID bound to an earlier process
is reported as untargetable after reuse. Losing a selected window remains a transient
error for a live PID target, preserving any action effects; failed window enumeration
also rechecks whether the process itself has exited.

These changes passed `cargo xtask check-changed --base
e61be9e2444c32e60a9d5fefc92ed7e0c12e7603`: 136 tests passed, six skipped,
with formatting, clippy, strict rustdoc and Windows/macOS target checks passing.
`cargo nextest run -p flui-desktop-mcp --test native_windows --run-ignored only
--test-threads 1 --no-capture` passed all three entries (two live scenarios and the
fixture helper). The new native fixture releases the primary button after movement;
the MCP drag reports partial interruption and an independent cursor read confirms it
does not reach the endpoint. The ordinary native controls scenario also passed.
Process replacement during provider readback is covered by an injected production
helper test, not a live PID-reuse race.

| Scenario | Repair and evidence |
|---|---|
| A queued window lookup starts after its deadline because of a minimum 50 ms grace | Check the deadline on the worker thread and withdraw queued reads. `expired_window_lookup_never_starts` verifies no read starts. [Original comment](https://github.com/vanyastaff/flui/pull/1287#discussion_r4097158852). |
| Direct `set_value` reaches a read-only provider despite the action being omitted from discovery | Check readable Value/RangeValue read-only flags before dispatch; use a writable range when the text pattern is read-only. Fake-writer tests panic on an unauthorized dispatch; the native fixture verifies an actual read-only edit remains unchanged. [Original comment](https://github.com/vanyastaff/flui/pull/1287#discussion_r4097158861). |
| A failed key press is counted as definitely sent | Count only confirmed presses, always attempt release, and retain `may_have_run` for ambiguous delivery. Failure-injection tests distinguish failed press from failed release, including CRLF. [Original comment](https://github.com/vanyastaff/flui/pull/1287#discussion_r4097158867). |
| A window closes during failed capture and reports `not_found`, or an observed-gone window later revives | Recheck identity after failed reads too, and permanently retire observed-gone window handles. Activation and tree ownership failures use the same retirement path. A backend error alone does not retire a still-live window. Two injected capture tests cover this distinction. [Original comment](https://github.com/vanyastaff/flui/pull/1287#discussion_r4097158875). |
| A UIA proxy follows a replacement after its old handle was observed gone | Retire both identity and stored proxy mappings, and invalidate handles on observed-gone UIA paths. Cache tests verify an old handle remains gone after identity reuse. |
| `wait_for` mistakes an unread focus/enabled flag for `false`, or matches a clipping marker as actual text | Track unread state properties separately and require the requested observation. `state_predicates_require_the_requested_observation` covers both cases. |
| An explicit never-issued element query reports success for `gone`, or later matches a newly issued ID | Validate explicit element handles before tree traversal; canonicalize accepted padded handles before matching. Query tests and the native MCP fixture cover these cases. |
| Cancellation fires but the async runtime has not polled it when the desktop thread claims a queued action | Inspect the token on the desktop thread before claiming work. The regression deliberately prevents runtime polling until the worker passes the cancelled action. |
| A panic follows a side effect but the reply suggests an ordinary failure safe to repeat | Desktop and blocking process work report `may_have_run`. Panic-injection tests also verify the worker continues serving. |
| macOS advertises an unidentifiable process as targetable, or targetless movement drags a physically held mouse button | Report `targetable: false` without process identity; refuse movement where the physical-button check is unavailable. macOS runtime behavior remains unverified on this Windows host. |
| Windows GDI capture crops after resizing, but the reply treats the result as a scaled image of the full window | Select xcap's WGC backend on Windows and reject raw dimensions that disagree with the physical source rect. A 466×313 source previously returned 459×311 pixels. Live assertions must check dimensions and independently located pixel landmarks, not just a large button's centre. |

The repeated suggestion to invert wheel values was not applied: enigo 0.6.1 already
converts positive vertical notches to negative Windows wheel deltas. The native fixture
observes `dy: 1` as `WM_MOUSEWHEEL` delta `-120`.

## Verification

The first regression experiment disabled six production fixes (read-only checks, cache
retirement, unread-state checks, confirmed-press accounting, worker-side cancellation,
and identity rechecking after a failed read). The selected eight tests all failed;
the source files were restored immediately afterwards. A second experiment disabled
the worker deadline and raw capture-size checks: both corresponding tests failed. A
live experiment shifted only the screenshot's declared origin by four pixels: the
landmark test failed on a white pixel where the dark marker should be. All mutations
were restored. Color comparisons allow a small compositor conversion tolerance, while
requiring the marker/background contrast to exceed both tolerances.

Two non-interactive integration tests launch a real child through stdio MCP and observe
its socket lifetime: stdin EOF ends the child, and on Windows hard termination of the
server ends the child through its job object. Their fixture also exits when the observing
socket closes, so failed assertions do not strand it.

The Win32 live fixture asserts actual text replacement, Backspace, Tab focus, read-only
refusal, checkbox transitions, wheel direction and drag displacement. It explicitly
reactivates its top-level window after the semantic toggle. It retries only `busy` with
no reported effect, at most three times; uncertain or partial actions are never retried.
The FLUI live test checks the counter's semantic value and changed pixels after invoke
and physical clicks. Earlier attempts stopped on foreground/cursor guards; both scenarios
passed once the shared desktop was idle. The WGC runs then passed with a 466×313 bitmap
for a 466×313 FLUI source, and the independent native marker's eight edge probes passed.

Commands and results on Windows:

```text
cargo xtask check-changed --base d7e728daa6dca68b162cbf825f597dc2ff10fcf7
  fmt, clippy all targets, nextest: 117 passed / 4 ignored,
  strict private rustdoc, explicit Windows-target clippy, cargo-hack: passed
  macOS target not installed: skipped explicitly by the gate
cargo nextest run -p flui-desktop-mcp --test native_windows --run-ignored only native_controls_through_mcp --no-capture
  passed, including independent pixel edges
FLUI_A11Y_PROBE=<existing release probe> live_windows-<hash>.exe --ignored --nocapture
  a11y_probe_counter_through_mcp: passed against this checkout's server
target/debug/xtask.exe docs-links
  230 markdown files; 0 errors (archival roots excluded)
git diff --check
  passed
```

The initial shared target was refused by `check-changed`; final validation uses this
checkout's own target. The default gate against `origin/main` selected the entire
workspace because of the original PR's changes and was stopped in favour of the pinned
PR head above: this gate validates the audit fixes, not a fresh full-workspace CI run.
The FLUI probe executable was prebuilt; that live run did not rebuild the framework.

## Remaining limits

Action/readback boundary changes passed
`cargo xtask check-changed --base 8b469ffc5a008bedd1f65c55ded1d558b5bdee50`:
130 tests passed, five were skipped, with strict docs and Windows/macOS clippy passing.
Regression checks cover extra physical buttons at a stationary drag drop, permanent
unidentified-PID metadata, process replacement during the final element refresh, and
expand/collapse transition eligibility shared between action descriptions and dispatch.
The Windows `native_controls_through_mcp` test now starts its fixture minimized and
passed restoration through MCP, asserting refreshed focus, minimization, geometry and
the retained session handle. Extra-button interleaving and provider replacement were
tested deterministically through production helpers, not by racing a live provider.

The later native-boundary changes passed the full workspace scope selected by
`cargo xtask check-changed --base 84d074534866584b68244fda778d3d6a12d07f63`:
10,115 tests passed, five were skipped, and workspace clippy, strict rustdoc, doctests,
Windows and macOS target checks passed. Android, iOS and wasm checks unavailable on this
host, and the Linux-only platform suite, remain CI coverage. `cargo xtask checks` also
passed. This rebuilt the workspace in the audit worktree's local target with one Cargo
build job; it does not substitute for executing the macOS backend.

Native input checks now bracket potentially slow target guards for clicks, initial drag
presses, scroll axes, text strokes, modifier presses and the main chord key. The final
snapshot never waits for physical input to clear. The ignored Windows tests
`native_controls_through_mcp` and `physical_state_change_inside_guard_blocks_event` both
passed: the latter sends a temporary Ctrl during the guard, observes it through Windows
key state, and proves the following event is refused. It does not emulate a slow UIA
provider or send a click while Ctrl is held.

Activation checks target identity after both success and failure. macOS identity checks
use a complete Quartz owner snapshot instead of xcap's screen-only list, preserving
hidden windows and propagating failed snapshots without retiring handles. The macOS
code and parser tests passed `cargo clippy -p flui-desktop-mcp --locked --all-targets
--target aarch64-apple-darwin -- -D warnings`; the native Quartz path has not been run.

Additional boundary checks were validated with
`cargo xtask check-changed --base f77de34d58fa8533e38b89753722525f9ed4052c`:
121 tests passed, four ignored fixtures were skipped; Windows and macOS target clippy,
formatting and strict rustdoc passed. Regression tests cover permanently retiring a
displaced HWND handle across X/Y/X class reuse, partial UIA expansion matching neither
completed boolean state, and refusing drag movement when a physical key appears during
the target guard. Worker cancellation is read after claiming the job, retaining the
existing queued-cancellation tests. No live physical-key interleaving was run; drag
cleanup must release its own button and cannot guarantee rollback or an unmodified drop.

The reported empty-window screenshot panic is not reachable through the current call
path: `Desktop::resolve` returns `no_window` for an empty result, and screenshot propagates
that error before selecting from the immutable returned vector.

- **Native allocation and execution isolation is not implemented.** UI Automation can
  allocate full BSTRs, runtime-ID arrays and cached native properties before Rust's
  truncation/budgets run. A huge or malicious provider can still exhaust this process;
  a hung native call cannot be forcibly cancelled on the worker thread. The same
  preflight/allocation race exists when a window resizes during xcap capture.
  [#1289](https://github.com/vanyastaff/flui/issues/1289) tracks isolation. A robust follow-up
  needs a memory-limited child, bounded IPC, retired handles on child restart and no
  automatic replay of an uncertain action. The README no longer claims these in-process
  limits contain hostile providers.
- Input checks and delivery are separate OS operations. Another program or the user can
  change the desktop between them. Shell-hotkey refusals do not detect arbitrary global
  hotkeys installed by other programs. These are explicit limits of external desktop input.
- Windows WGC requires the OS capture API and a usable D3D device. xcap waits up to three
  seconds for a frame; capture failures remain tool errors. This is not a fallback to the
  GDI path with incorrect coordinates.
- No macOS/Linux interactive run, hosted cross-process control run, malicious-provider
  stress test, or proof against every possible OS scheduling interleaving was performed.
  The native fixture closes meaningful gaps in [#1290](https://github.com/vanyastaff/flui/issues/1290),
  but does not replace the broader injected-backend work in
  [#1294](https://github.com/vanyastaff/flui/issues/1294).

This is evidence for the repaired scenarios, not a claim that no further defects exist
or that the tool safely contains an untrusted native provider.
