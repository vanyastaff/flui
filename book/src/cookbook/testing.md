# Testing

Enable `flui`'s `testing` feature in your development dependency and write a test the way a user
uses the screen: find what they see, act on it, expect what they should see next.

```rust,ignore
#[cfg(test)]
mod tests {
    use super::*;
    use flui::testing::{Tester, find};

    #[test]
    fn counter_increments() {
        let mut t = Tester::mount(CounterView);
        t.expect(find::text("0")).to_exist();
        t.tap(&find::button("Increment"));
        t.expect(find::text("1")).to_exist();
    }
}
```

`Tester::mount` mounts the view as `run_app` does, with the runtime's root scopes, in an 800 by
600 window on a manual clock (`Tester::mount_in` with a `TestWindow` for another size or
platform). `find` builds the same locators the driver and an agent use, over the semantics tree,
so the test reads what a screen reader reads. A tap lands at the centre of the element's visible
part through the window's own input path, after the tester checked it is visible, enabled and
not covered; it returns once the frame after the input ran. An expectation waits while the
application has a frame or work due now, and fails at once, listing what the window shows, when the
application is idle: advance the clock yourself with `pump_for` to reach a timer.

A failure names what was looked for and what the window showed instead. A widget that publishes
no semantics is found by the identifier you give it with `Semantics::identifier` and `find::id`.

`flui::testing::rendering` tests a render object of your own without widgets. The
[test tiers](https://github.com/vanyastaff/flui/blob/main/docs/testing.md) map the framework's own
test pyramid.
