# flui-animation — дефекты и план волн

> Исторический реестр на базе 2026-10-06. Текущий порядок работ и проверенные остатки:
> [readiness-plan.md](readiness-plan.md), [readiness-audit.md](readiness-audit.md).
> Старые D-ID сохранены для трассировки; их наличие в таблице не означает текущий дефект.

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Оркестрация:** [orchestration.md](orchestration.md); матрица эталона — [market.md](market.md).
- **База:** `main` 9a4daa3ed. `file:line` ниже — на этот коммит.

## Дефекты (без дублей, по убыванию серьёзности)

H — высокая, M — средняя, L — низкая. «Не подключено» — production-вызовов нет.

| ID | Дефект | Сер. | Где | Тема |
|----|--------|------|-----|------|
| D-01 | Panic слушателя статуса обрывает раздачу; переход потерян навсегда (маркер уже сдвинут); в `Vsync::tick_all` пропускает остальные контроллеры кадра, включая родительские после дочернего реестра | H | controller.rs:2371-2376, 2473; proxy.rs:43-45; switch.rs:260, 270-272; vsync.rs:462-466 | listener-delivery |
| D-02 | `AnimationSwitch` читает `value()`/`status()` родителей под своим нереентерабельным mutex | H | switch.rs:294-295, 445, 450, 492-499 | listener-delivery |
| D-03 | Lerp `Matrix4` даёт NaN при нулевом масштабе на конце; skew/perspective теряются | H | flui-foundation matrix4.rs:173-181 | interpolation |
| D-04 | Curved-прогон публикует выход кривой без проверки конечности и clamp | M-H | controller.rs:1921-1940, 2012 | controller-robustness |
| D-05 | Reduce motion нигде не проведён | M | status.rs:108-138; flui-app runtime.rs:97; media_query.rs:47-87 | motion-clock |
| D-06 | Реентерабельные смены статуса приходят не по порядку | M | controller.rs:2411-2418 | listener-delivery |
| D-07 | Снятый во время раздачи (или `dispose()`) слушатель статуса всё равно вызывается | M | controller.rs:2372; proxy.rs:36-46; switch.rs:270 | listener-delivery |
| D-08 | Implicit-анимации при retarget стартуют с 0: теряется скорость (C¹) | M | flui-widgets implicitly_animated.rs:147-152 | retarget |
| D-09 | Hop в switch не сообщает слушателям статус нового поезда | M | switch.rs:313-345 | listener-delivery |
| D-10 | `ProxyAnimation::set_parent` меняет три `RwLock` неатомарно | M | proxy.rs:193-226 | listener-delivery |
| D-11 | `is_animating` различается через обёртки | M | animation.rs:88; controller.rs:2715-2728 | controller-robustness |
| D-12 | Перцептивные конструкторы пружины принимают невалидный вход в release; pub-поля обходят проверку | M | simulation.rs:121-128, 183-225 | physics |
| D-13 | Scroll-симуляции используют допуск в долях единицы на пикселях | M | simulation.rs:42-46, 821-827, 922-929 | physics |
| D-14 | `FLING_TOLERANCE` мёртв: fling заканчивается ~0.26 с после границы | M | controller.rs:27-31, 1613-1628 | physics |
| D-15 | `BouncingScrollPhysics` без перехода friction→spring: fling в край останавливается намертво | M | flui-widgets scroll_physics.rs:349-355 | physics |
| D-16 | `RefreshIndicator` пересобирает поддерево на каждый пиксель прокрутки | M | refresh_indicator.rs:449 | integration |
| D-17 | Валидация кривых обходится через pub-поля и serde | M | curve.rs:140-148, 196-200, 229-239, 335-350, 586-590 | curves |
| D-18 | `ReverseCurve`/`Curve::reversed` нарушает контракт 0→0, 1→1 | M | curve.rs:51-60, 977-996 | curves |
| D-19 | `CatmullRomCurve` игнорирует x, концы не 0/1, tension задокументирован наоборот, panic на пустых точках; `SawTooth(1)=0` | M | curve.rs:791-861, 131-134 | curves |
| D-20 | `lerp_oklab` без premultiplied alpha | M | flui-painting color.rs:610-623 | interpolation |
| D-21 | Implicit-виджеты без `VsyncScope` не анимируются, docs обещают fallback | M | implicitly_animated.rs:72-74; vsync_scope.rs:21-23; ticker_mode.rs:180-196 | ownership |
| D-22 | `VsyncScope` никогда не уведомляет: замена/перенос реестра замораживает контроллеры | M | vsync_scope.rs:76-80; ticker_mode.rs:160-166 | ownership |
| D-23 | `friction.through` паникует на underflow drag, с `ve=0` не завершается | M | simulation.rs:699-708, 730 | physics |
| D-24 | `SmoothDamp`: panic при отрицательном `max_speed`, зависимость от частоты кадров, вечный NaN | M | smoothing.rs:144, 160-163, 187, 193-196 | physics |
| D-25 | Time dilation масштабирует весь прогон и глобальна на процесс | L-M | controller.rs:1897, 2521; scheduler config.rs:9 | motion-clock |
| D-26 | `Duration::MAX` паникует в `mul_f64` под lock после частичной мутации | L-M | controller.rs:2505-2515 | controller-robustness |
| D-27 | Нет типизированной идентичности слушателей: id с 1 в каждом реестре, общий тип value/status, счётчик может переполниться | L-M | controller.rs:658, 2696-2697; proxy.rs:30; constant.rs:15 | listener-delivery |
| D-28 | Слушатели статуса через `Compound`/`Reverse` живут на родителе: цикл ссылок | L-M | compound.rs:216-223; reverse.rs:91-108 | listener-delivery |
| D-29 | `Switch::dispose` держит `on_switched` и слушателей | L-M | switch.rs:402-430; transition_route.rs:443-450 | listener-delivery |
| D-30 | Скорость fling в Dismissible — фиксированный масштаб 1/300 | L-M | dismissible.rs:107, 1175, 1180 | retarget |
| D-31 | После `dispose` работают `set_value` и `add_*listener`; value-слушатели не очищаются | L | controller.rs:2151-2214, 2693-2700 | controller-robustness |
| D-32 | `on_frame_scheduled` вызывается под guard контроллера: инверсия lock и полузапущенный прогон | L | controller.rs:2319-2339; vsync.rs:340-351 | controller-robustness |
| D-33 | Неконечный `now_secs` навсегда закрепляет прогон на NaN | L | vsync.rs:484-489 | motion-clock |
| D-34 | `tick()` отматывает Vsync-прогон к началу | L | controller.rs:1854-1860 | controller-robustness |
| D-35 | Проверка цикла в `attach_child` — TOCTOU; дубли регистрации тикаются дважды | L | vsync.rs:225, 232 | controller-robustness |
| D-36 | `controller.velocity()` игнорирует кривую | L | controller.rs:1765-1787 | retarget |
| D-37 | Солвер cubic проверяет допуск только по x (ошибка y до 9e-3) | L | curve.rs:271-312 | curves |
| D-38 | Elastic-кривые прыгают на концах; политика NaN разная у кривых и tween'ов | L | curve.rs:606-704; tween_types.rs:117, 144 | curves |
| D-39 | Переполнение в пружине (`4mk`, `x(+inf)`=NaN); `AnimatedValue` держит NaN навсегда | L | simulation.rs:497, 572; spring.rs:113-181 | physics |
| D-40 | `Bounded`/`Clamped` паникуют на неупорядоченных или NaN-границах | L | simulation.rs:859-873, 889-892, 934 | physics |
| D-41 | Slide/Scale/Rotation-переходы, drawer, dismissible анимируют transform пересборкой | L | slide_transition.rs:111-117; drawer.rs:735-738; dismissible.rs:669-671 | integration |
| D-42 | Владение у потребителей: sliver header не освобождает контроллер, таймер snackbar перезаписывает регистрацию, `set_snap_controller` не переподписывается | L | sliver_persistent_header.rs:292-301; scaffold_messenger.rs:549-569; objects sliver_persistent_header.rs:1067-1069 | ownership |
| D-43 | `#[derive(Animatable)]` реализует `TwoWayConverter`; гигиена; принимает unit-структуры | L | flui-macros derive_animatable.rs:75-120 | derive |
| D-44 | Switch шлёт лишние уведомления значения; `Compound::divide` публикует NaN | L | switch.rs:78-79, 269; compound.rs:85-95 | listener-delivery |
| D-45 | Бенч `tick_at` меряет остановленный контроллер; таблицы PERFORMANCE.md не измерены или неверны | L | animation_bench.rs:133-145; PERFORMANCE.md:27, 160-194, 477-490 | quality |
| D-46 | Раскладка тестов: единственный бинарь `tests/derive_animatable.rs`, тесты публичного API в `src`, мёртвый `SERIAL`, чтение приватного поля, process markers | L | controller_tests.rs:12-20, 230, 571 | quality |
| D-47 | Ошибки несут `String`; `TickerNotAvailable` не создаётся | L | error.rs:28-95 | controller-robustness |

## Темы

| Тема | Форма | ADR | Закрывает |
|------|-------|-----|-----------|
| listener-delivery | спека | да (контракт наблюдателя, уточняет ADR-0064 §4) | D-01, 02, 06, 07, 09, 10, 27, 28, 29, 44 |
| controller-robustness | спека | да (Vsync — единственные часы контроллера; заменяет ADR-0064 §6) | D-04, 11, 26, 31, 32, 34, 35, 47 |
| motion-clock | спека | да (time dilation и motion policy у часов презентации; выход `TIME_DILATION` из ADR-0097) | D-05, 25, 33 |
| physics | спека | нет | D-12, 13, 14, 15, 23, 24, 39, 40 |
| retarget | спека | да (retarget сохраняет значение и скорость) | D-08, 30, 36 |
| ownership | спека | да (владеющий handle контроллера) | D-21, 22, 42; ручные register/unregister/dispose в ~14 файлах |
| integration | спека | нет | D-16, 41 |
| curves | design + tasks | нет | D-17, 18, 19, 37, 38 |
| interpolation | design + tasks | нет | D-03, 20 |
| derive | карточка | нет | D-43 |
| quality | карточки | нет | D-45, 46; proptest; PERFORMANCE.md |

Решения по вариантам — в `design.md` каждой темы.

## Волны

Задача фиксирует контракт первым коммитом: публичные типы с инертными телами и тесты, красные по
assertion; вывод красного прогона — в PR. Второй коммит — поведение. Каждая [P]-задача — своя ветка
и worktree от `Q0`.

```text
Q0 quality-base ─┬─ [P] A listener-delivery ───────┐
                 ├─ [P] B controller-robustness ───┼─► W2 ownership + миграция ─► W3 integration
                 ├─ [P] C motion-clock ────────────┼─► W2 reduce-motion (после B, C)
                 ├─ [P] D physics ─────────────────┼─► W2 retarget (после B, D; A для статусов)
                 ├─ [P] E curves  ├─ [P] F interpolation ├─ [P] G derive
                 └─────────────────────────────────┴─► Z: чистка lib.rs, docs, бенчи «после»
```

| Задача | Разрешённые файлы |
|--------|-------------------|
| Q0 quality-base | `controller.rs` → `controller/{mod,run,tick,status,dispose}.rs` без изменения поведения; `tests/main.rs`; `Cargo.toml` (proptest, `[[test]]`); `benches/*`; тест аллокаций |
| A listener-delivery | `controller/status.rs`, `animation.rs`, `proxy.rs`, `switch.rs`, `compound.rs`, `reverse.rs`, `curved.rs`, `vsync.rs` (сдерживание обхода) |
| B controller-robustness | `controller/{run,dispose,mod}.rs`, `error.rs`, `builder.rs`, `vsync.rs` (attach, дубли) |
| C motion-clock | `controller/tick.rs`, `status.rs`, flui-scheduler `config.rs`/`scheduler.rs`, flui-runtime `frame_clock.rs`/`frame.rs`/`presentation.rs`, flui-platform-api, flui-platform, flui-app `realm_dispatch.rs`, flui-widgets `media_query.rs` |
| D physics | `simulation.rs`, `spring.rs`, `smoothing.rs`, flui-widgets `scroll_physics.rs`, `page_view.rs` |
| E curves | `curve.rs` |
| F interpolation | `tween_types.rs`, flui-foundation `matrix4.rs`/`lerp.rs`, flui-painting `color.rs` |
| G derive | flui-macros `derive_animatable.rs` и его тесты |

Общие файлы: `lib.rs` и `Cargo.toml` крейта — каждая задача добавляет только свои строки,
конфликты разрешает интегратор при rebase; `docs/ARCHITECTURE.md` — задача Z; миграция
`flui-widgets` — W2.

## Бенчи

- Q0 чинит `controller/tick_at` (живой прогон, варианты: без кривой, `EaseInOut`, 1 и 4 слушателя,
  симуляция), `curved_value`, `animated_value_color_frame` (`iter_batched`), `smooth_damp_step`;
  добавляет раздачу статуса N=1/4/8 и стоимость `forward()`.
- «До» — исправленные бенчи на коде `main` (baseline `before`); «после» — на итоговой ветке, тот же хост.
- Аллокации на тик — отдельный `[[test]]` со счётным `#[global_allocator]` (шаблон
  `crates/flui-scheduler/tests/frame_telemetry_allocation.rs`); `cargo xtask globals` не сканирует
  `tests/` и `benches/`.
