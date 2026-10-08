# pointer-vocabulary — задачи

- **Статус:** P1 завершена (PR #1478 merged); P2 и производители P3 реализованы в интеграционной ветке; итоговые gates и платформенная проверка остаются условиями приёмки
- **Дата:** 2026-10-06
- **Дизайн:** [design.md](design.md); требования — [requirements.md](requirements.md)
- **Правила:** задача = ветка `interaction/<slug>` = worktree = draft-PR; `[P]` — можно
  параллельно с соседними `[P]` той же части. ID требований и задач — только здесь, не в коде,
  тестах и коммитах. Каждая сборка — через общий замок хоста. Win32 — прогон на Windows-хосте
  с выводом в PR; macOS/iOS/Android — только `cargo xtask cross-typecheck`, в матрице
  «скомпилировано, не запущено»; web — `cargo xtask wasm-check`.

## Текущая сверка

Сверка исходников 2026-10-07 на интеграционной базе `3cf7329c6`.
`PlatformInput`, binding, routing, распознаватели, runtime, widgets, testing и facade
используют owned-словарь. Общий pointer-мост удалён; каждый backend строит события
на своей границе. Сохранённый приватный keyboard adapter использует mature upstream
таблицы клавиш, не возвращая upstream-типы в публичные сигнатуры.

| Задачи | Реализация | Остаток проверки |
|---|---|---|
| V1–V4 | Типы и генератор уже merged; ADR-0143 принят | Исторический P1-мост заменён прямыми производителями |
| V5–V7 | Owned-события проходят production-конвейер; единицы scroll доходят до viewport | Итоговый gate зависимых потребителей и optional features |
| V8 | Миграция и changelog интегрированы | `cargo xtask check-changed` ещё не объявлен завершённым |
| V9 | Win32 pointer/mouse/keyboard производители интегрированы | Hidden-HWND Xbutton/coarse-clock проверки и откаты прошли; pen/touch activation отказал, поэтому CANNOT_VERIFY. Fractional wheel smoke не выполнен |
| V10 | winit producer интегрирован | Финальная optional-feature проверка и live smoke по доступности |
| V11–V13 | macOS/iOS/Android producer интегрированы | Предыдущая cross-typecheck прошла: скомпилировано, не запущено. После последних V15/API изменений требуется повторная финальная cross-typecheck |
| V14 | Web producer интегрирован | Живой Chrome smoke после V15, включая getter reentry, прошёл; это не заменяет финальный wasm gate после последующих изменений |
| V15 | `flui-platform-api` не зависит от `ui-events`/`keyboard-types`/`dpi`; общий мост удалён; xtask использует прямой `keyboard-types` | `ui-events` остаётся только там, где нужны platform keyboard tables; итоговые `deps`/`reach` впереди |

## P1 — словарь, ADR, мост (без изменения поведения)

| ID | Задача | Файлы | Требования | Доказательство |
|---|---|---|---|---|
| V1 | Типы `pointer`, `keyboard` (кроме таблиц), `EventTime`; rustdoc на каждом элементе | `crates/flui-platform-api/src/{lib.rs,event_time.rs,pointer/**,keyboard/{mod,modifiers}.rs}`, `ARCHITECTURE.md` | R1–R10, R12–R24, R26–R28 | `input_vocabulary_contract` |
| V2 | Генератор `cargo xtask key-vocabulary` (`--write`, `--self-test`), проверка в `checks`; сгенерированные `NamedKey`, `Code` | `tools/xtask/src/{key_vocabulary.rs,main.rs,tasks/checks.rs}`, `crates/flui-platform-api/src/keyboard/{named_key,code}.rs` | R25 | `key-vocabulary --self-test`; `every_generated_key_is_found_by_its_w3c_spelling` |
| V3 | Мост `ui-events` → словарь | `crates/flui-platform/src/shared/{mod.rs,input_vocabulary.rs}`, `crates/flui-platform/tests/{main.rs,input_vocabulary.rs}` | R2, R3, R14–R17, R26 | `input_vocabulary_conversion` |
| V4 | ADR-0143 (Proposed), строка проверки в ADR-0089, эта спека | `docs/adr/ADR-0143-*.md`, `docs/adr/ADR-0089-*.md`, `docs/plans/specs/flui-interaction/pointer-vocabulary/*` | — | ревью владельца |

## P2 — конвейер на словаре (одна ветка, после слияния I1–I5 волны 1)

| ID | Задача | Файлы | [P] |
|---|---|---|---|
| V5 | `PlatformInput::{Pointer, Keyboard}` несут словарь; корневые реэкспорты `flui-platform-api` → словарь; бэкенды зовут мост на своей границе (без смены того, что они производят) | `crates/flui-platform-api/src/{lib.rs,input.rs}`, `crates/flui-platform/src/platforms/*/{events,input,platform}.rs` (только точка выдачи `PlatformInput`), `crates/flui-platform/src/traits/mod.rs` | — |
| V6 | `flui-interaction` на словаре: binding, routing, recognizers, processing, `testing` builders; удалить `PointerDeviceKind`, свой `PointerPanZoomEvent`, `convert_gesture` и `pub type DeviceId = i32` (`ids.rs`, `events.rs`) в пользу `pointer::DeviceId` (`NonZeroU64`-newtype); `ScrollEventData` несёт `ScrollDelta` с единицей | `crates/flui-interaction/src/**` (согласовать с владельцами I1–I5) | вместе с V5 |
| V7 | Runtime, widgets, testing, app, facade, web-пример | `crates/flui-runtime/src/{held_input.rs,lifecycle_state.rs,ui_realm/**}`, `crates/flui-widgets/src/interaction/{listener,gesture_detector}.rs`, `crates/flui-widgets/src/scroll/scrollable.rs` (разрешение строк/страниц), `crates/flui-testing/src/{widgets.rs,widgets/harness.rs,replay.rs}`, `crates/flui-app/src/app/runner/realm_dispatch/tests.rs`, `src/interaction.rs`, `examples/web_demo/src/lib.rs` | вместе с V5 |
| V8 | `changelog.d/` (Changed: типы ввода facade), миграция всех вызовов workspace, `cargo xtask check-changed` | `changelog.d/interaction-pointer-vocabulary-pipeline.md` | вместе с V5 |

## P3 — производители по бэкендам (каждая — отдельная ветка, после P2)

| ID | Бэкенд | Что производит | Файлы | [P] | Доказательство |
|---|---|---|---|---|---|
| V9 | Win32 | `WM_POINTER*` (`EnableMouseInPointer`): touch, pen (давление, tilt → altitude/azimuth, twist, ластик), `POINTER_INFO.sourceDevice` → `DeviceId`, `WM_POINTERDEVICECHANGE`/`WM_POINTERDEVICEINRANGE`; `SetCapture`/`ReleaseCapture`, `WM_CAPTURECHANGED` → `CaptureLost`; `WM_XBUTTON*`; время `GetMessageTime`; `ButtonChange`; мышь без давления | `crates/flui-platform/src/platforms/windows/{events.rs,platform.rs}` (не `window.rs`, не `text_services/**` до слияния `text-ime/host-contract`) | [P] | Win32-unit + `cargo xtask device windows-input` на хосте, вывод в PR |
| V10 | winit | первым касанием — роль `Primary` (мост не может вывести её из id: winit выдаёт касаниям id с 2); `Force::Calibrated` → altitude; `PinchGesture`/`PanGesture`/`RotationGesture` с фазой → один `PanZoom` поток с накоплением; `MouseWheel{phase}` → `ScrollPhase`; `TouchpadPressure`; `DeviceEvent::Added/Removed`; точность по варианту дельты | `crates/flui-platform/src/platforms/winit/{events.rs,platform.rs}` | [P] | unit на хосте; `cargo xtask live-smoke` где доступен |
| V11 | macOS | `scrollWheel:` (включая двухпальцевый пан) → один `ScrollEvent`: `NSEvent` `phase`/`momentumPhase` → `ScrollPhase`, `hasPreciseScrollingDeltas` → `ScrollPrecision`; `magnifyWithEvent:`/`rotateWithEvent:` → общий накопительный `PanZoom` Start/Update/End без дублирования scroll; `pressure`/`stage` (Force Touch), `clickCount`, `buttonNumber`, время `timestamp` (ADR-0143 §5) | `crates/flui-platform/src/platforms/macos/{events.rs,view.rs}` | [P] | `native_scroll_phases_preserve_momentum_and_unphased_wheels`, `native_pinch_and_rotation_share_one_cumulative_gesture`; `cross-typecheck` подтверждает только компиляцию, запуск на macOS указывается отдельно |
| V12 | iOS | `UITouch.altitudeAngle`/`azimuthAngle`, `majorRadius` → размер контакта, `force` → давление (`maximumPossibleForce`), `UIHoverGestureRecognizer` → hover пера, `touchesCancelled` → `Platform` | `crates/flui-platform/src/platforms/ios/events.rs` | [P] | `cross-typecheck` |
| V13 | Android | `TOOL_TYPE_ERASER` → ластик, `getButtonState`, `AXIS_TILT`/`AXIS_ORIENTATION`, `TOUCH_MAJOR/MINOR` в логических px, `getHistorical*` → `coalesced`, `ACTION_SCROLL` → `ScrollEvent`, `getEventTimeNanos` | `crates/flui-platform/src/platforms/android/input.rs` | [P] | `cross-typecheck` |
| V14 | Web | `pointercancel` → `Platform`, `setPointerCapture`/`lostpointercapture` → `CaptureLost`, `getCoalescedEvents`/`getPredictedEvents`, `altitudeAngle`/`azimuthAngle`/`twist`, дробные координаты, `deltaMode` с единицей | `crates/flui-platform/src/platforms/web/events.rs` | [P] | `cargo xtask wasm-check`; браузерный unit — недоступно, так и записать |
| V15 | Уборка | Удалить мост и зависимости `ui-events`/`keyboard-types`/`dpi` из `flui-platform-api` (и из `flui-platform`, где не нужны); генератор `key-vocabulary` ищет `keyboard-types` через `ui-events` — перевести его на пакет, который останется в графе (или на dev-зависимость) | `crates/flui-platform-api/Cargo.toml`, `crates/flui-platform/src/shared/input_vocabulary.rs` | после V9–V14 | `cargo xtask deps`, `cargo xtask reach` |
