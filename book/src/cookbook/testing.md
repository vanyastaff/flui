# Testing

Flutter counterpart: `WidgetTester`, `find`, `testWidgets`.

Enable `flui`'s `testing` feature in your development dependency and write a test the way a user
uses the screen: find what they see, act on it, expect what they should see next.

```rust
use flui::prelude::*;
use flui::testing::scenario::matcher::exists;
use flui::testing::scenario::{Tester, find};

# #[derive(Clone, StatefulView)]
# struct CounterView;
# struct CounterState { count: Signal<usize> }
# impl StatefulView for CounterView {
#     type State = CounterState;
#     fn create_state(&self, ctx: &dyn LifecycleContext) -> CounterState {
#         CounterState { count: ctx.signal(0) }
#     }
# }
# impl ViewState<CounterView> for CounterState {
#     fn build(&self, _view: &CounterView, ctx: &dyn BuildContext) -> impl IntoView {
#         let count = self.count;
#         Column::new((
#             Text::new(count.get(ctx).to_string()),
#             RawButton::new(Text::new("Increment"))
#                 .on_pressed(move |ctx| count.update(ctx, |n| *n += 1)),
#         ))
#     }
# }
// In an application this is the body of a `#[test] fn counter_increments()`.
let mut t = Tester::mount(CounterView);
t.expect(&find::text("0"), exists());
t.tap(&find::button("Increment"));
t.expect(&find::text("1"), exists());
t.finish();
```

The view under test is a counter like the one in
[View, Element, RenderObject](../concepts/view-element-render.md), with a labelled button; its
definition is hidden here.

`Tester::mount` mounts the view as `run_app` does, with the runtime's root scopes, in an 800 by
600 window on a manual clock (`Tester::mount_in` with a `TestWindow` for another size or
platform). `find` builds the same `Find`s the driver and an agent use, over the semantics tree,
so the test reads what a screen reader reads. A tap lands at the centre of the element's visible
part through the window's own input path, after the tester checked it is visible, enabled and
not covered; it returns once the frame after the input ran. An expectation waits while the
application has a frame or work due now, and fails at once, listing what the window shows, when the
application is idle: advance the clock yourself with `pump_for` to reach a timer. A `Find` names
exactly one element and a second match fails the step; `.all()` with `count(n)` counts them, and
`exists()`, `gone()`, `name`, `value`, `checked()`, `enabled()` and `focused()` are the matchers.
`finish` ends the test with its end checks: the application settled, unmounted, nothing left running.

A failure names what was looked for and what the window showed instead. A widget that publishes
no semantics is found by the identifier you give a group around it,
`Semantics::new().container(true).role(SemanticsRole::Group).identifier(..)`, and `find::id`.

`flui::testing::rendering` tests a render object of your own without widgets. The
[test tiers](https://github.com/vanyastaff/flui/blob/main/docs/testing.md) map the framework's own
test pyramid.
