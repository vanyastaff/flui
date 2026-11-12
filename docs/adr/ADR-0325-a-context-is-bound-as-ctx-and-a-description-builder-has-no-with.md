# ADR-0325: A context is bound as `ctx`, and a description's builder method has no `with_`

- **Status:** Accepted (2026-11-12)
- **Date:** 2026-11-12
- **Related:** [ADR-0303](ADR-0303-a-rule-takes-the-first-rung-that-states-it-and-an-xtask-gate-is-the-last.md)
  (the ladder the gates are admitted under); [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md)
  §4 (the conditions a gate meets); `docs/plans/specs/naming/requirements.md` N1 and N11 (one
  spelling of a context, one form of a builder method); `docs/extending.md` "A widget's
  constructor and names" (the author-facing statement)

## Context and problem statement

The first signatures a newcomer reads and copies are a widget's constructor, its `build` and an
event handler. Two of them had more than one spelling.

- **Contexts.** A build, lifecycle, render-object or provider context was bound as `ctx` at about
  1,300 sites and as `cx` at about 170, an event context and a `poll`'s task context were `cx`,
  and context types were spelled three ways (`BuildContext`, `EventCx`, `BoxIntrinsicsCtx`).
- **Builder methods.** A widget's optional parameters were bare (`Container::new().padding(…)`),
  while `AppConfig`, `WindowOptions`, the platform requests and a few widgets (`OverflowBox`,
  `IntrinsicWidth`) spelled the same role `with_title`, `with_alignment`. 878 `with_*` methods
  had four meanings between them, so a reader could not tell from the prefix which one a method
  had.

A reader could not tell from a name which context a closure had or whether `with_x` sets a
parameter or derives a value, and every new type copied whichever neighbour it opened. Which
spelling, and what keeps it?

## Decision drivers

- One name for one role, with no exception a reader has to remember.
- The spelling most of the tree already has, so the change renames the minority.
- Flutter is the source: `BuildContext context` reads as `ctx`, and a Flutter constructor's
  named parameter `title:` reads as `.title(…)`.
- Rust has no overloading: a bare name a builder takes is a name a getter cannot have (C-GETTER).
- The rule holds without review, on the first rung of ADR-0303 that can state it.

## Considered options

For contexts:

1. `cx` for every context, as N1 first proposed.
2. `ctx` for every context except the task context, which keeps the standard library's `cx`.
3. `ctx` for every context.

For builder methods:

4. `with_x` for every builder, as `AppConfig` had.
5. The bare name for every builder, on every type.
6. The bare name on a description type; `with_x` keeps its other meanings.

## Decision

Options 3 and 6.

```rust
fn build(&self, ctx: &dyn BuildContext) -> impl IntoView;
fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_>) -> Size;
.on_tap(move |ctx| count.update(ctx, |n| *n += 1))
std::future::poll_fn(move |ctx| receiver.poll_recv(ctx))

AppConfig::new().title("Counter").size(800, 600)
OverflowBox::new().alignment(Alignment::TOP_LEFT).max_width(f64::INFINITY).child(content)
RouteSettings::named("/details").arguments(id)          // read back as `settings.arguments`

Color::rgb(1, 2, 3).with_alpha(128)                      // a finished value's `withX`
RenderFlex::row().with_vertical_direction(direction)     // the chaining pair of a setter
Vec::with_capacity(16)                                   // a std-style constructor
canvas.with_transform(transform, |canvas| …)            // a closure run inside a scope
```

### 1. Contexts

- A binding of a context, of any kind, is `ctx`: a parameter, a closure parameter, a `let`.
- A context type is named `…Context`; `Cx` and `Ctx` leave type names.
- `cargo xtask context-names` holds the binding rule. In every workspace member's library and bin
  targets, read through `source_tree` as `try-new` reads them, it reports a typed binding named
  `cx` or `_cx` whose type, through references, `dyn` and `impl`, ends in `Context`, `Cx` or
  `Ctx`, and an untyped closure parameter named `cx` or `_cx`. It has no allowlist.

### 2. Builder methods

- A **description type** is one the author only assembles and hands to the framework: every type
  that implements a view trait, `Application`, and every type named
  `…Config/Options/Settings/Request/Builder/Params/Spec`. Its builder method takes `self` and
  returns `Self`, sets one optional parameter, and carries the parameter's bare name:
  `.title(…)`, not `.with_title(…)`.
- A description is read by its fields. When a host or a recognizer reads a parameter back, the
  field is `pub` and the type is `#[non_exhaustive]`, as Flutter's `RouteSettings` has final
  fields: `settings.arguments`, not `settings.arguments()`. A field and a method of one name do
  not collide in Rust, so the builder keeps the bare name. A getter of the builder's name is
  removed, never renamed to `get_x` (C-GETTER).
- A field whose invariant the constructor or the builder checks stays private behind its getter.
  Its builder keeps `with_x` and is listed with that reason in one line.
- `with_x` keeps its other meanings, on types the rule does not name:
  - Flutter's `withX` and `copyWith` on a finished value (`Color::with_alpha`,
    `TextStyle::with_color`, `Size::with_width`, `BoxConstraints`, `SliverGeometry`,
    `SliverConstraints`);
  - the chaining pair of a setter on a read-write object, where Flutter has a setter: on
    `Render*`, `Paint`, `*ParentData` and `WindowState` the bare name is the getter, `set_x` the
    setter and `with_x` its chaining form;
  - a constructor without `self` in the standard library's shape (`with_capacity`,
    `BoxDecoration::with_color`);
  - a closure run inside a scope (`Canvas::with_transform(&mut self, transform, f)`);
  - an operating system's snapshot a test builds (`SystemPreferences`, `DisplayInfo`).
- `cargo xtask builder-names` holds the rule. In the same targets it reports a method of an
  inherent `impl` named `with_*` that takes `self` by value and returns `Self`, on a type of its
  crate that an `impl` of a trait ending in `View`, an `impl_*_view!` call or a
  `#[derive(…View)]` names, or that is `Application` or has a listed suffix.
  `tools/xtask/allowlists/builder-names.toml` lists, per crate, type and method, the builders that
  still carry the prefix, each with its reason; the list only shrinks.

The gates meet ADR-0303 §3:

- **Rung 1, a type**: neither a binding's name nor a method's prefix is part of any type.
- **Rung 2, a lint**: clippy's `disallowed_names` sees a name alone. `cx` is also the right name
  for a centre or a cubic's coefficient in about fifteen geometry files (the engine's
  tessellation, `window_insets`' easing curve, the Wayland frame), and a Win32 `SIZE` has a field
  `cx`; the lint would refuse those. `wrong_self_convention` covers `as_`, `to_`, `into_`, `is_`
  and `from_`, not `with_`, and no lint knows which types are descriptions. rustc has no lint over
  names and types.
- **Rung 3, a tool CI runs**: typos, taplo, lychee and cargo-deny do not parse Rust.
- **Rung 4, a test of the owning crate**: each rule is about every crate's text at once; the
  crates that own the context and view traits cannot read their dependents' sources from their
  tests.

Each reads structured data (a `syn` AST) and has a `--self-test` that `checks_contract` runs
(ADR-0078 §4 conditions 1–3). `context-names` has no list to keep; `builder-names` has a list
that only shrinks, each entry with its reason (condition 4).

### Consequences

- Good: one name for a context everywhere, and one form for a description's parameter: what
  the author reads in Flutter's constructor is the method's name.
- Good: an untyped event callback, the form copied most, is covered without knowing its type.
- Good: `with_` is left with meanings a reader can tell apart by the type, and the gate needs no
  exception for them.
- Bad: `poll` departs from the standard library documentation's `cx`; FLUI has about fifty such
  sites, in the scheduler, runtime and platform glue.
- Bad: a builder of a description no longer has a getter of its name; a reader of a parameter
  reads a field, and a type that makes a field public gives up changing its representation
  without a break.
- Bad: two more `xtask` commands, about two hundred and five hundred lines with their self-tests,
  and a list to shrink crate by crate.
- Neutral: test targets, examples and generic parameters (`cx: &S` with `S: ReadScope`) are out of
  the gates' reach; the renames moved them, and review keeps them.

### Confirmation

`cargo xtask context-names` and `cargo xtask builder-names`, run by `cargo xtask checks`, and
their self-tests through `every_self_test_reports_exactly_its_planted_findings`.

## Pros and cons of the options

### 1. `cx` everywhere

- Good: short; the `std::task` convention.
- Bad: renames about 1,300 bindings to fix 170, and moves away from Flutter's `context`.

### 2. `ctx`, with `cx` kept for the task context

- Good: `poll` keeps the standard library's spelling.
- Bad: an exception the reader cannot see in an untyped `poll_fn` closure, and the gate cannot
  either, so untyped closures would fall out of its reach.

### 3. `ctx` everywhere

- Good: one name, no exception; the gate covers untyped closures.
- Bad: renames the event handlers' and `poll`'s bindings once.

### 4. `with_x` for every builder

- Good: the bare name stays free for a getter on every type.
- Bad: renames every widget's parameters (`Container::with_padding`), the form an author writes
  most, and reads unlike Flutter's named parameters.
- Bad: `with_` would mean "set" on a description and "derive" on a value, the ambiguity this
  decision removes.

### 5. The bare name everywhere

- Good: one form for every builder.
- Bad: on a read-write object the bare name is the getter Flutter's property reads as
  (`render.clip_behavior()`), so its chaining form needs another prefix, and a finished value's
  `with_alpha` would lose the `withX` it has in Flutter.

### 6. The bare name on descriptions

- Good: the form authors write most is the one they already have; the other meanings of `with_`
  stay, each recognisable by its type.
- Bad: a type has to be recognised as a description, by its view trait or its suffix; a
  description named otherwise is out of the gate's reach until it is renamed or its suffix listed.
