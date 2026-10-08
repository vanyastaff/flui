# reduce-motion — системный сигнал движения, политика приложения, opt-out на анимацию

- **Статус:** черновик
- **Дата:** 2026-10-06
- **База:** `main` @ `9a4daa3ed`
- **Закрывает:** D-05 ([../tasks.md](../tasks.md)); строки [../market.md](../market.md) M-A11Y-1…5.
  M-A11Y-6 (отдельный флаг cross-fade) вынесена владельцем 2026-10-06.
- **Контракты:** [ADR-0078](../../../../adr/ADR-0078-rules-live-in-types-and-lints.md) (почему не
  метод `LifecycleContext`, design «Варианты (d)»),
  [ADR-0082](../../../../adr/ADR-0082-platform-api-contract-crate.md) (типы ОС не покидают
  `flui-platform`), [ADR-0097](../../../../adr/ADR-0097-no-process-global-state-gate.md); новый ADR —
  design «Черновик ADR».
- **Связанные спеки:** motion-clock (`MotionClock`, `FrameTick`, `tick_all(&FrameTick)` — шов, на
  котором применяется политика; reduce-требования из motion-clock уже перенесены сюда, там остался
  только шов — design «Граница»), listener-delivery (сдерживание panic в обходе), controller-robustness
  (NaN-политика значения, builder), ownership (переиспользует settle-путь R2–R4 для «нет часов»),
  physics (`is_done ⇔ t ≥ rest_time`, `rest_time` — `pub(crate)`).

## Сценарии

1. Пользователь Windows выключает «Animation effects»: открытые окна без перезапуска завершают
   переходы маршрутов в следующем кадре (`on_completed`, futures доставлены), скролл и fling — как
   прежде, snackbar показывается свою длительность; `MediaQuery::motion_of` даёт `Reduce`.
2. Android «Animator duration scale 0.5x»: переходы идут вдвое быстрее, fling — без изменений;
   «Remove animations» (0) — как сценарий 1.
3. Приложение задаёт `AppConfig::with_motion_preference(MotionPreference::Full)` для
   демонстрационного киоска: сигнал ОС игнорируется.
4. Тест: headless-окно переключает сигнал → следующий кадр завершает Normal-запуски, Preserve идёт.

## Требования

Тесты — через публичный API: `crates/flui-animation/tests/main.rs` (модуль `reduce_motion`,
раннер `run_table` после Q0), `HeadlessHost` в `crates/flui-testing/tests/headless_host.rs`,
пакеты — `tests/main.rs` пакета; realm и dispatch без внешнего API — in-src
`crates/flui-runtime/src/ui_realm/tests/frame_pipeline_and_vsync.rs` и таблица
`realm_dispatch_matrix` (`crates/flui-app/src/app/runner/realm_dispatch/tests.rs`). Время —
виртуальное: `MotionClock::frame(Duration)`, `ManualClock` у `HeadlessHost::pump(dt)`. Эталоны —
таблица разрешения (Motion `useReducedMotionConfig`, market-B A2), аналитика (`final_x` трения
`x₀ − v₀/ln(drag)`, `x(t) = t` тестовой симуляции), сумма `Δraw/s` в целых наносекундах —
никогда production-формула. PB — property-based (proptest, Q0).

### Политика

- **R1.** СИСТЕМА ДОЛЖНА разрешать политику: `FollowSystem` × {`NoPreference` → `Full`, масштаб 1;
  `Reduce` → `Reduce`; `Scaled(s)` → `Full`, масштаб s}; `Reduce` × любой → `Reduce`; `Full` ×
  любой → `Full`, масштаб 1. По умолчанию — `FollowSystem`. Тест:
  `motion_policy_resolves_preference_against_the_system` (таблица 3 × 3, эталон — таблица выше).
- **R2.** КОГДА политика `Reduce` и контроллер `AnimationBehavior::Normal` ведёт time-запуск,
  СИСТЕМА ДОЛЖНА на ближайшем тике его `Vsync` поставить значение = цель, выдать ровно один
  терминальный статус (последовательность `[Forward, Completed]` для `forward`), завершить future
  `Ok`, вызвать слушатель значения один раз. Тест: `reduced_motion_settles_normal_runs_on_the_next_tick`
  (строки: `forward`, `reverse`, `animate_to` с кривой, `animate_back`, `forward_from`, нулевая
  длительность).
- **R3.** КОГДА политика `Reduce`: конечный repeat ДОЛЖЕН завершиться в конечном состоянии
  последнего плеча с терминальным статусом и `Ok`; бесконечный repeat ДОЛЖЕН встать в начало
  первого плеча, не тикаться, не учитываться `Vsync::has_running()` (кадры не заказываются),
  future остаётся незавершённым; КОГДА политика снова `Full`, он ДОЛЖЕН стартовать с t = 0 от
  якоря этого тика. Тест: строки `finite_repeat_settles_at_its_last_leg`,
  `infinite_repeat_parks_at_its_first_leg_start`, `parked_repeat_resumes_from_zero_under_full`.
- **R4.** КОГДА Normal-контроллер с симуляцией завершается (Reduce или «нет часов» ownership),
  СИСТЕМА ДОЛЖНА взять первое `t ∈ {0.25·2ᵏ s, k = 0..8}` с `is_done(t)` и конечным `x(t)`;
  иначе — последний конечный `x(t)` сетки (64 s); если конечного нет — значение не меняется;
  во всех случаях статус терминален, future `Ok`. Тест: `simulation_settle_grid` (строки:
  трение → `final_x` замкнутой формы, пружина → `end` точно, тестовая `x(t) = t` без конца → 64,
  тестовая NaN-симуляция → значение прежнее).
- **R5.** `AnimationBehavior::Preserve` ДОЛЖЕН давать при любой политике и масштабе те же значения,
  что при `Full`. Тест (PB): `preserve_runs_identically_under_any_policy` — случайные
  последовательности смен политики/масштаба и разбиения кадров; эталон — тот же запуск на втором
  `Vsync` с `Full`.
- **R6.** Builder ДОЛЖЕН давать `Normal` по умолчанию для любой формы (`unbounded()` поведение не
  меняет); `behavior` задаётся явно. Скролл (3 сайта), таймеры (snackbar, InkWell) и индикаторы
  загрузки (`ActivityIndicator`, `LinearProgressIndicator`, `CupertinoActivityIndicator`) —
  `.behavior(Preserve)`; implicit-анимации и `AnimatedValue` — `Normal` (решения X2, X9,
  orchestration «По итогам adversarial review»). Тест: `default_behavior_is_normal` (таблица
  builder'ов) и строка `implicit_opacity_settles_under_reduce`.

### Системный масштаб длительностей

- **R7.** КОГДА сигнал `Scaled(s)` и политика `Full`, СИСТЕМА ДОЛЖНА вести Normal-запуски по времени
  `Σ Δraw_i / s_i` (300 ms при s = 0.5 завершается на 150 ms сырого времени, при s = 10 — на 3 s),
  Preserve — по `Σ Δraw_i`; смена s посреди запуска — без скачка значения. Тесты:
  `duration_scale_stretches_normal_runs_only` (таблица); PB
  `normal_timeline_integrates_inverse_scale_over_any_partition` (эталон — сумма в целых ns).
- **R8.** `SystemMotion::from_duration_scale` ДОЛЖЕН отображать 0 → `Reduce`, 1 → `NoPreference`,
  конечное > 0 → `Scaled`, а < 0, NaN, ±inf отвергать (`InvalidDurationScale::{Negative,
  NonFinite}`); backend при отказе ДОЛЖЕН сохранить последнее значение. КОГДА `Δraw / s`
  переполняет `Duration` (s = 1e-300), время насыщается и запуск завершается одним переходом.
  Тесты: `duration_scale_translation` (`flui-platform-api` `tests/main.rs`; строки −0.0 →
  `Reduce`, subnormal > 0 → `Scaled`), строка `tiny_scale_saturates_and_completes_once` в R7.
- **R9.** КОГДА политика меняется посреди запуска, СИСТЕМА ДОЛЖНА: Full→Reduce — завершить
  Normal-запуски на следующем тике и заказать кадр; Reduce→Full — возобновить запаркованные;
  Preserve не прыгает; Full→Reduce→Full до тика — ничего не завершать. Тест: `policy_flips_mid_run`.

### Сигнал ОС и доступ

- **R10.** КОГДА `UiRealm::set_system_motion` получает новое значение для презентации, СИСТЕМА
  ДОЛЖНА обновить её `MotionClock`, `MediaQueryData::motion` (виджет, читающий
  `MediaQuery::motion_of`, перестраивается один раз) и заказать кадр; повтор того же значения —
  без перестройки. Тест: `system_motion_change_reaches_media_query_and_the_clock` (`HeadlessHost`).
- **R11.** Каждый runner (desktop главное и вторичные окна, iOS, Android, web) ДОЛЖЕН засеять сигнал
  при создании презентации и пересылать изменения `PlatformToUi::SystemMotionChanged`. Тест:
  строки `system_motion_event_updates_the_addressed_presentation`,
  `secondary_window_receives_system_motion` в `realm_dispatch_matrix`; разводка мобильных и web —
  clippy/wasm-check, ручной прогон в PR.
- **R12.** Backend'ы ДОЛЖНЫ переводить сигнал ОС чистой функцией, тестируемой на любом хосте:
  Win32 `SPI_GETCLIENTAREAANIMATION` FALSE → `Reduce`; macOS/iOS bool → `Reduce`; Android
  `ANIMATOR_DURATION_SCALE` → R8; web `prefers-reduced-motion: reduce` → `Reduce`; Linux портал
  `org.freedesktop.appearance reduced-motion` 1 → `Reduce`, иное → `NoPreference`, запасной ключ
  `org.gnome.desktop.interface enable-animations` false → `Reduce`; ошибка чтения → прежнее
  значение. Тесты: `system_motion_translation` (in-src таблица `flui-platform/src/system_motion.rs`:
  функции перевода `pub(crate)`, без `cfg` ОС), `duration_scale_translation` (`crates/flui-platform-api/tests/main.rs`);
  headless — `headless_system_motion_notifies_and_reports` (`tests/headless.rs`); живые чтения —
  `#[cfg(windows)]` `windows_reads_client_area_animation` (только локально), остальные — PR-лог.
- **R13.** `AppConfig::with_motion_preference` ДОЛЖЕН действовать с первого кадра каждой
  презентации realm, включая открытые позже. Тест: `presentation_starts_with_the_realm_motion_preference`
  (runtime, in-src).
- **R14.** КОГДА политика `Reduce`, `cupertino_page_route` НЕ ДОЛЖЕН сдвигать страницу: кадр push
  рисует её в x = 0 (без фикса — x = ширина окна). Тест:
  `cupertino_route_does_not_slide_under_reduced_motion` (`packages/flui-cupertino/tests/route.rs`).
- **R15.** Таймеры на контроллерах (`ScaffoldMessenger` — показ snackbar, `InkWell` — задержка
  снятия pressed) ДОЛЖНЫ быть `Preserve`: под `Reduce` snackbar виден свою длительность. Тесты:
  `snack_bar_keeps_its_display_duration_under_reduced_motion` (`tests/snack_bar.rs`),
  `press_highlight_lasts_its_delay_under_reduced_motion` (`tests/ink_well.rs`).
- **R16.** СИСТЕМА НЕ ДОЛЖНА держать второй, мёртвый источник: `SharedEngineServices::accessibility_features`
  и поля `AccessibilityFeatures::{reduce_motion, disable_animations}` удалены. Доказательство:
  компиляция workspace, `rg` пуст (команда в PR).

### Отказы и реентерабельность

- **R17.** КОГДА статус-слушатель во время settle-тика делает `dispose`, `forward`, `animate_to`
  (retarget в последнем кадре), снимает регистрацию или отпускает последнего владельца, СИСТЕМА
  ДОЛЖНА: dispose/снятие — не трогать контроллер дальше; новый запуск — завершить на **следующем**
  тике (курсор обхода только вперёд), так что слушатель «всегда перезапускай» даёт один settle на
  кадр. Тест: `settle_listener_reentry` (таблица по строкам выше).
- **R18.** КОГДА слушатель паникует в settle-тике, СИСТЕМА ДОЛЖНА зафиксировать значение, статус и
  future до вызова, оставить первый panic первым, завершить остальные контроллеры кадра и тикать
  следующий кадр. Тест: строки `panic_in_settle_listener_keeps_the_frame` (контроллер позже в
  реестре всё равно завершён; следующий тик проходит) — опирается на сдерживание listener-delivery.
- **R19.** КОГДА два контроллера одного `Vsync` и контроллер вложенного реестра под `Reduce`,
  СИСТЕМА ДОЛЖНА завершить все в одном кадре: вложенный реестр первым, затем в порядке
  регистрации. Тест: `settle_order_follows_the_registry_walk`.
- **R20.** КОГДА реестр заглушён (`TickerMode`), СИСТЕМА ДОЛЖНА отложить settle до первого тика
  после снятия заглушки; КОГДА realm в `Stopping`, — не завершать ничего; КОГДА презентация скрыта,
  — применить политику на первом видимом кадре. Тесты: `muted_registry_settles_on_unmute`,
  `a_stopping_realm_settles_nothing` (runtime).
- **R21.** Settle НЕ ДОЛЖЕН зависеть от времени тика: dt = 0, повтор тика, огромный dt, время назад
  (держит motion-clock) дают один и тот же результат ровно один раз. Тест: `settle_is_time_independent`.
- **R22.** Колбэк платформы и ошибки чтения: panic в замыкании runner'а сдерживается существующим
  `panic_boundary` `flui-platform`, следующий сигнал доставляется; недоступный источник (SPI
  ошибка, JNI-исключение, нет D-Bus) — `NoPreference` или прежнее значение, одно `warn`, без panic.
  Тесты: строка `unavailable_source_keeps_the_last_value` в `system_motion_translation`; panic —
  существующий `window_callback_unwind.rs` дополняется строкой `system_motion_callback_panic_is_contained`.
