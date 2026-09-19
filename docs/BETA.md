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
| Performance and resilience | Static applications become idle; representative scrolling and editing workloads have recorded frame timing and memory behavior; supported recovery paths work. | Reproducible workload, hardware/OS, build profile, timing distribution, memory measurements, and explicit budgets chosen before acceptance. No invented performance claim. |
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
live-observer registration and native lifecycle transport across every supported
platform remain separate acceptance work.

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

Text deletion still documents scalar-value deletion in `TextEditingController`;
reproduce the grapheme-cluster cases before fixing them.

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
renamed package identities are resolved before checking. When the workspace
becomes a prerelease, internal requirements must pin the exact cohort version;
this change does not bump the current version.

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

This archive preview neither builds nor uploads packages. Archive inclusion rules,
first-party license-file packaging, and clean consumer verification remain separate
release work; successful archive creation does not establish distribution readiness.

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
external `CARGO_TARGET_DIR` (`/tmp/flui-beta-counter-desktop-fixed.log`). Launching
the resulting bundle through the UI automation service showed a white window;
directly launching the same bundled executable rendered the counter, and two
observed pointer clicks changed 18 to 19 to 20. The initial value of that live
observation was already 18, so it does not establish the initial zero state.
The close action reached the `Window closed` callback, but the process remained
inside `NSApplication.run` (`/tmp/flui-beta-counter-bundle-direct.log` and
`/tmp/flui-beta-counter-close-sample.txt`). At that point, bundle launch and ordinary shutdown
remained unresolved runtime checks; this successful artifact repair does not close
them. The independent fixture review also strengthened package selection coverage:
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
code 0 without a signal. A separate LaunchServices/background launch still showed
a blank window; that rendering/startup issue remains open and is not covered by
the direct-launch result.
