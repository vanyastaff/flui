### Changed

- `BoxedView` is a shared view: cloning shares it instead of copying it, and an element that
  is offered the `BoxedView` it already holds is not built again. A parent that rebuilds for
  its own reasons (a focus edge, a route change) no longer rebuilds the child it hands back
  unchanged; what the child reads reaches it through its own state, inherited values or
  signals (ADR-0245). The tuple field is private: build one with `BoxedView::new(view)` and
  read it with `BoxedView::as_view()`.

### Fixed

- `Focus` keeps the elements under it when its node stops being able to take focus (a popup
  route over its page), so a gesture in flight under it, such as a long-press drag handed off
  from a context menu, is no longer cancelled.
- The first focus gain no longer rebuilds the whole app: a root that made a
  `ScrollController` in `build` kept its drag through a drag's start.
