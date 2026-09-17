# Plan — issue #1147 (AppKit half): drain `MacOSPlatform.windows` on owner-thread close

Status: Fast path (Phase-0 triage: single obvious edit site, no design fork, no
unsafe/public-API/cross-crate ripple, no new dependency). Scout complete (2026-09-16,
this session); plan written per compaction directive before build. The issue's Win32
half is explicitly out of scope on this Mac (needs a Windows machine) — noted in §6.

## 1. Problem and resolution

`MacOSPlatform.windows: Arc<Mutex<HashMap<u64, Arc<MacOSWindow>>>>` (platform.rs:37,
`platform.rs:137-139`) pins **one `Arc<MacOSWindow>` clone per created window for the
process lifetime**. The map is inserted in `MacOSWindow::new_inner` (window.rs:363,
`windows_map.lock().insert(window_id, Arc::clone(&window))`) — the ONLY insertion — and
the ONLY removal sits inside `MacOSWindow::drop` (window.rs:962). The map's own clone
keeps `Arc::strong_count` ≥ 2 forever, so `Drop` (and its last-clone gate) can never
run; every closed window therefore leaks its wrapper, the native `NSWindow` (+1 retain
balanced only by `Drop`'s `send_release`), and the a11y adapter. No other code reads
the map for lookups (rg: only the insert, the Drop-remove, `Clone`'s shared `Arc`, and
the test helpers' manual remove) — so removing the entry changes nothing but lifetime,
which is exactly the point.

**Resolution:** remove the map entry in `handle_close` (window.rs:2023) — the
owner-thread `windowWillClose:` handler AppKit invokes for EVERY route through
`-[NSWindow close]` (vetoed or not, per `close`'s own doc at window.rs:723-730). That
is the AppKit analog of winit's `complete_window_close`, and the exact site this issue
names for the AppKit half. Placement: immediately after `self.closed.store(true,
Ordering::SeqCst)`, BEFORE `dispatch_close()`, so when the user's `on_close` callback runs
the closing window is already untracked. NOTE on the winit comparison (corrected in
review): the repo's own winit backend removes from tracking only AFTER its close callbacks
(winit/platform.rs dispatch_close before windows.remove); removal-before-callbacks is safe
on macOS only because the macOS map is private with no content readers — early removal is
observable solely as the intended lifetime change.
`Drop`'s own remove (window.rs:962) stays as the idempotent safety net (`HashMap::remove`
on an absent key returns `None`, no panic).

## 2. Design (final)

### window.rs `handle_close`
```rust
fn handle_close(&self) {
    self.closed.store(true, Ordering::SeqCst);
    // Untrack from the platform's window map NOW, on the owner thread, instead
    // of waiting for Drop (the one existing removal site, unreachable while the
    // map pins a clone) — issue #1147. Once the caller's last external handle
    // drops, the wrapper reaches Drop's last-clone gate and runs the AppKit
    // teardown tail; without this, every closed window (and its NSWindow + a11y
    // adapter) is pinned for the process lifetime. (Unlike the repo's winit
    // backend — which removes from tracking only after its close callbacks — this
    // ordering is safe because the macOS map is private with no content readers.)
    self.windows_map.lock().remove(&(self.ns_window as u64));
    self.callbacks.dispatch_close();
    tracing::debug!("Window closed");
    // ... existing callbacks.clear(), delegate nil ...
}
```

Safety/lifetime analysis (why removing the map entry here cannot dangle `self`):
- `handle_close` is reached ONLY via the delegate's `window_will_close`
  (window.rs:1762-1767), whose `get_window_from_delegate` returns an upgraded
  `Arc<MacOSWindow>` (window.rs:1880) bound by the `if let` — that Arc pins the wrapper
  across the whole `handle_close` body. Even if `dispatch_close()`'s `on_close`
  callback drops the application's last handle, `Drop` cannot run until
  `window_will_close` releases that upgraded Arc — `self` never dangles.
- The map is `parking_lot::Mutex` (window.rs:25): thread-agnostic, no reentrancy hazard;
  `handle_close` runs on the owner thread (AppKit delivers the delegate call there).
- After close: `closed` is set, the wrapper refuses further window ops
  (window.rs:844), `callbacks.clear()` (window.rs:2038) already releases the
  renderer/surface cycle — so a wrapper alive past close (until the app drops its
  handle) is the existing, guarded contract, unchanged.

## 3. Test strategy

- **New real-window test `close_drains_platform_map_entry`** (window.rs test module,
  same opt-in class as the two existing `#[ignore]`d tests): construct via
  `MacOSWindow::for_test(owner)`, assert the map contains the entry, drive `close()`
  (routes `[ns_window close]` → `windowWillClose:` → `handle_close` on the owner/
  test lane), assert the map no longer contains the entry. Directly executable proof of
  "closed windows drain the map" — the AppKit verification #1148 names for this issue.
- **Always-run carriers unchanged** — the two existing AppKit-free mechanism pins
  (`route_on_owner`/`owner_lane`) still run on every `cargo test -p flui-platform`.
- **Honest red→green limitation:** like its two siblings, the new test SIGABRTs NSWindow
  construction in a bare `cargo test` on this Mac (`_CFBundleGetValueForInfoKey`),
  so it is `#[ignore]`d with that reason. A run in an AppKit-pumping test process is
  ATTEMPTED on this real Mac during verification and the outcome reported honestly; if
  the environment cannot construct NSWindows, the executable proof is the reasoning
  trace in §1/§2 + the always-run suite staying green + this issue's own premise that
  only a real Mac resolves it.

## 4. Environment

`crates/flui-platform` links AppKit locally — builds need the lld-strip wrapper shim
(`--config 'build.rustc-wrapper="/tmp/lldstrip-wrapper.sh"'`): the committed
`.cargo/config.toml` appends `-fuse-ld=lld`, which this machine's clang 17 rejects.
`.cargo/` stays untouched (user constraint). `just runtime-conformance-check` needs the
`/tmp/py312shim` python3 symlink; `just ci` needs `/tmp/bashshim` for port-check.

## 5. Verification (expected at build close)

- `cargo clippy -p flui-platform --all-targets -- -D warnings` exit 0; `cargo fmt --check`
  clean; `cargo check` exit 0.
- `cargo test -p flui-platform` — existing always-run counts unchanged (161 passed /
  ignored-class grows by one real-window test, matching the issue's premise).
- Attempt `--run-ignored=all close_drains_platform_map_entry` (nextest) / `--ignored`
  (libtest) on this real Mac; report outcome honestly.
- Win32 half: NOT verified (no Windows machine here) — recorded as the residual the
  issue still carries; the AppKit half is the resolution a Mac can prove.

## 6. Explicitly NOT in this change

- **Win32 half of #1147** (`windows: HashMap` on the win32 backend, same insert-only /
  Drop-only-removal shape) — needs `WM_NCDESTROY`-site removal on a Windows machine.
- The a11y bridge lifecycle beyond what `handle_close`/`Drop` already do.
- The `PlatformProxy` redraw relay (#559/#551) and the wake-coverage ledger (#654) —
  different residual families.
