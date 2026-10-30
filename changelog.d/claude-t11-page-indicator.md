### Added

- `PageIndicator` in `flui-widgets` draws a dot per page of a `PageView`. `PageIndicator::controlled` follows a `PageController` and turns its pages by a tap or by assistive technology's increase and decrease. `PageIndicator::observed` only shows an `Animation<f64>` position. Two neighbouring dots share the highlight between pages. With many pages, a window of dots follows the position and its edge dots shrink. The indicator announces "page n of N", follows the text direction, and is hidden with one page or none. `PageIndicatorStyle::try_new` sets the dot, gap and window and refuses sizes that cannot be drawn.
- `CupertinoPageControl` in `flui-cupertino`, the iOS page dots on top of `PageIndicator`.
