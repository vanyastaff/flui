---
name: FLUI
last_updated: 2026-05-21
---

# FLUI Strategy

## Target problem

Rust-разработчик, любящий язык и Flutter widget style, сегодня вынужден выбирать между HTML/CSS-style фреймворками (Leptos, Dioxus), immediate-mode toolkits (egui, iced) или JS/TS embed-стеком (Tauri). Теряет widget-tree композицию, ergonomics тестирования компонентов, mobile-таргет и DX-инструменты вроде hot-reload и inspector.

## Our approach

Flutter-style three-tree архитектура (View → Element → Render) поверх wgpu GPU canvas как технический фундамент, в неделимой связке с DX-инструментами (hot-reload, inspector, devtools) с первого дня. Разделение этих двух ставок приведёт к переписыванию большой части кода позже; поддерживающие выборы — type-safe arity widget composition и mobile-native first-class.

**Порт, не редизайн.** Прецедент — Bun rewrite Zig+C++ → Rust ([oven-sh/bun#30412](https://github.com/oven-sh/bun/pull/30412), merged 2026-05-14): сохранили архитектуру и data structures, прошли existing test suite, выиграли memory safety + меньше runtime багов. FLUI применяет тот же принцип к Flutter → Rust: трёхдерево, lifecycle, layout-protocol повторяют Flutter; Rust-идиомы (arity-system, ambassador delegation, NonZeroUsize IDs) — поверх той же модели, не вместо неё.

**Архитектурные принципы порта.** Три правила решают конфликты Dart↔Rust mapping'а:

- **Behavior loyal, structure Rust-native.** Алгоритмы (build/layout/paint, lifecycle FSM, dependency tracking InheritedWidget, child reconciliation через keys) портируются 1:1 из `.flutter/`. Shape данных — Rust-native: trait + generic вместо inheritance, `Option<T>` + `NonZeroUsize` ID offset вместо nullable refs, Slab arena вместо tree pointers, `Result<T, E>` + `thiserror` вместо exceptions. **`flui-tree` крейт — прямое применение этого принципа к самим деревьям**: Flutter имеет four parallel tree implementations (Element / RenderObject / Layer / Semantics) каждое со своей traversal logic; `flui-tree` существует как unified Rust trait API (`TreeRead`/`TreeNav`/`TreeWrite` + `Arity` system + `Mountable`/`Unmountable` typestate + visitors/cursors/diffs) поверх которого все four trees должны строиться. Zero-consumer abstractions в `flui-tree` — это migration gap (production crates ещё пишут bespoke traversals), не deletion signal; миграция consumers К unified API делается, не наоборот.
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
- **Реинвент Flutter widget tree mental model** — порт, не редизайн. Любая попытка "сделать лучше через React signals / SwiftUI declarative" откатывается к Flutter-семантике. Это правило про widget-tree semantics, **не** про топологию процесса: multi-window ownership, runtime/scheduling topology, concurrency и presentation architecture — санкционированные leapfrog-зоны (ADR-0027, модель `UiRealm`); Flutter здесь behavioral reference, а не топологический образец.
- **Heavy dep tree** — каждая workspace dependency = транзитивные хвосты, binary size, compile time. После MVP — diet (`cargo bloat`, `cargo tree --duplicates`) baseline.
- **Telemetry / analytics в библиотеке** — никаких opt-out пингов. Метрики приходят через GH issues + external PR contributors, не через runtime instrumentation.
