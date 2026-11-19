# flui_animation

Values over time for FLUI: animation controllers, curves, tweens, springs and other
simulations, keyframe tracks, and the `Vsync` registry that a presentation's frame clock drives.

The crate sits below `flui-widgets`, which builds its implicit animations and transition widgets
on it. Application authors reach it as `flui::animation` (or `flui_sdk::animation`); every
public item is at the crate root, and `curves` holds the easing presets.

Every `rust` block in this document is compiled as a doctest against the current API. Lines
starting with `#` are hidden setup that the rendered page leaves out.

## Example

A controller produces progress from 0.0 to 1.0, a curve shapes it, a tween maps it to a value,
and a listener observes each change. In an application the controller is created with
`build_on(Some(&vsync))` and driven by the presentation's frames; here a manual controller is
sampled with `tick_at`.

```rust
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{Animatable, Animation, AnimationController, CurvedAnimation, Tween, curves};
use flui_foundation::Listenable;

let controller = AnimationController::builder(Duration::from_millis(300)).build();
let eased = Rc::new(CurvedAnimation::new(Rc::new(controller.clone()), curves::EASE_IN_OUT));
let width = Rc::new(Tween::new(0.0, 200.0).animate(eased));

let last = Rc::new(Cell::new(0.0));
let _listening = {
    let (width, last) = (Rc::clone(&width), Rc::clone(&last));
    controller.listen(Rc::new(move || last.set(width.value())))
};

controller.forward().expect("live controller");
controller.tick_at(Duration::from_millis(150));
assert_eq!(last.get(), 100.0); // halfway through an ease-in-out
controller.tick_at(Duration::from_millis(300));
assert_eq!(width.value(), 200.0);
```

A controller built with `build_on` is owned by the `DrivenController` it returns: keep that owner
for the widget's lifetime. Clones obtained from `controller()` observe the same run, and dropping
the owner unregisters it from the registry and cancels the run.

## Documents

- [Guide](docs/GUIDE.md): controllers, motion preferences, curves, tweens, keyframes,
  composition, physics, errors, diagnostics. The one user guide.
- [Architecture](docs/ARCHITECTURE.md): module map, ownership, invariants and the ADRs they come
  from.
- [Performance](docs/PERFORMANCE.md): allocation and per-frame cost rules, benchmarks.

## Features

- `diagnostics`: structured transition events and CPU spans (see the guide).
- `serde`: serialization of the animation data types. Error types are never serialized
  ([ADR-0381](../../docs/adr/ADR-0381-animation-errors-are-not-serialized.md)).
