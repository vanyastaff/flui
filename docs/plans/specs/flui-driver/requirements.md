# flui-driver: one tool to drive and check FLUI apps (requirements, level 1)

- **Status:** approved by the owner (2026-10-20)
- **Date:** 2026-10-17
- **Base:** `main` @ `0fdd19f1e`; the implementation also absorbs what `interaction/completeness`
  (`4579e0bcf`) adds under `tools/xtask/src/device/`, `crates/flui-platform/tests/native_recording/`
  and `crates/flui-testing/src/recording/`, so it starts once that branch has merged (tasks.md)
- **Level 0:** [../release/requirements.md](../release/requirements.md). This spec takes up the
  "`flui mcp` and agent workflow" item that level 0 listed out of scope, and roadmap track B3
  ("an agent creates a screen, runs a scenario and asserts the result through `flui mcp`")
- **Design:** [design.md](design.md). **Tasks:** [tasks.md](tasks.md)
- **ADRs:** [ADR-0238](../../../adr/ADR-0238-flui-driver-protocol.md) (protocol and crate),
  [ADR-0239](../../../adr/ADR-0239-every-debug-app-hosts-the-driver-agent.md) (agent in the app),
  [ADR-0240](../../../adr/ADR-0240-semantics-identifier.md) (test identifier),
  [ADR-0241](../../../adr/ADR-0241-driver-trace-format.md) (trace format),
  [ADR-0242](../../../adr/ADR-0242-typed-inspect-and-tracing-conventions.md) (`Inspect` and
  tracing conventions)

This spec sets the observable behaviour of one tool that drives a running FLUI app and checks
what it does, for three kinds of user: Rust tests, people and AI agents. It covers every FLUI
platform. How it is built (crates, wire types, lanes) is the design's business.

## Who uses it

**A test author** (depends on `flui-driver` as a dev-dependency):

1. writes one Rust test that finds a button by its role and name, presses it and waits until the
   counter reads 1. The same test runs headless in CI on a virtual clock, and against the real app
   on Windows, macOS, Linux, an Android device, the iOS Simulator and Chrome, with no change but
   the target;
2. chooses how input arrives: a semantics action, framework-level synthetic input or real OS
   input. Then reads from the result which path the input actually took and whether the app
   observed it;
3. asserts on pixels: a region against a baseline recorded for that backend, or a baseline-free
   predicate ("this region changed", "this pixel is red"). A failure leaves expected, actual and
   diff images;
4. gets a failure that says what was expected, what the app showed, which check failed and the
   trace to replay it, never a bare timeout.

**A developer** (has the `flui` CLI):

1. runs `flui drive launch` for an example on any platform, then `flui drive tree`, `find`,
   `tap`, `type`, `shot` and `wait` from the shell, readable by a person and as `--json`;
2. opens the inspector app, sees the live semantics, widget and render trees, picks an element
   on a screenshot and sees its node, and scrubs a recorded trace frame by frame;
3. records a session, replays it against a new build, and turns it into a Rust test.

**An AI agent** (has an MCP client):

1. runs `flui mcp`, launches the app, reads a compact semantics snapshot with refs, acts on a ref,
   and reads back the effect and the new snapshot;
2. gets typed refusals it can act on: stale ref, ambiguous locator, element not actionable and
   why, platform cannot do it, may have run.

**FLUI's own CI and device checks** stop being per-platform scripts. Every `cargo xtask device …`
check is a `flui-driver` scenario with one report format and one exit-code rule.

## Current state

- **Three stacks.** `tools/desktop-mcp` drives any Windows app from outside: UIA, xcap, enigo,
  24 MCP tools, ADR-0080's errors. macOS is partial and Linux unsupported. The devtools agent
  (`packages/flui-devtools/src/agent`, ADR-0095 §3) serves one app's semantics from inside it
  (`windows`, `read`, `act`, `motion` over NDJSON on a named pipe or Unix socket). The xtask device
  drivers are a third stack: `plan.rs`, `uia.rs`, `windows_input.rs`, and on the integration branch
  `android_input.rs`, `bidi.rs` and `web_input.rs`. On top of those come Python and Swift under
  `tools/device-checks/` and `tools/live-smoke` for X11.
- **The drivers disagree.** Verdicts are `A11Y=PASS`, `INPUT=CANNOT_VERIFY`, `NAME=PASS: …`,
  free text on Android and `*_PROBE_RESULT=PASS`. Cannot-verify becomes exit 1 in probe steps and
  exit 2 elsewhere. Results come back as stdout markers, `status.txt` polled over `adb`, a
  JavaScript probe API, an iOS container file and error strings. Each probe has its own command
  channel: F-keys on Android, `window.fluiProbe` on the web, env vars in an `.xctestrun` on iOS.
- **The app cannot be asked much.** The agent has no widget or render tree, no frames, no
  synthetic input, no screenshot, no events, no locator and no wait. Android and web hosts drive
  no agent hook at all. An app serves the agent only if its code installs `AgentServer`, so no
  example is drivable as built.
- **No stable test id.** FLUI semantics has no identifier. `automation_id` exists only when UIA
  reports one, and FLUI never sets it.
- **No live pixels.** `flui-engine` reads back only offscreen (`HeadlessRenderer`). A presented
  window can be captured only through the OS, and only on Windows and macOS.
- **No Android, iOS or web accessibility tree.** `flui-platform` has AccessKit adapters for
  Windows, macOS and Unix only, so on those three platforms an OS-side reader sees nothing of
  FLUI's semantics.

## Requirements

### Targets and connection

- **R1.** One API drives a *target*: a running FLUI app. The target kinds are headless
  (in-process), Windows, macOS, Linux X11, Linux Wayland, Android device or emulator over `adb`,
  iOS Simulator and Chrome. Code written against one target compiles and runs against any other.
- **R2.** The driver launches an example or binary on any target (build, install, start) and can
  also attach to one already running. A launched target is ended when its session ends, including
  when the driver panics or is killed.
- **R3.** A debug build of any FLUI app is drivable without code changes. An optimized build is
  drivable only when built with the explicit `driver` feature. Without it, no endpoint exists and
  nothing in the app reacts to the driver's environment.
- **R4.** The connection is local and authenticated: a token handed over at launch, at least 32
  bytes, compared in constant time. It never uses a TCP port on a network interface. Android uses
  an `adb`-forwarded local socket; Chrome uses the WebDriver BiDi session the driver owns.
- **R5.** Every target reports its *capabilities*: which input paths, trees, capture sources and
  recording layers it supports. An unsupported operation fails as `not_supported` and names the
  missing capability. It never silently falls back.

### Finding elements

- **R6.** Elements are located by semantics, never by pixel search. The criteria are role, name,
  value and description (exact, contains or prefix; case-sensitive by default), identifier, state
  (checked, disabled, focused, expanded, selected), scoping (within another locator, containing
  another locator) and window. Coordinates are a separate, explicit last resort.
- **R7.** A locator is a description, re-resolved at every use, never a cached node. An action
  through a locator that matches more than one element fails as `ambiguous` and lists the
  candidates. Choosing one is explicit (`first`, `last`, `nth`). `not_found` lists the nearest
  candidates by role, name distance and identifier. The driver never substitutes one: the
  documented failure of "self-healing" locators is a silent wrong match.
- **R8.** FLUI semantics carry an optional **identifier**: stable, never announced to users. A
  widget author sets it. It is exported to UIA `AutomationId`, macOS `AXIdentifier`, AT-SPI's
  accessible id, Android `viewIdResourceName` and a DOM attribute when those adapters exist, so
  one selector means the same element in-app and through each OS lane.
- **R9.** A resolved element has a handle (`e12`) that is generation-checked. A handle whose node
  is gone answers `gone` and is never re-bound to another node, including when the slot is reused.

### Acting

- **R10.** Every action names its input path: `semantic` (a semantics action), `synthetic`
  (platform input injected into the app's own input path) or `os` (real OS input through the
  target's lane). A session sets a default. Each result names the path actually used, and
  `require` refuses any other path. The default is `synthetic` on every target. `os` is an
  explicit opt-in (a session option, `--via os`, or a scenario marked as a certification run), so
  a developer's routine run never takes their mouse or keyboard.
- **R11.** Before pointer input to a located element, the driver checks *actionability*: attached,
  visible in the window, stable (same bounds on two consecutive frames), enabled, and receiving
  input (the app's hit-test at the action point reaches that element). Failing within the timeout
  is `not_actionable`, naming the failed check and what covered the point. `force` skips every
  check except attached.
- **R12.** Input covers mouse, touch, pen, trackpad gestures, wheel, keyboard (keys, chords, text)
  and IME composition. It is expressed as typed per-device action sequences with pauses (the shape
  of BiDi `input.performActions`). Common gestures are built in: tap, double tap, long press, drag,
  fling, swipe, pinch and scroll into view. A target supports these per device as its
  capabilities say.
- **R13.** Each action reports its *effect* in ADR-0080's terms (`ran`, `may_have_run`,
  `partial`, `incidental`). For `os` input, `ran` means the app's input journal observed the
  injected event, not merely that the OS accepted it. A timed-out action is never retried
  automatically.
- **R14.** OS input keeps desktop-mcp's safety rules: it requires a target window, re-checks
  foreground and the window under the point before each event, refuses when physical input is
  held, refuses shell hotkeys, and releases everything it pressed, including on failure.
- **R14a.** An OS-input run on the user's own desktop is announced before it takes input:
  what it will take (mouse, keyboard, touch) and for about how long. Non-interactive runs print
  the same line to the report. Any user activity during the run aborts it at once and releases
  everything, not only held input: a physical mouse move, key, touch or pen event that the lane did
  not inject. The run's verdict is then *cannot verify*, with "interrupted by user input" as its
  reason. A lane that cannot tell user input from its own refuses OS input on a shared desktop and
  requires an isolated one (R14b). The guard is best effort, not a security boundary: OS hooks miss
  input that does not pass through them (remote-desktop clients, switch devices), and an event
  already sent cannot be recalled. The announcement says so, and isolation is the real protection.
- **R14b.** Each platform has an **isolation mode** where OS input cannot reach the user's
  session, and the report names the mode a run used:
  - Windows: one of three candidates: Windows Sandbox, a second interactive user session, or a
    loopback RDP session (the separate-desktop approach UFO² documents). Each must pass a spike
    before the spec commits to it: injected input stays out of the host session, and GPU
    presentation works;
  - Linux: Xvfb or a separate seat (a nested or headless compositor for Wayland);
  - macOS: a second user session (fast user switching) or a virtual machine;
  - Android: an emulator when hardware fidelity is not the point, a phone when it is;
  - iOS: the Simulator, which is already isolated from the host's input;
  - Chrome: headless, which is isolated by construction.

  Certification runs that need the real desktop say so in their scenario and are always announced.

### Reading

- **R15.** The driver reads the semantics tree (ADR-0080 nodes plus `identifier`), the widget
  (element) tree, the render tree and the layer tree of any window. Every read has depth and node
  budgets and reports `truncated`. A truncated read says how to narrow it: which subtree was cut,
  and the root, locator or depth to request next. The trees link to each other: semantics node to
  render object to element.
- **R16.** For agents the default reading is the one-line-per-node outline with refs, compact
  enough for a model's context. After an action the default reading is a **snapshot delta**
  against the tree the action started from: nodes that appeared, nodes that are gone, and nodes
  whose name, value or state changed, each as an outline line with its ref. The full outline is
  available on request (`snapshot: full`), and so is none (`snapshot: none`, the Rust API's
  default). Outline reads take a token budget (`max_tokens`, default 8000), and they can carry each
  line's rect (`boxes: true`) for agents that reason about layout. None of Playwright MCP, Chrome
  DevTools MCP or Appium MCP documents a delta, which makes this a differentiator.
- **R17.** On Windows, macOS and Linux the driver also reads the OS accessibility tree and checks
  it against the in-app semantics through the normalized projection (ADR-0095 §4). This shows that
  assistive technology sees what the app publishes.

### Waiting

- **R18.** Waits are on the app's frame and event timeline, not on sleeps or polling. Conditions:
  a locator appears, disappears or changes state, value or name; the app settles; N frames have
  presented; an input or semantics event is observed; and combinations of these. Every wait has a
  timeout and returns either the observation that satisfied it or, on timeout, the last
  observation and why the app had not settled (pending frame, transient callbacks, timers).
- **R19.** On the headless target, time is virtual. A wait advances the manual clock frame by
  frame up to a frame limit and is deterministic across runs. On live targets time is real.
- **R20.** Assertions retry until they pass or time out (default 5 s, overridable per session and
  per call). One-shot reads stay available. A failed assertion reports expected against last
  actual.

### Pixels

- **R21.** The driver captures a window or an element's region from three sources, each named in
  the result: `presented` (the frame the app last presented, read back from its swapchain or
  surface), `os` (what the OS compositor shows) and `rendered` (the last committed layer tree
  re-rendered offscreen; also what headless offers). Each capture records size, scale factor, frame
  id and source.
- **R22.** Baseline comparison captures until two consecutive frames match. It masks regions by
  locator, compares with a per-pixel YIQ threshold plus a pixel-count or ratio budget, and keys
  baselines by backend and OS. On failure it writes `.new` and `.diff` images. The update modes are
  `none` (the default), `missing` (create absent baselines only), `changed` (rewrite failing ones)
  and `all`, the same set Playwright uses. None of them runs implicitly in CI. A missing baseline
  is a failure, never a pass, and a run that only created baselines still fails. Every comparison
  records the renderer (adapter and backend), the font set and the scale factor, because
  environment drift is the main flake source Argos and Playwright document.
- **R23.** Baseline-free predicates: colour at a point within tolerance, region differs from or
  equals an earlier capture, region is uniform, region is stable for N frames.

### Recording and replay

- **R24.** A session can record a *trace*: a versioned JSON Lines file of every driver command
  (with locators, not raw coordinates), its result and effect, waits and their observations, and
  evidence (tree snapshots, captures). It bundles the app-level input recording (ADR-0215) and,
  where the target supports capture, the native recording. Modes: off, on, retain on failure.
- **R25.** The CLI replays a trace against a target and reports where the results diverge. It also
  generates a Rust test from a trace. The inspector opens a trace and scrubs it.
- **R26.** Native recordings captured through the driver are the golden files of
  `crates/flui-platform/tests/native_recording/`. Blessing them is an explicit driver option that
  replaces `FLUI_NATIVE_RECORDING_BLESS`, and the existing hold tests read them unchanged.
- **R27.** Text in recordings and traces is redacted by default (ADR-0215 §5). Keeping it is an
  explicit `keep_text` choice. Captures cover the target's own windows only, never the whole
  screen.

### Surfaces

- **R28.** One protocol, three front ends: the Rust API, the `flui drive` CLI and the `flui mcp`
  server. All three expose the same operations, with the same names, parameters, results and
  errors, generated from one method table. None can do what the others cannot.
- **R28a.** MCP lists a **core tool set** by default, 13 tools enough to launch, read,
  locate, act, type, wait, capture and audit a screen. The other methods are grouped into capability sets the client
  enables with `--caps <group>`: `session` (attach, capabilities, window placement, frame and settle waits, the animation clock), `input`, `inspect`, `pixels`, `record`, `perf`, `audit`,
  `explore`, `vision`, `layout`. Every method stays reachable from MCP; tiering only changes what is listed by
  default, so a model does not choose among 50 tools. `--tools` and `--exclude-tools` filter by
  name. A test measures the token size of the listed schemas, since context cost is the point of
  tiering. Reads take `format: concise | detailed`.
- **R29.** The MCP server follows MCP revision 2026-07-28. Each tool has an input and output
  schema and honest `readOnlyHint`/`destructiveHint`, and tool failures come back as `isError`
  results with ADR-0080's error object. Handles are explicit and expiring, never session state
  inside the MCP connection. Tools are listed in a deterministic order. The adapter implements
  the revision's per-request versioning and discovery, checked against the revision's spec text
  when the adapter is built, and refuses an older revision with its version.
- **R30.** The CLI works across invocations without a daemon. `launch` writes a session file that
  later commands read; handles stay valid across commands because the app owns them.
- **R31.** The inspector is a FLUI desktop app built on `flui-driver`. It shows the live trees, a
  screenshot with element picking and highlight, element properties, the frame timeline with input
  and gesture-arena decisions, and opens traces. Because the inspector is itself a FLUI app, a
  FLUI defect can break it. The CLI is the fallback that needs no FLUI rendering: `flui drive`
  reads every tree as an outline or `--json`, captures to files, and prints traces. The docs say
  so, and the inspector failing never blocks a driver run.

### Platform lanes

- **R32.** Real OS input and OS reading per platform. Each is written in Rust with no Python,
  Swift or shell scripts:
  - Windows: SendInput, synthetic pointer injection for touch and pen, UIA, Windows Graphics
    Capture;
  - macOS: CGEvent posted to the target, AX, ScreenCaptureKit;
  - Linux X11: XTEST, AT-SPI, X11 capture;
  - Linux Wayland: libei through the RemoteDesktop portal, AT-SPI, ScreenCast portal capture;
  - Android: `adb` input; `uinput` scripts where the device allows them without root, which is
    probed per device and reported as a capability, never assumed; `uiautomator` for system UI
    such as the soft keyboard; `screencap`;
  - iOS Simulator: the simulator's HID channel for touch, keys and buttons, `simctl` capture;
  - Chrome: WebDriver BiDi `input.performActions`, CDP IME composition, `captureScreenshot`.
- **R32a.** OS-lane runs are **deterministic by rule**:
  - a scenario fixes its window size (logical size and scale factor), and the run refuses when the
    OS gives another;
  - OS-lane pointer moves are single steps separated by explicit pauses, never one interpolated
    multi-step move; Chrome interpolates such moves, so recordings varied between runs;
  - each lane documents its **replay fidelity**: whether positions are exact or only the event
    order and kinds are reproducible. Android resamples moves, so its goldens compare event order
    only. Replay and golden holds compare positions only where the lane is exact.
- **R33.** A lane the host cannot run (a locked desktop, no device, no Chrome, an unsupported
  simulator runtime, a portal the user refused) is *cannot verify*, never pass and never a silent
  skip.

### Reports and device checks

- **R34.** Every scenario ends with one report: verdict `pass`, `fail` or `cannot_verify`, each
  check with its evidence, and the trace path. Exit codes are 0, 1 and 2, the same everywhere.
- **R35.** Every existing device check (`windows-a11y`, `windows-input`, `windows-notes`,
  `web-input`, `android-input`, every `macos-*`, `ios-*` and `live-smoke`) is reimplemented as a
  driver scenario that checks at least what it checks today. The per-platform drivers, the Python
  and Swift scripts, the bespoke probes' command channels and `tools/desktop-mcp` are deleted.
- **R36.** An `interaction_gallery` scenario suite exercises the interaction widgets through each
  input path on each platform. It is the dogfooding consumer of every lane. The first scenario
  ported is `cargo xtask device windows-gallery`, which the integration branch is building now.
- **R36a.** Every interactive widget in `examples/interaction_gallery` carries an ADR-0240
  identifier, and the gallery suite selects by identifier. A test fails when an interactive gallery
  node has no identifier.

### Performance

- **R37.** The driver measures performance from the app's frame events and input journal over a
  marked span (`perf.start` … `perf.stop`):
  - frame times: p50, p90, p95, p99 and the worst frame;
  - **dropped** frames, a missed present deadline, kept separate from **janky** frames, whose work
    exceeded a multiple of the refresh period (default 2×, JankStats' default, configurable);
  - input-to-present latency: from an input's journal timestamp to the presentation of the first
    frame that consumed it, split into input delay, processing and presentation delay (INP's three
    phases);
  - spans tagged with states (`perf.tag("scroll")`), so the worst frame names the interaction it
    belonged to.
- **R37a.** Budgets are assertions, for example "a scroll gesture drops no frames" or "p95
  input-to-present ≤ 2 refresh periods". A failed budget reports the measured distribution and
  the worst frames with their phases. Budgets are keyed per target, so a budget for the Galaxy S21
  does not apply to headless. Headless reports frame counts and phase work, not wall-clock latency.
  Real-device budgets run a stated number of iterations (default 5) and judge the median of each
  iteration's percentile, because single runs are noisy (Macrobenchmark's practice).

### Accessibility audit

- **R38.** `audit.a11y` is a driver method and a standard scenario. It checks:
  - every interactive node has a name; siblings do not share a name or identical bounds unless the
    app waives it (ATF's duplicate-text and duplicate-clickable-bounds checks);
  - focus order visits every focusable node once, in reading order (direction-aware), with no
    cycles and no unreachable node;
  - interactive targets meet the target platform's minimum: 48×48 dp on Android, 44×44 pt on iOS,
    and on desktop and web the WCAG 2.2 2.5.8 minimum of 24×24 CSS px, with the larger 44 px
    recommendation available as a stricter profile. The ATF exemptions apply: a node at a
    scrollable edge, a target inside a larger clickable ancestor, a node clipped by its ancestor,
    and inline links. A finding names the exemption that was considered;
  - every gesture-only interaction has an accessible alternative (a semantics action or a
    keyboard path).

  The scenario performs each alternative and checks it has the same effect as the gesture. It
  reuses `flui-testing`'s `assert_gesture_alternatives` and minimum-target checks, which move
  under the driver so both share one implementation. Findings are typed, with the node's ref and
  the rule.

### Exploration

- **R39.** `explore.run` performs seeded random actions over the actionable elements of a window:
  taps, drags, scrolls and text on the elements it finds, through a chosen input path, for N steps
  or a time budget. After each step it checks these invariants:
  - no panic or contained frame failure;
  - no visible enabled element is `not_actionable` unless covered by another app element;
  - semantics stay consistent (unique refs, parents and children agree, bounds finite);
  - no injected input stays held after its step.

  A failure stops the run and keeps the trace. The seed replays it exactly, and the run shrinks the
  failing action list to a minimal trace that still fails. Shrinking works on the locator-level
  action list, never on raw coordinates. The run takes Monkey-style knobs: action-mix weights,
  a throttle between steps, and a continue-after-failure mode that collects distinct failures. The
  spec promises reproducible faults, not coverage, since the literature's coverage rankings
  disagree.

### Design checks

Agents that build screens in design tools (Figma, pen.dev and the like) then judge the result
from screenshots. They miss violations a designer sees at once: a label off centre in its button,
two edges that almost line up, uneven gaps, clipped text, poor contrast. Then they report that
everything is fine. FLUI knows its own geometry, so the driver checks these facts exactly instead
of guessing from pixels.

- **R40.** **Layout assertions** over located elements are exact and come from the render tree
  and text layout, not from pixels:
  - `to_be_centered_in(container, axis, tolerance)`: the text's ink bounds when the element is
    text, its box otherwise;
  - `to_be_aligned_with(others, edge | baseline | centre)`;
  - `to_have_gap(next, value)`;
  - `to_be_within(container)`;
  - `to_not_overlap(other)`;
  - `to_not_be_clipped` and `to_not_be_truncated`, from the paragraph's overflow and line limit;
  - `to_have_contrast_at_least(ratio)`, from the painted text colour against the composited
    background beneath it;
  - `to_have_size`, and `to_use_text_style`.

  Tolerances are in logical pixels and default to 0.5. Every spatial assertion first requires the
  element to be visible (Galen's rule), and `to_be_within` has a `partly` mode. A failure reports
  the measured numbers ("label is 6.0 px right of centre").
- **R41.** **`audit.layout`** lints a window without assertions written in advance. It reports:
  - text off centre in a control whose alignment is centre;
  - near-miss alignment: sibling edges, centres or baselines that differ by more than the tolerance
    (0.5 px, shared with R40) and by at most 4 px;
  - uneven gaps in a row or column whose other gaps are equal;
  - overlap between siblings outside a stack or overlay;
  - clipped, ellipsized or overflowing text;
  - content outside the window or the safe area;
  - text contrast below WCAG 2.2 AA: 4.5, or 3 for large text. Large text is 24 logical px, or
    18.66 logical px bold (WCAG's 18 pt and 14 pt, with a logical px taken as a CSS px). A 0.01
    ratio tolerance absorbs rounding, as in Flutter and ATF. Disabled controls are skipped and
    listed under `not_checked`. Non-text contrast (WCAG 1.4.11, 3:1 for control boundaries and
    focus indicators) is an optional rule;
  - interactive targets below the minimum size, shared with R38;
  - a hit region smaller than the visible bounds.

  Each finding carries the node refs, the measured numbers, the rule and an **outcome**: `error`,
  `warning`, `needs_review` or `suppressed`, as in ATF's result types and axe's "incomplete".
  `needs_review` is where judgement-dependent results land: a contrast whose background was
  sampled from pixels, or a ratio within the tolerance band. A waived finding is reported as
  `suppressed` with its reason, never dropped. The thresholds are configurable per app, and an app
  can waive a finding on a node by identifier and rule, with a reason.
- **R41a.** **`audit.text_scale`** re-lays out a window at larger text scales (default 1.0, 1.5
  and 2.0, and the platform's largest accessibility size) and reports text that becomes clipped or
  truncated, and overlap that appears. Apple's `dynamicType`/`textClipped` audits and Android's
  font-scale checks sample a screen; FLUI does it with exact geometry at every scale.
- **R41b.** The planted-defect fixtures of the layout rules are published as a **benchmark**: each
  defect with its corrected twin, plus a `cargo xtask` harness that runs FLUI's lint over them. Its
  documentation reports what a screenshot-only check catches on the same set, so the claim that
  vision misses these defects is measured, not assumed. Published evaluations point that way (VLMs
  average 58% on BlindTest's close and overlapping shapes, and Lovable documents that subtle visual
  differences are not reliably evaluated), but none isolates 1–4 px misalignment.
- **R42.** A **layout snapshot** is a structured golden of a window: for each node with an
  identifier, plus role and name for the rest, its rect, text ink bounds and baseline, text style,
  and the colours it paints. It compares with geometric tolerances and diffs as numbers ("Save:
  x 320 → 326, colour #1A73E8 → #1967D2"). Font changes that move glyphs by a pixel do not break
  it the way pixel goldens did. It is the default golden for design work; pixel baselines (R22)
  stay for raster correctness. A snapshot can be a partial template: child matching `contain`
  (default), `equal` or `deep-equal`, and nodes left out are not compared (Playwright's
  aria-snapshot model). Updates use R22's modes.
- **R43.** **Annotated captures** draw bounds, centre lines, baselines, gaps and audit findings
  over a presented capture, for a person or a vision model to look at. Annotation is evidence for a
  finding, never the source of one. The measured numbers are printed as text on the image, and each
  finding also gets a zoomed crop, because models perceive close shapes poorly.
- **R44.** **No pass without evidence.** A report lists every check that ran and every check that
  was not run, with the reason (capability missing, not requested). A design or screen check passes
  only when the requested layout, accessibility and pixel or layout-snapshot checks all ran. MCP
  includes one core tool, `audit_screen`, that runs the layout and accessibility audits on a window
  and answers findings plus the checked and unchecked lists. An agent's "looks right" is never a
  verdict. Any model-judged result (a `vision` tool, an agent's reading of a capture) is advisory:
  it can add evidence to a report but never move it to `pass` or `fail`. Every audit names its
  scope (for example "visible viewport of w3; scrolled-out content not checked"). `not_checked`
  reasons include scrolled-out content, disabled nodes, image backgrounds and windows not
  requested.

### Coverage of the parity matrix

- **R45.** Every row of `docs/platform-parity.md` that a driver can exercise has a named
  scenario. A ✅ cell cites the scenario, the report's date and the host it ran on. A
  `cargo xtask parity` gate fails when a ✅ cell cites no scenario, or a scenario that does not
  exist. `flui drive report --parity` prints the rows a run proved, ready to paste. Each
  `docs/gaps.md` entry names the scenario that will close it, or says why none can.

### Self-contained by construction

The market splits this work across paid services: Applitools, Percy and Chromatic for visual
review; BrowserStack, Sauce Labs and LambdaTest for devices; mabl and testRigor for "AI" tests;
hosted trace viewers and dashboards. A FLUI developer who wants to test, check and improve an app
needs none of them. Everything comes in the box.

- **R46.** Every capability in this spec works offline and locally, with no account, subscription,
  hosted service, telemetry or API key:
  - capture, comparison and baseline review;
  - layout and accessibility audits, performance budgets and exploration;
  - traces, replay and codegen;
  - the inspector, reports and the MCP server.

  An AI agent is an optional user of the tool, never a dependency of a check: no check calls a
  model, and every check passes or fails the same with no model present.
- **R47.** The only things outside FLUI a run may need are the platform vendors' own free tools
  for a target: the Android SDK (`adb`, emulator), Xcode and its Simulator, and Chrome with its
  matching ChromeDriver. ChromeDriver comes from the Chrome for Testing archive and is cached, and
  every one of these can be pointed at a local install for fully offline runs (`FLUI_CHROME`,
  `CHROMEDRIVER`, `ANDROID_HOME`, `DEVELOPER_DIR`). A missing tool is *cannot verify* with the
  command that installs it, never a silent download of anything else. The driver opens no network
  connection beyond loopback, the `adb` forward and that one archive fetch.
- **R48.** What paid services sell is built in:
  - **Visual and layout review:** `flui drive baselines review` and the inspector's review view list
    every changed, new and missing baseline (pixel and layout snapshot). Each shows expected,
    actual and diff side by side with the numeric layout diff, and is accepted or rejected per item,
    writing the baseline files. This is Chromatic's and Percy's review loop, local and in git.
  - **Reports for people and CI:** a static HTML report per run (checks, findings, captures,
    annotated crops, perf charts, trace link) that opens from disk with no server, plus JUnit XML
    for any CI and SARIF for audit findings, so code hosts show them inline.
  - **Devices:** local emulators and simulators are created, booted and wiped by the driver, so a
    matrix runs on one machine. The management code moves from `flui-cli`'s `emulators` command
    into the driver's Android and iOS lanes, and `flui emulators` calls it there. A device cloud is never
    required; one reachable over `adb` is just another Android target.
  - **Trace viewing:** in the inspector and in the HTML report, never a hosted viewer.
  - **Flake triage:** `--repeat N` and `--retain-on-failure` with a per-check pass rate in the
    report, so a flaky check is visible and is never retried into a pass.
- **R49.** Licences: everything the driver and inspector link passes `deny.toml`, so a user can ship
  and modify them under FLUI's own terms.

### Introspection and tracing across crates

The driver can only report what the framework can describe, and today that description is weak.
`Diagnosticable` stores every property as a `Display` string that tests parse back
(`get_property_f64`), and it allocates a `String` per name and value on every walk. Views and
elements describe nothing. Tracing has four target spellings, phase spans matched by name alone,
recognizer spans that are never entered, and ids logged as `Debug` strings. Errors carry bare ids.
Each of these was weighed against prior art in the
[introspection and tracing survey](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/research/2026-10-18-flui-introspection-and-tracing.md),
and each idea was accepted or rejected there with a reason.

- **R50.** **`Inspect` replaces `Diagnosticable`** (owner decision). It is a typed, borrowed
  visitor: a closed `Value` set (bool, integers, float, str, enum variant, size, offset, rect,
  colour, edge insets, matrix, duration, typed link, and one counted `Debug` escape hatch), with
  per-field metadata (level, unit, typed default) and no `Debug` supertrait. A walk allocates
  nothing unless its sink does. `#[derive(Inspect)]` covers named, tuple and unit structs and
  enums, refuses a field type with no value mapping at compile time, and resolves through
  `flui-sdk`, so official packages can use it. Render objects and layers require `Inspect`. Views
  describe themselves through a provided method that the derive fills, so a user view compiles
  without one. `Diagnosticable`, `DiagnosticsBuilder`, string properties and the type-name
  scanner are deleted, not deprecated.
- **R51.** **One description, every consumer.**
  - The tree, not the object, assigns identity: `InspectId` over the generational ids.
  - Typed `Link`s join element, render, layer and semantics nodes.
  - An owned snapshot serializes as the protocol's `InspectNode`.
  - Tests read typed fields (`node.get::<f64>("width")`, `find_by_link`), never rendered text.
  - A structural diff compares snapshots with unit-derived tolerances; trace diffs, layout
    snapshots and reports use it.
  - One tree renderer serves tests, `flui drive inspect`, dumps and error reports. Geometry has a
    compact, pinned `Display` (`360x48`, `@8,8`, `8,8 344x32`, `#RRGGBB`). Colour appears only on a
    TTY without `NO_COLOR`. `--json` is the machine path.
- **R52.** **Tracing conventions** hold in every framework crate:
  - contract targets are `flui.<subsystem>` (`flui.frame`, `flui.tree`, `flui.input`,
    `flui.animation`, `flui.signals`, `flui.pace`); the old spellings keep a one-release alias,
    after a test through the real `EnvFilter` confirms prefix matching;
  - a span is an interval, an event is a point. Spans that are never entered become events or are
    entered;
  - frame-path fields are primitives only (`u64`, `i64`, `bool`, `f64`, `&'static str`); ids are
    `u64` that map back to generational ids, and kinds are `&'static str`. No `?x` or `%x` on a hot
    path;
  - the hierarchy is a `frame` span, then phase spans (build, layout, compositing, paint,
    semantics, commit, submit) at DEBUG, then per-node spans at TRACE only at relayout-boundary and
    rebuild roots. `element_rebuild` moves from INFO to TRACE, and carries the rebuild reasons as a
    bitmask;
  - a disabled level costs nothing on the frame path: `tracing::enabled!` gates any field that
    costs to compute, and nothing allocates before the macro. Framework crates never set
    `max_level_*` features, so release profiling and the driver's `perf` keep working.
- **R53.** **One span stream.** A single capture layer, partitioned by runtime and presentation
  id, feeds the profiler, the devtools timeline, the driver's `perf` and trace rows, and a
  Chrome/Perfetto export with `X` events and counters. Phases are recognized by
  `(target, name)` from one table, so an app span named `paint` cannot spoof a phase. `Timeline`
  becomes a view over the capture, not a second recorder. Tree mutation stays on the ADR-0040
  observer and time stays on spans; the driver joins them by element id.
- **R54.** **Creation sites.** A debug-only `CreationSite` moves from `flui-animation` to
  `flui-foundation`. It is zero-sized in release and filled by `#[track_caller]` constructors and
  builders, which the widget derive generates. Elements copy it at mount, and the inspector,
  findings (SARIF locations), errors and traces show `file:line:col`. Struct-literal construction
  gets no site; constructors are the documented entry point.
- **R55.** **Errors with tree context.** A `TreeError<E>` wrapper carries an ancestor path (kind
  and id, capped) and the creation site. It is attached by the boundary that owns the tree, the
  pipeline owner or the build owner, so hot-path error values stay id-only. Frame containment adds
  the current phase and node to an internal-invariant report. Its `Display` reads "layout cycle at
  RenderPadding (r12) < RenderFlex (r7), created at src/app.rs:41:13", and the driver's findings
  and trace rows carry the same context.

## Failure scenarios (mandatory at level 1)

- **F1 Disconnection.** The app exits or crashes mid-command. The command fails as `gone` (kind
  `process`) with the exit status. Any pending OS input is released. A later command on the
  session fails the same way, never hangs.
- **F2 Driver crash.** The driver process dies. The app sees the connection close and leaves no
  held synthetic input (its held-input lane releases on disconnect). The launched app is ended by
  the OS-level owner of its process (job object, process group, `adb` force-stop or `simctl
  terminate`).
- **F3 Timeout of an accepted action.** The action stays queued and runs at the next frame
  boundary. The driver reports `may_have_run` and never resends. The next read shows whether it
  ran.
- **F4 Panic in app code during a driver request.** The request fails as `platform`, carrying the
  first failure. The agent stays serving, and the next request is answered (contained per
  `docs/PANIC-POLICY.md`).
- **F5 Reentrancy.** A user callback that the driver's action triggers closes the window,
  replaces the root or ends the app inside the frame. The action's result is consistent (`gone`
  or `ran`), and the agent never holds a borrow across user code.
- **F6 Stale identity.** An element removed and a new one created in the same slot: the old handle
  answers `gone`. Exhausting handle generations refuses permanently. Two windows with equal
  element numbers never alias.
- **F7 Concurrent drivers.** Two clients on one app are serialized per window. Two OS-input
  sessions on one desktop refuse with `busy` instead of interleaving events.
- **F8 Unsupported platform path.** A capability missing on a target gives `not_supported`,
  naming the capability, and the report's cannot-verify list names it too. It is never a panic or
  an empty success.
- **F9 Coordinates.** Mapping logical bounds to device pixels and screen points stays finite and
  in range for extreme scale factors and window positions. A rect outside the window is
  `outside_target`, never clamped into it.
- **F10 Resource exhaustion.** Oversized trees and captures are budgeted (`truncated`, size caps),
  and trace evidence has a size cap with a recorded `gap`. A full disk fails the trace write with
  an error and never corrupts an existing trace.

- **F11 User interrupts an OS run.** The user moves the mouse or types mid-run. The lane
  stops before its next event, releases everything it pressed, and the run ends *cannot verify*
  ("interrupted by user input") with the trace kept. It never fails and never continues.
- **F12 Exploration finds a fault.** The run stops at the first broken invariant, keeps the
  trace, and shrinks it. A shrink step that no longer fails is discarded. The reported seed and
  the minimal trace reproduce the failure on replay.

## Out of scope

- Driving non-FLUI applications. `tools/desktop-mcp` is retired, not generalized (owner decision,
  2026-10-17).
- Drivability of optimized builds without the `driver` feature.
- Network-remote targets other than `adb` devices: no TCP endpoint is ever opened by the app.

## Decisions (owner, 2026-10-17)

- **D1.** The truth source is an in-app agent (trees, frames, synthetic input, presented pixels).
  OS lanes deliver real input and cross-check. Alternatives rejected: OS-only black box (no
  widget or render trees, no frame waits), in-app only (never proves OS delivery).
- **D2.** One published crate, `flui-driver`, under the `flui drive` CLI and `flui mcp`; it
  absorbs desktop-mcp's OS lanes and the xtask drivers.
- **D3.** The agent is in debug builds; optimized builds get it only through an explicit `driver`
  feature.
- **D4.** Pixels: per-backend baselines with tolerance, masks and artifacts, plus baseline-free
  predicates. Bundled fonts.
- **D5.** iOS Simulator input goes through the simulator's HID channel from Rust, with a version
  probe and `not_supported` on mismatch. No Swift.
- **D6.** Semantics gain an identifier.
- **D7.** A trace is selector-based JSONL bundling the input and native recordings. The CLI
  replays it and generates Rust tests from it.
- **D8.** FLUI apps only; desktop-mcp is retired.
- **D9.** The spec goes to `main` now. Implementation follows the merge of `interaction/completeness`.
- **D10.** People get the `flui drive` CLI and an inspector app written in FLUI.

## Review decisions (owner, 2026-10-17, review of the first draft)

- **D11.** OS input does not take over the developer's desktop. `synthetic` is the default path;
  OS input is opt-in, announced, aborted by any user activity, and runs in an isolation mode where
  one exists (R10, R14a, R14b).
- **D12.** Agent ergonomics: MCP lists a core tool set of 13 tools, the rest behind
  `--caps` groups, and the post-action reading is a snapshot delta (R16, R28a).
- **D13.** Performance is measured and budgeted: frame times, jank and input-to-present latency
  (R37, R37a).
- **D14.** The accessibility audit is a first-class method and scenario (R38).
- **D15.** Seeded exploration runs with invariants and shrinking (R39).
- **D16.** OS-lane determinism rules: fixed window size, single-step moves, per-lane replay
  fidelity (R32a).
- **D17.** `windows-gallery` is the first scenario ported, and every interactive gallery widget
  carries an identifier (R36, R36a).
- **D18.** The driver also serves interface design. Exact layout assertions, a layout lint and
  layout snapshots catch what screenshot-judging design agents miss, such as text off centre or
  near-miss alignment (R40–R43).
- **D19.** No verdict without evidence: reports list what ran and what did not, and MCP's
  `audit_screen` is a core tool (R44).
- **D20.** The driver proves the parity matrix: ✅ cells cite scenario runs, and a gate checks
  them (R45).

- **D21 (owner, 2026-10-18).** Self-contained: the library provides everything a user needs to
  test, check and improve an app, with no paid or third-party service. Review, reports, devices
  and trace viewing are built in, and no check depends on an AI model (R46–R49).

- **D22 (owner, 2026-10-18).** `Diagnosticable` is replaced by a typed `Inspect` model.
  Observability across crates is designed from prior art, with each idea accepted or rejected
  explicitly (R50–R55; ADR-0242).
  - **Rejected:** `bevy_reflect` or `facet` as the core model; `valuable`, which is still behind
    `tracing_unstable`; serde as the description trait (it stays the output format);
    Compose-style source strings; a second profiling API; `max_level_*` in framework crates;
    `miette` source-span errors.

## Market survey refinements (2026-10-18)

The [market and prior-art survey](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/research/2026-10-18-flui-driver-market-survey.md)
corrected three draft errors:
- large-text contrast thresholds;
- the near-miss lower bound;
- a single minimum target size where the platforms differ.

It also added, from shipped tools:
- finding outcomes (`needs_review`, `suppressed`) and a text-scale audit (R41, R41a);
- partial layout templates and Playwright's update modes (R22, R42);
- dropped and janky frames kept separate, with INP's phases and iteration rules (R37, R37a);
- token budgets and narrowing hints for agents (R15, R16, R28a);
- Windows isolation candidates that must pass a spike (R14b);
- the honest limit of the activity guard (R14a);
- "advisory only" for model-judged results (R44);
- a benchmark that measures the vision-miss claim (R41b).

It confirmed R10, R16's delta, R18, R40–R42 and R45 as gaps in the market rather than copies.
