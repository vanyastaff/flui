### Changed

- A `Canvas` refuses non-finite input when it records it, by the op's kind: a draw holding a NaN or infinity (or recorded under a non-finite transform) is left out, a clip becomes an empty clip, and a save-layer opens unbounded, so one bad value costs that op instead of failing the frame. `DisplayList::non_finite()` counts the refusals as `NonFiniteOps`, and `Canvas::finish` warns once when one happened. The canvas's finiteness debug assertions are gone, so debug and release builds behave alike (ADR-0306 §4).
- `Canvas::reset` resets the canvas before it drops what it recorded, so a font's bytes whose destructor panics leave an empty canvas (ADR-0306 §7).

### Removed

- `Canvas::draw_picture`. Splice a recorded run with `DisplayList::append_isolated` (ADR-0306 §6).
