# AGENTS.md — flui-macros

Proc-macro crate (`proc-macro = true`). Must be a leaf — cannot depend on ordinary library crates.

## What lives here

- `#[derive(StatelessView)]` — emits `impl View` for stateless widgets
- `#[derive(StatefulView)]` — emits `impl View` for stateful widgets
- `#[derive(Animatable)]` — for custom spring-animatable types (used by `flui-animation`)
- `#[derive(Diagnosticable)]` — debug diagnostics support

## Key constraints

- Generated code uses **absolute paths** (`::flui_view::…`). Every consumer must have `flui-view` as a direct dependency
- Both View derives are re-exported from `flui_view::prelude` — widget authors write `use flui_view::prelude::*;`
- `flui-foundation` is a **dev-dependency only** (the derive emits paths into the consuming crate, not this one)
- Uses `syn 2.x` + `quote` + `proc-macro2` (standard derive stack)
