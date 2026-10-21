### Added

- **`flui-platform`**: the web backend decodes a trackpad pinch into `PanZoom`, from the
  Control-held wheel events Chromium and Firefox send for it (reported `Emulated`, classified by a
  sequence's first event, ending at a deadline on the event timeline) and from WebKit's
  `gesture*` events; an application that does not prevent a pinch's first event leaves it to the
  browser's page zoom. A pen reporting only `tiltX`/`tiltY` keeps its tilt.
- **`flui-platform`**: HTML drag and drop on the web canvas (ADR-0211 §11): drags from the page
  or the OS are offered by their types and files and copied out at the drop, `dragover` answers
  from the target's feedback, and a session the runtime starts on a press before the browser's
  drag threshold becomes an HTML drag with its representations stored at `dragstart`; outgoing
  drags are reported `Native`.
- **`flui-platform`**: text input on the web (ADR-0090): the web window is a `TextStoreHost`
  serving the focused field's store through an `EditContext` on the canvas, or a hidden textarea
  where the browser has none; a key the input method processes (composing, key code 229, or while
  the field composes) reaches the window marked `ImeComposition::Active`, so Escape cancels a
  composition before any shortcut sees it.
- **`flui-testing`**: the conformance table's IME group has its first row,
  `a_key_during_composition_is_the_input_methods`, with `Step::ComposingKey` and `StepKey::Escape`
  for a backend to drive it.
- **`cargo xtask device web-input`**: runs the web backend's browser tests in headless Chrome,
  fetching the ChromeDriver that matches the installed Chrome, then drives the `web_input_probe`
  page over WebDriver BiDi with trusted mouse, pen and touch input recorded into the golden
  native recording `web_bidi_mouse_pen_touch` (`--bless` rewrites it), the Vibration API before
  and after user activation, HTML drags in both directions, and Chrome's input-method path into
  a focused field through both text surfaces.

### Fixed

- **`flui-platform-api`**: `ProcessNonce::mint` no longer mints the same nonce on every browser
  page: wasm32's std seeds its hasher with fixed keys, so two pages minting at the same point of
  their start shared one and would take each other's drags for their own; the page's random
  generator now seeds it too.

### Changed

- **`flui-platform`**: a web wheel event's default (page scrolling) is cancelled, and its
  propagation stopped, only as the input callback's result says, instead of always.
