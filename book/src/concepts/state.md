# State: signals, InheritedView, ValueNotifier

Flutter counterpart: `State.setState`, `InheritedWidget`, `ValueNotifier`.

FLUI has three ways state enters the tree. Application and element-local state is a
presentation-scoped signal; scope-wide data is an `InheritedView`; a controller that lives
outside the tree is a `ValueNotifier` or another `Listenable`.

## Signals

`Signal<T>` (ADR-0074) is a `Copy` handle to a value in the presentation's reactive graph. A state
creates its signals in `create_state`, so it is born with them; `build` reads them, which
subscribes the element; an event callback writes them through the `ctx` it receives (ADR-0086):

```rust
use flui::prelude::*;

#[derive(Clone, StatefulView)]
struct Counter;

struct CounterState {
    count: Signal<u32>,
}

impl StatefulView for Counter {
    type State = CounterState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> CounterState {
        CounterState { count: ctx.signal(0) }
    }
}

impl ViewState<Counter> for CounterState {
    fn build(&self, _view: &Counter, ctx: &dyn BuildContext) -> impl IntoView {
        let count = self.count;
        RawButton::new(Text::new(format!("Pressed {} times", count.get(ctx))))
            .on_pressed(move |ctx| count.update(ctx, |n| *n += 1))
    }
}
```

`build` reads signals but never writes them: its `BuildContext` is not a context a write accepts,
so a write there does not compile, and a write opened through a `WriterSource` while a build runs
is refused at run time (`SignalError::WrittenDuringBuild`). A write rebuilds exactly the elements
that read the signal. A widget that has to write from a callback with no `ctx` (one shaped `Fn()`
or `Fn(&str)`) takes a `WriterSource` from `ctx.writer_source()` in `create_state`, keeps it in its
state, and moves a clone into the callback, which writes through it:
`source.write(|ctx| count.set(ctx, 0))`.

## `InheritedView`

Data placed higher in the tree and read from below. A descendant reads it with
`ctx.depend_on::<T, _>(…)`, which also registers the dependency, and `update_should_notify`
decides whether dependents rebuild when the data changes:

```rust
use flui::prelude::*;
use flui::view::{BoxedView, InheritedView, impl_inherited_view};

#[derive(Clone)]
struct Accent {
    color: Color,
    child: BoxedView,
}

impl InheritedView for Accent {
    type Data = Color;

    fn data(&self) -> &Color {
        &self.color
    }

    fn child(&self) -> &BoxedView {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.color != old.color
    }
}

impl_inherited_view!(Accent);

#[derive(Clone, StatelessView)]
struct Swatch;

impl StatelessView for Swatch {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let color = ctx.depend_on::<Accent, _>(|accent| *accent.data()).unwrap_or(Color::BLACK);
        Text::new(format!("{color:?}"))
    }
}

let tree = Accent { color: Color::rgb(0, 120, 215), child: BoxedView::new(Swatch.into_view()) };
# let _ = tree;
```

## `ValueNotifier`

A `ChangeNotifier` that holds a single value and notifies its listeners when the value changes.
It suits state observed from outside the view tree, or shared across more than one subtree
without an `InheritedView`:

```rust
use std::cell::Cell;
use std::rc::Rc;
use flui::foundation::ValueNotifier;

let mut volume = ValueNotifier::new(50_u8);
let heard = Rc::new(Cell::new(0));
let seen = Rc::clone(&heard);
let _subscription = volume.listen(Rc::new(move || seen.set(seen.get() + 1)));

volume.set_value(60);
assert_eq!(*volume, 60);
assert_eq!(heard.get(), 1);
```
