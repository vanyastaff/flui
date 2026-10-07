# flui-interaction — задачи (волна 1)

- **Статус:** черновик
- **Дата:** 2026-10-06, база `main` @ `9a4daa3ed`
- **Источник:** [orchestration.md](orchestration.md), [matrix.md](matrix.md); ledger'ы этапа 1 — вне репозитория.
- **Правила:** задача = ветка `interaction/<slug>` = worktree = draft-PR. Каждый фикс: тест через
  публичный API, красный с откатом фикса (вывод в PR), `cargo xtask check-changed` зелёный.
  ID задач — только здесь.

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
| S1 | Публичная поверхность: один `PointerEventExt`, настоящая запечатка, приватные конструкторы `FocusNodeId`/`HandlerId`, `DeviceId` без коллизий, `#[non_exhaustive]`, дубли `Key`/`Keyboard`, `get_`-префиксы и `bool`-параметр, `#[must_use]`, `__runtime` скрыт от внешних крейтов, маркер в `lib.rs:137` |
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
