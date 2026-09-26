# flui-sdk Architecture

The package-author surface of
[ADR-0088](../../docs/adr/ADR-0088-official-packages-sdk-and-facade.md) §4: one crate that an
official or third-party package depends on instead of the internal crates, so the core's crate
topology is not part of any package manifest.

## Invariants

- **Host-free.** The normal dependency closure names none of `flui-app`, `flui-engine`, `wgpu`
  or the platform backends: tier K's forbid set in the root `[workspace.metadata.flui.reach]`,
  checked by `cargo xtask reach` over this crate's default and all-features builds.
- **Same items, not copies.** Every path is a `pub use` of the item itself: a whole crate
  (`pub use flui_widgets as widgets`) or one item (`pub use flui_painting::Canvas`). No wrapper,
  newtype or re-declared trait, so `flui_sdk::m::T` and `flui::m::T` are one type wherever the
  facade has that path. `tests/surface.rs` (`the_re_exports_are_the_facades_types`) fails to
  build if a curated item or a whole-module representative stops being the facade's type.
- **Facade paths.** A module that the facade also has sits at the facade's path and holds a
  subset of it; an item the facade does not expose goes into an Evolving module, never into a
  facade-named one.
- **The list is pinned.** `tests/surface.rs` (`the_public_surface_is_the_measured_list`) compares
  the `pub use`/`pub mod` lines of `src/lib.rs` with a pinned list, so adding an item is a
  visible decision, and its `measured` module names every measured item through its SDK path,
  so removing one fails to build.
- **Evolving surface under the ceiling.** Only `pipeline` is Evolving; ADR-0088 §4 revisits the
  crate when its Evolving surface passes about thirty items.
- **Own version.** `version = "0.N"`, not the workspace's: `cargo xtask workspace` refuses an
  evolving crate that inherits the version or leaves major 0.
- **One train per graph.** The guard is not here: `flui-foundation` declares
  `links = "flui_train"`, because the facade does not depend on this crate and a guard here
  could not separate an application on one train from a package on another. `cargo xtask reach`
  states that `flui-foundation` is in this crate's build and in the facade's.
- **The facade's test edge only.** `flui` is a path-only dev-dependency for the identity test;
  `cargo package` drops it, and nothing in `src/` names the facade.

## The measured surface

The items are what `flui-material` and `flui-cupertino` import from the internal crates outside
their tests, measured at `431c8757c` (2026-09-26):

1. every `flui_*::` path under `crates/flui-{material,cupertino}/src` was flattened into leaf
   paths, from `use` trees (nested braces expanded, `as` renames dropped) and from inline paths,
   after removing comments and each file's trailing `#[cfg(test)] mod` block;
2. that gave 167 paths for Material, 92 for Cupertino and 195 in their union, from nine crates;
3. each leaf was rewritten to its SDK path, with an associated item reduced to its type.

| Crate | Leaf paths | SDK path |
|---|---|---|
| `flui_widgets` | 90 | `widgets` (whole crate) |
| `flui_types` | 37 | `types` (whole crate) |
| `flui_view` | 29 | `view` (whole crate); its macros expand through `$crate::`, so they work through the re-export |
| `flui_animation` | 19 | `animation` (whole crate) |
| `flui_foundation` | 7 | `foundation` (whole crate) |
| `flui_rendering` | 6 | `rendering::{RenderUpdateImpact, BoxConstraints, HitTestBehavior, BoxProtocol}`; `pipeline::Canvas` is `flui_painting::Canvas`, so `painting::Canvas` |
| `flui_interaction` | 3 | `interaction::{DragDownDetails, FocusNode}` |
| `flui_objects` | 3 | `pipeline::{PathClipConfiguration, RenderPhysicalShape, TranslationFraction}` |
| `flui_scheduler` | 1 | `LocalPostFrameHandle`, which `flui_view` already re-exports: `view::LocalPostFrameHandle` |

`painting::DrawOp` is added for the packages' paint tests, the only place they name it. Their
tests under `tests/` also use `flui-testing`, `flui-scheduler` and `flui-interaction`'s
`testing` feature; those stay dev-dependencies of the packages and are not SDK surface.

The count is from source, not from rustdoc JSON; the rustdoc measurement ADR-0088 §4 asks for
needs the nightly JSON tooling and replaces this table when it lands.

## Mapping decisions

### No Flutter counterpart

Flutter's packages import `package:flutter/*.dart` libraries directly, and the SDK version is
one constraint in `pubspec.yaml`. Cargo resolves each crate separately, so FLUI gives packages
one crate with its own version instead of the internal crates. The divergence and its reasons
are ADR-0088; the tests above pin it.
