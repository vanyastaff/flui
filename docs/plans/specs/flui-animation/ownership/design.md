# ownership — design

- **Статус:** черновик
- **Дата:** 2026-10-06
- **База:** `main` @ `9a4daa3ed`
- **Требования:** [requirements.md](requirements.md) (R1–R17)
- **ADR:** да — «Контроллер на часах принадлежит handle» (черновик ниже); уточняет ADR-0125
  (снимает фразу «dropping a token does not unregister work; lifecycle owners still remove their
  registrations explicitly»), опирается на ADR-0078 §1

## Текущее состояние (чтением)

- `Vsync::register(controller) -> VsyncRegistration` (`crates/flui-animation/src/vsync.rs:166`),
  `try_register` (`:179`), `unregister` (`:204`, идемпотентно, retire после unlock `:208-214`).
  Регистрация анкерится лениво: `run_start_secs: None` (`:189-193`), первый тик ставит
  `run_start = now` (`:484-489`) — перенос идущего run в другой реестр откатывает его к elapsed 0.
- Конструкторы контроллера: `without_ticker` (`controller.rs:429`), `with_detached_ticker`
  (`:485`), `unbounded_without_ticker` (`:719`), `new(d, &UpdateScheduler::new())` в пакетах.
  controller-robustness сводит их к builder без `Ticker`.
- `VsyncScope` (`crates/flui-widgets/src/animated/vsync_scope.rs:30-83`): `update_should_notify`
  = `false` (`:76-80`); docs обещают несуществующий fallback (`:21-23`).
- `TickerMode` перевкладывает реестр в `did_change_dependencies` (`ticker_mode.rs:164-166`), но
  читает scope через `get` (`:130`) — уведомление не придёт; без scope отдаёт ребёнка «голым»
  (`:189-196`).
- `ViewState::activate` не получает контекста, `on_activate` (`crates/flui-view/src/element/behavior.rs:840-854`)
  не вызывает `did_change_dependencies` — перенос по GlobalKey зависимости не пересматривает.
- Realm оборачивает корень в `VsyncScope::new(presentation.vsync(), ..)`
  (`crates/flui-runtime/src/ui_realm/attach.rs:67,106,157`); `set_vsync` (`frame_clock.rs:69`,
  `presentation.rs:866`) — без production-вызова.

## Варианты

| | A. Handle в flui-animation (выбран) | B. Capability `LifecycleContext` | C. Только `Vsync::controller(d) -> (ctl, reg)` |
|---|---|---|---|
| Суть | `DrivenController` = контроллер + `Option<(Vsync, VsyncRegistration)>`; Drop снимает и disposes; `rebind` | `ctx.animation_controller(d)` в flui-view; реестр — поле презентации | фабрика пары, владение остаётся ручным |
| Слои | flui-animation (layer 3) не знает flui-view; scope и rebind — flui-widgets (layer 6) | flui-view (5) должен знать поддеревные реестры `TickerMode` (сейчас inherited data в flui-widgets) → дубль механизма inherited | без изменений |
| Закрывает | D-21/22/42, класс «забыли dispose» — типом | то же, но ценой второго механизма scope | не закрывает D-42 и D-22 |
| Минусы | 2 строки на потребителя в двух хуках | ломает `TickerMode`, вводит subtree-state в flui-view | правило остаётся ревью |

**Выбор A.** Rust-форма владения — RAII: регистрация живёт ровно столько, сколько handle, а
ручная регистрация закрыта видимостью (`pub(crate)`). B отвергнут: подменяет inherited
`VsyncScope` (на нём держится `TickerMode`) вторым механизмом поддерева.

**Имя.** `DrivenController` — «контроллер, ведомый часами Vsync»; без grab-bag-суффиксов
(`VsyncBound` описывает связь, а не вещь). Альтернатива `ClockedController` — на выбор владельца.

## «Нет часов»: варианты

| | U1. Settle сразу + один `warn` (выбран) | U2. Заморозка + `error` | U3. Wall-clock fallback |
|---|---|---|---|
| Поведение | run завершается синхронно в конечном состоянии, статус и `Ok` ровно раз | run висит, future не резолвится | свой таймер |
| Почему | автомат UI (route pop, dismiss, snackbar) продвигается; эталон SwiftUI: completion «сразу, если анимаций нет» (market-A I9), reduce motion сокращает, а не теряет колбэки (market-B A4) | навигатор и Dismissible ждут вечно | нет кадров без презентации; путь кадра синхронный — невозможно |

Тот же settle-путь использует спека reduce-motion; ownership добавляет только
источник «нет реестра». Simulation без часов: первое `t` из `0.25·2ⁿ ≤ 64` s с `is_done(t)`,
иначе последний конечный сэмпл (R13).

## Решение

### Публичный API (дельта)

```rust
// flui-animation, controller/driven.rs (новый модуль после Q0)
/// An [`AnimationController`] together with its seat on the [`Vsync`] that drives it.
/// Dropping it unregisters, then disposes the controller; no lock is held while the
/// displaced run's continuations run. Without a registry every run settles at once.
#[must_use = "dropping a DrivenController cancels its animation"]
pub struct DrivenController { controller: AnimationController, seat: Seat }
enum Seat { Bound { vsync: Vsync, registration: VsyncRegistration }, Unbound { warned: bool }, Retired }

impl DrivenController {
    /// Moves the controller to `vsync` (`None` = no clock). Same registry → no-op.
    /// A live run keeps its elapsed time (`Vsync` anchors it at `now − last elapsed`).
    /// # Errors
    /// [`VsyncRegistrationError::Exhausted`]: the handle stays unbound; the old seat is released.
    pub fn rebind(&mut self, vsync: Option<&Vsync>) -> Result<(), VsyncRegistrationError>;
    /// Unregisters, then disposes. Idempotent. A continuation panic propagates after both.
    pub fn dispose(&mut self);
    #[must_use] pub fn controller(&self) -> &AnimationController;
    #[must_use] pub fn is_bound(&self) -> bool;
}
impl AsRef<AnimationController> for DrivenController { /* = controller() */ }
impl Drop for DrivenController { /* = dispose; under unwind — contained, payload retained */ }
// `Deref` нет (C-DEREF: handle не умный указатель; правило владельца «без Deref ради наследования»).

// AnimationControllerBuilder (из controller-robustness): терминальные методы (решение X8)
/// Infallible: исчерпание идентичностей реестра → handle `Unbound` + `error!` (R10).
pub fn build_on(self, vsync: Option<&Vsync>) -> DrivenController; // `None` — без часов (R12)
pub fn build(self) -> AnimationController;                         // ручной tick_at: тесты, бенчи

// AnimationController::dispose → pub(crate) после миграции (T6): контроллер на часах
// освобождает только handle; ручной — drop последнего клона.
// Vsync: register / try_register / unregister → pub(crate). Остаются pub: new, tick_all,
// has_running, attach_child/detach_child, is_same, set_muted, is_muted, len, is_empty.

// flui-widgets
impl VsyncScope {
    /// The registry for `ctx`'s subtree, registering a dependency: call from
    /// `init_state` and `did_change_dependencies` and pass it to `DrivenController::rebind`.
    #[must_use] pub fn maybe_of(ctx: &dyn LifecycleContext) -> Option<Vsync>;
}
// update_should_notify(&self, old) = !self.vsync.is_same(&old.vsync)
```

Без `Deref` вызовы идут через `controller()` (`self.controller.controller().forward()`,
~70 мест в 18 файлах) или `AsRef`, если generic-bound этого просит. Обход снятия регистрации
закрыт видимостью, а не разрешением методов: `AnimationController::dispose` — `pub(crate)`, так что
`driven.controller().clone().dispose()` не компилируется (trybuild
`driven_controller_has_no_bare_dispose.rs`). Клоны контроллера (`controller().clone()` для
`CurvedAnimation`, слушателей) — обычные наблюдатели.

### Инварианты

1. Контроллер стоит в реестре ⇔ им владеет живой `DrivenController` в состоянии `Bound`.
2. Порядок освобождения: `Seat` → `Retired` (state коммитится) → `unregister` (guard реестра
   отпущен до retire) → `controller.dispose()` (доставка без lock). Пользовательский код
   видит handle уже `Retired`.
3. `rebind` на тот же реестр не меняет ничего; на другой — сначала новая регистрация
   (`try_register`), потом снятие старой: отказ не оставляет контроллер без старых часов…
   кроме `Exhausted`, где по контракту handle уходит в `Unbound` (R10).
4. Перенос сохраняет время: `Vsync` получает `resume_elapsed: Duration` (crate-private чтение
   elapsed последнего сэмпла текущего run) и анкерит `run_start = tick.now() − resume_elapsed`
   (насыщение к `AnimationTime::ZERO`; типы — motion-clock). Якорь — общий `RunAnchor` реестра
   (вариант `Resume`), рядом с `Continue` retarget ([../retarget/design.md](../retarget/design.md));
   форму якоря задаёт motion-clock T3 (см. `../review.md`).
5. Без часов контроллер помечен `ClockBinding::Missing` (crate-private, ставит только handle);
   запуск run в этом состоянии идёт через settle-путь. Контроллер из `build()` —
   `ClockBinding::Manual`: поведение сегодняшнего `tick_at`.
6. Handle не вводит своих lock и общих ячеек: только контроллер (realm-local после
   frame-path-state) и значения `Vsync`/`VsyncRegistration`; `Send`/`Sync` наследует от них.
   Новых `static` нет (ADR-0097).

### Потребитель (образец)

```rust
fn init_state(&mut self, ctx: &dyn LifecycleContext) { self.bind(ctx) }
fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) { self.bind(ctx) }
fn bind(&mut self, ctx: &dyn LifecycleContext) {
    if let Err(err) = self.controller.rebind(VsyncScope::maybe_of(ctx).as_ref()) {
        tracing::error!(%err, "controller left without a clock");
    }
}
// dispose(): ничего — поле уходит вместе с state; явный `self.controller.dispose()` допустим.
```

`TickerMode`: `renest` читает через `maybe_of` (зависимость). С родителем `build` отдаёт свой
вложенный реестр, как сейчас. Без родителя его реестр никто не тикает, а «голый» ребёнок
(`:189-196`) унаследовал бы чужой scope выше, если тот появится позже. Поэтому `build` отдаёт
`VsyncScope::detached(child)`: `maybe_of` под ним = `None`, потомки — «без часов» по R12 (R14),
и смена на подключённый реестр приходит обычным уведомлением scope.

## Миграция (production, `rg` на `9a4daa3ed`)

| Файл | register / unregister / dispose (строки) | Что становится |
|---|---|---|
| `crates/flui-widgets/src/animated/implicitly_animated.rs` | 89 / 163 / 165, 258 | `ImplicitController.controller: DrivenController`; `register`/`dispose` удаляются |
| `animated_opacity.rs`, `animated_padding.rs`, `animated_align.rs`, `animated_container.rs` | 128/161, 88/119, 123/173, 161/230 (через ImplicitController) | `bind` в двух хуках |
| `animated/animated_size.rs` | 216 / 284 / 286 | handle; снятие status-слушателя остаётся |
| `animated/animated_switcher.rs` | 355, 499, 510 / 402 / 404, 522, 582, 585 | `ChildEntry` держит handle; rebind всех entry в `did_change_dependencies` |
| `animated/ticker_mode.rs` | `attach_child`/`detach_child` | `maybe_of`, `VsyncScope::detached` |
| `interaction/dismissible.rs` | 685, 1010, 1304 / 932, 944, 992 / 934, 946 | два handle; танец `unregister_move_controller_vsync`/`ensure_move_controller_registered` (`:988-1012`, вызовы `:1076,1139,1162,1227`) удаляется — гонку закрыл `walk_probe` |
| `navigator/transition_route.rs` | 659 / 772 / 778 | handle; реестр — от binding навигатора (`binding.vsync()`), `will_dispose_controller` (всегда `true`) удаляется |
| `scroll/scrollable.rs` | 509 / 821 / 831 | handle (unbounded builder) |
| `scroll/refresh_indicator.rs` | 432 / 601 / 603 | handle |
| `scroll/sliver_persistent_header.rs` | 246 / 299 / — (D-42a) | handle → dispose при drop |
| `flui-objects/src/sliver/sliver_persistent_header.rs` | `set_snap_controller` `:1067`, attach `:1289`, detach `:1297` | при attached: снять слушатель со старого, подписать новый (D-42c) |
| `packages/flui-cupertino/src/button.rs` | 500 / 604 / 607 | builder + handle (через `flui_sdk::animation`) |
| `packages/flui-material/src/drawer.rs` | 722 / 809 / 811 | handle |
| `packages/flui-material/src/ink_well.rs` | 525 / 264, 545 / 265 | handle на нажатие; замена поля освобождает прежний |
| `packages/flui-material/src/scaffold_messenger.rs` | 558, 711 / 575, 729 / 579, 731 | два handle; `replace` освобождает старый (D-42b) |

Итого 12 владельцев + 4 обёртки + `TickerMode` + render-object. Вне production: тесты и
примеры (`examples/animated_box_app.rs:200`, `vertical_slice_demo/frame_histogram.rs:121`,
`workload_probe.rs:468`, `lifecycle_probe.rs:153`) → `build()` или `build_on(Some(&vsync))`;
`flui-testing` (`lib.rs:120-272`, `widgets.rs:421`) регистрирует через handle. `flui-sdk`
поверхность: `DrivenController` добавляется в `tests/surface.rs` (SDK `0.N` bump вместе с
controller-robustness).

## Adversarial review

- **Reentry в контроллер и реестр.** Drop/`dispose` не держит guard при вызове пользовательского
  кода (инвариант 2); continuation может вызвать `tick_all`, `build_on(..)`, `rebind` другого handle.
  `rebind` из слушателя *этого же* контроллера: handle у владельца под `&mut` — недостижим из
  колбэка без `RefCell`; при `RefCell` повторный borrow — panic владельца, не наш дедлок. Тест R1, R6.
- **Снятие/добавление слушателей во время уведомления.** Handle не трогает раздачу;
  listener-delivery. Drop handle из слушателя: `dispose` очищает status-слушатели, текущая
  раздача по контракту listener-delivery не вызывает снятых.
- **Последний владелец из колбэка.** `tick_all` держит собственный клон контроллера на шаг
  (`vsync.rs:494-509`), unregister из колбэка удаляет запись — курсор её не встретит. R6.
- **Два контроллера на одном Vsync.** Один роняет другого → тот снят до своего шага, пропущен. R6.
  Дубль регистрации одного контроллера теперь непредставим: регистрирует только handle,
  контроллер у handle один.
- **Realm остановлен посреди анимации.** Teardown элементов роняет state → handle → cancel.
  Если handle утёк из дерева (захвачен в глобальном `Rc`), он держит `Vsync` сильно и run висит
  до drop — остаточный риск, тот же, что сегодня у утёкшего контроллера; `has_running` реестра,
  который никто не тикает, ничего не держит.
- **Panic в пользовательском коде.** Continuation в `dispose` — R7 (handle уже `Retired`, первый
  payload сохраняется по ADR-0106). Panic в `Simulation` при settle без часов (R13) —
  распространяется без lock, run остаётся установленным и `ClockBinding::Missing`, следующий
  запуск снова пробует settle; диагностика — один `warn`. Panic внутри `rebind` невозможен:
  `try_register` не вызывает пользовательский код.
- **Переполнение и NaN.** Счётчик — ADR-0125 (R10). Неконечное время непредставимо (`Duration`,
  `AnimationTime` motion-clock); огромный elapsed — `saturating_sub` к нулю, не паника.
- **Остаточные риски.** (1) R15 требует изменения `flui-view` (активация пересматривает
  зависимости) — вне разрешённых файлов, решение владельца. (2) Клон контроллера
  (`controller().clone()`) — наблюдатель: `dispose` у него нет (`pub(crate)` после T6), обойти
  снятие регистрации нельзя; до T6 запись в реестре остаётся до drop handle (не тикается:
  `live_running` = false). (3) Settle без часов меняет наблюдаемое поведение тестов без scope —
  тесты мигрируют на явный scope.

## Владение (долгоживущие объекты)

| Объект | Сильные ссылки | Слабые | Unmount | Замена `VsyncScope` | Teardown realm |
|---|---|---|---|---|---|
| `DrivenController` | поле state виджета (единственный владелец) | — | drop state → `dispose`: `Retired` → `unregister` → `controller.dispose()` | `rebind`: новая регистрация, затем снятие старой | teardown элементов роняет state → как unmount |
| `Seat::Bound { vsync, registration }` | handle | `VsyncRegistration` — `Weak` реестра | снимается первым | заменяется | реестр мёртв: `unregister` — no-op |
| запись реестра (клон контроллера) | `Vsync` | — | `unregister` | `unregister` старого | drop `Vsync` |
| клоны контроллера у обёрток/слушателей | обёртки, колбэки | — | живут до своих владельцев, реестр не держат | — | — |

Циклы: **C7** (`Vsync` → контроллер → слушатель с клоном `Vsync`) — регистрацию держит handle, его
drop снимает запись независимо от слушателя; тест «Drop == 1» `driven_controller_drop_unregisters`
(Weak-сентинел `None` после drop handle и `Vsync`). Предпосылки из других тем: **C1** — `dispose`
очищает value-слушателей (controller-robustness R6.3, `dispose_clears_value_listeners`); **C3** —
`Ticker` удалён (controller-robustness B, `last_handle_drop_cancels_running_run`). Порядок замков
(до send-flip T6c): реестр → контроллер; handle своих замков не вводит, `unregister` отпускает
guard реестра до retire (инв. 2).

## Паттерн

- **RAII guard** — `DrivenController` владеет местом в реестре; `Drop` снимает регистрацию.
- **Закрытый enum состояний** — `Seat { Bound, Unbound, Retired }` вместо `Option` + флагов;
  typestate не нужен: переходы только через `rebind`/`dispose` (`&mut self`).
- **Видимость как правило** — `register`/`unregister`/`AnimationController::dispose` →
  `pub(crate)`: ручную пару не написать (trybuild).
- **Infallible builder-терминал** — `build_on` без `Result`: единственный отказ (`Exhausted`)
  имеет определённое поведение (`Unbound`).

## Отличия от Flutter/Compose/SwiftUI

| Где было похоже | Чужая форма | Rust-форма здесь |
|---|---|---|
| `Deref<Target = AnimationController>` | наследование: handle «является» контроллером | `controller()`/`AsRef`, без `Deref` |
| `TickerProviderStateMixin` + `dispose()` в `State.dispose` | mixin и ручной парный вызов | RAII-handle: поле state, освобождение в `Drop` |
| `TickerMode`/`Ticker.muted` | флаг на изменяемом тикере | вложенный реестр + уведомление `VsyncScope` по идентичности |
| `build_manual` рядом с `build(Option) -> Result` | два вида контроллера, `Result` без причины отказа | `build()` — ручной `AnimationController`, `build_on` — handle, infallible |

## Конвенции Rust (аудит 2026-10-06)

| Пункт конвенции | Статус | Где (до правки) | Правка |
|---|---|---|---|
| Без `Deref` ради наследования | нарушала | design.md:82, 101-104, 199-201 | `controller()` + `AsRef`; `dispose` — `pub(crate)` |
| Один терминальный метод builder | нарушала | design.md:85-87, 168, 215 | `build_on(Option<&Vsync>) -> DrivenController`, `build()` |
| `Result` только с причиной отказа | нарушала | design.md:86 (`Result<_, AnimationError>`) | infallible, `Exhausted` → `Unbound` |
| Имена после удаления `Ticker` | нарушала | requirements R6–R7 (`TickerCanceled`) | `RunCanceled`/`RunFuture` |
| Таблица владения + «Drop == 1» | нарушала | — | раздел «Владение», C7 + предпосылки C1/C3 |
| Порядок замков | нарушала | — | реестр → контроллер |
| RAII, `#[must_use]` на handle | соответствует | design.md:67 | — |
| Новых `static`, lock — нет | соответствует | инв. 6 | — |
| Panic: первый после раунда | соответствует | design.md:77, 191-195 | — |
| `rebind(&mut self)` — реентри невозможно без `RefCell` | соответствует | design.md:177-178 | — |

## Открытые вопросы (владельцу)

1. ~~Имя~~ — решено: `DrivenController` (orchestration, «Решения по развилкам»).
2. R15: добавить в `flui-view` пересмотр зависимостей при `activate` (как Flutter
   `Element.activate` → `didChangeDependencies`) — отдельная задача T7 с файлами flui-view.

## Черновик ADR

**ADR-NNNN: Контроллер на часах принадлежит `DrivenController`.** *Supersedes:* ADR-0125, фраза
«Dropping a token does not unregister work; lifecycle owners still remove their registrations
explicitly». Решение: единственный способ поставить `AnimationController` на `Vsync` —
`AnimationControllerBuilder::build_on(Option<&Vsync>) -> DrivenController`; `Vsync::register`,
`try_register`, `unregister` — внутренние. Drop/`dispose` handle снимают регистрацию, затем
disposes контроллер, без удерживаемых guard. `rebind` переносит handle между реестрами, сохраняя
прошедшее время run. Без реестра каждый run завершается синхронно в конечном состоянии с одним
предупреждением. `VsyncScope` уведомляет зависимых при смене идентичности реестра; реестр берётся
только через `VsyncScope::maybe_of(&dyn LifecycleContext)`. Следствия: класс «забытая пара
register/dispose» исчезает типом; тесты без scope видят мгновенное завершение вместо заморозки.

## Changelog

```markdown
### Changed
- `AnimationControllerBuilder::build_on(vsync)` returns a `DrivenController` that unregisters and
  disposes its controller on drop; `Vsync::register`/`unregister` are no longer public.
- `VsyncScope` notifies dependents when its registry changes; animated widgets follow the new
  registry. Without a `VsyncScope`, implicit animations land on their target at once instead of
  freezing.
### Fixed
- The floating sliver header disposes its snap controller; the snack-bar display timer retires
  the previous timer; swapping a header's snap controller moves its layout listener.
```
