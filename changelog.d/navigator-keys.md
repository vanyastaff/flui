### Changed

- A navigator's first route no longer takes the keyboard focus on mount; the first Tab enters it.
- Keys reach only the current route: a route covered by a push, and a route leaving after a pop, stop admitting focus, so Tab and activation never land on them mid-transition.

### Added

- Inside a navigator, `BackIntent` (Alt+Left, `BrowserBack`) pops the current route, and `DismissIntent` (Escape) pops a route whose barrier is dismissible; Escape never pops a page.
- A pushed route's first focus is provisional: its `autofocus` control replaces it until the user moves the focus. The focus restored on pop is final.
