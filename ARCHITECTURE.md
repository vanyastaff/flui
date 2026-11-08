# FLUI application facade

Application packages depend on `flui`. The workspace's implementation crates
remain independently maintained internally; their package boundaries are not a
dependency checklist for application authors.

## Mapping decisions

### View authoring excludes the composition-root seam

The facade's `view` module shadows `flui_view::__runtime` with a private module
(ADR-0081 §4). Application authors can name `flui::view::View`; composition
roots reach `BindingRuntime` through the implementation crate. The
`thread_boundary_ui` compiler test pins E0603 for importing the facade's module
itself and compiles both intended caller paths. Checking the module directly
keeps an exposed empty module from hiding a privacy regression behind a missing
trait import.

### Design-neutral and Material preludes

The design-neutral editing primitives are
[`flui_widgets::RawTextField`](crates/flui-widgets/src/text/text_field.rs) and
`flui_widgets::RawTextFormField`; `RawTextFieldState` names the raw field's state, and the shared
`SubmitCallback` stays in `flui-widgets`. Material owns `TextField` and `TextFormField`.

`flui::prelude` is feature-independent and design-system-neutral: it exports no Material names, and
enabling `material` never changes what an existing prelude name resolves to. `flui-material` owns
`flui_material::prelude`, also available through the facade as `flui::material::prelude`. An
application that wants Material's glob selects it explicitly:

```rust
use flui::material::prelude::*;
use flui::prelude::*;
```

This ownership rule is part of [ADR-0315](docs/adr/ADR-0315-first-party-packages-remain-train-aligned-facade-features.md) §3.

### Standing first-party package features

The facade keeps `default = []` and permanent opt-in `material`, `cupertino`, `devtools` and
`hot-reload` features with their corresponding re-exports. Official capabilities use the same
mechanism behind off-by-default features and appear as `flui::services::<name>`. The facade is the
only standing aggregation exception to the core dependency-direction rule; aggregation gives no
core crate a package dependency.

An aggregated package ships with the framework train and participates in facade semver with every
feature enabled. It must leave the facade before adopting an independent release cadence. See
[ADR-0315](docs/adr/ADR-0315-first-party-packages-remain-train-aligned-facade-features.md) §1–§4.

### Explicit authoring modules

`painting`, `rendering`, and `interaction` expose canonical framework types through
explicit reexports. They complement the existing `view`, `widgets`, `types`, and
`foundation` modules rather than introduce wrapper types or a second widget model.
A support type has one canonical facade path in its owning layer: pointer,
gesture and hit-test vocabulary is under `interaction`; painting values and
custom-painter contracts are under `painting`; render-object, layout-delegate,
scroll-state and semantics contracts are under `rendering`. The widget catalog's
public modules re-export catalog-owned views and their local configuration, not
support types owned by another layer. Its prelude is a curated convenience
namespace, not another canonical owner. SDK-only render-catalog values live in
its Evolving `pipeline` module rather than acquiring a second facade home.

The export set follows concrete extension tasks: implement `CustomPainter`,
implement box/sliver rendering behind a `RenderView`, and create gesture
recognizers. Types needed to name override signatures belong to the same usable
surface: a painter needs `Canvas` and `SemanticsBuilder`, render objects need
typed contexts, arity, parent data, invalidation and semantics contracts, and
gesture callbacks need their detail payloads. Geometry and repaint notifications
remain available through `geometry` and `foundation::Listenable`.

These modules do not reexport entire implementation crates. Storage arenas,
pipeline owners, GPU execution, and the concrete render-object catalog are not
application extension contracts. Exported forwarding macros use `$crate` and a
hidden public type reexport in their defining crate, so dependency renaming or
access through the facade does not require extra consumer dependencies.

[Iced's advanced API](https://docs.rs/iced/latest/iced/advanced/index.html)
demonstrates a deliberate extension surface for custom widgets. FLUI adopts a
task-oriented grouping, keeping its typed Rust protocols and contexts. No extra `advanced` feature is required: these runtime
dependencies already participate in ordinary widget applications. Sources were
consulted on 2026-09-19.

### Testing is an opt-in development capability

The `testing` feature exposes the existing deterministic headless driver, widget
layout/input harness, render-object harness, accessibility queries, and replay
tools under `flui::testing`. It activates optional `flui-testing` plus the widget
and rendering harness features. Consumer applications enable it in their
`dev-dependencies`; no default feature activates the test driver. The workspace
topology policy records this optional facade edge explicitly.

External consumer tests use only `flui`, including a renamed dependency. They run
custom painting, render layout and paint, forwarding-macro queries, virtual-time
gestures, and pointer-driven widget rebuilds. They also exercise a downstream
recognizer's public extension traits, registering
competing members and observing arena acceptance and rejection after pointer
movement. Separate normal dependency graphs
with no feature and with a catalog must exclude testing and hot reload. Tests
in the facade's own package alone cannot establish that property because its
development dependencies expose implementation crates.

`testing::rendering::collect_commands` and `DrawKind` expose the existing
rendering test projection for assertions on a committed layer tree. A retained
offstage element alone cannot satisfy a command assertion. The projection walks
reachable picture commands, including nested display lists; it does not establish
pixel visibility through clipping or opacity and does not replace GPU readback.
This lets facade-only consumers distinguish a current page's painted text from
retained page elements.

### Products and registry support have different roles

The facade and CLI are the two products; implementation crates required by their
published manifests are registry support. Publication roles live in the workspace's
`tier` and `tier-kind` declarations ([ADR-0081](docs/adr/ADR-0081-workspace-tiers-and-reach-facts.md)),
while private examples/tools retain `publish = false`.
No parallel release package list is maintained. Cargo's normalized dependency
rules define the distribution closure: optional/build/target declarations and
versioned dev-dependencies count; versionless dev-dependencies do not. This is a
Rust packaging decision. The
[Cargo dependency reference](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#development-dependencies)
and [package command](https://doc.rust-lang.org/cargo/commands/cargo-package.html)
are the source contracts. Fixture tests inspect actual tiny archives and preserve
a separate regression for fresh dev-cycle resolution failure. Closure validity
is not an assertion that every selected version can already be packaged against
a registry. The [release requirements](docs/plans/specs/release/requirements.md) cover the
remaining distribution work.
