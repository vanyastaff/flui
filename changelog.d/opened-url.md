### Added

- `UiRuntime::open_url(presentation, url)` opens a URL the platform hands a window: it refuses one
  over `OpenedUrl::MAX_LEN` bytes, splits it (`OpenedUrl::parse`, RFC 3986 appendix B, the path
  without its authority as Flutter's `uri.path`), admits it by the application's
  `LinkDeclarations` (`url_schemes`, and `associated_domains` for `http`/`https`), and applies its
  path as a `Go`. A refusal is an `OpenUrlRefused`. `UiRuntime::set_link_declarations` sets the
  declarations; `HeadlessHost::open_url` and `HeadlessHost::set_link_declarations` drive the same
  path in tests.
