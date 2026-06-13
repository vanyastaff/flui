# AGENTS.md — flui-reactivity

Reactive state management with signals, memos, and hooks. Inspired by React Hooks.

**Status:** Not in workspace `default-members`. Build explicitly with `cargo build -p flui-reactivity`.

## What lives here

- **`Signal<T>`** — reactive state holder with automatic change tracking
- **`use_memo`** — memoized (cached) computations that update when dependencies change
- **`DependencyId`** — identifies reactive dependencies
- **Hooks system** — React-style hooks for FLUI

## Key constraints

- **Dependencies** — `parking_lot`, `dashmap`, `once_cell`, `futures`, `pin-project-lite`, `any_spawner`, `send_wrapper`
- **`async` feature** — gates `dep:tokio` for async reactivity
- **`serde` feature** — optional serialization
- **Edition 2021** — not on 2024 like the rest of the workspace
- **Not in workspace** — standalone crate, not compiled by default `cargo build --workspace`
