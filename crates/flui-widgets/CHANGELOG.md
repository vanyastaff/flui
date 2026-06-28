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
- Pointer family: `Listener` (raw pointer routing with `HitTestBehavior`), `GestureDetector` (`on_tap` plus `on_pan_start`/`on_pan_update`/`on_pan_end`, where tap and pan compete in a per-detector arena).
- Modifiers: `IgnorePointer`, `AbsorbPointer`, `Offstage`.
- Text widget: `Text` over `RenderParagraph`.
- Transition family: `FadeTransition`, `ScaleTransition`, `RotationTransition` driven by `flui-animation`.
- Stateful widget harness and 64 integration tests covering layout parity, `setState`, scroll, gestures, transitions, and composition.
