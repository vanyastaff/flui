# Имена до первой публикации — дизайн

- **Статус:** черновик на утверждение владельцу
- **Дата:** 2026-10-06
- **Требования:** [requirements.md](requirements.md) (реестр, правила N1–N10, гейт, решения
  владельца N1 и N2); полная таблица — [renames.md](renames.md)
- **Уровень 0:** [../release/requirements.md](../release/requirements.md) (D6, R18);
  связано: [../authoring-styles/requirements.md](../authoring-styles/requirements.md)
- **База:** `main` @ `4915054c8`

## 1. Что предлагается

Один механический проход после merge ядра `send-flip` (~11-09) и до `facade-surface`
(11-10…11-14) переименовывает **130 строк таблицы, ≈ 330 публичных идентификаторов** в 21 крейте,
12 групп публичных модулей и две Cargo-фичи. ≈ 20 из них удаляются: это непривязанная
поверхность и дубли. Проход выполняет табличная команда `cargo xtask rename`, без Python. Та же
таблица кормит новый гейт `cargo xtask names`: он не даёт старым именам вернуться из ветки,
открытой до прохода. Имена из Flutter/Dart/Skia, которые меняются, остаются находимыми на docs.rs
через `#[doc(alias)]` на новом определении (решение владельца N2).

Шаг до прохода, независимый от него: инструмент, гейт в режиме `--seed` и правила в AGENTS.md
мержатся отдельным PR до 11-09. Конфликтов с идущими ветками этот PR не создаёт.

## 2. Как выбирались имена

Правила N1–N10 из requirements.md остаются как есть. Ниже — решения, которые таблица добавляет к
ним. Каждое можно проверить по строкам renames.md.

**Когда имя Flutter остаётся.** Имя остаётся, если его поймёт Rust-разработчик, не видевший
Flutter. Это обычное английское слово (`Padding`, `Column`, `Row`, `Text`, `Stack`), словарь
Material 3 и Compose (`Scaffold`, `AppBar`, `Card`) или имя, которое уже есть в Rust-экосистеме
с тем же смыслом (`Container` — iced `widget::Container`; `Matrix4` — nalgebra). Остаются и
причастия-обёртки `Expanded`, `Flexible`, `Positioned`: прецедент std `Wrapping<T>`,
`Reverse<T>`. Остаётся ядро модели FLUI (`Sliver*`, `ParentData`, `RenderBox`, `Element`).
Полный список с причинами — renames.md §3. `SizedBox` остаётся: `Sized` занят std, а
`FixedSize` хуже читается в роли распорки.

**Когда меняется.** Имя меняется в пяти случаях:

1. Сокращение Skia или нарушение C-CASE: `RRect` → `RoundedRect` (kurbo), `HSLColor` → `Hsl`
   (palette, рядом с уже существующим `Oklab`), `SrcATop` → `SrcAtop` (peniko), `IOS*` → `Ios*`.
2. Артефакт Dart: `Mixin`, `*Base`, статический класс `Curves` → модуль `curves` с константами
   `EASE_IN`, `Disposable`, `VoidCallback`.
3. Коинедж Flutter без смысла вне Flutter: `Hero`, `InkWell`, `ScaffoldMessenger`, `Theater`.
4. Имя, которое в Rust значит другое. `*Builder` у виджета, строящего дерево из замыкания,
   сталкивается с C-BUILDER. `Listener` в Rust читается как наблюдатель, а виджет принимает
   сырые события указателя.
5. Два имени у одного понятия (N6): `PaintingStyle`/`PaintStyle`, `Clip`/`ClipBehavior`,
   `PictureLayer` при записи `DisplayList`, `Handler`/`Callback`.

**Значение и его поставщик (О1).** Значение получает существительное без суффикса, виджет,
который поставляет его потомкам, — `…Scope`: `Theme` и `ThemeScope`, `MediaQuery` и
`MediaQueryScope`, `IconTheme` и `IconThemeScope`, `TextStyleScope`, `TextDirectionScope`,
`TabControllerScope`. Прецеденты: тема как значение в iced (`Theme`), GPUI (`Theme`), egui
(`Style`); соглашение `…Scope` уже есть в FLUI (`HeroScope`, `VsyncScope`, `GlobalKeyScope`,
`GestureArenaScope`). Префикс `Default*` у поставщика уходит.

**Словарь стёртых и сырых форм.** `Erased*` — объектно-безопасная форма: `ErasedElement`,
`ErasedBoxLayoutContext`, `ErasedAssetCache`. `Raw*` — нижний контекст под удобной обёрткой
(прецедент std `RawWaker`/`Waker`). `Driver*` — реализация, которую строит обход pipeline.

**Колбэки и замыкания.** `Fn`-псевдоним или обёртка, которую вызывают, называется `*Callback`
(N6). Псевдоним замыкания, которое строит view, — `*Fn` (О2): `ValueViewFn`, `RouteContentFn`.
Слово `Builder` остаётся только у типов паттерна C-BUILDER (`AnimationControllerBuilder`,
`SchedulerBuilder`).

**`#[doc(alias)]`.** Alias ставится только на строки N2, где старое имя пришло из
Flutter/Dart/Skia. Значение — имя Flutter в его написании (`ThemeData`, `RRect`, `easeIn`,
`drawDRRect`). Собственным старым именам FLUI alias не нужен: потребителей до 0.2.0 нет.
Ограничения rustdoc учтены: alias стоит на определении, а не на `pub use`; в значении нет
пробелов и кавычек; alias не совпадает с именем самого элемента. Если имя Flutter после прохода
занято другим элементом FLUI (Flutter-виджет `Theme` против значения `Theme`), alias не ставится:
поиск находит значение, а его документация ссылается на `ThemeScope`.

## 3. Сводка таблицы

Двадцать самых заметных потребителю переименований: они стоят в tutorial, `prelude`, README и
книге.

| # | Было | Стало | Вопрос |
|---|---|---|---|
| 1 | `EventCx` | `EventContext` | — |
| 2 | `ThemeData` / `Theme` | `Theme` / `ThemeScope` | О1 |
| 3 | `MediaQueryData` / `MediaQuery` | `MediaQuery` / `MediaQueryScope` | О1 |
| 4 | `RRect`, `ClipRRect` | `RoundedRect`, `ClipRoundedRect` | — |
| 5 | `Curves::EaseIn` | `curves::EASE_IN` | — |
| 6 | `*ThemeData` (17 типов Material) | `*Theme` | О1 |
| 7 | `FutureBuilder`, `StreamBuilder` | `FutureView`, `StreamView` | О2 |
| 8 | `LayoutBuilder` | `ConstraintsView` | О2 |
| 9 | `InkWell` | `Ripple` | О4 |
| 10 | `SnackBar` | `Snackbar` | — |
| 11 | `ScaffoldMessenger` | `SnackbarHost` | О4 |
| 12 | `Hero` | `SharedElement` | О4 |
| 13 | `Material` (виджет) | `Surface` | О4 |
| 14 | `WidgetState`, `WidgetStateProperty<T>` | `InteractionState`, `InteractionValue<T>` | — |
| 15 | `DefaultTextStyle`, `Directionality` | `TextStyleScope`, `TextDirectionScope` | О1 |
| 16 | `ImageProvider` | `ImageSource` | — |
| 17 | `Listener` (виджет) | `PointerListener` | — |
| 18 | `FocusManager`, `focus_manager()` | `FocusTree`, `focus_tree()` | О5 |
| 19 | `PaintCx`, `BoxLayoutCtx` и ещё 8 `*Ctx` | `PaintContext`, `RawBoxLayoutContext`, … | — |
| 20 | `text_store_kit::assert_conforms` | `text_store_conformance::assert_conforms` | — |

По крейтам (строки renames.md §1): flui-widgets 22, flui-rendering 16, flui-view 15,
flui-interaction 13, flui-painting 11, flui-material 10, flui-platform 8, flui-foundation 7,
остальные 28. Риск: В — 12 строк, С — 50, Н — 68.

## 4. Контексты (решение владельца N1)

Типы-контексты называются `…Context`, привязки — `cx`. Написания `Ctx` и `Cx` из имён типов
уходят. Слово `Context` закрепляется за контекстами: тип, который не передаётся как контекст,
его не носит (`NodeContext` → `FocusNodePayload`).

| Было | Стало | Крейт | Ссылок | Замечание |
|---|---|---|---|---|
| `EventCx<'a>` | `EventContext<'a>` | flui-view | 509 | `EventContextError` уже в этом написании |
| `PaintCx<'a, A>` | `PaintContext<'a, A>` | flui-rendering | 190 | файл `context/paint_cx.rs` → `context/paint.rs` |
| `TextCx<'a>` | `TextContextMut<'a>` | flui-rendering | 28 | `TextContext` занят, это `RefMut` на него |
| `BoxLayoutCtx` | `RawBoxLayoutContext` | flui-rendering | 80 | под обёрткой `BoxLayoutContext` (= `LayoutContext<Box…>`) |
| `SliverLayoutCtx` | `RawSliverLayoutContext` | flui-rendering | 27 | то же |
| `BoxHitTestCtx` | `RawBoxHitTestContext` | flui-rendering | 31 | то же |
| `SliverHitTestCtx` | `RawSliverHitTestContext` | flui-rendering | 12 | то же |
| `BoxLayoutCtxErased` (трейт) | `ErasedBoxLayoutContext` | flui-rendering | 51 | объектно-безопасная форма |
| `SliverLayoutCtxErased` (трейт) | `ErasedSliverLayoutContext` | flui-rendering | 27 | то же |
| `ErasedBoxLayoutCtx` (struct) | `DriverBoxLayoutContext` | flui-rendering | 13 | драйверная реализация трейта выше |
| `ErasedSliverLayoutCtx` (struct) | `DriverSliverLayoutContext` | flui-rendering | 10 | то же |
| `BoxIntrinsicsCtx` | `BoxIntrinsicsContext` | flui-rendering | 212 | реэкспорт `src/rendering.rs` |
| `BoxDryLayoutCtx` | `BoxDryLayoutContext` | flui-rendering | 92 | то же |
| `BoxDryBaselineCtx` | `BoxDryBaselineContext` | flui-rendering | 72 | — |
| `NodeContext` | `FocusNodePayload` | flui-interaction | 15 | не контекст |

Без изменений остаются `BuildContext`, `LifecycleContext`, `ElementBuildContext`,
`RenderObjectContext`, `LayoutContext`, `HitTestContext`, `BoxLayoutContext`,
`SliverLayoutContext`, `BoxHitTestContext`, `SliverHitTestContext`, `MultiChildLayoutContext`,
`FlowPaintingContext`, `TextContext`, `TaskContext` и `ServiceContext`.

**Привязки.** 3559 вхождений `ctx` в 343 файлах становятся `cx`, 230 времён жизни `'ctx` —
`'cx`. Где в одной функции уже есть `cx` (параметр `BuildContext` и замыкание события) или в
одном типе уже есть `'cx` (`FlowPaintingContext<'ctx, 'cx>`), инструмент не переименовывает и
выводит список для ручной правки. Затенение `cx` в замыкании `move |cx| …` внутри `build(cx)`
допустимо и идиоматично (так в GPUI). Пример из authoring-styles меняется на
`count.get(cx)` … `.on_press(move |cx| count.update(cx, …))`.

**Затронутые спеки и документы `EventCx` (правятся после утверждения, в PR прохода):**

- спеки: `send-flip/requirements.md` (7 вхождений), `send-flip/design.md` (10),
  `send-flip/tasks.md` (1), `authoring-styles/requirements.md` (3), `release/requirements.md`
  (D4, 1), `focus-keyboard/tasks.md` (1), `realm-model/experiment.md` (6). `dx/research.md`
  оставить: это датированное исследование с цитатами. Сама эта спека и requirements.md
  описывают старые имена и не правятся;
- `design/`: `README.md`, `architecture.md`, `decisions.md`, `open-questions.md`;
- ADR: ADR-0086 (27 вхождений), ADR-0085 (9), ADR-0023 (5), ADR-0003, ADR-0054, ADR-0074,
  ADR-0124;
- `ARCHITECTURE.md`: flui-view, flui-widgets (17 со всеми контекстами), flui-rendering (10),
  flui-material, flui-cupertino; `crates/flui-rendering/docs/{LAYOUT_SYSTEM,HIT_TEST_SYSTEM}.md`,
  `crates/flui-interaction/docs/HIT_TESTING.md`; книга `getting-started/tutorial-todo.md`.

Новые спеки и ветки уже сейчас называют новые типы `…Context` (requirements, «Порядок внедрения»).

## 5. План прохода

### 5.1 Инструмент: `cargo xtask rename`

Таблица — `tools/xtask/renames/naming.toml`, одна запись на имя, порождается из renames.md
после утверждения. Код — `tools/xtask/src/rename.rs` + `rename/{table,apply,tests}.rs`. Лексер
Rust берётся из `markers` (`markers/tokens.rs`), разбор определений — `syn`. Скриптов на Python
нет; ast-grep не нужен. Почему не ast-grep: понадобилось бы ~330 правил, он не правит Markdown,
TOML и `.stderr`, а таблица нужна ещё гейту и веткам в полёте.

```toml
[[rename]]
kind = "ident"            # ident | path | method | module | file | feature | binding
old = "RRect"
new = "RoundedRect"
crate = "flui-foundation" # где определено; гейт проверяет, что определение одно
alias = "RRect"           # только для N2
strings = ["RenderClipRRect"]  # строковые литералы, которые тоже меняются (реестры, Debug)

[[rename]]
kind = "path"
old = "Curves::EaseIn"
new = "curves::EASE_IN"
alias = "easeIn"

[[rename]]
kind = "method"
old = "get_velocity"
new = "velocity"
scope = ["crates/flui-interaction/**", "crates/flui-widgets/**", "packages/**"]
```

Виды записей:

- `ident` — токен-идентификатор в `.rs`. Все записи одного прохода применяются одновременно, за
  один проход по файлу (подстановка, а не цепочка). Поэтому обмен (`Theme` → `ThemeScope` и
  `ThemeData` → `Theme`) не превращается в `ThemeScope` дважды. Перед применением таблица
  проверяет, что `old` определён ровно один раз и только в `crate`. Иначе нужен `scope`.
- `path` — последовательность токенов (`Curves::EaseIn`).
- `method` — имя метода, всегда со `scope`. Страховка — компилятор: метод чужого типа,
  переименованный по ошибке, не соберётся.
- `module` — идентификатор только в позициях `mod x`, `use …::x`, `x::`.
- `file` — `git mv` и правка `mod`/`#[path]`.
- `feature` — ключ `[features]` и каждое упоминание в `Cargo.toml`, `.config/nextest.toml`,
  шаблонах `flui-cli`, `docs/testing.md`, в `--features` скриптов xtask.
- `binding` — `ctx` → `cx`, `'ctx` → `'cx` по правилам §4.

**Где применяется.** `.rs` — по токенам. В строковых литералах правятся только слова из
`strings`. Литерал первого аргумента `debug_struct`/`debug_tuple` правится всегда, если он равен
`old` (R16). В Markdown (неархивном) правятся только code spans и fenced code: проза («Material
Design») не трогается. В `.toml`, `.wgsl` и `llms.txt` — слово целиком.

**Где не применяется.** `docs/plans/**`, `docs/research/**`, `CHANGELOG.md`, существующие
`changelog.d/*`, `target/`, `.worktrees/`. Не правятся строки внутри `#[serde(rename…)]` и
проводные имена `flui-protocol` (ADR-0080). Снимки `.snap` и `.stderr` не правятся текстом:
они пересоздаются прогоном (`TRYBUILD=overwrite`, `cargo insta review`), и PR объясняет каждое
изменение.

**`--add-aliases`** вставляет `#[doc(alias = "…")]` перед определением записи с `alias`, рядом с
остальными атрибутами.

**Защита от повторного применения.** Записи-обмены (О1) не идемпотентны. Поэтому инструмент
отказывается работать с файлом, где уже есть одно из «новых, но не старых» имён обмена
(`ThemeScope`, `MediaQueryScope`, `IconThemeScope`, `CupertinoThemeScope`, `TabControllerScope`).
Исключение — запуск с `--changed-since`, см. §5.4. Остальные записи идемпотентны.

### 5.2 Порядок (один PR, отдельный коммит на шаг)

1. Удаления и слияния дублей: F5, PL3, PL4, PL6, PT7, PT8, I1, I2, I3, V5, W20. Компилятор
   показывает, что ничего не держалось за удалённое.
2. Типы, трейты и псевдонимы, включая обмены О1 и контексты §4 (одна подстановка).
3. Варианты, константы, `Curves`, `CupertinoColors`, методы `get_*` и `rrect`.
4. Модули: пути `mod`/`use`, разбор мешков (renames.md §2).
5. Файлы и каталоги (`git mv`), сначала публичные модули, потом внутренние (renames.md §4).
6. Cargo-фичи `test-utils` и `test-support` → `testing`.
7. Привязки и времена жизни (`ctx` → `cx`).
8. `#[doc(alias)]`.
9. Имена тестов (renames.md §5); `rg` каждого старого имени по документам.
10. Документы: `ARCHITECTURE.md`, `docs/`, книга, `design/`, `llms.txt`, ADR, таблица в
    AGENTS.md (`focus_manager` → `focus_tree` при О5, `text_store_kit` → `text_store_conformance`
    в «Extending FLUI»).
11. Снимки и trybuild пересоздаются, каждое изменение объясняется в PR.
12. Allowlist `names.toml` сжимается; записи для имён из таблицы удаляются.

### 5.3 ADR, документы и changelog

ADR-решения не меняются, меняются только идентификаторы в тексте. ADR-0086, ADR-0090, ADR-0092
и другие правятся на месте, в том же PR. Новый ADR «Имена в публичном API» (следующий
свободный номер) фиксирует правила N1/N2 владельца, правило «значение — поставщик `…Scope`»,
словарь `Raw*`/`Erased*`/`Driver*`, `*Callback`/`*Fn`, политику `#[doc(alias)]` и решения
О1–О5. Каждый ADR, исправленный на месте, получает строку `Имена обновлены по ADR-NNNN`. Так
правка явная, как требует ADR Policy, а решения не пересматриваются.

Цитаты тестов в документах (R11) правятся шагом 9 и проверяются классом `test-citation` гейта.

Фрагмент `changelog.d/naming-sweep.md`. Формат `changelog --check` разрешает под заголовком
только списки, вложенные списки допустимы, ссылки корневые:

```markdown
### Changed
- Public names follow the Rust API Guidelines (ADR-NNNN); renamed Flutter names stay
  searchable on docs.rs through `#[doc(alias)]`.
  - `flui-foundation`: `RRect` → `RoundedRect`, `RSuperellipse` → `RoundedSuperellipse`
  - …one nested bullet per crate
- Cargo features `test-utils` (flui-view) and `test-support` (flui-runtime) are now `testing`.
### Removed
- Unused `Disposable`, `GestureCallback`, `ElementExt`, `VoidCallback`, …; `ClipBehavior` (use `Clip`).
```

Вложенные пункты порождает `cargo xtask rename --changelog`: тот же источник, без ручного
пересказа.

### 5.4 Ветки в полёте

AGENTS требует вливать `main` в ветку, без rebase. Порядок для ветки, открытой до прохода:

1. До слияния, на своей ветке:
   `cargo xtask rename --apply --changed-since $(git merge-base HEAD origin/main)`. Инструмент
   переименовывает только файлы, которые изменила ветка. Они ещё целиком в старых именах,
   поэтому обмены применяются корректно. Коммит `naming: apply rename table`.
2. `git merge origin/main`. Где обе стороны переименовали одинаково, git сливает сам. Остальные
   конфликты разрешаются (mergiraf). Файлы, перемещённые через `git mv`, git сопоставляет по
   сходству.
3. `cargo xtask names`: класс `retired` находит старое имя, которое ветка внесла после шага 1.
   Затем `cargo xtask check-changed`.

Ветки, которые сейчас в работе (send-flip, text-ime, focus-keyboard, persistence, teardown,
render-proof), получают эту инструкцию в своих `tasks.md`. send-flip мержится раньше прохода и
переименования не делает.

### 5.5 Проверка

`cargo xtask names`; `cargo xtask ci` локально, плюс `cargo xtask cross-typecheck` (IOS/MacOS и
Win32 только компилируются), `cargo xtask wasm-check`, `cargo xtask facade-combos`,
`cargo test --doc`, `cargo xtask doc-strict`. Отдельно — `flui-sdk/tests/surface.rs` (R12) и
`cargo test -p flui-objects --test render_object_harness` (строки реестра `RenderClipRRect`,
`RenderMetaData`). Платформенные пути, которые host только компилирует, PR перечисляет явно.

### 5.6 Оценка и риски

| Работа | Дни |
|---|---|
| `xtask rename` (виды записей, подстановка, `--changed-since`, `--changelog`, самопроверка) | 1.5 |
| `xtask names` (14 классов, allowlist, `--seed`, самопроверка) | 1.5 |
| Применение и доводка до компиляции на всех целях | 1.5 |
| Документы, ADR, снимки, ревью собственного PR | 1 |
| **Итого** | **5.5** (из них 3 до 11-09, вне окна прохода) |

| Сценарий отказа | Что происходит | Защита |
|---|---|---|
| Метод с общим именем (`get_property`, `get_string`) переименован у чужого типа | Не компилируется | `scope` в записи + компилятор |
| Обмен применён дважды | `Theme` (значение) стал `ThemeScope` | Отказ инструмента (§5.1) |
| Имя в строке реестра (`"RenderClipRRect"`) | Тест харнесса падает | `strings` в записи; харнесс в §5.5 |
| Проводное имя (`TargetPlatform` в serde, `flui-protocol`) | Меняется сериализация | Исключение `#[serde(rename)]`; проверка PA1 |
| Фича переименована не везде | Ломается только дорожка CI с этой фичей | Запись `feature` правит все места; `ci-full` локально |
| Ветка внесла старое имя после прохода | Старое имя возвращается | Класс `retired` |
| Цитата теста в ADR | Ссылка ведёт в никуда | Класс `test-citation` |

## 6. Правила для AGENTS.md

Вставить раздел после «Writing tests» и строку в таблицу «What the compiler and gates enforce».

```markdown
## Naming

Names say what a thing is or does. `cargo xtask names` checks the first five rules; the rest is review.

- **No bag words** in files, directories, modules or public items: `util(s)`, `helper(s)`,
  `common`, `support`, `misc`, `kit`, `shared`, `base`, `core`, `impl(s)`, `generic`, `types`,
  `traits`, `mixin`. A module is named after its subject; a module for one type is private
  and re-exported.
- **C-CASE**: an acronym is one word (`Hsl`, `Ios`, `MacOs`, `RoundedRect`, not `RRect`);
  constants are `SCREAMING_SNAKE_CASE`. No `#[expect(non_*_case*)]` outside `generated.rs`.
- **Getters have no `get_`** (std's `get`, `get_mut`, `get_disjoint_mut`, `get_or_insert*` aside).
- **One concept, one word**: contexts are `…Context` bound as `cx`; a called `Fn` alias or
  wrapper is `…Callback`; a closure that builds a view is `…Fn`; `Builder` is only C-BUILDER;
  `Cancelled`, `Adapter`. No two public items of a crate share a name.
- **Tests are named for their outcome**, without a `test_` prefix.
- **A value is a noun; the view that provides it to descendants is `…Scope`** (`Theme` and
  `ThemeScope`). Object-safe forms are `Erased…`, lower contexts under a wrapper are `Raw…`.
- **Flutter's name is a reference, not a default.** Keep it only when a Rust developer who never
  used Flutter reads it right (`Padding`, `Column`, `Scaffold`); otherwise choose the Rust
  ecosystem's word and put the Flutter name in `#[doc(alias = "…")]` on the definition.
- `FooExt` only extends another crate's type or marks a capability or object-safety boundary;
  never a `…Trait` suffix. Test-only Cargo features are named `testing`.
```

Строка таблицы гейтов:

```markdown
| Bag words, C-CASE, `get_` getters, one spelling per concept, `…Context`/`cx`, `testing` features, test names, retired names from the rename table | `cargo xtask names` (allowlist `tools/xtask/allowlists/names.toml`, shrink-only) |
```

## 7. Гейт `cargo xtask names`

**Где.** `tools/xtask/src/names.rs` + `names/{classes,scan,tests}.rs`. Лексер берётся из
`markers`, элементы разбирает `syn`, таблицы гоняет `table_test.rs`. Таблица переименований
читается из `tools/xtask/renames/naming.toml`.

**Вход.** `git ls-files` по `crates packages src examples tools` и Markdown вне архивных корней.
Вне проверки: `docs/plans`, `docs/research`, `**/generated.rs`, `target/`, `.worktrees/`.

**Классы.** 1–10 — как в requirements.md («Гейт `cargo xtask names`»): `path-word`, `mod-word`,
`item-word`, `getter`, `acronym`, `case-expect`, `dup-name` (вместе с проверкой литерала
`debug_struct`, R16), `ctx-spelling`, `feature`, `test-name`. Дизайн уточняет один и добавляет
четыре класса:

- `ctx-spelling` проверяет и обратное: тип не-контекст с суффиксом `Context` — находка, если он
  не передаётся `&`/`&mut` параметром ни одной `pub fn`. Так ловится `NodeContext`.
- `cx-binding` (новый): параметр функции или замыкания с типом на `…Context` назван не `cx`,
  `_cx` или `<префикс>_cx` (`child_cx`). То же для времени жизни у типа-контекста (`'cx`).
- `retired` (новый): имя из колонки `old` таблицы переименований найдено в `.rs` или в code
  span неархивного Markdown. Исключения: значение `#[doc(alias)]`, `CHANGELOG.md`,
  `changelog.d/`, сама таблица. Обмены О1 (`Theme`) не проверяются: их старое имя законно
  живёт как новое.
- `alias` (новый): запись таблицы с `alias` имеет `#[doc(alias = "<значение>")]` на определении
  `new`. Обратное тоже находка: alias, которого нет в таблице и в allowlist.
- `test-citation` (R11): имя в code span неархивного Markdown, похожее на имя теста
  (`snake_case` с `_contract`, `_matrix`, `harness_` или совпадающее с известным `#[test]`),
  должно существовать как `fn` под `#[test]` или строка таблицы.

**Allowlist.** `tools/xtask/allowlists/names.toml` пишет `cargo xtask names --seed`. Ключ —
(`path`, `class`, `word`), `count` точный и только убывает. Находками считаются рост, убывание
без правки записи и запись, которой ничего не соответствует. Это та же механика, что в
`markers` и `file-length`, общий код — в `ratchet.rs`. Каждая запись несёт `reason` и `exit`.
`exit` — ADR или шаг спеки, после которого запись уходит, либо `keep`. `keep` допустим только с
`reason`, который ссылается на правило или решение (семейство `*Binding` — ADR-0083;
`ParentData` — N8; `LayoutContextApi` — renames.md §3). `--release-surface` (R10) падает на
любой записи не-`keep` в крейте, который публикуется в 0.2.0.

**Самопроверка `cargo xtask names --self-test`.** Таблица строк, по строке на случай. Строки из
requirements.md (`src/util.rs` → находка, `src/baseline.rs` → нет, `window_ext.rs` с
`WindowsWindowExt` → нет, `PointerEventExtTrait` → находка, `get_mut`/`get_disjoint_mut` → нет,
`get_layer` → находка, `HSLColor` → находка, `iOS` → находка, `#[expect(non_upper_case_globals)]`
в `curve.rs` → находка и в `generated.rs` → нет, два `WindowManager` → находка, но не в
`platforms/macos` против `platforms/windows`, фича `test-utils` → находка, `fn test_x` → находка,
файл в `docs/research/` не читается, allowlist больше, меньше или без совпадений → три находки)
плюс новые:

- `pub struct PaintCx` → `ctx-spelling`; `pub struct PaintContext` → нет;
  `pub type NodeContext = Rc<dyn Any>` → `ctx-spelling`;
- `fn layout(ctx: &mut BoxLayoutContext<…>)` → `cx-binding`; `fn layout(cx: …)` и
  `child_cx` → нет; `struct X<'ctx>` у контекста → `cx-binding`;
- `use flui::painting::RRect` → `retired`; `#[doc(alias = "RRect")]` → нет; `RRect` в
  `CHANGELOG.md` → нет; `Theme` при записи-обмене → нет;
- запись `alias = "RRect"` без атрибута на `RoundedRect` → `alias`; лишний
  `#[doc(alias = "Foo")]` → `alias`;
- `` `text_store_kit_matrix` `` в `ARCHITECTURE.md` без такого теста → `test-citation`;
  `` `harness_clip_rounded_rect_wraps_child` `` при существующем тесте → нет;
- удаление строки из таблицы классов роняет самопроверку (R4).

**Подключение.** Пары `("names --self-test", …)` и `("names", …)` в `tools/xtask/src/tasks/checks.rs`
рядом с `markers`, так гейт попадает в job `checks` агрегатора `ci`. Строка в таблице гейтов
AGENTS.md (§6). `typos.toml` `[default.extend-words]`: `canceled` и `adaptor` как опечатки
(N6). `include_str!` гейт не использует, `DOCS_ONLY` не меняется.

**Ввод в два шага.** PR-A (до 11-09): гейт с `--seed`. Allowlist содержит всё найденное, R14
сверяет счёт с инвентарём ±5 %, CI зелёный. PR прохода сжимает allowlist до записей `keep`.

## 8. Открытые вопросы (решает владелец)

- **О1. Значение и поставщик: обменивать ли `Theme`.** Рекомендация: `ThemeData` → `Theme`,
  `Theme` → `ThemeScope`, так же `MediaQuery`, `IconTheme`, `CupertinoTheme`, 17 `*ThemeData` →
  `*Theme` и `Default*` → `…Scope` (≈ 35 имён, 1100+ ссылок). Это самое заметное изменение для
  читателя с опытом Flutter, и старое имя `Theme` начинает значить другое. Компилятор ловит
  смену смысла: сигнатуры не совпадают. Альтернатива А: виджеты не трогать, а значения назвать
  `ThemeSpec`, `MediaQueryValues` (обмена нет, но `Spec`/`Values` — новый суффикс-шум).
  Альтернатива Б: оставить `*Data` с записью allowlist `keep` (requirements, В4).
- **О2. Виджеты `*Builder`.** Рекомендация: `FutureBuilder` → `FutureView`, `StreamBuilder` →
  `StreamView`, `LayoutBuilder` → `ConstraintsView`, `AnimatedBuilder` → `ListenableView`,
  `ValueListenableBuilder` → `ValueListenableView`, псевдонимы замыканий `…Builder` → `…Fn`
  (≈ 20 имён). Причина — C-BUILDER: в Rust `FooBuilder` строит `Foo`. Альтернатива А: имена
  Compose и Leptos (`WithConstraints` как `BoxWithConstraints`, `Await` как в Leptos).
  Альтернатива Б: оставить, раз за ними стоит самый узнаваемый словарь Flutter.
- **О3. `*Delegate`.** Рекомендация: трейт называется ролью — `MultiChildLayout`,
  `SingleChildLayout`, `FlowLayout`, `GridTiling` с `FixedCrossAxisCount`/`MaxCrossAxisExtent`,
  `PersistentHeaderContent`, `LocalizationsLoader`, `LazyChildren`; модуль `delegates` →
  `custom_layout` (≈ 16 имён). Минус: рядом трейты протокола `ChildLayout` и `BoxLayout`.
  Альтернатива: оставить `Delegate` (паттерн знаком по Cocoa и Kotlin), переименовать только
  `LocalizationsDelegate` → `LocalizationsLoader` и длинные `SliverGridDelegateWith…`.
- **О4. Коинеджи Flutter и Material.** Рекомендация: `Hero` → `SharedElement` (13 типов),
  `InkWell` → `Ripple`, `Material` → `Surface`, `ScaffoldMessenger` → `SnackbarHost`.
  Источники: Android/Compose shared element transitions, Compose `ripple()`, `Surface`,
  `SnackbarHost`. Минус `Surface`: в FLUI это слово уже означает GPU-поверхность
  (`SurfaceGeneration`, `SurfaceState` в engine и app). Альтернатива А: `Material` →
  `MaterialSurface`. Альтернатива Б: оставить `Hero` и `InkWell` (их знают и вне Flutter),
  переименовать только `ScaffoldMessenger`.
- **О5. `FocusManager` (В1 requirements).** Рекомендация: `FocusTree`, метод
  `LifecycleContext::focus_tree()`. Это дерево узлов фокуса со слушателями; затрагивает
  таблицу возможностей AGENTS.md и ADR-0078. `Focus` занят виджетом. Альтернатива:
  `FocusSystem`, или оставить с записью `keep` и причиной.

## Решения по открытым вопросам (2026-10-06)

- **О1 (владелец).** `Theme` — значение, `ThemeScope` — виджет-провайдер. Так же для всех ~35 пар:
  `MediaQuery`/`MediaQueryScope`, `*ThemeData` → `*Theme`.
- **О2 (владелец).** Виджеты `*Builder` → `*View`: `FutureView`, `StreamView`, `ConstraintsView` и т. д.
  Слово «builder» остаётся только за builder-паттерном (D6).
- **О3 (оркестратор).** Трейты `*Delegate` получают имена по роли, как предложено в таблице.
- **О4 (владелец).** `Hero` → `SharedElement`, `InkWell` → `Ripple`, `ScaffoldMessenger` →
  `SnackbarHost`; виджет `Material` сохраняет имя (`Surface` уже означает GPU-поверхность).
  Flutter-имена — `#[doc(alias)]`.
- **О5 (владелец).** `FocusManager` → `FocusTree`.

Остальная таблица следует правилам N1/N2. Владелец может вычеркнуть любые строки до прохода
(окно 11-10…11-14); без возражений таблица считается утверждённой. Новый ADR «Имена в публичном
API» и правка идентификаторов в текстах существующих ADR идут в PR прохода и требуют согласия
владельца на merge, как любой merge в `main`.

## Сделано раньше прохода (владелец, 2026-10-06)

«Realm» — внутренний архитектурный термин (прецедент — realm в ECMAScript: изолированное окружение,
несколько на одном потоке; совпадает с D4). Автор приложения его не встречает. Отдельным PR от `main`
(ветка `naming/window-policy`): `WindowPolicy::SeparateRealms` → `Isolated`, `SharedRealm` → `Shared`,
`flui_testing::HeadlessRealm` → `HeadlessHost`, модуль `testing::realm` → `testing::host`; старые
имена — `#[doc(alias)]`. `UiRealm` и термин в ADR/ARCHITECTURE.md остаются.
