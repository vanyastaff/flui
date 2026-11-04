# State: signals, InheritedView, ValueNotifier

FLUI has three ways state enters the tree. Application and element-local state is a presentation-scoped
signal; scope-wide data is an `InheritedView`; a controller that lives outside the tree is a `ValueNotifier`
or another `Listenable`.

## Signals

`flui::prelude::Signal<T>` (ADR-0074, placed by ADR-0085) is a `Copy` handle to a value in the
presentation's reactive graph. `examples/counter.rs` keeps its count in one: created in `init_state`
(`self.count = ctx.signal(0)`), read in `build` (`count.get(ctx)`, which subscribes the element), and written
from an event callback through the `cx` it receives (ADR-0086):

```rust,ignore
RawButton::new(Text::new("Increment"))
    .on_press(move |cx| count.update(cx, |n| *n += 1)),
```

*(from `examples/counter.rs` — copied verbatim.)* `build` has no `cx`, so `count.set(cx, ..)`
cannot be written there; a write from `build` through another route is refused at run time
(`SignalError::WrittenDuringBuild`). A widget that has to write from a callback with no `cx`
takes a `WriterSource` in `init_state` and opens one: `source.write(|cx| count.set(cx, 0))`. A write rebuilds exactly the
elements that read the signal.

## `InheritedView`

```rust,ignore
pub trait InheritedView { type Data; /* ... */ }
```

*(`crates/flui-view/src/view/inherited.rs`)* — data
placed higher in the tree, read from below via `ctx.depend_on::<T>()`, with `update_should_notify`
deciding whether a dependent rebuilds when the data changes.

## `ValueNotifier`

```rust,ignore
pub struct ValueNotifier<T: Clone> { /* value, plus a ChangeNotifier */ }
```

*(`crates/flui-foundation/src/notifier.rs`)* — a `ChangeNotifier` (the base "something changed, notify listeners" primitive)
that holds a single value and notifies on change. Useful for state that needs to be observed from
outside the `View`/`Element` tree, or shared across more than one subtree without going through
`InheritedView`.

## What's not here

A full chapter on signals (derived values, collections, cross-thread writes through
`SignalSender`) is still to be written; the section above covers what the examples use.
