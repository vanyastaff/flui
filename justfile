# --- FLUI justfile ---
# Cross-platform task runner for the FLUI Rust workspace.
# Usage: just [recipe]
# Install: https://just.systems/man/en/

set shell := ["bash", "-euo", "pipefail", "-c"]
set windows-shell := ["bash", "-euo", "pipefail", "-c"]
set dotenv-load
set export
set positional-arguments

# --- Variables ---
version := `git describe --tags --always --dirty 2>/dev/null || echo "dev"`
commit  := `git rev-parse --short HEAD 2>/dev/null || echo "unknown"`

# Active workspace members (must match crates/* in Cargo.toml [workspace.members])
active_crates := "flui-animation flui-app flui-assets flui-testing flui-build flui-cli flui-cupertino flui-devtools flui-engine flui-foundation flui-geometry flui-hot-reload flui-interaction flui-layer flui-localizations flui-log flui-macros flui-material flui-objects flui-painting flui-platform flui-rendering flui-scheduler flui-semantics flui-tree flui-types flui-view flui-widgets"

# Default recipe — show help
[doc("Show available recipes grouped by category")]
default:
    @just --list --unsorted

# =============================================================================
# Build
# =============================================================================

[group("build")]
[doc("Type-check the entire workspace (fast, no codegen)")]
check:
    cargo check --workspace --all-targets

[group("build")]
[doc("Build the workspace (default profile)")]
build:
    cargo build --workspace

[group("build")]
[doc("Build the workspace in release mode (LTO enabled)")]
build-release:
    cargo build --workspace --release

[group("build")]
[doc("Build a single crate by name (e.g. just build-crate flui-engine)")]
build-crate crate:
    cargo build -p {{crate}}

[group("build")]
[doc("Build foundation layer first, then up the DAG (manual incremental build)")]
build-layered:
    cargo build -p flui-geometry
    cargo build -p flui-types
    cargo build -p flui-foundation
    cargo build -p flui-macros
    cargo build -p flui-log
    cargo build -p flui-tree
    cargo build -p flui-platform
    cargo build -p flui-assets
    cargo build -p flui-painting
    cargo build -p flui-semantics
    cargo build -p flui-scheduler
    cargo build -p flui-layer
    cargo build -p flui-interaction
    cargo build -p flui-animation
    cargo build -p flui-engine
    cargo build -p flui-hot-reload
    cargo build -p flui-rendering
    cargo build -p flui-objects
    cargo build -p flui-view
    cargo build -p flui-widgets
    cargo build -p flui-localizations
    cargo build -p flui-material
    cargo build -p flui-cupertino
    cargo build -p flui-testing
    cargo build -p flui-app
    cargo build -p flui-devtools
    cargo build -p flui-build
    cargo build -p flui-cli

[group("build")]
[doc("Type-check and clippy the wasm-capable crates for wasm32-unknown-unknown (mirrors the CI wasm-check job)")]
wasm-check:
    cargo check --workspace --locked --target wasm32-unknown-unknown \
      --exclude flui-assets --exclude flui-build --exclude flui-cli \
      --exclude flui-web-server --exclude hot-reload-counter-host \
      --exclude hot-reload-counter-logic --exclude hot-reload-counter-types
    # Lib/bin targets only: test targets pull native-only dev-deps (tokio).
    # This is the ONLY lint pass over flui-platform's wasm32-only web backend.
    cargo clippy --workspace --lib --bins --locked --target wasm32-unknown-unknown \
      --exclude flui-assets --exclude flui-build --exclude flui-cli \
      --exclude flui-web-server --exclude hot-reload-counter-host \
      --exclude hot-reload-counter-logic --exclude hot-reload-counter-types \
      -- -D warnings
    # The development feature has no web runner integration, but enabling it
    # must remain a well-formed build rather than exposing native-only imports.
    cargo check -p flui --locked --target wasm32-unknown-unknown \
      --no-default-features --features hot-reload

# Compiling for wasm32 is not running on wasm32. Until this recipe existed the
# workspace only ever type-checked and linked for the target (`wasm-check` and
# `wasm-link-check` above), so every "works on the web" claim rested on the
# linker succeeding — see issue #985. Hosts the tests on node; no browser and no
# wasm-pack involved.
[group("test")]
[doc("Actually EXECUTE the wasm32 tests (node, via wasm-bindgen-test-runner)")]
wasm-test:
    #!/usr/bin/env bash
    set -euo pipefail
    # The runner's version must match the LOCKED wasm-bindgen exactly or it
    # refuses to start. Read it out of Cargo.lock rather than hardcoding it, so
    # a dependabot bump moves the tool with the lock instead of breaking this
    # recipe with a message that reads like a toolchain fault.
    want=$(python3 -c "import tomllib;print(next(p['version'] for p in tomllib.load(open('Cargo.lock','rb'))['package'] if p['name']=='wasm-bindgen'))")
    have=$(wasm-bindgen --version 2>/dev/null | cut -d' ' -f2 || true)
    if [ "$have" != "$want" ]; then
        echo "wasm-bindgen-cli $want required (have: ${have:-none}); installing" >&2
        cargo install wasm-bindgen-cli --version "$want" --locked
    fi
    # `out=$(cargo test ...)` under `set -e` exits AT THE ASSIGNMENT when cargo
    # fails, so the panic message and the runner's own diagnostics -- the only
    # things that say WHY -- are captured and then thrown away. Branch on the
    # command so the output is printed either way. `--locked` matches CI and
    # keeps a local run from quietly re-resolving Cargo.lock.
    # DISCOVER the participating crates rather than list them. Naming them
    # here means the next crate to add wasm tests is silently never run --
    # the defect this whole recipe exists to fix, one level up.
    #
    # The opt-in signal is the crate's own manifest: a wasm32 dev-dependency on
    # wasm-bindgen-test. That is greppable, it is where a contributor already
    # has to declare intent, and it cannot drift from the code the way a list
    # here would.
    #
    # Both target kinds run, because which one is possible depends on
    # visibility: an integration test (tests/wasm32.rs) sees only the public
    # API, while a `pub(crate)` seam -- flui-app's `Backend::Sequential`, for
    # instance -- is reachable ONLY from a lib test.
    # Mirror the CI step's environment, not just its command. CI denies build
    # warnings workspace-wide; the wasm lib-test configuration legitimately has
    # dead code (its callers are the native-only test modules), so that step
    # relaxes the deny and this must too -- otherwise a local green means less
    # than CI's, which is the one thing this recipe exists not to do.
    export CARGO_BUILD_WARNINGS=warn
    total=0
    crates=0
    # Parsed, not grepped. A substring search over the manifest also matches a
    # comment, a normal dependency, or a non-wasm32 target table -- none of
    # which mean "this crate has wasm tests" -- so the check would not match the
    # contract it documents, silently.
    for name in $(python3 scripts/wasm-test-crates.py); do
        dir="crates/$name"
        crates=$((crates + 1))
        crate_total=0
        # Modes, not raw argv: a flat array of "--lib --test wasm32" iterates as
        # three items and needs guards to reassemble, which is how a bug hides.
        modes=(lib)
        [ -f "$dir/tests/wasm32.rs" ] && modes+=(integration)
        for mode in "${modes[@]}"; do
            if [ "$mode" = "lib" ]; then
                args=(--lib)
            else
                args=(--test wasm32)
            fi
            # `out=$(cargo test ...)` under `set -e` exits AT THE ASSIGNMENT
            # when cargo fails, so the panic and the runner's own diagnostics --
            # the only things that say WHY -- are captured and thrown away.
            # Branch on the command so the output is printed either way.
            if ! out=$(cargo test -p "$name" --locked --target wasm32-unknown-unknown "${args[@]}" 2>&1); then
                echo "$out"
                echo "wasm-test: $name ($mode target) failed -- see the output above" >&2
                exit 1
            fi
            echo "$out"
            passed=$(echo "$out" | sed -n 's/^test result: ok\. \([0-9][0-9]*\) passed.*/\1/p' | tail -1)
            crate_total=$((crate_total + ${passed:-0}))
        done
        # Per crate, not only in aggregate: a newly opted-in crate with no
        # executing test contributes 0 while a healthy sibling keeps the total
        # non-zero, so exactly the crate someone just added is the one that can
        # be silently inert. Not per TARGET -- a crate legitimately has only one
        # kind (flui-foundation has no wasm lib tests, flui-app no integration
        # ones), and a zero there is expected rather than a defect.
        if [ "$crate_total" -eq 0 ]; then
            echo "wasm-test: $name opted in but executed no wasm32 assertions -- inert" >&2
            exit 1
        fi
        total=$((total + crate_total))
    done
    # A `for` over a glob that matches nothing never runs its body and exits 0,
    # which looks exactly like success.
    if [ "$crates" -eq 0 ]; then
        echo "wasm-test: no crate declares a wasm32 wasm-bindgen-test dev-dependency" >&2
        exit 1
    fi
    echo "wasm-test: $total wasm32 assertions executed across $crates crate(s)"

# `cargo check` does not link, and on wasm32 even a link does not fail on an
# undefined symbol (rust-lld turns it into an import) — hence the committed
# import allowlist. Requires wasm-tools (cargo binstall wasm-tools).
[group("build")]
[doc("Really link the two wasm cdylibs and check their import surface (mirrors the CI wasm-check link steps)")]
wasm-link-check:
    cargo build --locked --target wasm32-unknown-unknown -p flui-web-demo -p flui-painting-demo
    bash scripts/check-wasm-imports.sh \
      target/wasm32-unknown-unknown/debug/flui_web_demo.wasm \
      target/wasm32-unknown-unknown/debug/flui_painting_demo.wasm

# `cargo clippy` does not link, so the per-OS backends in flui-platform can be
# type-checked from any host. Without this they are only ever compiled by
# whoever happens to develop on that OS — which is how the Windows backend
# came to have a hard `missing_docs` error, both desktop backends accumulated
# missing Debug impls, and Android accumulated ~24 more of its own (found the
# day it joined this matrix) that no gate could see.
#
# The triples are the ones actually shipped: MSVC (what `gpu-test` runs on
# windows-latest), not the GNU ABI, aarch64 for macOS, and aarch64 for
# Android. `--all-targets` so per-OS test targets are compiled too — omitting
# it is what made live code look dead on wasm32.
#
# LINT, NO LINK: clippy (not plain check) because cfg(windows)/cfg(macos)/
# cfg(target_os = "android") code is invisible to every other lint gate —
# check-only let ~80 deny-level violations accumulate unseen on Windows/macOS
# and a further ~24 on Android. It still does not link and runs no tests —
# Android's own build needs the NDK's cross-linker, which `cargo clippy`
# never reaches. The `test` job's dedicated flui-platform step only runs the
# Linux-buildable backends (headless + winit-on-X11); the Windows, macOS, and
# Android backends this lints are never linked or executed by anything else.
# Green here means "compiles clean under the workspace lints", nothing more.
# Requires: rustup target add x86_64-pc-windows-msvc aarch64-apple-darwin aarch64-linux-android
[group("build")]
[doc("Clippy flui-platform's Windows, macOS, Android, and iOS backends plus flui-app's mobile runners from this host (mirrors the CI cross-typecheck job)")]
cross-typecheck:
    # `--features a11y` on every line: the UIA/NSAccessibility bridges are
    # feature-gated and this job is the ONLY gate that compiles them at all
    # (the a11y-off configuration is a strict subset — no cfg(not(a11y))
    # code exists — so checking with the feature supersedes checking without).
    cargo clippy -p flui-platform --locked --all-targets --features a11y --target x86_64-pc-windows-msvc -- -D warnings
    cargo clippy -p flui-platform --locked --all-targets --features a11y --target aarch64-apple-darwin -- -D warnings
    cargo clippy -p flui-platform --locked --all-targets --features a11y --target aarch64-linux-android -- -D warnings
    # iOS: the native UIKit backend had no lint gate at all before this line,
    # and it did not compile for the target until the platform work landed.
    # `aarch64-apple-ios` (device) rather than `-sim`, matching the other
    # targets: sim and device differ only in the slice, not in the API surface
    # this lint sees, and `just ios-sim` executes the simulator one.
    cargo clippy -p flui-platform --locked --all-targets --features a11y --target aarch64-apple-ios -- -D warnings
    # The mobile runners live in flui-app behind `cfg(target_os = ...)`, so
    # the platform lines above never compile them; the `flui` facade rides
    # along because its re-exports are what a consumer builds against there.
    # Library targets only (the tests are host-run). Android's `psm` C shim
    # (via stacker) is cross-compiled by the host clang — no NDK needed for a
    # compile-only lint.
    cargo clippy -p flui-app -p flui --locked --target aarch64-apple-ios -- -D warnings
    CC_aarch64_linux_android=clang CFLAGS_aarch64_linux_android=--target=aarch64-linux-android21 AR_aarch64_linux_android=ar \
        cargo clippy -p flui-app -p flui --locked --target aarch64-linux-android -- -D warnings

# =============================================================================
# Testing
# =============================================================================

[group("test")]
[doc("Run all tests across the workspace")]
test *args:
    cargo test --workspace {{args}}

[group("test")]
[doc("Live E2E smoke: a REAL windowed demo driven by REAL X11 input (XTEST) — launch, mid-drag tracking, scroll, clean WM_DELETE close, then a second launch that closes itself through PlatformWindow::close (the programmatic route, issue #919). Needs xvfb-run (apt install xvfb). Covers the band synthetic tests can't: platform translation, the wake chain, teardown")]
live-smoke:
    cargo build --package flui --features material --example sliver_demo
    cargo build --package flui-live-smoke
    xvfb-run -a -s "-screen 0 1200x800x24" target/debug/flui-live-smoke target/debug/examples/sliver_demo

[group("test")]
[doc("Wayland live-smoke: close-path teardown ordering under a headless weston compositor — the demo self-closes (FLUI_SELF_CLOSE_AFTER_MS drives the real CloseRequested arm) and must exit 0, five cycles, then two more cycles on the programmatic PlatformWindow::close route (issue #919). Catches the post-quit wl_proxy use-after-free class (issue #713) the X11 smoke can never see. SKIPs with a message when weston is absent (apt install weston)")]
live-smoke-wayland:
    cargo build --package flui --features material --example sliver_demo
    cargo build --package flui-live-smoke
    target/debug/flui-live-smoke target/debug/examples/sliver_demo wayland

[group("test")]
[doc("Executable AppKit close/teardown coverage on a real Mac (issue #1148 AppKit half): builds the close_path_probe example, stages it into a minimal .app bundle — the committed Info.plist clears _CFBundleGetValueForInfoKey, and the binary's fn main IS the AppKit main thread, the two floors that make unbundled libtest unable to host real AppKit windows — then runs it with RUST_LOG=info and asserts exit 0 plus the CLOSE_PATH_PROBE_RESULT=PASS marker. macOS-only by construction; skips with a message on other hosts")]
macos-close-path:
    {{ if os() == "macos" {
"cargo build -p flui-platform --locked --example close_path_probe\nAPP=target/macos-close-path/ClosePathProbe.app\nrm -rf \"$APP\"\nmkdir -p \"$APP/Contents/MacOS\"\ncp crates/flui-platform/examples/Info.plist.close_path_probe \"$APP/Contents/Info.plist\"\ncp target/debug/examples/close_path_probe \"$APP/Contents/MacOS/close_path_probe\"\nrc=0; out=$(RUST_LOG=info \"$APP/Contents/MacOS/close_path_probe\" 2>&1) || rc=$?\nprintf '%s\\n' \"$out\"\nif [ \"$rc\" -ne 0 ] || ! printf '%s\\n' \"$out\" | grep -q 'CLOSE_PATH_PROBE_RESULT=PASS'; then\n  echo 'macos-close-path FAILED: probe exit code or PASS marker missing (output above)'\n  exit 1\nfi"
} else {
"echo 'Skipping macos-close-path on this host: the probe needs a real macOS host with an active GUI session and a staged .app bundle (AppKit window construction requires the main thread and a bundle); on a Mac run: just macos-close-path'"
} }}

[group("test")]
[doc("Executable AppKit frame-pump coverage on a real Mac: builds the frame_pump_probe example, stages it into a minimal .app the same way macos-close-path does, runs it with RUST_LOG=info, and asserts exit 0 plus the FRAME_PUMP_PROBE_RESULT=PASS marker. The probe counts frames from a REAL visible window whose frame callback re-arms itself the way the engine's frame does, and requires frames to keep arriving after the primer that started them stopped — the AppKit display pass discards an in-pass setNeedsDisplay:, so a backend without the deferral runs exactly one frame and reports FAIL. macOS-only by construction; skips with a message on other hosts")]
macos-frame-pump:
    {{ if os() == "macos" {
"cargo build -p flui-platform --locked --example frame_pump_probe\nAPP=target/macos-frame-pump/FramePumpProbe.app\nrm -rf \"$APP\"\nmkdir -p \"$APP/Contents/MacOS\"\ncp crates/flui-platform/examples/Info.plist.frame_pump_probe \"$APP/Contents/Info.plist\"\ncp target/debug/examples/frame_pump_probe \"$APP/Contents/MacOS/frame_pump_probe\"\nrc=0; out=$(RUST_LOG=info \"$APP/Contents/MacOS/frame_pump_probe\" 2>&1) || rc=$?\nprintf '%s\\n' \"$out\"\nif [ \"$rc\" -ne 0 ] || ! printf '%s\\n' \"$out\" | grep -q 'FRAME_PUMP_PROBE_RESULT=PASS'; then\n  echo 'macos-frame-pump FAILED: probe exit code or PASS marker missing (output above)'\n  exit 1\nfi"
} else {
"echo 'Skipping macos-frame-pump on this host: the probe needs a real macOS host with an active GUI session and a staged .app bundle — it measures frames from a visible window, so it cannot run headless or on another OS; on a Mac run: just macos-frame-pump'"
} }}

[group("test")]
[doc("Executable resize-transient coverage on a real Mac: builds the resize_jitter_probe example into a staged .app the same way macos-frame-pump does, runs it with RUST_LOG=info, and asserts exit 0 plus the RESIZE_JITTER_PROBE_RESULT=PASS and RESIZE_JITTER_PROBE_STALE=0 markers. The probe drives a scripted burst of REAL window resizes while rendering continuously into the Metal swapchain, with the surface deliberately held frames behind the window, and counts Renderer::warn_on_size_mismatch — the acquired swapchain texture differing from the configured surface size, i.e. the frame a compositor would stretch. It pins that invariant; it does NOT discriminate desired_maximum_frame_latency, which it was built to do and measurably cannot (zero at 1 and at 2, four runs) — see the probe's own module doc and the literal's comment in renderer.rs. macOS-only by construction; skips with a message on other hosts")]
macos-resize-jitter:
    {{ if os() == "macos" {
"cargo build -p flui --locked --example resize_jitter_probe\nAPP=target/macos-resize-jitter/ResizeJitterProbe.app\nrm -rf \"$APP\"\nmkdir -p \"$APP/Contents/MacOS\"\ncp examples/Info.plist.resize_jitter_probe \"$APP/Contents/Info.plist\"\ncp target/debug/examples/resize_jitter_probe \"$APP/Contents/MacOS/resize_jitter_probe\"\nrc=0; out=$(RUST_LOG=info \"$APP/Contents/MacOS/resize_jitter_probe\" 2>&1) || rc=$?\nprintf '%s\\n' \"$out\"\nif [ \"$rc\" -ne 0 ] || ! printf '%s\\n' \"$out\" | grep -q 'RESIZE_JITTER_PROBE_RESULT=PASS' || ! printf '%s\\n' \"$out\" | grep -q 'RESIZE_JITTER_PROBE_STALE=0'; then\n  echo 'macos-resize-jitter FAILED: probe exit code, PASS marker, or the zero stale-size marker is missing (output above)'\n  exit 1\nfi"
} else {
"echo 'Skipping macos-resize-jitter on this host: the probe measures the Metal swapchain of a real visible AppKit window under a resize burst, so it needs a real macOS host with an active GUI session and a staged .app bundle; on a Mac run: just macos-resize-jitter'"
} }}

[group("test")]
[doc("Executable macOS text-input coverage on a real Mac: builds the ime_probe example, stages it into a staged .app the same way macos-frame-pump does, runs it with RUST_LOG=info, and asserts exit 0 plus the IME_PROBE_RESULT=PASS marker. The probe runs the real backend through the production launch path, reaches the window's content view through AppKit, and drives four assertions: (A, ADR-0069) one synthesized keyDown for one letter reaches the application as exactly one ImeEvent::Commit with zero Key::Character while a text input is attached, and the exact inverse with it detached; (B) the NSTextInputClient queries AppKit makes answer correctly, including the UTF-16 to byte cursor conversion; (C) a cursor area set through the trait comes back as a non-zero rect; (D) unmarkText announces the end of composition. The key events are synthesized, not human keystrokes, and NO genuine input method runs, so a real composition stays undriven - the probe covers the routing and the protocol, not the input method. macOS-only by construction; skips with a message on other hosts")]
macos-ime:
    {{ if os() == "macos" {
"cargo build -p flui-platform --locked --example ime_probe\nAPP=target/macos-ime/ImeProbe.app\nrm -rf \"$APP\"\nmkdir -p \"$APP/Contents/MacOS\"\ncp crates/flui-platform/examples/Info.plist.ime_probe \"$APP/Contents/Info.plist\"\ncp target/debug/examples/ime_probe \"$APP/Contents/MacOS/ime_probe\"\nrc=0; out=$(RUST_LOG=info \"$APP/Contents/MacOS/ime_probe\" 2>&1) || rc=$?\nprintf '%s\\n' \"$out\"\nif [ \"$rc\" -ne 0 ] || ! printf '%s\\n' \"$out\" | grep -q 'IME_PROBE_RESULT=PASS'; then\n  echo 'macos-ime FAILED: probe exit code or PASS marker missing (output above)'\n  exit 1\nfi"
} else {
"echo 'Skipping macos-ime on this host: the probe needs a real macOS host with an active GUI session and a staged .app bundle — it routes AppKit key events into a visible window, so it cannot run headless or on another OS; on a Mac run: just macos-ime'"
} }}

[group("test")]
[doc("Executable launch-route render coverage on a real Mac: builds the colored_box_app example and runs it through scripts/check-macos-launch-render.py, which launches the SAME bundled artifact three ways (direct exec, `open` / LaunchServices, `open -g` / LaunchServices without activation), 5 launches each, finds each launch's window by owning PID in the CoreGraphics window list, and photographs it by window number. The oracle is the pixels of that window, not frames or survival: the fixture paints pure red, and every launch must show it, because a window can exist, hold a live frame pump, and still be blank. A genuinely blank window is the control and fails (see the checker's own validation) — so the gate discriminates instead of merely passing. Each launch is also given a bounded settle, because macOS orders a window front before its first frame is presented: the oracle is retried until it holds, so a window that is merely early is not mistaken for a blank one, and the time to the capture that passed is reported as first frame after. A window-scoped capture of an unpainted window is a flat dark image whatever the display shows, so on a failure the checker also takes one screen capture of the window rectangle, only when that window is frontmost, and reports what a viewer had on screen — as a diagnostic that decides nothing. Needs Screen Recording, which is preflighted: without it the checker exits 2 (CANNOT VERIFY) rather than reporting a blank window it never saw. This closes the LaunchServices half of the blank-window observation in docs/BETA.md. macOS-only by construction; skips with a message on other hosts")]
macos-launch-render:
    {{ if os() == "macos" {
"cargo build -p flui --locked --example colored_box_app\nrc=0\npython3 scripts/check-macos-launch-render.py target/debug/examples/colored_box_app --runs 5 --expect 240,0,0 || rc=$?\nif [ \"$rc\" -eq 2 ]; then\n  echo 'macos-launch-render CANNOT VERIFY: this host could not take the measurement (Screen Recording not granted, or swiftc missing) — a denied capture is NOT a blank window, so nothing was decided; details above'\nelif [ \"$rc\" -ne 0 ]; then\n  echo 'macos-launch-render FAILED: a launch route was refused, put no window on screen, or put up a window that stayed blank for the whole settle - details and images above'\nfi\nexit \"$rc\""
} else {
"echo 'Skipping macos-launch-render on this host: the gate photographs a real window on a real display, so it needs macOS with an active GUI session; on a Mac run: just macos-launch-render'"
} }}

[group("test")]
[doc("Runs the iOS demo on an iOS Simulator (the only executing coverage of the native UIKit backend). Builds examples/ios_demo for aarch64-apple-ios-sim, stages it into a minimal .app, boots a simulator, installs and launches it, captures a screenshot, and asserts the app got as far as a created Metal device and a rendered frame — read out of the simulator's unified log, since UIApplicationMain owns the process and no test harness can. macOS-host only (needs Xcode + simctl); skips with a message elsewhere.")]
ios-sim:
    {{ if os() == "macos" {
"set -e\nDEVICE=\"${FLUI_IOS_SIM_DEVICE:-iPhone 17 Pro}\"\nxcrun simctl boot \"$DEVICE\" 2>/dev/null || true\nxcrun simctl bootstatus \"$DEVICE\" -b >/dev/null 2>&1 || true\n\n# Arm 1 — a static Material app: Metal device created and a frame rendered.\nBUNDLE=dev.flui.ios-demo\nAPP=target/ios-sim/IosDemo.app\ncargo build -p flui --locked --features material --example ios_demo --target aarch64-apple-ios-sim\nrm -rf \"$APP\"\nmkdir -p \"$APP\"\ncp examples/Info.plist.ios_demo \"$APP/Info.plist\"\ncp target/aarch64-apple-ios-sim/debug/examples/ios_demo \"$APP/ios_demo\"\nxcrun simctl install booted \"$APP\"\nxcrun simctl terminate booted \"$BUNDLE\" 2>/dev/null || true\nxcrun simctl launch booted \"$BUNDLE\" >/dev/null\nsleep 12\nLOG=target/ios-sim/app.log\nxcrun simctl spawn booted log show --last 5m --process ios_demo > \"$LOG\" 2>/dev/null || true\nxcrun simctl io booted screenshot target/ios-sim/screen.png >/dev/null 2>&1 || true\nif ! grep -q 'Selected GPU:.*Metal' \"$LOG\" || ! grep -q 'First frame rendered' \"$LOG\"; then\n  echo 'IOS_SIM_RESULT=FAIL - arm 1 (static app): expected \"Selected GPU ... Metal\" and \"First frame rendered\"; tail:';\n  grep 'flui]' \"$LOG\" | tail -20;\n  exit 1;\nfi\necho 'IOS_SIM arm1=PASS (Metal device created, first frame rendered)';\n\n# Arm 2 — an ANIMATED app, and the regression guard for the iOS frame-source\n# bug. Survival alone is NOT the discriminator: a request_redraw that\n# dispatches a frame synchronously AND calls setNeedsDisplay() can still\n# complete and log frames while the screen stays WHITE, because UIKit repaints\n# the opaque UIView's empty layer over the CAMetalLayer the renderer presented\n# into. The honest signal is the pixels: two screenshots of a live, animating\n# tree must DIFFER. A cropped centre square is compared so a ticking status-bar\n# clock can never masquerade as motion.\nANIM=dev.flui.anim-demo\nAAPP=target/ios-sim/AnimDemo.app\ncargo build -p flui --locked --example animated_box_app --target aarch64-apple-ios-sim\nrm -rf \"$AAPP\"\nmkdir -p \"$AAPP\"\ncp examples/Info.plist.ios_anim \"$AAPP/Info.plist\"\ncp target/aarch64-apple-ios-sim/debug/examples/animated_box_app \"$AAPP/animated_box_app\"\nxcrun simctl install booted \"$AAPP\"\nxcrun simctl terminate booted \"$ANIM\" 2>/dev/null || true\nxcrun simctl launch booted \"$ANIM\" >/dev/null\nsleep 12\ntarget_io=target/ios-sim\nxcrun simctl io booted screenshot \"$target_io/anim_a.png\" >/dev/null 2>&1 || true\nsleep 2\nxcrun simctl io booted screenshot \"$target_io/anim_b.png\" >/dev/null 2>&1 || true\ncp \"$target_io/anim_a.png\" \"$target_io/anim_a_crop.png\"\ncp \"$target_io/anim_b.png\" \"$target_io/anim_b_crop.png\"\nsips -c 240 240 \"$target_io/anim_a_crop.png\" >/dev/null 2>&1 || true\nsips -c 240 240 \"$target_io/anim_b_crop.png\" >/dev/null 2>&1 || true\nHA=$(md5 -q \"$target_io/anim_a_crop.png\")\nHB=$(md5 -q \"$target_io/anim_b_crop.png\")\nALIVE=$(pgrep -f animated_box_app | wc -l | tr -d ' ')\nxcrun simctl terminate booted \"$ANIM\" 2>/dev/null || true\nif [ \"$HA\" = \"$HB\" ]; then\n  echo 'IOS_SIM_RESULT=FAIL - arm 2 (animated app): two screenshots 2 s apart are IDENTICAL, so no animation reached the screen (frozen or white) even though frames may be logged; see target/ios-sim/anim_{a,b}.png';\n  exit 1;\nfi\nif [ \"$ALIVE\" -lt 1 ]; then\n  echo 'IOS_SIM_RESULT=FAIL - arm 2 (animated app): the app did not survive to the screenshot pass - request_redraw likely never returned to UIKit and iOS scene-create watchdog killed it';\n  exit 1;\nfi\necho 'IOS_SIM arm2=PASS (animated pixels differ, app survived)';\necho 'IOS_SIM_RESULT=PASS (static + animated, screenshots under target/ios-sim/)'"
} else {
"echo 'Skipping ios-sim on this host: it needs a macOS host with Xcode and the iOS Simulator (xcrun simctl) plus the aarch64-apple-ios-sim target; on a Mac run: just ios-sim'"
} }}

[group("test")]
[doc("Executable iOS touch-and-resume coverage on the ALREADY BOOTED simulator named by <udid>: builds the Material demo for aarch64-apple-ios-sim, stages it into a minimal .app, and drives it through scripts/check-ios-input.py. The instrument is XCUITest, because nothing else can put a UITouch into the application: simctl has no touch subcommand, and host UI automation needs the Accessibility grant (and photographs the host's desktop) - XCUITest synthesizes the touch inside the simulator through the platform's own automation channel. The oracle is pixels, and has to be: the iOS backend publishes no accessibility tree, so a widget cannot be read by identifier. A real tap on a list row must change the displayed selection, Home-then-return must still display it, and two controls must behave - a fresh launch resets it (so the return comparison could have failed) and a tap on no target changes nothing (so the tap comparison distinguishes a hit from any touch). The demo's list rows are idempotent -- tapping an already-selected row changes nothing -- so retention here rests on display equality alone; a subject whose display keeps advancing can be held to the stronger oracle with `--post-return-tap`, which proves the resumed screen is live rather than the system's snapshot of the pre-Home frame. Exit 2 (CANNOT VERIFY) when the host cannot take the measurement. This closes the 'simulator UI automation timed out' gap in docs/BETA.md. macOS-host only; on a Mac run: just ios-input-check <udid>")]
ios-input-check udid:
    {{ if os() == "macos" {
"set -e\nBUNDLE=dev.flui.ios-demo\nAPP=target/ios-input/IosDemo.app\ncargo build -p flui --locked --features material --example ios_demo --target aarch64-apple-ios-sim\nrm -rf \"$APP\"\nmkdir -p \"$APP\"\ncp examples/Info.plist.ios_demo \"$APP/Info.plist\"\ncp target/aarch64-apple-ios-sim/debug/examples/ios_demo \"$APP/ios_demo\"\n\n# Remove any previous install before the run. xcodebuild installs the artifact\n# it was pointed at, and XCUIApplication launches whatever is installed under\n# that bundle id - so a stale copy left here could be the binary actually\n# measured. With it gone, a failed install means nothing launches and the probe\n# reports CANNOT VERIFY instead of testing the wrong build.\nxcrun simctl uninstall " + quote(udid) + " \"$BUNDLE\" 2>/dev/null || true\nrc=0\npython3 -B scripts/check-ios-input.py " + quote(udid) + " \"$APP\" || rc=$?\nif [ \"$rc\" -eq 2 ]; then\n  echo 'ios-input-check CANNOT VERIFY: this host could not take the measurement (no Xcode toolchain, the simulator was not booted, or the probe produced no report) - nothing was decided about the framework; details above'\nelif [ \"$rc\" -ne 0 ]; then\n  echo 'ios-input-check FAILED: a real touch did not reach a widget, or the state it changed did not survive Home/return, or a control did not behave - details and per-stage screenshots above'\nfi\nexit \"$rc\""
} else {
"echo 'Skipping ios-input-check on this host: it needs a macOS host with Xcode, the aarch64-apple-ios-sim target and an already booted simulator; on a Mac run: just ios-input-check <udid>'"
} }}

[group("test")]
[doc("Runs the same iOS touch-and-resume gate against an arbitrary staged .app, for candidates the CLI built rather than the in-repo demo. <app> is an already-staged .app directory (xcodebuild installs it) and <udid> an already booted simulator; extra arguments go to scripts/check-ios-input.py. Tap geometry is a property of the application, so a candidate whose widgets are not the demo's needs it: for the generated sole-flui counter, whose Increment button is at normalized y 0.097 and whose only changing text sits directly under the status-bar clock (so the region must be a centre band, or it crops out exactly what the tap changes): just ios-input-check-app <app> <udid> --target-tap 0.5,0.097 --empty-tap 0.5,0.5 --region 0.25,0.0,0.75,0.96 --post-return-tap. The last of those is the stronger resume oracle and the counter is the subject that can carry it: its button accumulates (0 -> 1 -> 2), so a tap after Home/return must advance the display, which a system snapshot of the pre-Home frame never does. It cannot be used on an idempotent subject -- the demo's rows -- where a second tap on the same row changes nothing and the requirement would fail a correct application; there the run's report names display-equality as the oracle that carried the claim. See scripts/check-ios-input.py for why each of those arguments exists. macOS-host only")]
ios-input-check-app app udid +ARGS:
    {{ if os() == "macos" {
"python3 -B scripts/check-ios-input.py " + quote(udid) + " " + quote(app) + " " + ARGS
} else {
"echo 'Skipping ios-input-check-app on this host: it needs a macOS host with Xcode, the aarch64-apple-ios-sim target and an already booted simulator; on a Mac run: just ios-input-check-app <app> <udid>'"
} }}

[group("test")]
[doc("Live iOS safe-area layout check: builds the sole-facade fixture for aarch64-apple-ios-sim through scripts/check-ios-safe-area.py and runs it on the ALREADY BOOTED simulator named by <udid>, asserting the marker the application writes after comparing both laid-out geometries against the view's own safeAreaInsets. Evidence belongs in docs/BETA.md § 'iOS safe-area layout'. Needs a macOS host with Xcode, the aarch64-apple-ios-sim target and a booted arm64 simulator; on a Mac run: just ios-safe-area-check <udid>")]
ios-safe-area-check udid:
    {{ if os() == "macos" {
"python3 -B scripts/check-ios-safe-area.py " + quote(udid) + " target/ios-safe-area-check"
} else {
"echo 'Skipping ios-safe-area-check on this host: it needs a macOS host with Xcode, the aarch64-apple-ios-sim target and an already booted simulator; on a Mac run: just ios-safe-area-check <udid>'"
} }}

[group("test")]
[doc("Run the workspace test scope used by CI (the flui-platform step needs xvfb-run on Linux — apt install xvfb; skipped with a message on other hosts)")]
test-ci:
    cargo nextest run --workspace --exclude flui-platform --locked --no-fail-fast
    # The facade defaults to Material only, so the run above skips
    # `tests/cupertino_demo.rs` (required-features) and the localizations
    # assertions in `tests/facade_smoke.rs`. Same precedent as CI's
    # flui-assets/flui-widgets feature-gated run.
    cargo nextest run -p flui --locked --features cupertino,localizations --no-fail-fast
    # Mirrors CI's dedicated flui-platform step, guarded by host OS:
    # `--all-features` is required just to compile the winit backend
    # (invisible under `default = ["desktop"]`); `FLUI_HEADLESS=1` routes
    # `current_platform()` to the
    # `HeadlessPlatform` mock so most of the suite needs no display server;
    # a handful of winit-internals unit tests construct `WinitPlatform::new()`
    # directly and need a real (if virtual) X11 connection for clipboard
    # init, which `xvfb-run` supplies — a Linux-only tool, hence the guard.
    # On Windows this is not a missing-tool gap: STATUS_HEAP_CORRUPTION
    # (H9, docs/ROADMAP-TRACKER.md) is an unresolved crash in this crate's
    # Windows backend, so the tests must not run there at all. 175/175
    # pass on Linux, 5x-verified stable — see docs/testing.md for what stays
    # excluded and why.
    {{ if os() == "linux" { "FLUI_HEADLESS=1 xvfb-run -a cargo nextest run -p flui-platform --locked --all-features --no-fail-fast" } else if os() == "windows" { "echo 'Skipping flui-platform tests: STATUS_HEAP_CORRUPTION (H9, docs/ROADMAP-TRACKER.md) is an unresolved Windows crash in this crate -- do not run its tests on a Windows host until that investigation lands a fix.'" } else { "echo 'Skipping flui-platform tests on this host: the CI-mirroring invocation needs xvfb-run (Linux-only) for the winit backend X11-dependent tests; see docs/testing.md.'" } }}

[group("test")]
[doc("Test a single crate (e.g. just test-crate flui-tree)")]
test-crate crate *args:
    cargo test -p {{crate}} {{args}}

[group("test")]
[doc("Run a single named test with stdout/stderr surfaced (e.g. just test-name flui-tree element_id)")]
test-name crate name:
    cargo test -p {{crate}} {{name}} -- --nocapture

[group("test")]
[doc("Run tests with debug logging (RUST_LOG=debug)")]
test-debug *args:
    RUST_LOG=debug cargo test --workspace {{args}}

[group("test")]
[doc("Run tests, keep going after the first failure")]
test-all:
    cargo test --workspace --no-fail-fast

# Excludes flui-platform to match the CI `test` job — that crate's suite is red
# independently of the profile (STATUS_HEAP_CORRUPTION investigation, see
# AGENTS.md), so including it would make this recipe permanently red and
# useless as a gate.
[group("test")]
[doc("Run tests against the release profile (excludes flui-platform, as CI does)")]
test-release:
    cargo test --workspace --exclude flui-platform --release

[group("test")]
[doc("Run rustdoc examples as tests (CI gate; nextest does not execute doctests)")]
test-doc:
    # flui-platform is not excluded: its doctests need neither
    # `--all-features` nor a display server (verified locally with and
    # without DISPLAY set), so there is no green-by-construction reason to
    # carve it out — see CI's `doc-test` job comment.
    cargo test --workspace --locked --doc

[group("test")]
[doc("Structural snapshots of the demo trees' painted layer trees (no GPU; CI runs these in the facade non-default-catalogs step)")]
demo-snapshots:
    # Both catalogs: the suite snapshots the Material and the Cupertino demo,
    # and Material is the facade default.
    cargo nextest run -p flui --locked --features cupertino --test demo_layer_snapshots

[group("test")]
[doc("Review and accept changed demo snapshots interactively (needs cargo-insta: cargo install cargo-insta)")]
demo-snapshots-review:
    # Never blanket-accept: a snapshot diff is the regression report, so read
    # it before blessing it. `cargo insta review` shows one diff at a time.
    cargo insta review

[group("test")]
[doc("Run the flui-assets/Image feature-gated tests CI also runs (default = [] hides them otherwise)")]
test-assets:
    cargo nextest run -p flui-assets --features full
    cargo nextest run -p flui-widgets --features images --test image
    cargo nextest run -p flui-widgets --features asset-images --lib
    cargo nextest run -p flui-widgets --features asset-images --test image_async
    cargo nextest run -p flui-widgets --features network-images --test image_network

[group("quality")]
[doc("Dependency audit: advisories, bans, licenses, sources (requires cargo-deny)")]
deny:
    cargo deny check

# SCOPE: widened from `pipeline::owner::subtree_arena` to `pipeline::owner`
# to pick up `cell.rs`'s PipelineCell checkout tests and two new
# real-NodePtr walks alongside the original subtree_arena suite. The
# `subtree_arena` unit tests still include the two pre-existing real-NodePtr
# walks that drive `layout_dirty_root` through every reborrow phase of
# `layout_subtree_borrowed_impl` — one straight parent→leaf pass, one cyclic
# `children()` edge that exercises the in-flight gate on the baseline
# callback (removing that gate makes miri fail this suite) — untouched by
# the widening. New alongside them: an owner-local traversal (a full
# run_frame over a real 3-node tree, driven through PipelineCell::with_mut)
# and a reentrant-layout walk (a Sliver child that issues a mid-layout
# child-build request against the checked-out owner, then mark_needs_layout
# right after). Measured at 55 tests / ~21s wall, in budget. Still narrow:
# only `pipeline::owner`, only box + leaf-sliver
# layout — deeper sliver walks and intrinsics queries are not interpreted
# here.
#
# The second invocation covers flui-view's GlobalKey plane (ADR-0050): the
# identity registry, the cross-owner claim table, the per-frame reservation
# ledger, and the tree-driving verification/repair tests that put a real
# `ElementTree` through `BuildOwner::finalize_tree`. Measured 28 tests / ~7s.
#
# The third covers flui-engine's surface lease (issue #1043/#1149): the whole
# wgpu-free `SurfaceLease` protocol at `wgpu/surface_lease.rs`, which is where
# the two test-only `borrow_raw` calls and the surface-before-target drop
# order live; plus `cancelling_renderer_new_mid_flight_releases_the_target`,
# which drives the same `probe_then_build` seam with a never-resolving builder
# and so never touches wgpu either. `SurfaceLease` is generic over its surface
# type precisely so this runs without a GPU. Measured 7 tests / ~4s warm.
[group("test")]
[doc("Run miri on flui-rendering's pipeline::owner, flui-view's owner::global_key, and flui-engine's GPU-free surface-lease tests (requires nightly + miri)")]
miri:
    cargo +nightly miri test -p flui-rendering --lib pipeline::owner
    cargo +nightly miri test -p flui-view --lib owner::global_key
    cargo +nightly miri test -p flui-engine --lib wgpu::surface_lease
    cargo +nightly miri test -p flui-engine --lib cancelling_renderer_new

# Local mirror of the weekly `nightly-canary` job. Advisory: nightly is where
# the next stable's deprecations, new lints, and future-incompat errors show
# up first. Its own target dir keeps nightly artifacts from evicting the
# stable build in `target/debug` (a different rustc means a full rebuild in
# both directions).
[group("quality")]
[doc("Nightly canary: check + clippy the workspace on nightly, then print the future-incompat report (advisory; mirrors the weekly job)")]
nightly-check:
    CARGO_TARGET_DIR=target/nightly CARGO_BUILD_WARNINGS=warn cargo +nightly check --workspace --all-targets --locked
    CARGO_TARGET_DIR=target/nightly CARGO_BUILD_WARNINGS=warn cargo +nightly clippy --workspace --all-targets --locked
    CARGO_TARGET_DIR=target/nightly cargo +nightly report future-incompatibilities || true

[group("test")]
[doc("Generate an HTML coverage report (requires cargo-llvm-cov)")]
coverage:
    cargo llvm-cov --workspace --html
    @echo "Coverage report: target/llvm-cov/html/index.html"

# =============================================================================
# Quality gates
# =============================================================================

[group("quality")]
[doc("Run clippy exactly as CI does: workspace, then flui-engine's GPU-gated code")]
clippy:
    # Both invocations, both `--locked`, because that is what the CI job runs.
    # The second one is not optional: `testing` gates a body of code
    # -- the readback suite and the deterministic-replay tests -- that the
    # workspace pass never compiles, so a break there is invisible until CI.
    # A marker sweep missed an entire file for exactly this reason.
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo clippy -p flui-engine --all-targets --locked --features testing -- -D warnings

[group("quality")]
[doc("Run clippy and apply auto-fixes (uncommitted changes only)")]
clippy-fix:
    cargo clippy --workspace --all-targets --fix --allow-dirty -- -D warnings

[group("quality")]
[doc("Per-feature clippy via cargo-hack (mirrors the CI feature-matrix job; requires cargo-hack)")]
feature-matrix: facade-combos
    cargo hack clippy --workspace --locked --each-feature --optional-deps --keep-going -- -D warnings
    cargo hack clippy --workspace --locked --each-feature --optional-deps --keep-going --tests --benches --examples -- -D warnings

[group("quality")]
[doc("Compile every supported facade feature combination in isolation")]
facade-combos:
    #!/usr/bin/env bash
    # Each combination is its own cargo invocation on the `flui` package alone.
    # A `--workspace` build proves nothing here: workspace feature unification
    # would enable `material` from a sibling and turn a broken combination
    # green. `--all-targets` is deliberate — a missing `required-features` on an
    # example or test is exactly the kind of wiring these builds exist to catch.
    set -euo pipefail
    for combo in "--no-default-features" \
                 "--no-default-features --features material" \
                 "--no-default-features --features cupertino" \
                 "--no-default-features --features material,cupertino" \
                 "--no-default-features --features localizations" \
                 "--no-default-features --features material,localizations" \
                 "--no-default-features --features cupertino,localizations" \
                 "--no-default-features --features material,cupertino,localizations" \
                 "--no-default-features --features hot-reload" \
                 "--no-default-features --features serde" \
                 "--all-features" \
                 ""; do
        echo "==> cargo clippy -p flui --locked --all-targets ${combo:-(default features)}"
        # shellcheck disable=SC2086
        cargo clippy -p flui --locked --all-targets $combo -- -D warnings
    done
    # Hot reload must be absent from an ordinary production graph, not merely
    # unused by it.
    echo "==> cargo tree -p flui-app: flui-hot-reload must be absent"
    if cargo tree -p flui-app --locked -e normal | grep -q flui-hot-reload; then
        echo "flui-hot-reload is in flui-app's default normal dependency graph" >&2
        exit 1
    fi
    if ! cargo tree -p flui-app --locked -e normal --features hot-reload | grep -q flui-hot-reload; then
        echo "the hot-reload feature did not bring in flui-hot-reload" >&2
        exit 1
    fi
    # The first-party host is the executable contract for `flui run`. A direct
    # dependency on flui-hot-reload does not activate flui-app's feature.
    if ! cargo tree -p hot-reload-counter-host --locked -e features -i flui-app \
        | grep -q 'flui-app feature "hot-reload"'; then
        echo "hot-reload-counter-host does not enable flui-app/hot-reload" >&2
        exit 1
    fi

[group("quality")]
[doc("Format the entire workspace with rustfmt")]
fmt:
    cargo fmt --all

[group("quality")]
[doc("Check formatting without modifying files (CI gate)")]
fmt-check:
    cargo fmt --all -- --check

[group("quality")]
[doc("Build rustdoc for FLUI crates only")]
doc:
    cargo doc --workspace --no-deps

[group("quality")]
[doc("Build rustdoc and open in browser")]
doc-open:
    cargo doc --workspace --no-deps --open

[group("quality")]
[doc("Build rustdoc with -D warnings (CI gate)")]
doc-strict:
    bash scripts/doc-strict.sh

[group("quality")]
[doc("Check crate inventories + the docs/workspace-layers.toml layer policy against Cargo metadata")]
inventory-check: release-policy-test
    bash scripts/check-workspace-inventory.sh

[group("quality")]
[doc("Validate the Runtime contract registry (docs/runtime-contract.toml) against the source tree")]
runtime-conformance-check:
    bash scripts/check-runtime-conformance.sh

[group("quality")]
[doc("Gate the expect(\"BUG: …\") panic-policy convention (docs/PANIC-POLICY.md) via its per-file ratchet")]
panic-policy-check:
    bash scripts/check-panic-policy.sh

# =============================================================================
# Port methodology
# =============================================================================

[group("port")]
[doc("Run refusal-trigger grep regressions (22 triggers + named guards from docs/PORT.md)")]
port-check:
    bash scripts/port-check.sh

[group("port")]
[doc("Run refusal-trigger checks with verbose pass/fail per trigger + marker totals")]
port-check-verbose:
    bash scripts/port-check.sh -v

# Advisory by design, and NOT part of `just ci`. It reports a LOWER BOUND on
# citation rot: an out-of-range line or a vanished path is provably stale, but a
# line still in range is only "not disproved" -- lines move under edits far more
# often than files shrink. Gating on a number that cannot see most of its own
# subject would buy false confidence, not accuracy (issue #993).
[group("docs")]
[doc("Measure provably-stale line-number citations in docs/adr/ (issue #993)")]
adr-citations *args:
    python3 scripts/check-adr-citations.py {{args}}

[group("port")]
[doc("Per-file breakdown of TODO(port) / PERF(port) / PORT NOTE markers across crates/")]
port-markers:
    bash scripts/port-check.sh -b

# =============================================================================
# Benchmarks
# =============================================================================

[group("bench")]
[doc("Run benchmarks for a single crate (criterion)")]
bench crate:
    cargo bench -p {{crate}}

[group("bench")]
[doc("Run benchmarks across the workspace")]
bench-all:
    cargo bench --workspace

# Criterion writes baselines under target/criterion; this quiet dev box is
# where real A/B comparisons belong (CI runners are noisy — the weekly job
# only uploads trend artifacts, it never compares or gates). The script
# enumerates the real [[bench]] targets because plain `cargo bench` also
# selects libtest-harness targets, which reject criterion CLI flags.
[group("bench")]
[doc("Run every criterion bench and save the numbers under a named baseline (e.g. just bench-save before-fix)")]
bench-save name:
    bash scripts/bench-collect.sh {{name}}

[group("bench")]
[doc("Compare two saved baselines (requires critcmp: cargo binstall critcmp)")]
bench-compare old new:
    critcmp {{old}} {{new}}

# =============================================================================
# Examples
# =============================================================================

[group("examples")]
[doc("Run the hello_world platform smoke test")]
example-hello:
    cargo run --example hello_world

[group("examples")]
[doc("Run an example by name (e.g. just example direct_render)")]
example name:
    cargo run --example {{name}}

[group("examples")]
[doc("Run the desktop_scene hot-reload example")]
example-desktop-scene:
    cargo run -p desktop_scene

[group("examples")]
[doc("List all available examples")]
example-list:
    @ls examples/*.rs 2>/dev/null | xargs -n1 basename | sed 's/\.rs$//'
    @echo "(plus per-target crates under examples/: desktop_scene, web_demo, painting_demo, android_*)"

# =============================================================================
# Web / WASM
# =============================================================================

[group("web")]
[doc("Run the built-in dev server (wasm-pack + HTTP serve)")]
web-server:
    cargo run -p web-server

[group("web")]
[doc("Build examples/web_demo to WASM (requires wasm-pack)")]
web-demo-build:
    cd examples/web_demo && wasm-pack build --target web --out-dir pkg

[group("web")]
[doc("Build examples/painting_demo to WASM (requires wasm-pack)")]
painting-demo-build:
    cd examples/painting_demo && wasm-pack build --target web --out-dir pkg

# =============================================================================
# Android (NDK)
# =============================================================================

[group("android")]
[doc("Build the Android GPU demo for arm64 (requires cargo-ndk + Android NDK)")]
android-demo target="arm64-v8a":
    cargo ndk -t {{target}} build -p flui-android-demo

[group("android")]
[doc("Build the Android scene plugin (requires cargo-ndk + Android NDK)")]
android-scene target="arm64-v8a":
    cargo ndk -t {{target}} build -p flui-android-scene

[group("android")]
[doc("Build the widget-based Android plugin (requires cargo-ndk + Android NDK)")]
android-app target="arm64-v8a":
    cargo ndk -t {{target}} build -p flui-android-app

# =============================================================================
# Setup
# =============================================================================

[group("setup")]
[doc("Install development tools used by the workspace")]
setup:
    rustup component add clippy rustfmt
    cargo install --locked cargo-llvm-cov
    cargo install --locked cargo-watch
    @echo ""
    @echo "Optional, for cross-target builds:"
    @echo "  cargo install --locked wasm-pack       # for examples/web_demo, examples/painting_demo"
    @echo "  cargo install --locked cargo-ndk        # for examples/android_*"
    @echo "  cargo install --locked cargo-hack       # for just feature-matrix (CI per-feature gate)"
    @echo "  cargo install --locked zizmor           # workflow security audit (CI checks gate)"

[group("setup")]
[doc("Show installed Rust toolchain and FLUI workspace info")]
info:
    @rustc --version
    @cargo --version
    @echo "Active workspace members: {{active_crates}}"
    @echo "Version: {{version}} (commit {{commit}})"

# =============================================================================
# Watch mode
# =============================================================================

[group("watch")]
[doc("Re-run check on file change (requires cargo-watch)")]
watch:
    cargo watch -x "check --workspace"

[group("watch")]
[doc("Re-run tests on file change (requires cargo-watch)")]
watch-test crate="":
    cargo watch -x "test {{ if crate == '' { '--workspace' } else { '-p ' + crate } }}"

# =============================================================================
# CI aggregate
# =============================================================================

# `just ci` stays the FAST local gate on purpose. The heavy CI-only jobs have
# their own recipes — run them deliberately before pushing risky changes:
#   just feature-matrix   (per-feature clippy, minutes)
#   just wasm-check       (wasm32 target check)
#   just cross-typecheck  (windows + macos + android backends, type-check only)
#   just deny             (advisories / bans / licenses / sources)
#   just miri             (nightly UB check, narrow scope — see its comment)
# Everything in `ci` except the test suites: ~2 minutes on a warm tree. This is
# what the pre-push hook runs for code/config changes; markdown-only pushes use
# the hook's text fast path. Every gate this repository lost time to recently
# was caught by something in here, not by a test.
[group("ci")]
[doc("Spell-check (typos) and TOML formatting (taplo) — the two CI gates with no cargo step")]
text-check:
    #!/usr/bin/env bash
    set -euo pipefail
    # Skipped with a message rather than failing when the tool is absent: these
    # are two extra binaries, and a contributor without them should still be
    # able to run `just gate`. CI installs both, so a skip here is a slower
    # feedback loop, never a hole in the gate.
    if command -v typos >/dev/null 2>&1; then
        typos
    else
        echo "typos: not installed, skipped (cargo install typos-cli)"
    fi
    if command -v taplo >/dev/null 2>&1; then
        taplo fmt --check
    else
        echo "taplo: not installed, skipped (cargo install taplo-cli)"
    fi

[group("ci")]
[doc("The non-test half of `ci` — what the pre-push hook runs")]
gate: fmt-check text-check font-assets-check inventory-check runtime-conformance-check panic-policy-check port-check clippy doc-strict

[group("ci")]
[doc("Run local CI gates (gate + test + doctests)")]
ci: gate test-ci test-doc

# =============================================================================
# Maintenance
# =============================================================================

[group("maintenance")]
[doc("Point git at the repo's checked-in hooks (runs `just gate` before every push)")]
install-hooks:
    git config core.hooksPath scripts/githooks
    @echo "core.hooksPath -> scripts/githooks (git push --no-verify still bypasses it)"

[group("maintenance")]
[doc("Prune stale build artifacts: current-toolchain sweep + anything older than 7 days (requires cargo-sweep)")]
sweep:
    cargo sweep --installed
    cargo sweep --time 7

[confirm("Remove target/ and all build artifacts?")]
[group("maintenance")]
[doc("Wipe target/ directory and Cargo build artifacts")]
clean:
    cargo clean

[group("maintenance")]
[doc("Update workspace dependencies (Cargo.lock)")]
update:
    cargo update --workspace

[group("maintenance")]
[doc("Audit dependencies for known vulnerabilities (requires cargo-audit)")]
audit:
    cargo audit

[group("maintenance")]
[doc("Show outdated dependencies (requires cargo-outdated)")]
outdated:
    cargo outdated --workspace

[group("quality")]
[doc("Test release roles, declaration closure and Cargo archive normalization on tiny fixtures")]
release-policy-test:
    python3 -B -m unittest discover -s scripts/tests -p test_release_policy.py

[group("quality")]
[doc("Show the computed product/support release inventory without creating archives")]
release-inventory:
    python3 -B scripts/release_policy.py --json

[group("quality")]
[doc("Create and inspect local archives for the selected release set; no build or upload")]
release-package-check *options:
    python3 -B scripts/release_policy.py --package {{options}}

[group("quality")]
[doc("Build and test a fresh CLI-generated consumer against the release ARCHIVES, offline: packages the release set, vendors every third-party dependency, installs the archives as a Cargo directory source, and runs `flui create` without --local so the registry dependency form is what gets resolved. Pass --preview-dirty on an uncommitted tree. Slow (vendors the whole dependency set) and disk-hungry (target/release-consumer)")]
release-consumer-check *options:
    python3 -B scripts/release_consumer_check.py {{options}}

[group("quality")]
[doc("Verify font provenance/notices, generated fixture bytes, and Cargo package file selection offline")]
font-assets-check:
    python3 -B -m unittest discover -s scripts/tests -p test_font_assets.py
    python3 -B scripts/font_assets.py --package-list
