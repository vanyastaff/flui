# flui-interaction — задачи (волна 1)

- **Статус:** в работе; текущий остаток сверяется с кодом и merged-PR
- **Дата:** 2026-10-06, база `main` @ `9a4daa3ed`
- **Источник:** [orchestration.md](orchestration.md), [matrix.md](matrix.md); ledger'ы этапа 1 — вне репозитория.
- **Правила:** задача = ветка `interaction/<slug>` = worktree = draft-PR. Каждый фикс: тест через
  публичный API, красный с откатом фикса (вывод в PR), `cargo xtask check-changed` зелёный.
  ID задач — только здесь.

## Текущее выполнение

Сверка 2026-10-08 по интеграционной базе `028df0a8f`, коду и именам тестов.
Отмеченные прогоны — целевые проверки интеграции. Итоговые `check-changed`,
optional-feature/platform gates, CI и слияние в `main` ещё не объявляются завершёнными.

| Задачи | Состояние | Доказательство или следующий шаг |
|---|---|---|
| I1, C1 | Реализованы | PR #1474 и #1494 merged; таблица `gesture_lifecycle_matrix` и property-тест `arena_settles_every_member_exactly_once` |
| I2, C2, R1–R4 | Реализованы | PR #1472 и #1500 merged; таблицы многоконтактных и реентерабельных жестов |
| I3, C3 | Численные исправления и перенос времени в потребителей реализованы; итоговый gate впереди | PR #1479 merged; `velocity_and_resampling.rs`; C5 подключает время Up и `velocity_at` во всех четырёх производителях, целевые проверки и откаты прошли |
| I4, C4 | Реализованы в интеграционной ветке; PR ещё не опубликован | Шесть контрактов включены и проходят; добавлены конкурирующие отказы и восстановление. ADR-0158 фиксирует cursor/finite-offset контракт |
| I5 | Конвейер реализован локально; системный источник настроек остаётся I11/LY8 | Owner-local binding сохраняет admission, claim, finite вход и coalesced history. Восемь slop-sensitive распознавателей учитывают измеренную историю до текущей позиции; origin-return, prediction control и восстановление проверены отдельными публичными строками и независимыми откатами admission. Authored settings доходят через `GestureArenaScope` до production-распознавателей. Restored-прогон проверил binding, private resampling, allocator, 43 pointer- и 56 scroll-контрактов; принятая доставка сохраняется при конкурирующих отказах |
| I6 | Реализована локально; итоговый gate впереди | Уведомления продолжаются после паники, принятый запрос фокуса сохраняется, первая ошибка остаётся исходной. Все 28 строк `focus_actions_and_shortcuts` и public/private failure matrices проходят; откаты порядка siblings и provider containment воспроизводят нарушения. ADR-0160 и ADR-0165 |
| I7 | Реализована локально; итоговый gate впереди | Lifecycle pause подключён к drain каждого input owner, deferred Down отменяется; публичные runtime-проверки прошли. Итоговая проверка зависимых потребителей впереди |
| I8 | Реализована локально; аппаратная проверка ограничена | Owned Win32 producer и owner-local MessageClock интегрированы; hidden-HWND Xbutton/coarse-clock и откаты прошли. Финальные decoder и fractional-wheel hidden-HWND проверки прошли. ForcePress отвергает mouse без датчика. Full pen/touch activation отказал (CANNOT_VERIFY) |
| I9 | Реализована локально; финальный wasm gate впереди | В живом Chrome после V15 проверены cancel/recovery, capture, дробные координаты и getter reentry. Контракт transformed canvas ограничен задокументированным fallback; общая поддержка DOM-трансформаций не заявлена |
| I10 | Реализована в интеграционной ветке; итоговый gate впереди | Owner-local состояние, постоянный отказ при исчерпании signal ID и удержание отклонённого callback; lifecycle/property проверки проходят. ADR-0159 |
| I11 | Authored-settings consumer реализован; системный producer не реализован | `GestureArenaScope::settings` → `GestureDetector`/production builders проверен RED/GREEN и откатом. Не вводится дублирующий settings scope. `SystemPreferences::gestures()` требует внешней Proposed LY8; OS timings/slop и динамическое обновление не объявляются реализованными |
| C5 | Реализована в интеграционной ветке; PR ещё не опубликован | Четыре производителя используют время Up и `velocity_at`; строки движения/паузы/восстановления проходят и падают при откате production-hunk |
| C6 | Реализована | PR #1478 merged; типизированные Down/Up и `DeviceId(NonZeroU64)` в новом словаре |
| C7 | Реализована локально | Owner-local MessageClock использует `wrapping_sub` тиков; мёртвый `is_key_pressed` удалён. Реальная очередь hidden HWND и rollover прошли, обе проверки падают при откате и снова проходят после восстановления |
| S1, S2 | Основные миграции интегрированы; итоговая проверка поверхности впереди | Owned vocabulary, checked focus ID, typed focus contracts, RAII listeners, immutable Rc builders и production estimator selection подключены. HandlerId, ложная sealed-иерархия, team/standalone signal resolver, predictor и общий vocabulary bridge удалены по scope; `__runtime` остаётся намеренным контрактом ADR-0081 |
| S3, S4 | Source doctests прошли; итоговые Markdown-проверки и бенчи впереди | Пустые `include_str!` модули удалены. На базе `3139a3193` all-features source doctests: 51 runtime-пример и один compile-fail прошли, ignored нет. Прямой повторный прогон восьми Markdown примеров ещё не объявляется завершённым. Предыдущие ownership timings относятся к прежней форме событий и не подменяют текущие wire-бенчи. Counting-allocator контракт `resolved_route_move_invocation_allocates_no_heap_after_setup` проверяет ноль аллокаций scalar Move и не более двух на каждый translated target с обеими history: измерены 2/8/32 для 1/4/16 targets, с проверкой всех sample fields и global history; PERFORMANCE.md описывает этот bound отдельно от elapsed time |
| S5 | Реализована локально; итоговый gate впереди | DPI исправлен PR #1493; frame flush, drain и hover refresh обходят все input owners. Публичные runtime-проверки прошли |
| R5 | Текущий same-pointer контракт закреплён; итоговый gate впереди | В `gesture_lifecycle_matrix` сохраняются `drag_cancel_callback_admits_the_next_contact_once` и `drag_cancelled_end_callback_admits_the_next_contact_once`. Таблица `drag_lifecycle_contracts` проверяет same-pointer replacement из terminal callback, первую панику и следующий Up; она прошла в restored-прогоне 49 связанных тестов на базе `502a8334f` до main merge. Этот прогон не объявляется пост-merge gate; исторический guard inverse не выдаётся за новый дефект изменённого drag |

RA0 подготовлена: живые baseline-бенчи сохранены, исходные E0277 и E0038 подтверждены.
RA1–RA4 интегрированы атомарно с публичными потребителями; обычный `trybuild_ui`
проверяет три E0277 и два успешных внешних extension-контракта. Исправления retirement и
diagnostics подтверждены откатом production-хунков. RA5 и ADR-0161 интегрированы,
RA6 ownership-измерения сохранены; итоговый gate зависимых потребителей и текущие wire-бенчи впереди.
Задачи идут по графу `recognizer-api/tasks.md`. P1 словаря завершена; P2/P3 реализованы,
но финальная приёмка и ограничения producer smoke остаются в `pointer-vocabulary/tasks.md`.
Все 20 утверждённых NEW-строк повторно сверены в `scope-closure.md`: наличие реализации
отделено от отсутствующих inverse/нативных и итоговых проверок.

Restored-прогон 49 связанных тестов на базе `502a8334f` после восстановления
13 независимых inverse-хунков прошёл:

```text
cargo nextest run --locked -p flui-interaction -p flui-widgets -p flui-rendering -p flui-runtime -p flui-semantics -p flui-testing -E 'test(recognizer) | test(containment_and_isolation_matrix) | test(pointer) | test(scroll) | test(binding) | test(reveal) | test(resampl)' --no-fail-fast
```

Он включает `drag_lifecycle_contracts`, binding/private resampling, allocator,
43 pointer- и 56 scroll-строк, 15 runtime containment-строк и lower reveal.
Последующее слияние актуального `origin/main` в `8cfc0ea64` требует итоговой
проверки интеграции; этот более ранний целевой прогон её не заменяет.

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
| R5 — контракт закреплён | Исторический дефект: повторный допуск того же указателя из cancel-колбэка drag запускал жест дважды. Текущие exact-contact проверки сохраняют replacement; same-pointer reentry и следующий Up проверяются `drag_lifecycle_contracts`. Итоговый post-merge gate остаётся впереди | `recognizers/drag.rs` |

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
restored-прогон и gate зависимых потребителей ещё не объявляются завершёнными.
`resampler_interpolates_on_event_time_and_never_drops_terminals` отдельно
проверяет три пакета с шестью measured samples и только newest prediction
family. Это локальные публичные binding/recognizer доказательства, не native
hardware smoke и не обещание безграничного окна velocity estimator.

Контрольный запуск: `cargo nextest run --locked -p flui-interaction
tap_and_drag_resolves_through_the_shared_arena --no-capture`.

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
расширенный прогон связанных семейств, итоговый gate зависимых потребителей,
optional-feature/platform gates и CI ещё не объявляются завершёнными.
Предыдущие ownership-измерения и historical baseline не доказывают performance
нового barrier: актуальные wire-бенчи и native producer smoke сохраняют свои
отдельные задачи приёмки. Эти пять доказательств не означают live hardware
проверку Keyboard/IME timing или нового input sampling поведения.
