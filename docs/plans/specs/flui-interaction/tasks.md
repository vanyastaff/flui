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
| I4 | Hover и hit-test: выход одного устройства гасит `on_exit` другого; `update_all_devices` — изоляция паники по устройствам; drop `Rc` под borrow; курсор «как у родителя» ≠ стрелка; `with_paint_offset` не принимает non-finite; локализация coalesced/predicted/scroll delta | `fi/src/routing/{mouse_tracker,hit_test}.rs` | [P] | строки таблиц |
| I5 | Binding: смена кнопки на активном указателе — не новая последовательность; промежуточные move сохраняются в `coalesced` и доходят до velocity; `DashMap` → однопоточное состояние; `GestureSettings` binding'а доходят до распознавателей; NaN/inf позиции отклоняются | `fi/src/binding.rs`, `fi/src/events.rs` (только helpers) | после I1 (drag время) | строки таблиц, бенч до/после |
| I6 | Фокус: паника листенера не обрывает остальных; порядок Tab с группировкой строк и RTL | `fi/src/routing/{focus,focus_scope}.rs` | после сверки с focus-keyboard T3 | строки таблиц |
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
