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
tests under `tests/` also use `flui-widgets`' and `flui-interaction`'s `testing` features and
`flui-testing`; those stay dev-dependencies of the packages and are not SDK surface.

## Consumers

- **`flui-material`** (`packages/flui-material`) builds on this crate alone: its normal
  dependencies are `flui-sdk` and `tracing`, and every path in its `src`, doctests and tests goes
  through `flui_sdk::` (`flui_material_builds_on_the_sdk_alone` in `tools/xtask` pins the
  manifest). The port needed no new item.
- **`flui-cupertino`** moves next (ADR-0088 move 3).

An item a package needs that is not here is added by ADR-0088 §4 (at the facade's path when the
facade has one, otherwise in `pipeline`) with a line in `tests/surface.rs`'s pinned list.

## The derives resolve through the SDK

`flui-macros` looks up `flui-sdk` in the consumer's manifest before the owning crate and the
facade, and expands to `::flui_sdk::{view,foundation,animation}`, so `#[derive(StatelessView)]`
and the others work in a package that names no internal crate. The SDK comes first because a
package may carry the facade or an internal crate as a dev-dependency, which `proc-macro-crate`
does not tell from a normal one. The one shape this order cannot serve, an owner crate as a
normal dependency beside `flui-sdk` as a dev-dependency only, has no instance
(`crates/flui-macros/ARCHITECTURE.md`, "Resolve runtime paths"). The `Diagnosticable` derive
itself has no SDK path; its expansion still resolves through the SDK.

Because `flui-sdk`'s dev-dependency on the facade reaches `flui-material` through the facade's
default `material` feature, a unit test inside this crate would see a second copy of
`flui_sdk` (the one Material links); the surface test is an integration test and is not
affected.

The count is from source, not from rustdoc JSON; the rustdoc measurement ADR-0088 §4 asks for
needs the nightly JSON tooling and replaces this table when it lands.

## Mapping decisions

### No Flutter counterpart

Flutter's packages import `package:flutter/*.dart` libraries directly, and the SDK version is
one constraint in `pubspec.yaml`. Cargo resolves each crate separately, so FLUI gives packages
one crate with its own version instead of the internal crates. The divergence and its reasons
are ADR-0088; the tests above pin it.
