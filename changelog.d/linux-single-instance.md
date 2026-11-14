### Added

- `Application::single_instance(bool)`: on Linux a later launch of the application hands the running instance its links and files and ends, and the running window comes forward with the launcher's activation token; on by itself when the application declares a URL scheme.
- `CurrentHost::start_single` and `HostStart` in `flui-native`, which keep one instance per user session on the session bus (`org.freedesktop.Application`).
- `LinkDeclarations::declared_url_schemes`.
