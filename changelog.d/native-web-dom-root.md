### Added

- On the Web, the window reports the safe area: the part of each band `env(safe-area-inset-*)` names unsafe (a notch, rounded corners, a home indicator) that the canvas lies under, in `MediaQuery`'s padding.
- `flui.toml`'s `[web]` table: `base_href`, the path a web build is served from (an absolute path of plain segments; the page's `<base href>`), `description` and `theme_color`, which the page and the manifest state, and `page`, which names a project's own page template.
- `flui run`'s dev server serves the build under that base path and answers a browser navigating to a path that is no file (a deep link into the app's routes) with the page; a missing asset, a request that is not a navigation, a percent-encoded `..` and a symbolic link out of the build directory get a 404.
- Page templates: a template holds `{{flui_module}}` where the app starts and may hold `{{flui_base}}`, `{{flui_manifest}}` and `{{flui_stylesheet}}`.
- `InMemoryTextStore::set_single_line`, so a test can serve a field of many lines.

### Changed

- `flui build web` writes `index.html`, `flui.js` and `flui.css` on every build instead of copying `platforms/web/index.html`; the page has no inline script or style, so it runs under `script-src 'self'; style-src 'self'`. `flui platform add web` no longer scaffolds a page, and its manifest comes from the same renderer as the one a build writes for a project that keeps none.
- **An existing web project must act:** while `platforms/web/index.html` exists, `flui build web` stops with an error until the file is deleted (the generated page takes over) or named as the page template with `page = "platforms/web/index.html"` under `[web]` (add `{{flui_module}}` where the app starts).
- On a page with no `<canvas id="flui-canvas">`, the web window puts one `<flui-view>` root in the body, fixed over the viewport, holding the canvas, the text-input host and the semantics host in that order, instead of a `100vw`×`100vh` canvas; on the page's own canvas, the two hosts follow it. Closing the window takes out what it added and gives the page's canvas back its `width`, `height`, `tabindex` and inline style.
- A `#flui-canvas` that another FLUI instance's window made is no longer taken over: a second instance on the page makes a root of its own.

### Fixed

- iOS Safari no longer zooms the page into a text field the web window focuses: the hidden textarea and the accessibility tree's fields are 16 CSS pixels.
- On a phone, the textarea is focused inside the tap's handler, the one moment a mobile browser opens its keyboard: a tap with no field served focuses one that the field the app focuses for that tap takes over, and a tap on a field whose keyboard the user hid opens it again, while other taps and scrolls leave it closed. No focus the web text input makes scrolls the page.
