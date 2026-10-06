# flui-interaction до рыночной нормы — оркестрация

- **Статус:** в работе
- **Дата:** 2026-10-06
- **База:** `main` @ `9a4daa3ed`
- **Смежные спеки:** [../focus-keyboard/](../focus-keyboard/), [../text-ime/](../text-ime/),
  [../release/](../release/) — задачи, уже назначенные там, здесь не дублируются, а
  ссылаются.

## Цель

Крейт `flui-interaction` соответствует норме UI-фреймворка, а не MVP: им завтра пользуются
сторонние разработчики на всех заявленных платформах. Pre-1.0 API можно ломать — только с
миграцией всех вызовов в workspace, фрагментом `changelog.d/` и ADR при смене межкрейтового
контракта.

**Готово, когда:**

1. Матрица рыночного эталона ([matrix.md](matrix.md)): каждая строка «есть» или вынесена в
   scope-решение, утверждённое владельцем.
2. Каждый найденный дефект и разрыв — draft-PR: тест, падающий без фикса (вывод в PR),
   зелёный `cargo xtask check-changed`, SHA.
3. Property-тесты арены и velocity/resampler проходят.
4. Бенчи до/после на одном окружении приведены.
5. `crates/flui-interaction/docs/*` и README соответствуют коду.

Без merge, force-push, правок `.github/workflows/` и ослабления gates.

## Этапы

1. **Факты.** Read-only ledger по зонам: (Z1) `arena/` + `recognizers/`; (Z2) `routing/`;
   (Z3) `processing/` + `velocity.rs` + `pan_zoom.rs`; (Z4) модель данных и публичная
   поверхность; (Z5) `text_input.rs` + `clipboard.rs`; (Z6) тесты, бенчи, примеры;
   (Z7) потребители и путь «событие ОС → колбэк». Каждый вывод — файл:строка.
2. **Эталон.** (M1) указатель, колесо, тачпад; (M2) жесты, арена, velocity; (M3) hit-test,
   hover, фокус, клавиатура, IME, a11y, RTL, high-DPI, несколько окон. Первичные источники —
   чек-лист, не шаблон (AGENTS.md: FLUI — не порт Flutter). Итог — `matrix.md`.
3. **Спеки.** Значимый разрыв — `<topic>/{requirements,design,tasks}.md`; мелкий фикс —
   карточка в [tasks.md](tasks.md). Design проходит adversarial review: reentry, отмена
   посреди последовательности, гонка двух указателей, unmount во время жеста, panic в колбэке,
   NaN/переполнение, неподдерживаемое устройство.
   Приоритет: корректность → недостающее поведение → производительность → эргономика.
4. **Реализация.** Сначала задача-контракт (публичные типы + падающие тесты), затем все
   `[P]`-задачи, каждая в своей ветке/worktree. Один владелец у общих файлов: `Cargo.toml`
   крейта, `lib.rs`, `tests/main.rs`, `docs/ARCHITECTURE.md`.

## Правила (проверяются на ревью)

- Rust API Guidelines; `#[non_exhaustive]` на публичных enum событий и деталей; newtype-ID;
  enum вместо `bool`-параметров; builders вместо setter/getter.
- Нет lock на per-node состоянии в горячем пути dispatch; guard не в публичных сигнатурах и не
  держится при вызове пользовательского кода. `dashmap`/`parking_lot` — только при реальной
  межпоточности.
- Пользовательский код реентерабелен; panic в колбэке не ломает арену и router, следующее
  событие обрабатывается.
- Состояние принадлежит realm/окну; новых `static` нет (`cargo xtask globals`).
- ADR-0098: логические и device-пиксели не смешиваются; velocity/LSQ/scale не публикуют
  NaN/inf.
- `thiserror`, без `unwrap` в production (docs/PANIC-POLICY.md).
- Нет неподключённого `pub`; rustdoc на каждом публичном элементе.
- Тесты через публичный API (`tests/main.rs`), семейства — таблицы; синтетические события —
  как их выдаёт платформа. Каждый фикс: тест красный с откатом фикса. Failure-path матрица:
  каждая точка отказа, две в конкуренции, следующее событие после восстановления.
- Платформы: Linux исполняется; Win32 — прогон на Windows-хосте; AppKit/Android/iOS — только
  `cargo xtask cross-typecheck`. Отчёт различает: прочитано / скомпилировано / запущено /
  прошло / упало / недоступно.
- ID требований и задач — только в `docs/plans`, не в коде, тестах и коммитах.

## Конвенции Rust

Toolchain 1.99.0, edition 2024 (MSRV = channel). Версия у std-средства — строка из
`rust-lang/rust` `RELEASES.md`; пробная компиляция на 1.99 — см. «Проверка» ниже.
Правила `AGENTS.md` и `docs/PANIC-POLICY.md` старше этого раздела.

**Типы вместо проверок**
- ID — newtype над `NonZero<u64>` (1.79), приватный конструктор, выдача `checked_add` с
  постоянным отказом при исчерпании; слоты арены — generational key. `pub type DeviceId = i32`
  (`ids.rs:223`) заменяется `DeviceId(NonZeroU64)` словаря P1.
- Время — `EventTime`/`Duration`, не f64-секунды/мс; сравнение окна и стоп-гейта —
  `Duration` с `Duration` (`processing/velocity.rs:341` сейчас сравнивает `as_secs_f64()*1000.0`).
- Диапазонная величина — validated newtype с `try_new -> Result<_, thiserror-enum>` (образец —
  `flui-platform-api::pointer::value`); «нет датчика» — `None`, не 0.5/π/2/1×1.
- `enum` вместо `bool`-параметра и `bool`-результата (`FlingGate`, `TraversalDirection`,
  `KeyEventResult` вместо `-> bool`); сентинел (`0` = нет времени, `Offset::ZERO` = нет позиции,
  `CursorIcon::Default` = наследовать) — `Option`/enum.
- Закрытые наборы — sealed trait; иерархии Flutter (`OneSequence → PrimaryPointer`, `Disposable`)
  не воспроизводятся: общее поведение — поле-структура, освобождение — `Drop`.
- `#[non_exhaustive]` на публичных событиях, деталях, ошибках и исходах (сейчас 21 enum без него,
  в т.ч. `FocusTreeError`, `FocusRequestOutcome`); exhaustive оставляется только у закрытого по
  смыслу (`GestureDisposition`, `ButtonTransition`) — с фразой в rustdoc. `#[must_use]` — на
  исходах, guard'ах, токенах и `Option`-отказах (`with_paint_offset`).

**Владение**
- Owner-local (send-flip a1/b2): `Rc` + `RefCell`/`Cell`; `Cell::update` (1.88) для счётчиков.
  `Arc`/`Mutex`/`DashMap`/`parking_lot` — только для реально межпоточного (`FrameWaker`).
  Нарушают: `GestureArena` (`!Send+!Sync`, `arena/mod.rs:1777`, но `DashMap`+`Mutex` внутри),
  10 × `Arc<Mutex<GestureSettings>>` в распознавателях, `with_on_*(self: Arc<Self>)`.
- **Ни один borrow/guard не жив при вызове пользовательского кода.** Правило edition 2024
  (Edition Guide `if_let_rescope`/`tail_expr_drop_order`; RELEASES 1.91 распространяет его на
  `pin!`/`format_args!`/`write!`; пробная компиляция — см. «Проверка»;
): временные из скрутини `if let` живут **весь then-блок** и
  отпускаются до `else` (в 2021 — и в `else`); скрутини `match` и `while let` — **все ветки/тело**;
  условие обычного `if` — до входа в ветку; `let x = c.borrow().f();` — до `;` (копирование
  `.copied()` в скрутини срок **не** сокращает); хвостовое выражение блока отпускает временные
  до локальных переменных блока (2024). Отсюда форма: `let v = cell.borrow().get(..).cloned();`
  затем `if let Some(v) = v { callback(v) }`. Присваивание `x.f = c.borrow_mut().take()` дропает
  старое `x.f` под живым `RefMut` — сначала `let`. Решение копится в enum-исходе под borrow,
  колбэки — после (`DeferredResolution`, `Outcome`/`Notice`).
- Подписки и регистрации — RAII-guard (`Subscription`, `#[must_use]`, `Drop` снимает) вместо пар
  `add_listener/remove_listener`, `register_*/unregister_*`.
- `clone()` ради borrowck не допускается; клон `Rc` колбэка перед вызовом — допустим и
  предпочтителен (это и есть «отпустить borrow до user code»).

**Ошибки и паники**
- Публичный отказ — `Result<_, E>`, `E: thiserror::Error + #[non_exhaustive]`; диапазонный
  параметр — `try_new` (молчаливый clamp/замена в `GestureSettings`/`PredictionConfig` — нет).
- `expect("BUG: …")` — только на инварианте модуля; `debug_assert!` не единственная защита от
  входа вызывающего (`recognizer.rs:249`, `focus.rs:400`).
- `f64::clamp` паникует при `min > max` или NaN-границе: границы проверяются до вызова.

**API**
- C-COMMON-TRAITS (`Debug, Clone, Copy, PartialEq, Eq, Hash` где применимо), C-CONV-TRAITS
  (`From`/`TryFrom` для ID), C-GETTER (без `get_`: `velocity()`, `hit_test()`), C-BUILDER
  (builder до `Rc`, не setter/fluent на разделённом объекте), `impl Fn(..) + 'static` в
  аргументах, `impl Iterator` (+ `use<..>`, 1.82; в трейтах 1.87) вместо `Vec` в результатах.
  `SmallVec`/`Cow` — только с бенчем.

**Арифметика**
- Счётчики/ID/поколения — `checked_add` (+ `AtomicU64::try_update`, 1.95, для глобальных по
  ADR-0097); `strict_*` (1.91) — где переполнение есть BUG. Нарушение:
  `arena/signal_resolver.rs:152` `next_handler_id += 1`.
- Сортировка/мин-макс f64 — `total_cmp` (1.62); `midpoint` (f64 и беззнаковые — 1.85, знаковые —
  1.87); `==` на вычисленных f64 — нет (`velocity.rs:247`). Порядок, наблюдаемый пользователем,
  не зависит от `HashMap` (`scale.rs:555,579`, `mouse_tracker.rs:245,395,559`).
- Каст: `try_from`/`From` вместо `as` для целых; `as` из f64 — только после `clamp`/проверки,
  с комментарием. Ни одно значение non-finite не публикуется: проверка на входе (`try_new`).

**unsafe** — в крейте нет (`rg unsafe` пусто); если появится, `SAFETY:` называет
установленный здесь инвариант.

**Документация** — `# Errors` на каждом `Result` (сейчас 33 без), `# Panics` на каждом
`expect`/`panic!` пути (8 без), `# Examples` компилируемые, с `?` (C-QUESTION-MARK); 50 блоков
`ignore`/`no_run` в `src/` переводятся в компилируемые (S3).

**Линты** (измерено `clippy 0.1.99` на `main` `9a4daa3ed`, lib / all-targets; pedantic уже
включён workspace'ом). Cargo не смешивает `[lints] workspace = true` с собственными
записями крейта, поэтому включаются `#![warn(clippy::…)]` в `lib.rs` (после чистки, отдельным PR):

| Линт | lib | all | Решение |
|---|---|---|---|
| `iter_over_hash_type` | 5 | 5 | **warn** — ловит только `for` по `HashMap`, не `.values()`-цепочки |
| `missing_errors_doc` | 33 | 33 | **warn** (крейт перекрывает workspace `allow`) |
| `missing_panics_doc` | 8 | 8 | **warn** |
| `allow_attributes_without_reason` | 16 | 20 | **warn**; `allow_attributes` — 0 |
| `exhaustive_enums` | 21 | 22 | разовый аудит в S1, не lint (закрытые enum законны) |
| `significant_drop_tightening` | 4 | 5 | **warn** (nursery; guard дольше нужного) |
| `unwrap_in_result` | 2 | 2 | **warn** |
| `float_cmp` | 1 | 1 | **warn** для крейта (workspace `allow` — ради layout; здесь f64 — скорости) |
| `let_underscore_must_use` | 2 | 16 | **warn** |
| `clone_on_ref_ptr` | 17 | 103 | warn для lib (`Rc::clone(&x)` видно в ревью); тесты — `allow` |
| `cast_possible_truncation` / `cast_lossless` / `as_conversions` | 1 / 2 / 12 | 5 / 3 / 39 | первые два — **warn** (почти чисто); `as_conversions` — нет |
| `missing_const_for_fn` 117, `must_use_candidate` 209, `return_self_not_must_use` 17, `std_instead_of_core` 186, `arithmetic_side_effects` 89, `indexing_slicing` 60, `float_arithmetic` 91, `default_numeric_fallback` 60 | | | **не включать**: шум для графического/геометрического кода; `#[must_use]` ставится по правилу выше, не линтом |
| `float_cmp_const`, `large_types_passed_by_value`, `trivially_copy_pass_by_ref`, `arc_with_non_send_sync`, `rc_buffer`, `mutex_atomic`, `manual_midpoint`, `lossy_float_literal`, `infinite_loop` | 0 | 0 | **warn** как регрессионная планка (бесплатно) |

**Проверка (2026-10-06, rustc 1.99.0).** Все std-средства, упомянутые в разделе, взяты из
строк `RELEASES.md` с версией и собраны пробным крейтом edition 2024 (`cargo check` — exit 0;
`assert_matches!` — по пути `std::assert_matches!`). Правило временных значений измерено
`RefCell::try_borrow_mut` внутри каждой формы: borrow скрутини жив в then-блоке `if let` — да,
в `else` — нет, во всех ветках `match` — да, в теле `while let` — да, после `let x = c.borrow().f();`
— нет, в ветке обычного `if` — нет; `.copied()` в скрутини срок не сокращает; форма
«сначала `let`, потом `if let`» — borrow снят. Тот же крейт в edition 2021 не компилируется на
хвостовом `c.borrow().len()` (E0597) — подтверждение порядка drop хвостового выражения в 2024.

Паттерны по темам и формы других фреймворков с заменой — [patterns.md](patterns.md); сверка
спек и веток волны 1 — [conventions-audit.md](conventions-audit.md).

## Журнал

Рабочий журнал (зона, задача, ветка, SHA, статус, доказательство) — в git-ignored
`TASKS.md` в корне worktree. Здесь фиксируются только утверждённые scope-решения.

## Scope-решения (утверждённые владельцем)

- **2026-10-06, словарь указателя — в этой работе** (ADR-0089: Stable-крейты без upstream-типов;
  иначе `ui-events` замерзает в стабильном API). Спека `pointer-vocabulary/`, три части:
  - **P1.** Свои типы событий в `flui-platform-api` (+ ADR или ревизия ADR-0089): поля под строки
    матрицы, `#[non_exhaustive]`; `ui-events` остаётся только внутри бэкендов как источник
    конвертации. Поведение не меняется.
  - **P2.** Перевод `flui-interaction` и facade на новые типы — после P1, с согласованием файлов с
    I1–I4.
  - **P3.** Производители по бэкендам, каждый отдельной `[P]`-задачей: Win32 (`WM_POINTER`,
    touch/pen) и winit — с прогоном на этом хосте; macOS/iOS/Android — `cross-typecheck`, в
    матрице помечены «скомпилировано, не запущено».
- **2026-10-06, файлы ветки `text-ime/host-contract`** (`windows/window.rs`,
  `windows/text_services/**`) не трогаются до её merge; нужные там изменения передаются её
  владельцу.
- **2026-10-06, сборки:** одна сборка на хост одновременно; писать параллельно, собирать по очереди
  (или в облачных сессиях).
- **2026-10-06, граница с send-flip T6d:** T6d владеет `crates/flui-interaction/**` кроме
  `recognizers/**` и арены (`Rc`-слушатели, hit-test, перевод на `!Send`). Здесь в её зоне —
  только контрактные тесты `#[ignore = "contract: <поведение>"]` и проверенный патч фикса,
  переданный владельцу T6d. `recognizers/**`, `arena/**` и словарь указателя (ADR-0089) — эта
  работа; Win32 `window.rs` — через владельца `text-ime/host-contract`. Численные фиксы
  `processing/`, `velocity.rs`, `settings.rs`, `pan_zoom.rs` остаются здесь (не касаются
  владения и `Send`); замены `Arc<Mutex>`/`DashMap` на owner-local — T6d.
- **2026-10-06, сквозной рефакторинг** — отдельная спека с подтверждением владельца; правки
  конвенций — только в коде, который и так меняется по спекам.
- **2026-10-06, воспроизведение:** каждый фикс начинается с теста, воспроизводящего дефект через
  публичный путь; не воспроизвёлся — гипотеза, без фикса.
