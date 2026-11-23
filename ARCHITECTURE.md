# FLUI application facade

Application packages depend on `flui`. The workspace's implementation crates
are maintained as separate packages; their boundaries are not a dependency
checklist for application authors. `src/lib.rs` lists each module, the crate
behind it and the feature that gates it, and each feature of the manifest.

## Mapping decisions

### View and animation authoring exclude their runtime seams

The facade's `view` and `animation` modules are glob re-exports of `flui_view`
and `flui_animation` that shadow the crate's `__runtime` module with a private
one (ADR-0081 §4, ADR-0382). `__runtime` is the composition roots' seam in
`flui-view` and the frame driver's seam in `flui-animation`; an application
names neither. Composition roots reach it through the implementation crate.

Two tests pin the view half. The trybuild fixture
`tests/ui/thread_boundary/facade_runtime_seam_is_private.rs`, run by
`thread_boundary_ui`, expects E0603 for `use flui::view::__runtime`, and
`tests/ui_pass/facade_runtime_seam.rs` compiles the intended callers. The
animation half is the `compile_fail,E0603` doctest on `flui::animation`.
Naming the module directly keeps an exposed empty module from hiding a privacy
regression behind a missing trait import.

### `TextField` is the Material type; the widgets primitive is `RawTextField`

`flui::prelude` names `flui_material::TextField` when the `material` feature is
on, and `flui-widgets` names its theme-free text-editing primitive
`RawTextField`. Two same-named types in one glob would have needed one to
shadow the other. A shadow makes the meaning of `TextField` depend on a
feature, which breaks feature additivity: enabling a feature may add names but
not change what an existing name resolves to. Leaving the Material type out
instead lets `use flui::prelude::*` on a Material screen compile with an
unstyled field where a themed one was expected. With the rename, `TextField`
belongs to `flui-material` alone and the prelude lists it like every other
Material type.

### Authoring modules

`painting`, `rendering`, `interaction`, `semantics` and `app` re-export
canonical framework types by name. They introduce no wrapper types and no
second widget model, and they complement `view`, `widgets` and `foundation`.

The export set follows the extension tasks an application performs:

- `painting`: implement `CustomPainter` for `widgets::CustomPaint`, with
  `Canvas`, `DisplayList`, the paint, style and typography modules, and
  `SemanticsRegion`, `SemanticsRegionId`, `SemanticsRegionKey` and
  `SemanticsRegions`, which a painter reports to assistive technology.
- `rendering`: implement `RenderBox` or `RenderSliver` behind a
  `view::RenderView`, with typed contexts, the arity markers, parent data,
  constraints, delegates and invalidation. The built-in render objects are
  `rendering::objects`. `PipelineOwner` and `PipelineCell` are exported because
  the view layer's signatures name them (a lifecycle hook's pipeline handle, a
  custom element's owner); the render tree they own and the GPU compositor are
  not.
- `interaction`: create gesture recognizers and members of the gesture arena,
  and name the callback payloads, focus, hit-test and pointer vocabulary that
  handlers receive.
- `semantics`: the types a render object's
  `describe_semantics_configuration` and `widgets::Semantics` take: roles,
  states, labels, actions, relations, sort keys and text content.
- `app`: the application's configuration, the work it runs off the frame, how
  its windows close and what it does when a frame fails.

A type belongs when a signature an application writes names it. The modules do
not re-export whole implementation crates: storage arenas, the render tree,
the semantics tree and its platform bridges, and GPU execution stay in their
crates. The forwarding macros that crates export use `$crate` and a hidden
public type re-export in their defining crate, so a renamed dependency or
access through the facade needs no extra consumer dependency. No `advanced`
feature gates these modules: the crates behind them are already in an ordinary
widget application's graph.

### Testing is an opt-in development capability

The `testing` feature publishes `flui::testing`. It activates the optional
`flui-testing` dependency, `flui-rendering`'s `testing` feature and
`flui-platform`'s `scripted` feature. Consumer applications enable it in
`dev-dependencies`; no default feature activates it.

`flui::testing` exposes what `src/testing.rs` re-exports:

- `Tester` mounts an application on a manual clock and acts through the
  window's input path; `find` builds the `Locator`s it looks up over the
  window's semantics tree; `Expectation`, `TestWindow`, `TestError` and the
  `Inspect*` types read the result.
- `DropLedger`, `MemoryStorage` with its holds and `CommitBarrier`, and
  `HostClock` for tests of ownership, storage and time.
- `testing::rendering`: `RenderTester` and its runs, `box_node` and
  `sliver_node`, for a render object tested without a widget tree, and
  `collect_commands` with `DrawKind`, which inspects the picture commands
  reachable from a committed layer tree. That projection walks nested display
  lists. It does not evaluate clipping or opacity and does not replace GPU
  readback; its use is telling a page's painted text from retained offstage
  elements that have no commands in that tree.
- `platform::capability::scripted`, scripted capability scenarios, under the
  same feature.

External consumer tests use only `flui`, including a renamed dependency
(`tests/facade_consumer.rs`, with its fixtures). They run custom painting,
render layout and paint, forwarding-macro queries, virtual-time gestures,
pointer-driven rebuilds and a downstream recognizer that competes in the arena.
`ordinary_facade_graph_excludes_test_support` runs `cargo tree` over
consumer projects with and without default features and asserts the normal
graph holds neither catalog, `flui-testing`, `flui-system` nor
`flui-hot-reload`. The facade's own package cannot establish that, because its
development dependencies expose implementation crates.

### Products and registry support

The facade and the CLI are the two products an application author installs.
The implementation crates their published manifests require are published as
registry support, and examples and tools stay `publish = false`. The roles are
the `tier` and `tier-kind` declarations in each manifest
([ADR-0081](docs/adr/ADR-0081-workspace-tiers-and-reach-facts.md)); no second
list of release packages exists.

Cargo's normalized dependency rules define the distribution closure: optional,
build and target-specific declarations and dev-dependencies that carry a
`version` count, and a dev-dependency with only a `path` is dropped from the
published manifest
([Cargo reference](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#development-dependencies)).
The root manifest keeps such path-only dev edges for the crates that would
otherwise form a publication cycle, each with its own comment. The facade's
`include` list decides what its archive carries. A closure that is valid does
not mean every selected version can already be packaged against a registry;
the [release requirements](docs/plans/specs/release/requirements.md) cover the
remaining distribution work.
