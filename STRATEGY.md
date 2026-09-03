---
name: FLUI
last_updated: 2026-09-03
---

# FLUI Strategy

## Target problem

Rust-разработчик, любящий язык и Flutter widget style, сегодня вынужден выбирать между HTML/CSS-style фреймворками (Leptos, Dioxus), immediate-mode toolkits (egui, iced) или JS/TS embed-стеком (Tauri). Теряет widget-tree композицию, ergonomics тестирования компонентов, mobile-таргет и DX-инструменты вроде hot-reload и inspector.

## Our approach

Flutter-style three-tree архитектура (View → Element → Render) поверх wgpu GPU canvas как технический фундамент, в неделимой связке с DX-инструментами (hot-reload, inspector, devtools) с первого дня. Разделение этих двух ставок приведёт к переписыванию большой части кода позже; поддерживающие выборы — type-safe arity widget composition и mobile-native first-class.

**Flutter — референс и оракул, не потолок.** Flutter даёт две вещи, которых больше ни у кого нет: проверенную десятью годами продакшна модель (три дерева, lifecycle, box/sliver layout-protocol, reconciliation по ключам) и тестовый корпус на ~3000 сценариев, который эту модель фиксирует. FLUI берёт у Flutter именно это — *наблюдаемое поведение как нижнюю границу* и тесты как самую дешёвую верификацию (прецедент — Bun rewrite Zig+C++ → Rust, [oven-sh/bun#30412](https://github.com/oven-sh/bun/pull/30412): прошли existing test suite, выиграли memory safety). Всё остальное — структура, архитектура, стиль кода, выбор механизмов — проектируется для Rust и для рынка 2026 года, а не переносится из Dart 2015-го: решения Flutter — это контракты своего времени (один isolate, nullable-ссылки, исключения, строковые aspects, `const`-хак компилятора), и там, где сегодня известно лучшее решение, FLUI обязан его выбрать. Каждое улучшение над референсом ведёт учёт: что лучше и почему (ADR для протокольного контракта, `## Mapping decisions` в `ARCHITECTURE.md` крейта для локального), какой Flutter-тест перестаёт применяться и какой тест FLUI занимает его место, какие edge cases сохранены. «Как во Flutter» — достаточная причина только когда лучшего не известно. Уже сделанные улучшения такого рода: field-granular inherited dependencies (ADR-0008), capability-scoped `BuildContext` (ADR-0018/21/30/37), realm-модель (ADR-0027), `Result` вместо исключений.

**Архитектурные принципы.** Три правила решают конфликты Dart↔Rust mapping'а:

- **Behavior as floor, everything else designed for Rust.** Наблюдаемые контракты (результат build/layout/paint, порядок lifecycle-событий, edge cases dependency tracking и reconciliation) берутся из `.flutter/` как минимум, который надо превзойти или как минимум не потерять — и доказать портированным или заменяющим тестом. Shape данных и механизмы — Rust-native и современные: trait + generic вместо inheritance, `Option<T>` + `NonZeroUsize` ID offset вместо nullable refs, Slab arena вместо tree pointers, `Result<T, E>` + `thiserror` вместо exceptions, типизированные field-mask aspects вместо `InheritedModel`. **`flui-tree` крейт — прямое применение этого принципа к самим деревьям**: Flutter имеет four parallel tree implementations (Element / RenderObject / Layer / Semantics) каждое со своей traversal logic; `flui-tree` существует как unified Rust trait API (`TreeRead`/`TreeNav`/`TreeWrite` + `Arity` system + `Mountable`/`Unmountable` typestate + visitors/cursors/diffs) поверх которого все four trees должны строиться. Zero-consumer abstractions в `flui-tree` — это migration gap (production crates ещё пишут bespoke traversals), не deletion signal; миграция consumers К unified API делается, не наоборот.
- **Compile-time over runtime** где возможно. Arity system (`Leaf`/`Single`/`Optional`/`Variable`) ловит arity-mismatch на этапе компиляции, а не paint. Typestate (`BuilderContextBuilder<P, Pr>`) валидирует Android/iOS/Desktop/Web config. Sealed traits (`PlatformBuilder`) дают exhaustive match. TypeId-registry для InheritedView lookup — единственное допустимое runtime-reflection окно.
- **Sync hot path, async на краях.** Render pipeline (build → layout → paint → composite) строго синхронен; frame budget critical, async overhead неприемлем. Async OK на границах: IO в `flui-assets`, scheduler в `flui-scheduler`, build pipeline в `flui-build`. Никакого `async fn` в `View::build` или `RenderObject::paint`.

## Who it's for

**Primary:** Rust-разработчик, отвергающий JS-стек и HTML/CSS mental model — нанимает FLUI чтобы быстро поднять cross-platform UI на чистом Rust через компонентную widget-композицию, без CSS и div.

## Key metrics

- **GH issue mix** — соотношение bug / question / feature-request labels по кварталам. Сдвиг к bug = качество ↓; к question = docs ↓.
- **External PR contributors per quarter** — количество non-maintainer контрибьюторов с merged PR. Растёт = mental model понятен снаружи.
- **Sample apps build pass-rate** — собственные example apps собираются clean на каждом тэге без breaking changes. Регрессия = API нестабилен.

<!-- Метрики намеренно минимальны, без telemetry. Revisit после появления первых external users. -->

## Tracks

### Platform foundation

flui-platform MVP и native backends (Win32/AppKit/Wayland/Android/iOS) — window/input/clipboard абстракции, raw-window-handle, event dispatch.

_Why it serves the approach:_ обеспечивает mobile-native first-class и pixel-perfect cross-platform консистентность, без которых GPU canvas теряет смысл.

### Render pipeline

wgpu integration, three-tree lifecycle (build → layout → paint), layer compositing, frame budget, paint optimization.

_Why it serves the approach:_ техническое тело архитектурной ставки — без надёжного render core three-tree остаётся диаграммой.

### Developer tooling (DX)

flui-cli, flui-devtools, hot-reload pipeline, widget inspector, build automation.

_Why it serves the approach:_ approach #3 буквально — DX day-1 без отдельного track останется аспирацией.

## Not working on

- **Async в render hot path** — `tokio::spawn` / `async fn` ограничены scheduler/IO/build pipeline. Layout/paint синхронны, frame budget critical. Bun-прецедент: rewrite без async подтвердил жизнеспособность.
- **Смена mental model для пользователя фреймворка** — декларативная композиция виджетов над retained three-tree, ключи, lifecycle — это то, за что FLUI нанимают; она не меняется на React signals / SwiftUI attribute graph / CSS-layout как *внешнюю модель*. Всё, что под ней — механизмы инвалидации, dependency tracking, планирование, потоки, рендер — открыто для лучших решений при условии учёта из «Flutter — референс и оракул» выше (ADR-0008 — пример: тот же контракт для пользователя, другой и лучший механизм внутри). Топология процесса вообще не наследуется: multi-window ownership, runtime/scheduling topology, concurrency и presentation architecture — leapfrog-зоны (ADR-0027, модель `UiRealm`).
- **Heavy dep tree** — каждая workspace dependency = транзитивные хвосты, binary size, compile time. После MVP — diet (`cargo bloat`, `cargo tree --duplicates`) baseline.
- **Telemetry / analytics в библиотеке** — никаких opt-out пингов. Метрики приходят через GH issues + external PR contributors, не через runtime instrumentation.
