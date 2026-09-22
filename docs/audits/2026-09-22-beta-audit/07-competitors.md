# Competitive Landscape: Rust GUI Frameworks vs. Flutter (2026)

*Compiled 2026-09-22 for FLUI (wgpu renderer, View→Element→Render three-tree, Material+Cupertino catalogs, CLI, dlopen hot reload, AccessKit a11y).*

---

## 1. Framework Profiles

### Flutter (reference baseline)
- **Latest stable**: Flutter 3.44 (Google I/O 2026 wave), with DevTools at 2.28.5 and ongoing point releases; Dart 3.12. Cadence: ~quarterly stable, nightly/beta channels continuously. [State of Flutter 2026](https://devnewsletter.com/p/state-of-flutter-2026/), [Flutter CHANGELOG](https://github.com/flutter/flutter/blob/stable/CHANGELOG.md), [DevTools release notes](https://docs.flutter.dev/tools/devtools/release-notes)
- **Rendering**: Impeller is now the default and *only* backend on Android 10+ (Skia removed for that path); Impeller also default on iOS/desktop. [FlutterSolution: Impeller/build modes](https://www.fluttersolution.com/2026/09/flutter-prep-14-impeller-build-modes.html)
- **Hot reload**: Stateful hot reload now works on **web** too (parity with mobile/desktop) — a major 2026 milestone; plus "Agentic Hot Reload" wired to MCP-compatible coding agents (Claude Code, Gemini CLI, Cursor, Copilot Workspace). [Hot reload docs](https://docs.flutter.dev/tools/hot-reload), [devnewsletter](https://devnewsletter.com/p/state-of-flutter-2026/)
- **DevTools**: Mature inspector, timeline, memory/CPU profiler, layout explorer, and a widget that visualizes how Impeller batches draw calls.
- **Widget/design catalog**: Material 3 + Cupertino fully built out, extensive form widgets, `Slivers`, virtualized lists (`ListView.builder`), routing (`Navigator`/`go_router`), rich `TextField`/IME support, i18n.
- **Weaknesses**: Two-language split when embedding native Rust/C++ (FFI complexity cited as a reason developers leave Flutter — see §4), Dart VM/AOT size, Skia/Impeller platform quirks, plugin ecosystem fragmentation on desktop.

### iced
- **Version/cadence**: Actively maintained monorepo, ~31.5k GitHub stars, last push within days of writing; releases roughly every few months. [iced GitHub](https://github.com/iced-rs/iced), [star-history](https://www.star-history.com/iced-rs/iced/)
- **Platforms**: Desktop (Win/macOS/Linux) + Web (wasm) + experimental mobile.
- **Rendering**: `iced_wgpu` is the default GPU renderer; also has a tiny-skia software path. [iced_wgpu README](https://github.com/iced-rs/iced/blob/master/wgpu/README.md)
- **Layout**: Flexbox-like custom layout (not Taffy).
- **Architecture**: Elm/TEA — pure `update(Message) -> Command`, `view() -> Element`. Very type-safe, but boilerplate-heavy for large apps.
- **Accessibility**: AccessKit integration exists but is less complete than egui/Vizia's.
- **Hot reload**: None built-in.
- **Docs**: Improving but described as having "gaps" by community reviewers. [Wren Learns Rust](https://wrenlearnsrust.com/posts/2026-03-11-rust-gui-landscape-2026.html)
- **Weaknesses**: No design-system catalog (Material/Cupertino), state management purely message-passing (verbose for deep component trees), text-editing depth behind Xilem/Slint.

### egui / eframe
- **Version/cadence**: eframe 0.36.x on crates.io as of Sept 2026; ~30.5k stars; frequent (near-monthly) releases. [eframe crates.io](https://crates.io/crates/eframe), [egui releases](https://github.com/emilk/egui/releases), [star-history](https://www.star-history.com/emilk/egui/)
- **Platforms**: Desktop, web (wasm/webgl), limited mobile.
- **Rendering**: Immediate-mode canvas renderer (glow/wgpu backends).
- **Accessibility**: AccessKit integration is optional but enabled by default; historically weak on web (no AccessKit web backend as of 2024, improving since). [egui PR on servo](https://github.com/servo/servo/pull/42402)
- **Hot reload**: No real hot reload; feature requests for widget "preview" tooling remain open. [egui #5561](https://github.com/emilk/egui/issues/5561)
- **Notable 2026 addition**: A new **agent-inspection protocol** — `EGUI_INSPECTION=1` exposes the AccessKit tree plus event injection over MCP (`egui_mcp`) so AI agents can drive/inspect a running egui app. This is a direct precedent for "AI-agent-driven testing/inspection" as a differentiator. [eframe CHANGELOG](https://github.com/emilk/egui/blob/main/crates/eframe/CHANGELOG.md)
- **Strengths**: Fastest path from zero to a window ("15 lines of code"); simplest mental model in the ecosystem.
- **Weaknesses**: Immediate-mode look ("looks like a debug UI" per multiple reviewers), no design-system catalog, weaker on rich text editing/virtualized lists than retained-mode competitors, styling is manual.

### Slint
- **Version/cadence**: Slint 1.17 shipped June 24, 2026 (semver-stable 1.x line, no breaking changes since 1.0); ~23.8k stars. [Extenly: Slint 1.17](https://extenly.com/2026/07/09/slint-1-17-whats-new-in-the-modern-ui-toolkit/), [star-history](https://www.star-history.com/slint-ui/slint/)
- **Platforms**: Desktop, embedded/no_std, web; multi-language (Rust, C++, JS, Python).
- **Rendering**: Custom Skia-like software + GPU renderers tuned for embedded.
- **Tooling — best-in-class**: `.slint` DSL with LSP, VS Code "Show Preview" live-reloads any unsaved edit; a **Design Mode** lets you drag/drop and reposition widgets with visual drop zones, writing changes back to source; browser-based SlintPad; and (new in 1.17) **live preview on a physical phone/tablet**. This is the most mature visual-design tooling story in the whole Rust GUI space. [DeepWiki: Dev Tools](https://deepwiki.com/slint-ui/slint/7-development-tools)
- **Weaknesses**: Separate DSL (not idiomatic Rust-only view code) is a barrier for teams wanting pure-Rust; ecosystem/plugin catalog smaller than Flutter's; commercial licensing tiers for some use cases.

### Dioxus (incl. Dioxus Native / Blitz)
- **Version/cadence**: Dioxus 0.7 released 2025→2026, ~39.1k stars — highest of any Rust GUI project surveyed. [Dioxus 0.7 blog](https://dioxuslabs.com/blog/release-070/), [star-history](https://www.star-history.com/dioxuslabs/dioxus/)
- **Platforms**: Web (wasm), desktop (webview via Tauri-like wry, and now native), mobile (iOS/Android).
- **Headline 2026 feature — "Subsecond" hot-patching**: True Rust hot-patching (binary/incremental linking) that preserves app state across edits, working on **web, desktop (mac/Linux/Windows), and mobile (iOS/Android)** — the most complete hot-reload story of any framework in this survey, native or web-based. [Dioxus 0.7 release notes](https://github.com/DioxusLabs/dioxus/releases/tag/v0.7.0)
- **Dioxus Native / Blitz**: Blitz is a modular HTML/CSS rendering engine (cross-pollinated from Firefox/Servo/Bevy work) that paints entirely on GPU via wgpu — Dioxus's answer to "get off the webview." [Blitz GitHub](https://github.com/DioxusLabs/blitz)
- **Accessibility**: Free via webview mode (Windows Narrator etc. work out of the box) when using the webview renderer; native/Blitz renderer accessibility is younger.
- **State management**: React-like hooks/signals; familiar to web devs, closest mental model to component frameworks (not Flutter's).
- **Weaknesses**: Native (Blitz) renderer materially less mature than the webview renderer; CSS-based styling model, not a Flutter-style widget tree; no Material/Cupertino-equivalent catalog out of the box (relies on CSS/Tailwind-like styling).

### Xilem / Masonry (Linebender)
- **Status**: Explicitly experimental/alpha — Masonry "is alpha-quality software with plenty of missing features," Xilem ~5.5k stars. [GitHub xilem](https://github.com/linebender/xilem), search result on stars
- **Architecture**: Reactive, closest in spirit to a "typed Flutter" (declarative tree diffing) among Rust frameworks; team is ex-Druid (Druid was Rust's earlier "official" GUI bet, now sunset).
- **Rendering**: Vello (GPU vector renderer) with a new Vello Hybrid / Vello CPU path so Masonry can run without a GPU; recent "Glifo" (renamed from parley_draw) handles font/text rendering, moved into the Vello repo. [Linebender tmil-25](https://linebender.org/blog/tmil-25), [tmil-24](https://linebender.org/blog/tmil-24)
- **Text**: Parley (typed, shaping-aware text layout) — one of the most sophisticated text stacks in the ecosystem, purpose-built rather than bolted on.
- **Layout**: New custom layout system added to Masonry in 2026 (not Taffy).
- **Widgets**: 2025–2026 additions include Svg, Divider, CollapsePanel, StepInput, RadioButtons, Switch, Clip, Split — still a fairly bare widget set vs. Material/Cupertino.
- **Weaknesses**: Not production-ready by team's own admission; no hot reload; small ecosystem; steep churn (APIs still moving).

### GPUI (Zed)
- **Status**: Zed editor itself is hugely popular (~85k+ stars) and GPUI is its internal UI framework, open-sourced but coupled to Zed's release cycle; Zed stable 1.15.0 (Aug 12, 2026). [Zed on Wikipedia](https://en.wikipedia.org/wiki/Zed_(text_editor))
- **Rendering**: Originally custom (Blade), migrated toward wgpu in 2026 for broader backend support (`gpui_wgpu`), including a path to run GPUI natively on iOS/Android via wgpu. [HN: Zed switching to wgpu](https://news.ycombinator.com/item?id=47002825)
- **Accessibility**: AccessKit support is planned/in progress, not yet complete; a community fork "GPUI-CE" (Community Edition) ships flexbox-style layout, text shaping, animation primitives and an accessibility tree independent of Zed's release train. [gpui.rs](https://www.gpui.rs/), [GPUI-CE](https://gpui-ce.github.io/)
- **Weaknesses**: Tied to Zed's roadmap and internal needs rather than being a general-purpose app framework; no first-party CLI/scaffolding for non-editor apps; contributions must stay in sync with the main Zed repo.

### Makepad
- **Status**: ~4.2k stars, "1.0" milestone discussed on HN in 2025; positions itself as an "AI-accelerated" cross-platform framework/IDE. [HN: Makepad 1.0](https://news.ycombinator.com/item?id=43971829)
- **Rendering**: Fully custom GPU pipeline with its own shader DSL (MPSL) that compiles to Metal/HLSL/GLSL — unique in the survey for owning the shader layer end-to-end.
- **Live tooling**: "Live styling" reflects UI DSL edits immediately without recompilation — a hot-reload-adjacent feature built into the "Live" system.
- **2026 differentiator**: Makepad Studio lets an AI agent generate Rust UI code, run the app, inspect visual results via screenshots, and interact with the widget tree autonomously — an explicit agent-driven-development pitch. [Makepad Book](https://makepad.rs/)
- **Weaknesses**: Smallest ecosystem/stars of the actively-developed frameworks surveyed; custom shader DSL raises the learning curve; sparse third-party widget/plugin catalog.

### Freya (Dioxus-style API + Skia)
- **Status**: Cross-platform, Skia-based, historically built on Dioxus core crates (0.1–0.3) but as of 0.4 has its **own reactive core** (partially Dioxus-inspired). Undergoing a large rewrite in 2026 — "main branch differs a lot from the latest stable release." [Freya announcement](https://freyaui.dev/posts/announcement), [Freya 0.2 post](https://freyaui.dev/posts/0.2)
- **Built-ins**: Button/scroll/switch components, animation & text-editing hooks, and dev tools (tree inspector, FPS overlay) — a notably practical starter kit.
- **Weaknesses**: In-flux core (rewrite risk), small community relative to Dioxus itself, Skia CPU/GPU story less mature than Flutter's Impeller.

### Floem (Lapce)
- **Status**: ~4.2k stars, built by the Lapce editor team.
- **Architecture**: Fine-grained reactive signals (inspired by `leptos_reactive`) rather than Elm/vdom-diffing; view tree built once, updated in place.
- **Rendering**: Multiple backends — GPU via Vger/Vello/Skia, CPU via tiny-skia.
- **Layout**: Taffy (Flexbox/Grid) — matches CSS-familiar developers.
- **Platforms**: Windows/macOS/Linux/wasm.
- **Weaknesses**: No mobile; smaller widget catalog; primarily driven by Lapce's own needs, so breadth outside code-editor-style UI is thinner.

### Vizia
- **Status**: ~2.2k stars, active (last update Aug 21, 2026), pure-Rust declarative/reactive, no DSL/macros required.
- **Widget catalog**: 25+ ready-made views, two built-in themes (light/dark), 4,250+ SVG icons (Tabler Icons) bundled — unusually generous icon set for a small project.
- **Accessibility**: AccessKit-powered, screen-reader support built in.
- **Rendering**: Skia, with dirty-region optimization to only redraw what changed.
- **Weaknesses**: Desktop-only (no mobile/web claimed), smallest community of the actively developed retained-mode frameworks, no hot reload, no visual design tool.

### Tauri 2 (alternative architecture, not a native-UI competitor per se)
- **Status**: Stable v2.10.1 (March 4, 2026); frontend is web tech (React/Vue/Svelte/HTML) with a Rust backend; OS-native webview (WebView2/WebKit/WebKitGTK) rather than bundling Chromium. Binaries ~95% smaller than Electron, ~3.7x faster startup, ~75% lower idle memory. [Tauri v2 tutorial](https://rustify.rs/articles/rust-tauri-v2-desktop-app-tutorial-2026), [Tech Insider Tauri vs Electron](https://tech-insider.org/tauri-vs-electron-2026/)
- **Relevance to FLUI**: Represents the "just use the webview" alternative that Dioxus desktop also leans on; accessibility and text-input/IME come largely free from the OS webview — a bar native-rendered Rust UIs (FLUI included) must still clear deliberately.
- **Mobile**: iOS/Android added in Tauri 2.0.

### Notable 2025–2026 entrants
- **Blinc** — very new framework, first version released early 2026 (limited public detail yet).
- **Ply** — introduced March 1, 2026.
- Both are too new for meaningful star/maturity comparison but confirm the space is still attracting fresh general-purpose Rust GUI attempts. [libs.tech GUI frameworks](https://libs.tech/rust/gui-frameworks)
- **COSMIC desktop** (System76, uses iced) shipped as Pop!_OS 24.04's default DE (Dec 2025) — a production-scale validation of iced at desktop-environment scale. [COSMIC desktop](https://en.wikipedia.org/wiki/COSMIC_desktop)

---

## 2. Feature Matrix

| Framework | Latest ver. (2026) | Stars (~) | Platforms | Render | Layout | Text | A11y (AccessKit) | Hot reload | Design catalog | State model |
|---|---|---|---|---|---|---|---|---|---|---|
| **Flutter** | 3.44 | n/a (not Rust) | Mob/Desk/Web | Impeller (Skia gone on Android) | Flutter box/sliver | Custom (Skia text) | Native a11y trees | Stateful, incl. web + agentic | Material 3 + Cupertino, full | Widget tree + many (Provider/Riverpod/Bloc) |
| **iced** | rolling | 31.5k | Desk, Web(wasm) | wgpu / tiny-skia | Custom flex | Custom | Partial | None | None | Elm/TEA |
| **egui/eframe** | eframe 0.36.x | 30.5k | Desk, Web | glow/wgpu, immediate | Custom | Custom | Default-on, web gaps closing | None (agent-inspection protocol instead) | None | Immediate-mode, manual |
| **Slint** | 1.17 | 23.8k | Desk, Embedded, Web | Custom SW/GPU | Custom | Custom | Yes | Best-in-class live preview + Design Mode | Fluent/Material-ish widget styles | Declarative DSL + Rust binding |
| **Dioxus** | 0.7 | 39.1k | Web/Desk(webview+native)/Mobile | Webview or Blitz(wgpu) | CSS/Taffy-like | Browser or Blitz text | Free via webview; native younger | Best-in-class (Subsecond, all platforms, state-preserving) | CSS/Tailwind-style, no native Material/Cupertino | React-like hooks/signals |
| **Xilem/Masonry** | alpha | 5.5k | Desk (Linux/Mac/Win) | Vello/Vello CPU | Custom (new 2026) | Parley/Glifo (strong) | In progress | None | Sparse widget set | Reactive tree diffing |
| **GPUI (Zed)** | tied to Zed 1.15 | Zed ~85k | Desk (mobile via wgpu emerging) | Blade→wgpu | Flexbox-like | Custom | Planned/partial (GPUI-CE has it) | None | None (editor-focused) | Entity/model system |
| **Makepad** | ~1.0 era | 4.2k | Desk/Web(wasm)/Mobile | Custom GPU + MPSL shaders | Custom | Custom | Limited | Live styling (DSL-level) | Sparse | Live DSL + Rust |
| **Freya** | 0.4.x (rewrite) | small | Desk | Skia | Custom | Custom | Limited | None | Basic components | Own reactive core |
| **Floem** | rolling | 4.2k | Desk, Web(wasm) | Vger/Vello/Skia/tiny-skia | Taffy | Custom | Limited | None | Sparse | Fine-grained signals |
| **Vizia** | rolling | 2.2k | Desk only | Skia | Custom (morphorm) | Custom | Yes | None | 25+ views, 2 themes, 4250+ icons | Declarative reactive, no macros |
| **Tauri 2** | 2.10.1 | large (framework, not UI) | Desk+Mobile | OS webview | HTML/CSS | Browser | Free (native a11y via webview) | Standard web hot reload | Whatever web framework brings | Any JS framework |

---

## 3. Table Stakes for a 2026 Rust UI Beta

Based on the above, users evaluating *any* new Rust UI framework at "beta" in 2026 now expect, at minimum:

1. **Text input with IME support** — CJK/emoji composition, not just ASCII (Dioxus gets this free via webview; native frameworks must build it explicitly).
2. **Virtualized/lazy lists** — smooth scrolling over large data sets without full materialization (table stakes since egui/iced/Flutter all handle it, at least partially).
3. **Routing / navigation stack** — declarative navigation, deep links, back-stack (Flutter's Navigator/go_router is the bar; most Rust frameworks are weak here).
4. **Theming with light/dark mode** — a switchable theme system, not hardcoded colors (Vizia and Slint ship this out of the box).
5. **Accessibility via AccessKit (or equivalent)** — screen reader support is now assumed, not optional (egui, Vizia, Slint have it; GPUI and Xilem are catching up).
6. **Some hot-reload or fast-iteration story** — Dioxus's Subsecond and Slint's live preview have reset expectations; "restart to see a style change" now reads as dated.
7. **A real docs site with live/runnable examples** — Slint's SlintPad and Flutter's DartPad-style docs set the bar; scattered rustdoc alone is not enough.
8. **Published on crates.io with semver discipline** — expected baseline for any serious adoption; Slint's "no breaking changes since 1.0" is the gold standard cited approvingly.
9. **A wasm/web demo runnable in-browser** — iced, egui, Dioxus, Floem, Makepad all have this; it is now the default way people first "try" a Rust GUI framework.
10. **Working Windows + Linux + macOS parity** — reviewers repeatedly flag "Windows support varies wildly" across the ecosystem as a trust-breaker.

---

## 4. Where FLUI's Flutter-like Architecture Can Differentiate — and Where Competitors Lead

### FLUI's potential differentiators
- **True Flutter parity for Flutter developers**: a View→Element→Render three-tree, Material 3 + Cupertino catalogs, and slivers are something *no* surveyed Rust framework offers together. Dioxus is closest in ergonomics (declarative, hooks) but is CSS-styled, not widget-styled; Xilem is closest in architecture (reactive diffing) but has no widget catalog and is alpha.
- **Sliver protocol**: Flutter's sliver-based scrolling (CustomScrollView, nested scroll coordination) has no real equivalent in any Rust framework surveyed — a genuine gap FLUI could own.
- **AI-agent-driven testing/inspection**: egui's `EGUI_INSPECTION`/`egui_mcp` and Makepad Studio's agent-driven UI generation show this is now an active investment area, not a novelty. FLUI's AccessKit tree plus a similar inspection/control protocol would put it in the same tier as the two frameworks currently leading here, and ahead of iced/Slint/Vizia/GPUI which lack it.
- **A single coherent design system out of the box**: Only Slint (fluent-ish) and Vizia (basic 2-theme) ship anything resembling a default design system; none ship Material *and* Cupertino. This is an open lane.
- **dlopen-based hot reload with state preservation**: matches the ambition of Dioxus's Subsecond (binary patching) but for a native-rendered, non-webview framework — if it works reliably, it would be unique among GPU-native Rust frameworks (iced, Xilem, GPUI, Vizia, Makepad all lack this).

### Where competitors are clearly ahead (bars FLUI must clear, not just match on paper)
- **Dioxus**: hot-patching (Subsecond) already ships across web/desktop/mobile with state preservation — the most proven implementation of the exact capability FLUI is betting on.
- **egui**: unmatched simplicity/time-to-first-window, and now a working agent-inspection protocol in production use.
- **Slint**: by far the most mature *visual* tooling — live preview, Design Mode drag-and-drop, SlintPad, phone-based live preview — a bar for "docs site with live examples" and design tooling.
- **Xilem/Masonry (Parley/Glifo)**: the most sophisticated dedicated text-shaping stack in the ecosystem; FLUI's text stack will be judged against this even though Xilem itself is alpha.
- **Vizia**: pragmatic accessibility + theming + icon set bundled with zero DSL — a lower-friction "batteries included" bar for polish.
- **Tauri/Dioxus-webview**: text input, IME, and OS-native accessibility come essentially free via webview; FLUI must deliberately build all of this to reach parity, and any gap here (IME bugs, missing screen-reader labels) will be an immediate, visible regression versus the webview alternatives.

---

## 5. Top 10 Flutter-Developer Pain Points with Rust GUI (2025–2026, from blogs/forums/HN)

1. **Two-language FFI overhead is the #1 reason people leave Flutter+Rust hybrids entirely** — one blogger described dropping the Flutter/Rust split (FFI complexity, API design overhead) and rewriting the whole UI in egui over a weekend rather than keep maintaining the boundary. [Why I Switched from Flutter+Rust to egui](https://jdiaz97.github.io/greenblog/posts/flutter_to_egui/)
2. **No single "obvious" framework to pick** — reviewers repeatedly note Rust GUI "still lacks a single, mature 'pure Rust' standard," unlike Flutter's one-true-way. [Are We GUI Yet](https://areweguiyet.com/)
3. **Steep mental-model tax** — TEA/Elm (iced) vs immediate-mode (egui) vs reactive signals (Floem) vs React-like hooks (Dioxus) are all different enough that switching frameworks means relearning state management from scratch, unlike Flutter's single widget model.
4. **Immediate-mode frameworks "look like a debug UI" out of the box** — egui's default look is repeatedly flagged as unpolished/prototype-grade compared to Material/Cupertino. [Wren Learns Rust](https://wrenlearnsrust.com/posts/2026-03-11-rust-gui-landscape-2026.html)
5. **Windows support varies wildly** across frameworks — a recurring, explicit complaint that undermines "cross-platform" claims Flutter developers take for granted.
6. **No design-system parity** — none of the native-rendered Rust frameworks ship a Material *and* Cupertino catalog; developers coming from Flutter must rebuild theming, forms, and adaptive widgets from scratch.
7. **Hot reload absence or immaturity** outside Dioxus/Slint — Flutter developers accustomed to sub-second stateful reload find most native Rust frameworks (iced, Xilem, GPUI, Vizia, Makepad) require full recompiles, a productivity regression called out repeatedly.
8. **Documentation gaps and framework churn** — iced's docs are described as having "gaps"; Xilem/Masonry API surface is still actively moving (alpha), meaning tutorials go stale quickly compared to Flutter's stable, versioned docs.
9. **Accessibility is inconsistent/bolted-on** — several frameworks (Xilem, GPUI, Makepad, Freya) have partial or in-progress AccessKit integration, versus Flutter's mature native a11y trees on every platform.
10. **Weak or missing routing/navigation and sliver-like scrolling primitives** — Flutter developers expect `Navigator`/nested scroll coordination out of the box; no surveyed Rust framework offers an equivalent, forcing hand-rolled navigation stacks.

---

## Sources
- [iced GitHub](https://github.com/iced-rs/iced) · [iced_wgpu README](https://github.com/iced-rs/iced/blob/master/wgpu/README.md) · [iced star-history](https://www.star-history.com/iced-rs/iced/) · [SE Radio 713](https://se-radio.net/2026/03/se-radio-713-hector-ramon-jimenez-on-building-a-gui-library-in-rust/)
- [eframe crates.io](https://crates.io/crates/eframe) · [egui releases](https://github.com/emilk/egui/releases) · [egui CHANGELOG](https://github.com/emilk/egui/blob/main/crates/eframe/CHANGELOG.md) · [egui #5561 hot reload issue](https://github.com/emilk/egui/issues/5561) · [servo PR on AccessKit](https://github.com/servo/servo/pull/42402) · [egui star-history](https://www.star-history.com/emilk/egui/)
- [Slint homepage](https://slint.dev/) · [Slint 1.17 writeup](https://extenly.com/2026/07/09/slint-1-17-whats-new-in-the-modern-ui-toolkit/) · [Slint DeepWiki Dev Tools](https://deepwiki.com/slint-ui/slint/7-development-tools) · [Slint star-history](https://www.star-history.com/slint-ui/slint/)
- [Dioxus 0.7 blog](https://dioxuslabs.com/blog/release-070/) · [Dioxus 0.7 release](https://github.com/DioxusLabs/dioxus/releases/tag/v0.7.0) · [Blitz GitHub](https://github.com/DioxusLabs/blitz) · [Dioxus star-history](https://www.star-history.com/dioxuslabs/dioxus/)
- [Linebender tmil-25 (2026 Q1)](https://linebender.org/blog/tmil-25) · [tmil-24 (Dec 2025)](https://linebender.org/blog/tmil-24) · [Xilem GitHub](https://github.com/linebender/xilem)
- [Zed on Wikipedia](https://en.wikipedia.org/wiki/Zed_(text_editor)) · [gpui.rs](https://www.gpui.rs/) · [GPUI-CE](https://gpui-ce.github.io/) · [HN: Zed switching wgpu](https://news.ycombinator.com/item?id=47002825)
- [Makepad Book](https://makepad.rs/) · [HN: Makepad 1.0](https://news.ycombinator.com/item?id=43971829)
- [Freya announcement](https://freyaui.dev/posts/announcement) · [Freya 0.2](https://freyaui.dev/posts/0.2)
- [Floem docs](https://docs.floem.dev/) · [Floem GitHub](https://github.com/lapce/floem)
- [Vizia GitHub](https://github.com/vizia/vizia) · [Vizia site](https://vizia.github.io/vizia-site/)
- [Tauri v2 tutorial 2026](https://rustify.rs/articles/rust-tauri-v2-desktop-app-tutorial-2026) · [Tauri vs Electron 2026](https://tech-insider.org/tauri-vs-electron-2026/)
- [Flutter CHANGELOG](https://github.com/flutter/flutter/blob/stable/CHANGELOG.md) · [State of Flutter 2026](https://devnewsletter.com/p/state-of-flutter-2026/) · [Hot reload docs](https://docs.flutter.dev/tools/hot-reload) · [DevTools release notes](https://docs.flutter.dev/tools/devtools/release-notes) · [FlutterSolution Impeller](https://www.fluttersolution.com/2026/09/flutter-prep-14-impeller-build-modes.html)
- [Are We GUI Yet](https://areweguiyet.com/) · [Wren Learns Rust: Rust GUI Landscape 2026](https://wrenlearnsrust.com/posts/2026-03-11-rust-gui-landscape-2026.html) · [libs.tech new GUI frameworks](https://libs.tech/rust/gui-frameworks) · [COSMIC desktop](https://en.wikipedia.org/wiki/COSMIC_desktop)
- [Why I Switched from Flutter+Rust to egui](https://jdiaz97.github.io/greenblog/posts/flutter_to_egui/)
