### Changed

- `AnimatedSize::new()` takes no duration: its motion comes from `MotionBuilder` (`duration`, `curve`, `spring`, `motion`) and defaults to a critically damped spring of stiffness 400 instead of a linear curve. `reverse_duration` is removed; it never affected a resize.
- `AnimatedSize` animates the shown size itself rather than a progress through a tween: a resize retargeted mid-flight keeps the size's velocity, an underdamped spring overshoots, reduced motion lands on the exact size, and a motion-only rebuild carries the run on from the size shown. A curve run starts and arrives at rest, so a linear curve eases at both ends.
- `RenderAnimatedSize::new` takes a `SizeMotionHandle` instead of an `AnimationController` and a curve; the owning view holds the new `SizeMotion` (the shown size as an `AnimatedValue<Size>`), which binds the frame registry, sets the motion and reports completed runs. `set_curve`/`set_duration`/`set_reverse_duration` are removed.
- `AnimatedCrossFade` resizes with the same motion as its fade.
