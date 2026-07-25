# Аудит FLUI — upgrade pack (Rust 1.97+ / архитектура 2026)

**Статус:** дополнение к [`2026-07-23-architecture-audit.md`](2026-07-23-architecture-audit.md) (разделы 8.1–8.10). Нумерация разделов ниже соответствует пунктам 13–32 постановки задачи.

**Дата:** 2026-07-25. **Срез кода:** `main` @ `3bb08700` + незакоммиченное рабочее дерево (важно: часть находок относится именно к рабочему дереву — см. §19).

**Формат уверенности** (как в основном аудите): **доказанный дефект** — есть evidence по `file:line` или воспроизведённая команда; **риск** — правдоподобно, не подтверждено исполнением; **гипотеза** — требует замера.

**Что в этом аудите НЕ делалось** (честная граница, см. §32):

- не запускался полный `cargo hack --feature-powerset` (комбинаторно дорого на 35 пакетах);
- не запускались профайлеры (flamegraph / samply / Tracy / WPA) против release-сборки — §21 опирается на конфигурацию профиля и на измеренную мономорфизацию, а не на снятые стеки;
- не проводился построчный аудит всех 236 файлов с атомиками — разобраны только несущие счётчики;
- бенчмарки §30 **не написаны** — раздел является спецификацией, а не отчётом о замерах;
- матрица экосистемы (§31.4) — экспертная оценка по состоянию знаний на май 2026, не измерение.

---

## Статус: что уже применено (2026-07-25, после аудита)

Часть находок закрыта в том же проходе. Всё проверено исполнением на 1.97.1.

| Находка | Действие | Проверка |
|---|---|---|
| §13 / U14 | Toolchain 1.96.1 → **1.97.1**; MSRV 1.96 → **1.97**. Контуры расцеплены: `rust-toolchain.toml` объявлен development-тулчейном и больше не зеркалит MSRV; список точек бампа в его шапке дополнен (`clippy.toml`, джоб `msrv`, два шаблона `flui-cli`) | `cargo clippy --workspace --all-targets -- -D warnings` — чисто; `cargo nextest run --workspace --exclude flui-platform` — **7866 passed, 12 skipped, 0 failed**; `cargo fmt --check`, `port-check.sh`, `check-workspace-inventory.sh` — чисто |
| U1 + U11 | Wasm-гейт починен. Долг оказался **больше заявленного**: не 2 ошибки, а **11 в трёх крейтах** — `flui-scheduler` (2), `flui-platform` (4, web-бэкенд), `flui-app` (5). Причина последних пяти — джоб гоняет `cargo check` **без** `--all-targets`, поэтому потребители из desktop-runner (`cfg(not(target_arch = "wasm32"))`) и из тестов исчезают, и живой код выглядит мёртвым; помечено `#[cfg_attr(target_arch = "wasm32", allow(dead_code))]` с указанием потребителя, а не глухим `allow` | `RUSTFLAGS="-D warnings" cargo check --target wasm32-unknown-unknown --workspace <минус 7 крейтов>` — **зелено** |
| U10 | `RUSTFLAGS: -D warnings` → **`CARGO_BUILD_WARNINGS: deny`** (Cargo 1.97) в `ci.yml` и `weekly.yml`; джоб miri вместо обнуления `RUSTFLAGS` целиком ставит `CARGO_BUILD_WARNINGS: warn` и сохраняет общий кэш | Измерено: переключение `build.warnings` — **0 перекомпиляций**, переход на `RUSTFLAGS` — **1**. На 1.96.1 настройка молча игнорируется, на 1.97.1 даёт `error: warnings are denied by build.warnings configuration` |
| U4 | `strip = "symbols"` → **`strip = "debuginfo"`**: символы в release сохранены, DWARF по-прежнему отброшен | Замер на `target/release/flui`: **3 437 096 → 4 394 832 байт (+935 KiB, +27.9%)**, символов 0 → 8654. В бинаре **5110 v0-символов, 0 legacy**; generic-кадры демангятся с полными типами (`pollster::block_on::<<flui_build::desktop::DesktopBuilder as …PlatformBuilder>::build_rust::{closure#0}>`) |
| U13 | Устаревшая ссылка на пин «1.96.0» в `ci.yml` убрана | — |
| §18 / §31.2 | **Поправка:** рекомендация применить `assert_matches!` была ошибочной — макрос не стабилизирован ни в 1.96.1, ни в 1.97.1 (проверено компиляцией). См. §18 |
| U2 | `Lifecycle::can_transition_to` фиксирует единственный однозначный инвариант — **`Defunct` поглощающее** — и проверяется `debug_assert` в `mount`/`activate`/`deactivate`. Предикат намеренно разрешает повторный вход в текущее состояние: обходы дерева легитимно повторно активируют уже-Active элемент | Два `#[should_panic]`-теста доводят элемент до `Defunct` и реактивируют/деактивируют его — без гардов оба красные; третий проходит всю таблицу 4x4 |
| U5 | Перф-оверлей подключён: `AppConfig::show_performance_overlay` → `AppBinding::set_performance_overlay` (bootstrap, 3 пути) → `Mutex<Option<PerformanceStats>>` → слой добавляется последним ребёнком корня в фазе 4 `draw_frame`. `Some` **и есть** флаг включения, поэтому «включено, но без статистики» непредставимо | 4 теста на хелпер: выключено — дерево не меняется; включено — один связанный слой (обе стороны связи); дерево без корня — no-op; выключение сбрасывает окно. Кадровый вызов и bootstrap-проводка тестами **не покрыты** |
| U19 | **Полностью.** Workspace-линты `unexpected_cfgs`, `unsafe_op_in_unsafe_fn`, `unused_must_use`, **`undocumented_unsafe_blocks`** = `deny`. Все 41 недокументированных `unsafe`-сайта закрыты: 7 в `subtree_arena` и 4 в парах `Send`/`Sync` уже имели обоснование абзацем выше — перепривязано к самому выражению; 11 в `flui-hot-reload`, 4 в тестах планировщика и 4 прочих написаны заново и сверены с кодом (`load_library` null-проверяет `dlopen`, `DynLib` не `Clone` и закрывается один раз). 12 в выводе стороннего `wgsl_bindgen` — `allow` точечно на шести `include!`-мостах, не на workspace | `clippy --all-targets -D warnings` чисто с активным линтом |
| §15 / U12 | `TextRange` был объявлен дважды; консолидирован в `flui-types` (её копия — строгое надмножество), канонический тип получил `Clone, Copy, PartialEq, Eq, Hash` | port-check trigger #14 больше не срабатывает на это имя |
| Модернизация std | `once_cell` удалён из workspace: мёртвая зависимость в `flui-assets` и `flui-interaction`, единственное реальное применение (`Lazy` в `flui-reactivity`) → `std::sync::LazyLock`. Десять `&expr as *const/*mut` → `&raw const`/`&raw mut` | Полный свип `crates/*/src` не находит ни одного оставшегося |
| Кросс-таргеты | **Оба нативных бэкенда были сломаны и это не мог увидеть ни один гейт.** Windows не собирался вовсе (`pub mod win32` без документации под `#![deny(missing_docs)]`); девять типов в Windows и macOS без `Debug`. Добавлен джоб `cross-check` + `just cross-check` | `cargo check` не линкует, поэтому оба бэкенда проверяются с Linux; оба чисты под `-D warnings` |
| U19 (было частично) | Workspace-линты: `unexpected_cfgs`, `unsafe_op_in_unsafe_fn`, `unused_must_use` = `deny` (замерено 0 сайтов, значит это барьер против регрессии, а не миграция). `rust_2018_idioms` опущен до priority -1 | `clippy --all-targets -D warnings` чисто |

**Цена U4 — решение, а не данность.** +935 KiB (+27.9%) на бинарь — плата за профилируемый и символизируемый release. Альтернатива (ship stripped + отдельный symbol-сервер) требует инфраструктуры, которой у проекта нет. Если размер критичнее диагностики, вернуть `strip = "symbols"` — одна строка.

**`undocumented_unsafe_blocks` НЕ принят** (остаток U19): замерено **47 сайтов** — 16 `flui-hot-reload`, 12 в коде, сгенерированном build-скриптом, 7 `flui-rendering`, 6 в тестах, 6 в types/layer/view/platform. SAFETY-комментарий обязан доказывать инвариант, поэтому массовая простановка сфабриковала бы непроверенные утверждения; сгенерированной дюжине нужен другой генератор, а не правка вывода.

**Отдельно замечено:** `flui-widgets::parity dismissible_test::resize_collapse_starts_at_full_size_then_runs_to_completion` — флейк. Упал один раз в полном прогоне, проходит изолированно и при повторном полном прогоне. Не связан с правками этой сессии; `dismissible.rs` — один из немногих production-файлов с `SeqCst` (§17).

**Остаётся открытым:** U3, U6, U7, U8, U9, U12, U15–U18, U20 и остаток U19.

---

## 13. Актуальность toolchain и MSRV

### Фактическое состояние

| Артефакт | Значение | Источник |
|---|---|---|
| Локальный `rustc` | `1.96.1 (31fca3adb 2026-06-26)`, LLVM 22.1.2 | `rustc -vV` |
| Локальный `cargo` | `1.96.1 (356927216 2026-06-26)` | `cargo --version` |
| Пин toolchain | `channel = "1.96.1"`, profile `default`, components `rustfmt`+`clippy` | `rust-toolchain.toml` |
| MSRV | `rust-version = "1.96"` | `Cargo.toml` `[workspace.package]` |
| Edition | `2024` (весь workspace) | `Cargo.toml` |
| **Актуальный stable** | **`1.97.1 (8bab26f4f 2026-07-14)`** | `rustup check` |
| Nightly-контур | `nightly` + пин `nightly-2026-03-20`; используется **только** для miri | `rustup show`, `justfile:141-143` |
| Установленные targets | `x86_64-unknown-linux-gnu`, `wasm32-unknown-unknown`, `x86_64-pc-windows-gnu`, `x86_64-apple-darwin`, `aarch64-linux-android` | `rustup show` |

Состав: 28 каталогов в `crates/`, из них 27 — члены workspace; всего workspace резолвится в 35 пакетов (плюс фасад `flui`, демо и `hot-reload-counter-*`). Все 27 наследуют `rust-version.workspace = true` — расхождений по MSRV внутри workspace нет. Исключение — `flui-reactivity` (см. §29): он **не является членом workspace** (`Cargo.toml:63-68`, закомментирован), стоит на **edition 2021**, версии `0.1.0`, без `rust-version` и без `[lints]`.

### Разделение четырёх контуров

Постановка требует различать development / CI stable / MSRV / nightly. В FLUI сейчас **первые три схлопнуты в один**:

- `rust-toolchain.toml` пинит `1.96.1` — значит и разработчик, и большинство CI-джобов (кроме тех, что явно ставят `dtolnay/rust-toolchain@stable`) работают на MSRV;
- CI-джоб `msrv` проверяет `1.96` — но остальные джобы, ставящие `stable`, получают **1.97.1**, т.е. `rust-toolchain.toml` в CI переопределяется action'ом;
- фактически «development toolchain» = «MSRV» = 1.96.1, а «CI stable» = 1.97.1 — расхождение существует, но не выражено намеренно.

**Доказанный дефект (документация).** В рабочем дереве `.github/workflows/ci.yml:539` комментарий утверждает «the committed rust-toolchain.toml pin (1.96.0)», тогда как пин — `1.96.1`. Дрейф комментария; на поведение не влияет.

**Риск.** Пин development-toolchain на MSRV означает, что разработчик никогда не видит диагностики нового компилятора локально: новые clippy-линты, улучшенные сообщения borrow-checker'а и новые `unexpected_cfgs`-предупреждения проявятся только в CI-джобах на `stable`. Это ровно тот случай, который постановка требует разделять.

### Отчёт по компонентам

| Компонент | Текущая версия | Рекомендуемая | Причина | Breaking risk |
|---|---:|---:|---|---|
| rustc (development) | 1.96.1 | **1.97.1** | Патч-релиз актуальной ветки; разработчик должен видеть те же диагностики, что и CI stable | **Низкий** — в дереве ноль nightly-фич, edition 2024 без изменений |
| Cargo | 1.96.1 | 1.97.1 | Идёт в комплекте с rustc | Низкий |
| Clippy | из 1.96.1 | из 1.97.1 | Новые линты попадут в гейт `-D warnings` при апгрейде | **Средний** — новые линты в pedantic могут потребовать точечных `allow` с обоснованием |
| rustfmt | из 1.96.1 | из 1.97.1 | Стабильные правила форматирования между патч-релизами не меняются | Низкий |
| Edition | 2024 | **2024 (оставить)** | Актуальная; смысла двигаться нет | — |
| **MSRV** | 1.96 | **1.96 (не двигать сейчас)** | См. измерение ниже | — |

### MSRV: измерение перед решением

Постановка прямо запрещает автоматический подъём MSRV. Измеряю по четырём указанным критериям:

1. **Пользователи на старой версии.** Крейты не опубликованы на crates.io (`version = "0.2.0"`, публикации не было — см. §31.1). Внешних потребителей MSRV **нет**, значит цена подъёма сегодня близка к нулю, а через год — нет. Это аргумент за то, чтобы политику MSRV зафиксировать *сейчас*, но не за то, чтобы поднимать её без нужды.
2. **Платформы со старым toolchain.** Ограничивающий фактор — не дистрибутивы, а образы CI и Android NDK-сборка; ни один из них не закрепляет 1.96.
3. **Зависимости, поднявшие MSRV.** `wgpu` 29.x — самый агрессивный по MSRV элемент стека; `cosmic-text`/`glyphon`/`lyon` идут следом. Пин `--locked` и джоб `deny` защищают от внезапного дрейфа, но **не проверяют MSRV транзитивно**: джоб `msrv` делает `cargo check` с закоммиченным lockfile, поэтому подъём MSRV у зависимости обнаружится только при `cargo update` (еженедельный `weekly.yml` это и делает — но он не merge-gate).
4. **Что даёт новый Rust.** См. §31.2 — конкретной фичи 1.97, которая упрощает FLUI, я не нашёл. Единственный кандидат (`core::range`) в 1.96 уже доступен и всё равно не используется (§15).

**Вывод по MSRV.** Поднимать 1.96 → 1.97 **не нужно**: пользы нет, а MSRV — это обещание, которое дёшево дать и дорого забрать. Правильное действие — **разделить контуры**: поднять *development/CI stable* до 1.97.1, оставив `rust-version = "1.96"` как MSRV, и проверять MSRV отдельным джобом (он уже есть).

Механически это значит: `rust-toolchain.toml` **не должен** зеркалить MSRV. Сейчас он это делает намеренно (комментарий в файле: «Mirrors `[workspace.package].rust-version`»), и именно это склеивает два разных контура. Предлагаемая замена — пин `channel = "1.97.1"` в `rust-toolchain.toml` + `rust-version = "1.96"` в `Cargo.toml` + существующий джоб `msrv`, который единственный проверяет нижнюю границу.

---

## 14. Stable-first архитектура

**Результат проверки: чисто.**

- `#![feature(...)]` — **0 вхождений** во всём дереве (`crates/`, `src/`, `examples/`).
- `rustc_private`, `RUSTC_BOOTSTRAP` — **0 вхождений**.
- Нестабильные ABI, specialization, экспериментальный trait solver, async-trait-расширения — не используются.
- `async_trait` (крейт) — **0 вхождений**; async в публичной поверхности построен на стабильных `Future`/`Pin<Box<dyn Future>>` (§24).

### Таблица nightly-зависимостей

| Feature | Где используется | Зачем нужен | Stable fallback | План удаления |
|---|---|---|---|---|
| *(нет языковых nightly-фич)* | — | — | — | — |
| nightly **toolchain** (не feature) | CI-джоб `miri` (`ci.yml:529-543`), `just miri` (`justfile:141-143`) | `cargo miri test -p flui-rendering` по `pipeline::owner::subtree_arena` — единственный `unsafe`-hotspot | Нет и не нужен: miri принципиально nightly-only | Не удалять. Это санкционированное применение из списка постановки («sanitizers», «MIR inspection») |

**Вердикт §14: соответствует требованию.** Публичная архитектура 1.0 собирается на stable без оговорок. Nightly изолирован в один advisory-джоб и не влияет на runtime. Это одно из немногих мест, где проект строго лучше типичного Rust-UI-стека.

Единственное замечание — **риск**: джоб miri вынужден сбрасывать `RUSTFLAGS: ""` (`ci.yml:518-523`), потому что глобальный `-D warnings` ломается о deprecation-предупреждения nightly. Это симптом проблемы §20, а не §14.

---

## 15. Аудит новых Range API

### Инвентаризация

| Тип | Файл | Форма |
|---|---|---|
| `TextRange` | `crates/flui-types/src/typography/text_metrics.rs:70` | собственная структура |
| `TextRange` | `crates/flui-rendering/src/parent_data/table_text.rs:133` | **вторая структура с тем же именем** |
| `TextSpan` | `crates/flui-types/src/typography/text_spans.rs:142` | собственная структура |
| `Range<T>` (std) | `flui-widgets/src/text/controller.rs` (4), `flui-objects/src/text/editable.rs` (4), `flui-tree/src/arity/types.rs` (3), `flui-tree/src/arity/traits.rs` (2), ещё 2 файла | локальные использования |

Из перечисленных в постановке типов **отсутствуют**: `SelectionRange`, `GlyphRange`, `LineRange`, `ChildRange`, `LayoutRange`, `DamageRange`, `DisplayCommandRange`, `NodeSpan`, `FrameSpan`, `BufferSlice`, `ArenaSpan`.

- `core::range::Range` (новый `Copy`-тип) — **0 использований**.
- `RangeBounds` в публичных сигнатурах — **0 использований**.

### Оценка: где новый тип даёт семантическое преимущество

Постановка требует не механической замены. Проверяю по её же критериям:

| Кандидат | `Iterator` нужен? | Должен быть `Copy`? | В compact node? | Через события? | Hot path? | Вердикт |
|---|---|---|---|---|---|---|
| `TextRange` (flui-types) | нет | **да** — семантика значения, копируется в selection/IME | нет | **да** — IME/selection | нет | **Стоит рассмотреть** `core::range::Range<usize>` внутри |
| `TextRange` (flui-rendering table_text) | нет | да | нет | нет | нет | Сначала устранить дублирование имени |
| `Range<usize>` в `flui-tree/arity` | да, используется как итератор дочерних индексов | нет | нет | нет | **да** | **Не трогать** — здесь `Iterator` и есть смысл |
| `Range` в `text/controller.rs`, `text/editable.rs` | частично | да | нет | да | нет | Кандидат на унификацию с `TextRange` |

**Находка (доказанный дефект, низкая тяжесть).** Два разных публичных типа с именем `TextRange` в двух крейтах (`flui-types` и `flui-rendering`) — источник путаницы при импорте и препятствие для любой унификации range-семантики. Это не про новый API Rust, это про дисциплину именования; но чинить надо до того, как вводить `core::range`.

**Находка (риск).** Отсутствие `impl RangeBounds<usize>` в публичных сигнатурах означает, что любой будущий переход на `core::range::Range` будет breaking change для потребителей: сегодня API принимают конкретные типы. Постановка рекомендует принимать абстракцию — там, где функция логически берёт диапазон (выделение текста, срез списка), сигнатура `impl RangeBounds<usize>` сделала бы миграцию аддитивной.

**Вердикт §15: действий по существу не требуется сейчас, но нужны два дешёвых шага.** (1) Развести одноимённые `TextRange`. (2) В новых публичных API, принимающих диапазон, писать `impl RangeBounds<usize>`. Переход на `core::range::Range` внутри `TextRange` — только после бенчмарка; сегодня доказательств выигрыша нет (**гипотеза**), а текстовый стек и без того переделывается по K3 основного аудита.

---

## 16. Compact bitsets и dirty flags

### Что уже сделано хорошо

**Render-дерево уже использует именно ту схему, которую предлагает постановка.** `crates/flui-rendering/src/storage/flags.rs:99` — `bitflags! { pub struct RenderFlags: u32 }` с документированной раскладкой битов и хранением в `AtomicU32` (`AtomicRenderFlags`):

```
Bit 0: NEEDS_LAYOUT        Bit  6: (reserved)
Bit 1: NEEDS_PAINT         Bit  7: HAS_OVERFLOW (debug)
Bit 2: NEEDS_COMPOSITING   Bit  8: NEEDS_LAYOUT_PROPAGATION
Bit 3: IS_RELAYOUT_BOUNDARY  Bit  9: NEEDS_PAINT_PROPAGATION
Bit 4: IS_REPAINT_BOUNDARY   Bit 10: WAS_REPAINT_BOUNDARY
Bit 5: NEEDS_SEMANTICS       Bit 11: NEEDS_COMPOSITING_BITS_UPDATE
```

Всего в дереве 4 объявления `bitflags!`: этот, `flui-widgets/src/widget_state.rs:140` (`WidgetState`), и два в `flui-platform/src/platforms/linux/window_ext.rs:339,446`.

### Где схема не доведена

**Доказанный дефект.** Element-дерево (`flui-view`) **не** использует компактный битсет — оно хранит разрозненные `bool`:

- `crates/flui-view/src/view/root.rs:118` — `needs_build: bool`;
- `crates/flui-view/src/element/generic.rs:483` — `self.dirty.load(Ordering::Relaxed) && self.lifecycle.can_build()` — отдельный атомик `dirty` плюс отдельное поле `lifecycle`;
- `crates/flui-rendering/src/view/scroll_position.rs:215` — `metrics_dirty: bool`.

Флаги, перечисленные в постановке (`focus`, `hover`, `active`, `disabled`, `mounted`, `detached`, `animation`, `transform`, `clipping`, `accessibility`, `cache validity`), распределены между `WidgetState` (bitflags), `Lifecycle` (enum), `RenderFlags` (bitflags) и отдельными `bool`/`AtomicBool` полями. Единой модели нет.

**Доказанный дефект.** Ни одного использования современных целочисленных операций для выбора следующей грязной стадии: `trailing_zeros`, `leading_zeros`, `count_ones`, `isolate_lowest_one` по `RenderFlags` не применяются. Выбор стадии пайплайна — линейный перебор.

**Важное уточнение, которое меняет приоритет.** Оптимизация выбора стадии через `trailing_zeros` даст выигрыш в наносекундах на узел. Основной аудит показал (K1), что paint обходит **всё дерево целиком** на любой dirty-узел. Пока это так, битовые трюки на выборе стадии — оптимизация не того порядка. **Приоритет §16 ниже, чем K1/K2 основного аудита**, и я не рекомендую браться за него раньше.

### Чего нельзя делать

Постановка предупреждает: не заменять понятные enum на bit flags автоматически. Конкретно в FLUI **`Lifecycle` (Initial/Active/Inactive/Defunct) заменять битами нельзя** — это взаимоисключающие состояния, а не флаги; битовое представление сделало бы представимым `Active|Defunct`. Наоборот, его нужно усилить (§18).

### Требуемые замеры (не выполнены)

Прежде чем сливать `bool`-поля Element в битсет, постановка требует измерить. Ни одного из этих замеров в проекте нет:

| Замер | Статус |
|---|---|
| размер структуры узла (`size_of::<ElementCore>()`) | **отсутствует** |
| число загружаемых cache lines на обход | отсутствует |
| число branches | отсутствует |
| стоимость обновления флага | отсутствует |
| качество generated assembly | отсутствует |
| влияние на отладку | отсутствует |

Бенчмарки §16 (один флаг / несколько флагов / миллион заголовков / очистка после кадра / поиск первой стадии) — **не написаны**, спецификация в §30.

---

## 17. Atomic portability

### Инвентаризация

1729 объявлений атомиков в **236 файлах**:

| Тип | Кол-во |
|---:|---|
| `AtomicUsize` | 787 |
| `AtomicBool` | 516 |
| `AtomicU32` | 131 |
| `AtomicU64` | 123 |
| `AtomicU8` | 32 |
| `AtomicI32` | 17 |
| `AtomicI64` | 3 |

Ordering:

| Ordering | Всего | В `crates/*/src` | В `crates/*/tests` |
|---|---:|---:|---:|
| `SeqCst` | 1087 | 426 | 636 |
| `Relaxed` | 720 | — | — |
| `Acquire` | 112 | — | — |
| `Release` | 72 | — | — |
| `AcqRel` | 48 | — | — |

### Поправка к первому впечатлению

На первый взгляд доминирование `SeqCst` (53% всех orderings) выглядит как системное нарушение правила «не используй SeqCst по умолчанию». **Проверка не подтверждает эту трактовку.** Разбивка показывает, что подавляющая часть `SeqCst` — тестовые счётчики: 636 из 1087 в каталогах `tests/`, плюс внутри `src/` основные носители — файлы `*_tests.rs` (`flui-widgets/src/navigator/navigator_tests.rs` — 24, `offstage_measurement_tests.rs` — 7, серия `hero_*_tests.rs` — 19 суммарно).

Реальные production-места с `SeqCst` немногочисленны:

| Файл | Кол-во | Оценка |
|---|---:|---|
| `crates/flui-semantics/src/binding.rs` | 8 | Наиболее весомое; требует ревизии |
| `crates/flui-widgets/src/interaction/dismissible.rs` | 7 | UI-состояние жеста |
| `crates/flui-platform/src/platforms/android/mod.rs` | 4 | Lifecycle-флаги платформы |
| `crates/flui-binding/src/lib.rs` | 3 | Binding-синглтон |
| `crates/flui-app/src/app/runner.rs` | 3 | Bootstrap |

**Вердикт: `SeqCst` в тестах — не дефект** (там он и должен стоять: цена нулевая, рассуждать не о чем). Дефект — **отсутствие документированного обоснования ordering в production-местах**: ни в одном из перечисленных файлов рядом с `SeqCst` нет комментария, почему более слабый ordering недостаточен.

### Portability: проверено

Постановка требует не предполагать, что `AtomicU64`/`AtomicUsize` одинаково доступны везде. Проверил `rustc --print cfg` по всем объявленным targets:

| Target | `target_has_atomic` 8/16/32/64/ptr |
|---|---|
| `wasm32-unknown-unknown` | все ✅ |
| `armv7-linux-androideabi` | все ✅ |
| `aarch64-linux-android` | все ✅ |
| `x86_64-pc-windows-gnu` | все ✅ |

**Вывод: для объявленного набора targets проблемы разрядности атомиков нет.** Придумывать здесь находку было бы натяжкой. `cfg(target_has_atomic)`-гейтов в коде нет, и сегодня они не нужны. Они понадобятся только при добавлении bare-metal target (`thumbv6m-*`), что не запланировано.

### Реальный wasm-риск (доказанный дефект)

`.cargo/config.toml` держит `+atomics,+bulk-memory,+mutable-globals` **закомментированными**. Следствие: на `wasm32-unknown-unknown` все 1729 атомиков компилируются в **неатомарные однопоточные операции**. Само по себе это корректно (потоков нет), но это означает, что вся синхронизационная логика на wasm **молча вырождается**, а код, который её предполагает — прежде всего `background_executor` (`crates/flui-platform/src/traits/platform.rs:177`) и `PlatformExecutor::spawn` (`:475`) — на wasm не имеет рабочей реализации. Поскольку wasm-контур проверяется только через `cargo check` (§19), это никогда не всплывает.

### Таблица по несущим атомикам

| Atomic | Назначение | Ordering | Почему достаточен | Portability fallback |
|---|---|---|---|---|
| `NEXT_IDENTITY: AtomicU64` (`flui-scheduler/src/scheduler.rs:92`) | Генерация identity планировщика | `Relaxed`/`SeqCst` — **не задокументировано** | Для монотонного счётчика без публикации данных достаточно `Relaxed` | Не требуется (все targets имеют atomic64) |
| `frame_count`, `janky_frame_count`, `skipped_frames: AtomicU64` (`scheduler.rs:341,343,353`) | Статистика кадров | — | Чистая телеметрия → `Relaxed` корректен | Не требуется |
| `first_frame_deferred_count: AtomicU32` (`flui-app/src/bindings/renderer_binding.rs:136`) | Счётчик отложенных первых кадров | — | Телеметрия → `Relaxed` | Не требуется |
| `AtomicRenderFlags: AtomicU32` (`flui-rendering/src/storage/flags.rs`) | Dirty-флаги render-узла | смешанный | **Требует ревизии**: флаги публикуют факт готовности данных → нужен `Acquire`/`Release`, не `Relaxed` | Не требуется |
| Генерационные ID (`flui-foundation/src/id.rs:983-1023`) | ABA-защита | — | Разобрано в основном аудите как сильная сторона | Не требуется |

**Не проверено:** overflow/wraparound генерационных счётчиков, false sharing, contention — требуют замеров, которых нет.

---

## 18. Assert-based invariant testing

### Состояние

- `assert_matches!` — **0 использований**, и использовать его нельзя: макрос **не стабилизирован**. Проверено прямой компиляцией на обоих тулчейнах — `use std::assert_matches::assert_matches;` даёт `E0432: could not find assert_matches in std` и на 1.96.1, и на 1.97.1. (Некоторые сводки release notes ошибочно относят его к стабилизациям 1.96 — это не так.) Пример из постановки требует nightly; рекомендация ниже строится на `matches!` + `debug_assert!`.
- `proptest` / `quickcheck` / `arbitrary` — **0 зависимостей** (ни в `[workspace.dependencies]`, ни в крейтах).
- fuzz-таргеты — **0** (каталогов `fuzz/` нет).
- `debug_assert` — используется, но неравномерно; лидеры: `flui-engine/src/wgpu/state_stack.rs` (20), `flui-reactivity/src/runtime.rs` (17), `flui-widgets/src/navigator/history.rs` (13), `flui-view/src/owner/build_owner.rs` (11).

В дереве **~29 enum'ов состояний**, включая: `Lifecycle`, `TickerState`, `TickerFutureState`, `SchedulerPhase`, `FramePhase`, `AppLifecycleState`, `AnimationStatus`, `GestureRecognizerState`, `WidgetState`, `ViewFocusState`, `ConnectionState`, `LoadState`, `WindowState`, `AnimatedSizeState`, и 8 фазовых enum'ов распознавателей жестов (`TapState`, `DragPhase`, `ScalePhase`, `LongPressPhase`, `DoubleTapPhase`, `MultiTapPhase`, `ForcePressPhase`, `Phase` в `tap_and_drag`).

### Главная находка (доказанный дефект)

**Гарды переходов `Lifecycle` объявлены, но не применяются ни в одном месте кода.**

`crates/flui-view/src/element/lifecycle.rs:74-84` определяет `can_activate()` (истинно только для `Inactive`) и `can_deactivate()` (истинно только для `Active`). Поиск по всему дереву показывает: **эти два предиката вызываются исключительно в тестах** (`crates/flui-view/tests/lifecycle_tests.rs:111-126` и юнит-тест в самом `lifecycle.rs:99-106`). В production-коде используется только `can_build()` — в двух местах (`flui-view/src/owner/build_owner.rs:653`, `flui-view/src/element/generic.rs:483`).

Все 12 присваиваний состояния — безусловные:

```rust
// crates/flui-view/src/element/generic.rs:305-313
pub fn activate(&mut self) {
    self.lifecycle = Lifecycle::Active;   // ← ни гарда, ни debug_assert
    tracing::debug!(...);
}
```

То же в `generic.rs:291` (`unmount` → `Defunct`), `generic.rs:321` (`deactivate` → `Inactive`), `view/root.rs:189,264,268,272`, `view/error.rs:292,296,300,304`.

**Следствие: переход `Defunct → Active` — ровно тот, который постановка перечисляет как недопустимый («Destroyed → Mounted») — представим и достижим.** `activate()` публичный и восстановит в `Active` элемент, чей state уже disposed. Документация типа (`lifecycle.rs:10`) обещает `Initial → Active ⇄ Inactive → Defunct`; код это не обеспечивает.

Это **не гипотетическая** дыра: `Defunct` по контракту означает «state has been disposed» (`lifecycle.rs:44`), поэтому реактивация — работа с освобождённым состоянием.

### Чего нет

Постановка требует для каждого state machine: таблицу переходов, property-based тесты, negative-тесты, fuzz-таргет, debug-assertions, release-safe валидацию на публичных границах.

| Требование | Статус по `Lifecycle` | По остальным ~28 enum'ам (не проверялись поштучно) |
|---|---|---|
| Таблица допустимых переходов | Есть в docstring, не в коде | Как правило отсутствует |
| Property-based тесты | Нет (нет proptest) | Нет |
| Negative-тесты | Частично: `lifecycle_tests.rs:112,126` проверяют предикаты, но **не** то, что запрещённый переход отвергается | Нет |
| Fuzz-таргет | Нет | Нет |
| Debug assertions | Нет | Разрознены |
| Release-safe валидация на публичной границе | Нет — `activate()` публичный и без проверки | Нет |

### Рекомендация

Минимальное изменение с максимальным эффектом — превратить предикаты из декоративных в несущие:

```rust
pub fn activate(&mut self) {
    debug_assert!(
        self.lifecycle.can_activate(),
        "BUG: activate() from {:?}; only Inactive may be reactivated",
        self.lifecycle
    );
    self.lifecycle = Lifecycle::Active;
}
```

Это соответствует `docs/PANIC-POLICY.md` (`expect`/`assert` с префиксом `BUG:` для внутренних инвариантов) и не меняет release-поведение. Для публичных границ — возврат `Result`, а не `debug_assert`.

Добавление `proptest` (переходы как последовательность операций, инвариант «`Defunct` — поглощающее состояние») закрывает требование property-тестов дёшево — это ~30 строк на enum.

---

## 19. WebAssembly linking и portability

### Что проверяется сейчас

`justfile` (рабочее дерево) и CI-джоб `wasm-check` выполняют **только**:

```bash
cargo check --workspace --locked --target wasm32-unknown-unknown \
  --exclude flui-assets --exclude flui-build --exclude flui-cli \
  --exclude flui-web-server --exclude hot-reload-counter-{host,logic,types}
```

**Доказанный дефект (архитектурный).** `cargo check` **не линкует**. Все требования постановки, относящиеся к линковке — undefined symbols, случайные импорты из модуля `env`, некорректные `extern`-декларации, linker arguments, список imports/exports модуля — принципиально **не проверяются** и не могут быть проверены этой командой. Заявка «web platform claim, machine-checked» (комментарий `ci.yml:588`) в этой части не обеспечена.

Я выполнил недостающий шаг:

```
cargo build --locked --target wasm32-unknown-unknown -p flui-view -p flui-rendering -p flui-engine
→ 0 errors, 2 warnings (148 crates)
```

Сборка библиотек проходит. Но это по-прежнему `rlib` — настоящей линковки (`cdylib`/бинарь) в проверке нет ни у меня, ни в CI.

### Доказанный дефект: джоб `wasm-check` красный ещё до первого запуска

Джоб `wasm-check` **отсутствует в закоммиченном `ci.yml`** (проверено: `git show HEAD:.github/workflows/ci.yml` — ноль вхождений `wasm`) и существует только в рабочем дереве. Именно поэтому CI на `main` зелёный (последние прогоны: `30122851253`, `30120424714`, `30046231837` — все ok).

При этом workflow задаёт глобальный `RUSTFLAGS: -D warnings` (`ci.yml:42`), а джоб `wasm-check` его не переопределяет. Воспроизвёл:

```
$ RUSTFLAGS="-D warnings" cargo check --locked --target wasm32-unknown-unknown -p flui-scheduler
error: unused import: `Listener`
   --> crates/flui-scheduler/src/ticker.rs:962:29
error: unused variable: `listener`
    --> crates/flui-scheduler/src/ticker.rs:1147:13
→ 2 errors
```

Причина — `crates/flui-scheduler/src/ticker.rs:1155-1162`:

```rust
#[cfg(not(target_arch = "wasm32"))]
{
    listener.wait();
}
callback();
```

На wasm32 блок исчезает, `listener` (`:1147`) и импорт трейта `Listener` (`:962`) становятся неиспользуемыми. **На host-таргете сборка чистая** (проверил: `RUSTFLAGS="-D warnings" cargo check -p flui-scheduler` → успех). Это классический target-conditional класс предупреждений, который ловится только на соответствующем таргете.

**Вывод: `wasm-check` упадёт при первом же прогоне.** Починка тривиальна (`let _listener = ...` + `#[cfg_attr(target_arch = "wasm32", allow(unused_imports))]` или перенос импорта под cfg), но находка важна как индикатор: wasm-контур ни разу не проходил свой собственный гейт.

### Не проверяется вообще

| Требование постановки | Статус |
|---|---|
| undefined symbols | **не проверяется** (нет линковки) |
| случайные imports из `env` | **не проверяется** |
| список imports/exports модуля в CI | **отсутствует** |
| `wasm-bindgen` compatibility | нет зависимости `wasm-bindgen` в workspace |
| threading assumptions | **дефект**: `background_executor`/`PlatformExecutor::spawn` не имеют wasm-реализации (§17) |
| atomics | `+atomics` выключен (`.cargo/config.toml`, закомментировано) |
| blocking I/O | **дефект**: `listener.wait()` исключён через `cfg`, но `when_complete_or_cancel` документирован как блокирующий (`ticker.rs:1133-1135`) — на wasm он молча вырождается в немедленный вызов callback, меняя семантику |
| filesystem assumptions | crates с `mio`/`uuid` исключены — корректно |
| native dynamic libraries | `flui-hot-reload` (dlopen) исключён — корректно |
| windowing abstraction | `crates/flui-platform/src/platforms/web/platform.rs` существует, но обрабатывает лишь `Created` и `RedrawRequested` |
| timers, clipboard, drag-and-drop, IME, accessibility bridge | **отсутствуют для web** |

**Требование постановки «Undefined symbol нельзя автоматически превращать в допустимый WebAssembly import»** выполнить сейчас невозможно: списка импортов нет, потому что нет линковки.

### Минимальная починка

1. Исправить два предупреждения в `ticker.rs` — иначе гейт не стартует.
2. Заменить `cargo check` на `cargo build` хотя бы для одного `cdylib`-таргета (`web_demo`), чтобы линковка реально происходила.
3. Добавить в CI проверку списка импортов/экспортов (`wasm-tools print` / `wasm-objdump -x`) с закоммиченным ожидаемым списком — тогда «намеренный, типизированный, документированный, покрытый тестом» импорт станет проверяемым утверждением.

---

## 20. Cargo warnings policy

### Что есть

`Cargo.toml` `[workspace.lints]`:

```toml
[workspace.lints.rust]
unsafe_code = "warn"
missing_docs = "warn"
missing_debug_implementations = "warn"
rust_2018_idioms = "warn"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
unwrap_used = "warn"
# + 16 обоснованных `allow` с комментариями
```

**Сильная сторона:** каждое из 16 исключений в clippy-секции снабжено развёрнутым комментарием с технической причиной (графическая арифметика для `cast_*`/`float_cmp`, декларативный UI-идиом для `needless_pass_by_value`, и т.д.). Это ровно то, чего требует постановка от списка исключений — за вычетом ссылки на issue и условия удаления.

**Наследование:** 27 из 28 каталогов в `crates/` содержат `[lints] workspace = true`; наличие проверяется скриптом `scripts/check-workspace-inventory.sh` (джоб `checks`). Единственное исключение — `flui-reactivity` (ниже).

### Доказанный дефект: `RUSTFLAGS=-D warnings` как единственный механизм

Постановка прямо предупреждает против этого. FLUI делает именно так:

- `.github/workflows/ci.yml:42` — `RUSTFLAGS: -D warnings` на уровне workflow (все джобы);
- `.github/workflows/weekly.yml:27` — то же;
- `RUSTDOCFLAGS: -D warnings` (`ci.yml:43`).

Симптом проблемы уже проявился **дважды**, оба раза видны в самом репозитории:

1. **`ci.yml:518-523`** — джоб `miri` вынужден делать `RUSTFLAGS: ""` с комментарием «Override the workflow-level `-D warnings`: nightly deprecates APIs». То есть глобальный флаг пришлось локально отключить целиком — вместе со всеми настоящими проверками, а не только с мешающей.
2. **`wasm-check`** (§19) — джоб красный именно из-за взаимодействия глобального `RUSTFLAGS` с target-conditional предупреждениями.

Второе следствие, о котором говорит постановка, — **инвалидация build cache**. `RUSTFLAGS` входит в fingerprint единицы компиляции: джоб с `RUSTFLAGS=""` (miri) и джобы с `RUSTFLAGS="-D warnings"` **не могут переиспользовать артефакты друг друга**, а `Swatinem/rust-cache` кэширует их как разные ключи. На wgpu-workspace такого размера это заметная стоимость.

### Доказанный дефект: `flui-reactivity` вне всех политик

`crates/flui-reactivity/Cargo.toml`:

- **не член workspace** — `Cargo.toml:63-68`, строка `"crates/flui-reactivity"` закомментирована;
- `edition = "2021"` (остальные — 2024);
- `version = "0.1.0"` (остальные — 0.2.0);
- **нет** `[lints] workspace = true` — единственный крейт без наследования;
- **нет** `rust-version` — MSRV не декларирован.

При этом крейт содержит рабочий код (`src/runtime.rs` — 17 `debug_assert`, `src/async.rs`, `src/hooks/resource.rs`) и `BENCHMARK_RESULTS.md`. Он **не компилируется, не линтуется и не тестируется ни одним гейтом**. Это ~мёртвый крейт в дереве, который выглядит живым.

### Чего нет в политике

Постановка предлагает конкретный набор. Сверка:

| Линт | Статус в FLUI | Оценка |
|---|---|---|
| `unsafe_op_in_unsafe_fn = "deny"` | **отсутствует** | **Стоит добавить.** Edition 2024 уже делает это warn-by-default; явный `deny` фиксирует намерение |
| `unused_must_use = "deny"` | **отсутствует** (warn by default) | Стоит поднять до `deny` — в UI-коде проигнорированный `Result` от layout/paint молчалив |
| `unexpected_cfgs = "deny"` | **отсутствует** | **Стоит добавить.** В дереве много `cfg(target_arch)`/`cfg(feature)`; опечатка в cfg сегодня не ловится |
| `clippy::undocumented_unsafe_blocks = "deny"` | **отсутствует** | **Стоит добавить.** В дереве 24 инлайновых `#[allow(unsafe_code)]` + `#![allow(unsafe_code)]` в `flui-platform/src/lib.rs:153` и `flui-layer/src/tree/layer_tree.rs:981`. Без этого линта нет механической гарантии, что у каждого `unsafe` есть `SAFETY:` |
| `clippy::missing_safety_doc = "deny"` | **отсутствует** (входит в `clippy::all` как warn) | Поднять до `deny` |

Отмечу отдельно: **`unsafe_code = "warn"`, а не `"deny"`**. Учитывая, что `unsafe` в проекте локализован (`flui-platform` — весь крейт по документированному решению, `flui-rendering/subtree_arena` — покрыт miri), правильная форма — `deny` на workspace с точечными `allow` там, где он санкционирован. Сейчас граница между «санкционированным» и «просочившимся» `unsafe` не проверяется механически.

### Разделение классов предупреждений

Постановка требует разделять compiler warnings / clippy / linker / doc / generated / third-party. Сейчас FLUI разделяет только clippy (отдельный джоб) и doc (`RUSTDOCFLAGS`). Compiler warnings идут общим `RUSTFLAGS` со всеми последствиями выше; linker-сообщения не выделены.

**Рекомендация.** Перенести `-D warnings` из `RUSTFLAGS` в `[workspace.lints.rust]` (`warnings = "deny"` там недоступен, но конкретные группы — да) и/или использовать `CARGO_BUILD_WARNINGS=deny`, где поддерживается. Ключевое: политика должна жить в манифесте, где она не участвует в fingerprint как переменная окружения и не требует «выключить всё» ради одного джоба.

---

## 21. Symbol mangling и production diagnostics

### Доказанный дефект: release-сборка не диагностируема

`Cargo.toml` `[profile.release]`:

```toml
opt-level = 3
lto = "thin"
codegen-units = 1
strip = "symbols"     # ← таблица символов удаляется целиком
                      # ← `debug` не задан → debug = 0
```

Следствия, прямо отвечающие на вопросы постановки:

| Инструмент | Работает на release-сборке FLUI? |
|---|---|
| flamegraph / `perf` | **Нет** — стеки будут состоять из адресов |
| samply | **Нет** |
| Tracy / Instruments / WPA | **Нет** (символов нет) |
| crash reports / minidumps | **Нет** — не символизируются |
| stack traces | Только адреса |
| symbol servers | Нечего публиковать: нет ни символов, ни отдельного symbol-файла |
| binary-size analyzer / linker maps | **Нет** |

Это не компромисс «размер против диагностики» — это односторонний выбор в пользу размера без страховки. Индустриальная норма — `strip` в поставляемом артефакте **плюс** сохранённый отдельно symbol-файл (`split-debuginfo = "packed"` / `objcopy --only-keep-debug`). В `.cargo/config.toml` строка `split-debuginfo = "unpacked"` присутствует, но **закомментирована и относится к `[profile.dev]`**, а не к release — для release-диагностики её нужно вводить отдельно.

`-C symbol-mangling-version` не задан нигде (`Cargo.toml`, `.cargo/config.toml`, CI, justfile) — используется дефолт компилятора. Для 1.96/1.97 это `legacy`. Дефолт работоспособен, но `v0` даёт корректное отображение generic-параметров и путей крейтов — ровно то, что постановка требует проверить. Для кодовой базы с 29 573 мономорфизациями (ниже) разница в читаемости стеков существенна. **Оценка: риск, не дефект** — переход на `v0` полезен, но бессмыслен, пока символов в release нет вообще.

### Измеренная мономорфизация

Выполнено: `cargo llvm-lines -p flui-widgets --lib`

**Итого: 642 928 LLVM-строк, 29 573 мономорфизированные копии** в одном крейте.

| Символ | LLVM-строк | Копий | Строк/копию |
|---|---:|---:|---:|
| `flui_view::element::dispatch::dispatch_view_update` | 45 472 (7.1%) | 133 | **342** |
| `ElementCore<V,A>::set_pipeline_owner_any` | 29 792 (4.6%) | 133 | 224 |
| `RenderBehavior<V> as ElementBehavior<V,A>>::on_mount` | 18 912 (2.9%) | 61 | 310 |
| `ElementCore<V,A>::mount` | 16 663 (2.6%) | 133 | 125 |
| `RenderBehavior<V>::build_into_views` | 16 226 (2.5%) | 61 | 266 |
| `ElementCore<V,A>::set_parent_render_id` | 11 438 (1.8%) | 133 | 86 |
| `ElementCore<V,A>::activate` | 11 172 (1.7%) | 133 | 84 |
| `ElementCore<V,A>::deactivate` | 11 172 (1.7%) | 133 | 84 |
| `ElementCore<V,A>::unmount` | 11 172 (1.7%) | 133 | 84 |
| `ElementCore<V,A>::new` | 7 896 (1.2%) | 133 | 59 |

Топ-27 символов = 11.3% всех строк.

### Разбор причины

Множитель — **generic-триплет `Element<V, A, B>`** (View × Arity × Behavior). В `flui-widgets` он инстанцируется **133 раза**, и вместе с ним монотонно копируются ~10 методов жизненного цикла `ElementCore<V,A>`.

Ключевое наблюдение: **`activate`/`deactivate`/`unmount`/`mount`/`set_parent_render_id`/`set_pipeline_owner_any` — холодные пути.** Они вызываются при монтировании и перемещении узлов, не на каждом кадре. Их мономорфизация не даёт производительности, но платит кодом: только эти шесть — 88 133 LLVM-строки (13.7% крейта).

`dispatch_view_update` (342 строки на копию, самый крупный) — это горячий путь rebuild, и его основной аудит уже помечает как дефект по другой причине (K2: безусловные клоны). Здесь важно, что он ещё и самый дорогой по коду.

### Что делать (и чего не делать)

Постановка запрещает «исправлять переводом всего API на `Box<dyn Trait>`». Согласен — и это тем более верно здесь, потому что статическая диспетчеризация во View-API является частью контракта Arity-системы (`docs/PORT.md`, FR-036).

Сравнение вариантов, в порядке предпочтения:

| Вариант | Оценка |
|---|---|
| **Selective boxing холодной половины** | **Лучший.** Вынести lifecycle-методы (`mount`/`unmount`/`activate`/`deactivate`/`set_parent_render_id`/`set_pipeline_owner_any`) за `dyn`-границу, сохранив `build`/`layout`/`paint` статическими. Потенциально снимает ~14% кода крейта. `dyn`-границы в проекте уже санкционированы реестром FR-036 — потребуется добавить запись |
| Shared generic implementation | Хорошо: вынести тело метода в неgeneric-функцию, оставив generic-обёртку тонкой. Дёшево, не меняет API. Применимо к `set_pipeline_owner_any` (224 строки/копию при тривиальной семантике — подозрительно много) |
| Enum dispatch | Не подходит: множество View открыто для пользователя |
| Thin fn-pointer tables | Возможная альтернатива boxing, но выигрыш тот же при большей сложности |
| codegen-units / LTO | Уже настроены агрессивно (`codegen-units = 1`, `lto = "thin"`); резерва нет |
| Полный `Box<dyn View>` | **Запрещено** port-check'ом (триггер: `Box<dyn View>` как поле структуры) |

**Не измерено:** реальный размер бинаря и вклад в него (нужен `cargo bloat --release`), а также влияние на время компиляции. Это следующий шаг, а не вывод.

---

## 22. Современная data-transfer архитектура

### Состояние: clipboard есть, drag-and-drop отсутствует полностью

**Clipboard.** `crates/flui-platform/src/traits/platform.rs:484`:

```rust
pub trait Clipboard: Send + Sync {
    fn read_text(&self) -> Option<String>;
    fn write_text(&self, text: String);
    fn has_text(&self) -> bool { self.read_text().is_some() }
}
```

Плюс `ClipboardItem` (`:502`) с полями `text: Option<String>` и `metadata: Option<String>` — где `metadata` документирована как «source application, MIME type hints», то есть **MIME смоделирован строкой без структуры**. Доступ идёт через `Platform::clipboard()` (`:236`) и `AppBinding` (ADR-0034, `crates/flui-app/src/app/binding.rs`, `Arc<Mutex<Option<Arc<dyn Clipboard>>>>`).

**Drag-and-drop.** Поиск по всему дереву (`DragAndDrop|drag_and_drop|FileDropped|DroppedFile|HoveredFile|DragEnter|drop_file`, `--type rust`) — **ноль совпадений**. В winit-бэкенде обрабатываются 20 вариантов `WindowEvent` (`crates/flui-platform/src/platforms/winit/platform.rs:424-629`), среди них **нет** `DroppedFile`/`HoveredFile`/`HoveredFileCancelled`. Слова «drag» в UI-крейтах относятся к жестам перетаскивания внутри приложения (`Draggable`, `Dismissible`, `DragPhase`), а не к системному DnD.

Постановка предупреждает: «не моделируй drag-and-drop только как `Event::FileDropped(PathBuf)`». FLUI не моделирует его **никак** — это состояние хуже описанного.

### Оценка текущей модели clipboard против требований

| Требование | Статус |
|---|---|
| plain text | ✅ |
| HTML, rich text, URI list, images, custom MIME, platform-native | ❌ |
| несколько представлений одного объекта | ❌ (одно поле `text`) |
| lazy payload | ❌ — `read_text()` возвращает `Option<String>` немедленно |
| non-blocking | ❌ — синхронный `&self -> Option<String>` |
| event-loop aware | ❌ (§23) |
| cancellable | ❌ — нечего отменять |
| ограничение по размеру | ❌ — `String` любого размера |
| защита от stale IDs | ❌ — нет ID предложения |
| запрос только выбранного представления | ❌ |
| совместимость Wayland/X11/Windows/macOS/web | Частично: Windows/macOS/winit(`arboard`); web — нет |

Синхронный `read_text()` особенно проблематичен для **Wayland и X11**, где чтение буфера — это асинхронное согласование с другим клиентом через pipe. Реализация поверх `arboard` скрывает это блокировкой, что на UI-потоке означает потенциальное залипание кадра. **Риск** (не измерено, но архитектурно неизбежно).

### Требуемая архитектура

Постановка задаёт разделение на 7 стадий (offer → negotiation → request → async delivery → decoding → drop action → completion/cancellation). Предложенная в постановке модель (`DataTransferOffer`, `TransferType`) применима к FLUI напрямую, с двумя привязками к его специфике:

1. **Generational ID.** У FLUI уже есть проверенный механизм ABA-защиты (`crates/flui-foundation/src/id.rs:983-1023`, отмечен в основном аудите как сильная сторона). `DataTransferId` должен быть построен на нём — тогда «истечение generational ID» и «stale offer» получают ту же гарантию, что и остальные ID проекта, бесплатно.
2. **Доставка через уже существующий async-контур.** `TaskToken` с отменой по drop (`crates/flui-scheduler/src/async_driver.rs:135-167`) — готовый механизм для «cancellable, non-blocking delivery». Изобретать второй не нужно.

Общий транспорт для clipboard и DnD при разной высокоуровневой семантике (как требует постановка) ложится на эту же пару: один `DataTransferOffer` + разные фасады (`ClipboardOffer` без drop-action, `DragOffer` с `TransferActions`).

**Не проверено:** поведение при закрытии source-окна во время передачи, timeout, backpressure, streaming больших файлов, security boundary для недоверенного внешнего ввода — проверять нечего, механизма нет.

---

## 23. Event-loop affinity

### Доказанный дефект: модели affinity нет

`Platform` (`crates/flui-platform/src/traits/platform.rs`) объявляет ~30 методов. **Все принимают `&self`.** Ни один не требует токена главного потока, активного event loop или capability-объекта:

| Метод | Строка | Требует main thread / активный loop на реальных ОС |
|---|---:|---|
| `open_window(&self, …)` | 213 | **Да** (AppKit — строго; Win32 — привязка к потоку сообщений) |
| `clipboard(&self)` | 236 | **Да** на macOS/Wayland |
| `write_to_clipboard` / `read_from_clipboard` | 279 / 287 | **Да** |
| `displays(&self)` / `primary_display` | 228 / 231 | **Да** на macOS |
| `set_cursor_style(&self, …)` | 272 | **Да** |
| `activate` / `hide` / `hide_other_apps` | 244 / 249 / 252 | **Да** (AppKit) |
| `prompt_for_paths` / `prompt_for_new_path` | 313 / 321 | **Да** |
| `window_appearance` | 260 | **Да** |
| `quit(&self)` | 205 | **Да** |

**Проверка на защиту:** поиск `main_thread|is_main_thread|MainThread|event_loop_thread` по `crates/flui-platform/src` и `crates/flui-app/src` — **ноль совпадений**. Ни assert'а, ни debug-assert'а, ни runtime-проверки.

Поскольку `Platform` раздаётся как `Arc<dyn Platform>` и `Clipboard: Send + Sync` (`:484`), **любой worker-поток может вызвать любой из этих методов**. На macOS это UB-класс ошибок (AppKit вне main thread), на Windows — молчаливо неверная привязка окна к потоку. Ровно то, что постановка запрещает: «Не позволяй platform API неявно вызываться из произвольного worker thread».

### Что в проекте уже есть в правильном направлении

Не всё плохо: `Platform` объявляет **два исполнителя** (GPUI-подобная схема):

```rust
fn background_executor(&self) -> Arc<dyn PlatformExecutor>;   // :177
fn foreground_executor(&self) -> Arc<dyn PlatformExecutor>;   // :182
```

и `PlatformExecutor::is_on_executor()` (`:478`). То есть **механизм для command-channel существует**, но:

- он **необязателен** — ничто не заставляет проходить через `foreground_executor`;
- `is_on_executor()` имеет дефолтную реализацию и нигде не проверяется на входе в platform-методы;
- типовой гарантии нет: `&self` не отличает вызов с UI-потока от вызова с worker'а.

Также существует `UiRealm` (ADR-0027, «owner-affine UI realms») — модель владения, которая концептуально и есть нужный контейнер affinity. Но она регулирует владение деревьями, а не доступ к platform API.

### Рекомендация

Постановка предлагает два варианта; для FLUI подходит **комбинация**, и она дешевле, чем кажется, потому что обе половины уже есть:

1. **Capability-объект** для методов, которые обязаны идти с UI-потока:
   ```rust
   pub struct ActivePlatformContext<'event_loop> { /* !Send + !Sync */ }
   ```
   `!Send`-маркер даёт **компиляторную** гарантию вместо runtime-проверки. Методы `open_window`, `clipboard`, `set_cursor_style`, `displays` переезжают на него.
2. **Command channel** для всего, что инициируется из worker'а — поверх существующего `foreground_executor`.

Открытые вопросы, которые постановка требует проверить и которые **сейчас не адресованы ничем**: порядок команд, повторный вход в event loop, команды от уничтоженных узлов, отмена, результат для устаревшего кадра, идентичность окна. Последнее частично закрыто `WindowId` + генерационными ID; остальное — нет.

**Отдельно (положительно):** глобального platform-синглтона нет — `Platform` резолвится через `AppBinding` и ADR-0027-реалмы. Требование «отсутствие глобального platform singleton» **выполнено**.

---

## 24. Async API: настоящее и будущее Rust

### Классификация текущего состояния

**Stable today — всё, на чём построен FLUI:**

| Механизм | Где | Оценка |
|---|---|---|
| Явные `Future` + `Pin<Box<dyn Future>>` (`BoxedTask`) | `flui-scheduler/src/async_driver.rs` | ✅ stable |
| Cancellation token с отменой по drop | `async_driver.rs:135-167`, `TaskToken::cancel` (`:156`), `Drop` (`:167`) | ✅ Сильное решение |
| `#[must_use = "dropping the TaskToken immediately cancels the task"]` | `scheduler.rs:1082,1091`, `async_driver.rs:221` | ✅ Ошибку «забыл handle» ловит компилятор |
| Scoped runtime-owned task handles | `spawn_local` / `spawn_local_eager` (`scheduler.rs:1083,1092`) | ✅ |
| Генерационные гейты для результатов | отмечено в основном аудите | ✅ |
| Command/event channels | `crossbeam`, drain-циклы | ✅ |
| async closures / async fn | `flui-assets`, `flui-build`, `flui-reactivity` | ✅ stable |

**Experimental — не используется ничего.** Native async-trait dispatch, RTN, TAIT, новый trait solver, immovable types, guaranteed destructors — **0 вхождений**. `async_trait` (крейт-обходной путь) — тоже **0**.

**Forbidden dependency — нарушений нет.** Публичный стабильный API не требует ни одной незавершённой возможности языка.

`tokio` 1.43 присутствует (`Cargo.toml:113`), но локализован: `flui-assets`, `flui-build`, `flui-reactivity` (опционально, через feature `async`), `flui-cli`. **UI-ядро (view/rendering/scheduler/app) на tokio не завязано** — оно использует собственный кооперативный `async_driver`. Это правильное разделение: UI-кадр не должен зависеть от многопоточного рантайма.

### Ответы на 10 вопросов постановки

| # | Вопрос | Ответ |
|---:|---|---|
| 1 | Работает на stable? | **Да**, полностью |
| 2 | Требует `Send`? | `BoxedTask` — да; `spawn_local` подразумевает локальность, но тип `BoxedTask` требует уточнения. **Не проверено детально** |
| 3 | Может работать с `!Send` future? | По названию `spawn_local` — предполагается да; **требует проверки сигнатуры** |
| 4 | Нужен dynamic dispatch? | Да, `Pin<Box<dyn Future>>` — на границе стирания, что постановка разрешает |
| 5 | Где происходит allocation? | При `Box::pin` на `spawn_local` — одна аллокация на задачу |
| 6 | Кто отменяет future? | Владелец `TaskToken`; отмена по drop **и** явная (`cancel()`) |
| 7 | Гарантируется ли destructor? | **Нет** — как и везде в Rust. `mem::forget(token)` оставит задачу жить. Это не дефект FLUI, а свойство языка; важно, что API не *зависит* от гарантии |
| 8 | Можно забыть task handle? | `#[must_use]` предупреждает, но `let _ = token;` обойдёт. **Риск, малый** |
| 9 | Что при unmount? | Основной аудит отмечает структурную отмену через `TaskToken` + генерационные гейты как работающую. Отдельно **не перепроверялось** в этом аудите |
| 10 | Безопасно ли применить результат к новой версии UI? | Да — генерационные гейты для того и введены |

### Точки расширения на будущее

Постановка требует не проектировать в ожидании будущего Rust, но заложить точки расширения. Текущая форма для этого удобна: раз задачи стираются в `Pin<Box<dyn Future>>` на одной границе (`async_driver`), появление native async-trait dispatch позволит убрать боксинг **в одном месте**, не трогая потребителей. Это хорошая позиция; менять ничего не нужно.

**Вердикт §24: раздел в хорошем состоянии — один из двух (наряду с §14), где нет находок по существу.**

---

## 25. Современный конкурентный baseline

Матрица заполнена по коду. «Реализовано» ≠ «есть публичный тип» — там, где есть только тип или пример, это отмечено.

| Capability | Реализовано | Качество API | Runtime overhead | Diagnostics | Production ready |
|---|---|---|---|---|---|
| Typed builder API | ✅ | Высокое (Arity-система) | **Высокий по коду** — 29.5k мономорфизаций (§21) | — | Да |
| Custom elements | ✅ | Высокое | — | — | Да |
| Component diffing | ✅ | Среднее | **Высокий** — безусловные клоны (K2) | нет | Нет |
| Keyed reconciliation | ✅ GlobalKey/LocalKey, 139 файлов | Высокое | Средний | нет | Да, но H4 (устаревшая глубина при reparent) |
| Virtualized lists | ✅ настоящая ленивая виртуализация | Высокое | Низкий | нет | Да |
| Text editing primitives | ⚠️ `EditableText`, `TextEditingController` (24 файла) | Среднее | **Высокий** — K3 (двойной shaping, глобальный мьютекс) | нет | Нет |
| Multi-window | ✅ `UiRealm`, ADR-0027 (75 файлов) | Высокое (leapfrog-зона) | — | нет | Частично |
| **System tray** | ❌ **0 файлов** | — | — | — | Нет |
| **Drag-and-drop (системный)** | ❌ **0 файлов** (§22) | — | — | — | Нет |
| Routing | ✅ Navigator, ADR-0019/0020/0024/0025 (243 файла) | Высокое | Средний | нет | Да |
| Shared state | ✅ Inherited + точные dependent-множества | Высокое | Средний (H5 — утечка dependents) | нет | Частично |
| Animations | ✅ | Высокое | — | нет | Да |
| Shaders | ✅ ShaderMask, WGSL (104 файла) | Среднее | — | `lyon-debugger`, `gpu-profiler` (features) | Частично |
| **Accessibility** | ⚠️ дерево семантики есть; **платформенного моста нет** (`accesskit` — 0 файлов) | — | **Высокий** — H6 (полная пересборка) | нет | **Нет** |
| **Performance overlay** | ⚠️ слой реализован, **не подключён** (§27) | — | — | — | **Нет** |
| Profiler integration | ⚠️ `flui-devtools::profiler` изолирован (§26) | Низкое | — | — | Нет |
| Hot reload | ⚠️ dlopen, host-only (§28) | Среднее | — | Частично | Нет |
| Devtools | ❌ не подключены к дереву (§26) | — | — | — | **Нет** |
| Custom renderer backends | ✅ `CommandRenderer`/`LayerRender` (23 файла) | Высокое | — | — | Да |
| Android / mobile | ⚠️ 104 файла, `cargo-ndk`, вне default-members | Среднее | — | нет | Нет |

**Три пробела, которых нет ни у одного зрелого конкурента:** системный drag-and-drop, system tray, платформенный a11y-мост. Первые два — отсутствие функции; третий — отсутствие моста при наличии всей внутренней машинерии, что делает пробел особенно заметным.

---

## 26. Devtools как часть архитектуры

### Доказанный дефект: devtools физически не могут наблюдать дерево

`crates/flui-devtools/Cargo.toml` — секция `[dependencies]` целиком:

```toml
web-time = "1.1"
serde, serde_json
flui-hot-reload = { optional = true }     # ← единственная зависимость на flui-*
parking_lot
windows-sys                               # target: windows
```

И явное подтверждение в документации крейта (`src/lib.rs:93-94`):

> **Note**: This crate has NO dependency on `flui_core` to avoid circular dependencies. Widget inspection is available through separate tools.

«Separate tools» не существуют. Крейт содержит 5 модулей общим объёмом ~1 580 строк: `profiler.rs` (614), `timeline.rs` (619), `hot_reload.rs` (202), `common.rs` (91), `lib.rs` (152). Это самостоятельный таймер кадров, не инспектор.

Сверка со списком постановки — что devtools должны позволять наблюдать:

| Объект наблюдения | Доступен |
|---|---|
| logical tree, state scopes, dependency graph, layout tree, render tree, semantics tree, focus tree, event path, dirty flags, active animations, async tasks, renderer resources, cache entries, platform commands | ❌ **все — нет** |
| frame timeline | ⚠️ есть в `timeline.rs`, но питается вручную, а не из планировщика |

Ни одно из 16 полей на узел, которые постановка требует от инспектора (Node ID, Generation, Logical/Layout/Render parent, Component type, State version, Dependencies, Last rebuild/layout/repaint reason, Memory footprint, Active tasks, Event handlers, Accessibility role) — **не доступно**, потому что нет доступа к дереву.

### Доказанный дефект: документация обещает несуществующее

`src/lib.rs:25-38` рекламирует три раздела возможностей:

```
## 🌐 Network Monitor (feature: network-monitor)
## 💾 Memory Profiler (feature: memory-profiler)
## 🔌 Remote Debug (feature: remote-debug)
```

Соответствующие модули **закомментированы** (`lib.rs:102-110`), features — закомментированы в `Cargo.toml` («TODO: Add dependencies for these features»), re-export'ы в prelude — закомментированы (`lib.rs:139-141`). Крейт-документация описывает три подсистемы, которых нет. Раздел `## Feature Flags` (`lib.rs:87-90`) перечисляет их как доступные.

Это прямо противоречит правилу «Definition of Done» из `AGENTS.md` («No fake-passing… If a behavior is not implemented, say so explicitly»).

### Release overhead: требование выполняется, но по неинтересной причине

| Требование постановки к release без devtools | Статус |
|---|---|
| нет открытого порта | ✅ (remote-debug не существует) |
| нет фонового сервера | ✅ (по той же причине) |
| нет string-heavy metadata | ✅ |
| нет регулярного polling | ✅ |
| overhead нулевой | ✅ |

`default = []` — все три реальные features (`profiling`, `timeline`, `hot-reload`) выключены по умолчанию, plugin-граница через feature-flag присутствует. **Архитектурно граница выбрана верно** — проблема в том, что за ней ничего нет.

Бенчмарков из постановки (release без devtools / release с выключенным tracing / debug с отключённым inspector / debug с подключённым) — **нет**; `[[bench]] profiler_bench` закомментирован в `Cargo.toml`.

### Рекомендация

Обоснование «NO dependency on flui_core to avoid circular dependencies» — реальная проблема, но у неё есть стандартное решение, которое в проекте уже применяется в других местах: **инверсия зависимости**. Ядро публикует наблюдения через узкий trait (`trait TreeObserver` / канал событий), объявленный в низкоуровневом крейте (`flui-foundation`); devtools **подписывается**, а не импортирует ядро. Цикла не возникает, и `#[cfg(feature)]`-гейт сохраняет нулевой overhead.

До этого шага корректнее убрать из документации крейта три несуществующих раздела.

---

## 27. Performance overlay

### Состояние: реализовано на 80%, не подключено

Что **есть**:

| Компонент | Расположение | Состояние |
|---|---|---|
| `PerformanceOverlayLayer` | `crates/flui-layer/src/layer/performance_overlay.rs:258` | Полноценный тип; `PerformanceOverlayOption` (`:136`) — битовая маска |
| Вариант дерева слоёв | `flui-layer/src/layer/mod.rs:212`, `:302` (`always needs compositing`) | Интегрирован |
| Отрисовка | `flui-engine/src/wgpu/layer_render.rs:437-451` → `renderer.add_performance_overlay(...)` | Интегрирована |
| **Реальная реализация в wgpu-бэкенде** | `flui-engine/src/wgpu/backend.rs:1342-1400+` | **Настоящая**: полупрозрачный фон, RRect, цветные метки, текст (MangoHud-стиль) |
| Конфиг-флаг | `flui-app/src/app/config.rs:87` `show_performance_overlay: bool`, builder `:185` | Есть |
| Cargo-features | `flui-app/Cargo.toml:25-26` `debug-overlay`, `performance-overlay` | Есть |

Чего **нет** (доказанный дефект):

1. **`show_performance_overlay` никогда не читается.** Полный поиск по `crates/`, `examples/`, `src/` даёт три вхождения — объявление (`config.rs:87`), инициализация в `Default` (`:116`) и запись в builder'е (`:186`). **Ни одного чтения.** Флаг мёртв.
2. **Ничто не конструирует `PerformanceOverlayLayer`** в кадровом пайплайне. Слой достижим только если пользователь соберёт его вручную.
3. **Features `debug-overlay` и `performance-overlay` пустые** (`= []`) и ничего не гейтят — ни одного `#[cfg(feature = "performance-overlay")]` в `flui-app`.

Итог: цепочка «конфиг → сборка слоя → отрисовка» разорвана ровно в одном звене — между конфигом и построением слоя. Отрисовка готова.

### Полнота метрик против требований

Постановка перечисляет 19 метрик. `PerformanceOverlayLayer` несёт **три**: `fps`, `frame_time_ms`, `total_frames` (сигнатура `add_performance_overlay`, `flui-engine/src/traits.rs:362-369`).

Отсутствуют: build time, reconciliation time, layout time, text shaping time, paint time, compositing time, GPU submit time, present latency, число перестроенных / пере-layout'ленных / перерисованных узлов, display-list commands, draw calls, allocations per frame, bytes per frame, dirty queue size, active async tasks, cache hit ratio.

Статистика тоже сведена к текущему значению: **нет** moving average, p50, p95, p99, worst frame, счётчика пропущенных deadline'ов. Постановка отдельно предупреждает «не ограничивайся FPS — FPS скрывает причину задержки»; текущая модель — ровно FPS плюс среднее время кадра.

Частичный источник данных существует: `flui-scheduler/src/scheduler.rs:341-353` считает `frame_count`, `janky_frame_count`, `skipped_frames`. Это ещё не перцентили, но `janky_frame_count` и `skipped_frames` — уже ближе к «причине задержки», чем FPS, и не выведены никуда.

### Рекомендация

Дешёвый шаг с непропорционально большим эффектом: связать три существующих конца — читать `show_performance_overlay`, строить слой в конце построения кадра, питать его из счётчиков планировщика. Это не новая подсистема, а ~50 строк соединительного кода. Расширение набора метрик до полного списка — отдельная работа, которая осмысленна только после K1 основного аудита (пока paint неинкрементален, «число перерисованных узлов» всегда равно числу всех узлов).

---

## 28. Hot reload и state preservation

### Состояние

Две отдельные реализации:

| Крейт | Объём | Механизм |
|---|---:|---|
| `flui-hot-reload` | ~1 850 строк (`dynlib.rs`, `host.rs`, `driver.rs`, `engine.rs`, `plugin.rs`, `strategy.rs`, `worker.rs`, `dispatch.rs`, `pipeline.rs`, `dev/source_watch.rs`) | **dlopen** динамических библиотек |
| `flui-devtools::hot_reload` | 202 строки | Наблюдение за файлами (`HotReloader`), feature `hot-reload` |

Документация: `docs/hot-reload.md` (8.1K). Тесты: `crates/flui-hot-reload/tests/loader.rs` (123 строки). Пример: `hot-reload-counter-{host,logic,types}` — три workspace-члена, **исключены** из wasm-контура как «dlopen-based hot-reload, host-only by design» (`ci.yml:604-605`).

### Сверка с требованиями постановки

Постановка требует разделять шесть классов изменений. Формально в проекте есть `strategy.rs` (101 строка), но соответствие классам не выражено:

| Класс изменения | Поддержан? |
|---|---|
| 1. обновление тела функции | ✅ (основной сценарий dlopen) |
| 2. обновление декларативного дерева | ⚠️ через перезагрузку логики целиком |
| 3. обновление style/resource | ❌ не выделено |
| 4. изменение типа | ❌ |
| 5. изменение ABI | ❌ — **и это опасно** (ниже) |
| 6. изменение layout памяти | ❌ — **и это опасно** |

**Риск (высокий).** dlopen-механизм передаёт данные между host и перезагруженной библиотекой. Если между сборками изменится `#[repr]`/layout типа из `hot-reload-counter-types`, host будет интерпретировать байты по старой раскладке. Rust не даёт стабильного ABI для `repr(Rust)`, поэтому **любое** изменение типа — включая добавление поля или смену порядка — потенциально меняет layout. Проверки совместимости (версионный хеш layout'а, `TypeId`-сверка на границе) я в коде не нашёл; для утверждения об их отсутствии нужен построчный разбор `dynlib.rs`/`plugin.rs`, который в этот аудит не входил — поэтому это **риск, а не доказанный дефект**.

Постановка предупреждает: «Не обещай state-preserving hot reload там, где изменение типа делает его небезопасным». `flui-devtools/src/lib.rs:23` обещает: «## 🔥 Hot Reload — Watch file changes / Trigger rebuilds automatically / **State preservation**». Механизма сохранения состояния при перезагрузке кода в `flui-devtools` нет — там только file watcher. Найденные в дереве упоминания «state preserved» относятся к **другому** механизму: сохранению состояния при GlobalKey-reparent (`flui-view/tests/global_key_reparent.rs:312`, `global_key.rs:400`) и при `Visibility`/`Offstage` (`flui-widgets/tests/visibility.rs:627`). Это не hot reload.

**Вывод: обещание state-preserving hot reload в документации не обеспечено кодом.**

### Не проверено

Отмена старых задач при перезагрузке, сохранение GPU-ресурсов, rollback неудачного патча, диагностика — требуют отдельного разбора `flui-hot-reload`, который в объём этого аудита не входил. Бенчмарки §30 (100 патчей подряд, рост памяти) отсутствуют.

---

## 29. Feature architecture

### Инвентаризация

| Крейт | Features |
|---|---|
| `flui-animation` | `default`, `serde` |
| `flui-app` | `default`, `desktop`, `android`, `ios`, `web`, `debug-overlay`, `performance-overlay` |
| `flui-assets` | `default`, `images`, `network`, `full` |
| `flui-cli` | `default`, `devtools` |
| `flui-devtools` | `default`, `profiling`, `timeline`, `hot-reload`, `full` |
| `flui-engine` | `default`, `wgpu-backend`, `vulkan`, `metal`, `dx12`, `webgpu`, `gles`, `images`, `assets`, `serialization`, `lyon-debugger`, `gpu-profiler`, `enable-wgpu-tests` |
| `flui-foundation` | `default`, `serde`, `pretty` |
| `flui-geometry` | `default`, `serde`, `mint`, `kurbo`, `full` |
| `flui-hot-reload` | `default`, `app-plugin`, `source-watch` |
| `flui-interaction` | `default`, `testing`, `serde` |
| `flui-layer` | `default`, `testing` |
| `flui-objects` | `default`, `testing`, `serde` |
| `flui-painting` | `serde`, `testing` |
| `flui-platform` | `default`, `desktop`, `winit-backend`, `web`, `wayland`, `x11` |
| `flui-reactivity` | `default`, `async`, `serde` *(вне workspace)* |
| `flui-rendering` | `default`, `serde`, `testing`, `experimental-delegates` |
| `flui-scheduler`, `flui-semantics`, `flui-tree`, `flui-macros` | `default` + `serde`/`testing` |
| `flui-types` | `default`, `serde`, `simd`, `mint`, `full` |
| `flui-view` | `default`, `test-utils`, `runtime-internals` |
| `flui-widgets` | `default`, `images`, `asset-images`, `network-images`, `serde` |
| `flui` (фасад) | `default`, `serde` |

Разделение по категориям постановки в целом соблюдено: core runtime / renderer backend (`vulkan`/`metal`/`dx12`/`webgpu`/`gles`) / platform backend (`winit-backend`/`wayland`/`x11`) / devtools / profiling — разнесены по крейтам, фасад не превращён в сборник интеграций. **Это сделано правильно.**

### Находки

**Доказанный дефект (1).** `flui-reactivity` не является членом workspace (`Cargo.toml:63-68`) — следовательно **ни один** из его features (`async`, `serde`) не проверяется никаким гейтом, включая feature-matrix. Крейт с рабочим кодом полностью вне CI (см. также §20).

**Доказанный дефект (2).** Features `debug-overlay` и `performance-overlay` в `flui-app` объявлены как `= []` и **не используются ни одним `cfg`** (§27). Это accidental semver surface: features опубликованы, потребитель может их включить, и ничего не произойдёт.

**Доказанный дефект (3).** Фасадный `Cargo.toml` содержит шесть закомментированных feature-блоков (`parallel`, `profiling`, `tracy`, `full-profiling`, `devtools`, `memory-profiler`, `full`) с пометками «temporarily disabled» / «TODO: Fix … before enabling». То же в `flui-devtools`. Комментарии описывают возможности, которых нет.

**Риск.** Взаимоисключающие backend-features `flui-engine` (`vulkan`/`metal`/`dx12`/`webgpu`/`gles`): при feature-unification в графе, где два крейта просят разные бэкенды, cargo включит оба. Проверяется ли это — не установлено; `cargo hack --each-feature` **не** проверяет комбинации, только одиночные включения.

### Покрытие CI

| Требуемая постановкой проверка | Статус в FLUI |
|---|---|
| `cargo check --workspace --no-default-features` | ✅ входит в `--each-feature` |
| `cargo check --workspace --all-features` | ✅ входит в `--each-feature` |
| `cargo test --workspace --all-features` | ⚠️ тесты гоняются на default-features (`cargo nextest run --workspace --exclude flui-platform`) |
| `cargo hack check --feature-powerset` | ❌ **не выполняется** |

CI-джоб `feature-matrix` делает:

```bash
cargo hack clippy --workspace --locked --each-feature --optional-deps --keep-going -- -D warnings
cargo hack clippy --workspace --locked --each-feature --optional-deps --keep-going --tests --benches --examples -- -D warnings
```

`--each-feature` проверяет каждый feature **по отдельности** + `--no-default-features` + `--all-features`. Он **не обнаружит** дефект, возникающий только при комбинации двух features. Для `flui-engine` с 13 features полный powerset — 2¹³ конфигураций, что действительно нецелесообразно.

**Рекомендация (совпадает с разрешением постановки):** не гнать полный powerset, а **задокументировать матрицу поддерживаемых комбинаций** (какой backend с какой платформой) и проверять именно её — десяток именованных конфигураций вместо тысяч. Такого документа сейчас нет.

---

## 30. Новые обязательные benchmark-сценарии

**Статус раздела: спецификация. Ни один из перечисленных бенчмарков не написан.**

Существующая база — 21 файл в 6 крейтах:

| Крейт | Бенчмарки |
|---|---|
| `flui-types` | `color_bench`, `geometry_bench`, `conversions_bench` |
| `flui-animation` | `animation_bench` |
| `flui-engine` | `offscreen_resource_cache`, `render_throughput` |
| `flui-rendering` | `layout`, `paint`, `intrinsic_parent_data`, `virtualizer`, `helpers` |
| `flui-view` | `global_key_reparent_latency`, `key_storage_shape`, `static_path_algorithm` (+ `shared/`) |
| `flui-interaction` | `gesture_arena`, `pointer_resampler`, `tap_detector`, `velocity_tracker`, `pointer_route` |

CI-джоб `bench-compile` проверяет только компиляцию (`cargo bench --no-run`) — **числа никуда не собираются и не сравниваются между сборками**. Регрессия производительности не будет замечена.

Требуемые постановкой сценарии и их выполнимость **сегодня**:

| Группа | Сценарий | Выполним сейчас? |
|---|---|---|
| **Toolchain** | debug/release на текущем stable; сравнение с MSRV; assembly критических функций; code size; symbol demangling; cross-platform linker diagnostics | Частично. **Code size и demangling бессмысленны при `strip = "symbols"`** (§21) — сначала §21 |
| **Data transfer** | drop одного пути; drop 10 000 файлов; несколько MIME; большое изображение; отмена; закрытие source-окна; удаление target-узла; clipboard 100 MB; медленный producer | ❌ **Нечего мерить — DnD отсутствует** (§22) |
| **Devtools** | overhead выключенного/подключённого inspector; сериализация 100k узлов; diff snapshot; одновременное обновление UI и inspector | ❌ **Нечего мерить — inspector не подключён к дереву** (§26) |
| **Hot reload** | обновление функции/дерева; сохранение state; отмена задач; invalid patch; 100 патчей; рост памяти | ⚠️ Частично — обновление функции измеримо; state preservation отсутствует (§28) |
| **Multi-window** | 100 окон; shared state; закрытие с активными задачами; DnD между окнами; разные DPI/refresh/GPU | ⚠️ Частично — `UiRealm` есть; DnD-сценарий невыполним |
| **Accessibility** | обновление одного leaf; перестроение большого поддерева; keyboard navigation; scroll-to-focus; event storm; dynamic text scaling | ⚠️ Внутренние измеримы; всё, что требует screen reader — **нет** (нет платформенного моста) |
| **§16 bitsets** | один флаг; несколько флагов; миллион заголовков; очистка после кадра; поиск первой стадии | ✅ **Выполнимо сразу** — единственная группа без блокеров |

**Вывод по §30: пять из семи групп бенчмарков нельзя написать, потому что измеряемой функциональности не существует.** Это делает раздел не задачей «добавить бенчмарки», а индикатором: сначала функциональность (§22, §26, §27), потом измерение. Начинать следует с двух незаблокированных групп — §16 (bitsets) и Toolchain (после починки §21).

Отдельная рекомендация вне списка: наладить **сбор и сравнение** чисел существующих 21 бенчмарка между сборками — сейчас гейт проверяет только, что они компилируются.

---

## 31. Дополнительные выходные документы аудита

### 31.1. Toolchain Migration Report

**Текущее состояние**

| Параметр | Значение |
|---|---|
| Development toolchain | 1.96.1 (пин `rust-toolchain.toml`) |
| CI stable | 1.97.1 (джобы с `dtolnay/rust-toolchain@stable` переопределяют пин) |
| MSRV | 1.96 (`Cargo.toml`), проверяется джобом `msrv` |
| Nightly | `nightly` + пин `nightly-2026-03-20`, только miri |
| Edition | 2024 |
| Рекомендуемый stable | **1.97.1 (8bab26f4f 2026-07-14)** |

**Compatibility failures при переходе 1.96.1 → 1.97.1:** не обнаружено блокеров. Ноль nightly-фич (§14), edition не меняется, все атомики поддержаны на всех targets (§17).

**Lint changes:** основной ожидаемый источник трения — новые/ужесточённые линты `clippy::pedantic`, которые попадут под `-D warnings`. Оценка объёма не производилась (**гипотеза**); проверяется одной командой `cargo +1.97.1 clippy --workspace --all-targets -- -D warnings`.

**Codegen changes:** LLVM 22.1.2 в 1.96.1. Изменения codegen между патч-релизами не ожидаются; при переходе на минорную версию требуется перепроверка ассемблера критических функций — чего в проекте нет (§30).

**Cargo changes:** не выявлено требующих действий.

**Platform changes:** не выявлено.

**План обновления (порядок важен):**

1. Починить `crates/flui-scheduler/src/ticker.rs:962,1147` — иначе `wasm-check` не стартует (§19).
2. Закоммитить `wasm-check` и убедиться, что джоб зелёный.
3. **Расцепить контуры:** `rust-toolchain.toml` → `channel = "1.97.1"`; `Cargo.toml` → `rust-version = "1.96"` **оставить**; убрать из `rust-toolchain.toml` комментарий «Mirrors `[workspace.package].rust-version`», заменив на явное «development toolchain, НЕ MSRV».
4. Прогнать `cargo clippy --workspace --all-targets -- -D warnings` на 1.97.1; разобрать новые линты (чинить, не подавлять).
5. Обновить доки, перечисленные в процедуре внутри `rust-toolchain.toml` (`AGENTS.md`, `README.md`, `docs/FOUNDATIONS.md`, `docs/getting-started.md`, `openspec/config.yaml`) — с явным различением двух версий.
6. Исправить дрейф комментария `ci.yml:539` («1.96.0» → фактический пин).
7. Убедиться, что джоб `msrv` остаётся единственным, проверяющим 1.96, и что он не переопределяется action'ом `@stable`.

**Явно не делать:** не поднимать `rust-version` до 1.97. Пользы нет (§13), а обещание MSRV дёшево дать и дорого отозвать.

### 31.2. Stable Feature Adoption Report

| Возможность | Где полезна | Текущая замена | Выигрыш | Стоимость миграции |
|---|---|---|---|---|
| `core::range::Range` (`Copy`-range) | `TextRange` (`flui-types/.../text_metrics.rs:70`) | Собственная структура | **Не доказан.** Тип уже `Copy`, отдельных `start`/`end` полей нет | Низкая, но **не рекомендуется без бенчмарка** |
| `impl RangeBounds<usize>` в публичных сигнатурах | Новые API, принимающие диапазон (selection, срезы списков) | Конкретные типы | Аддитивная миграция вместо breaking change в будущем | **Низкая — рекомендуется** |
| `bit_width` / `isolate_lowest_one` / `trailing_zeros` | Выбор следующей dirty-стадии по `RenderFlags` | Линейный перебор | Наносекунды на узел | Низкая, но **приоритет ниже K1** (§16) |
| ~~`assert_matches!`~~ | — | — | — | **Недоступно: не стабилизирован** ни в 1.96.1, ни в 1.97.1 (проверено компиляцией). Использовать `matches!` + `debug_assert!` |
| `unsafe_op_in_unsafe_fn` (deny) | Workspace lints | Отсутствует | Явная граница unsafe | Низкая — рекомендуется |
| `unexpected_cfgs` (deny) | Workspace lints | Отсутствует | Ловит опечатки в `cfg(target_arch)`/`cfg(feature)` | **Низкая — рекомендуется** |
| `clippy::undocumented_unsafe_blocks` | 24 инлайновых `#[allow(unsafe_code)]` + 2 крейтовых | Отсутствует | Механическая гарантия `SAFETY:` | Средняя (потребует дописать комментарии) |
| `-C symbol-mangling-version=v0` | Профилирование, crash-репорты | Дефолт (`legacy`) | Корректные generic-параметры в стеках | Низкая, но **бессмысленна до снятия `strip = "symbols"`** (§21) |
| `split-debuginfo` | Release-диагностика | Закомментировано | Символы для профайлера при stripped-бинаре | **Средняя — высокий приоритет** (§21) |

Возможности, которые **не предлагаются**, потому что не улучшают конкретный участок: специализация range-типов в hot path без замеров, механическая замена enum на битфлаги (§16), перевод API на `Box<dyn Trait>` (§21).

### 31.3. Future Rust Dependency Risk

| Будущая возможность | Зависит ли архитектура | Stable fallback | Риск |
|---|---:|---|---:|
| Async trait object dispatch | **Нет** | `Pin<Box<dyn Future>>` на одной границе (`async_driver.rs`) | **Нет** |
| Return type notation (RTN) | **Нет** | Явные ассоциированные типы | **Нет** |
| Next-generation trait solver | **Нет** | — | **Нет** |
| Guaranteed destructors | **Нет** | `TaskToken` отменяет по drop, но корректность не *зависит* от гарантии выполнения деструктора | **Нет** |
| Immovable types | **Нет** | `Pin` | **Нет** |
| Advanced const generics | **Нет** | Arity-система построена на обычных дженериках | **Нет** |
| Specialization | **Нет** | — | **Нет** |
| TAIT | **Нет** | Боксинг на границе | **Нет** |

**Вывод: риск будущих зависимостей — нулевой.** Проект не спроектирован в ожидании ни одной незавершённой возможности языка. Это подтверждается механически: ноль `#![feature(...)]`, ноль `rustc_private`, ноль `async_trait`.

Единственная точка расширения, которую стоит сохранять осознанно: стирание задач в `Pin<Box<dyn Future>>` сосредоточено в `async_driver.rs`, поэтому появление native async-trait dispatch позволит убрать аллокацию в одном файле, не трогая потребителей.

### 31.4. Ecosystem Parity Matrix

**Ограничение достоверности:** сравнение опирается на знание экосистемы по состоянию на май 2026 и **не является измерением**. Строка «FLUI» — на данных этого аудита; остальные строки — экспертная оценка (**гипотеза**). Сравниваются, как требует постановка, не количество фич, а глубина.

| Критерий | Flutter | GPUI | Xilem | Masonry | Slint | Freya | Dioxus | Iced | egui | winit | **FLUI** |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Архитектурная глубина | Очень высокая | Высокая | Высокая | Средне-высокая | Высокая | Средняя | Средняя | Средняя | Низкая (IM) | н/п | **Высокая** — три дерева, slivers, arena, inherited |
| Performance ceiling | Высокий | Очень высокий | Высокий | Высокий | Высокий | Средний | Средний | Средний | Средний | н/п | **Ограничен** — неинкрементальный paint (K1), клоны при rebuild (K2) |
| Diagnostics | Очень высокие (DevTools) | Средние | Низкие | Низкие | Средние | Средние | Средние | Низкие | Встроенные | н/п | **Очень низкие** — devtools не подключены (§26), overlay не подключён (§27), release без символов (§21) |
| Production readiness | Да | Да (Zed) | Нет | Нет | Да | Нет | Частично | Частично | Да (для своей ниши) | Да | **Нет** |
| Platform correctness | Высокая | Высокая (macOS-first) | Средняя | Средняя | Высокая | Средняя | Средняя | Средняя | Средняя | Высокая | **Средняя** — нет DnD, нет tray, event-loop affinity не выражена (§23), тесты flui-platform исключены из CI |
| Accessibility | Высокая | Средняя | AccessKit | AccessKit | Высокая | AccessKit | Частично | Частично | Ограниченная | н/п | **Нет платформенного моста** — дерево семантики есть, `accesskit` = 0 файлов |
| Extensibility | Высокая | Средняя | Высокая | Высокая | Средняя | Средняя | Высокая | Средняя | Средняя | Высокая | **Высокая** — `CommandRenderer`/`LayerRender`, custom backends |
| State model | StatefulWidget | Модель Zed | Xilem-view | Widget-tree | Property/binding | Signals | Signals/hooks | Elm | Immediate | н/п | **Flutter-паритет** + точные dependent-множества (лучше broadcast) |
| Async safety | Средняя (Dart isolates) | Высокая | Средняя | Средняя | Средняя | Средняя | Средняя | Средняя | н/п | н/п | **Высокая** — `TaskToken` cancel-on-drop, `#[must_use]`, генерационные гейты (§24) |
| Renderer independence | Skia/Impeller | GPU-специфичен | Vello | Vello | Несколько | Skia | Несколько | wgpu/tiny-skia | Несколько | н/п | **Высокая** — трейтовая граница рендера |

**Где FLUI объективно сильнее большинства Rust-конкурентов:** глубина модели дерева, async-безопасность (cancel-on-drop + `#[must_use]` + генерационные гейты — редкое сочетание), stable-first дисциплина (§14), независимость рендера, тестовая инфраструктура (render-object harness с CI-каталогом).

**Где отстаёт от всех:** диагностика (devtools/overlay/символы), accessibility-мост, системная интеграция (DnD, tray).

---

## 32. Финальные ответы

**1. Следует ли обновить проект с Rust 1.96?**
Да — но обновить **development/CI toolchain**, а не MSRV. Актуальный stable — 1.97.1 (2026-07-14). Блокеров нет: ноль nightly-фич, edition 2024 без изменений, атомики поддержаны на всех объявленных targets.

**2. Какая версия должна стать development toolchain?**
**1.97.1.** `rust-toolchain.toml` не должен зеркалить MSRV — сейчас он делает это намеренно, и именно это склеивает контуры.

**3. Какая версия должна остаться MSRV?**
**1.96.** Поднимать нечего ради чего: ни одна фича 1.97 не упрощает FLUI (§31.2). Крейты не опубликованы, поэтому цена подъёма сегодня нулевая — но это аргумент за фиксацию политики, а не за движение границы.

**4. Какие возможности нового stable Rust реально упрощают проект?**
Три, все из 1.97 и все проверены компиляцией: **v0 symbol mangling по умолчанию** (§21 — generic-параметры в символах вместо непрозрачного хеша), **`build.warnings` в Cargo** (§20 — гейт предупреждений вне rustc-fingerprint, измерено: переключение не вызывает перекомпиляции), **целочисленные bit-API** `bit_width`/`isolate_lowest_one`/`isolate_highest_one`/`lowest_one`/`highest_one` (§16 — недоступны на 1.96). Дополнительно полезны lint-настройки `unexpected_cfgs`/`unsafe_op_in_unsafe_fn`/`undocumented_unsafe_blocks` (§20) и `impl RangeBounds` в новых API (§15). `assert_matches!` в этот список **не входит** — вопреки некоторым сводкам release notes он не стабилизирован.

**5. Какие новые API могут уменьшить memory footprint?**
Не API нового Rust, а устранение мономорфизации: `flui-widgets` — 642 928 LLVM-строк / 29 573 копии; холодные lifecycle-методы `ElementCore<V,A>` (mount/unmount/activate/deactivate/set_parent_render_id/set_pipeline_owner_any) дают 88 133 строки (13.7%) без выигрыша в скорости. Selective boxing холодной половины — главный резерв (§21). Влияние на итоговый размер бинаря **не измерено**.

**6. Какие API могут улучшить dirty tracking?**
Render-дерево уже имеет то, что нужно: `RenderFlags: u32` в `AtomicU32` (`flui-rendering/src/storage/flags.rs:99`). Улучшить можно (а) распространив эту схему на element-дерево, где флаги — разрозненные `bool`, (б) выбирая следующую стадию через `trailing_zeros` вместо перебора, (в) уточнив ordering у флагов, публикующих готовность данных (`Acquire`/`Release` вместо `Relaxed`). **Приоритет ниже, чем K1/K2** — при неинкрементальном paint это оптимизация не того порядка.

**7. Какие unstable возможности нельзя использовать в публичной архитектуре?**
Все перечисленные в постановке — и проект их **уже не использует** (§14, §31.3). Правило соблюдается; его следует зафиксировать как явную политику, чтобы оно не размылось: сейчас оно соблюдается по факту, а не по договорённости.

**8. Готова ли platform abstraction к современному clipboard и drag-and-drop?**
**Нет.** Clipboard — синхронный, блокирующий, text-only, с MIME в виде `Option<String>` (`traits/platform.rs:484-507`). Drag-and-drop **отсутствует полностью** — ноль совпадений в дереве; winit-бэкенд не обрабатывает `DroppedFile`/`HoveredFile`. Требуемая семиступенчатая модель (offer → negotiation → request → delivery → decoding → action → completion) не представлена ничем. Хорошая новость: два несущих механизма для неё уже есть — генерационные ID (`flui-foundation/src/id.rs:983-1023`) и `TaskToken` (§22).

**9. Есть ли полноценная event-loop affinity model?**
**Нет.** Все ~30 методов `Platform` принимают `&self`; `Arc<dyn Platform>` и `Clipboard: Send + Sync` позволяют вызвать `open_window`/`clipboard`/`set_cursor_style` из любого worker-потока. Проверок main-thread — **ноль совпадений** в `flui-platform` и `flui-app`. Заготовки есть (`background_executor`/`foreground_executor`, `is_on_executor()`, `UiRealm`), но они необязательны и не дают типовой гарантии. Отсутствие глобального platform-синглтона — выполнено.

**10. Можно ли подключить devtools без overhead в release?**
Граница выбрана верно (feature-flag, `default = []`, нулевой overhead), но **подключать нечего**: `flui-devtools` не зависит ни от одного flui-крейта кроме `flui-hot-reload` и физически не может наблюдать дерево (`src/lib.rs:93-94`). Плюс документация крейта рекламирует три несуществующие подсистемы (network-monitor, memory-profiler, remote-debug). Решение — инверсия зависимости через trait/канал наблюдений в `flui-foundation`.

**11. Объясняет ли framework причину каждого rebuild/layout/repaint?**
**Нет.** Полей `Last rebuild reason` / `Last layout reason` / `Last repaint reason` не существует. Есть `tracing::debug!` в точках жизненного цикла (`element/generic.rs:293,308,323`) и счётчики планировщика (`frame_count`, `janky_frame_count`, `skipped_frames`) — но они никуда не выводятся. При этом K1 основного аудита делает вопрос частично бессмысленным: причина каждого repaint сегодня одна — «что-то в дереве стало грязным».

**12. Поддерживает ли архитектура state-safe hot reload?**
**Нет, и документация обещает больше, чем есть.** `flui-devtools/src/lib.rs:23` заявляет «State preservation», но в крейте только file watcher. Реальный механизм (`flui-hot-reload`, dlopen, ~1 850 строк) host-only и не различает шесть классов изменений постановки; изменение layout типа между сборками — **риск** класса «host читает байты по старой раскладке». Найденные в дереве упоминания «state preserved» относятся к GlobalKey-reparent, а не к hot reload.

**13. Не раздувает ли generic API binary size?**
**Да, измеримо.** `flui-widgets` (один крейт): 642 928 LLVM-строк, 29 573 мономорфизации. Лидер — `dispatch_view_update`: 45 472 строки в 133 копиях (342 строки/копию). Множитель — триплет `Element<V, A, B>`. Влияние на итоговый размер бинаря **не измерено** (`cargo bloat --release` не запускался) — это следующий шаг.

**14. Работают ли profiler и crash symbols с текущим symbol mangling?**
**Нет — и проблема не в mangling.** `[profile.release]` содержит `strip = "symbols"` без `debug` и без `split-debuginfo` (последний закомментирован в `.cargo/config.toml`). В release-сборке **символов нет вообще**, поэтому flamegraph, perf, samply, Tracy, Instruments, WPA, minidump-символизация и linker maps не работают ни с каким mangling. `-C symbol-mangling-version` не задан; переход на `v0` полезен для читаемости generic-стеков, но бессмыслен до восстановления символов.

**15. Может ли проект считаться современным Rust UI-runtime в 2026 году?**
**Как runtime-ядро — приближается; как продукт — нет.**

*За:* stable-first без единой nightly-зависимости (редкость); async-модель с отменой по drop, `#[must_use]` и генерационными гейтами — лучше, чем у большинства конкурентов; глубина модели дерева на уровне Flutter; независимость рендера; дисциплинированный `unsafe` под miri; тестовая инфраструктура с CI-каталогом render-объектов.

*Против:* потолок производительности задан неинкрементальным paint (K1) и клонами при rebuild (K2) — то есть архитектурно, а не константами. Диагностика — самое слабое место по всем трём осям сразу: devtools не подключены, overlay не подключён, release-сборка не символизируется. Системная интеграция отсутствует (DnD, tray, a11y-мост). Event-loop affinity не выражена в типах. Два гейта не работают: `wasm-check` красный до первого запуска, `flui-reactivity` вне CI целиком.

Ни один из этих пунктов не требует переписывания. Все они — работа по достройке того, что уже спроектировано правильно.

---

## Сводка находок upgrade pack

| # | Раздел | Находка | Тип |
|---|---|---|---|
| U1 | §19 | `wasm-check` падал при первом запуске — **11 ошибок в 3 крейтах**, не 2 | **Доказанный дефект — ИСПРАВЛЕНО** |
| U2 | §18 | Гарды `Lifecycle` не применялись нигде в production; `Defunct → Active` был достижим | **Доказанный дефект — ИСПРАВЛЕНО** |
| U3 | §26 | `flui-devtools` не зависит ни от одного flui-крейта → не может наблюдать дерево; документация рекламирует 3 несуществующие подсистемы | **Доказанный дефект** |
| U4 | §21 | `strip = "symbols"` → release не профилировался и не символизировался | **Доказанный дефект — ИСПРАВЛЕНО** (+935 KiB) |
| U5 | §27 | `show_performance_overlay` не читался; `PerformanceOverlayLayer` не конструировался | **Доказанный дефект — ИСПРАВЛЕНО** (features `*-overlay` по-прежнему пустые) |
| U6 | §22 | Системный drag-and-drop отсутствует полностью; clipboard синхронный/text-only/MIME строкой | **Доказанный дефект** |
| U7 | §23 | Ни одной проверки main-thread; все ~30 методов `Platform` вызываемы из любого потока | **Доказанный дефект** |
| U8 | §20/§29 | `flui-reactivity` вне workspace: не компилируется, не линтуется, не тестируется; edition 2021 | **Доказанный дефект** |
| U9 | §21 | 29 573 мономорфизации в `flui-widgets`; холодные lifecycle-методы — 13.7% кода крейта | **Доказанный дефект** |
| U10 | §20 | `RUSTFLAGS=-D warnings` как единственный механизм → miri сбрасывал его целиком, кэш фрагментирован | **Доказанный дефект — ИСПРАВЛЕНО** (`CARGO_BUILD_WARNINGS`) |
| U11 | §19 | wasm проверяется только `cargo check` → линковка, undefined symbols и список импортов не проверяются | **Доказанный дефект — ЧАСТИЧНО** (гейт зелёный; линковки по-прежнему нет) |
| U12 | §15 | Два разных публичных `TextRange` в `flui-types` и `flui-rendering` | **Доказанный дефект** (низкая тяжесть) |
| U13 | §13 | `ci.yml` ссылался на пин «1.96.0» вместо фактического | **Доказанный дефект — ИСПРАВЛЕНО** |
| U14 | §13 | Development toolchain был схлопнут с MSRV | **Риск — ИСПРАВЛЕНО** (пин 1.97.1, MSRV 1.97, контуры расцеплены) |
| U15 | §17 | Ordering в production-местах не обоснован комментариями; `RenderFlags` вероятно требует `Acquire`/`Release` | **Риск** |
| U16 | §28 | dlopen hot-reload: изменение layout типа между сборками не проверяется на совместимость | **Риск** |
| U17 | §29 | Взаимоисключающие backend-features `flui-engine` не проверяются на комбинации (`--each-feature` ≠ powerset) | **Риск** |
| U18 | §22 | Синхронный `read_text()` на Wayland/X11 — блокировка UI-потока | **Риск** |
| U19 | §20 | `unsafe_code = "warn"`, не `deny`; нет `undocumented_unsafe_blocks` | **Риск — ЧАСТИЧНО** (3 линта приняты; `undocumented_unsafe_blocks` — 47 сайтов, отдельная работа) |
| U20 | §30 | 21 бенчмарк компилируется, но числа не собираются и не сравниваются → регрессии не видны | **Риск** |

**Пересечения с основным аудитом:** U3/U5 углубляют H9 (наблюдаемость ≈ 0), U9 добавляет измерение к K2, U15 уточняет H8. Новыми относительно 2026-07-23 являются U1, U2, U4, U6, U7, U8, U10, U11.

### Рекомендуемый порядок (не смешивать с фазами 8.10 основного аудита)

**Сделано:** U1, U2, U4, U5, U10, U13, U14, частично U19 (см. раздел «Статус» выше).
**Немедленно (часы):** U12 (развести одноимённые `TextRange`).
**Ближайшее (дни):** U8 (решить судьбу `flui-reactivity`), остаток U19 (35 рукописных SAFETY-комментариев + генератор).
**Средний срок (недели):** U11 (настоящая линковка + список импортов в CI), U3 (инверсия зависимости devtools), U9 (selective boxing холодной половины `ElementCore`).
**Требует проектирования:** U6 (data-transfer), U7 (capability-объект affinity), U16.
**Требует проектирования:** U6 (data-transfer архитектура), U7 (capability-объект affinity), U9 (selective boxing), U16.
