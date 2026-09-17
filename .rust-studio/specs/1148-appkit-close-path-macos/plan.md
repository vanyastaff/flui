# Plan: make the AppKit close/teardown path executable on a real Mac (#1148, AppKit half)

Reviewed twice before approval (full gate): `tooling-lead` (workspace topology)
and `harsh-critic` (adversarial coverage). Both returned RESHAPE NEEDED; both
sets of findings are folded in below — the decisive one being that a
`tools/` member is the wrong home and that my probe's original four
assertions passed even when the #1147 `windows_map.remove` was deleted
(vacuously green on the process-lifetime leak).

## 1. Problem restated — acceptance criteria (observable)

**Issue #1148** names the AppKit close/teardown obligations as "clippy-clean
under `cross-typecheck`, never executed" (ADR-0063 decision 5). The teardown
is already implemented and routed (#1043/#1145, #1147, #1194) — the gap is
**executable coverage on a real macOS host**. Nothing automated executes
`MacOSWindow`'s real close route today.

Acceptance, from outside (public surface only):

- GIVEN a real macOS host with an active GUI session AND the probe example
  staged into a minimal `.app` bundle AND `just macos-close-path` invoked,
  WHEN the probe (running on the AppKit main thread as `fn main`) calls
  `current_platform()` → `open_window()` → registers `on_close` +
  `on_should_close` → `PlatformWindow::close()` → drops wrapper and platform,
  THEN every checked obligation logs its own PASS/FAIL marker and the process
  exits 0 if and only if all pass:
  1. `current_platform()` returns a platform named `macOS (AppKit)`.
  2. `window_handle()` returns `Ok` before close — liveness sanity on a real
     constructed window.
  3. `window_handle()` returns `HandleError::Unavailable` after close — not a
     stale/dangling handle. **Sensitive to `handle_close`'s `closed` flag**:
     the documented mutation gate (`closed.store` removed) fails here.
  4. The `on_close` callback registered before close fires during the close
     route — `windowWillClose:` → `handle_close` ordering (sets `closed`,
     runs `dispatch_close`, clears callbacks, nils the delegate). Sensitive
     to the dispatch/clear ordering.
  5. The `on_should_close` callback is **NOT consulted** during programmatic
     close — pins the documented contract (traits/window.rs:357: `close()`
     "bypasses the should-close veto (on_should_close)"). A regression that
     rewires `close()` to `performClose:` (honouring a veto) fails here.
  6. Dropping the last wrapper after close does not over-release the NSWindow
     **immediately in `Drop`** (the exact class `setReleasedWhenClosed: NO`
     at construction prevents): the process stays alive and exits 0; an
     immediate over-release would abort via SIGABRT (nonzero exit).

**Red gate (fails first):** today these obligations have ZERO executable
coverage on macOS — the three real-window `#[ignore]`d tests in `window.rs`
cannot run as libtest (libtest runs `#[test]` on worker threads; AppKit
requires window construction on the main thread — proven on this Mac: even
bundled, `NSWindow alloc/init` on a worker thread throws
`NSInternalInconsistencyException`). Before this change
`just macos-close-path` does not exist. The probe's assertion set is
regression-failing **for the obligations it asserts** — NOT blanket: the
#1147 map-drain regression is explicitly NOT detectible (see residual 1b).

## 2. Honest residuals (recorded, not silently dropped)

1. **Map-drain (#1147) is unobservable through the public surface — the
   probe cannot assert it, and that is stated, not hidden.** `handle_close`
   ordering is `closed.store` → `windows_map.remove` → `dispatch_close` →
   `callbacks.clear` → `setDelegate:nil`. Deleting ONLY `windows_map.remove`
   (the #1147 fix; a process-lifetime pinned window in any real app) leaves
   every probe obligation green, because `closed.store` still runs and the
   map's pinned `Arc` clone means `drop(window)` never reaches the
   last-clone release gate. The existing unit test
   (`close_drains_platform_map_entry`, window.rs) covers it but is
   libtest-bound; a read-only `Platform::windows()`/`window_count()`
   enumeration API was **considered and rejected here** — it is a monitored
   public surface (runtime-contract registry) added purely for harness
   observability; if a future AppKit-pumping harness needs it, that is a
   separate decision. **Claim explicitly: the probe's PASS does not cover
   map drain.**
2. **Red-button arm + on-screen teardown are a different AppKit class.** The
   probe's window is `visible:false` and never order-fronted — no
   window-server connection, trivial teardown + notification. The real user
   path — ordered-front window, red button (`performClose:`),
   `windowShouldClose:` consultation (`handle_close_request`),
   occlusion/visibility (`handle_visibility_status_change`), `windowDidResignKey` —
   is not drivable through the public `dyn PlatformWindow` surface and stays
   recorded. The shared `windowWillClose:` → `handle_close` teardown that
   route ultimately invokes IS the path exercised.
3. **Autorelease-pool-deferred over-release is not detectible** — the probe
   exits via `std::process::exit` (deliberate: deterministic marker reporting)
   and never drains an autorelease pool, so any re-release scheduled via the
   pool silently leaks and PASSes. Obligation 6 is scoped to **immediate
   explicit releases in `Drop`** only; the claim is worded to that scope.
4. **`closed.store`-before-`dispatch_close` ordering is not pinned.** Frame
   callbacks fire from AppKit's draw cycle (`drawRect` → `dispatch_request_frame`),
   which cannot run in a no-run-loop probe, so a mid-dispatch `window_handle`
   read is not drivable; the code comment at window.rs:2018-2022 is the only
   pin. `dispatch_close`'s own ordering is still exercised (obligation 4).
5. **`surface_released` line-before-exit and device-loss**
   `recover()` → `SurfaceTargetUnavailable` are *engine-side* (flui-engine
   `SurfaceLease::Drop`, a flui-app-lived Renderer). flui-platform sits below
   flui-engine, so this slice cannot live in the same probe; the GPU-free
   engine contract (`renderer_new_fails_before_instance_creation_when_target
   _is_unavailable`) already pins the unavailable-decision. Recorded.
6. **Win32 half**: not this slice (same issue, different host).
7. **"No event loop needed" is thread-affinity reasoning plus ONE empirical
   data point** — the probe passed on this machine with no `run()`. It is not
   asserted as an AppKit law (the deferred-close-during-modal-session class
   exists); recorded as an assumption with its single observation.

## 3. Edit-site map (from scout + both reviews)

| File | Change |
|------|--------|
| `crates/flui-platform/examples/close_path_probe.rs` (exists, untracked, proven on this Mac; now committed) | The probe. **Home decision: a crate example, NOT a `tools/` workspace member** — the member route costs a feature-matrix group placement (CI hard-fail without it) plus a new cross-typecheck step, both of which the example route avoids: flui-platform is already a member in `FM_GROUP_1`, and cross-typecheck's existing step `cargo clippy -p flui-platform --locked --all-targets --features a11y --target aarch64-apple-darwin -- -D warnings` (ci.yml:1311) already compiles+lints **examples** for the Apple target, so the probe's real macOS body is linted by CI with **zero workflow changes**. `[dev-dependencies] tracing-subscriber` is already present — **zero Cargo.toml changes**. Body is `#[cfg(target_os = "macos")]` with a clear no-op main elsewhere (`current_platform()` on Linux returns an Init error — compiling is fine, running must not panic). No `unsafe`; no `run()` |
| `crates/flui-platform/examples/Info.plist.close_path_probe` (new) | Minimal committed bundle plist (`CFBundlePackageType` APPL, `NSPrincipalClass` NSApplication) — the committable piece that clears the `_CFBundleGetValueForInfoKey` construction floor; the just recipe copies it into the staged `.app` |
| `justfile` | New `macos-close-path` recipe (`[group("test")]`), following the repo's host-conditional-convention (mirror `test-ci`/`live-smoke-wayland`: a guard that prints a clear "macOS-only" skip message and exits 0 on non-Darwin — never a silent no-op). On Darwin: `cargo build -p flui-platform --example close_path_probe` → stage `.app` under `target/macos-close-path/` (copy binary + plist) → run with `RUST_LOG=info` → assert exit 0 **and** `CLOSE_PATH_PROBE_RESULT=PASS` from the captured log |
| `docs/runtime-contract.toml` | **Concrete (not deferred):** a new `[[contract]]` entry `key = "macos-window-close-path-executable"`, `state = "partial"` (residuals 2/4/5 stay unexecuted — the registry reflects that), `domain = "platform"`, `owner_issue = 1148`, `mechanical = false`, `evidence = [{ kind = "symbol", file = "crates/flui-platform/src/platforms/macos/window.rs", contains = "fn handle_close" }, { kind = "symbol", file = "crates/flui-platform/src/platforms/macos/window.rs", contains = "closed: AtomicBool" }, { kind = "symbol", file = "crates/flui-platform/examples/close_path_probe.rs", contains = "CLOSE_PATH_PROBE_RESULT" }, { kind = "test", file = "crates/flui-platform/src/platforms/macos/window.rs", contains = "fn close_drains_platform_map_entry" }]` (the last pins the map-drain residual's only executable anchor) |
| `.rust-studio/specs/1148-appkit-close-path-macos/` | This plan + acceptance ledger |

**Precision on the change's size:** no production *module* source changes —
no file under `crates/flui-platform/src/` is touched. The additions are a
crate-shipped example (non-functional surface), the registry entry, and a
recipe. No public API, no `unsafe`, no `[dependencies]` change.

## 4. Approach

1. Commit the proven probe as `flui-platform`'s `close_path_probe` example,
   extended with the on_should_close negative assertion, the macOS cfg gate,
   and the marker/exit discipline. It drives the REAL AppKit close route from
   a bundled binary's `fn main` (the AppKit main thread — the exact floor
   libtest cannot clear) so `_CFBundleGetValueForInfoKey` + main-thread both
   clear (both floors proven empirically on this Mac).
2. Stage the committed `Info.plist` + binary into a minimal `.app` under
   `target/macos-close-path/` via the `just` recipe; run with `RUST_LOG=info`;
   assert exit 0 + `CLOSE_PATH_PROBE_RESULT=PASS`.
3. CI coverage of the probe body comes from the existing cross-typecheck
   Apple step (examples are in `--all-targets`) — the "macOS-only gate
   breakage class" (an Apple-only clippy failure silently green on every
   Linux runner) is closed without new workflow lines.

Verification on this machine:
- `just macos-close-path` exits 0 with all PASS markers (fresh run).
- Mutation 1: remove `closed.store` in `handle_close` → probe FAILS
  obligation 3 (red proven). Mutation 2 (the honest denominator): remove
  `windows_map.remove` → probe does **not** fail — that is residual 1,
  recorded, not claimed.
- `cargo clippy -p flui-platform --all-targets --features a11y -D warnings`
  (Mac) and the aarch64-apple-darwin cross-typecheck shape stay green;
  `cargo fmt --all --check` green; `cargo check -p flui-platform --example
  close_path_probe` on Linux compiles the no-op body (workspace matrix
  intact).
- Honest denominator in the final report: all of §2.

## 5. Test strategy

The probe IS the test; there is no separate harness test in the traditional
sense because the subject is real AppKit teardown which only a real bundled
binary can drive. The `just` recipe's marker+exit assertion is the CI-style
gate a future macOS runner could adopt. The three `#[ignore]`d real-window
tests stay ignored (libtest cannot host AppKit regardless) but their
assertions become locally executable through this probe — except the
map-drain one (residual 1), whose executable anchor remains the unit test
itself.

## 6. Gates

- `cargo fmt --all --check`
- `cargo clippy -p flui-platform --all-targets --features a11y -- -D warnings`
  on this Mac; the equivalent aarch64-apple-darwin line stays CI-green
- Linux: `cargo check -p flui-platform --example close_path_probe` (no-op
  body); no new workspace member, so the feature-matrix union assertion is
  untouched
- `just macos-close-path` PASS on this real Mac (exit 0, all markers)
- Mutation: remove `closed.store` → probe FAILS (red proven); removing
  `windows_map.remove` → probe stays green (residual 1, stated, not claimed)

## Scope decision

This is a **full-loop** task: it adds an executable-coverage example + a
`just` recipe + a registry entry. Full review mode because it touches a
shipped crate example and the runtime-contract registry, and it revokes a
documented ignore-class carrier (the "clippy-clean, never executed" class).
The map-drain observability gap was raised in review and resolved by an
explicit recorded residual, not silently dropped.
