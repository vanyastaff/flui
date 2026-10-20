# flui-driver: market and prior-art survey

- **Date:** 2026-10-18
- **For:** `docs/plans/specs/flui-driver/` (requirements.md R1-R45, design.md)
- **Method:** primary sources fetched on 2026-10-18 (vendor docs, GitHub source, arXiv, W3C/MCP specs).
  Where only a secondary source (blog, forum, search summary) was available, the claim is tagged
  **[secondary]**. Anything not checked is listed in section 9. No product capability is assumed
  where its documentation is silent; those cases say "undocumented".

## 1. Summary

1. **No surveyed AI design/build tool documents a geometric verification step.** The documented
   loops are: screenshot plus model judgement (Lovable browser testing, Anthropic computer use,
   Maestro `assertWithAI`), console/log error reading (Bolt, v0, Lovable), or "the agent gets a
   screenshot and decides" (Figma MCP `get_screenshot`). Figma, pen.dev, Stitch, Uizard, Galileo,
   Builder.io, Anima and Locofy document no output-checking tool at all (section 2.A).
2. **Vendors state the weakness themselves.** Lovable's docs: "Subtle visual design or color
   differences aren't reliably evaluated." Anthropic's computer-use docs: "Claude sometimes assumes
   an action worked without checking" and recommend a prompt that forces a screenshot-and-evaluate
   step after every action.
3. **Published VLM evidence supports the owner's concern.** BlindTest: four VLMs average 58.07% on
   overlap/intersection/count tasks, best 77.84% (Claude 3.5 Sonnet); accuracy falls when shapes
   are close or overlap and returns to near 100% with more space between them. UI-Lens (CVPR 2026)
   **[secondary]**: F1 of 20.36% for text overflow and 31.21% for container overlap. Design2Code:
   models "lag" at recalling visual elements and at producing correct layouts. No benchmark that
   isolates 1-4 px misalignment or uneven gaps was found.
4. **Exact-geometry layout checking exists only for the DOM, and only as a niche.** Galen
   Framework (`centered`, `aligned`, `inside`, `near` with pixel tolerances) is the closest
   precedent for R40; it is Selenium-bound and its maintenance status was not verified. Playwright
   has no geometry assertion beyond `toBeInViewport`. Nothing found does a lint of near-miss
   alignment or uneven gaps (R41), or a structured geometric golden with numeric diffs (R42).
5. **Accessibility auditors are the mature prior art for typed, thresholded rules.** Android ATF has
   14 check classes with result types ERROR/WARNING/INFO/NOT_RUN/SUPPRESSED, explicit "could not
   get background colour" results and a 0.01 contrast tolerance. XCTest audits take a handler that
   suppresses issues per type and element. Flutter's guidelines use 48/44 logical px targets and a
   self-described "very naive" pixel-histogram contrast estimate. FLUI can compute contrast from the
   layer tree; that is an accuracy advantage over all of these.
6. **Honest "could not check" is rare and valuable.** ATF returns `NOT_RUN` and per-cause result
   IDs; axe marks some contrast cases "incomplete". Most other tools silently skip. R44 formalizes
   what only ATF and axe do partially.
7. **Tool-count practice is inconsistent.** Official MCP servers list 17 (Anthropic computer-use
   toolset), 21 (Windows-MCP), about 28 default of 31 (Appium MCP), 41 (Figma, my count), 59
   (Chrome DevTools MCP) and 72 (Playwright MCP) tools. Playwright MCP and Chrome DevTools MCP
   tier by `--caps` / `--slim`; Windows-MCP by `--tools`/`--exclude-tools`. FLUI's 13-tool core with
   capability groups is in line with the best of them and smaller than all of them.
8. **Snapshot deltas are not in the official Playwright MCP docs.** Only a third-party fork
   advertises a differential mode **[secondary]**. Official snapshots are full accessibility
   trees with refs; `--snapshot-boxes` adds `[box=x,y,w,h]`. A delta default (R16) is a real
   differentiator, and the missing piece in the market is geometry in the outline.
9. **Desktop isolation is mostly unsolved in the market.** Windows-MCP warns that its actions
   "can interfere with your own input" and that user takeover is "best-effort, not a security
   boundary". Terminator claims it "Doesn't take over your cursor or keyboard" without describing
   a mechanism. Only UFO² documents a mechanism: a Picture-in-Picture separate virtual desktop
   **[secondary]**. Anthropic recommends a dedicated VM or container. R10/R14a/R14b are ahead of
   the field.
10. **Self-healing tests substitute locators silently.** Katalon, Healenium-style tools store
    fallback locators and ask an LLM when they run out; sources agree false heals (the wrong
    element) are the main weakness and recommend human review of heal reports. FLUI's
    refuse-on-ambiguity (R7) is the opposite choice and the safer one.
11. **Performance budgets are rarely asserted in UI tests.** Flutter's `TimelineSummary` writes
    numbers and asserts nothing. Macrobenchmark reports P50/P90/P95/P99 and `frameOverrunMs`; the
    CI guidance page was not retrievable. JankStats defines jank as a frame taking twice the refresh
    interval by default. Apple's hitch ratio target is 5 ms/s or less **[secondary]**. INP: 200 ms
    good, 500 ms poor, at p75. R37/R37a are stronger than any one of these.
12. **Exploration tools are seeded but shallow.** Android Monkey is seed-repeatable (`-s`), with
    throttle, event-mix percentages and a package fence; Firebase Robo is deterministic per device
    config but documents no seed, and Test Lab is deprecated (shutdown 2027-09-30). Research tools
    (Stoat, APE, Sapienz, Humanoid) report coverage wins mostly in the authors' own papers.
    Hypothesis shrinks on the generator's intermediate representation, which supports shrinking an
    action list of locators rather than coordinates (R39).
13. **MCP revision 2026-07-28 is current** and drops the initialization handshake for a
    per-request version in `_meta` plus a mandatory `server/discover` RPC. R29 should name that.
14. **Where FLUI can be clearly better:** exact geometry facts (ink bounds, baselines, composited
    colour), a lint that is allowed to say "not checked", a geometric golden, per-cell parity
    evidence, an input path that does not take the desktop, and a verdict that cannot be "looks
    right".

## 2. A. AI design and build agents: how they verify output

| Tool | Documented verification loop | Failure modes stated or evident | Source |
|---|---|---|---|
| Figma MCP server | Read tools (`get_design_context`, `get_metadata` XML outline, `get_screenshot`, `get_variable_defs`); write tools (`use_figma`, `generate_figma_design`, `html_to_figma`). No tool compares output to design. | Comparison is left to the agent; screenshot is the only visual signal | https://developers.figma.com/docs/figma-mcp-server/tools-and-prompts/ |
| pen.dev (pencil.dev) | MCP tools `get_style`, `read_skill`, `get_app_state`, `execute` (+ `browser`, `spawn_agents` conditional). No screenshot or layout-check tool is listed. | Docs only advise confirming the active document | https://docs.pencil.dev/getting-started/ai-integration |
| Lovable | "Browser testing": real browser in a remote VM; screenshots, console, network; clicks, forms, three viewport sizes. Runs selectively. | "Subtle visual design or color differences aren't reliably evaluated"; icon-only buttons harder; canvas unusable | https://docs.lovable.dev/features/browser-testing |
| Bolt.new | WebContainer dev server; agent reads build/runtime errors and fixes | Documented as an error loop; no visual check described **[secondary]** | https://addyo.substack.com/p/ai-driven-prototyping-v0-bolt-and |
| v0 | No official verification doc found. Third-party skills pair coding agents with Vercel's `agent-browser` (ref-annotated a11y tree + screenshots) | Undocumented for v0 itself | (none; see section 9) |
| Google Stitch | MCP gives agents layout, colours, HTML, screenshots; `DESIGN.md` carries rules. No validation step described. | Output "a starting point, not shippable" **[secondary]** | https://pasqualepillitteri.it/en/news/647/google-stitch-mcp-export-claude-code-design-to-code |
| Builder.io Visual Copilot, Anima, Locofy | Vendor docs not fetched; reviews describe fidelity as reviewer opinion; no automated screenshot-versus-render check found | Undocumented | search only (section 9) |
| Uizard, Galileo | Not researched beyond the search; undocumented here | Undocumented | (section 9) |
| Anthropic computer use | Screenshot + coordinates; 17 member tools; docs recommend "After each step, take a screenshot and carefully evaluate" | "Sometimes assumes an action worked without checking"; small targets missed after downscale; dropdowns and scrollbars hard | https://platform.claude.com/docs/en/docs/agents-and-tools/tool-use/computer-use-tool |
| Maestro `assertWithAI` | Screenshot sent to an LLM with a plain-language assertion; true/false | "Experimental"; `optional` defaults to true "so AI responses don't break CI/CD" | https://docs.maestro.dev/api-reference/commands/assertwithai |
| Playwright MCP / Chrome DevTools MCP / agent-browser | Accessibility snapshot with refs is the primary signal; screenshot is read-only ("You can't perform actions based on the screenshot") | Geometry only via opt-in `--snapshot-boxes` (Playwright MCP) | https://raw.githubusercontent.com/microsoft/playwright-mcp/main/README.md |

### Published evaluations of model layout judgement

| Source | Result | Relevance |
|---|---|---|
| BlindTest, "Vision language models are blind" | Four VLMs avg 58.07%; Claude 3.5 Sonnet 77.84% (best); accuracy drops when shapes overlap or sit close, near 100% with more spacing | Near-miss and tight-gap cases are exactly the regime where it fails. https://arxiv.org/abs/2407.06581 |
| Design2Code (484 real pages) | GPT-4o/4V, Gemini, Claude "lag in recalling visual elements" and in layout | Generation-side evidence, not detection. https://arxiv.org/abs/2403.03163 |
| UI-Lens, CVPR 2026 | F1 20.36% text overflow, 31.21% container overlap, described as near random for boundary tasks. Dataset size differs between sources (4,759 vs 4,566 pages) | Detection of clipped text and overlap by MLLMs. **[secondary]**: paper PDF returned 403. https://openaccess.thecvf.com/content/CVPR2026/html/Xiang_UI-Lens_Assessing_General_MLLMs_Potential_to_Automate_UI_Display_Quality_CVPR_2026_paper.html |
| CANVAS (598 tool-based design tasks) | Models "commonly fail to infer appropriate dimensions from text attributes (font, size, spacing)" **[secondary]** | Agents operating design tools lack dimension sense. https://arxiv.org/abs/2511.20737 |

No benchmark isolating near-miss alignment (1-4 px) or uneven gap detection was found. The claim in
the spec's design.md ("the model does not notice a label 6 px off centre") is consistent with the
above but is not directly measured by any source; the spec should call it an expectation, and the
project could publish its own measurement (a fixture set with and without planted defects, judged by
a vision model) as evidence.

### Decisions drawn from A

- Treat every screenshot-derived verdict as untrusted; keep R43 ("annotation is evidence, never the
  source of a finding") and R44.
- BlindTest's result that spacing restores accuracy implies annotated captures should carry the
  measured numbers as text and offer a per-finding zoomed crop; do not rely on the model perceiving a
  gap.
- Maestro's default `optional: true` shows the market treats AI visual assertions as non-blocking.
  FLUI's `vision` group must never be able to turn a failing run into a pass or a passing run into
  a fail on its own.

## 3. B. Layout and visual verification tools

| Tool | What it compares | Tolerance model | Golden format | Source |
|---|---|---|---|---|
| Galen Framework | Element boxes from a live browser: `inside`, `near`, `above/below`, `centered`, `aligned`, `width/height`, `text`, `css`, `absent` | Ranges (`50 to 200 px`), `~ 100 px` (tolerance undefined in docs), optional error px on `centered`/`aligned` | Spec text files (hand-written rules), no golden | https://galenframework.com/docs/reference-galen-spec-language-guide/ |
| Applitools Eyes | Screenshot image; match levels Strict (default for regression), Layout, Ignore Colors, Exact | Level, not a number; Strict "aims to ignore" platform pixel noise; Layout checks relative positions and ignores text/colour | Server-side baseline images | https://applitools.com/docs/eyes/concepts/best-practices/match-levels |
| Playwright `toHaveScreenshot` | Pixels | `threshold` 0 to 1 default 0.2 (pixelmatch), `maxDiffPixels`; retakes until two consecutive shots match | PNG named with browser and platform suffix | https://playwright.dev/docs/test-snapshots |
| Playwright `toMatchAriaSnapshot` | Accessibility tree as YAML | Partial templates; `/children`: `contain` (default), `equal`, `deep-equal`; regex names | `.aria.yml`; update modes `missing`, `changed`, `all`, `none` | https://playwright.dev/docs/aria-snapshots |
| Chromatic | Pixels | Default threshold .063; anti-aliased pixels ignored by default | Cloud baselines | https://canon.chromatic.com/docs/threshold |
| Argos | Pixels | Advises one canonical render environment; diff that ignores 1 px glyph-edge noise | Cloud baselines | https://argos-ci.com/blog/fix-flaky-visual-tests |
| swift-snapshot-testing | Image, JSON, plist, `recursiveDescription` | `precision` (share of identical pixels) and `perceptualPrecision` (max colour deviation) | Files beside the test | https://github.com/pointfreeco/swift-snapshot-testing/pull/638 |
| Roborazzi / Paparazzi | Compose/View screenshots on JVM | Roborazzi `changeThreshold`; fix device qualifiers to avoid flake | PNG | https://spin.atomicobject.com/snapshot-testing-roborazzi/ **[secondary]** |
| Percy, BackstopJS, Loki, reg-suit, Happo, Lost Pixel, Meticulous | Not researched (section 9) | | | |
| Figma lint plugins (Design Lint, Formalist, Stark) | Missing styles, unnamed layers, fractional coordinates, WCAG contrast | Plugin-specific | n/a | https://designcode.io/figma-handbook-design-lint **[secondary]** |

### Accessibility and contrast rule sets

| Source | Rules and thresholds | False-positive handling |
|---|---|---|
| Android ATF (source on GitHub) | Check classes: `TouchTargetSizeCheck`, `TextContrastCheck`, `ImageContrastCheck`, `TextSizeCheck`, `DuplicateClickableBoundsCheck`, `DuplicateSpeakableTextCheck`, `SpeakableTextPresentCheck`, `TraversalOrderCheck`, `EditableContentDescCheck`, `RedundantDescriptionCheck`, `ClickableSpanCheck`, `ClassNameCheck`, `LinkPurposeUnclearCheck`, `UnexposedTextCheck`. Touch target 48x48 dp; 32 dp at the scrollable edge or in an IME container. Contrast tolerance 0.01. `TextSizeCheck`: minimum 16, adequate 28 (units per source constants), occupied-fraction threshold 0.70. | Result types ERROR, WARNING, INFO, NOT_RUN, SUPPRESSED; per-cause IDs such as "could not get background colour", "text must be opaque", "borderline" contrast, "screen capture uniform colour". Touch target check models `TouchDelegate`, clickable ancestor, scrollable edge, clipping by ancestor. https://github.com/google/Accessibility-Test-Framework-for-Android |
| Android Accessibility Scanner | Touch target 48x48 dp (32 dp in IME window or at display edge); configurable contrast minimums | "Doesn't replace manual testing" https://support.google.com/accessibility/android/faq/6376582 |
| Flutter `meetsGuideline` | `androidTapTargetGuideline` 48x48, `iOSTapTargetGuideline` 44x44 (paint bounds / devicePixelRatio); `textContrastGuideline` 4.5, large 3.0 (large = size >= 18, or bold and >= 14), -0.01 tolerance; AAA 7.0/4.5; `labeledTapTargetGuideline`: tap or long-press action with empty label and tooltip fails | Contrast is a histogram of rendered pixels split by lightness ("very naive" per source comment; single-colour region reports 1.0). Skips: merged, hidden, disabled, links, nodes touching a scrollable or view edge. Only `Text`/`EditableText` supported. https://raw.githubusercontent.com/flutter/flutter/master/packages/flutter_test/lib/src/accessibility.dart |
| XCTest `performAccessibilityAudit` | Types (from search, Apple page did not render): contrast, dynamicType, elementDetection, hitRegion, sufficientElementDescription, textClipped, trait, all | Issue handler returns true to suppress; per call only visible elements are inspected **[secondary]**. https://wwdcnotes.com/notes/wwdc23/10035 |
| axe-core `color-contrast` | 4.5:1; 3:1 large (18 pt = 24 CSS px, or 14 pt bold = 19 CSS px) | Skips text over background images or obscured; 1:1 ratio becomes "incomplete" for manual review; gradients/overlaps are known hard cases. https://dequeuniversity.com/rules/axe/html/4.10/color-contrast |
| WCAG 2.2 | 1.4.3 contrast 4.5:1, large 3:1; 1.4.11 non-text 3:1 for UI component boundaries and states | https://www.w3.org/TR/WCAG22/ (SC 2.5.8 24 px target text was beyond the fetched portion; section 9) |

### Decisions drawn from B

- **Rule shapes worth copying:** ATF's result type set with `NOT_RUN` and `SUPPRESSED`; per-cause
  result IDs; Galen's "every spec first checks the element is visible"; Galen's optional error value
  on centred/aligned checks (R40 tolerance parameter); Playwright's retry-until-stable capture (R22
  already has it) and its `missing` update mode.
- **Platform-specific targets, not one number.** 48 dp Android (32 at edges/IME), 44 iOS, 24 CSS px
  WCAG minimum **[WCAG number unverified here]**. R38's single `min_target` should have per-platform
  defaults and the exemptions ATF documents (scrollable edge, clickable ancestor, clipping).
- **Contrast has a three-state outcome, not two.** ATF's borderline result, axe's incomplete, and
  Flutter's 0.01 tolerance all exist because sampled backgrounds are uncertain. FLUI's composited
  colour is exact when the layer tree has opaque paints; where it falls back to a `sampled` capture
  the finding should be `needs_review`, not `pass`.
- **Golden format.** Applitools Layout is relative-position only and ignores text and colour;
  aria snapshots are semantic and carry no geometry; swift `recursiveDescription` is a text dump.
  None is a numeric geometric golden. R42 occupies an empty slot, and borrows the aria snapshot
  `contain`/`equal`/`deep-equal` children modes and regex names.
- **Pixel diff tolerance.** Playwright's pixelmatch threshold 0.2 and Chromatic's .063 with
  anti-alias ignoring are the reference points for R22's "per-pixel YIQ threshold"; the spec should
  state its default and whether anti-aliased pixels are ignored.

## 4. C. Mobile, desktop and web automation market

| Tool | Locator model | Waiting / determinism | Isolation of desktop input | Source |
|---|---|---|---|---|
| Playwright | Role/name/test id; aria snapshot; auto-wait with actionability | Auto-retrying assertions; `toHaveScreenshot` stabilization | Headless browser contexts | https://playwright.dev/docs/test-assertions |
| Playwright Test Agents | Planner (Markdown plan), Generator (verifies selectors live), Healer (replays, patches locator/wait/data, may skip the test if it judges the app broken) | Guardrails not described | n/a | https://playwright.dev/docs/test-agents |
| Maestro | YAML flows; text/id selectors; MCP tools `inspect_view_hierarchy` (CSV), `take_screenshot`, `run_flow`, `run_flow_files`, `check_flow_syntax`, `launch_app`, `list_devices` | Docs index did not state waiting or flakiness policy (section 9) | Emulator/simulator/cloud | https://docs.maestro.dev/get-started/maestro-mcp |
| Appium 2 + Appium MCP | Accessibility id > id > iOS predicate / UiAutomator > XPath "last resort"; `appium_ai` (screenshot to vision model) opt-in via `AI_VISION_ENABLED` | Session-based; 31 tools, about 28 by default | Real devices, simulators | https://github.com/appium/appium-mcp |
| Detox | Matchers on native views | Gray-box synchronization: waits for network, timers, animations, main-thread work, RN bridge; `--debug-synchronization` prints what is busy | Simulator/emulator | https://github.com/wix/DetoxSync, https://github.com/cliqz-oss/detox/blob/detox@5.9.1/docs/Troubleshooting.Synchronization.md **[secondary, older docs]** |
| Patrol (Flutter) | Flutter finders plus `$.native` over UIAutomator / XCUITest | Built on `integration_test` | Android and iOS only; Windows/Linux not supported **[secondary]** | https://github.com/leancodepl/patrol |
| Katalon / Healenium-style self-healing | Primary locator plus 3-5 stored fallbacks; LLM fallback reads page source, a11y tree, screenshots | Heals silently, suggests locator update after the run | n/a | https://docs.katalon.com/katalon-studio/maintain-tests/self-healing-tests-in-katalon-studio |
| Firebase Test Lab Robo | Crawls widget tree; falls back to Monkey taps on non-widget screens; Robo scripts | "Always simulates the same user activities in the same order" on the same config; no seed documented; 120-300 s typical budgets; Test Lab deprecated, shutdown 2027-09-30 | Google-hosted devices | https://firebase.google.com/docs/test-lab/android/robo-ux-test |
| Android Monkey | None (random events) | `-s <seed>` repeatable; `--throttle`; `--pct-touch/motion/nav/...`; `-p` package fence; stops on crash unless `--ignore-crashes` | Device/emulator | https://developer.android.com/studio/test/other-testing-tools/monkey |
| Selenium 4/BiDi, WebdriverIO, Cypress, TestCafe, Robot Framework, mabl, testRigor, Functionize, BrowserStack/Sauce/LambdaTest | Not researched (section 9) | | | |

### Decisions drawn from C

- **Locator ordering.** Appium MCP's stated order (accessibility id, id, native predicate, XPath
  last) matches R6's "identifier and semantics first, coordinates last".
- **Self-healing.** The real mechanism is stored fallback locators plus an LLM fallback; the
  documented risk is a plausible wrong match. R7's `ambiguous` refusal is the opposite choice. A
  useful middle ground that does not heal: on `not_found`, list the nearest candidates (by role,
  name edit distance, identifier) in the error so an agent or `flui drive` user can correct the
  locator explicitly.
- **Gray-box synchronization is the proven answer to flake** (Detox tracks network, timers,
  animations). R18 already waits on the frame and event timeline, and its "why the app had not
  settled" output matches `--debug-synchronization`.
- **Test agents** (Playwright) work from locators and the a11y tree, not from vision. That supports
  the semantics-first MCP design.
- **Desktop isolation in this category** is: headless browser contexts, emulators, cloud devices.
  Native desktop test tools were not surveyed (section 9); Microsoft's older UI-testing guidance
  says visible-UI tests need an unlocked, focused desktop, so parallel runs need separate machines
  (https://github.com/MicrosoftDocs/azure-devops-docs/blob/main/docs/pipelines/test/ui-testing-considerations.md
  **[secondary]**).

## 5. D. Desktop computer-use and MCP servers

| Server | Tools | Snapshot format | Takeover / interference handling | Source |
|---|---|---|---|---|
| Anthropic computer use | 17 member tools in one toolset (screenshot, zoom, clicks, drag, move, scroll, type, key, hold_key, wait...) | Screenshot only; coordinates in screenshot pixels; max 2576 px long edge (current models) | Recommends dedicated VM or container, minimal privileges, allowlisted domains, human confirmation for consequential actions | https://platform.claude.com/docs/en/docs/agents-and-tools/tool-use/computer-use-tool |
| Microsoft UFO / UFO² | Host agent + app agents; hybrid UIA + vision control detection; GUI actions plus API calls; speculative multi-action batching "cutting LLM calls by 51%" | Not described in README | Picture-in-Picture: automation in a separate virtual desktop, built on RDP loopback **[secondary]** | https://github.com/microsoft/UFO, https://www.microsoft.com/en-us/research/publication/ufo2-the-desktop-agentos/ |
| Windows-MCP (CursorTouch) | 21 tools (Click, Type, Snapshot, Screenshot, App, PowerShell, Registry...) | `Snapshot` returns interactive element ids and scrollable regions; vision opt-in via `use_vision` | Desktop control policy `strict` / `balanced` / `relaxed`; docs: actions "can interfere with your own input"; takeover "best-effort, not a security boundary"; no VM advice; telemetry on by default | https://github.com/CursorTouch/Windows-MCP |
| terminator (mediar-ai) | MCP agent; tool list not in README | `key:value` selectors (`role:Button`, `name:Seven`, `window:Calculator`) | Claims it "Doesn't take over your cursor or keyboard"; no mechanism or evidence given; Windows only; "100x faster" claim unsubstantiated | https://github.com/mediar-ai/terminator |
| Playwright MCP | 72 tools in groups: core 24, tabs 1, config 1, network 4, storage 17, devtools 13, vision 6, pdf 1, testing 5 (some via `--caps`) | A11y tree `- role "name"` with refs; `--snapshot-boxes` adds `[box=x,y,w,h]`; `browser_find` is "cheaper than capturing the whole snapshot"; `--snapshot-mode full|none` | `--headless`, `--isolated` in-memory profile | https://raw.githubusercontent.com/microsoft/playwright-mcp/main/README.md |
| Chrome DevTools MCP | 59 tools; categories include input 10, navigation 6, performance 3, debugging 9 (`lighthouse_audit`, `take_snapshot`, `take_screenshot`), memory 14 | `take_snapshot`: a11y-tree text with `uid`; no diff | `--slim`, `--headless`, `--isolated`; browser launched lazily | https://raw.githubusercontent.com/ChromeDevTools/chrome-devtools-mcp/main/docs/tool-reference.md |
| Appium MCP | 31 tools (about 28 default) | Page source XML inline or saved to file with XPath/pattern filter | n/a | https://github.com/appium/appium-mcp |
| Maestro MCP | See section 4 | View hierarchy as CSV | n/a | https://docs.maestro.dev/get-started/maestro-mcp |
| Figma MCP | 41 tools by my count of the listing (18 read, 14 write, 9 Weave) + 1 prompt | `get_metadata` XML outline; `get_screenshot` | n/a | https://developers.figma.com/docs/figma-mcp-server/tools-and-prompts/ |
| OpenAI Operator/CUA, macOS AX MCPs | Not researched (section 9) | | | |

Agent-ergonomics evidence:

- Anthropic's tool-writing guidance: consolidate multi-step workflows into fewer tools; offer a
  `response_format` of concise versus detailed (206 versus 72 tokens in their example); cap
  responses (Claude Code default 25,000 tokens) and, when truncating, tell the agent how to narrow
  the next call; return natural-language names rather than opaque ids where possible, but keep ids
  in detailed mode because later calls need them. https://www.anthropic.com/engineering/writing-tools-for-agents
- Cursor caps MCP tools at 40 per user per forum reports; whether the cap still applies could not be
  confirmed **[secondary]**. https://forum.cursor.com/t/tools-limited-to-40-total/67976
- Snapshot size: a tinyfish write-up reports about 114,000 tokens for a task in MCP mode versus
  27,000 in Playwright CLI mode **[secondary, not independently reproduced]**.
- A third-party Playwright MCP fork adds `--differential-snapshots`, `--max-snapshot-tokens`,
  `--no-snapshots`; its "99% reduction" claim is the fork's own **[secondary]**.
  https://git.supported.systems/MCP/playwright-mcp/src/branch/main/DIFFERENTIAL_SNAPSHOTS.md
- MCP spec revision 2026-07-28 is the current one. Each request carries
  `io.modelcontextprotocol/protocolVersion` in `_meta`; `server/discover` is a mandatory RPC; the
  `initialize` handshake is retained only for older revisions.
  https://modelcontextprotocol.io/specification/versioning
- Computer-use accuracy context: OSWorld human baseline about 72.36% on OSWorld-Verified; leaderboard
  figures disagree between sources **[secondary]**, so no agent score is quoted here.

### Decisions drawn from D

- Every competitor with a geometry-capable surface exposes it only as opt-in (`--snapshot-boxes`) or
  not at all. A compact outline with an optional `rect` per line is cheap and gives a model what a
  screenshot cannot.
- Tiering is established practice (`--caps`, `--slim`, `--tools`). R28a is conventional in form;
  its distinguishing feature is that every method is reachable from MCP.
- Only UFO² documents a non-takeover mechanism, and it is Windows-only and secondary-sourced.

## 6. E. Performance measurement in UI test tools

| Tool | Metrics | Statistics | Budget assertion | Source |
|---|---|---|---|---|
| Flutter `traceAction` + `TimelineSummary` | build and raster time average/worst, missed-budget counts, frame count, per-frame arrays | Mean, worst; percentiles not in the cookbook page | None: "the test doesn't assert on any metric"; requires `--profile` | https://docs.flutter.dev/cookbook/testing/integration/profiling |
| Android Macrobenchmark `FrameTimingMetric` | `frameDurationCpuMs`, `frameOverrunMs` (API 31+, positive = missed deadline) | P50, P90, P95, P99 over a journey; startup min/median/max | Not in the fetched page (CI page unreadable) | https://developer.android.com/topic/performance/benchmarking/macrobenchmark-metrics |
| Android JankStats | per-frame `isJank`, duration, optional CPU time and overrun; state tagging (`putState`) | Default jank = frame takes twice the refresh frame duration; multiplier adjustable | Library reports; app decides | https://developer.android.com/topic/performance/jankstats |
| XCTest hitch metrics | Hitch time ratio (ms of lateness per second) | Target at most 5 ms/s; 10 ms/s or more critical **[secondary, WWDC20 notes]** | XCTest baseline mechanism | https://wwdcnotes.com/notes/wwdc20/10077 |
| Web Vitals INP | Worst interaction (one outlier dropped per 50); split into input delay, processing, presentation delay | Good <= 200 ms, poor > 500 ms, at p75 of loads; lab results depend on which interactions the tester runs | Field-driven | https://web.dev/articles/inp |
| Chrome DevTools MCP | `performance_start_trace` / `stop` / `analyze_insight`; `lighthouse_audit` excludes performance | Insights | Not asserted | https://raw.githubusercontent.com/ChromeDevTools/chrome-devtools-mcp/main/docs/tool-reference.md |

### Decisions drawn from E

- R37's "input-to-present" equals INP's three phases summed. Decomposing it (input delay before the
  handler, processing, presentation delay) is already how the web explains where a slow interaction
  loses time.
- Define "dropped" and "janky" separately: dropped = missed the presentation deadline
  (`frameOverrun > 0`); janky = beyond a multiplier of the refresh interval (JankStats uses 2x).
- State tagging (JankStats `putState`) answers "which interaction owned the slow frame" and fits
  R37's marked span.
- Macrobenchmark reports a distribution per journey and repeats iterations; UI-test perf on real
  devices is noisy. Budgets should be evaluated over repeated iterations (median of per-run p95) and
  say how many.

## 7. F. Exploration and property-based UI testing

| Approach | Determinism | Oracle | Shrinking | Evidence |
|---|---|---|---|---|
| Android Monkey | Same `-s` seed gives the same sequence | Crash, ANR, security exception | None | https://developer.android.com/studio/test/other-testing-tools/monkey |
| Firebase Robo | Same actions in the same order per device config | Crashes; screenshots, video, crawl graph | None | https://firebase.google.com/docs/test-lab/android/robo-ux-test |
| Sapienz, Stoat, APE, Humanoid, DroidBot, Fastbot | Varies | Crashes, coverage | Sapienz minimises sequences; others not verified | https://arxiv.org/pdf/1901.02633 (Humanoid), https://cs.uwaterloo.ca/~cnsun/publication/icse19/icse19.pdf (APE) |
| Hypothesis stateful testing | Seeded; example database | User invariants | Shrinks on the generator's intermediate representation, not the value type | https://hypothesis.works/articles/rule-based-stateful-testing/, https://www.drmaciver.com/2015/05/notes-on-the-implementation-of-hypothesis-stateful-testing |
| Meticulous | Not researched | | | (section 9) |

Empirical caution: APE's authors report 62 unique crashes against Monkey 44, Sapienz 40, Stoat 31 on
15 Google Play apps in an hour; Humanoid's authors report 43.3% average line coverage, the highest
in their comparison; an industrial paper (MUBot) found Humanoid performed much worse on large
commercial apps than on open-source ones. These are mostly authors' own evaluations
**[secondary]**. A later independent comparison (APEX) found per-app variation in who wins. The
ranking of tools is not settled, which argues against promising coverage numbers.

### Decisions drawn from F

- Monkey's knobs are the proven ergonomic surface: seed, throttle, event-type mix, a fence on where
  exploration may go (`-p`, mapped by `stay_within`), and stop-on-crash versus continue.
- Shrinking should operate on an action list of locator-based steps (R24 traces already store
  locators), re-validating each candidate by replay, in the Hypothesis style of reducing the
  generator's choices rather than raw event streams.
- No surveyed UI tool checks R39's non-crash invariants (unique refs, parent/child agreement, finite
  bounds, no held input). Those are FLUI-specific and cheap because the agent is in the app.

## 8. Implications for the flui-driver spec

Legend: **keep** (evidence supports), **change** (evidence argues for editing), **add** (new
requirement or clause), **NEW** (no existing ID).

### Contradictions and errors found in the draft

1. **design.md "Lint" table, `low_contrast` large text.** It says large text is ">= 18.66 px, or
   >= 14 px bold". WCAG defines large text as 18 pt (24 CSS px) or 14 pt bold (about 18.66 CSS
   px); axe documents 24 CSS px and 19 CSS px for bold
   (https://dequeuniversity.com/rules/axe/html/4.10/color-contrast). 18.66 is the bold threshold,
   not the regular one; the draft's "14 px bold" is also wrong in logical px. Flutter uses 18 and
   bold 14 in logical px, which differs from WCAG's point conversion. **Change:** pick one, state
   the unit conversion (logical px treated as CSS px), and use 24 / 18.66 unless the spec records
   a deliberate Flutter-style departure. Also add the 0.01 ratio tolerance that Flutter and ATF both
   apply (4.49 should not fail where 4.5 is borderline-rounded; decide explicitly).
2. **R41 versus design.md on near-miss.** R41 says "within 1-4 px of each other but not equal";
   design.md fires on `0.5 < delta <= 4` with a 0.5 tolerance. Two different lower bounds. **Change:**
   state it once as `tolerance < delta <= 4`, with the tolerance shared with R40.
3. **R38 `min_target` single value.** Evidence shows 48 dp (Android; 32 at scroll edge/IME), 44
   (iOS), and 24 CSS px (WCAG 2.5.8, unverified here). **Change:** per-platform defaults plus the ATF
   exemptions, otherwise the audit produces the false positives ATF and Flutter both special-case.
4. **R14b Windows isolation.** "Windows Sandbox or a second interactive user session" is plausible
   but no source found confirms input injected in a Sandbox cannot reach the host, or that
   GPU-backed rendering works there. UFO²'s documented mechanism is an RDP-loopback virtual desktop
   **[secondary]**. **Change:** keep as a requirement but mark each Windows mode as "spike before
   committing" and add the loopback-session option.
5. **R22 update modes `failing` or `all`.** Playwright separates `missing` (create absent baselines)
   from `changed`; by default a run that only generated baselines still fails. **Add:** a `missing`
   mode, and make "baseline absent" a failure, not a pass, in CI (consistent with R44).

### Mapping by requirement

| # | Req | Action | Evidence and specifics |
|---|---|---|---|
| 1 | R40, R41, R42, D18 | **keep** | No surveyed tool documents exact ink/centre/gap checks outside Galen's DOM boxes; vision-only loops are documented as unreliable for subtle visuals (Lovable) and near/overlapping shapes (BlindTest). The premise is supported; a direct measurement of VLMs on near-miss defects was not found, so cite it as expectation and consider publishing a planted-defect fixture benchmark. |
| 2 | R40 | **add** | Galen precedent: every spatial check first requires the element to be visible; `inside partly`; optional per-check error px. Add a `visible` precondition and a "partly" mode for `to_be_within`. |
| 3 | R41 findings | **add** | A severity/outcome per finding: `error`, `warning`, `needs_review`, `suppressed` (ATF ERROR/WARNING/NOT_RUN/SUPPRESSED; axe incomplete). `needs_review` is where a `sampled` background or borderline contrast lands. A waived finding is reported as `suppressed`, never dropped (the draft already says this; align the name). |
| 4 | R41 rule set | **add** | From ATF/XCTest, rules FLUI can do exactly: duplicate interactive bounds (`DuplicateClickableBoundsCheck`), duplicate accessible names among siblings (`DuplicateSpeakableTextCheck`), redundant description, hit-region smaller than visible bounds (`hitRegion`), text clipped at the largest supported text scale (`dynamicType` + `textClipped`). The last is **NEW**: `audit.text_scale` re-lays out at e.g. 1.0/1.5/2.0 text scale and reports clipping, truncation and overlap; FLUI can do this with exact geometry where Apple/Android audits sample a screen. |
| 5 | R40/R41 contrast | **change** | Compute from the layer tree (keep). Mark `sampled` results `needs_review`. Apply the 0.01 tolerance explicitly (Flutter, ATF). Add non-text contrast (WCAG 1.4.11, 3:1) for control boundaries and focus indicators as an optional rule. Skip disabled controls, as Flutter and axe do, but list them in `not_checked`. |
| 6 | R38 target size | **change** | See contradiction 3. Exemptions: node at a scrollable edge, clickable ancestor, clipped by ancestor (ATF); merged/hidden/link/action-less nodes (Flutter). Report which exemption applied. |
| 7 | R44 | **keep, extend** | ATF `NOT_RUN`/"could not get" results and axe "incomplete" are the only precedents. Extend `not_checked` reasons to include: scrolled-out content (XCTest audits only inspect visible elements **[secondary]**), disabled nodes, image backgrounds, windows not requested. An audit should name its scope ("visible viewport only"). |
| 8 | R42 | **keep, add** | Empty slot in the market. Add aria-snapshot-style partial templates: children modes `contain`, `equal`, `deep-equal`; regex on names; and an update-mode set `missing`/`changed`/`all`/`none` (Playwright). Default tolerance: keep 0.5 px; colour ΔE as drafted. |
| 9 | R22 | **change** | State defaults: Playwright 0.2 (pixelmatch YIQ), Chromatic .063, anti-alias handling on/off. Adopt "ignore anti-aliased pixels by default, opt in to strict" (Chromatic `diffIncludeAntiAliasing`). Add `max_diff_pixel_ratio` next to the pixel-count budget. Keep per-backend baselines (Playwright suffixes by browser and platform). |
| 10 | R43 | **keep, change** | Per BlindTest, perception fails on close shapes. Annotation must print measured numbers as text and the driver should produce a zoomed crop per finding (computer-use `zoom` exists for the same reason). |
| 11 | R16 | **keep, add** | Delta default is not in official Playwright MCP, Chrome DevTools MCP or Appium MCP docs. Add: optional `rect` per outline line (Playwright `--snapshot-boxes` equivalent), a `max_tokens` budget with an explicit truncation message that says how to narrow (Anthropic guidance, 25k cap), and a `find` tool cheaper than a full snapshot (Playwright `browser_find`). |
| 12 | R15 | **add** | `truncated` results should carry a hint (which subtree, which locator or depth to request next). |
| 13 | R28a | **keep, add** | Core 13 versus 17-72 elsewhere. Add `--tools`/`--exclude-tools` style filtering (Windows-MCP), and a test that measures the listed schemas' token size, since the whole point is context cost. Consider a `response_format: concise|detailed` parameter on reads (Anthropic: 72 versus 206 tokens). |
| 14 | R29 | **change** | Name the revision's mechanics: per-request `protocolVersion` in `_meta` and mandatory `server/discover`; "handles never session state in the connection" already matches the stateless direction. Verify the 2026-07-28 `isError`/schema rules against the spec text; only the versioning page was read here. |
| 15 | R10, R14a, R14b, D11 | **keep** | Market shows interference as an accepted hazard (Windows-MCP) or an unsubstantiated claim (terminator). Anthropic's advice is a VM. UFO²'s PiP is the one documented in-session mechanism. Add the loopback virtual-desktop mode as a Windows candidate (contradiction 4) and keep `synthetic` default. |
| 16 | R14a | **change** | Windows-MCP concedes its user-takeover is "best-effort, not a security boundary" and that already-started commands may continue. State the same honest limit for FLUI's activity guard (the draft's Risks already says input can be missed; promote it into R14a). |
| 17 | R7 | **add** | On `not_found`/`ambiguous`, list nearest candidates by role, name distance and identifier, never auto-substitute. Self-healing's documented failure is a wrong match; a human-confirmed suggestion keeps the benefit. |
| 18 | R6 | **keep** | Matches Appium MCP's stated locator ordering and Playwright's role/name model. |
| 19 | R18, R19 | **keep** | Detox's gray-box synchronization is the established fix for flake; FLUI's frame timeline is stronger because the framework is the app. Keep "why not settled" and align its output with Detox's busy-resource report. |
| 20 | R37 | **change, add** | Define `dropped` (missed deadline) and `janky` (frame > multiplier x refresh interval, JankStats default 2x) separately. Report p50/p90/p95/p99 and worst (Macrobenchmark). Split input-to-present into input delay, processing, presentation delay (INP). Accept tagged states on a span (JankStats `putState`) so the worst frame names the interaction. |
| 21 | R37a | **keep, add** | Flutter asserts nothing; this is a gap FLUI fills. Add repeated iterations with a stated count and a median-of-p95 rule, since real-device numbers are noisy. Budget keys per target stay. Hitch ratio (ms of lateness per second, 5 ms/s target) is a cheap normalised metric to offer **[secondary]**. |
| 22 | R39 | **keep, add** | Add Monkey-style knobs: event-mix weights, `throttle`, continue-after-failure mode, and the existing `stay_within` fence. Shrink the locator-level action list (Hypothesis-style reduction), not raw coordinates. Do not promise coverage; the literature ranking is contested. |
| 23 | R38 | **add** | Align finding kinds with `labeledTapTargetGuideline` (actionable node with empty label and tooltip) and ATF `TraversalOrderCheck` (cycles, over-constrained order). R38's focus-order rule can cite cycles and unreachable nodes in those terms. |
| 24 | R45 | **keep** | No tool found ties a platform-support matrix to run evidence; novel. Do not depend on Firebase Test Lab (deprecated, shutdown 2027-09-30). |
| 25 | R17, R32 (Android/iOS) | **keep** | Patrol shows native automation via UIAutomator/XCUITest is accepted practice but is Android/iOS only; FLUI's cross-check on desktop OS trees (R17) is additional. |
| 26 | `vision` group (R28a) | **change** | Treat any model-judged assertion as advisory only (Maestro defaults `optional: true`); a `vision` tool cannot contribute to `pass` or `fail`, only to evidence. State this in R44. |
| 27 | R12/R32a BiDi | **unchanged** | The claim that Chrome interpolates multi-step moves could not be checked against the BiDi spec text in this pass (section 9); leave R32a as the owner's recorded observation. |
| 28 | R22/R42 CI | **add** | Argos and Playwright both identify environment drift (fonts, GPU path) as the main flake source. D4's bundled fonts address it; add a report line recording renderer, font set and scale factor with every baseline comparison. |

## 9. Could not verify

- **UI-Lens paper details**: PDF returned HTTP 403; numbers (F1 20.36% / 31.21%) and category list
  come from a search summary of the CVPR abstract; dataset size differs between sources.
- **Apple docs**: `performAccessibilityAudit` and `XCUIAccessibilityAuditType` pages did not render;
  the audit-type list and handler semantics come from WWDC23 notes and secondary guides. Hitch ratio
  thresholds are from WWDC20 notes. XCTest baseline mechanics and scroll-deceleration metrics not
  checked.
- **WCAG SC 2.5.8 (24 CSS px) and 2.5.5 (44 px)** text: the fetch truncated before those sections.
  The 24 px figure is my knowledge, not a fetched fact.
- **WebDriver BiDi `input.performActions`** body (pointer types, `duration` interpolation) was not
  reached; only the module table of contents.
- **Compose layout assertions** (`assertPositionInRootIsEqualTo`, touch-target assertions,
  `enableAccessibilityChecks`): the relevant page returned 404; only finders, assertions and
  `printToLog` were confirmed.
- **Documented verification loops not found**: Figma Make/AI, pen.dev (beyond its MCP tool list;
  note pen.dev/pencil.dev is distinct from Penpot), v0, Bolt (only secondary), Stitch (only
  secondary), Uizard, Galileo, Builder.io Visual Copilot, Anima, Locofy.
- **Not researched**: Percy, BackstopJS, Loki, reg-suit, Happo, Lost Pixel, Meticulous, SwiftUI
  snapshot internals beyond swift-snapshot-testing's strategies, Cypress, TestCafe, Selenium 4
  BiDi, WebdriverIO, Robot Framework, mabl, testRigor, Functionize, Sauce Labs, BrowserStack,
  LambdaTest (pricing and positioning omitted), OpenAI Operator/CUA, macOS AX MCP servers, UI
  Automator, Lighthouse internals, Chrome tracing, Xcode scroll-deceleration metrics.
- **Maestro**: the docs index did not state selector rules, waiting policy or flakiness policy.
- **ATF numeric contrast thresholds**: source constants for text-contrast ratios were not extracted;
  only the 0.01 tolerance, touch target sizes and text-size constants were read from source. The
  `TextSizeCheck` constants are read as dp/sp by assumption.
- **Galen**: the `~` tolerance is undefined in the docs, and measurement source (DOM boxes) is
  inferred; maintenance status not checked.
- **Windows Sandbox / second user session** as input-isolating modes: no source found either way.
- **UFO²** PiP details come from the paper's abstract page and summaries; README did not mention it.
- **terminator** "background" claim has no mechanism in the README.
- **Playwright MCP** diff mode: absent from official README as fetched; fork claims are unverified.
  The README's `--caps` values and tool groups are not fully reconciled in the doc itself.
- **Cursor 40-tool limit**: forum reports only; current behavior unconfirmed.
- **Token-cost claims** (tinyfish 114k vs 27k) are a single secondary source.
- **OSWorld scores**: leaderboard sources disagree; not used.
- **Meaning of Android Accessibility Scanner "duplicate bounds"**: not found in the scanner docs
  (it is an ATF check).
- **MCP 2026-07-28 tool-result rules** (`isError`, schemas, hints): only the versioning page was
  read.
