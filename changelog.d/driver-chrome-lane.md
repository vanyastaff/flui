### Added

- `flui-driver`'s Chrome lane (feature `chrome`): `Driver::launch` of an `Artifact::WasmBundle` serves the web build on the loopback, starts ChromeDriver (`CHROMEDRIVER`, a cached one, or the one for the installed Chrome fetched from Chrome for Testing), opens Chrome headless at `Launch::viewport` and reaches the page's agent through its export; `Driver::attach_page` attaches to a BiDi session already open. OS input goes through WebDriver BiDi `input.performActions`, one command of one instant action an event, and an input method's composition through Chrome's DevTools input domain; `Window::capture_os` captures the canvas.
- `testing::ScriptedBrowser`: a stand-in BiDi session playing a page whose agent a script answers.

### Changed

- An input-method composition in an OS-path sequence becomes `OsEvent::Composition` steps, which only a lane whose `LaneCapabilities::compositions` is set takes; `lane::send_guarded` refuses it for every other lane, where `steps::os_events` refused it before.
- `lane::send_guarded` answers `Error::ReleaseFailed` when the release after a failed send leaves input held, the send's failure kept in its message.
