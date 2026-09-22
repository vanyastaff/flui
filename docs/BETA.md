# FLUI beta release

The next product milestone is a beta of a cross-platform, Rust-native declarative
UI framework that developers and AI agents can use to build and verify real
applications. This is the active release objective as of 2026-09-19.

**Status: preparation, not release-ready.** The workspace version is not a
readiness verdict. Historical completion notes require fresh verification on the
release candidate. No platform is certified by this document yet.

## What beta must demonstrate

**Application dependency contract:** application developers use one direct
framework dependency, `flui`, and its public re-exports. Generated applications,
derives, hot-reload authoring, and documented ordinary UI examples must work
through that facade without requiring direct `flui-*` implementation dependencies.
Verify this in consumer projects outside the workspace, including a renamed
`flui` dependency; tests within the facade package can accidentally see its
implementation dependencies and conceal broken macro expansion.

The workspace and the crates.io publication set are separate. Define the release
package set explicitly from the facade's required dependency closure and selected
tooling; do not publish every workspace member automatically. A required registry
dependency must be available to Cargo even when it is an implementation package,
so `publish = false` is not a substitute for designing that closure.

| Area | Observable release condition | Evidence to collect |
|---|---|---|
| First application | A fresh consumer can create, compile, run, edit, test, and build an application using only the public documentation and declared dependencies. | Clean-directory CLI/template tests and a live first-run check; no unpublished registry dependencies or accidental workspace feature unification. |
| Application behavior | A representative application supports navigation, editable forms, validation, scrolling lists, async loading/error/retry, and retained state across rebuilds. | Public-API integration tests plus real-window input and pixel checks. Include cancellation, unmount, resize, focus changes, and teardown. |
| Text and accessibility | Text editing handles grapheme clusters, selection, clipboard, and IME composition; controls expose useful roles, names, states, and actions. | Editing regressions, semantics tests, and native IME/assistive-technology checks on each advertised platform. A headless semantics tree alone does not prove native accessibility. |
| Platform behavior | Every platform advertised as beta runs the application, accepts its native input, resizes, suspends/resumes where applicable, and exits cleanly. | Per-platform execution evidence below. Compilation alone cannot certify runtime support. |
| Developer iteration | Documented reload modes apply edits predictably and state preservation matches their advertised contract. Failed edits can be corrected without corrupting the running application. | Tests and live checks for repeated edits, idle applications, failed builds, state preservation, and shutdown. Publish target-specific limitations. |
| Agent workflow | An agent can discover the public API, create a UI, inspect structure/semantics, drive an interaction, and assert the result through documented interfaces. | A reproducible consumer example using the existing diagnostics and testing APIs, with meaningful assertions and actionable command failures. |
| Performance and resilience | Static applications become idle; representative scrolling and editing workloads have recorded frame timing and memory behavior; supported recovery paths work. | Reproducible workload, hardware/OS, build profile, timing distribution, memory measurements, and explicit budgets chosen before acceptance. No invented performance claim. Recorded for macOS in ["Performance and resilience: the representative workload — 2026-09-22"](#performance-and-resilience-the-representative-workload--2026-09-22) (`just macos-workload`); recovery paths are the device-loss and surface-recreation retries, exercised by their unit tests and not yet by a live fault. |
| Distribution | The candidate installs and builds outside this checkout, with its full dependency closure available through the chosen distribution channel. | Package/dependency audit, clean consumer build, licenses, changelog, version/migration notes, and reproducible release instructions. |

The user-facing mental model remains declarative composition over the retained
View → Element → Render trees, with keys and lifecycle. Flutter is a useful
behavioral reference; copying its entire catalog is not the beta acceptance
condition. Deliberate contract differences still need an ADR or mapping decision
and a test, as required by [AGENTS.md](../AGENTS.md).

## Platform evidence

### Application lifecycle acceptance

Lifecycle is a framework contract exposed through `flui`, including startup,
window close versus application quit, minimize/restore, focus gain/loss,
hide/show, continued work without visible windows, reopening the interface,
and platform suspension/resumption. Window visibility, input focus, rendering
eligibility, and process lifetime must remain distinct. Background services
follow the host OS's execution rules; keeping a desktop process alive does not
prove mobile background execution.

Verify each transition through a public consumer, including multiple windows
and realms, repeated notifications, cancellation, and teardown. Explicit quit
must notify every surviving realm and finish application services. A resident
application must be able to reopen its UI and explicitly quit through public
capabilities. Native activation/reopen and background-launch behavior need their
own live checks. The macOS shutdown probes below cover only their named cases.

Desktop quit now has a loop-owned notification walk across surviving realms,
including when the primary realm was removed. Application seam regressions cover
shared realms, deferred installs, rejected late secondary completions, reentry,
and observer/dispatch panic ordering. This closes the primary-only notification
gap. Presentation-owned focus/visibility now drive local binding notifications,
input cancellation and resource suspension, while each realm derives its frame
eligibility from its live presentations. Scoped regressions cover separate and
shared realms, both focus-event orders, pause/resume, restoration redraw, and
terminal notification before disposal even when an observer panics. Public
weak lifecycle subscriptions now have mounted app and sole-facade headless
regressions, including renamed dependencies. Native lifecycle transport across
every supported platform remains separate acceptance work.

The native macOS `on_reopen` transport now routes both visible-window and
no-window reopen events through the owned application delegate. The bounded
`reopen_probe` separates direct selector tests from actual `open -a` AppleEvents;
the latter require callback delivery and normal return in the original process.
Its direct cases cover starting-phase delivery, nested pumping, callback
replacement and destruction, hostile panic payloads, quit fences and stale
per-run delegates. AppKit's default untitled-document creation is suppressed;
the callback receives a signal and owns the decision to show or create UI.

The native signal is one part of resident application support. The rendered
main-window factory, public control and window-independent pending creation are
now covered by the resident validation below. Rendered secondary-window content
and native background-launch rendering remain separate workflow gaps.

Primary references: [AppKit last-window termination policy](https://developer.apple.com/documentation/appkit/nsapplicationdelegate/applicationshouldterminateafterlastwindowclosed(_:))
separates window closure from application termination; [winit application lifecycle](https://docs.rs/winit/0.30.13/winit/application/trait.ApplicationHandler.html)
documents platform-specific suspend/resume and redundant notifications. These
inform the acceptance cases, not a claim that FLUI already implements them all.

### Candidate evidence records

Track macOS, Windows, Linux, Android, iOS, and Web separately. For each candidate,
record the commit, OS/device/browser, toolchain, backend, command, test result,
and artifact or log location. Distinguish physical devices from simulators,
and X11 from Wayland. Record the public feature set actually exercised.

Each platform receives one explicit status: **unverified**, **blocked** (with a
reproducer), **experimental** (limitations published), or **beta verified**
(all advertised workflows passed). Start all rows unverified for a new candidate;
reuse evidence only when its applicability to the candidate is explained.
Experimental support must be visible in installation instructions and release
notes. Narrowing beta platform scope is a product decision, not a way to turn a
failed check green.

## Platform status — candidate: this branch at `v0.1.0` and after

None of these are **beta verified**: none has passed every advertised workflow
in [What beta must demonstrate](#what-beta-must-demonstrate). "Beta candidate"
below means the strongest evidence recorded is live, operator-equivalent input
on a real OS, not that beta acceptance is complete.

| Platform | Status | Evidence | Published limitations |
|---|---|---|---|
| macOS (AppKit, Metal, ARM64) | **beta candidate** | Live operator-equivalent input through real OS channels — `CGEventPost`/`CGHIDEventTap` clicks and `CGWindowListCopyWindowInfo` capture, no accessibility-tree shim (["Live verification through direct OS interaction — 2026-09-20"](#live-verification-through-direct-os-interaction--2026-09-20)); native close/quit/reopen (["Native macOS last-window exit"](#native-macos-last-window-exit), `exit_policy_probe`, `just macos-close-path`); launch-route rendering across direct exec, `open`, and `open -g` (`just macos-launch-render`, cited in the same live-verification section); the deferred-first-reveal fix for the white-window observation, commit `fd9f2938` ("Reveal a macOS window only once its first frame has been presented", `PlatformWindow::reveal_after_first_frame` / `FirstReveal`); IME routing/protocol coverage via the `just macos-ime` script (`ime_probe`, ADR-0069) | No native accessibility/assistive-technology check on any workflow; no physical Cmd+Q or menu-bar routing, nested modal loops, or foreign-loop embedding (only programmatic quit/terminate paths are proven); `just macos-ime` exercises routing and the `NSTextInputClient` protocol with synthesized key events — no genuine input method runs, so real IME composition is unverified; `open_window`'s `SharedRealm` policy is refused at admission (only `SeparateRealms` has content); a single unattributed first-run flake is recorded in "Live verification" and not reproduced; clipboard and OS suspend/resume are unverified |
| iOS Simulator (iPhone 16e, iOS 26.2) | **experimental** | Touch input and Home/return state retention via XCUITest, since the backend publishes no accessibility tree (["iOS execution lifecycle foundation"](#ios-execution-lifecycle-foundation), `just ios-input-check`, `scripts/check-ios-input.py`); safe-area inset layout (["iOS safe-area layout"](#ios-safe-area-layout), `just ios-safe-area-check`); scene disconnect/reconnect protocol probe (["UIKit scene ownership"](#uikit-scene-ownership)) | No physical device tested; the oracle is pixels only (no a11y tree, so nothing is read by identifier); no IME or keyboard check; landscape orientation, keyboard occlusion, and other device classes are untested; background execution grants and full multiwindow/background-launch rendering remain unverified; the measured counter bundle was the CLI's 2026-09-19 build, not a fresh build of the current revision; `just ios-sim` (static Material app renders, animated app's pixels change between two screenshots 2 s apart and the process survives) was re-run on 2026-09-22 at `51c8fe63` on an iPhone 17 Pro simulator and passed both arms — the XCUITest touch check was not re-run |
| Linux (X11 / Wayland) | **experimental** | CI-executed live smoke only: the `live-smoke` job in `.github/workflows/ci.yml` builds `flui`'s `sliver_demo` example and `flui-live-smoke`, then drives a real window with real X11 input under Xvfb (pixel and exit-code checks, occlusion verified against a real cover window), plus a Wayland variant under headless weston for close-path teardown ordering (`live-smoke` / `live-smoke-wayland` recipes in `justfile`; also described in the README under "Resilience that is tested, not assumed") | No operator-equivalent input verification as used on macOS (only the harness's synthetic/scripted input); no IME check; no resident/background lifecycle coverage; native accessibility bridges are not exercised by this job; X11 and Wayland coverage differ in scope (Wayland covers only close-path teardown ordering) |
| Windows (Win32) | **unverified** | Cross-compiled Clippy only: `just cross-typecheck` runs `cargo clippy -p flui-platform --target x86_64-pc-windows-msvc --features a11y` | No live window, input, lifecycle, or IME verification has been performed on Windows for this candidate; the "Window-independent owner turns" and "Resident main-window validation" sections explicitly note Windows show/worker paths as cross-compilation evidence only |
| Android (emulator, android-35 arm64) | **experimental** | First emulator run, 2026-09-22, from the CLI's `flui build android` / `flui run --device` work in the peer session (generated counter with `android_main`, `cargo ndk` arm64-v8a debug APK, android-35 google_apis arm64 on Apple Silicon, `-gpu host`, density 420, 1080×2400): first frame ~10 s after launch, the counter visible; two `adb shell input tap 540 1284` on Increment showed «2» on the screenshot taken right after — once the backend handed the framework logical pointer positions (`bf2725be`; before it every touch landed past the viewport's edge, diagnosed through the `28e048f1` first-motion-event trace: `x=540.0 y=1284.0 scale_factor=2.625`). Cross-compiled Clippy for `aarch64-linux-android` in `just cross-typecheck`. | One emulator, one host, debug build, an `adb` tap rather than a finger; no lifecycle (pause/resume/rotate) or keyboard verification; on `-gpu swiftshader_indirect` a debug build produced no first frame in four minutes (process at ~80 % CPU after "Selected GPU: SwiftShader", no errors in logcat) — software rendering is unverified; the debug APK is 406 MB because the `.so` ships uncompressed with full debug info; the automatic-retry surface-recreation backoff (`21af0752`) is host-tested against a scripted backend only, not a real device or emulator failure |
| Web / WASM | **experimental** | The counter template's widget tree run through `flui::run_app` in a browser (`examples/web_counter`, `just web-counter-build`, WebGPU): rendered, three clicks on Increment advanced 0 → 3, a click with no target changed nothing, no console errors — see ["Web: the counter in a browser"](#web-the-counter-in-a-browser). Compile coverage stays `just wasm-check`. | One browser (the desktop app's Chromium-based pane) on one machine, served from `localhost`; no Firefox/Safari, no WebGL fallback (WebGPU only), no touch, no IME, no resize/visibility lifecycle check, hot-reload has no web runner. The shader uniformity defect this run exposed is fixed and guarded by `scripts/check-wgsl-uniformity.py`, whose rule is structural, not Tint itself. |

## Verification order

1. Establish a baseline with `just ci`. Record missing tools and skipped checks.
   Run the additional relevant platform, feature, security, and live checks from
   [Testing](testing.md) and the `justfile`; `just ci` alone does not cover them.
2. Verify the fresh-consumer workflow and correct broken setup instructions or
   generated projects before expanding the catalog.
3. Build the representative application from public APIs and turn observed
   failures into regression tests at the shallowest effective testing tier.
4. Close runtime, input, accessibility, lifecycle, and diagnostics defects in
   those workflows. Validate on each target rather than inferring from macOS.
5. Measure performance, exercise failure recovery, and audit distribution.
6. Re-run candidate checks, publish limitations and migration notes, then create
   the release. An unchecked item stays open; a passing unrelated test cannot
   substitute for its evidence.

## Ecosystem references

Current primary sources consulted on 2026-09-19 inform the acceptance bar:

- [Slint UI testing](https://testing.slint.dev/) exposes UI inspection and actions
  through accessible properties. FLUI's agent workflow should likewise prove
  that an interaction changes observable state, using its own testing layers.
- [Dioxus hot reload](https://dioxuslabs.com/learn/0.7/essentials/ui/hotreload/)
  treats iteration as part of the development workflow. FLUI needs an exercised
  edit/reload loop with an explicit state-preservation contract.

These references motivate workflow checks, not adoption of their architectures.
Use [Testing](testing.md) to reuse existing FLUI diagnostics, headless frames,
semantics queries, gesture replay, and live smoke infrastructure.

## Initial baseline — 2026-09-19

Starting revision: `fbed9408` on macOS ARM64. The first local `just ci` run
required Python 3.12 on `PATH` instead of the system Python 3.9, and execution
outside the restricted sandbox for the text-check tools. Formatting, text,
workspace inventory, runtime conformance, panic policy, and port checks passed.
The runtime registry reports 32 implemented, 22 partial, and 4 planned contracts;
a passing registry check validates consistency, not completion of those contracts.

The run then failed during Clippy compilation because the committed macOS
configuration required an unavailable `lld`. The full test and rustdoc stages
were not reached. This baseline must not be reported as a green CI run.

The forced Darwin linker flags have now been removed in favor of the default
Apple toolchain. `cargo build -p flui-macros --locked` passes after that change.
Verification also encountered a full disk; cleaning only the nested hot-reload
fixture's compiled artifacts freed space while preserving its source files.
A successful proc-macro build proves the default linker works here, not that
the workspace or any platform's release workflows pass.

The subsequent workspace and GPU-feature Clippy invocations both passed.
That run's rustdoc stage was interrupted when a concurrent Cargo cleanup
removed shared host artifacts as well as the requested target's artifacts.
It is invalid as a complete CI result and must be repeated with no concurrent
cleanup. Build-cache maintenance must be serialized with verification.

The CLI's local dependency layout defect is repaired: `--local` selects the
current source checkout, and `--local=PATH` selects an explicit one. Generated
dependencies use validated absolute paths rather than fixed parent traversals.
The regression failed on the old relative path and passed with the fix; actual
Cargo checks of basic, counter, and hot-reload projects in external temporary
directories all passed on macOS. See the [CLI mapping decision](../crates/flui-cli/ARCHITECTURE.md).
This does not yet prove a complete first-run or published-install workflow.
The four derives now resolve runtime paths through Cargo dependencies, including
renamed facades and direct owning crates. Isolated facade consumers and owner
library tests pass. Templates now declare only `flui`; their manifest regression
passed after first failing on the old internal-crate dependencies. External
Cargo checks of all three updated template shapes passed on macOS; the full
CLI suite passed 41 unit and 45 integration tests, followed by strict scoped
Clippy. Normal dependency graphs exclude hot-reload with default or no-default
features and include it only when enabled. Live first-run and platform verification
remain outstanding.

`TextEditingController` now moves, extends and deletes by extended grapheme
cluster, and the obscured-field mask counts the same unit (`unicode-segmentation`,
already in the graph via cosmic-text). The ZWJ-family, regional-indicator-flag
and combining-mark cases are headless regressions in `controller.rs` and
`editable_text.rs`; each fails when the helper is swapped back to
`char_indices`. Live IME and clipboard checks remain separate.

The default counter template owns its count in retained state and provides a
Material Increment button. Its generated test sends pointer down/up events and
checks rendered text 0 → 1 → 2 using scheduled frame ticks. A parent rebuild
preserves 2, while an independent application starts at 0. The external CLI test
runs that named test in a fresh generated project, checks its normal build, and
verifies that `flui-testing` is absent from the normal dependency graph. Testing
is enabled only through the generated `flui` development dependency. This is
headless interaction evidence; live first-run verification remains separate.
The external regression first failed against the static template; the updated
creation suite passes 20 tests, including generated basic, counter and hot-reload
projects. Removing the generated callback's rebuild scheduling fails at
“counter should display 1”; restoring it passes. Scoped CLI Clippy and generated
source formatting checks pass. Evidence: `/tmp/flui-counter-red.log`,
`/tmp/flui-counter-cli-suite.log`, `/tmp/flui-counter-mutation.log`,
`/tmp/flui-counter-restored.log`, and `/tmp/flui-counter-clippy.log`.

Publication graph audit (manifests, not a successful package dry run): the facade
currently reaches 20 workspace packages without default features, 21 with
Material, and 24 with all advertised runtime features. Including inactive
optional normal dependencies raises the manifest closure to 26: `flui-widgets`
also declares `flui-assets` and `flui-testing`. Those edges must be accounted for
before marking either package nonpublishable. CLI/build/devtools and example/tool
packages are outside the facade runtime closure; normalized dev-dependencies,
embedded assets, archive contents, and publication order still need verification.
This inventory does not designate implementation crates as application APIs.

Release archive audit remains open. The checked release inventory identifies 29
product/support packages (the facade, CLI, and 27 required implementation
packages) and 12 private example/tool packages: nine workspace members and three
excluded Android packages, all marked `publish = false`. The root archive listing
includes 633 files, including IDE and internal CI/documentation files, so explicit
package inclusion rules are still needed. The restricted painting test fixture `Arial.ttf` has been
removed and replaced by the generated first-party FLUI Probe Sans fixture. Roboto's exact bytes now match the official v2.138 Android archive; the
Cupertino and Material font sources have also been identified at pinned revisions.
Full upstream license texts, project license copies for generated fixtures,
and font copyright attributions now live inside painting's font assets.
These observations are an archive audit, not a successful publication dry run.


## Release inventory and validation boundary

`docs/workspace-layers.toml` records product/support roles alongside the existing
layer inventory. `just release-inventory` prints the computed package set,
private packages, dependency paths, and retained dependency cycles. Required
implementation packages remain registry-distributed support, not separate
application APIs. `just inventory-check` verifies this policy and exercises
Cargo normalization using tiny temporary packages; it does not package FLUI.

The release closure includes optional and target-specific normal/build edges,
and dev-dependencies that Cargo retains because their resolved declaration has a
version. Versionless dev-dependencies are omitted. Workspace inheritance and
renamed package identities are resolved before checking. The workspace is now
version `0.1.0`, and every internal requirement pins that exact
cohort version (`=0.1.0`), so a published facade can never resolve a
sibling from a later cohort.

Twelve backward or self dev declarations are explicitly checkout-only in
`docs/workspace-layers.toml`: six self feature activations, foundation → macros,
rendering → objects, and interaction/scheduler/view/devtools → testing. Their
local paths and features are preserved, but Cargo omits these unversioned dev
declarations from published manifests. All normal/build dependencies and forward
dev dependencies retain their versions. The checker validates the exact source,
package identity, alias, target, and resolved path before omission; missing,
duplicate, unexpected, and stale records fail. The retained graph is now acyclic.

The complete cross-crate regression suite is supported from the matching
repository checkout. Some archived tests require the intentionally omitted
checkout-only dependencies; standalone registry archives do not promise that
complete suite. No tests were removed or disabled. Before/after Cargo metadata
confirmed identical dependency identities, paths, features, default-feature
settings, optionality, targets, and kinds across the workspace, with exactly the
12 intended version omissions.

Tiny Cargo fixtures cover both outcomes: a fresh versioned dev cycle fails
packaging and online `cargo publish --dry-run --no-verify`, while a focused
checkout-only cut passes both. Every fixture archive retains its lockfile,
including the CLI. These checks never upload packages or change credentials.

`just release-package-check` creates local archives for the computed set in one
explicit `cargo package --registry crates-io --no-verify` invocation and checks
normalized dependencies, including feature activation settings. A dirty preview
requires explicit `--preview-dirty`; the default requires a clean tree. The
29-package dirty preview and normalized-manifest inspection passed on macOS
(`/tmp/flui-cycle-real-package-preview-network.log`). The initial restricted run
failed DNS resolution; the permitted network retry completed successfully.

This archive preview neither builds nor uploads packages. Archive inclusion
rules and first-party license-file packaging are now in place: the facade
declares an anchored `include` list (67 files in its archive instead of 666),
and every published crate carries `LICENSE`, `LICENSE-APACHE` and `NOTICE`,
which `verify_archives` requires. Clean consumer verification is
`just release-consumer-check` (`scripts/release_consumer_check.py`): it
packages the release set, vendors every third-party dependency with
`cargo vendor`, installs the archives as a Cargo directory source, generates a
counter project with `flui create` *without* `--local`, and builds and tests
it offline; the consumer's lockfile must resolve every `flui-*` package to an
archive digest.

First run, 2026-09-21 on the `0.1.0` cut (`/tmp/flui-beta-consumer-check4.log`):
`cargo vendor` produced a 971 MiB third-party set; all 29 archives installed
as a directory source; `flui create beta_consumer --template counter` without
`--local` wrote `flui = "0.1.0"`; `cargo build --offline` compiled the
consumer from the archives in 7 min 31 s (debug), and `cargo test --offline`
ran the template's two generated tests (`counter_responds_to_pointer_input`,
`counter_content_is_centred`), 2 passed. The consumer's lockfile resolved 22
`flui-*` packages, every one to an archive digest — the counter's dependency
closure; the CLI, devtools, hot-reload and localizations packages are not in
it. This is the first clean consumer build from the archives; it does not
exercise a registry index, upload, or docs.rs.

Baseline verification before the release-policy and surface-color changes:
`just ci` completed on macOS with 9,439 workspace tests and 52 GPU tests passing,
plus doctests (`/tmp/flui-beta-ci-facade-final.log`). That run skipped three
explicitly ignored tests; all three were then invoked directly and passed
(`/tmp/flui-three-ignored-tests.log`). The platform native test suite remained
excluded by the macOS recipe because its X11-backed suite requires Linux/Xvfb;
this does not certify that platform suite. Ignored doctest examples retain their
existing status. These results describe that baseline, not subsequent changes.


## Font asset verification

`crates/flui-painting/assets/fonts/inventory.toml` is the font provenance inventory.
It pins three upstream fonts and all generated fixtures, with SHA-256 hashes and
complete associated license/attribution files. The offline checker rejects
unlisted fonts, modified bytes, missing notices, and reintroduced restricted
Arial bytes even under another filename. Generation into a temporary directory
reproduces every fixture; the four pre-existing probes retain their prior hashes.

FLUI Probe Sans replaces the former Arial test dependency while preserving the
two load orders, regular/static W400 metadata controls, and actual shaped letter
and space family checks for `Ao Bo`. Coverage and positive advances are verified
explicitly. Empty outlines are intentional; this is a shaping fixture, not a
rasterization oracle. Bypassing resolution must fail the existing family-selection
regression before the restored implementation is accepted.

After the 29-package preview, the actual painting `.crate` was inspected:
eight fonts and six notices match the reviewed SHA-256 hashes, the archived
inventory matches the source, and Arial is absent. The facade and CLI archives
both retain `Cargo.lock` (`/tmp/flui-font-archive-verification.log`). This verifies
archive contents; clean archived builds and broader inclusion/license-file work
remain open. No registry upload or version bump is part of this repair.

The surface-color repair separately replaced the observed Metal
`Rgba16Float`/`Auto` → `ExtendedSrgbLinear` choice with `Bgra8Unorm`/`Srgb` for the
encoded-sRGB renderer. Selector regressions, three GPU checks, 23 surface checks,
strict Clippy, and a live Metal probe passed. This does not certify every platform
or ordinary application shutdown, and the earlier full-CI baseline predates the
color, release-policy, font, and test-cleanup changes.

Font repair evidence: the policy first rejected the existing Arial asset and six
missing notice files, then passed eight offline mutation/reproduction tests and
Cargo file-selection checks. The resolution-bypass mutation failed the expected
family comparison (`FLUI Probe Sans` versus `Roboto`); the restored implementation
passed all 175 painting tests with zero skips. Scoped all-target/all-feature
Clippy, Rust formatting, TOML formatting, and focused spelling checks passed.
No full-workspace CI result is claimed for these subsequent changes yet.


A bounded AppKit close-path probe also passed (`just macos-close-path`,
`/tmp/flui-beta-macos-close-path.log`): programmatic close makes the native handle
unavailable, invokes the callback, bypasses the veto, and permits immediate
wrapper drop. The probe uses a non-visible real window and exits the process
explicitly; it does not certify ordinary GUI shutdown or complete autorelease
pool teardown.


The CLI run admission check now recognizes the sole-facade applications it
creates. It uses Cargo-resolved normal dependency identities, including aliases
and workspace inheritance, rather than searching manifest text for internal
crate names. Headless CLI fixtures execute marker binaries and reject unrelated
members, dev/build-only dependencies, comments and prefix lookalikes. This tests
admission, not live UI behavior. Evidence: `/tmp/flui-run-admission-red.log`
(original sole-facade rejection) and `/tmp/flui-run-admission-suite.log`.

## Desktop build artifact discovery

The first external generated-app `flui build desktop` probe completed Cargo
compilation but failed artifact lookup because the output directory was configured
through `CARGO_TARGET_DIR`. The pre-beta `DesktopBuilder::new(&root) -> Result`
constructor becomes stateless `new() -> Self` with `Default`; operation contexts
own the working directory. Desktop builds now consume Cargo's actual executable
artifact and metadata rather than reconstructing a path or guessing the first binary
in a manifest. Package/default-run/example selection is explicit; ambiguous and
non-executable selections fail with the underlying cause visible in CLI output.

Present application configuration uses the existing typed loader, preserving the
configured display name and organization. Invalid configuration fails; absent
configuration retains the example-directory fallback. Bundle names are checked before
filesystem mutation, and existing bundle-path symlinks are rejected. These checks
preserve ordinary Unicode names and spaces.

Tiny real Cargo fixtures verify external/configured output directories, package and
example selection, cached artifacts, failed builds and staging. This is scoped build
verification; a new generated-FLUI application build and visible launch are recorded
separately when performed. No new cross-platform runtime or shutdown claim follows
from artifact discovery alone.

The retained generated counter subsequently passed `flui build desktop` with its
external `CARGO_TARGET_DIR` (`/tmp/flui-beta-counter-desktop-fixed.log`). The
launch sequence recorded there showed a white window through the UI automation
service while a direct launch of the same bundled executable rendered the
counter; two observed pointer clicks changed 18 to 19 to 20. The initial value of
that live observation was already 18, so it does not establish the initial zero
state.

That white window is now attributed, and it is not a launch-route difference: a
macOS window is ordered front before its first frame is presented
(`crates/flui-platform/src/platforms/macos/window.rs:359` runs inside
`open_window`, ahead of the GPU stack and the first `queue.present`), so until a
frame reaches the compositor the window shows its own background. Measured on a
3440x1440 display, a window with nothing drawn fills 99.78 % of its rect with
`rgb(240,240,240)`, against 98.70 % for the same window once the counter is drawn
— one near-white field either way, separated only by the content. The two
observations are consistent with warmth rather than with route: a cold launch's
first frame was still undrawn 2.81 s after its window appeared, while on every
warm launch since, the window was already drawn at its first sighting; the three
launch routes were also measured directly and rendered 15/15 and 9/9 across all of
them. The fix now exists: a macOS window opened visible with
`WindowOptions::reveal = WindowReveal::AfterFirstFrame` — what `flui-app`'s
runner asks for; a direct `flui-platform` consumer keeps the default
`AtOpen`, having no first frame to report — is ordered front at
`alphaValue` 0 and made opaque when the desktop runner reports the first
presented frame (`PlatformWindow::reveal_after_first_frame`, driven by
`flui-app`'s `FirstReveal` policy with a one-second fallback measured from the
first frame that ran and presented nothing). Hidden-then-shown was tried first
and refuted live: an un-ordered window gets no Metal drawable, so every frame
came back withheld until the fallback. With the transparent window the log
reads create → one withheld acquire → present → reveal 85 ms after the first
frame, and the CoreGraphics window list's first sighting of the bundled
example (`open`, 1.41 s after launch on a warm cache) was already painted
(`/tmp/flui-reveal-live5.log`, computer-use window capture). The winit and
Win32 backends still reveal at open; see the platform architecture document
for why the deferral is macOS-only for now.

The early observation that the process remained inside `NSApplication.run` after
that same close was recorded against a pre-`0ae979fd` binary, and is not
reproducible on the current candidate: see the live verification below. The
independent fixture review also strengthened package selection coverage:
same-named binaries now emit distinct package identities, which the tests execute
and verify (`/tmp/flui-desktop-package-identity-repair.log`, nine tests passed).

### Native macOS last-window exit

AppKit now consults the existing exit-policy hook after close callbacks and
re-evaluates it on the owner thread when a worker releases a keep-alive holder.
Explicit quit and native `terminate:` requests stop and wake the loop, returning
through Rust cleanup instead of exiting the process inside AppKit. The standalone
runner rejects a running NSApplication or existing delegate before mutation;
foreign delegate and activation-policy changes are preserved.

The bounded `exit_policy_probe` reproduces the original failure: its close
callback ran but `Platform::run` did not return within eight seconds. The repaired
native cases verify post-return and destructor markers, exactly-once quit,
replacement-window survival, veto/re-evaluation, bootstrap errors and native
window weak references becoming nil after autorelease-pool drain. The platform
architecture document records the ownership and callback-lifetime decisions.
This certifies native-loop behavior, not physical Cmd+Q/menu routing, nested modal
loops, or foreign-loop embedding. Full CI remains a final-release gate.

A generated counter built through `flui build desktop` was also launched directly
from its produced macOS bundle: native UI interaction advanced 0 → 1 → 2, then
closing the window logged both window close and platform quit and returned exit
code 0 without a signal.

### Live verification through direct OS interaction — 2026-09-20

The checks above were rerun on the current candidate (`e6429242`) by direct
operator-equivalent input rather than through the recorded automation suites:
real pointer clicks posted through `CGEventPost` against `CGHIDEventTap`, and
window content read by photographing the window number from
`CGWindowListCopyWindowInfo`. This exercises the same OS channels a user does
— no accessibility-tree reading, no headless driver shim.

| Step | Result |
|---|---|
| `material_demo` direct launch, window 800×632 rendered with title and list | ✅ |
| Click on Item 0 | ✅ «Selected: none» → «Selected: Item 0» on screen |
| Click on Item 3 | ✅ → «Selected: Item 3» |
| Click on the app bar (control: no target there) | ✅ selection unchanged |
| Red-button close | ✅ «Root widget detached» logged, exit 0 |
| Cmd+Q against a windowless restarted instance | ✅ process exits (re-tested on 3 runs) |

The same counter template was then generated from the CLI and driven end to
end in a temporary consumer directory:

| Step | Result |
|---|---|
| `flui create --local=<checkout> myapp --template counter` | ✅ sole-`flui` project generated |
| `cargo build` in the consumer directory | ✅ clean, 69 s cold build |
| First launch | ✅ window on screen with content vertically centred (the `3f98e0e9` template fix is visible) |
| Three successive clicks on Increment | ✅ 0 → 1 → 2 → 3 |
| Red-button close | ✅ clean exit |
| Relaunch of the same bundle | ✅ counter at 0 — retained state resets across processes, as documented |
| Click on Increment on the relaunched instance | ✅ 0 → 1 |

First-run flake observed once, not reproduced: a fresh consumer launch
(`pid=74480`) opened its native window (`Created NSWindow` logged) but exited
~2 s later without a panic, after the surface's drawable stayed unavailable
long enough for the frame pump to give up («Frame rendered but never shown»,
ending in `run_paint` as the last line). Three immediate follow-up launches of
the same binary were uneventful, so the cause is not yet attributed; the
observation is recorded so it is neither silently ignored nor silently
forgotten, and the first-frame race fix below is the candidate explanation.

This live verification covers real-window input, retained state across
rebuilds (the counter's own `Rc<Cell<usize>>` survives its parent rebuild, as
the two generated tests assert headlessly), and the clean native shutdown
path — but not IME, clipboard, suspension, or any platform other than macOS.

A separate LaunchServices/background launch was recorded here as showing a blank
window. Re-measured, it does not reproduce, and the cause is now attributed to
the first-frame race recorded above, not the route. `just macos-launch-render`
launches one bundled artifact three ways — direct exec, `open`, and `open -g`,
which does not activate the app — five times each, finds each launch's window by
owning PID, and photographs it by window number, because a window can hold a
live frame pump and still show nothing. Each launch is given a bounded settle
(10 s) because a macOS window is ordered front before its first frame is
presented: the oracle is retried until it holds, and the time to the capture
that passed is reported as `first frame after` — on the fixture, ~0.13 s on
every route. A genuinely blank window is not the same thing as one caught early,
and the gate now distinguishes them: a failed capture is re-taken until either
the oracle holds or the bound expires, with an optional screen capture of the
window's rectangle (taken only when that window is frontmost) reporting what a
viewer had on screen as a non-deciding diagnostic.

All fifteen launches rendered the fixture's red, and the counter bundle
rendered on both LaunchServices routes as well: 10 distinct colour buckets in
its window content on each of the three routes, the ink being the counter's own
label (a 107×34 cluster inside a window that is otherwise 99.94 % one colour).
The sentence is therefore not carried forward. The gate is validated against a
control that draws nothing — an empty AppKit window fails on every capture
across the whole settle bound, not only once — which is what makes a pass a
discrimination rather than an absence of measurement. A host that has not
granted Screen Recording exits "cannot verify" instead of reporting a blank
window it never saw. The gate needs a GUI session, so it remains a local manual
check like the other native probes.

### Window-independent owner turns

The app's pending native-window completion no longer depends on the first
realm's frame driver. Headless coverage includes acceptance with no realms and
worker completion after the originating realm closes. Native macOS probes verify
windowless worker delivery and ordinary return; Windows is cross-compiled only.
The owner callback is fallibly registered, signals are coalesced, and quit fences
new work immediately. Physical posting failure remains an explicit progress
limit, with cancellation and reservation release rather than spinning retries.

The subsequent Application increment supplies a rendered designated main-window
factory and public control handle through `flui::app`. Secondary windows still
have no mounted content or frame renderer. Full mobile lifecycle and native
OS-suspend transport are not established by this desktop result.


### Resident main-window validation

Desktop `flui::app::Application` owns a reusable rendered main-window factory.
`StartupWindow::None` and `ExitPolicy::ExplicitQuit` support a windowless owner
loop; a Send + Sync `AppHandle` admits show/quit commands without transferring UI
closures between threads. A new main window has fresh widget state while captured
application data persists. Existing-window show preserves maximized/fullscreen
mode; unsupported Wayland reveal returns a typed error rather than silent success.

The sole-facade native counter passed two modes: startup with a window, and a
worker-requested first window after windowless startup. Real input changed its
counter, native close disposed the root, and OS reopen recreated rendered content
in the original PID. Local state reset while application state persisted. Both
runs ended through AppHandle quit with ordinary exit 0 and cooperative service
cancellation/drop. The windowless case dropped the show receiver immediately,
proving admitted intent survives receiver abandonment. A non-keeping service did
not mask the explicit exit policy. Feature-enabled tests separately verify a real
artifact watcher retains its thread across failed reopen attempts and stops with
the loop. Windows show has strict cross-compilation evidence; live mode tests are
macOS-only.

Run `python3.12 -B scripts/check-resident-reopen.py prepare /tmp/flui-resident-probe`,
then `python3.12 -B scripts/check-resident-reopen.py run /tmp/flui-resident-probe`
with an active macOS GUI and CUA interaction. Add `--windowless` to run the worker
startup scenario. The driver requires GPU presentation after each initialization,
input and disposal witnesses, same-process OS reopen and normal return. Failure
cleanup signals never count as success.

This covers one designated rendered main window. `open_secondary_window` still
has no widget-content/renderer API on `WindowPolicy::SharedRealm`, but
**`flui::app::open_window(config, policy, root)` now opens a content-bearing
secondary window** on `WindowPolicy::SeparateRealms`: a fully mounted widget
tree, its own `RasterLane` and frame pump, all dispatched through the same
addressed-routing seams as the primary window. `SharedRealm` is refused at
admission by a typed error because a per-presentation raster contract does
not exist yet. Live-verified on macOS by `examples/multi_window_demo.rs`:
the primary's button opens a second titled window with real content, and
closing the secondary then the primary exits cleanly. Full mobile lifecycle,
live Windows runtime behavior, and native OS-suspend transport remain
separate work; these desktop results do not imply beta release readiness by
themselves.

### Fresh consumer on the candidate — 2026-09-21

Regenerated from the CLI at `e8604b0d` (`flui create beta_counter --template
counter --local=<checkout>`): `cargo build` clean in 7 min 22 s cold (debug),
`cargo test` ran the two generated tests, 2 passed. Bundled and opened
through LaunchServices, the window entered the CoreGraphics on-screen list
3.85 s after `open` — and with the deferred first reveal (`fd9f2938`) a window
in that list is one whose first frame has been presented, so the cold launch
showed no bare background. Driven through the desktop app's background
window control (raw input on the AppKit window, window capture by window id):
the counter read 3 on first sighting (an operator had clicked it), two
posted clicks advanced it to 4 (the first of the two activated the
application, as AppKit's first click on an inactive app does), the window was
resized to 1568×595 while mounted and its content stayed centred, a further
click was posted, and the red close button ended the process. The
archive-based consumer build above is the same template through the registry
dependency form.

## Developer iteration: the hot-reload loop, driven

`just macos-hot-reload-loop` (`scripts/check-hot-reload-loop.py`) generates a
`--hot-reload` project with the CLI, runs `flui --json run` on it, and drives
the loop through the CLI's own event stream rather than its narration. The
first edit changes the worker's label and adds a witness to its build
function — a line that bumps the host-owned counter and prints it — so the
witness's appearance proves the reload ran code that did not exist before it,
inside the host started before it, and its value is the state the host
carried across.

Run on 2026-09-21 (`/private/tmp/.../hrloop/run.jsonl`; host PID 752 for the
whole run): initial build 408 s cold; edit #1 → `run.build.done ok=true` in
12.0 s → `run.reload kind=hot ok=true` → `PROBE count=1`; edit #2 (an
unterminated string) → `run.build.done ok=false` in 3.4 s → `run.reload
ok=false`, host alive, no restart; edit #3 (the fix) → build 9.1 s → reload →
`PROBE count=2`, strictly above the count after edit #1, which is the
state-preservation proof — a restart, or a worker owning its own state,
would have started over; 20 s idle produced no build or reload event; SIGINT
exited `flui run` with 130 (an `error` event naming the interrupt) and the
host was gone within the bound. No synthetic input is posted: a probe that
drives the pointer would hijack an operator's mouse, and the witness needs
none.

Two things the run taught, both recorded rather than papered over. A reload
applied while the host's window is occluded is reassembled but not rebuilt
until the window is visible again — frames are disabled while hidden — so
the probe brings the host to front before each edit and reports an occluded
window as CANNOT_VERIFY, not as a missing reload; the first two attempts
failed exactly that way while an operator's windows covered the host. And
at the time of the run the host's stderr reached `flui --json run`'s stdout
unwrapped (its stdout was wrapped as `run.app.log`), which broke the
one-object-per-line contract; the probe tolerates raw lines, and the CLI has
since wrapped both streams (`b53ab703`) and closes every run with
`run.stop {interrupted}` before the interrupt's `error` event.

This covers the worker (`WorkerHost`) reload tier on macOS. Hot restart on a
types change, the scene-plugin tier, and the iOS worker path are not driven by
this probe.

## Web: the counter in a browser

`examples/web_counter` is the generated counter template behind a
`#[wasm_bindgen(start)]` entry point: `flui::run_app` dispatches to the web
runner on `wasm32`, which mounts the tree into the page's `#flui-canvas` and
renders through WebGPU. `just web-counter-build` compiles it with plain
`cargo build --target wasm32-unknown-unknown --release` and `wasm-bindgen`
(10.6 MiB of wasm, unoptimised by `wasm-opt`); the page is served with any
static HTTP server.

First run, 2026-09-21, in the desktop app's Chromium-based browser pane: the
page loaded, the runtime set the document title, and the canvas stayed white
with every clip-capable pipeline invalid — the console carried Tint's
`'dpdx' must only be called from uniform control flow` for the rect, circle,
arc, texture and glyph shaders. The cause was two shader shapes native naga
accepts and every browser rejects: `clipAlpha` called `sdfToAlpha` (which
takes screen-space derivatives) inside an `if` on the per-instance clip kind,
and the arc shader took its angular gradient after a per-instance early
`return`. Both now compute every derivative unconditionally and choose with
`select`; the engine's 624 tests including the GPU-gated suite still pass on
Metal. naga's validator with every flag on accepts the old source, so there
is no host-side oracle; `scripts/check-wgsl-uniformity.py` is the structural
stand-in (a derivative-taking call inside a branch or after a conditional
return is refused), it is red on the old shaders for exactly the two sites the
browser named, and it runs in `just gate` and CI.

After the fix the counter rendered — label, `0`, and the Material button —
three clicks on Increment advanced the count 0 → 1 → 2 → 3, a click on empty
canvas left it at 3, and the console had no errors. This is one browser, one
machine, `localhost`; it is evidence for the Web row's move from unverified
to experimental, not a browser matrix.

Second observation, 2026-09-22, from the CLI's `flui run --device browser`
work: the canvas did not fill the viewport although the page's CSS said
`100vw`/`100vh`. The web backend was pinning the canvas to `AppConfig::size`
in inline CSS and never dispatching a resize. It now takes the canvas's CSS
box as the window size and follows it (`ResizeObserver` plus the window's
`resize` event, backing store at the device pixel ratio). Same browser pane,
a page styling the canvas `100vw`/`100vh`: the canvas measured 1100×700 for
an 1100×700 viewport with no inline style, followed a viewport change to
980×1260 at device pixel ratio 2 (backing store 1960×2520) with the counter
re-centred, and a click on the re-laid-out button advanced the count.

## Window lifecycle on macOS: minimize, hide, resize — 2026-09-22

`examples/lifecycle_probe.rs` (`just macos-lifecycle`, release build) runs a
Material tree through the ordinary `flui::app::Application` path with a
free-running `AnimationController` demanding frames, then drives its own
window from a driver thread through AppKit on the main queue — no operator
input, no synthetic OS events — and counts the frames the runner produces
through each transition with a self-re-arming post-frame callback:

| phase | driver | frames | budget | verdict |
| --- | --- | --- | --- | --- |
| first frame | wait | 0.892 s after the window factory | ≤ 15 s (hang guard) | PASS |
| visible | — | 201 in 2 s | ≥ 30 | PASS |
| minimized | `-[NSWindow miniaturize:]` (`isMiniaturized` confirmed) | 0 in 3 s | ≤ 5 | PASS |
| restored | `-[NSWindow deminiaturize:]` | 199 in 2 s | ≥ 30 | PASS |
| hidden | `-[NSApplication hide:]` (`isHidden` confirmed) | 0 in 3 s | ≤ 5 | PASS |
| unhidden | `-[NSApplication unhide:]` | 200 in 2 s | ≥ 30 | PASS |
| resized | `-[NSWindow setFrame:display:]` +200×+100 | layout saw 840×580 = the new content size | ±1 px within 1 s | PASS |

Same host and display as the workload run (M1, macOS 27.0, 100 Hz). A
minimized or hidden window costs the runner nothing at all — zero frames
against a controller that never stops asking — and the loop is back at the
panel rate within the half-second settle after each restore; the resize
reaches the root's constraints exactly. Budgets were declared in the
probe's module doc before its first run; the first run failed only its
baseline, which had started before the cold GPU stack produced a frame,
and the probe now waits for the first frame and reports the wait instead.
Not covered: occlusion by another window (the runner's `occlusion_visible`
path), display sleep, and a live drag-resize's intermediate frames.

One observation from the run, not a budget: at startup the rendering
pipeline warns once — `run_layout: no cached state.constraints() AND no
root_constraints … skipping dirty entry` for a non-root render node — on
the frame in which the `LayoutBuilder` child is first marked dirty before
its parent has laid it out. The pipeline recovers on that same frame's
layout pass (every later phase renders), so it is recorded here as a
diagnostic to quieten, not a failure.

## Performance and resilience: the representative workload — 2026-09-22

`examples/workload_probe.rs` (`just macos-workload`, release build) is the
reproducible workload the row asks for: a `Scaffold` with an `AppBar`, a
Material `TextField` and a 2,000-row `ListView::builder` of `ListTile`s,
900×700 logical, driving itself from an `AnimationController` tick — 20 s of
scrolling at 18 px per frame bouncing between both ends, then 500 characters
inserted one per frame into the field, then 5 s of enforced idleness — with
no operator input and no synthetic OS events. The probe prints one JSON
line per phase; `scripts/check-macos-workload.py` samples RSS every 0.5 s,
reads the main display's refresh period through CoreGraphics and hands it to
the probe, and applies the budgets declared in its own header before the
first run: scroll and type p99 within two display periods, under 1 % of
scroll frames over two periods, RSS growth under 10 % from a baseline five
seconds in, at most 5 frames during idleness. Every run writes
`target/workload/<timestamp>.json` with the phase lines, the RSS series and
the per-budget verdict.

Host: MacBook Air (M1), macOS 27.0 (26A428), main display 3440×1440 at
100 Hz (10.0 ms period, `CGDisplayCopyDisplayMode`), release profile.

**First run — a finding, not a pass.** With the swapchain at
`desired_maximum_frame_latency: 1` (the value the engine had carried since
ADR-0029) every phase presented at a rock-steady **20.0 ms p50 — exactly
two periods, 50 fps**: scroll 984 frames in 20 s (p90 20.3, p99 24.9, max
131.7 ms), type 500 frames at p50 20.005 / p99 20.6 ms. The bare platform
frame pump on the same display (`just macos-frame-pump`) ran 100.2 fps, so
the halving was in the rendering path. Setting the latency to 2 and
re-running: scroll p50 **9.998 ms**, type p50 9.998 ms — the full panel
rate. The mechanism is the one ADR-0029's AppKit subsection had measured on
a 3 % tail and judged tolerable: with two drawables, the acquire for the
next frame waits for the previous drawable to leave scanout, so a frame
whose own work does not fit in what remains of the period misses the next
vsync — on every frame, once the frame does real work. The literal is now
2 (wgpu's default); the reasoning, the earlier resize-axis measurement that
made this a free choice there, and the numbers are in the literal's own
comment and in ADR-0029's dated addendum.

The first run's RSS check also failed — 75.6 MiB at 1 s to 201 MiB at the
end, +166 % — and that one was the script's: the series reaches 199.6 MiB
by 2.2 s (GPU stack, glyph atlas, the first laid-out screen) and is flat to
within 1 % for the remaining 33 s. The baseline moved from 1 s to 5 s, past
the startup ramp and inside the scroll phase; the 10 % budget did not
change.

**Accepted run, at latency 2 with the real period:**

| Budget | Measured | Verdict |
| --- | --- | --- |
| scroll p99 ≤ 2 periods (20.0 ms) | p99 10.10 ms, 1,967 frames / 20 s | PASS |
| scroll frames over 2 periods ≤ 1 % | 6 / 1,967 (0.31 %) | PASS |
| type p99 ≤ 2 periods | p99 10.04 ms, 500 frames | PASS |
| idle frames ≤ 5 in 5 s | 1 | PASS |
| RSS growth ≤ 10 % from 5 s | 253.1 → 185.0 MiB (−26.9 %; peak 253.6) | PASS |

The single idle frame is the one the controller's `stop()` lands on; the
runner then produces nothing until the process quits, which is the "static
applications become idle" half of the row measured rather than asserted.
Limits: one host, one display, one build; the probe drives controllers, not
the platform's input path (by design — a real operator's mouse and keyboard
are in use on this machine), so input-translation cost is outside this
number; and the probe cannot read the display period through the facade
(`PlatformWindow::refresh_period` is not exposed to application code), so
the script supplies it.

## Native iOS application delivery

`flui build ios` now selects a Rust executable and stages an unsigned native UIKit
`.app`. Static-library consumers opt into `--lib`, including `--lib --universal`
for XCFramework delivery. Simulator builds use `--simulator <exact UDID>`;
`flui run --device <exact UDID>` builds, installs and launches once without a
host-executable fallback or desktop watcher. A missing simulator is a failure.

The SDK fixture verifies real simulator Mach-O architecture/platform/minimum OS,
canonical bundle identity, numeric Apple versions plus full SemVer metadata,
legacy Flutter project bypass and preservation of previous output on mismatch.
The verified SDK is Xcode 26.2. Device signing/distribution and UIKit UIScene
migration are not implemented by packaging. The existing UIKit runner's full
background scheduling/lifecycle semantics remain separate beta work; launching
an application alone does not prove them.

## iOS execution lifecycle foundation

The CLI rebuilt from `52c0a03a` subsequently built, installed and launched the
external generated sole-`flui` counter on the dedicated iOS 26.2 simulator. The
installed bundle's identifier, executable permissions, SHA-256 equality with
the Cargo executable, and single-scene `FluiSceneDelegate` manifest passed
direct checks. The run log is `/tmp/flui-ios-counter-final-run.log`.
Simulator UI automation timed out in that attempt, so real touch input and
retained displayed counter state after Home/return were unverified **by it**.
Both are now measured, on that same candidate and on the in-repo Material demo,
by `just ios-input-check <udid>` (`scripts/check-ios-input.py` driving
`scripts/ios-input-probe.swift`).

The instrument is XCUITest, because nothing else can put a `UITouch` into the
application: `xcrun simctl` has no touch subcommand, and driving the Simulator
window through host UI automation needs the Accessibility grant and photographs
the host's desktop instead of the device. XCUITest synthesises the touch inside
the simulator through the platform's own automation channel, so the host needs
no desktop permission. The oracle is pixels, and has to be: this backend
publishes no accessibility tree, so a widget cannot be read by identifier.

On the iPhone 16e simulator (iOS 26.2), both subjects carried all four stages:

| subject | tap | Home / return | fresh launch | tap on no target |
|---|---|---|---|---|
| `dev.flui.beta-counter` (the generated sole-`flui` candidate) | `0` → `1` | `1` retained | reset to `0` | unchanged |
| `dev.flui.ios-demo` (in-repo Material demo) | `Selected: none` → `Selected: Item 0` | retained | reset | unchanged |

The changed pixels are one glyph and one line and nothing else. The counter's
tap moved 817 pixels, all inside a 25×34 box at x[572,597] y[101,135] of
1170×2532 — its own digit — and the demo's moved 8 383 across the single 301×34
status line at x[434,735] y[197,231]. The compared content-region hashes ran
`75a445c4 → a341befd → a341befd → 75a445c4 → 75a445c4` (counter) and
`7512336b → f0f7afd5 → f0f7afd5 → 7512336b → 7512336b` (demo) for
initial → tap → return → relaunch → empty tap. Evidence, including the screen
and the exact region hashed for every stage, is under `target/ios-input*/<run>/`.

Two of those stages are controls, so the pass is a discrimination rather than an
absence of measurement: the fresh launch resets, which is what makes the return
comparison falsifiable, and a real tap at a point with no target changes nothing,
which is what separates a hit from any touch. Both tap points are required to lie
inside the compared region — enforced by the checker, not a convention — because
a no-target control measured outside the window it is a control for cannot fail.
The gate was also run against subjects that must not pass, and each failure mode
was reached: a UIKit application that draws and installs no touch handling at all
fails with "the tap changed nothing on screen" and its relaunch control reports
that it therefore proves nothing (exit 1); an animating application, whose screen
never stops changing, is CANNOT_VERIFY rather than a verdict (exit 2); an unknown
simulator is CANNOT_VERIFY (exit 2); a malformed `--region` or a tap outside it is
CANNOT_VERIFY before anything is launched (exit 2).

**The return comparison alone does not prove the application was running.** iOS
keeps a snapshot of the pre-Home frame and shows it while an application returns,
so "the display is unchanged" is satisfied by a screen that never resumed — the
pixels cannot carry a claim about the process. The stronger oracle is a second
real touch after the return that must *advance* the display, which a snapshot
cannot do; it is opt-in (`--post-return-tap`) because it needs a subject whose
display advances per tap, and the run's report names which oracle carried the
claim. The counter's `Increment` button accumulates, so it can carry it; the
demo's list rows are idempotent, so it is measured by equality with that stated
rather than hidden. The oracle itself is validated in both directions: it refuses
the demo, whose second tap on the selected row correctly changes nothing (exit 1,
"a touch after Home/return changed nothing on screen"), and it passes an
accumulating subject built for the purpose — a UIKit application, since the
counter's bundle was no longer on the host and rebuilding it did not fit the free
space — whose hashes ran
`dfc58830 → 855a4776 → 855a4776 → becbf2bf → dfc58830 → dfc58830`, the fourth
term being the post-return tap's advance.

Four defects surfaced while validating the gate, all in the harness and all
fixed — each had silently produced a confident wrong answer first:

- **A predecessor run's evidence was read as this run's.** The test host's
  container survives reinstallation, and the probe wrote to a fixed path in it,
  so screenshots of a stage that never happened were copied into the next run's
  record and were byte-identical across stages that its own report said had
  changed. Evidence now goes to a directory named for the run, and the checker
  clears the probe's `tmp/` before starting. This is the same defect class the
  macOS launch-route gate records for owner-name window matching.
- **The compared region cropped out the only pixels the tap was meant to
  change.** The counter draws its count immediately below the status-bar clock,
  and the region's 6 % top inset excluded it, so a delivered touch and a lost
  one looked identical. The region is now a rectangle — `--region` — and the
  caller must ensure it covers what the tap changes.
- **A blank screen was read as a lost touch.** One run caught the application
  before it had drawn anything (ink 0.00 %) and reported that a real touch did
  not reach a widget; there was no widget. A first stage below 0.05 % ink is now
  CANNOT_VERIFY, the same "was anything drawn at all" distinction the macOS
  launch-route gate draws with its colour oracle.
- **The no-target control was measured outside its own window.** Its default
  point sat at y 0.035, above the compared region's 6 % top edge, so a touch that
  had changed the display there would still have been reported as an unchanged
  one: the control could only ever pass. A band-by-band profile of the demo's
  frame answers where a touch has no target at all — the app bar (top 11 %,
  `#fef7ff`) is the only such area — and the app bar's centre is the default
  point now. The invariant is enforced, so the mistake cannot recur silently on
  another subject.

The observations this measurement does **not** close. First, the bundle measured
for the counter is the one the CLI produced on 2026-09-19; the gate re-measured
that artifact rather than rebuilding it, so this is the candidate as it exists,
not a fresh build of the current revision, and the counter's own run still rests
on the equality oracle rather than the live-touch one. Second, the counter —
whose template asks for `Center` — lays its column at the top of the screen
(content at y 0-305 of 2532) instead of the middle. That is a template misuse and
not a backend property: `FlexStyle::default()` is `MainAxisSize::Max` with
`MainAxisAlignment::Start` (`crates/flui-widgets/src/flex/flex.rs:35-45`) and
`Center` shrink-wraps only when a factor is set
(`crates/flui-objects/src/layout/align.rs:39-59`), so `Center(child: Column(...))`
lays out exactly as it does in Flutter — the column fills the height and packs
its children at the top. Flutter's own counter sample passes
`MainAxisAlignment.center`; the same misuse appears at four sites — the generated
template, `examples/material_demo/tree.rs`'s counter tab, and
`examples/cupertino_demo/tree.rs`'s home page, details route and settings counter
(the earlier citation named only the first two of those) — with
`MainAxisAlignment::Center` used nowhere under `examples/` before this.
**Nothing about it was changed by this task** — it was recorded here first and
fixed in the template work that followed, which is where it belongs. All four
sites now pass `main_axis_alignment(MainAxisAlignment::Center)`, and the fix is
confirmed against committed geometry rather than asserted: the Cupertino layer
snapshot moved by exactly the translation that implies (four lines, each +279.40
in y, x and size and colour and font unchanged), and the generated project gained
the `counter_content_is_centred` regression test, which fails without the
alignment — content at y 0..105.6 of a 320pt surface, centre 52.8 rather than
160. Closing that test's hole is what exposed the second defect: the CLI's
template harness ran the generated binary with a `--exact` filter naming only
`counter_responds_to_pointer_input`, so the new generated test was compiled,
never executed, and the harness reported a pass either way — which is how the
first version of the test appeared to pass with the fix removed. It now runs the
whole binary and asserts both the per-test lines and the `test result` count, so
a template test that does not run fails the harness. An earlier form of this
paragraph also claimed the
demo's app bar title sits beneath the status-bar clock and that the layout
therefore appears shared across backends — both halves are withdrawn. A band
profile of the demo's current frame puts its title at y 177-253, wholly below the
clock's y 50-127, and the counter's overlap comes from the stale pre-safe-area
bundle it was measured from. This does not complete native application acceptance
or its final code-quality review.

Window execution eligibility is independent of focus, visibility and GPU surface
availability. Temporary UIKit inactivity preserves the surface and frame delivery;
true background suspension caps the addressed presentation. Public presentation
lifecycle subscriptions observe the derived state while a shared realm can retain
an eligible sibling. See ADR-0072 for precedence, initial snapshots and reentrancy.

The dedicated UIKit protocol probe reproduces resign-active/active without a
foreground notification using the actual owned delegate and CADisplayLink. This
original foundation probe tests protocol delivery rather than OS lifecycle or
GPU rendering. The scene migration below adds native ownership, terminal cleanup,
window-independent owner signaling, matching bundle configuration and separate
GPU evidence.

## UIKit scene ownership

The native scene delegate now drives presentation-local execution, focus and
visibility. A controlled owned-delegate probe has verified disconnect without
frames and reconnect with actual GPU submission/presentation, retained local
widget state and a single service start. These are protocol tests on Xcode 26.2,
not a claim of OS-driven scene reclamation or SDK 27 certification.

Shipping bundles declare one scene at a time. Programmatic destruction and new
activation are not universally available under UIKit's single-scene policy;
real OS delivery and deterministic controller coverage must be reported
separately. Full multiwindow rendering and background execution grants remain
beta work. The scene-ownership increment passed its scoped gates and
independent reviews; see ADR-0073. Full beta release validation remains pending.

Automatic retry after a failed surface recreation is now implemented for both
mobile runners (`21af0752`): a genuine rebuild failure after the window was
reported available arms the same deadline-paced backoff device-loss recovery
uses, carried by the platform's wake-deadline hook and consulted by the frame
closure's dirty predicate; the expected "no window yet" answer is classified
once, in `runner/surface_lifecycle.rs`, and never polled. This is host-tested
against a scripted backend and raster lane (retry arming, deferral before the
deadline, recreation once due, a held lane skipped without disarming, a later
release disarming), and both runners lint clean for their own targets. It is
not live-verified: no device or simulator run has yet produced a genuine
rebuild failure to recover from, and the runner-side lint the fix needed did
not exist until the same change (`cross-typecheck` now clippies `flui-app`
and `flui` for `aarch64-apple-ios` and `aarch64-linux-android`; the facade
did not build for iOS before it).

## iOS safe-area layout

The native content-view inset now reaches the widget tree. `PlatformWindow`
exposes `safe_area_insets()` and an `on_safe_area_change` observer; the UIKit
content view samples `safeAreaInsets` on the owner thread — at attach, after a
resize, and from `safeAreaInsetsDidChange`, which forwards to the superclass
implementation — and reports changes to the presentation its window was opened
for. Each presentation owns its root `MediaQuery`, seeded from the live window
when the presentation is created, so a consumer that mounts after a change reads
the current value instead of a default. `SafeArea` now consumes the edges it
selects in the descendant `MediaQuery` (Flutter's `removePadding` behaviour),
which removes the divergence its own docs previously carried — nested safe areas
over-padded because nothing reduced the ambient padding. Consuming exactly the
selected edges is verified headless; `Scaffold`'s own slot padding removal
operates on the same fields and is unaffected.

Live check on the iPhone 16e simulator (`iOS 26.2`, portrait), built from the
revision carrying this record:
`python3 -B scripts/check-ios-safe-area.py <UDID> /tmp/flui-ios-safe-area`. The
fixture is a sole-`flui` application whose Stack holds one bare leaf and one
`SafeArea`-wrapped leaf; it compares both laid-out geometries against the view's
own `safeAreaInsets`, read inside the running application, so the oracle is the
platform's value rather than a recorded constant
(`/tmp/flui-ios-safe-area-check.log`). Native and ambient padding agreed at
`[47, 0, 34, 0]`; the wrapped leaf landed at `(0, 47)` with size `390x763` and
the bare leaf filled `390x844`. Backends without inset reporting keep the zero
default, so no other platform's behavior changes.

This is simulator evidence for one device class in one orientation. Landscape,
keyboard occlusion, other device classes, physical devices, window resize while
mounted, and the equivalent work on the Android and web backends remain
unverified. The iOS-gated code is also outside what the Linux CI job compiles,
so these checks are local-only until the release-candidate pass re-runs them.

Two limits of the addressing half, stated rather than implied. First, the root
`MediaQueryRoot` is installed on the `primary()` attach path — the only path
that carries content today — so a non-primary presentation's source is written
but not yet read. What this change fixes is the write side: a secondary window's
resize or appearance change used to land in the primary presentation's tree and
no longer does. Consuming a secondary window's own source belongs with
secondary-window content, which is separate work. Every iOS build is unaffected,
because the scene policy admits one logical session at a time; so is any
single-window application on other backends.

Second, the iOS-gated modules are invisible to the host-target lint job, so
`just cross-typecheck`'s iOS line is their only compile gate. Clean on this
revision: `cargo clippy -p flui-platform --locked --all-targets --features a11y
--target aarch64-apple-ios -- -D warnings` (and, for the addressed realm arms,
`cargo clippy -p flui-app -p flui-platform -p flui-widgets -p flui-cli
--all-targets --locked -- -D warnings`). `just ios-safe-area-check` runs the
live check above against a booted simulator.
