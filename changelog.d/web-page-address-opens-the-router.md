### Added

- On the web, the page's address opens the application: the web host reports `location.href` as
  the link it launched with, so the first `Router` opens on the page's path and query, and the
  URLs of the page's own origin (`location.origin`: scheme, host and port) open without declaring
  its domain. `OpenedUrl::port`, `OpenedUrl::same_origin` and `UiRuntime::set_page_origin`
  (`HeadlessHost::set_page_origin` in tests) carry it.
