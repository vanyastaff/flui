# Device checks

End-to-end checks that need real Apple hardware or a booted iOS Simulator:
a window on screen, the accessibility API, XCUITest, `xcrun simctl`. Each
script builds or receives an example binary, drives it, and exits 0 (pass),
1 (fail) or 2 (cannot verify on this host).

Most run through `cargo xtask device <check>`, which builds the probe first;
`cargo xtask device --help` lists them and their arguments, and on another host
each prints a skip message and exits 0 (`macos-hot-reload-loop` refuses with
exit 1). The rest take a probe you build yourself and run directly with
`python3` on a Mac: `check-ios-execution.py`, `check-macos-exit.py`,
`check-macos-reopen.py`, `check-owner-wake.py` and `check-resident-reopen.py`.
`check-macos-exit.py` and `check-macos-reopen.py` take the probe executable
and optionally the case names; the other three print their options under
`--help`. `crates/flui-platform/ARCHITECTURE.md` and `docs/BETA.md` record
what each proves.

The drivers are Python + Swift rather than Rust because they can only be run,
and so only be ported with evidence, on a Mac. Everything that runs on any
host lives in `tools/xtask`, and so do the Windows checks
(`cargo xtask device windows-a11y`, `windows-input`): they are Win32 API clients xtask calls
in-process, with no driver script.
