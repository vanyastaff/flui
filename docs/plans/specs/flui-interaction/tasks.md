# flui-interaction — задачи (волна 1)

- **Статус:** в работе; текущий остаток сверяется с кодом и merged-PR
- **Дата:** 2026-10-06, база `main` @ `9a4daa3ed`
- **Источник:** [orchestration.md](orchestration.md), [matrix.md](matrix.md); ledger'ы этапа 1 — вне репозитория.
- **Правила:** задача = ветка `interaction/<slug>` = worktree = draft-PR. Каждый фикс: тест через
  публичный API, красный с откатом фикса (вывод в PR), `cargo xtask check-changed` зелёный.
  ID задач — только здесь.

## Текущее выполнение

Сверка 2026-10-08 по интеграционной базе `5f28646ad`, коду и именам тестов.
Локальный `check-changed` завершился exit 0; CI ещё не опубликован, слияние
в `main` не выполнено. I11 остаётся явно отложенной внешней зависимостью.
Пропущенные native платформы и physical pen/touch не объявляются проверенными.

Финальная команда на `5f28646ad`:

```text
cargo xtask check-changed --base d6ad274194483c6d1bc100f9a14d42e3890b6c0e
```

На Windows-хосте с build jobs 6 и test threads 4 прошли strict workspace
all-targets clippy, engine/testing clippy, driver 46/46 (31.961 s), workspace
793/793 (80.478 s; 62 skipped), strict private-items workspace rustdoc и
workspace doctests. Windows native all-targets с required optional features
strict clippy прошла (52.18 s); wasm workspace lib/bins strict clippy (15.93 s)
и facade no-default/hot-reload (9.99 s), обычный platform trybuild 1/1
(9.207 s) тоже прошли. Классифицированный план не запускал отдельную
cargo-hack matrix каждого feature.

Финальная native проверка macOS пропущена без cargo-zigbuild, iOS — без
genuine Apple SDK на macOS, Android — без NDK/CC/AR. Linux native execution
требует Linux/xvfb и пропущена. Более ранние narrow cross-compiles ниже
сохраняют свой источник и не подменяют эти пропуски. Physical Win32 pen/touch
activation остаётся CANNOT_VERIFY; ранее выполненные hidden-HWND и browser
smoke отделены от текущей компиляции.

| Задачи | Состояние | Доказательство или следующий шаг |
|---|---|---|
| I1, C1 | Реализованы | PR #1474 и #1494 merged; таблица `gesture_lifecycle_matrix` и property-тест `arena_settles_every_member_exactly_once` |
| I2, C2, R1–R4 | Реализованы | PR #1472 и #1500 merged; таблицы многоконтактных и реентерабельных жестов |
| I3, C3 | Численные исправления и перенос времени в потребителей реализованы; локальный gate прошёл; CI/merge впереди | PR #1479 merged; `velocity_and_resampling.rs`; C5 подключает время Up и `velocity_at` во всех четырёх производителях, целевые проверки и откаты прошли |
| I4, C4 | Реализованы в интеграционной ветке; локальный gate прошёл; CI/merge впереди | Hover/hit-test контракты включены и проходят; добавлены конкурирующие отказы и восстановление. Две независимые inverse-проверки hover retirement воспроизводят потерю first-failure authority и безопасного retirement хвоста; точные source-хунки восстановлены. ADR-0158 фиксирует cursor/finite-offset контракт, ADR-0127 — exceptional ownership |
| I5 | Конвейер реализован локально; системный источник настроек остаётся I11/LY8 | Owner-local binding сохраняет admission, claim, finite вход и coalesced history. Восемь slop-sensitive распознавателей учитывают измеренную историю до текущей позиции; origin-return, prediction control и восстановление проверены отдельными публичными строками и независимыми откатами admission. Authored settings доходят через `GestureArenaScope` до production-распознавателей. Целевые прогоны проверили binding, private resampling, 43 pointer- и 56 scroll-контрактов; allocator matrix проверена отдельно. Принятая доставка сохраняется при конкурирующих отказах |
| I6 | Реализована локально; локальный gate прошёл; CI/merge впереди | Уведомления продолжаются после паники, принятый запрос фокуса сохраняется, первая ошибка остаётся исходной. Все 28 строк `focus_actions_and_shortcuts` и public/private failure matrices проходят; откаты порядка siblings и provider containment воспроизводят нарушения. ADR-0160 и ADR-0165 |
| I7 | Реализована локально; локальный gate прошёл; CI/merge впереди | Lifecycle pause подключён к drain каждого input owner, deferred Down отменяется; публичные runtime-проверки прошли. Локальная проверка зависимых потребителей прошла; CI/merge впереди |
| I8 | Реализована локально; аппаратная проверка ограничена | Owned Win32 producer и owner-local MessageClock интегрированы; hidden-HWND Xbutton/coarse-clock и откаты прошли. Финальные decoder и fractional-wheel hidden-HWND проверки прошли. ForcePress отвергает mouse без датчика. Full pen/touch activation отказал (CANNOT_VERIFY) |
| I9 | Реализована локально; локальный wasm gate прошёл; CI/merge впереди | В живом Chrome после V15 проверены cancel/recovery, capture, дробные координаты и getter reentry. Контракт transformed canvas ограничен задокументированным fallback; общая поддержка DOM-трансформаций не заявлена |
| I10 | Реализована в интеграционной ветке; локальный gate прошёл; CI/merge впереди | Owner-local состояние, постоянный отказ при исчерпании signal ID и удержание отклонённого callback; lifecycle/property проверки проходят. ADR-0159 |
| I11 | Authored-settings consumer реализован; системный producer остаётся внешней зависимостью | `GestureArenaScope::settings` → `GestureDetector`/production builders проверен RED/GREEN и откатом. Не вводится дублирующий settings scope. `SystemPreferences` и его host/realm доставка выполняются отдельно через LY8; эта зависимость не закрыта текущей interaction-приёмкой. OS timings/slop и динамическое обновление не объявляются реализованными |
| C5 | Реализована в интеграционной ветке; PR ещё не опубликован | Четыре производителя используют время Up и `velocity_at`; строки движения/паузы/восстановления проходят и падают при откате production-hunk |
| C6 | Реализована | PR #1478 merged; типизированные Down/Up и `DeviceId(NonZeroU64)` в новом словаре |
| C7 | Реализована локально | Owner-local MessageClock использует `wrapping_sub` тиков; мёртвый `is_key_pressed` удалён. Реальная очередь hidden HWND и rollover прошли, обе проверки падают при откате и снова проходят после восстановления |
| S1, S2 | Основные миграции и compiler-контракты проверены; локальный gate прошёл; CI/merge впереди | Owned vocabulary, checked focus ID, typed focus contracts, RAII listeners, immutable Rc builders и production estimator selection подключены. Public gesture details защищены non-exhaustive compiler fixtures; обычный `trybuild_ui` прошёл все 12 fixtures без обновления expected stderr, включая external construction failures и законные constructors. HandlerId, ложная sealed-иерархия, team/standalone signal resolver, predictor и общий vocabulary bridge удалены по scope; `__runtime` остаётся намеренным контрактом ADR-0081 |
| S3, S4 | Source doctests, Markdown-примеры, allocator contracts и timing acceptance проверены; локальный gate прошёл; CI/merge впереди | Пустые `include_str!` модули удалены. На базе `3139a3193` all-features interaction source doctests: 51 runtime-пример и один compile-fail прошли, ignored нет. Прямая Markdown-проверка: README 4, GESTURES 3, HIT_TESTING 1 — все восемь прошли; ARCHITECTURE/PERFORMANCE не содержат executable examples, ignored нет. Runnable GestureDetector snippet проверен widget doctests: 33 runtime-примера и три compile-fail прошли; 13 ignored относятся к другим примерам. Окончательные пять wire-бенчей дали 33 полные BEFORE/AFTER пары и шесть AFTER-only cases; separate fresh five-estimate pair проверяет private LSQ kernel isolation. PERFORMANCE.md сохраняет means, confidence intervals, source revisions и объяснения regressions. Публичная allocator matrix ниже отделяет storage bounds от elapsed time; локальный gate прошёл; CI/merge впереди |
| S5 | Реализована локально; локальный gate прошёл; CI/merge впереди | DPI исправлен PR #1493; frame flush, drain и hover refresh обходят все input owners. Публичные runtime-проверки прошли |
| R5 | Текущий same-pointer контракт закреплён; локальный gate прошёл; CI/merge впереди | В `gesture_lifecycle_matrix` сохраняются `drag_cancel_callback_admits_the_next_contact_once` и `drag_cancelled_end_callback_admits_the_next_contact_once`. Таблица `drag_lifecycle_contracts` проверяет same-pointer replacement из terminal callback, первую панику и следующий Up; она прошла в restored-прогоне 49 связанных тестов на базе `502a8334f` до main merge. Этот прогон не объявляется пост-merge gate; исторический guard inverse не выдаётся за новый дефект изменённого drag |

RA0 подготовлена: живые baseline-бенчи сохранены, исходные E0277 и E0038 подтверждены.
RA1–RA4 интегрированы атомарно с публичными потребителями; обычный `trybuild_ui`
прошёл без обновления expected stderr: семь compile-fail fixtures (включая три
E0277 и non-exhaustive details) и пять успешных внешних fixtures. Исправления retirement и
diagnostics подтверждены откатом production-хунков. RA5 и ADR-0161 интегрированы,
RA6 ownership-измерения сохранены; окончательные wire timings записаны в
PERFORMANCE.md с source revisions, means и confidence intervals. Итоговый
gate зависимых потребителей прошёл; CI и слияние ещё впереди.
Задачи идут по графу `recognizer-api/tasks.md`. P1 словаря завершена; P2/P3 реализованы,
но финальная приёмка и ограничения producer smoke остаются в `pointer-vocabulary/tasks.md`.
Все 20 утверждённых NEW-строк повторно сверены в `scope-closure.md`: наличие реализации
отделено от отсутствующих inverse/нативных и итоговых проверок.

Restored-прогон 49 связанных тестов на базе `502a8334f` после восстановления
13 независимых inverse-хунков прошёл:

```text
cargo nextest run --locked -p flui-interaction -p flui-widgets -p flui-rendering -p flui-runtime -p flui-semantics -p flui-testing -E 'test(recognizer) | test(containment_and_isolation_matrix) | test(pointer) | test(scroll) | test(binding) | test(reveal) | test(resampl)' --no-fail-fast
```

Он включает `drag_lifecycle_contracts`, binding/private resampling,
43 pointer- и 56 scroll-строк, 15 runtime containment-строк и lower reveal.
Allocator test `resolved_route_move_invocation_allocates_no_heap_after_setup`
не совпадает с этим `test(...)` фильтром: слово pointer в имени test binary
не расширяет фильтр по имени теста. Отдельная allocator matrix, включая
последующие performance-исправления ниже, не включается в число 49.
Последующее слияние актуального `origin/main` в `8cfc0ea64` требует итоговой
проверки интеграции; этот более ранний целевой прогон её не заменяет.

После main merge и точного восстановления hover/debounce inverse-хунков
связанный прогон четырёх публичных семейств прошёл, включая настоящий
EditableText. Это целевая проверка текущих изменений, не итоговый gate.

Свежая строгая проверка Android-зависимых libraries на базе `64ab42b43`
завершилась с exit 0:

```text
cargo clippy -p flui-runtime -p flui-app -p flui --locked --target aarch64-linux-android --message-format=json -- -D warnings
```

Android здесь скомпилирован, не запущен. Команда использует default features
и не объявляет пройденными all-features/all-targets, native smoke или весь
platform gate.

## Публичные allocation contracts

Отдельный `resolved_route_move_invocation_allocates_no_heap_after_setup`
проверяет реальные route и resampler paths с обеими history и всеми sample
fields. Scalar Move, global route и точный identity transform из
`HitTestResult::add` дают ноль allocations. Translated и near-identity targets
с ненулевым смещением порядка `1e-6` сохраняют локализованные данные и bound
2/8/32 для 1/4/16 targets. До исправления exact identity копировал две history
(2 вместо 0); независимый inverse identity guard снова дал 2 вместо 0.
Оба случая восстановлены и отдельная публичная matrix прошла.

Sample и Stop с неизменённым временем передают принадлежащие resampler данные
без копирования history: ноль allocations, весь принятый Down/Move/Up и
полное равенство metadata. Контроль с действительно поднятым timestamp floor
сохраняет checked history policy и повторно проверяет predictions. До
исправления и при независимом inverse unchanged-time delivery копировал две
history (2 вместо 0); точный source-хунк восстановлен, matrix прошла.

Saturated admission ограничивает уже проверенную coalesced history через
`PointerMove::retain_latest_coalesced`, удаляя старый prefix без повторного
копирования. Публичная platform-api table проверяет limits 0/1/2/3/4/99,
разные readings с одинаковым coarse timestamp, metadata и predictions.
Production resampler задаёт cap 100; его публичный allocator case проверяет
не более одной allocation на saturated admission, все 199 сохранённых
readings в 99 Move, последние принятые данные, Down/Up и predictions.
До исправления было семь allocations; consumer-only inverse возвращает семь
при зелёной platform-api table. Точный consumer-хунк восстановлен, все девять
целевых тестов прошли. Последующий ownership transfer через
`PointerMove::try_coalesce_from` передаёт storage старого packet новому;
Несовпадение полного pointer identity не изменяет оба packet. Новый публичный case был RED:
три allocations при budget не более одной. После подключения production
consumer десять целевых тестов API, resampling, allocator и binding прошли.
Runtime containment этим прогоном не выбран: его matrix находится в
flui-testing main. Независимый producer-only inverse возвращает две
allocations при budget одной, тогда как новая API table остаётся GREEN;
точный source-хунк восстановлен, diff обратного отката пустой.
Этот bounded contract не обещает хранить бесконечную историю без потерь
и не подменяет окончательные elapsed-time measurements.

Borrowed-transform candidate дал means 234.505 против 216.349 для четырёх
targets (+8.39%) и 724.587 против 692.167 для 16 targets (+4.68%). Это
промежуточные elapsed-time regressions, а не улучшение и не окончательные
AFTER results. Окончательная парная проверка пяти бенчей выполнена на logical
CPU 0, normal priority: 33 полные BEFORE/AFTER пары и шесть AFTER-only cases.
PERFORMANCE.md сохраняет исходные means, 95% confidence intervals, revisions,
различия workload contracts и regressions; ускорение всех путей не заявляется.
После обнаруженного LSQ20 regression отдельный свежий paired run пяти
estimate cases на `c1dc17881` проверил изоляцию существующего private numerical
kernel: 557.446 против 566.027 ns для LSQ20 (-1.52%), LSQ3 +0.53%, четыре
queries +8.24%, Impulse -0.05%, Ios +237.51%. Ios сохраняет исправленный
eligible continuous-history window; старые timings не перезаписаны.
После точного восстановления production sources одиннадцать публичных тестов
прошли, включая настоящий flui-testing runtime containment. Этот прогон
отделён от десяти тестов выше, где containment не был выбран. Timing acceptance
записана и объяснена; локальный gate прошёл, CI и слияние остаются pending.

## Focus subscription в package-потребителе

Material TextField использует canonical `FocusSubscription`, экспортированный
через SDK. Публичный lifecycle case повторно подключает прежний внешний
FocusNode после replacement и после unmount, затем проверяет доставку через
реальный build inbox. Два независимых unsubscribe-only inverse воспроизводят
лишнюю доставку: pending external build 1 вместо 0 для заменённого и отдельно
для disposed field. Оба production-хунка восстановлены точно; целевые Material
и SDK surface проверки проходят. Более ранние paint-only и detached-node
inbox-only cases прошли и без исправления и не считаются inverse proof.
Это production package-потребитель observer-контракта I6, а не private-counter
проверка; целевой прогон не заменяет final gate зависимых потребителей.

## Hover retirement и double-tap debounce

Публичный `binding_input_contract_matrix` проверяет healthy metadata Drop,
одиночный callback failure, metadata failure, конкурирующие отказы,
queued/mismatched hover, реентерабельную замену и последующее восстановление.
Две независимые inverse-проверки разделяют разные containment обязанности:

- Без `HitTestEntry` Drop containment хвост metadata освобождается после
  первого отказа: счётчик 2 вместо 1; competing destructor case обрывает
  дочерний процесс.
- Без explicit retirement в hover dispatch, с сохранённым Drop containment,
  теряется authority сохранённого callback failure и обязательство доставки
  здоровому соседу. Это отдельный дефект от отсутствующего Drop guard.

Оба исходных production-хунка восстановлены в точности. Связанные публичные
семейства и локальный final gate прошли после восстановления; CI/merge впереди.

40 ms double-tap debounce реализован на frozen owner-clock snapshot первого
Up. Публичные Mouse/Touch строки проверяют 39 ms bounce, exact 40 ms,
удержанный первый контакт и здоровую повторную последовательность с тем же ID.
Независимый inverse нижней границы доставляет bounce callback при 39 ms
(1 вместо 0), сохраняя GREEN для exact 40 ms; production-хунк восстановлен.
Этот потребительский контракт не означает поставку OS double-click interval:
системный источник остаётся I11/LY8.

## Занятые файлы (не трогать без согласования)

| Файл | Кто | Решение |
|---|---|---|
| `fi/src/text_input.rs`, `fi/src/__runtime.rs` (text input) | ветка `text-ime/store-contract` | дефекты ввода текста (смена поля без сброса сессии IME, токен без владельца, исчерпание токена, `set_cursor_area` без проверки, повтор enable) передаются в text-ime карточкой, не правятся здесь |
| `flui-platform/src/platforms/windows/{window.rs,text_services/}` | ветка `text-ime/host-contract` | Win32-правки ввода — только в `events.rs`/`platform.rs` (ветки мыши) |
| `fi/src/routing/{focus,focus_scope}.rs` (Stale, история, порядок уведомлений) | focus-keyboard T3 | здесь — только изоляция паники листенеров и порядок Tab (RTL, строки), после сверки с T3 |
| `flui-runtime/src/ui_realm/frame.rs` | focus-keyboard T7, send-flip | мультиоконные правки — после них |

## Волна 1 (корректность, [P] — параллельно)

| ID | Задача | Файлы | [P] | Доказательство |
|---|---|---|---|---|
| I1 | Арена и tap/double/long press: потерянный удержанный tap; второй палец рвёт drag/long press; borrow `RefCell` через колбэк (dispose из колбэка); паника колбэка оставляет double/multi-tap и team в залипшем состоянии; арена без победителя, удержанный слот после `sweep` | `fi/src/arena/**`, `fi/src/recognizers/{tap,double_tap,long_press,drag,multi_tap,multidrag,recognizer}.rs`, новый `fi/tests/arena_*.rs`-модуль | [P] | строки таблиц; property-тест арены (proptest) |
| I2 | Scale, ForcePress, TapAndDrag, Eager: деление на ноль и NaN, вход в арену всех указателей, rebaseline при добавлении/снятии пальца, wrap угла, порядок вне `HashMap`; давление 0.5 без датчика; TapAndDrag в общей арене; borrow через колбэк | `fi/src/recognizers/{scale,force_press,tap_and_drag,eager}.rs` | [P] | строки таблиц, NaN-строки |
| I3 | Скорость и обработка: линейный fallback LSQ, время события вместо времени dispatch, ограничение fling (`clamp_fling_velocity` — знак, NaN, min>max), часы для 40 мс вместо `Instant::now`, resampler (время события, коэффициент, Up/Cancel никогда не теряются), `RawInputHandler` borrow, валидация predictor, NaN в pan-zoom | `fi/src/processing/**`, `fi/src/velocity.rs`, `fi/src/settings.rs`, `fi/src/pan_zoom.rs` | [P] | property-тест velocity/resampler (конечность, монотонность времени) |
| I4 | (зона T6d: только контрактные тесты + патч владельцу) Hover и hit-test: выход одного устройства гасит `on_exit` другого; `update_all_devices` — изоляция паники по устройствам; drop `Rc` под borrow; курсор «как у родителя» ≠ стрелка; `with_paint_offset` не принимает non-finite; локализация coalesced/predicted/scroll delta | `fi/src/routing/{mouse_tracker,hit_test}.rs` | [P] | строки таблиц |
| I5 | (зона T6d: контрактные тесты + передача) Binding: смена кнопки на активном указателе — не новая последовательность; промежуточные move сохраняются в `coalesced` и доходят до velocity; `DashMap` → однопоточное состояние; `GestureSettings` binding'а доходят до распознавателей; NaN/inf позиции отклоняются | `fi/src/binding.rs`, `fi/src/events.rs` (только helpers) | после I1 (drag время) | строки таблиц, бенч до/после |
| I6 | (зона T6d: контрактные тесты + передача) Фокус: паника листенера не обрывает остальных; порядок Tab с группировкой строк и RTL | `fi/src/routing/{focus,focus_scope}.rs` | после сверки с focus-keyboard T3 | строки таблиц |
| I7 | Runtime-шов: `handle_lifecycle_pause` вызывается на suspend; отложенный Down отменяется при потере фокуса окна; DnD не теряется молча | `flui-runtime/src/{held_input.rs,ui_realm/input.rs,ui_realm/presentations.rs}` | [P] | строки `addressed_input_routing` |
| I8 | Win32-шов (W): `SetCapture`/`ReleaseCapture`, `WM_CAPTURECHANGED` → Cancel; давление мыши без датчика не 0.5-как-сила (через I2-контракт); время события из `GetMessageTime` | `flui-platform/src/platforms/windows/{events.rs,platform.rs}` (ветки мыши) | [P] | Win32-unit на Windows-хосте, вывод в PR |
| I9 | Web-шов: `pointercancel`, `setPointerCapture`, дробные координаты | `flui-platform/src/platforms/web/events.rs` | [P] | `cargo xtask wasm-check`; unit без браузера — недоступно, так и записать |

## Волна 2 (после слияния волны 1; один владелец `lib.rs`)

| ID | Задача |
|---|---|
| S1 | Публичная поверхность: один `PointerEventExt`, настоящая запечатка, приватные конструкторы `FocusNodeId`/`HandlerId`, `DeviceId` без коллизий, `#[non_exhaustive]`, дубли `Key`/`Keyboard`, `get_`-префиксы и `bool`-параметр, `#[must_use]`, `__runtime` — doc-hidden public runtime-шов по ADR-0081 §4.2, скрытый на путях facade/SDK, маркер в `lib.rs:137` |
| S2 | Неподключённый `pub` (≈125 в арене/распознавателях, ≈30 в routing, processing): подключить или удалить — по scope-решению |
| S3 | Документация крейта: GESTURES/ARCHITECTURE/PERFORMANCE/HIT_TESTING/README под код; доктесты вместо `ignore` |
| S4 | Бенчи: исправить resampler-бенч, цифры до/после в PERFORMANCE.md, аллокации dispatch |
| S5 | Мультиоконность runtime: hover refresh, flush move, drain арены для всех окон; DPI в нужное окно |

## Поправки по «Конвенциям Rust» в ветках волны 1

Источник — [conventions-audit.md](conventions-audit.md), раздел B. Исполняются новыми коммитами
в тех же ветках (без force-push), до перевода PR из draft.

| ID | Ветка | Поправка |
|---|---|---|
| C1 | `interaction/arena-recognizer-lifecycle` | поздний вердикт — только через `TapArenaMember` своей последовательности (`tap.rs:925,932`); `event_nanos == 0` как «нет времени» → `Option` (`recognizer.rs:38,66`); `Arc<Mutex<TapSequences>>` → `RefCell` (`tap.rs:138`); одна функция кнопки tap (`tap.rs:725/768`); колбэки подряд — через общий `invoke_callback` |
| C2 | `interaction/multi-pointer-recognizers` | паника колбэка не оставляет жест со start без end (`force_press.rs:388-395`, `tap_and_drag.rs:292-310,497-502`); отклонённый контакт завершает scale (`scale.rs:991-1008`); NaN глобальной позиции (`tap_and_drag.rs:560-605`); `#[non_exhaustive]` на деталях с новыми полями; помощники сдерживания — в `recognizers/mod.rs`, без копии в `drag.rs` |
| C3 | `interaction/velocity-and-resampling` | окно и стоп-гейт — `Duration` (`velocity.rs:110-132`); `try_new` вместо молчаливого clamp (`settings.rs`, `prediction.rs:84-92`); `estimate_at`/`velocity_at`/`add_event_at` — подключить в распознавателях (C5) или не делать `pub`; ручные часы без `Instant::now()` (`sampling_clock.rs:118`) |
| C4 | `interaction/hover-and-hit-test` (зона T6d) | в патч владельцу: порядок устройств и возобновляемая паника не зависят от `HashMap` (`mouse_tracker.rs:536,368`); retirement по ADR-0127 (`:284,300,342`); drop старого значения после `RefMut` (`:408`) |
| C5 | после C1+C3 | распознаватели drag/multidrag/scale/tap_and_drag вызывают `velocity_at(время события)` на отпускании — стоп-гейт на шкале событий |
| C6 | `interaction/pointer-vocabulary-types` | мост против текущего API (`input_vocabulary.rs:144,266-272`); `Down` нельзя построить с `Released` (`pointer/mod.rs:551`); `DeviceId(NonZeroU64)` |
| C7 | `interaction/win32-mouse-capture` | время сообщения — через `wrapping_sub` тиков (передаётся владельцу `window.rs` вместе с полем `MessageClock`); мёртвый `util::is_key_pressed` удалить |

## Волна 1b (зона арены и распознавателей)

| ID | Задача | [P] |
|---|---|---|
| I10 | Арена: `DashMap` + `parking_lot::Mutex` внутри `!Send + !Sync` `GestureArena` → однопоточное хранилище (`RefCell` + слоты с поколением); `Arc` участника не роняется под lock слота (`arena/mod.rs:942,1221,1313`); порядок map/slot зафиксирован; `signal_resolver.rs:152` — `checked_add` | после I1 |
| I11 | Распознаватели: `GestureSettings` строится из `SystemPreferences::gestures()` (ADR-0151 §4, после platform-layer LY8) через `GestureSettingsScope`; `Arc<Mutex<GestureSettings>>` (10 мест) → `Cell<GestureSettings>`; `GestureSettings` из binding/виджета доходит до распознавателя (matrix X2) | после I1, I3 |

## Спека `recognizer-api/` (сквозной рефакторинг; подтверждена владельцем 2026-10-06)

- Builder до `Rc` вместо 45 методов `with_on_*(self: Arc<Self>)`; `Arc` → `Rc` у распознавателей.
- Свернуть `OneSequenceGestureRecognizer`/`PrimaryPointerGestureRecognizer` в поле-помощник;
  удалить ложные `Sealed`, `Disposable`, `GestureCallback`, `GestureRecognizerExt`,
  `CustomGestureRecognizer`, `CustomHitTestable`/`HitTestable`/`HitTestTarget` (последние три — зона T6d).
- `GestureRecognizer` — dyn-compatible точка расширения; `Listener::recognizer(..)` вместо ручной
  проводки в 4 виджетах; `add_pointer(dispatch)` вместо `add_pointer`/`add_pointer_with_kind`.

## Реентерабельность уведомлений распознавателей (после I2)

Колбэк, вошедший в тот же распознаватель, завершает или открывает последовательность, пока
уже собранные уведомления ещё доставляются. Общее решение — уведомление несёт поколение своей
последовательности; доставка прекращается после её завершения, но не после допуска следующей.

| ID | Сценарий | Файл |
|---|---|---|
| R1 ✅ | `on_start` снимает контакт при давлении выше пика: `Start → End → Peak`; так же `on_peak` перед `Update` | `recognizers/force_press.rs` |
| R2 ✅ | `on_start` снимает контакт при захвате: `Start → End → Update` | `recognizers/scale.rs` |
| R3 ✅ | `on_tap_down` допускает следующий контакт — поколение растёт и уже принятый `TapUp` теряется (счёт 2 без первого Up) | `recognizers/tap_and_drag.rs` |
| R4 ✅ | Самоуправляемая арена с соперником: Cancel у Eager делает sweep с семантикой Up и награждает соперника; отмена должна снимать поколение без победителя (и в других путях withdraw-and-sweep) | `recognizers/eager.rs`, `arena/**` |
| R5 — контракт закреплён | Исторический дефект: повторный допуск того же указателя из cancel-колбэка drag запускал жест дважды. Текущие exact-contact проверки сохраняют replacement; same-pointer reentry и следующий Up проверяются `drag_lifecycle_contracts`. Локальный post-merge gate прошёл; CI/merge впереди | `recognizers/drag.rs` |

## Измеренная история и gesture admission

Дополнение I5/M1-9/M2-V4 реализовано локально. Binding сохраняет измеренные
samples при объединении пакетов; Tap, DoubleTap, LongPress, MultiTap, Drag,
MultiDrag, TapAndDrag и Scale проверяют slop по всей доставленной measured
истории и текущей позиции. Движение 100 → 200 → 100 внутри одного кадра не
восстанавливает tap viability и не скрывает crossing у движущихся recognizers.
Predicted samples не участвуют в admission. Callback cadence, текущие local/root
координаты, event timeline и обычное arena ordering сохраняются; исторические
samples не порождают отдельные пользовательские callbacks.

Публичная таблица `tap_and_drag_resolves_through_the_shared_arena` проверяет
каждое из восьми семейств отдельно: два queued Move и authored coalesced packet,
следующий здоровый контакт с повторным pointer ID, prediction-only control и
реентерабельные Cancel/Down/Up из coalesced drag start. Для moving-семейств
сохраняется настоящий rival с большим slop, чтобы default arena win не подменял
проверку crossing. Составной Tap/Drag конфликт остаётся дополнительной строкой,
а не единственным доказательством каждого admission.

Все восемь независимых production-откатов воспроизвели ожидаемый отказ своей
строки: stationary cancellation или moving start оставались равны нулю вместо
одного. Prediction control проходил при каждом откате; точные production-хунки
восстановлены. До откатов полный recognizer-прогон прошёл. Итоговый расширенный
restored-прогон связанных семейств и локальный gate зависимых потребителей прошли.
`resampler_interpolates_on_event_time_and_never_drops_terminals` отдельно
проверяет три пакета с шестью measured samples и только newest prediction
family. Это локальные публичные binding/recognizer доказательства, не native
hardware smoke и не обещание безграничного окна velocity estimator.

Контрольный запуск: `cargo nextest run --locked -p flui-interaction
tap_and_drag_resolves_through_the_shared_arena --no-capture`.

## Минимальный интервал DoubleTap

Утверждённый M2-T4 закрыт локально: DoubleTap сохраняет время первого Up на
owner clock арены и допускает иначе подходящий второй Down начиная с 40 ms.
Более ранний Down игнорируется без потери удержанного первого verdict и без
перезапуска его timeout; последующий Up не допускает этот контакт задним
числом. Duration первого нажатия не подменяет интервал после первого Up.
Timeout и межконтактный slop по frozen settings сохраняют отдельную политику.

`tap_and_drag_resolves_through_the_shared_arena` проверяет Mouse/Touch,
39 ms refusal, exact40 admission, первое нажатие продолжительностью 0/250 ms
и следующую здоровую пару с повторным pointer ID. Baseline и независимый
production inverse воспроизвели лишний second-down callback на 39 ms; контроль
40 ms прошёл при откате. Точные production-хунки восстановлены; расширенная
публичная таблица, обычный trybuild и `tap_builder_lifecycle_contract` после
коррекции его нулевого timing premise прошли. Локальный gate зависимых
потребителей прошёл; CI и слияние остаются отдельными условиями приёмки.

## Порядок принятого motion перед Keyboard и IME

Дополнительная runtime-регрессия реализована локально: `UiRealm` доставляет
замороженный measured prefix перед наблюдающим Keyboard/IME без продвижения
frame clock и без завершения живых контактов. Keyboard сохраняет уже выбранную
активную presentation при реентерабельной смене фокуса; IME остаётся у
адресованной presentation и её действующего text store. Первая ошибка
сохраняется, а последующий принятый ввод и здоровые хвосты доставляются.
Долговечное решение дополнено в ADR-0163.

Все 15 публичных строк `flui-testing::containment_and_isolation_matrix`
подтверждены RED до соответствующего исправления и GREEN после него:

- Mouse/Touch с обоими resampling policy: measured координаты перед Key.
- Одиночный отказ motion, одиночный отказ Keyboard, конкурирующие отказы и
  следующая здоровая операция с настоящим contact terminal.
- Настоящий EditableText: commit наблюдает обновлённое motion-состояние;
  competing `on_changed` не заменяет более ранний отказ motion.
- Замороженные контакты, здоровый сосед после отказа, чтение motion-состояния
  из Key и новое реентерабельное движение в следующем barrier.
- Непрерывность настоящего GestureDetector Scale и snapshot resolved focus
  owner при смене активного окна из motion callback.
- Замена coalesced marker не стирает принятый prefix живого контакта;
  реентерабельный capture release доставляет frozen Move, последующий принятый
  tail и один CaptureLost, без дубликата от старого native Up.

Контрольный запуск: `cargo nextest run --locked -p flui-testing
containment_and_isolation_matrix --no-capture`. Это локальные public runtime и
headless widget доказательства, не native hardware smoke.

Пять независимых production inverse-проверок воспроизвели ожидаемые отказы:

- Без Keyboard wiring measured motion отсутствует до Key, а competing Key
  failure заменяет ожидаемую более раннюю ошибку motion.
- Без IME wiring настоящему EditableText `on_changed` доступно x=0 вместо
  x=30; competing owner failure заменяет ожидаемую ошибку motion.
- Если measured prefix каждого контакта забирается лишь перед его callback,
  реентерабельный B80 попадает в первый Key вместо следующей операции.
- Без coalesced prefix authority новый queued marker стирает frozen B40:
  обе строки direct replacement и capture release теряют этот Move.
- Без direct capture guard B40 теряется только в capture-release строке;
  исходный prefix, поздний tail и CaptureLost больше не доставляются в
  требуемом порядке.

Production-хунки после каждого отката восстановлены в точности. Restored
таблица всех 15 causal строк и полный recognizer-прогон ранее прошли; повторный
расширенный прогон связанных семейств и локальный gate зависимых потребителей
прошли. Точные optional/native ограничения указаны выше; CI ещё не опубликован.
Предыдущие ownership-измерения и historical baseline не подменяют текущие
парные wire-бенчи: их окончательные means и объяснения записаны в PERFORMANCE.md.
Native producer smoke сохраняет отдельные ограничения приёмки. Эти пять
доказательств не означают live hardware
проверку Keyboard/IME timing или нового input sampling поведения.
