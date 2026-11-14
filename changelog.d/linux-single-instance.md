### Added

- `Application::single_instance(bool)`, with flui's `single-instance` feature (off by default): on Linux a later launch of the application hands the running instance its links and files and ends, and the running window comes forward with the launcher's activation token; on by itself when the application declares a URL scheme.
- `CurrentHost::start_single` and `HostStart` in `flui-native`, behind its `single-instance` feature, which keep one instance per user session on the session bus (`org.freedesktop.Application`).
- `LinkDeclarations::declared_url_schemes`.
- `flui build` and `flui run` refuse a Linux or Windows build of an application that declares URL schemes or file types without flui's `single-instance` feature, naming what it opens and the feature to turn on; `flui doctor` reports the same as `project.single-instance`.
