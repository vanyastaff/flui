### Added

- On the Web, the window reports the safe area: the part of each band `env(safe-area-inset-*)` names unsafe (a notch, rounded corners, a home indicator) that the canvas lies under, in `MediaQuery`'s padding.
- `flui.toml` takes `[web] base_href`, the path a web build is served from; the page names it as its `<base href>`, so an app can be hosted under a sub-path.

### Changed

- `flui build web` writes `index.html` on every build (the module import, the manifest link, `viewport-fit=cover`, the `<base href>`) instead of copying `platforms/web/index.html`, which it no longer reads and `flui platform add web` no longer writes; a project with no `platforms/web/manifest.json` gets one named after the app.
- On a page with no `<canvas id="flui-canvas">`, the web window puts one `<flui-view>` root in the body, fixed over the viewport, holding the canvas, the text-input host and the semantics host in that order, instead of a `100vw`×`100vh` canvas; on the page's own canvas, the two hosts follow it. Closing the window takes out what it added and gives the page's canvas back its own `style` and `tabindex`.
- A `#flui-canvas` that another FLUI instance's window made is no longer taken over: a second instance on the page makes a root of its own.

### Fixed

- iOS Safari no longer zooms the page into a text field the web window focuses: the hidden textarea and the accessibility tree's fields are 16 CSS pixels.
- A mobile browser that does not focus a tapped canvas, or a field that attaches after its tap ended, no longer leaves the soft keyboard closed: the editor takes focus inside the tap, or at the next touch on the canvas.
