# Plan — live-Mac `platform_it` red: encode the ADR-0039 main-thread-assert class as the documented AppKit-ignore class

Status: Full loop (design fork — how a live-Mac bare `cargo test` should treat the
macOS-live platform surface). Scout + plan-review complete (2026-09-16, this session;
**four independent plan reviewers all returned ACCEPTABLE** — harsh-critic, qa-lead,
api-design-lead, chief-architect — with the amendments below folded in). Derived from
the `/goal` Mac-only menu (no dedicated GitHub issue; nearest tracking issue is the
AppKit executable-coverage gap, whose harness is the residual named in §6).

## 1. Problem and resolution

On a real Mac, `cargo test -p flui-platform` fails **33 tests in the `platform_it`
suite** at the ADR-0039 debug assert in every one of them:

```
test contract::test_platform_name_contract ... FAILED        (and 32 siblings)
thread '...' panicked at src/platforms/macos/platform.rs:...: BUG:
`MacOSPlatform::with_config` must run on the AppKit main thread — thread-affine
AppKit APIs are reached through the owner thread (ADR-0039)
```

Linux CI never sees this class: the CI test step runs the same suite with
`FLUI_HEADLESS=1` (headless mock), and the macOS backend is otherwise
cross-typecheck-only. The exact failing tests (33, across 7 files) all construct the
platform through `current_platform()` (or a module helper that calls it) — i.e. they
ask for the REAL macOS platform:

- `contract.rs` (3): `test_display_enumeration_contract`,
  `test_platform_name_contract`, `test_window_lifecycle_contract`
- `display_enumeration.rs` (7): `test_display_enumeration_performance`,
  `test_displays_enumeration`, `test_high_dpi_scale_factor`,
  `test_macos_nsscreen_enumeration`, `test_multi_monitor_bounds_arrangement`,
  `test_primary_display_detection`, `test_usable_bounds_exclude_system_ui`
- `event_contracts.rs` (4): `test_cross_platform_event_consistency`,
  `test_event_dispatch_latency_benchmark`, `test_event_handling_performance_baseline`,
  `test_platform_event_contract`
- `event_handling.rs` (7): `test_event_callback_registration`,
  `test_event_coordinate_system`, `test_keyboard_with_modifiers`,
  `test_mouse_click_pointer_event`, `test_mouse_movement_pointer_event`,
  `test_multi_touch_pointer_events`, `test_window_resize_event`
- `integration_template.rs` (3): `test_clipboard_integration`,
  `test_executor_integration`, `test_window_handle_compatibility`
- `window_lifecycle.rs` (5): `test_multiple_concurrent_windows`,
  `test_request_redraw`, `test_window_close_event`,
  `test_window_creation_with_options`, `test_window_resize_event`
- `window_modes.rs` (4): `test_dpi_scaling_change`, `test_macos_mode_transitions`,
  `test_per_monitor_dpi`, `test_window_modes`

**Why none of them can genuinely pass in a bare `cargo test` on a real Mac** (two
independent, empirically- and source-established floors):

1. **The platform surface is deliberately main-thread-only (ADR-0039).** Every affine
   `MacOSPlatform` method asserts `+[NSThread isMainThread]` (platform.rs:58-74), not
   just the constructor: `with_config` (:116), `run` (:171), `quit` (:214),
   `open_window` (:228), `active_window` (:237), `displays` (:253),
   `primary_display` (:260). libtest runs every test on a worker thread; the OS main
   thread sits in libtest's join loop and never services the AppKit main queue — so a
   would-be `dispatch_sync(main_queue, …)` from a worker deadlocks (the documented
   owner-lane hazard), and no real AppKit call can run on the main thread.
2. **The bundled-process floor (probe, cf. `macos-nsapp-bare-test-window-construction`
   memory):** `NSApp()` alone succeeds off-main in a bare test, but real `NSWindow`
   allocation throws the `_CFBundleGetValueForInfoKey` NSException ("Rust cannot
   catch foreign exceptions, aborting") — a window-owning test aborts the WHOLE
   process, not just itself. The tests that open windows (`window_lifecycle`,
   `window_modes`, `event_handling`, …) cannot run unbundled, period.

Weakening `with_config` (or any affine method) to "construct off-main" greens
NOTHING — the failing tests fail next at the affine `displays()`/`open_window()`
asserts they then hit, and the relaxation would erode a real production contract to
serve a test environment. Running the tests headless on a Mac would be fake-passing by
the repo's anti-cheating rule (green for `MacOSPlatform` code that never ran on a
Mac). The honest outcome for this environment is the same documented-ignore class the
repo already uses for its real-window tests (window.rs real-window ignore family:
*"requires an AppKit-run-loop-pumping test process; …"*).

**Resolution:** add `#[cfg_attr(target_os = "macos", ignore = "<one canonical
reason>")]` to the 33 tests, where the reason names ADR-0039 + the two floors +
the headless coverage location. `cargo test -p flui-platform` on a real Mac then
reports **0 failures / N ignored** — the suite is green and ACCURATELY describes
coverage. Non-macOS hosts (Linux CI) are unaffected; the `#[ignore]` is the
opt-in-for-next-harness shape (runnable via `--ignored` from the future AppKit
harness, no source change).

**Honest consequence (accepted, per api-design-lead review finding):** the
`#[cfg_attr]` ignore is compile-time and environment-independent, so on a real Mac
THERE IS NO SAFE EXECUTING PATH for these 33 in any bare-test mode: `--ignored`
SIGABRTs the process on window tests (not per-test failures); `FLUI_HEADLESS=1`
returns the headless platform via `current_platform()` but the tests are STILL
`#[ignore]`d — the attribute ignores regardless of env. Local (on-this-Mac) headless
verification of these bodies is therefore lost; the executing coverage of these
bodies lives on non-macOS hosts (Linux CI, `FLUI_HEADLESS=1`) and in the future
AppKit-pumping harness. This is the cost of an honest per-test gate, endorsed by all
four reviewers as the right trade against fake-green alternatives.

## 2. Design (final)

### One canonical reason literal, sharing the existing family's grep phrase

The window.rs real-window ignores already phrase the floor as *"requires an
AppKit-run-loop-pumping test process"* — the new literal MUST reuse that exact phrase
so one grep finds every macOS-live ignore (and the future harness-revocation sweep is
one grep). Canonical literal, inlined per test:

```rust
#[cfg_attr(target_os = "macos", ignore = "requires an AppKit-run-loop-pumping test process (ADR-0039): the macOS platform surface asserts the owner main thread, a bare macOS cargo test cannot pump it and unbundled NSWindow construction aborts the process — these run headless on CI (FLUI_HEADLESS=1) and from an AppKit-pumping process only")]
```

Invariants (all four reviewers agreed): names ADR-0039 (load-bearing exemption,
marker rule); states the main-thread contract and the bare-process floor; locates the
executing coverage without overclaiming it for a Mac; no internal markers.

### Guard against drift (harsh-critic required change)

No gate enforces "any `current_platform()`-constructing test in this suite must carry
the macOS ignore," and macOS is not in CI, so a future test following the file idiom
would silently re-red the live-Mac suite. Fix: a one-line invariant in `tests/main.rs`
(the consolidation root, the single place every platform_it module is declared):

```rust
//! macOS invariant: any `platform_it` test that constructs the platform through
//! `current_platform()` (or a helper reaching it) must carry
//! `#[cfg_attr(target_os = "macos", ignore = "requires an AppKit-run-loop-pumping
//! test process (ADR-0039): …")]` — the macOS surface asserts the AppKit main
//! thread, which a bare `cargo test` cannot host (ADR-0039; unbundled NSWindow aborts).
```

(No port-check trigger — that is scope-add; the comment is the cheap, greppable form.)

### Placement

Each of the 7 files: the failing `#[test] fn` gets the `#[cfg_attr]` immediately below
its `#[test]`. Two macOS-`cfg`'d tests need special handling:

- `test_macos_nsscreen_enumeration` (display_enumeration.rs): add the `cfg_attr` to
  the `#[cfg(target_os = "macos")]` arm ONLY; its `#[cfg(not(target_os = "macos"))]`
  stub twin keeps running everywhere else.
- `test_macos_mode_transitions` (window_modes.rs): `#[test]` +
  `#[cfg(target_os = "macos")]`, **no** stub twin (verified in source) — add the
  `cfg_attr` to its macos arm; off-macOS it simply does not compile. (NB: it asserts
  `platform.name() == "macOS"`, so it was never greenable headless on a Mac anyway.)

Site map (verified): `contract.rs` :281/:430/:476; `display_enumeration.rs`
:24/:98/:150/:231/:354(macos arm)/:410/:559; `event_contracts.rs` :30/:182/:262/:330;
`event_handling.rs` :40/:87/:152/:223/:285/:343/:367; `integration_template.rs`
:85/:233/:268; `window_lifecycle.rs` :11/:78/:120/:181/:221; `window_modes.rs`
:12/:108(macos arm)/:152/:205.

## 3. Test strategy

- **Red → documented-ignore (the honest direction):** these 33 are ALREADY red on this
  Mac (measured today: `36 passed / 33 failed / 8 ignored`). Each red converts to a
  documented `ignore`; nothing that could have passed is hidden. No anti-cheating
  line crossed: `#[ignore]` reports truthfully (0 failed / N ignored); it falsifies
  nothing that ran.
- **Gate = "0 failed", pass band is 34–36 (qa-lead F1):** the passing count is NOT
  stable — three pre-existing timing-wall flakes explain the observed 34↔36 drift:
  `contract_background_executor` (contract.rs:193, 100 ms bare sleep), 
  `test_executor_spawn_performance` (performance.rs:197), 
  `test_clipboard_operations_performance` (performance.rs:160). The post-change
  expectation is `0 failed / 41 ignored` (33 moved + 8 existing) with a 34–36 pass
  band — a flaky red among those three would re-drift the pass count, not the failed
  ledger, and is not introduced by this change (it touches none of the 36).
- **Linux CI unaffected:** the `cfg_attr` is macOS-only; CI runs the suite headless on
  Linux (where the attr is absent — the 33 still execute there) and never links the
  macOS backend's tests (flui-platform is cross-typecheck-only in CI).
- **Live-Mac verification (this Mac):** `cargo test -p flui-platform` (lldstrip
  wrapper) → platform_it `0 failed`, `41 ignored`, the 33 names present in the
  ignored ledger; lib `164 passed / 0 failed / 3 ignored`; headless `12 passed`.
  `FLUI_HEADLESS=1 cargo test` — full suite green EXCEPT the 33 now appear ignored on
  macOS (documented consequence above, NOT a regression to chase).
- **`clippy -D warnings` + `fmt`** clean on the touched files (mechanical attrs).
- Honest limitation, re-stated: this does NOT ADD macOS-live surface coverage — it
  makes the live-Mac DEFAULT run truthful (green + ignored) instead of 33 unexplained
  reds. Genuine macOS-live surface execution is the future AppKit-pumping harness
  (§6), unchanged.

## 4. Environment

`crates/flui-platform` links AppKit locally — builds need the lld-strip wrapper shim
(`--config 'build.rustc-wrapper="/tmp/lldstrip-wrapper.sh"'`); `.cargo/` stays
untouched (user constraint). `just runtime-conformance-check` needs `/tmp/py312shim`;
`just ci` needs `/tmp/bashshim`.

## 5. Verification (to be observed at build close)

- `cargo test -p flui-platform` (wrapper): platform_it **0 failed**, 41 ignored, all
  33 names in the ignored ledger; lib/headless green unchanged.
- `cargo clippy -p flui-platform --all-targets --all-features -- -D warnings` exit 0;
  `cargo fmt --all --check` clean (touched files).
- Diff is exactly the 33 `#[cfg_attr]` insertions + the `tests/main.rs` invariant
  note + nothing else.
- Red→red honesty proof: the pre-change run (36/33/8) is on record; post-change run
  must list the same 33 as ignored and nothing as newly-passing.

## 6. Explicitly NOT in this change

- **The AppKit-pumping test process (the harness this class eventually runs under).**
  When that harness lands it must REVOKE this class — delete these 33 `#[cfg_attr]`
  attrs and the existing window.rs real-window ignores — so the documented-ignore
  layer does not ossify (chief-architect handover). Bundling a test binary's
  Info.plist is fragile/non-committable per the earlier probe; the harness is a
  deliberate, separate, larger change.
- **A bundle-free main-thread pump harness** (proposed by plan-review; considered and
  deferred): a custom `[[test]] harness = false` binary that pumps the AppKit main
  queue could TODAY, unbundled, genuinely exercise the read-only subset (~10 of the
  33: `name`/`displays`/`primary_display`/`high_dpi`/`usable_bounds`/`multi_monitor`/
  nsscreen + non-window `event_contracts`) — `NSApp()` works unbundled and
  `[NSScreen screens]` reads come from the window server, not the bundle. The
  window-bearing tests still SIGABRT, and the pump is the previous harness's core
  infrastructure — so deferral is legitimate, not a lost opportunity. Naming it here
  keeps the read-only subset from being treated as permanently unrunnable.
- **Windows-live analog.** The pattern extends additively
  (`cfg_attr(target_os = "windows", ignore = "<reason>")`), but the failure mode on
  Win32 may be SILENT, not an assert: per ADR-0039's hazard analysis, off-thread
  Win32 window creation silently binds the message queue to the wrong thread (no
  red, no abort) — a strictly worse failure than the Mac's loud assert. The
  encoding's fit on Windows is unmeasured and is a separate design question (whether
  the Win32 backend ever asserts `IsGUIThread`). Deferred; no Windows machine here.
- **`current_platform()` / `MacOSPlatform` threading contract** — untouched; the
  ADR-0039 main-thread asserts are the correct production contract and stay exactly
  as they are. The test environment cannot host them; it gives, not the contract.
- **Two pre-existing observations, no behavior change, explicitly deferred:**
  (a) `contract.rs::get_test_platform()` (contract.rs:37-45) returns
  `headless_platform()` in BOTH branches — this is a LIVE instance of exactly the
  headless-masking pattern §1 rejects, doing so on every platform including a live
  Mac; it is deferred as-is (fixing it to the event_handling shape would add ~10 more
  names to the macOS ignore ledger for zero CI benefit). It is the reason ~10 of the
  "36 passed" are headless-in-disguise, so the pass count is framed accordingly.
  (b) `current_platform()`'s doc (lib.rs) still describes the macOS backend as a
  "stub" and never mentions the main-thread requirement — a docs-surface artifact, a
  separate docs ticket, out of scope here.
