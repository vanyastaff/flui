# Build orchestration

`flui-build` prepares Cargo commands and stages their outputs. Platform-specific
builders keep the public `BuildUnit` selection and `BuildArtifacts` result shape;
Cargo owns package discovery, compiler target identity, and output locations.
`DesktopBuilder` is stateless: `new()` returns `Self` and `Default` is available.
`BuilderContext` is the sole source of the workspace directory for each operation.
This intentionally replaces the pre-beta fallible constructor with a workspace
argument; there is no separate initialization step that can fail.

## Mapping decisions

### Desktop artifacts come from Cargo's protocol

Desktop builds use `cargo locate-project` and `cargo metadata --no-deps` from the
same directory and inherited environment as compilation. A current package takes
precedence over virtual-workspace default members. Named workspace packages are
resolved by package identity rather than an assumed `crates/` layout. Binary
selection honors `package.default-run`; otherwise exactly one executable target
must remain. Examples must have the executable crate type, and ambiguous selections
fail before compilation. No new public selector is introduced.

The private Cargo helper consumes `compiler-artifact` JSON with matching opaque
package ID, target name and kind, and a non-null executable path. Both newly built
and cached artifacts are accepted, but only after Cargo exits successfully and the
reported file exists. The builder does not reconstruct target directories, profile
paths or executable suffixes. This follows the [Cargo JSON protocol](https://doc.rust-lang.org/cargo/reference/external-tools.html#json-messages)
and [Cargo's default binary selection](https://doc.rust-lang.org/cargo/commands/cargo-run.html#target-selection),
using the maintained `cargo_metadata` types rather than another manifest parser.

Stdout is consumed asynchronously; stderr is inherited with
`json-render-diagnostics`, so diagnostics cannot deadlock behind an undrained pipe.
Plain tool output and rendered compiler messages remain visible independently of
tracing filters. Unknown message types are tolerated; malformed recognized messages
and read failures terminate and reap the child before returning. Future cancellation
uses Tokio `kill_on_drop`, which provides termination with best-effort reaping;
it does not promise synchronous reaping after an arbitrary future is dropped.

### Application names are validated at the staging boundary

A public `AppBundle` may be constructed without the CLI, so macOS staging validates
its display name before creating or deleting directories: it must be one normal,
nonempty filename component without either separator spelling or NUL. Unicode,
spaces and XML metacharacters remain valid; the plist escapes XML separately.
Existing symlinks, including dangling links, at the application bundle path are
rejected. The caller chooses the output directory; this is not a sandbox against
hostile concurrent filesystem mutation.

Real Cargo fixtures cover configured and environment-selected target directories,
workspace/package/default-run/example selection, cached results, failed builds and
visible diagnostics. Staging fixtures preserve external sentinels for invalid names
and symlinks. The CLI loads present `flui.toml` files through its existing typed
configuration loader and reports invalid files instead of silently changing identity;
only an absent file enables the example-directory fallback.

### iOS Rust artifacts use the same Cargo protocol

`IOSBuilder` is stateless and uses `BuilderContext.workspace_root` for every
operation. Default/package/example selections mean executables on every platform.
Explicit `BuildUnit::Library { package }` selects an iOS library declaring
`staticlib` (other platforms reject this mode). Library
selection ignores `default-run` and excludes example-library targets. Cargo
metadata supplies the package and target identity, including custom `[lib].name`.
The shared message collector additionally matches crate type and excludes test
artifacts, then accepts only an existing reported file after successful Cargo exit.
Configured and environment-selected target directories need no reconstruction.

Each requested library triple is built separately, and `rust_libs` retains the
requested order. Empty target lists fail before building. Executable examples
require exactly one triple: the current result model has only one executable,
so multi-triple examples (including CLI `--example` with `--universal`) are rejected
instead of silently discarding outputs. Supporting multiple executable slices is
a separate artifact-model decision. Rust compilation does not delete or populate
`platforms/ios/Frameworks`. With no consumer Xcode project, delivery packages all
requested libraries into an XCFramework as described below. This does not sign
an application or verify the existing Xcode-project integration.

This follows Cargo's [artifact message contract](https://doc.rust-lang.org/cargo/reference/external-tools.html#artifact-messages),
not filesystem naming conventions. Host fixtures exercise custom names, mixed
crate types, package/default-member selection, examples, configured output paths,
cached builds and failed recompilation with an old archive still present. The
SDK-required device/simulator test is explicit:

```sh
cargo test -p flui-build --test ios_artifacts ios_device_and_simulator_static_libraries -- --ignored --exact
```

It requires macOS/Xcode and both `aarch64-apple-ios` and
`aarch64-apple-ios-sim` Rust targets. Its success proves real Rust static-library
compilation and discovery for both triples, not simulator execution.


### iOS library delivery preserves platform variants

Without `platforms/ios/flui.xcodeproj`, `build_platform` returns
`output_dir/flui.xcframework`, including a single-library request. It maps each
ordered archive to its requested triple, rejects unsupported/duplicate slices,
and queries actual archive architectures. `lipo` combines architectures only
inside the simulator variant; device and simulator ARM64 never share one fat
archive. This follows Apple's [multi-platform binary framework guidance](https://developer.apple.com/documentation/xcode/creating-a-multi-platform-binary-framework-bundle).
`xcodebuild -create-xcframework` remains the authority for Mach-O platform
classification. Generated plist entries must exactly match expected variants and
architectures, contain safe relative nonsymlink file references, and reference
bytes identical to each corresponding staged input group. The byte comparison
also rejects swapped device/simulator archives with identical ARM64 architecture.
No input filename is treated as architecture or platform evidence.

The private blocking operation owns both tools and temporary staging. Dropping
the async result requests cancellation; the worker requests child termination
and waits before removing scratch. A five-minute tool deadline bounds when a
kill is requested, not operating-system wait latency. If terminal status cannot
be established, including on unwind, scratch is retained and its path logged.
There are no undrained output pipes. No packaging tool writes into the final
output or user Frameworks directory.

Validation and size calculation finish before publication. Cancellation and
commit compete through one atomic state; after commit wins, synchronous
publication can complete even if the receiver disappears. Staging is on the
output filesystem. Existing/dangling final symlinks are refused; replacing a
previous directory uses a backup and rollback. If rollback itself fails, the
backup is preserved and the returned error supplies its recovery path. This is
not a concurrent-writer atomicity or crash-durability guarantee. Input archives
inside the old output are copied before replacement.

Portable tests cover slice planning, unsafe plist references, cancellation,
publication failure and preserved recovery backups. Explicit native verification:

```sh
cargo test -p flui-build --test ios_artifacts ios_delivers_device_and_simulator_xcframework -- --ignored --exact
```

Requires Xcode and `aarch64-apple-ios`, `aarch64-apple-ios-sim`, and
`x86_64-apple-ios` Rust targets. It verifies three real slices, same-platform
merging, wrong-variant rejection, replacement, and a single slice whose input
is inside the old output. It does not certify signing, simulator launch, C
headers/module maps, or the existing consumer Xcode-project branch. Executable
examples retain their separate single-triple staging behavior.

### Native iOS application delivery

Executable selection stages a native UIKit `.app`, independent of any legacy
Flutter Runner or consumer Xcode project. One executable requires one target.
`lipo -archs` and `vtool -show-build` must agree with the requested architecture
and device/simulator platform; missing or conflicting metadata is rejected.
The actual linked minimum OS is written to `MinimumOSVersion`. Executable
permissions are required. Validation precedes the shared transactional publication
worker, so failed packaging retains the previous bundle.

AppBundle carries the full SemVer. Apple version keys use numeric major.minor.patch
(including zero); `FLUIVersion` preserves prerelease/build metadata. This mapping
makes beta versions usable without claiming unique distribution build numbers.
Canonical CLI config supplies name, organization and version; absent config uses
the selected Cargo package name/version. Identifier validation is shared across
the delivered plist and simulator install/launch. Device output is unsigned.

The native fixture uses Xcode 26.2 and the installed Rust simulator target:

```sh
cargo test -p flui-build --test ios_artifacts simulator_application_uses_actual_executable_metadata_and_native_bundle -- --ignored --exact
```

It proves executable discovery, real Mach-O inspection, native bundle metadata,
legacy-project bypass and previous-output preservation on platform mismatch.
The scene manifest names the implemented FluiSceneDelegate and single-scene
policy (ADR-0073), matching dynamic UIKit configuration. This packaging fixture
does not certify signing or simulator interaction; native scene behavior has
separate protocol and GPU fixtures.
