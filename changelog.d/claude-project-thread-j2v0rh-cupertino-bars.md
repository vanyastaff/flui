### Added

- `flui_cupertino::ObstructingPreferredSizeView`: a `PreferredSizeView` that answers `should_fully_obstruct(ctx)`. `CupertinoNavigationBar` and `CupertinoTabBar` implement it, answering whether their background is fully opaque.
- A translucent `CupertinoNavigationBar` or `CupertinoTabBar` (the theme's default background) blurs what lies behind it with a sigma-10 backdrop blur over the bar; an opaque bar keeps the filter switched off, so a bar whose background crosses full opacity keeps its slots' state.

### Changed

- `CupertinoPageScaffold::navigation_bar` takes an `ObstructingPreferredSizeView`. Under a bar that does not fully obstruct (the default, translucent navigation bar) the content runs under the bar, and its `MediaQuery` `padding.top` is the bar's height plus the top inset; under an opaque bar the content is padded below it, as before.
- The navigation and tab bars' default hairline border is one device pixel (`1 / device_pixel_ratio` logical pixels) instead of one logical pixel.
- `CupertinoNavigationBar` lays out `middle` between `leading` and `trailing`: centred on the bar while it fits clear of both, otherwise moved toward the side with room and limited to the space between them, so a long title no longer paints over the side slots. `leading` and `trailing` follow the ambient text direction.

### Removed

- `CupertinoTabBar::opaque`: use `should_fully_obstruct(ctx)` from `ObstructingPreferredSizeView`.
