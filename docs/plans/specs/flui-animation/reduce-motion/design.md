# reduce-motion — design

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Требования:** [requirements.md](requirements.md); задачи — [tasks.md](tasks.md).

## Итог

1. **Сигнал ОС** — значение `SystemMotion { NoPreference, Reduce, Scaled(DurationScale) }` в
   `flui-platform-api`, читается методом `PlatformWindow::system_motion()`, изменение —
   колбэк `on_system_motion_changed` (шаблон `appearance`/`on_appearance_changed`). Бэкенды —
   в `flui-platform`, перевод — чистые функции.
2. **Политика** `MotionPolicy { Full, Reduce }` = `MotionPreference` приложения поверх сигнала;
   живёт в `MotionClock` презентации (motion-clock), приходит в реестр с `FrameTick`.
3. **Применение — в обходе `Vsync`**: Normal-запуск под `Reduce` завершается settle-путём
   контроллера (статусы, futures), бесконечный repeat паркуется; `Preserve` не трогается;
   системный масштаб растягивает только Normal-время.
4. **Виджеты читают политику как inherited-значение** `MediaQueryData::motion`
   (`MediaQuery::motion_of(ctx)` — в `build` и в `did_change_dependencies`). Метода
   `LifecycleContext` нет (Варианты (d)). Потребитель — `cupertino_page_route`.
5. `behavior` задаётся явно у каждого контроллера (решение X2, «unbounded ⇒ Preserve» не выводится): таймеры (snackbar, InkWell), скролл и индикаторы загрузки (X9) — `Preserve`, implicit-анимации — `Normal`. Мёртвые `AccessibilityFeatures`-поля
   движения удаляются. ADR нужен.

## Граница с motion-clock

motion-clock владеет временем: `MotionClock` (скорость, шаг, монотонность, насыщение), `FrameTick`,
`tick_all(&FrameTick)`, скрытые презентации, devtools, скорость на анимацию. Политика движения
(`MotionPolicy`/`MotionPreference`), `SystemMotion`, системный масштаб (вместо прежнего
`set_system_rate`; действует только на Normal — см. (e)) и правило settle — здесь; в motion-clock
их уже нет (его requirements «Не здесь», design «Шов для reduce-motion»). motion-clock выпускает
`FrameTick` с приватными полями без политики; эта спека добавляет поле политики и второй таймлайн
Normal (не ломающее изменение).

## Текущее состояние (чтением, база `9a4daa3ed`)

- `AnimationBehavior` (`crates/flui-animation/src/status.rs:114`) без потребителей; док `:99`
  обещает несуществующий слой, `Preserve` описан неверно («when not in view», `:119`);
  `should_preserve`/`is_normal` (`:130`, `:137`) никто не зовёт.
- Обход реестра — `Vsync::tick_all(now_secs: f64)` (`vsync.rs:440`): якорь на поколение запуска
  (`:484-489`), `walk_probe` (`controller.rs:1843`), `tick_at` (`:1870`). Синхронный settle в цель
  уже есть для нулевой длительности — `settle_at_target` (`controller.rs:1292`).
- `AccessibilityFeatures` (`crates/flui-semantics/src/accessibility.rs:15`, поля
  `disable_animations` и `reduce_motion` — два флага одного смысла) лежит в
  `crates/flui-app/src/app/runtime.rs:97` под `expect(dead_code)`, создаётся `:127`; никто не пишет.
  `did_change_accessibility_features` (`crates/flui-view/src/binding.rs:313`, `:1458`) без источника.
- Модель реактивной настройки: `on_appearance_changed` (`flui-platform-api/src/platform_window.rs:573`),
  слот `shared/handlers.rs:272`, Win32 `WM_SETTINGCHANGE` → `dispatch_appearance_changed`
  (`windows/platform.rs:1814`); runner `desktop.rs:676-690` (seed + колбэк) →
  `PlatformToUi::AppearanceChanged` (`realm_dispatch.rs:201`) → `media_query_for(..).update`
  (`:416-433`). **Только главное desktop-окно**: `secondary_window.rs`, `ios.rs`, `android.rs`,
  `web.rs` его не шлют (rg `PlatformToUi::` по runner'ам).
- `MediaQueryData` (`crates/flui-widgets/src/app/media_query.rs:47`) без поля движения; аксессоры
  вида `platform_brightness_of` (`:201`). Realm-настройки из `AppConfig` идут сеттером
  `UiRealm::set_performance_overlay` во всех четырёх runner'ах (`desktop.rs:138`, `ios.rs:301`,
  `android.rs:212`, `web.rs:129`).
- Бэкенды (`crates/flui-platform/src/platforms/mod.rs`): windows, macos, ios, android (NativeActivity,
  без `jni`), web, winit (production на Linux), linux (stub), headless. Ни один не читает настройку
  движения. CI исполняет только headless (Linux); win32/macOS/iOS/Android — `cross-typecheck`
  (clippy), web — `wasm-check`; winit на Linux компилируется, D-Bus в CI нет.
- Контроллеры-таймеры: `ScaffoldMessenger::start_display_timer`
  (`packages/flui-material/src/scaffold_messenger.rs:553`), `InkWell` `PRESS_DEACTIVATION_DELAY`
  (`packages/flui-material/src/ink_well.rs:524`). Под Normal-settle snackbar исчез бы за кадр.
- Скролл — `unbounded_without_ticker` (`scrollable.rs:347`, `scroll_controller.rs:588`,
  `refresh_indicator.rs:400`): получают явный `.behavior(Preserve)` (X2: поведение не выводится из границ).

## Варианты

### (a) Где применяется политика

| | Как | Оценка |
|---|---|---|
| A1 | Контроллер читает политику сам (поле/`Arc` общего флага) | второй канал состояния в контроллер; lock или атомик на пути кадра; контроллер без реестра не знает презентацию |
| A2 | Виджеты (Flutter: `MediaQuery.disableAnimations` читает каждый) | рынок показывает: доходит не до всех (market-B A5, issue #4827); пропуск = анимация |
| **A3** | Обход `Vsync` под `FrameTick::motion()` зовёт crate-private `settle_run` | одна точка, один кадр; вложенные реестры получают тот же тик; поведение — неизменяемая конфигурация, читается в `walk_probe` |

### (b) Что значит Reduce

Решение (принято здесь; в прежней редакции motion-clock было его «(d)», ныне там «(d)» — скрытая
презентация): конец в следующем кадре (Compose scale 0, market.md [axui-mds];
GPUI oneshot → конец, repeat → начало, market.md [gpui-animation]); ×0.05 Flutter отвергнут.
Уточнение здесь — сетка симуляции: одна для Reduce и для ownership «нет часов»
(`0.25·2ᵏ ≤ 64 s`, как в ownership design); правило «не завершилась — идти как Full» прежней
редакции motion-clock отвергнуто (в motion-clock его больше нет): под Reduce движение не должно продолжаться; существенное движение помечается
`Preserve`. Точный конец пружин даёт physics (`is_done ⇔ t ≥ rest_time`, `x = end`).

### (c) Форма платформенного контракта

| | Форма | За | Против |
|---|---|---|---|
| P1 | **Методы `PlatformWindow`**: `system_motion()`, `on_system_motion_changed(cb)` | слот колбэка, `CallbackLease`, `panic_boundary` и замена уже есть (`handlers.rs`); flui-app держит окно, а не `Platform` (`haptics.rs` модуль-док, п. 3) | два метода в широком трейте |
| P2 | Трейт-capability `PlatformMotionSettings` через `PlatformWindow::motion() -> Option<Arc<dyn _>>` (шаблон haptics) | отдельный трейт | источник общий на процесс (NSWorkspace, Settings.Global): одно `Arc` на все окна с однослотовым колбэком — второе окно затирает подписку первого; нужен токен подписки — новая машинерия |
| P3 | `Platform::system_motion` | процесс-глобальная настройка | `Platform` поглощается `run()`, runner'у недоступен |

**Выбран P1**: трейт — `PlatformWindow` в `flui-platform-api`; форма совпадает с appearance,
а подписка на окно исключает затирание. Отдельный колбэк, а не переименование
`on_appearance_changed` в «settings changed»: меньше churn, Win32 шлёт оба на `WM_SETTINGCHANGE`.

### (d) Доступ виджетов

| | Форма | Оценка |
|---|---|---|
| L1 | `LifecycleContext::motion_policy()` — снимок/handle | ADR-0078 — для handles, которые **действуют**; значение, взятое в `init_state`, устаревает без второй подписки параллельно inherited-зависимостям; писать политику из виджета некому (нет production-потребителя) |
| **L2** | `MediaQueryData::motion`, `MediaQuery::motion_of(ctx)` | реактивно (inherited-зависимость), читается и в `did_change_dependencies` (`LifecycleContext: BuildContext`), как `platform_brightness`; build читать значение может — это не capability |

Платформенная capability до виджетов не доходит вовсе: её потребляет realm (flui-app → `UiRealm`).

### (e) Системный масштаб длительностей

Compose применяет `MotionDurationScale` к анимациям, но скролл явно исключает
(`DefaultScrollMotionDurationScale` = 1, `compose/foundation/.../gestures/AbstractScrollableNode.kt`,
проверено `gh search` 2026-10-06). Варианты: масштаб в общей скорости часов (motion-clock
`set_system_rate`) — растянул бы и fling; **второй таймлайн Normal** в `MotionClock` с тем же
ребейзом эпохи — выбран. `FrameTick` несёт `now` (Preserve) и `normal_now`.

## Публичный API (дельта)

```rust
// flui-platform-api, новый модуль motion.rs
/// The OS motion setting, as the platform reports it for a window.
#[derive(Clone, Copy, Debug, Default, PartialEq)] #[non_exhaustive]
pub enum SystemMotion { #[default] NoPreference, Reduce, Scaled(DurationScale) }
impl SystemMotion {
    /// 0 (and −0) → `Reduce`, 1 → `NoPreference`, finite > 0 → `Scaled`.
    /// # Errors  negative or non-finite scale.
    pub fn from_duration_scale(scale: f64) -> Result<Self, InvalidDurationScale>;
}
/// Finite, > 0 and ≠ 1 (1 is `NoPreference`): one spelling per setting.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct DurationScale(f64);
impl DurationScale { #[must_use] pub const fn get(self) -> f64; }
#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)] #[non_exhaustive]
pub enum InvalidDurationScale { #[error("..")] Negative(f64), #[error("..")] NonFinite(f64) }
// PlatformWindow (дефолты инертны):
fn system_motion(&self) -> SystemMotion { SystemMotion::NoPreference }
fn on_system_motion_changed(&self, callback: Box<dyn FnMut() + Send>) { let _ = callback; }

// flui-platform, headless: HeadlessWindow::simulate_system_motion(&self, SystemMotion)

// flui-animation (motion.rs motion-clock'а); зависимость flui-animation → flui-platform-api
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)] #[non_exhaustive]
pub enum MotionPolicy { #[default] Full, Reduce }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)] #[non_exhaustive]
pub enum MotionPreference { #[default] FollowSystem, Reduce, Full }
impl MotionClock {
    pub fn set_preference(&mut self, preference: MotionPreference);   // ребейз Normal
    pub fn set_system_motion(&mut self, system: SystemMotion);       // ребейз Normal
    pub fn policy(&self) -> MotionPolicy;
}
impl AnimationControllerBuilder { pub fn behavior(self, behavior: AnimationBehavior) -> Self; }
// AnimationBehavior: rustdoc по R2–R6; should_preserve/is_normal удаляются (match по enum).
// crate-private: FrameTick::{motion, time(AnimationBehavior)}, AnimationController::settle_run

// flui-widgets
pub struct MediaQueryData { …, pub motion: MotionPolicy }
impl MediaQuery { pub fn motion_of(ctx: &dyn BuildContext) -> Option<MotionPolicy>; }
// flui-runtime
impl UiRealm {
    pub fn set_system_motion(&self, id: PresentationId, motion: SystemMotion);
    pub fn set_motion_preference(&self, preference: MotionPreference); // все презентации, и будущие
}
// flui-app
pub struct AppConfig { …, pub motion_preference: MotionPreference }
impl AppConfig { pub fn with_motion_preference(self, preference: MotionPreference) -> Self; }
// flui-testing
impl HeadlessHost { pub fn set_system_motion(&self, motion: SystemMotion); }
```

Потребители каждого `pub`: `SystemMotion`/`DurationScale` — бэкенды и `MotionClock`;
`from_duration_scale` — Android; `PlatformWindow`-методы — runner'ы flui-app; `MotionPolicy` —
`MediaQueryData`, cupertino; `MotionPreference`/`set_motion_preference` — `AppConfig`;
`set_system_motion` — `realm_dispatch`; `behavior` — snackbar, InkWell; `motion_of` — cupertino.

## Алгоритм обхода (`Vsync::tick_all(&FrameTick)`)

Для записи реестра после `walk_probe` (добавляет `behavior` и вид запуска):
1. `Preserve` или политика `Full`: если `parked` — якорь = `tick.time(behavior)`, `parked = false`;
   далее как в motion-clock: прошедшее = `tick.time(behavior) − якорь`.
2. `Normal` и `Reduce`: под lock реестра — только снять клон; lock отпущен; `settle_run()` →
   `Settled` (запуск завершён через `finish`, слушатели — после отпускания guard, как
   `settle_at_target`) | `Parked` (бесконечный repeat: значение = начало первого плеча, одно
   уведомление значения) → запись `parked = true`, `has_running()` её не считает.
3. Новое поколение запуска, начатое слушателем, получает якорь/settle только на следующем тике
   (курсор вперёд, `fence` — как сейчас).

## Инварианты

- **I1** Политика и масштаб — только в `MotionClock` презентации (`&mut self`, без lock, без
  статиков); реестр видит их только через `FrameTick` текущего тика.
- **I2** `behavior` фиксируется при создании контроллера; его смена не нужна ни одному потребителю.
- **I3** Settle фиксирует значение, статус и доставку future до вызова пользовательского кода
  (R18); повторный тик того же поколения ничего не делает (R21).
- **I4** Сигнал ОС проходит одну дорогу: колбэк окна → `PlatformToUi::SystemMotionChanged` →
  `UiRealm::set_system_motion` → часы + `MediaQuerySource::update`; одинаковое значение не
  перестраивает (сравнение в `update`).
- **I5** Все runner'ы зовут одну функцию разводки `wire_system_settings(window, dispatch)` (seed +
  колбэк); вторичное окно не пропускается (сейчас пропускает appearance).

## Платформы и CI

| Backend | Запрос | Изменение | Проверено | В CI |
|---|---|---|---|---|
| Win32 | `SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)` (Chromium `ui/gfx/animation/animation_win.cc`) | `WM_SETTINGCHANGE` (есть, `:1814`), перечитать и сравнить | [V] gh search | clippy; локальный тест + ручной тумблер в PR |
| macOS | `NSWorkspace.accessibilityDisplayShouldReduceMotion` | `NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification` на `NSWorkspace.notificationCenter` | [U] | clippy |
| iOS | `UIAccessibility.isReduceMotionEnabled` (market-A) | `UIAccessibilityReduceMotionStatusDidChangeNotification` | [U] уведомление | clippy |
| Android | `Settings.Global.ANIMATOR_DURATION_SCALE` через JNI (`jni`, `AndroidApp::vm_as_ptr`); `TRANSITION_ANIMATION_SCALE` не читается (Compose тоже) | перечитать на `Resume`/`ConfigChanged` (ContentObserver требует Java-класс) | [V] Compose, [U] JNI-путь | clippy |
| web | `matchMedia("(prefers-reduced-motion: reduce)")` | `change` у `MediaQueryList` (фичи есть) | [V] mq5 | wasm-check, не исполняется |
| Linux/winit | портал `org.freedesktop.appearance reduced-motion` (u: 0/1; Settings v2) → запасной `org.gnome.desktop.interface enable-animations` | сигнал `SettingChanged`; `zbus::blocking` на своём потоке, без async в кадре | [V] `xdg-desktop-portal` `data/org.freedesktop.portal.Settings.xml` | компиляция с `a11y`; D-Bus не исполняется |
| headless | `simulate_system_motion` | колбэк | — | исполняется |

Linux-портал — за фичей `a11y` (она уже тянет `zbus` через `accesskit_unix`), без фичи —
`NoPreference`; `MotionPreference::Reduce` в приложении работает везде.

## Миграция

- `AnimationController::unbounded*` в production — 3 вызова (скролл); после controller-robustness
  это `builder(..).unbounded()` с явным `.behavior(Preserve)`; `AnimatedValue` (retarget, unbounded) — явно `Normal` (X2).
  Остальные 13 файлов-конструкторов (rg `AnimationController::(new|without_ticker|…)(`
  без tests) — `Normal`; таймеры `scaffold_messenger.rs:553`, `ink_well.rs:524` → builder с
  `.behavior(Preserve)` (builder — controller-robustness; терминальный метод с часами — ownership).
- `MediaQueryData {` литералы — 9 файлов (rg): `media_query_root.rs`, `media_query.rs`,
  `tests/media_query_fields.rs`, `flui-cli` шаблон `counter.rs`, `flui-macros/src/lib.rs` (док),
  material `tests/{scaffold,material_app}.rs`, cupertino `tests/{theme,page_scaffold}.rs`.
- `on_appearance_changed` — 15 упоминаний (rg); не меняется, рядом добавляется
  `on_system_motion_changed` в `handlers.rs` (слот, `CallbackLease`, макрос `:1058`).
- `AccessibilityFeatures` — 2 поля удаляются; `SharedEngineServices::accessibility_features` (3
  строки `runtime.rs:51,97,127`) удаляется; `flui-runtime/src/semantics_host.rs:11` — док.
- `flui-sdk/tests/surface.rs` — новые `MotionPolicy`, `MotionPreference`, удалённые
  `should_preserve`/`is_normal`.

## Черновик ADR

**ADR-NNNN: Reduced motion — сигнал ОС, предпочтение приложения, поведение анимации.**
Контекст: настройка движения ОС не доходила до FLUI; `AnimationBehavior` и
`AccessibilityFeatures` были без потребителей. Решение. (1) Сигнал ОС — `SystemMotion` в
`flui-platform-api`, читается `PlatformWindow::system_motion`, изменение —
`on_system_motion_changed`; типы ОС остаются в `flui-platform` (ADR-0082). (2) Политика `Full |
Reduce` = `MotionPreference` (`FollowSystem` по умолчанию, `Reduce`, `Full`) поверх сигнала;
хранится в часах презентации, приходит в реестр с тиком. (3) Под `Reduce` Normal-запуски
завершаются на следующем тике с доставкой статусов и futures, бесконечный repeat паркуется в
начале; `Preserve` (скролл, таймеры, индикаторы загрузки — задаётся явно у контроллера) не меняется. (4) Масштаб
длительностей ОС (0 → `Reduce`, < 0 и нефинитный — отказ) растягивает только Normal-время.
(5) Виджеты читают политику как inherited-значение `MediaQueryData::motion`; capability на
`LifecycleContext` не вводится — ADR-0078 относится к handles, которые действуют. Альтернативы:
политика в контроллере, только виджетами, трейт-capability с общим `Arc`, масштаб на всё время —
отвергнуты (design «Варианты»). Последствия: новое поле `MediaQueryData`, зависимость
`flui-animation → flui-platform-api`, удаление полей `AccessibilityFeatures`. Верификация: R2,
R5 (PB), R7 (PB), R10, R14, R15.

## Фрагмент changelog

```markdown
### Added
- **`flui-platform-api`**: `SystemMotion` and `PlatformWindow::system_motion`; Windows, macOS, iOS,
  Android, web and (with `a11y`) Linux report the OS reduce-motion setting and animation scale.
- **`flui-app`**: `AppConfig::with_motion_preference`; **`flui-widgets`**: `MediaQueryData::motion`.
### Changed
- **`flui-animation`**: under reduced motion, `AnimationBehavior::Normal` runs finish on the next
  frame and still report status and completion; scroll, timers and progress indicators are `Preserve` explicitly;
  the OS animation scale stretches `Normal` runs only.
- **`flui-cupertino`**: page routes do not slide under reduced motion.
### Removed
- **`flui-semantics`**: `AccessibilityFeatures::{reduce_motion, disable_animations}`;
  **`flui-animation`**: `AnimationBehavior::{should_preserve, is_normal}`.
```

## Adversarial review

- **Реентри слушателя в контроллер и реестр.** Settle идёт клоном вне lock реестра, слушатели — после
  отпускания guard контроллера (I3). Перезапуск из слушателя — новое поколение, settle на следующем
  тике (R17); livelock в одном кадре невозможен; цена — такой слушатель заказывает кадр каждый кадр,
  как и при `Full`. Регистрация из слушателя — за `fence`.
- **Снятие/добавление слушателей во время уведомления** — контракт listener-delivery; своих списков
  не вводим. Unregister себя из слушателя — запись снята, `parked` теряется вместе с ней (R17).
- **Drop последнего владельца из колбэка.** Обход держит клон на шаг; после шага контроллер
  освобождается вне lock (строка R17). ownership-handle в Drop снимает регистрацию — не под lock реестра.
- **Два контроллера на одном реестре, вложенный реестр** — один тик, порядок обхода (R19).
- **Realm остановлен / реестр заглушён / окно скрыто** — тиков нет, settle откладывается (R20);
  futures при остановке закрывает dispose (controller-robustness).
- **Retarget в последнем кадре** под Reduce — новый запуск, settle следующим тиком; retarget-спека
  (якорь `Continue`) не важна: settle не зависит от времени (R21).
- **dt = 0, огромный dt, время назад** — settle от времени не зависит (R21); масштаб: `Δ/s`
  насыщается (R8). **NaN/inf**: масштаб — отказ на границе (R8); симуляция — сетка пропускает
  нефинитные сэмплы, все нефинитны — значение прежнее (R4); цель NaN — отказ controller-robustness.
- **Переполнение** — сетка ограничена 9 сэмплами и 64 s, `x(inf)` не вызывается (D-39).
- **Panic в пользовательском коде**: слушатель в settle — R18 (сдерживание listener-delivery);
  `Simulation::x`/`is_done` в сетке — тот же контейнмент, что у тика: запуск не завершается,
  следующий кадр повторяет settle (как ownership «нет часов»); колбэк окна — `panic_boundary` (R22);
  виджет в `build` при перестройке из-за `motion` — обычный контейнмент кадра.
- **Две презентации в одном realm** — сигнал адресуется по `PresentationId`; предпочтение
  realm-уровня (`set_motion_preference`) применяется к каждой и к будущим (R13).

## Остаточные риски

- macOS, iOS, Android и JNI-путь — [U]; CI их не исполняет; Android без ContentObserver видит
  смену только после возврата в приложение.
- Linux без `a11y` — сигнал ОС не читается; документируется.
- `MotionPreference` нельзя сменить из поддерева (Motion `MotionConfig` — на поддерево): только
  realm целиком; runtime-сеттера для виджетов нет без потребителя.
- Пользовательские `RouteTransitionsBuilder` получают instant-переход (контроллер Normal) и кадр
  push в начальной позиции, если не читают `motion_of`; cross-fade вместо instant — решение владельца.
- Сторонний таймер на контроллере без `Preserve` под Reduce сработает через кадр — rustdoc
  `AnimationBehavior` говорит об этом прямо.

## Владение

| Объект | Владелец | Слабые | Unmount / замена `VsyncScope` / teardown realm |
|---|---|---|---|
| колбэк `on_system_motion_changed` | слот `handlers.rs` окна (`CallbackLease`) | — | замена колбэка ретирует старый; drop окна |
| политика и масштаб | `MotionClock` презентации | — | с презентацией |
| флаг `parked` | запись реестра | — | `unregister` снимает запись вместе с флагом (R17) |

Новых циклов нет: колбэк окна несёт только `PresentationId` и dispatch-канал в realm, не
контроллеры. Settle идёт клоном вне замка реестра; порядок замков — реестр → контроллер (как в
controller-robustness). Panic слушателя в settle — политика listener-delivery R9 (первый после
раунда); колбэк окна — `panic_boundary`.

## Паттерн

- **Закрытые `#[non_exhaustive]` enum** — `SystemMotion`, `MotionPolicy`, `MotionPreference`,
  `AnimationBehavior` (match вместо `should_preserve`/`is_normal`).
- **Newtype с инвариантом** — `DurationScale` (конечен, > 0, ≠ 1: одно написание на настройку).
- **Inherited-значение** — `MediaQueryData::motion`, не capability `LifecycleContext`.
- **Применение в одной точке** — обход `Vsync` по `FrameTick`, а не каждым виджетом.

## Отличия от Flutter/Compose/SwiftUI

| Где было похоже | Чужая форма | Rust-форма здесь |
|---|---|---|
| `MediaQuery.disableAnimations`, читаемый каждым виджетом | Flutter: каждый виджет решает сам | обход `Vsync` применяет политику ко всем `Normal` |
| `AccessibilityFeatures { reduce_motion, disable_animations }` | два bool одного смысла | `MotionPolicy` enum |
| `should_preserve()`/`is_normal()` | bool-геттеры над enum | `match` по `AnimationBehavior` |
| ×0.05 длительности под reduce (Flutter) | масштаб вместо конца | settle в следующем кадре |
| «unbounded ⇒ Preserve» | поведение как побочный эффект конструктора | `behavior` явно (X2) |

## Конвенции Rust (аудит 2026-10-06)

| Пункт конвенции | Статус | Где (до правки) | Правка |
|---|---|---|---|
| enum вместо неявного правила / bool | нарушала | design.md:63-64, 215-216, 240, 258; R6 | `behavior` явно, `Normal` по умолчанию (X2) |
| Решение владельца применено | нарушала | индикаторы не названы (X9) | индикаторы — `Preserve` |
| `#[non_exhaustive]` на растущих enum | соответствует | design.md:120-143 | — |
| Newtype с проверкой, `thiserror` | соответствует | `DurationScale`, `InvalidDurationScale` | — |
| Нет `static`/lock | соответствует | I1 | — |
| `pub` с потребителем | соответствует | design.md:168-171 | — |
| pub-поле + `with_*` у `AppConfig`/`MediaQueryData` | соответствует существующей форме | design.md:154, 162-163 | — |
| Таблица владения | нарушала | — | добавлена |
