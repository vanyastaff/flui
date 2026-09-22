# State: setState, InheritedView, ValueNotifier

FLUI has three ways state enters the tree, matching Flutter's three:

## `setState`

A `ViewState` schedules its own rebuild by calling `Element::set_state`/`set_state_scheduled`
(`crates/flui-view/src/element/unified.rs`) — the doc comment on it says, in as many words, that
it works "like Flutter's `setState`". In application code you don't usually call it directly:
`StateCell<T>`/`StateHandle<T>` (`crates/flui-view/src/state_cell.rs`, re-exported from
`flui::prelude`) wrap that call behind a small handle you `bind(ctx)` once in `init_state`, then
mutate with `.update(...)`/`.set(...)`:

```rust,ignore
count.update(|n| n + 1);
```

*(from `examples/counter.rs` — copied verbatim.)* This replaces the older pattern of a raw
`Rc<Cell<T>>` field plus a hand-threaded rebuild handle with one field.

## `InheritedView`

```rust,ignore
pub trait InheritedView { type Data; /* ... */ }
```

*(`crates/flui-view/src/view/inherited.rs`)* — the equivalent of Flutter's `InheritedWidget`: data
placed higher in the tree, read from below via `ctx.depend_on::<T>()`, with `update_should_notify`
deciding whether a dependent rebuilds when the data changes.

## `ValueNotifier`

```rust,ignore
pub struct ValueNotifier<T: Clone> { /* value, plus a ChangeNotifier */ }
```

*(`crates/flui-foundation/src/notifier.rs`, whose own doc comment says "Similar to Flutter's
`ValueNotifier`")* — a `ChangeNotifier` (the base "something changed, notify listeners" primitive)
that holds a single value and notifies on change. Useful for state that needs to be observed from
outside the `View`/`Element` tree, or shared across more than one subtree without going through
`InheritedView`.

## What's not here

This book does not describe a reactive "signals" system: there is no such primitive in `crates/`
today. Planned: realm-scoped signals — see [PR #1242](https://github.com/vanyastaff/flui/pull/1242)
(ADR-0074, status: draft) — but until that lands, the three mechanisms above are the whole state
story.
