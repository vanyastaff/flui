### Changed

- A child that Flutter leaves nullable is set with `.child(…)` and may be absent:
  `AnimatedOpacity`, `AnimatedScale`, `AnimatedRotation`, `AnimatedAlign`, `AnimatedPadding`,
  `AnimatedSlide`, `FadeTransition`, `ScaleTransition`, `RotationTransition`, `SizeTransition`
  and `SlideTransition` drop the child argument of `new`; `AnimatedContainer::new()` takes no
  argument; `SliverFadeTransition` takes its sliver through `.sliver(…)`.
- A required child is an argument of `new`: `SafeArea::new(child)`,
  `InteractiveViewer::new(alternatives, child)`; their `.child(…)` builders are gone.
- The child goes last: `Dismissible::new(alternatives, child)`,
  `RefreshIndicator::new(alternatives, child)`, `PageView::new(alternatives, children)`.
- `Border`, `BorderSide`, `BorderDirectional`, `Shadow`, `BoxShadow` and `BoxDecoration`
  default their unit to logical pixels (`T: Unit = f64`): a signature writes `BoxDecoration`.
