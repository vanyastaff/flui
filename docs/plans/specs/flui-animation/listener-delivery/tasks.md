# listener-delivery — задачи

- **Статус:** черновик · **Дата:** 2026-10-06
- **Design:** [design.md](design.md); требования — [requirements.md](requirements.md)
- **Порядок работы (решение владельца 2026-10-06):** тема выходит первой, вместе с
  controller-robustness; **один PR на тему**, одна ветка `animation/listener-delivery` от `Q0`.
  T1–T8 — коммиты этой ветки в порядке графа: T1 — первый коммит (контракт + красные тесты, вывод
  в PR), [P] — разрабатываются параллельно в отдельных worktree и вливаются в ветку темы
  (непересекающиеся файлы). Изменение API и миграция `crates/`, `packages/` и
  `crates/flui-sdk/tests/surface.rs` (ADR-0088 §4) — в этом же PR. `cargo xtask check-changed`
  зелёный на голове ветки. ID R/T/D — только в этом каталоге. Пути — относительно
  `crates/flui-animation/`, если не указано иное.
- **Стык с controller-robustness:** общие файлы — `controller/{status,dispose}.rs` (здесь только
  `finish`, `take_status_change`, `channel.dispose()`) и `vsync.rs` (здесь только `tick_all`;
  там attach/дубли). Кто сливается вторым, тот rebase'ит; `finish` принадлежит этой теме.
- **Зависимости вне темы:** Q0 (split `controller.rs`, `tests/main.rs`, proptest); frame-path-state
  F2 слит до правки внутренностей (порядок задаёт спека frame-path-state): канал берёт
  `Shared`/`StateCell` из `share.rs`, своих псевдонимов не заводит; переход на `Rc`/`RefCell` —
  F3 в ядре send-flip. `CompoundAnimation` удаляет controller-robustness T10: ни одна задача этой
  темы её не трогает.
## Определение готовности (для каждой задачи)
- Тест задачи падает с откатом production-хунка по заявленной причине (откат в отдельном checkout);
  вывод красного прогона — в PR.
- Тесты через публичный API (`tests/contracts/status_delivery.rs`), семейства — строки
  `run_table`; deadlock/abort-строки — `child_process::run_rows`; время — явные `tick_at`/`tick_all`.
- Нет замка/заимствования при вызове пользовательского кода; нет новых `static`; нет
  process markers; `cargo xtask check-changed` зелёный.
## T1 — контракт (первый, блокирует остальные)
- **Файлы:** `src/status_channel.rs` (новый), `src/animation.rs`, `src/lib.rs` (2 строки
  реэкспорта), `tests/main.rs` (строка `mod status_delivery;`), `tests/contracts/status_delivery.rs`
  (новый), все реализации трейта в крейте и его тестах (`controller/status.rs`, `proxy.rs`,
  `switch.rs`, `reverse.rs`, `curved.rs`, `tween.rs`, `constant.rs`, `tests/contracts/*.rs`,
  `controller_tests.rs`, `vsync.rs` tests).
- **Что:** публичный `StatusSubscription { detach, Drop }` (`inert` — `pub(crate)`),
  `pub(crate) StatusChannel { new, subscribe, enqueue, drain, dispose }`, новая сигнатура `Animation::add_status_listener`, удаление
  `remove_status_listener`. Тела инертны там, где меняется поведение: канал хранит слушателей и
  раздаёт прежним снимком без сдерживания; владельцы держат guard вместо id. Все семейства тестов
  R1–R19 написаны и красны по assertion (не по компиляции и не по `todo!`).
- **DoD:** workspace компилируется (механическая замена у потребителей входит в этот коммит:
  сохранённый id → сохранённый guard, выброшенный id → `.detach()`, чтобы поведение не
  изменилось до T7), старые тесты зелёные, новые красные — вывод в PR; сигнатуры
  совпадают с design «Публичный API»; rustdoc с контрактом, panics и примером.
## T2 [P] — очередь, живой поиск, сдерживание (R1–R3, R5, R6, R9, R18, R19)
- **Зависит от:** T1. **Файлы:** `src/status_channel.rs`, `src/controller/status.rs`,
  `src/controller/dispose.rs` (только `channel.dispose()`), `src/controller_tests.rs`,
  `tests/contracts/controller_sources.rs`, `tests/contracts/status_delivery.rs`.
- **Что:** `StatusQueue`, `enqueue`/`drain` с `fence`, элемент `Delivery`; `finish` по design;
  сдерживание по ADR-0109 §3; исчерпание `Slot`; переписать ожидания
  `a_panicking_status_listener_leaves_the_finished_run_ok` и
  `status_failure_retains_retired_source_and_callback`; in-src тест исчерпания.
- **DoD:** строки `status_delivery_order`, `status_listener_removal`, `status_listener_failures`,
  `status_subscription_identity` (кроме обёрток), `status_delivery_lifecycle`, property
  `status_order_property` зелёные; каждая падает с откатом.
## T3 [P] — Vsync: сдерживание обхода (R10)
- **Зависит от:** T1. **Файлы:** `src/vsync.rs` (только `tick_all`),
  `tests/contracts/status_delivery.rs` (семейство `vsync_walk_containment`).
- **DoD:** строки `panicking_curve_before_sibling`, `child_registry_panic_parent_still_ticks`,
  `two_panics_first_wins`, `next_frame_ticks_all` зелёные и красные с откатом.
## T4 [P] — ProxyAnimation: атомарная связка (R15, R16)
- **Зависит от:** T2 (канал). **Файлы:** `src/proxy.rs`, `tests/contracts/proxy.rs`.
- **DoD:** семейство `proxy_binding` зелёное; `proxy_parent_queries_allow_reentrant_replacement`
  зелёный; `concurrent_set_parent_converges` живёт, пока `Animation: Send + Sync`.
## T5 [P] — AnimationSwitch (R11–R14)
- **Зависит от:** T2. **Файлы:** `src/switch.rs` (in-src тест на `debug_*_listener_count`
  переносится в `tests/` через публичный API), `tests/contracts/status_delivery.rs`
  (семейство `switch_contract`).
- **DoD:** строки `switch_contract` зелёные; deadlock-строки R11 через `run_rows` красные (timeout)
  с откатом; `dispose_breaks_proxy_cycle` проверяет счётчик drop без переподвешивания proxy.
## T6 [P] — обёртки (R7, R8)
- **Зависит от:** T2. **Файлы:** `src/reverse.rs`, `src/curved.rs`, `src/tween.rs`,
  `src/constant.rs`, `src/animation.rs` (статусная часть `ParentSubscription` удаляется).
- **DoD:** `wrapper_capture_freed_on_drop`, `reverse_maps_status`, `listener_drops_last_*`
  зелёные; `ConstantAnimation` не использует глобальный счётчик для статуса.
## T7 — миграция потребителей (в том же PR)
- **Зависит от:** T1 (механическая часть уже там), T2. Здесь — смысловая часть: хранение guard
  вместо выброшенных id, `detach` у ink_well, исправление transition_route, SDK-строка.
- **SDK:** `flui-material` начнёт импортировать `flui_sdk::animation::StatusSubscription` (поле
  состояния) — строка в `measured` `crates/flui-sdk/tests/surface.rs`. `ListenerId` остаётся в
  списке: его держат value-слушатели пакетов (`button_style_button.rs:82`, `input_decorator.rs:45`
  и др.); `StatusCallback` пакеты не импортируют (только doc-комментарий scaffold_messenger.rs:48).
- **Файлы:** `crates/flui-widgets/src/scroll/{scrollable.rs,scroll_controller.rs}`,
  `interaction/dismissible.rs`, `animated/{animated_size.rs,animated_switcher.rs}`,
  `navigator/{hero_flight.rs,transition_route.rs,navigator.rs,back_gesture.rs}`;
  `packages/flui-material/src/{drawer.rs,ink_well.rs,scaffold_messenger.rs,
  scaffold_messenger/tests/failure_cases.rs}`; `crates/flui-sdk/tests/surface.rs` (1 строка).
- **Что:** таблица design «Миграция»; `transition_route` хранит guard и снимает его в `dispose`.
- **DoD:** `rg 'remove_status_listener' crates packages src` пуст; тест в
  `crates/flui-widgets/tests/transition_route.rs`: маршрут с `will_dispose_controller == false`
  после `dispose` не получает статусы внешнего контроллера (красный с откатом).
## T8 — ADR, документация, changelog
- **Зависит от:** T2–T7. **Файлы:** `docs/adr/ADR-NNNN-animation-status-delivery.md` (номер —
  следующий свободный при слиянии), `docs/adr/ADR-0064-*.md` (строка `Superseded-by`),
  `crates/flui-animation/docs/{ARCHITECTURE,GUIDE,PATTERNS,PERFORMANCE}.md`, `README.md` крейта,
  `changelog.d/<branch-slug>.md`.
- **DoD:** текст ADR — из design «Черновик ADR»; `## Mapping decisions` получает запись «Status
  delivery is queued per channel» с именем теста `status_delivery_order`; `rg` старых имён тестов
  в docs выполнен; `cargo xtask checks` зелёный. Задача Z (общий `docs/ARCHITECTURE.md` крейта)
  принимает эти правки через своего владельца.
## Граф
Коммиты одной ветки: `Q0 → F2 (frame-path-state) → T1 → {T2 → {T4, T5, T6} [P], T3 [P]}; T1 → T7;
все → T8` (см. «Зависимости вне темы»).
