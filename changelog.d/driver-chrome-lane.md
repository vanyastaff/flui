### Added

- `flui-driver`'s Chrome lane (feature `chrome`): `Driver::launch` of an `Artifact::WasmBundle` serves the web build on the loopback (for its own `Host` alone), starts ChromeDriver, opens Chrome headless at `Launch::viewport` and reaches the page's agent through its export; `Driver::attach_page` attaches to a BiDi session already open. OS input goes through WebDriver BiDi `input.performActions`, one command of one instant action an event, and an input method's composition through Chrome's DevTools input domain; `Window::capture(CaptureSource::Os)` captures the canvas, decoded by the driver's one PNG decoder.
- `Launch::chromedriver` and `ChromeDriverSource`: by default a launch takes the `CHROMEDRIVER` file, the directory `CHROMEWEBDRIVER` names, or a cached ChromeDriver whose SHA-256 matches its record, and reaches no network; `ChromeDriverSource::FetchAllowed` fetches the installed Chrome's ChromeDriver (or, when that patch is not published, the newest of its build) over HTTPS from Chrome for Testing and records its SHA-256.
- `testing::ScriptedBrowser`, a stand-in BiDi session playing a page whose agent a script answers, and `testing::chrome`, the lane's host side for its tests.

### Changed

- An input-method composition in an OS-path sequence becomes `OsEvent::Composition` steps, which only a lane whose `LaneCapabilities::compositions` is set takes; it is refused before the desktop is locked, at `lane::send_guarded`, and by every other lane's `send`, where `steps::os_events` refused it before.
- `Error::ReleaseFailed` carries the failure that stopped the input before the release (`first`), and `Error::effect`, `Error::may_have_run`, `Error::is_cannot_verify` and `Error::missing_capability` answer from it: `lane::send_guarded` keeps an interrupted run cannot-verify and an unanswered act `may_have_run` when the release after it fails.
