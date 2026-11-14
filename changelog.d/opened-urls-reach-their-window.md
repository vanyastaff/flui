### Added

- `Application::declares(LinkDeclarations)` names the URL schemes and web domains an application
  opens, and `Application::on_open_files` hears the files the operating system opens it with.
- A URL the operating system opens the application with reaches the `Router` of its window: the
  window it was addressed to, else the one focused last, else the first. A link that launched the
  application opens that window's first `Router` on it, so its initial page is never built
  (`OwnerPlatform::take_launch_opened` hands it to the runner before the first window opens).
- `OpenUrlRefused::reason`, a short label of why a URL was not opened.
