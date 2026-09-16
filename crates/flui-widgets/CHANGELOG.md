# Changelog

All notable changes to `flui-widgets` are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning: per `docs/release.md` policy.

## [Unreleased]

### Added

- Initial `flui-widgets` Core.1 vertical-slice catalog.
- Layout family: `Padding`, `Align`, `Center`, `SizedBox`, `ConstrainedBox`, `LimitedBox`, `Transform`, `AspectRatio`, `Baseline`, `FittedBox`, `FractionallySizedBox`, `FractionalTranslation`.
- Flex/stack family: `Row`, `Column`, `Flex`, `Expanded`, `Flexible`, `Stack`, `Positioned`.
- Paint/effect family: `ColoredBox`, `DecoratedBox`, `Opacity`, `ClipRect`, `ClipRRect`, `ClipOval`, `RepaintBoundary`.
- Scrolling family: `SingleChildScrollView`, `ListView`, `SliverFixedExtentList`, `SliverOpacity`, `SliverPadding`, `SliverToBoxAdapter`, `Viewport`.
- Pointer family: `Listener` (raw pointer routing with `HitTestBehavior`), `GestureDetector` (`on_tap` plus `on_pan_start`/`on_pan_update`/`on_pan_end`, where recognizers compete in the presentation binding's shared arena).
- Modifiers: `IgnorePointer`, `AbsorbPointer`, `Offstage`.
- Text widget: `Text` over `RenderParagraph`.
- Transition family: `FadeTransition`, `ScaleTransition`, `RotationTransition` driven by `flui-animation`.
- Stateful widget harness and 64 integration tests covering layout parity, `setState`, scroll, gestures, transitions, and composition.

### Changed

- Widget-visible consequences of `flui-animation`'s zero-duration synchronous
  settle (issue #1171): an implicitly-animated widget (`AnimatedContainer`,
  `AnimatedOpacity`, …) retargeted with `Duration::ZERO` now lays out the new
  target on the SAME pump that observes the widget's new configuration,
  instead of one frame later (`ImplicitController::restart_from_zero`'s
  `forward_from(Some(0.0))` settles at the call). A `Navigator` route pushed
  or popped with `transition_duration(Duration::ZERO)` finalizes its POP
  synchronously too: `handle_pop` reads `finished_when_popped()` in the same
  call `did_pop` returns from, so the entry disposes with no `Popping` park
  and no pump in between. PUSH does not get the same synchronous settle: the
  underlying `AnimationController` still completes at the call, but
  `RouteLifecycle::Pushing -> Idle` is driven by a `RouteCommand` a
  continuation queues post-flush, so it still needs one pump regardless of
  how quickly the controller itself resolved.
