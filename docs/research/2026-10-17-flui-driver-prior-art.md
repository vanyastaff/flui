# flui-driver: prior-art checklist (driving and checking UI from tests and agents)

- **Date:** 2026-10-17
- **For:** [the flui-driver spec](../plans/specs/flui-driver/design.md), its "Prior art read" line
- **Method:** primary sources (official docs, W3C and MCP specs, source read with `gh api`), read
  as a checklist rather than a target. The market and design-tool survey is a separate document:
  [2026-10-18 market survey](2026-10-18-flui-driver-market-survey.md). Sections near the end say
  what could not be verified.

## Playwright

- **Locator priority:** `getByRole` first, then label, placeholder, text, alt text and title, then
  `getByTestId`. CSS and XPath are a last resort because they break when the DOM changes.
  https://playwright.dev/docs/locators
- **Strict by default.** An action on a locator that matches more than one element throws.
  `count()` and `all()` are explicitly multi-element. `first`, `last` and `nth` opt out of
  strictness and are discouraged. Locators chain, and filter with `hasText`, `hasNotText`, `has`,
  `hasNot` and `visible`; `and()` and `or()` combine them. Same source.
- **Actionability per action:**
  - visible: non-empty box, not `visibility:hidden`;
  - stable: the same box over 2 animation frames;
  - enabled;
  - editable;
  - receives events: the element is the hit target at the action point.

  Click needs visible, stable, receives events and enabled. `force` skips the non-essential checks.
  https://playwright.dev/docs/actionability
- **Web-first assertions:**
  - they retry until they pass or time out, 5 s by default;
  - plain value assertions check once;
  - `expect.poll` backs off at 100, 250, 500 and 1000 ms;
  - `expect.soft` records the failure and continues.

  https://playwright.dev/docs/test-assertions
- **ARIA snapshots:** YAML lines of `role "name" [attr=value]`, where a name is exact or a
  `/regex/`. Children match as `contain` (the default), `equal` or `deep-equal`.
  `--update-snapshots` patches the test source. https://playwright.dev/docs/aria-snapshots
- **`toHaveScreenshot`:**
  - it captures until two consecutive frames match;
  - `threshold` is 0.2, the YIQ per-pixel distance;
  - `maxDiffPixels` and `maxDiffPixelRatio`;
  - masks paint locators `#FF00FF`;
  - `animations: "disabled"` and `caret: "hide"`;
  - baseline file names carry the browser and platform.

  https://playwright.dev/docs/api/class-pageassertions and https://playwright.dev/docs/test-snapshots
- **Trace:** `trace.zip` holds, per action, the locator, source line, wait log, before, action and
  after DOM snapshots, a screencast, network and console. Modes are `on-first-retry`,
  `retain-on-failure` and `on`. The viewer is client-side. https://playwright.dev/docs/trace-viewer.
  Codegen prefers role, text and test-id locators and refines a locator until it is unique.
  https://playwright.dev/docs/codegen
- **Wire:**
  - requests are JSON `{id, guid, method, params}`;
  - responses are `{id, result | error, log}`;
  - the server pushes `__create__`, `__adopt__` and `__dispose__` for remote objects;
  - the schema is generated from YAML.

  https://github.com/microsoft/playwright/blob/main/packages/playwright-core/src/client/connection.ts

## W3C WebDriver BiDi (Editor's Draft, 2026-10-08)

Source for this section: https://w3c.github.io/webdriver-bidi/

- **Messages:**
  - command: `{id, method, params}`, in the form `module.method`;
  - success: `{type:"success", id, result}`;
  - error: `{type:"error", id, error, message, stacktrace?}`, with string codes;
  - event: `{type:"event", method, params}`.
- `session.subscribe` takes events or whole modules, optionally scoped to contexts.
- `browsingContext.locateNodes` takes these locators: `css`, `xpath`, `innerText`,
  `accessibility {role, name}` and `context`, scoped through `startNodes` and capped by
  `maxNodeCount`.
- `input.performActions` takes `none`, `key`, `pointer` (mouse, pen, touch) and `wheel` sources.
  Its actions are `pause`, key down and up, pointer down, up and move, and `scroll`, grouped into
  ticks. `input.releaseActions` undoes held state.
- `script.evaluate` and `script.callFunction` run in a realm or a sandbox and return `sharedId`
  references.

## Flutter flutter_driver and integration_test

- **flutter_driver finders** are serializable descriptions: `byValueKey`, `byType`, `byTooltip`,
  `bySemanticsLabel`, `text`, `ancestor` and `descendant`.
  https://api.flutter.dev/flutter/flutter_driver/CommonFinders-class.html
- **Waits are framework-aware:** `waitFor`, `waitForAbsent`, `waitForTappable`,
  `waitUntilNoTransientCallbacks`, `waitUntilFirstFrameRasterized` and `waitForCondition`.
  Synchronization is on by default (`runUnsynchronized` turns it off).
  https://api.flutter.dev/flutter/flutter_driver/FlutterDriver-class.html
- **Superseded in December 2020.** Host-process tests could not reach app state or run on device
  farms, and the finder API was too complex. `integration_test` bundles the tests into the app
  instead. https://docs.flutter.dev/release/breaking-changes/flutter-driver-migration
- **Goldens:** `matchesGoldenFile` captures the nearest `RepaintBoundary` of a single match. The
  default comparator is exact, and a tolerance means subclassing it. Tests use the Ahem font to
  sidestep font drift.
  https://github.com/flutter/flutter/blob/master/packages/flutter_test/lib/src/goldens.dart

## Appium, UiAutomator2, XCUITest

- **UiAutomator2** runs an on-device instrumentation server. Its locators are `id` (resource-id),
  `accessibility id` (content-desc), `className`, `-android uiautomator` and `xpath` over the page
  source. It waits for idle (10 s) before each lookup, and gestures are first-class commands.
  https://github.com/appium/appium-uiautomator2-driver
- **XCUITest locators:** `accessibility id` and `name` are aliases. `-ios predicate string` is the
  fastest and `xpath` is up to 10× slower.
  https://github.com/appium/appium-xcuitest-driver/blob/master/docs/reference/locator-strategies.md
- **Apple XCUIAutomation:** `XCUIElementQuery.element` fails unless exactly one element matches.
  `isHittable` is false when the element is offscreen or covered. The waits are
  `waitForExistence` and `waitForNonExistence`.
  https://developer.apple.com/documentation/xcuiautomation/xcuielementquery/element

## AccessKit and kittest

- **AccessKit `TreeUpdate`** is push-only:
  - a node replaces the old one whole;
  - the parent is sent again when its children change;
  - focus is sent with every update;
  - subtrees are grafted by `TreeId`;
  - an `ActionRequest` targets a node.

  Adapters exist for UIA, macOS, iOS, Android and AT-SPI.
  https://github.com/AccessKit/accesskit/blob/main/accesskit/src/lib.rs
- **kittest** takes Testing Library-style queries over AccessKit: `get_*` fails on zero or several
  matches, `query_*` returns an `Option`. The `By` filter is a builder.
  `Harness::run` steps until no repaint is requested and reports the causes when it overruns.
  egui_kittest snapshots set `threshold` 0.6 and `max_failed_pixels` 0 with per-OS overrides. It
  writes `.new.png` and `.diff.png`, and `UPDATE_SNAPSHOTS` rewrites only the failing ones.
  https://github.com/rerun-io/kittest and https://github.com/emilk/egui/tree/main/crates/egui_kittest

## MCP servers and the MCP spec

- **Playwright MCP:**
  - defaults to an accessibility snapshot with refs (`button "Submit" [ref=e2]`);
  - a stale ref fails with "take a new snapshot";
  - each action returns the updated snapshot;
  - coordinate tools appear only with `--caps=vision`.

  https://github.com/microsoft/playwright-mcp
- **Windows-MCP:** UIA snapshots with element ids, a `WaitFor` tool, a takeover gate it calls best
  effort, and screenshot scaling. https://github.com/CursorTouch/Windows-MCP
- **Slint's in-app MCP server** (Streamable HTTP, `SLINT_MCP_PORT`) exposes trees, query pipelines,
  element actions and event recording, with `{index, generation}` handles.
  https://github.com/slint-ui/slint/blob/master/internal/backends/testing/mcp_server.rs
- **MCP 2026-07-28:**
  - JSON-RPC 2.0;
  - stateless requests that carry their version in `_meta`;
  - tools with `inputSchema` and `outputSchema`, and `annotations` that clients must treat as
    untrusted;
  - tool failures as `isError: true`, kept apart from protocol errors;
  - explicit expiring handles and a deterministic tool order.

  https://modelcontextprotocol.io/specification/latest/server/tools

## Rust UI test harnesses

- **Masonry `TestHarness`:** synthetic pointer, text and access events, where actions arrive as
  AccessKit `ActionRequest`s. It has a mocked clock and `assert_render_snapshot!` with a size budget.
  https://github.com/linebender/xilem/blob/main/masonry_testing/src/harness.rs
- **GPUI:** a deterministic `TestDispatcher`, `run_until_parked` and seeded scheduling iterations.
  It fakes prompts, clipboard and displays, and `debug_bounds` works in debug builds only.
  https://github.com/zed-industries/zed/blob/main/crates/gpui/src/app/test_context.rs
- **iced_test:** `simulator(view)` selects by text and returns the produced messages. Snapshots
  match an image or a hash, and `.ice` files are recordable scripts.
  https://github.com/iced-rs/iced/tree/master/test/src
- **Slint testing backend:** mock time, and `ElementHandle` queries by id, type, role or label.
  `SLINT_TEST_SERVER` makes the app connect out over protobuf.
  https://github.com/slint-ui/slint/blob/master/internal/backends/testing/README.md

## Cross-cutting lessons the spec adopted

1. **One semantic tree as the query model**, with a test id as the escape hatch and coordinates as
   a last resort.
2. **Strict locators:** ambiguity is an error that lists the candidates.
3. **Locators are lazy descriptions**, re-resolved at each use, never cached nodes.
4. **Actionability before input,** and the report names the check that failed.
5. **Settle on the framework's own signals,** and report why the app has not settled.
6. **A deterministic clock in-process;** real time only on device or OS backends.
7. **Retrying assertions with timeouts,** kept apart from one-shot reads.
8. **Every action reports its effect;** a stale ref fails with a hint, and handles are
   generational.
9. **Agents get compact ref-annotated snapshots by default,** with vision tools behind a flag.
10. **A versioned, typed protocol,** with tool failures kept apart from protocol failures.
11. **Input as typed per-device action sequences,** with gestures built on top.
12. **Screenshot discipline:** stable capture, masks, YIQ threshold plus a pixel budget,
    per-platform baselines, `.new` and `.diff` artifacts, and gated updates.
13. **Traces are first-class artifacts** that keep the locators.
14. **In-process first, out-of-process second:** one introspection core with several front ends
    (Slint, Flutter's reason for moving to `integration_test`).

## Could not verify

- The tick semantics are defined in Classic WebDriver §17, which was not read.
- Flutter's default comparator tolerances beyond `LocalFileComparator`.
- Released-crate status of AccessKit's subtree fields, and of egui_kittest's accessibility check and
  recording (both read on `main`).
- How Playwright MCP generates its refs (inferred from tests and `tab.ts`).
- No first-party Microsoft desktop MCP server was found.
