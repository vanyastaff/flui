# reduce-motion — задачи

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Design:** [design.md](design.md); [requirements.md](requirements.md)
- **Порядок:** волна W2 — после controller-robustness (B: Vsync — единственные часы, форма
  `tick_at`), motion-clock (C: `MotionClock`, `FrameTick`, `tick_all(&FrameTick)`) и
  listener-delivery (A: сдерживание panic в обходе, нужно R18). До ownership T2: его settle «нет
  часов» — это `settle_run` из T2 здесь. **Один PR на спеку** (ветка `animation/reduce-motion`);
  [P]-задачи — отдельные worktree, вливаются в ветку спеки. ID R/T — только в этом каталоге, не в
  коде, именах тестов и коммитах.
- **Готово для задачи:** каждый названный тест падает с откатом production-ханка (прогон в
  отдельном worktree, вывод в PR) и проходит с ним; `cargo xtask check-changed` зелёный; rustdoc
  каждого нового `pub` — контракт, единицы, ошибки, panics, пример; у каждого `pub` есть
  production-потребитель из списка design «Публичный API».
- **Не исполняется в CI:** win32 (T4), macOS/iOS (T5), Android (T6) — clippy через
  `cross-typecheck`; web (T7) — `wasm-check`; Linux-портал (T8) — компиляция. Для них PR содержит
  датированный локальный прогон или пометку «не запускалось».

## Граф

```text
T1 контракт ─┬─▶ T2 обход Vsync + settle_run + Normal-таймлайн ─┬─▶ T9 потребители, ADR, docs
             ├─▶ T3 runtime/app/testing: сигнал → часы + MediaQuery ┤
             └─▶ T4–T8 [P] бэкенды (после T3 — для ручного прогона) ┘
```

## Задачи

| ID | Работа | Требования | Файлы | Зависит | [P] |
|---|---|---|---|---|---|
| T1 | Контракт: `SystemMotion`, `DurationScale`, `InvalidDurationScale`, `from_duration_scale` (инертно: всегда `Ok(NoPreference)`); `PlatformWindow::{system_motion, on_system_motion_changed}` с инертными дефолтами; `MotionPolicy`, `MotionPreference`, `MotionClock::{set_preference, set_system_motion, policy}` (политика всегда `Full`); `AnimationControllerBuilder::behavior` (игнорирует); `MediaQueryData::motion` + `motion_of`; `UiRealm::{set_system_motion, set_motion_preference}`, `AppConfig::with_motion_preference`, `HeadlessHost::set_system_motion`, `HeadlessWindow::simulate_system_motion` — no-op; все тесты R1–R22 красные по assertion | все | `flui-platform-api/src/{motion.rs,platform_window.rs,lib.rs}`; `flui-animation/src/{motion.rs,builder.rs,status.rs,lib.rs}`, `Cargo.toml` (+`flui-platform-api`), `tests/main.rs` + `tests/contracts/reduce_motion.rs`; `flui-widgets/src/app/media_query.rs`; `flui-runtime/src/ui_realm/{presentation_lifecycle.rs,tests/frame_pipeline_and_vsync.rs}`; `flui-app/src/app/config.rs`, `runner/realm_dispatch/tests.rs`; `flui-testing/src/host.rs`, `tests/headless_host.rs`; `flui-platform-api/tests/main.rs`; `flui-platform/src/{system_motion.rs,platforms/headless/platform.rs}`, `tests/{headless,window_callback_unwind}.rs`; 9 файлов литералов `MediaQueryData` | B, C | — |
| T2 | Обход: `walk_probe` отдаёт `behavior` и вид запуска; `Reduce`×`Normal` → `settle_run` (`Settled`/`Parked`), запись `parked`, `has_running` без запаркованных, возобновление с t = 0; сетка симуляции; `FrameTick::time(behavior)`, второй таймлайн Normal с ребейзом в `MotionClock`; разрешение политики; `behavior` по умолчанию `Normal` (явный `Preserve` у скролла, таймеров, индикаторов — X2, X9); rustdoc `AnimationBehavior`, удалить `should_preserve`/`is_normal` | R1–R9, R17–R21 | `flui-animation/src/{vsync.rs,motion.rs,status.rs,builder.rs}`, `controller/{run,tick,mod}.rs` (раскладка Q0) | T1, A | — |
| T3 | Сигнал → realm: `on_system_motion_changed` в `shared/handlers.rs` (слот, `CallbackLease`, `panic_boundary`, макрос) и headless; `PlatformToUi::SystemMotionChanged` + обработка (часы + `MediaQuerySource::update` + кадр); одна `wire_system_settings` для desktop главного/вторичного, iOS, Android, web; `set_motion_preference` из `AppConfig` во всех runner'ах; удалить `SharedEngineServices::accessibility_features`, поля `AccessibilityFeatures::{reduce_motion, disable_animations}` | R10, R11, R13, R16, R22 | `flui-platform/src/shared/handlers.rs`, `platforms/headless/platform.rs`; `flui-runtime/src/{presentation.rs,ui_realm/presentation_lifecycle.rs,media_query_root.rs}`; `flui-app/src/app/{runtime.rs,config.rs,runner/{realm_dispatch.rs,desktop.rs,secondary_window.rs,ios.rs,android.rs,web.rs}}`; `flui-semantics/src/accessibility.rs`; `flui-testing/src/host.rs` | T1 | ‖ T2 |
| T4 | Win32: `system_motion()` через `SPI_GETCLIENTAREAANIMATION`, на `WM_SETTINGCHANGE` — `dispatch_system_motion_changed`; чистая функция перевода + строка таблицы; `#[cfg(windows)]` `windows_reads_client_area_animation`; ручной тумблер «Animation effects» — лог в PR | R12, R22 | `flui-platform/src/platforms/windows/{platform.rs,window.rs}`, `Cargo.toml` (фича `Win32_UI_WindowsAndMessaging` уже есть — сверить), `tests/main.rs` | T1 | ‖ |
| T5 | macOS: `NSWorkspace` чтение + наблюдатель на `notificationCenter` workspace, снятие при закрытии окна; iOS: `UIAccessibility` + уведомление. Сверить имена [U] по SDK-заголовкам | R12, R22 | `flui-platform/src/platforms/{macos,ios}/window.rs`, `Cargo.toml` (фичи objc2) | T1 | ‖ |
| T6 | Android: `jni` (workspace-зависимость, `cargo xtask deps`), чтение `ANIMATOR_DURATION_SCALE` через `ContentResolver` на `InitWindow`/`Resume`/`ConfigChanged`; JNI-исключение → прежнее значение; `from_duration_scale` | R8, R12, R22 | `flui-platform/src/platforms/android/{mod.rs,window.rs}`, `Cargo.toml`, корневой `Cargo.toml` | T1 | ‖ |
| T7 | web: `matchMedia("(prefers-reduced-motion: reduce)")`, `change`-слушатель, снятие при закрытии | R12 | `flui-platform/src/platforms/web/{window.rs,platform.rs}` | T1 | ‖ |
| T8 | Linux/winit за `a11y`: `zbus::blocking` на своём потоке — `ReadOne` `org.freedesktop.appearance reduced-motion`, запасной `org.gnome.desktop.interface enable-animations`, `SettingChanged` → колбэк окна; без D-Bus — `NoPreference` + одно `warn` | R12, R22 | `flui-platform/src/platforms/winit/{window.rs,…}`, новый `platforms/linux/portal_settings.rs`, `Cargo.toml` | T1 | ‖ |
| T9 | Потребители: `cupertino_page_transitions` под `Reduce` без slide (R14); snackbar-таймер и InkWell → `Preserve` (R15); аудит 13 файлов-конструкторов — строка в PR на каждый; ADR; `changelog.d/<branch>.md`; `flui-sdk/tests/surface.rs`; раздел в `crates/flui-animation/docs/GUIDE.md` (ARCHITECTURE — задача Z) | R14, R15 | `packages/flui-cupertino/src/route.rs`, `tests/route.rs`; `packages/flui-material/src/{scaffold_messenger,ink_well}.rs`, `tests/{snack_bar,ink_well}.rs`; `docs/adr/ADR-NNNN-reduced-motion.md`; `crates/flui-sdk/tests/surface.rs` | T2, T3 | — |

### T1 — детали контракта
- Тела инертны без `todo!`/`unimplemented!` (clippy). Красный прогон:
  `cargo nextest run -p flui-animation -E 'test(/reduce|settle|scale|policy|preserve/)'`, плюс
  `-p flui-platform`, `-p flui-testing`, `-p flui-runtime`, `-p flui-app` по именам из requirements;
  вывод — в описание PR. R16 доказывается командой `rg`, не тестом.
- Если motion-clock уже выпустил `MotionPolicy`/`MotionPreference`, T1 только добавляет методы и
  не дублирует типы (design «Граница»).

### T2 — детали
- `settle_run` — тот же путь, что `settle_at_target` (`controller.rs:1292`): мутация и доставка
  под guard, слушатели после; для симуляции сэмплы сетки — вне guard, фиксация — с проверкой
  `matches_sample`, как в `tick_at`.
- PB-тесты R5, R7 — proptest с фиксированным seed в `.proptest-regressions`.

### T4–T8 — общее
- Перевод — `pub(crate) fn` в модуле backend'а без `cfg` целевой ОС (компилируется и тестируется
  на любом хосте); его строки — in-src таблица `system_motion_translation` в
  `crates/flui-platform/src/system_motion.rs` (приватный шов, AGENTS «Writing tests»). Через
  публичный API — только `from_duration_scale` (`flui-platform-api/tests/main.rs`) и headless (`tests/headless.rs`).
  FFI-чтение — тонкая обёртка без логики.
