# ADR-0355: A Windows test runs on a desktop of its own

- **Status:** Accepted (2026-11-16)
- **Date:** 2026-11-16
- **Related:** [ADR-0243](ADR-0243-one-testing-kit-per-layer.md) (where test support lives),
  [ADR-0303](ADR-0303-a-rule-takes-the-first-rung-that-states-it-and-an-xtask-gate-is-the-last.md)
  (the place of a rule; §1, what `xtask` holds)

## Context and problem statement

flui-native's Win32 suites (`windows_window_commands`, `windows_host_events`, `windows_foreground`,
the contract's Win32 rows, the `#[cfg(test)]` modules under `src/**/windows/`, the OLE drag and
TSF tests) and the facade's `surface` test open real windows. On Windows they opened them on the
desktop the user works on: shown, some topmost, some brought to the foreground, so they flashed
over the user's work and took the keyboard focus, and the user's clicks and typing reached them
and broke cases. Several agents run these suites on one machine all day. On Linux the same suites
run on an X server or a compositor of their own (Xvfb with openbox, a headless Weston or sway);
Windows had nothing of the kind.

A Win32 desktop is a kernel object in a window station. Only one desktop of the interactive
window station takes input and is displayed (the input desktop); a window on any other is never
shown and receives no mouse or keyboard input, while activation, focus, messages, hooks, DPI,
monitors, DXGI presentation, DWM attributes, OLE's drop targets and the text services work there.
A process started with `STARTUPINFOW::lpDesktop` naming a desktop has every thread on it, and its
children inherit it. Measured on Windows 11 (build 26300) with the suites of `origin/main` at
`b1c36805f`:

| Suite | On a desktop of its own |
|-------|-------------------------|
| flui-native, all features (772 tests) | 763 pass at once. Of the 9 that do not: an Android intent test fails on the user's desktop too; the idle modal-loop case counts the activation messages of windows earlier cases left shown (§4); the 5 touchpad tests read the OS cursor, which `GetCursorPos` refuses there; `windows_foreground` and 3 contract rows need the foreground and injected input (5 more such rows already said they could not verify) |
| flui-engine readback and the facade's composited-layer readback on Direct3D 12 (133 tests) | all pass |
| flui-native in the logon session's own window station | 25 more cases fail and one test hangs (§3) |

The question: where does a Windows test run, so that nobody sees it and nobody's input reaches
it, for every way the suite is started, without code in each test?

## Decision drivers

- Every way the suite is started here starts it on that desktop: `cargo heli nextest run`,
  `cargo nextest run`, `cargo xtask test`, `platform-test`, `gpu-test`.
- No code in each test: a new test cannot be left on the user's desktop.
- A crashed run leaks nothing, and parallel tests do not each take a desktop (the desktop heap is
  a limited resource of the session).
- Nothing installed and no administrator rights; Rust in the repository.
- `cargo run` of an application stays as it is.
- What a test proves stays proved: a case that needs the user's desktop says so and runs there on
  request.

## Considered options

1. A nextest wrapper script that starts each test process on the run's desktop.
2. A Cargo target runner for Windows that does the same.
3. `cargo xtask test` starting nextest on the desktop.
4. Each test moving itself to the desktop through a fixture.

## Decision

Chosen option: 1, because nextest is the one program every way of starting the suite here goes
through, and a wrapper it applies to every test needs no code in any test.

### §1. nextest starts every Windows test through `test-desktop`

`.config/nextest.toml` names the wrapper script `test-desktop` for every test on Windows
(`filter = 'all()'`, `platform = 'cfg(windows)'`) and the setup script `build-test-desktop`, which
builds it with `cargo build -p xtask --bin test-desktop --target-dir target` before the tests
run: into the workspace root's `target/` whatever target directory the tests use, since nextest
tells neither script which one that is, and the wrapper names the program from the workspace root.
`test-desktop` (`tools/xtask/src/bin/test-desktop.rs`) opens the desktop
`flui-test-<NEXTEST_RUN_ID>` in the current window station with `CreateDesktopW`, which creates it
when no test of the run holds it and opens it otherwise, starts the test there with
`CreateProcessW`, inheriting its standard handles and environment, waits for it and exits with its
exit code, a crash's included. It ignores Ctrl+C from the moment the test has started, so it waits
for the test, which shares its console, to end; the test itself keeps Ctrl+C. Both scripts are
nextest features that are still experimental (`experimental = ["setup-scripts",
"wrapper-scripts"]`).

### §2. One desktop per run, released by the kernel

The desktop is named per run: the tests of one run share it, and two runs at once (two agents,
two worktrees) never share a desktop or the window active on it. Each `test-desktop` holds a
handle while its test runs, and the kernel destroys the desktop when the last handle closes and no
thread is attached to it, a crashed `test-desktop` included. There is no cleanup to forget and no
desktop left behind, and the run's tests take one desktop heap, not one each. A process a test
starts and leaves running keeps its thread on the desktop, and the desktop with it, until that
process ends.

### §3. The window station is the user's

The clipboard, the global atom table and the registered clipboard formats and window messages
belong to the window station, not to the desktop. A window station of the run's own would keep
the clipboard tests off the user's clipboard, but none fits:

- `CreateWindowStationW` with a name is refused without administrator rights.
- Without a name it creates the logon session's window station (`Service-0x0-<logon id>$`),
  which needs no rights but is not interactive. On it 25 flui-native cases that pass on the
  run's desktop fail (the window-mode cases of `windows_window_commands` and the conformance
  kit, the OLE cases that find a window under a point, the contract's dead-key and deadline rows,
  the touchpad's paint request), and `windows_host_events` hangs until nextest ends it.

So the test desktop is created in the caller's window station, and the clipboard tests (test group
`system-clipboard`) still write the user's clipboard: a copy the user makes while one runs can
fail it, and the user's clipboard holds what the test wrote.

### §4. What needs the user's desktop runs there on request

The foreground, the cursor and injected input (`SendInput`, a synthetic pointer device) belong to
the input desktop. A case that needs them reads whether its desktop takes input
(`GetUserObjectInformationW(UOI_IO)`, `input_desktop_for` in `crates/flui-native/tests/main.rs`)
and, where it does not, prints `CANNOT_VERIFY` with the reason and returns, or fails under
`FLUI_REQUIRE_INPUT_DESKTOP=1`, as `FLUI_REQUIRE_GPU` and `FLUI_REQUIRE_WAYLAND` turn their
skips into failures. A test that reads the cursor for a position the case does not depend on is
given one instead (the touchpad tests' `TestCursor`). OLE's drop targets, called as OLE calls
them, work on the test desktop; OLE's `DoDragDrop` loop needs the input desktop. Woken by a
mouse move posted to its capture window, the loop enters the window under the move's position on
Windows 11 build 26300 on either desktop. On Windows Server 2025 (build 26100) the case dropped
through the loop on the input desktop, the system pointer left where it was; whether that loop
reads the posted position or the pointer is not separated. On the test desktop there the loop
entered only a window covering the whole screen, whichever position, button state or coordinate
space the move carried. So the case that drops
through the loop runs on the input desktop
(`a_drag_into_a_window_of_this_process_drops_through_the_loop`). `FLUI_TEST_VISIBLE_DESKTOP=1` makes
`test-desktop` start the test on the caller's desktop, where those cases run; the nightly Windows
run sets both variables for them, since its runner has no user. `docs/testing.md` ("The Windows
test desktop") lists them.

No window of the test desktop is in the foreground. A process's other shown windows then take and
lose the activation while one of them stands in a modal loop, so a case that counts what runs on
the owner thread during an idle modal loop runs in a process of its own
(`windows_window_commands::native_idle_modal_loops`). The window the activation moves to goes
above the others of its band, so a window a case raised topmost does not stay above a topmost
window an earlier case of the process left shown, and OLE's `DoDragDrop` loop and
`WindowFromPoint` answer the earlier one for a point over both. A table of cases that asks which
window is under a point hides each case's windows as the case ends, since a dropped platform
leaves its windows open (`no_case_leaves_a_window_shown` in `drag::windows::tests`).

### Consequences

- Good, because no Win32 test window is shown to the user or reached by the user's input, for
  every nextest invocation, with no code in a test.
- Good, because two runs no longer compete for one desktop's activation.
- Good, because the cases that read the user's cursor or foreground stop failing when the user
  moves the mouse: the touchpad tests hold a cursor of their own, and the interactive cases run
  only where they can verify.
- Bad, because both nextest features are experimental: a nextest release may change their syntax,
  and the CI jobs install the newest 0.9 release.
- Bad, because the setup script runs a `cargo build` at the start of every Windows run (a no-op
  once built), and a run started while another uses `test-desktop.exe` cannot replace it after its
  source changes. A run whose tests build into another target directory builds xtask's
  dependencies once more in `target/`, unless a shared build directory (heli's) holds them.
- Bad, because a case that cannot verify off the input desktop is reported as passed; only
  `FLUI_REQUIRE_INPUT_DESKTOP` and its `CANNOT_VERIFY` line tell it apart.
- Bad, because plain `cargo test` still starts its tests on the caller's desktop.
- Bad, because the clipboard is still shared with the user (§3).

### Confirmation

- `a_window_of_a_test_is_on_the_test_desktop_and_not_on_the_users` (`tools/xtask/tests/test_desktop.rs`)
  starts a process through `test-desktop` that shows a window, and finds the window among the run
  desktop's windows and not among the input desktop's (`EnumDesktopWindows`), from another process.
- `windows_test_desktop::nextest_starts_a_test_on_the_runs_own_desktop` (flui-native's
  `platform_it`) reads, under nextest, its own thread's desktop: `flui-test-<run id>`, taking no
  input, or not that desktop under `FLUI_TEST_VISIBLE_DESKTOP=1`. Without the scripts of §1 it
  fails.
- `a_command_line_parses_back_into_its_arguments` (in `test-desktop.rs`) pins the quoting that
  hands the test its arguments.
- The nightly Windows run (`nightly-platforms.yml`) runs `platform-test` and `gpu-test` on the run's
  desktop and the interactive cases with `FLUI_TEST_VISIBLE_DESKTOP=1` and
  `FLUI_REQUIRE_INPUT_DESKTOP=1`.

## Pros and cons of the options

### 1. A nextest wrapper script

- Good, because `cargo heli nextest run`, `cargo nextest run` and every `xtask` task read the same
  `.config/nextest.toml`.
- Good, because a filter of `all()` leaves no test out and touches no test's code.
- Bad, because wrapper and setup scripts are experimental nextest features.
- Bad, because the wrapper is a program that has to exist before the tests run: a setup script
  builds it.

### 2. A Cargo target runner

- Good, because `cargo test` would go through it too.
- Bad, because `target.<triple>.runner` also runs every `cargo run` of an application, which would
  open the application on a desktop nobody sees.

### 3. `cargo xtask test` starting nextest on the desktop

- Good, because it needs no nextest feature: every process nextest starts inherits the desktop.
- Bad, because a direct `cargo nextest run` or `cargo heli nextest run`, the way most runs start
  here, would stay on the user's desktop.

### 4. A fixture in each test

- Good, because it needs no wrapper.
- Bad, because libtest runs the body on a thread of its own and `SetThreadDesktop` moves only that
  thread; a thread the test or a library starts, and a child process, take the process's desktop.
  Moving the process means starting it again, which only an entry point every test calls can do,
  and the next test can forget it.

## More information

- nextest wrapper scripts: <https://nexte.st/docs/configuration/wrapper-scripts/>; setup scripts:
  <https://nexte.st/docs/configuration/setup-scripts/>.
- Desktops and window stations:
  <https://learn.microsoft.com/windows/win32/winstation/desktops>,
  `STARTUPINFOW::lpDesktop`:
  <https://learn.microsoft.com/windows/win32/api/processthreadsapi/ns-processthreadsapi-startupinfow>,
  `UOI_IO`: <https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-getuserobjectinformationw>.
- Revisit §1 when nextest stabilizes or changes the scripts, and §3 if the clipboard tests move to
  a clipboard of their own.
