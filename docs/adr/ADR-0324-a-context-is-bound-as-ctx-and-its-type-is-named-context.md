# ADR-0324: A context is bound as `ctx`, and its type is named `…Context`

- **Status:** Accepted (2026-11-12)
- **Date:** 2026-11-12
- **Related:** [ADR-0303](ADR-0303-a-rule-takes-the-first-rung-that-states-it-and-an-xtask-gate-is-the-last.md)
  (the ladder the gate is admitted under); [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md)
  §4 (the conditions a gate meets); `docs/plans/specs/naming/requirements.md` N1 (one spelling of
  a context); `docs/extending.md` "A widget's constructor and names" (the author-facing
  statement)

## Context and problem statement

The first signatures a newcomer reads and copies are a widget's `build` and an event handler.
Before this decision a build, lifecycle, render-object or provider context was bound as `ctx` at
about 1,300 sites and as `cx` at about 170, an event context and a `poll`'s task context were
`cx`, and context types were spelled three ways (`BuildContext`, `EventCx`, `BoxIntrinsicsCtx`).
A reader could not tell from a name which context a closure had, and every new widget copied
whichever neighbour it opened. The naming requirements' N1 asks for one spelling. Which one, and
what keeps it?

## Decision drivers

- One name for one role: a context reads the same in `build`, a layout pass, an event handler and
  a `poll`, with no exception a reader has to remember.
- The spelling most of the tree already has, so the change renames the minority.
- Flutter's `BuildContext context` reads as `ctx`.
- The rule holds without review, on the first rung of ADR-0303 that can state it.

## Considered options

1. `cx` for every context, as N1 first proposed.
2. `ctx` for every context except the task context, which keeps the standard library's `cx`.
3. `ctx` for every context.

## Decision

Option 3, with N1's type names.

```rust
fn build(&self, ctx: &dyn BuildContext) -> impl IntoView;
fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_>) -> Size;
.on_tap(move |ctx| count.update(ctx, |n| *n += 1))
std::future::poll_fn(move |ctx| receiver.poll_recv(ctx))
```

- A binding of a context, of any kind, is `ctx`: a parameter, a closure parameter, a `let`.
- A context type is named `…Context`; `Cx` and `Ctx` leave type names.
- `cargo xtask context-names` holds the binding rule. In every workspace member's library and bin
  targets, read through `source_tree` as `try-new` reads them, it reports a typed binding named
  `cx` or `_cx` whose type, through references, `dyn` and `impl`, ends in `Context`, `Cx` or
  `Ctx`, and an untyped closure parameter named `cx` or `_cx`. It has no allowlist.

The gate meets ADR-0303 §3:

- **Rung 1, a type**: a binding's name is not part of any type.
- **Rung 2, a lint**: clippy's `disallowed_names` sees the name alone. `cx` is also the right
  name for a centre or a cubic's coefficient in about fifteen geometry files (the engine's
  tessellation, `window_insets`' easing curve, the Wayland frame), and a Win32 `SIZE` has a field
  `cx`; the lint would refuse those. rustc has no lint over names and types.
- **Rung 3, a tool CI runs**: typos, taplo, lychee and cargo-deny do not parse Rust.
- **Rung 4, a test of the owning crate**: the rule is about every crate's text at once; the
  crates that own the context types cannot read their dependents' sources from their tests.

It reads structured data (a `syn` AST), has a `--self-test` that `checks_contract` runs, and has
no list to keep (ADR-0078 §4 conditions 1–4).

### Consequences

- Good: one name everywhere; the gate keeps it.
- Good: an untyped event callback, the form copied most, is covered without knowing its type.
- Bad: `poll` departs from the standard library documentation's `cx`; FLUI has about fifty such
  sites, in the scheduler, runtime and platform glue.
- Bad: one more `xtask` command, about two hundred lines with its self-test.
- Neutral: test targets, examples and generic parameters (`cx: &S` with `S: ReadScope`) are out of
  the gate's reach; the rename moved them, and review keeps them.

### Confirmation

`cargo xtask context-names`, run by `cargo xtask checks`, and its self-test through
`every_self_test_reports_exactly_its_planted_findings`.

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
