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
operation. Default/package iOS selections mean a library target declaring
`staticlib`; executable examples use the existing executable selection. Library
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
`platforms/ios/Frameworks`. The existing no-Xcode fallback returns only the first
library; this increment does not produce an XCFramework, stage every slice, sign
an application, or verify Xcode packaging.

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
