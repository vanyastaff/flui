# FLUI application facade

Application packages depend on `flui`. The workspace's implementation crates
remain independently maintained internally; their package boundaries are not a
dependency checklist for application authors.

## Mapping decisions

### `TextField` prelude collision — renamed the widgets primitive, not shadowed

`flui-widgets` and `flui-material` used to ship two distinct types both named
`TextField` — a design-agnostic text-editing primitive and the M3-styled
input. Three revisions of how `flui::prelude` handles this, kept in order
because each corrected a real flaw in the one before it:

1. **Omission.** The first revision left the Material type out of the
   curated glob entirely, reasoning that "a curated glob cannot carry both
   without one silently shadowing the other," and kept `flui_widgets::TextField`
   as the ambient default. That reasoning stated the mechanism correctly but
   reached the wrong conclusion for this prelude's actual audience: an
   application author writing an ordinary Material screen with
   `use flui::prelude::*;` who reached for `TextField` got a *compiling*
   result either way, so the omission never surfaced as an error — it
   surfaced as an unstyled field silently rendered where a themed one was
   expected, exactly the "quiet Material-shaped mistake" this facade's
   curation exists to prevent for every other Material type.
2. **Explicit shadow.** The second revision had `flui::prelude` explicitly
   name `flui_material::TextField` in its material re-export list (Rust's
   explicit-import-over-glob-import rule, the same pattern every other name
   in that list already follows, e.g. `ElevatedButton`/`IconButton` are also
   explicit despite not being ambiguous), shadowing the
   `flui_widgets::prelude::*` glob's copy of the same name whenever the
   `material` feature was on. This fixed the silent-mistake problem but
   introduced a different one: `TextField`'s meaning then depended on
   whether the `material` feature happened to be enabled, which violates
   Cargo's feature-additivity contract — enabling a feature is supposed to
   only *add* symbols, never change what an existing name already resolves
   to for code that hasn't opted into the new feature's own symbols.
3. **Rename (current).** `flui-widgets`' primitive is now
   [`flui_widgets::RawTextField`](../crates/flui-widgets/src/text/text_field.rs),
   not `TextField` — a breaking rename, sanctioned pre-1.0. `TextField`
   belongs to `flui_material` alone, unconditionally; `flui::prelude` lists
   it in the material re-export block like every other Material type, with
   no shadowing and no feature-dependent meaning. `RawTextFieldState` was
   renamed alongside it for the same reason (an orphaned `TextFieldState`
   with no `TextField` beside it would have been confusing); `SubmitCallback`
   (the shared `Rc<dyn Fn(&str)>` alias both `on_submitted` methods use) was
   made `pub` and exported from `flui-widgets` so `flui_material::TextField`
   reuses the same type instead of declaring its own copy.

The codebase's every real caller of `flui::material::TextField`
(`examples/material_demo`, `examples/workload_probe`, `examples/todo`)
already imported it explicitly under all three revisions and needed no
changes across any of them — every fix here was about what an *unqualified*
`TextField` resolves to for code that hasn't been written yet, never about
existing call sites.

### Explicit authoring modules

`painting`, `rendering`, and `interaction` expose canonical framework types through
explicit reexports. They complement the existing `view`, `widgets`, `types`, and
`foundation` modules rather than introduce wrapper types or a second widget model.

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
demonstrates a deliberate extension surface for custom widgets. Flutter groups
[rendering](https://api.flutter.dev/flutter/rendering/) and
[painting](https://api.flutter.dev/flutter/painting/) concepts into separate
libraries. FLUI adopts the task-oriented grouping, keeping its typed Rust
protocols and contexts. No extra `advanced` feature is required: these runtime
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
with and without the default catalog must exclude testing and hot reload. Tests
in the facade's own package alone cannot establish that property because its
development dependencies expose implementation crates.
### Products and registry support have different roles

The facade and CLI are the two products; implementation crates required by their
published manifests are registry support. Publication roles live in the existing
workspace layer inventory, while private examples/tools retain `publish = false`.
No parallel release package list is maintained. Cargo's normalized dependency
rules define the distribution closure: optional/build/target declarations and
versioned dev-dependencies count; versionless dev-dependencies do not. This is a
Rust packaging decision, not a Flutter protocol. The
[Cargo dependency reference](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#development-dependencies)
and [package command](https://doc.rust-lang.org/cargo/commands/cargo-package.html)
are the source contracts. Fixture tests inspect actual tiny archives and preserve
a separate regression for fresh dev-cycle resolution failure. Closure validity
is not an assertion that every selected version can already be packaged against
a registry. See [the beta release record](docs/BETA.md) for remaining blockers.
