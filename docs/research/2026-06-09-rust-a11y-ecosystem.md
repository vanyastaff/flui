# Rust A11y Ecosystem + Competitor Framework Matrix (2026-06-09)

Caveman research-mode synthesis. Companion to [[2026-06-09-flui-semantic-current-state]] (flui side) and [[2026-06-09-flutter-3-41-semantic-reference]] (Flutter reference). Triggers: which Rust crates to use for flui's platform bridges + how the competitor Rust UI frameworks handle a11y in mid-2026.

## Bottom line

**Industry standard IR = AccessKit** (Linebender / Matt Campbell + Arnold Loubriat). 19M lifetime downloads. Schema based on Chromium a11y, **push `TreeUpdate` model**, stable `NodeId`. Used in production by **Slint** (merged 2023), **egui** (via accesskit_winit path), **plushie-iced** (vendored fork, v0.8.4 May 2026), **Servo servoshell** (merged Jun 2025, web content a11y tree Apr 2026). Recommended path for flui.

**Web adapter does NOT exist** in AccessKit (planned). For flui-wasm target: manual `web-sys` ARIA, model = Flutter Web PR #168653 (`SemanticRole` per-role + `SemanticBehavior` cross-cutting).

## 1. AccessKit ecosystem (the standard)

Repo: <https://github.com/AccessKit/accesskit>. Workspace. MIT OR Apache-2.0 (BSD-3-Clause для chromium-derived частей). MSRV 1.85 на всех crates с мая 2026 (edition 2024 prep per v0.33.0 changelog).

**Architecture**: provider (app/toolkit) pushes `TreeUpdate` → consumer (adapter) holds the tree, diffs, translates to platform API. Only the adapter holds the full tree in memory → suitable for immediate-mode GUI.

| Crate | Ver (2026-06) | Platform | Role | Downloads (lifetime) | Maintained | MSRV |
|---|---|---|---|---:|---|---|
| `accesskit` | 0.24.0 (2026-02-01) | cross | core: schema `Tree`/`Node`/`Role`/`TreeUpdate`/affine, provider-side | 19M | YES (active) | 1.85 |
| `accesskit_consumer` | 0.36.0 | cross | platform-agnostic consumer: `Tree`+`ChangeHandler`+`common_filter` (hashbrown only, no OS deps) | 11.8M | YES | 1.85 |
| `accesskit_winit` | 0.33.0 (2026-05-11) | cross | auto-select adapter под winit, default: `accesskit_unix+async-io+rwh_06+winit/{x11,wayland}` | 13.7M | YES | 1.85 |
| `accesskit_windows` | 0.33.0 | Windows | UI Automation (UIA), MSAA legacy НЕ support, native UIA tree | 8.5M | YES | 1.85 |
| `accesskit_macos` | 0.26.1 | macOS | NSAccessibility (AppKit) | 8.2M | YES | 1.85 |
| `accesskit_unix` | 0.21.1 | Linux/BSD | AT-SPI2 через `zbus` | 8.2M | YES | 1.85 |
| `accesskit_android` | 0.7.3 | Android | Java-based Android accessibility API (JNI) | (young) | YES (active hardening) | 1.85 |
| `accesskit_ios` | 0.1.0 | iOS | UIAccessibility (UIKit), initial release | — | YES (initial, May 2026) | 1.85 |
| `accesskit-c` | bundled | cross | FFI, cbindgen, есть Win32 + SDL пример | — | YES | 1.85 |
| `accesskit` (Python) | bundled | cross | PyO3 bindings, Pygame пример | — | YES | n/a |
| **web-adapter** | **—** | **—** | **PLANNED, не released** | — | — | — |

**8 releases** с 2026-01-03 по 2026-05-11.

**Gaps**:
- Web-adapter (canvas-rendered UIs → ARIA) — planned, not shipped
- Rich text + hypertext support — not yet

Source: <https://github.com/AccessKit/accesskit/releases>, <https://docs.rs/crate/accesskit_consumer/latest>

## 2. `atspi` ecosystem (odilia-app)

Repo: <https://github.com/odilia-app/atspi>. License: Apache-2.0 OR MIT. MSRV 1.77.2. Pure-Rust, async, `#![deny(unsafe_code)]`, `#![deny(clippy::all, clippy::pedantic)]`, **type-validation против AT-SPI2 spec через `zbus-lockstep`**.

| Crate | Ver (2026-06) | Role |
|---|---|---|
| `atspi` (meta) | 0.30.0 (2026-05-06) | re-exports; default = `proxies+connection+p2p+wrappers` |
| `atspi-common` | 0.14.0 | types: `Event`, `State`, `Role` enums |
| `atspi-connection` | 0.14.0 | `AccessibilityConnection` — receive-side event stream, `p2p` feature = peer-to-peer (bus bypass) |
| `atspi-proxies` | 0.14.0 | auto-generated D-Bus proxies (zbus) для query/send |

**Features**: `proxies`, `connection`, `p2p`, `wrappers` (categorized `Event` enum + `event_stream`), `tokio` (zbus/tokio), `tracing`. Async runtime: tokio/smol/async-std; НЕ glomio.

**Use case для flui**: НЕ адаптер, это **client-side** binding (для screen reader'а, типа Odilia). Если flui = provider — используй `accesskit_unix` (он уже зовёт `atspi` под капотом).

Downloads: `atspi` 8.6M, `atspi-proxies`/`atspi-common` 7.8M, `atspi-connection` 7.2M.

## 3. Windows UIA

**Нет отдельного mature UIA-only crate для приложений.** Доступные варианты:
- `windows` crate (microsoft/windows-rs) — Windows API bindings, содержит UIA COM interfaces. Comprehensive, official. Используется `accesskit_windows` под капотом.
- `uiautomation` crate (VivoM/pythonista) — high-level UIA wrapper (community).
- `accesskit_windows` — recommended path.

**MSAA (legacy)**: deprecated с Windows 7. UIA полностью его заменяет. Не нужно поддерживать отдельно.

## 4. macOS NSAccessibility

- `accesskit_macos` 0.26.1 — recommended.
- Альтернатива (если AccessKit schema не подходит): `accessibility` 0.2.0 + `accessibility-sys` 0.2.0 (eiz) — direct NSAccessibility bindings, MIT/Apache-2.0. Сырые FFI, 42.9K downloads. **Stagnant** (last update 2025-03). Подходит если хочешь свой адаптер.
- `core-foundation` 0.10.1 (servo) + `cocoa` 0.26.1 (servo) — 322M / 25M downloads. Используются `accesskit_macos` под капотом.

## 5. Web (wasm-bindgen) — ARIA

`accesskit-c` exists, but **web-adapter not released**. Варианты:
- `web-sys` 0.3.x + `wasm-bindgen` 0.2.x + ARIA attributes напрямую через DOM (`Element.set_attribute("aria-label", ...)` и т.п.). Low-level, без tree management.
- `accessibility-rs` 0.1.7 (a11ywatch/j-mendez) — runtime **WCAG audit engine** для HTML (markup5ever+taffy+selectors+spider). 99.5K downloads. **Не** provider-side accessibility tree, а static/dynamic auditor. MIT/Apache-2.0. Полезен для CI gate.
- `rsx-a11y` 0.x (CHildebrandt, 2026-02) — clippy-style lint для ARIA в `view!`/`rsx!`/`html!` макросах (Yew/Leptos/Dioxus), 36 правил WAI-ARIA 1.2. License MIT. **Не** provider tree, static analysis на source.

**Web adapter status (2026)**: ждать. Для flui-wasm target реальный путь = manual ARIA через `web-sys` (как Flutter web engine сейчас и делает через canvas+`SemanticsNode.toAria()`).

## 6. Orca / AT client tools (testing harness)

| OS | Tool | Notes |
|---|---|---|
| **Linux** | `accerciser` (GNOME a11y explorer, GTK3), `atspi-inspect` (CLI) | Orca — consumer, не devtool |
| **Windows** | **Accessibility Insights** (MS, recommended), `Inspect.exe` (Win SDK), `UI Automation Verify` (UIAVerify) | Все читают UIA tree. AccProbe — Adobe legacy, 2014, НЕ рекомендуется |
| **macOS** | **Xcode Accessibility Inspector** (Xcode → Open Developer Tool) | Читает NSAccessibility tree |
| **Web** | Chrome DevTools → Elements → Accessibility pane, **Axe DevTools** (Deque), Lighthouse | |
| **Cross-platform Rust** | `a11y-rust-gui-inspector` (nicolegordon, 2025-Q1) | proof-of-concept, `accesskit_consumer::Tree`. Toy state. |

## 7. UI framework integrations — competitor matrix

| Framework | Status | PR/Issue | Source |
|---|---|---|---|
| **Slint** | MERGED (winit backend), tested Windows Narrator + macOS VoiceOver | PR #2865 (2023-06), PR #3833 (2023-11) | github.com/slint-ui/slint |
| **Iced** | Draft PR #1849 open с 2023-05; System76/COSMIC fork (pop-os/iced) carries работающий a11y; milestone 0.15 | PR #1849, Issue #552 (34 👍) | github.com/iced-rs/iced |
| **Druid** | PAUSED. Matt Campbell's `accesskit` branch existed (Windows-only, no text editing), work stopped because maintainers moved to Xilem. | Issue #2088 (2021) | github.com/linebender/druid |
| **Xilem** | Planned via glazier layer. No merged PR as of 2026-06. | — | github.com/linebender/glazier |
| **Tauri** | Winit-based → `accesskit_winit` auto-wire possible; не документировано в Tauri core. Community a11y tracking via webview (Chromium) — основной путь a11y для Tauri. | — | — |
| **egui** | Indirect: `egui-winit` → `accesskit_winit`. Per `accesskit_winit` changelog был bugfix "reduce winit version requirement to match egui". Not first-class. | — | — |
| **Egui / Masonry** | Masonry (Druid-precursor Xilem widget layer) — `accesskit` branch существует, не merged. | — | — |

**Вывод**: **Slint = единственный production Rust UI framework с merged+tested accesskit**. COSMIC (System76 pop-os iced fork) = production usage через свой fork, не mainline. Plus **plushie-iced** (vendored fork v0.8.4) is the closest second.

## 8. Framework × platform × a11y-depth comparison table

| Framework | iOS | Android | Web | Windows (UIA) | macOS (NSA) | Linux (AT-SPI) | Notes |
|---|---|---|---|---|---|---|---|
| **Flutter** | VoiceOver full | TalkBack full | ARIA opt-in (perf) | UIA full | VoiceOver full | Orca full (AT-SPI2) | Engine owns tree; embedder bridges. iOS WebKit perf regression #179784 still open. |
| **Druid** | n/a | n/a | n/a | PoC branch only | n/a | n/a | Maintenance mode, focus → Xilem. |
| **Xilem** | alpha (a11y basic) | alpha | web backend only | alpha (via Masonry+AccessKit) | alpha | alpha | Alpha, MSRV 1.92, AccessKit v0.32.x. |
| **egui** | limited | limited | limited | AccessKit (stable path) | AccessKit | AccessKit | Recommended real-world Rust a11y path; default in 2026 trajectory. |
| **iced (upstream)** | none | none | none | blocked (winit fork conflict) | blocked | PR #3281 closed Mar 26 | maintainer working on it. |
| **plushie-iced (fork)** | none | none | none | full | full | full (AT-SPI2) | Vendored fork v0.8.4; production-ready a11y. |
| **Slint** | basic (accesskit_ios v0.1.0) | basic (accesskit_android v0.7.3) | canvas (no AccessKit web yet) | UIA (Narrator/NVDA) | NSA (VoiceOver) | Orca (AT-SPI) merged 2023+ | `accessible-*` properties, AccessKit stable. |
| **Makepad** | none | none | none | none | none | none | Backbone in system, no screen-reader bridge. |
| **Dioxus** | WebView a11y (promise) | WebView a11y | ARIA (web) | WebView a11y (Chromium) | WebView a11y (WKWebView) | WebView a11y (WebKitGTK) | No native layer shipped на 2026. |
| **Tauri** | WKWebView a11y | Android WebView a11y | WebView2/WebKit a11y (native) | WebView2 + #12901 NVDA bug (frameless) | WKWebView | WebKitGTK | Inherits webview a11y; Tauri-specific overlay bugs. |
| **Servo (servoshell)** | n/a | n/a | WIP (PR #42338 merged Apr 26) | AccessKit (servoshell UI only) | AccessKit (servoshell UI only) | AccessKit+ORCA confirmed working | Web content a11y tree landing 2026. |
| **Qt (reference)** | UIAccessibilityTraits | TalkBack | Qt WebEngine/Chromium | UIA full | NSA full | AT-SPI full (D-Bus) | Reference impl; QAccessible 3-layer. |
| **GTK (reference)** | n/a | n/a | n/a | n/a | n/a | AT-SPI2 full | Reference impl; ATK → at-spi2-atk → D-Bus. |

## 9. zbus vs atspi для AT-SPI2

- `zbus` 5.x — general D-Bus binding, sync+async, code-gen из introspection XML. Not a11y-specific.
- `atspi` = **typed** AT-SPI2 wrapper поверх zbus. `atspi-lockstep` валидирует types против spec. Pure async (tokio/smol/async-std). Правильный выбор для a11y client.
- Если provider = `accesskit_unix` — zbus transitive dep, but flui не пишет D-Bus напрямую, `accesskit_unix` это делает.

## 10. Bin size impact (wasm)

Точных цифр не публиковали. Rough estimates (по dep tree):
- `accesskit` core + `accesskit_consumer` ≈ 50-80 KB wasm (через wasm-bindgen, stripped)
- `accesskit_winit` подтягивает winit (X11/wayland бэкенды) → ~500KB+ на wasm = **НЕПРИГОДНО** для wasm target. Нужен условный cfg: `accesskit` core only, адаптеры platform-specific.
- `atspi` подтягивает zbus + tokio → **НЕ** для wasm. Только на native Linux build.
- `web-sys` (для ARIA) = самый дешёвый путь для wasm: 0 extra bytes runtime, ARIA = string attributes на DOM.

## 11. Web a11y path — Flutter PR #168653 as model

Flutter Web's `engine/src/flutter/lib/web_ui/lib/src/engine/semantics/` is the production model for canvas-rendered → ARIA. Architecture:
- `abstract class SemanticRole` — per-role DOM `apply()`
- `abstract class SemanticBehavior` — cross-cutting DOM `update()` (`Focusable`/`Tappable`/`LabelAndValue`/`Expandable`/`LiveRegion`/`Requirable`/`CanDisable`/`RouteName`/`Checkable`/`Selectable`)
- `SemanticsEnabler` (`:69`) → `DesktopSemanticsEnabler` (`:133`) / `MobileSemanticsEnabler` (`:235`)

**Rules to follow** (from PR #168653, applied to Rust):
- `label` = short name → `aria-label`
- `description` = long description (hint/tooltip) → `aria-description` (with `aria-describedby` fallback через hidden span)
- `value` = current value (slider/progress) → `aria-valuenow` (numeric) или `aria-valuetext` (text), only for non-incrementable
- **DO NOT** concatenate everything into a single `aria-label`
- Use `aria-live` `polite`/`assertive` host elements for live announcements
- `liveMessageDuration=300ms` workaround for VoiceOver trailing ` ` bug

**Architecture port** (Rust):
- `enum SemanticRole { Button, Tab, TabList, TabPanel, ... }` (32 values, mirror Flutter)
- `trait SemanticBehavior { fn apply(&self, node: &SemanticsNode, dom: &Dom) -> Result<()>; }`
- `impl SemanticRole for ButtonRole { fn apply() { /* emit <button> + aria-* attrs */ } }`
- Behaviors composed: `Button + Tappable + LabelAndValue + Focusable + CanDisable`

## 12. Recommendation matrix for flui

| Target | Primary path | Backup |
|---|---|---|
| **Windows** (native) | `accesskit_winit` → `accesskit_windows` (UIA) | direct `windows` crate UIA COM |
| **macOS** (native) | `accesskit_winit` → `accesskit_macos` (NSAccessibility) | `accessibility` 0.2.0 (eiz, stagnant) |
| **Linux/X11** (native) | `accesskit_winit` → `accesskit_unix` (AT-SPI2 zbus) | direct `atspi` 0.30 (provider-side, but type validation benefits) |
| **Linux/Wayland** | `accesskit_winit` → `accesskit_unix` (same) | — |
| **Android** | `accesskit_android` 0.7.3 (young, active) | wait or `accesskit_winit` doesn't apply (no winit on Android) |
| **iOS** | **WAIT** — `accesskit_ios` 0.1.0 initial, May 2026 | direct `accessibility` 0.2.0 eiz (stagnant) |
| **Web/WASM** | `web-sys` + ARIA (model: Flutter PR #168653) | wait AccessKit web-adapter |
| **Embedded** | none — `accesskit_winit` brings winit dead weight | none |

**Single dep recipe (native cross-platform)**:
```toml
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
accesskit = "0.24"
accesskit_consumer = "0.36"
accesskit_winit = "0.33"  # auto-selects accesskit_<platform> per OS

[target.'cfg(target_arch = "wasm32")'.dependencies]
web-sys = "0.3"
wasm-bindgen = "0.2"
```

## 13. Open PRs reference (potentially useful for flui PR-planning)

- egui AccessKit (PR #2294)
- Slint initial AccessKit (PR #2865)
- Servo servoshell (PR #37519, merged Jun 2025)
- Servo web contents tree (PR #42338, merged Apr 2026)
- Dioxus `inert` (PR #5476, merged Apr 2026)
- Flutter ARIA refactor (PR #168653, May 2025)
- iced AccessKit (PR #3281, closed Mar 2026, maintainer working on it)

## 14. Caveats

- **Rich text + hypertext НЕ поддержан** в AccessKit adapters. Flutter `SemanticsNode.textDirection` + `Link` semantics node → НЕ map-ится чисто. Workaround: mark as text range with hyperlink action.
- **MSRV 1.85** жёсткий (2025-02 release). Если flui MSRV < 1.85 — pin на `accesskit 0.21.x` (последний pre-edition-2024). Currently flui workspace MSRV must be checked before dep bump.
- **`accesskit_ios` 0.1.0** — initial, API может поменяться. Avoid как primary path пока не 0.5+.
- **`accesskit_android` 0.7.3** — young, active hardening. Selected state, range info, URL property, scrolling fixes Feb 2026.
- **MSAA legacy** (pre-Windows 7) — deprecated. UIA replaces entirely. Don't support.
- **Test harness**: `accesskit_consumer::Tree` lets flui write unit tests for tree diffing without OS dep. Pair with `accerciser`/`Accessibility Insights` for end-to-end smoke.

## Sources

- [AccessKit GitHub](https://github.com/AccessKit/accesskit)
- [accesskit_winit v0.32.2 release](https://github.com/AccessKit/accesskit/releases/tag/accesskit_winit-v0.32.2)
- [accesskit_consumer docs.rs](https://docs.rs/crate/accesskit_consumer/latest)
- [odilia-app/atspi](https://github.com/odilia-app/atspi)
- [atspi 0.30.0](https://crates.io/crates/atspi)
- [Slint #2865 Initial AccessKit support](https://github.com/slint-ui/slint/pull/2865)
- [Slint #3833](https://github.com/slint-ui/slint/pull/3833)
- [Iced #1849](https://github.com/iced-rs/iced/pull/1849)
- [Druid #2088](https://github.com/linebender/druid/issues/2088)
- [plushie-iced](https://crates.io/crates/plushie-iced)
- [accessibility-rs](https://crates.io/crates/accessibility-rs)
- [rsx-a11y](https://docs.rs/rsx-a11y/latest/rsx_a11y/)
- [a11y-rust-gui-inspector](https://github.com/nicolegordon/a11y-rust-gui-inspector)
- [DeepWiki AccessKit](https://deepwiki.com/AccessKit/accesskit)
- [Qt QAccessible](https://doc.qt.io/qt-6/accessible.html)
