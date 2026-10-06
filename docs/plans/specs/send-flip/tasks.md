# send-flip — задачи

- **Статус:** черновик
- **Дата:** 2026-10-05
- **Design:** [design.md](design.md) («Работы», «Тестовая стратегия»); требования — [requirements.md](requirements.md);
  уровень 0 — [../release/requirements.md](../release/requirements.md) D3, D5 и [../release/tasks.md](../release/tasks.md)
- **База:** `main` @ `4915054c8`
- **Итог:** 13 задач (T1–T4 в `main`, T5–T7 и T9 в `send-flip/core`, T8 — прогон); ≈ 48,5 инженеро-дня;
  критический путь 24 рабочих дня; мерж ядра ~11-09; go/no-go 11-17; отсечка 11-24

## Правила исполнения

- ID совпадают с пунктами «Работ» design (T6a–T6e = 6a–6e). Задача = worktree (`cargo xtask worktree new
  send-flip/<slug>`) = PR, `cargo xtask check-changed` зелёный до ревью. ID задач и требований — только в этом
  каталоге: не в коде, именах тестов, причинах `ignore`, коммитах.
- **В `main`** (T1–T4) уходит только согласованная модель: окончательный async-драйвер, путь записи, ослабления.
  **Ядро** (T5–T7, T9) — ветка `send-flip/core`, один PR в `main`. T6x — ветки от `send-flip/core`, PR в неё с CI
  (`check-changed`), слияние squash: история ядра линейна (см. «Перебазирование»). Файлы T6a–T6e не
  пересекаются, поэтому они сливаются в ядро в любом порядке; остальные после каждого слияния делают
  `git rebase send-flip/core`.
- **Тесты первыми.** Первый коммит PR — тесты нового контракта, красные (по assert, trybuild «expected compile
  failure» или E0277), вывод — в PR. T5 вносит строки R7/R11–R13 ядра с `#[ignore = "contract: <поведение>"]`
  в файлы их будущих владельцев; T6x снимает `ignore` и переносит строки в таблицу семейства.
- **Мутация** для каждой строки R7/R11–R13 — названная правка production-кода в изолированном checkout (или
  с восстановлением исходных байтов в `finally`); строка обязана упасть по названной причине, вывод — в PR.
- **Где:** L — Linux remote (всё, кроме T8); W — Windows host (только T8). trybuild — группа `trybuild`,
  consumer — `nested-cargo` (`.config/nextest.toml`); обе исполняются в L.

## Граф зависимостей

```mermaid
graph LR
  T3["T3 путь записи ≤10-14"] -.flui-testing/src/lib.rs.-> T1["T1 AsyncDriver 10-06→≤10-17"]
  T1 --> P4["persistence P4 (≤10-17)"]
  T3 --> FK4["focus-keyboard T4"]
  T1 --> T4["T4 ослабления"] --> T5["T5 C1, ядро 10-14→10-19"]
  T1 & T2["T2 gate ≤10-16"] --> T5
  T5 --> T6a & T6b & T6c & T6d & T6e
  T6a & T6b & T6c & T6d & T6e & T3 --> T7["T7 закрытие 10-27→10-30"]
  TD8["teardown T8"] --> T7
  T7 --> T8["T8 Windows smoke ~11-05"] --> M["ядро → main ~11-09"]
  M --> GO{"go/no-go 11-17"} --> CUT["отсечка 11-24"]
  T6e & TI2a["text-ime T2a ≤10-17"] & TI5["text-ime T5 ≤11-04"] --> T9["T9 TextEditingController"]
  M --> FK["focus-keyboard T7, T9, T10, T12"] & FS["facade-surface"] & AS["authoring-styles"]
  TI3["text-ime T3"] -.ребейз frame.rs, pump.rs.-> T6a
```

Кросс-фичевые рёбра: **T1 → persistence P4** (owner-local `AsyncDriver`, `FutureBuilder` с `!Send` future; ≤10-17
жёстко, иначе путь persistence удлиняется на опоздание); **T3 → focus-keyboard T4** (`flui-view/src/owner/**`);
**teardown T8 → T7** (настоящий `report_contained_panic` для строки отчёта realm); **text-ime T2a, T5 → T9**
(`text/controller.rs`); **ядро → focus-keyboard T7, T9, T10, T12, facade-surface, authoring-styles**.

## Задачи

| ID | Задача | Требования | Файлы | Зависит | P | Где | Проверка | Готово (мутации) | Дн | Статус |
|---|---|---|---|---|---|---|---|---|---|---|
| T1 | `AsyncDriver`/`FutureBuilder` в окончательной форме (строки 1–3, 33): `OwnerFrame`, `TaskStore`, `Weak` `AsyncDriver`, `FrameWaker`, входы кадра с `&OwnerFrame`, drain в teardown; отдельный PR в `main`; карточка ниже | R4 (`FrameWaker`), R9, R10, R13 | `crates/flui-scheduler/src/{async_driver,scheduler,post_frame}.rs`, `crates/flui-scheduler/tests/**`, `crates/flui-view/src/element/future_builder.rs`, `crates/flui-widgets/src/image/resolve.rs`, `crates/flui-widgets/tests/future_builder.rs`, `crates/flui-runtime/src/{realm_services.rs,ui_realm/*}`, `crates/flui-testing/src/{lib,bootstrap}.rs`, `crates/flui-testing/tests/async_driver.rs`, `crates/flui-app/src/app/runner/**`, `crates/flui-sdk/tests/surface.rs`, корневые `Cargo.toml`, `tests/thread_boundary_ui.rs`, `tests/ui/thread_boundary/`, `.config/nextest.toml`, новый ADR, `changelog.d/` | T3 по `flui-testing/src/lib.rs` | [P] с T2 | L | `cargo nextest run -p flui-scheduler -p flui-runtime -p flui-testing -p flui-view -p flui-widgets -p flui-app`; `cargo nextest run -p flui -E 'binary_id(flui::thread_boundary_ui)'`; `cargo test -p flui-view --doc`; `cargo xtask cross-typecheck`; `cargo xtask wasm-check`; `cargo xtask check-changed` | слит ≤10-17; контракт красный до, зелёный после; `AsyncDriver` с `Rc` вместо `Weak` → падает строка утёкшего драйвера; drain `TaskStore` после `resume_unwind` или не на owner → строка позднего результата (id потока); снятие защёлки после drain → `finish_async_pump_reissues_a_stranded_live_waiters_demand` | 6 | — |
| T2 | Gate `thread-boundary`: загрузчик rustdoc JSON (`rustdoc-types`, `--document-private-items`, `RUSTC_BOOTSTRAP=1`, общий с `api-closure`), вызов из `doc-strict`, `--report`, `--self-test` на 8 позиций, скан `unsafe impl` через `syn`, баны; allowlist = **сегодняшняя** поверхность в режиме храповика; замер времени джоба `doc` (25 мин) | R3, R5 | `tools/xtask/**`, `tools/xtask/allowlists/thread-boundary.toml`, `deny.toml`, `[workspace.dependencies]`, `Cargo.lock`, ADR-0089 §6 | — | [P] | L | `cargo test -p xtask`; `cargo xtask doc-strict`; `cargo xtask checks`; `cargo xtask deps` | каждая self-test fixture (supertrait; bound/where; alias; поле; синтетический auto-trait; impl-блок и blanket; приватный sealed supertrait; `-> impl Trait + Send`) роняет gate; fixture с `send_wrapper` роняет `deps` | 6 | — |
| T3 | Путь записи: `SignalWrite` несёт `FnOnce(&mut EventCx) + Send` и открывает `WriterSource` выпустившей presentation; удалены `WriteTarget for Reactive`, `BuildContext::reactive`, `BuildOwner::reactive`, `HeadlessBinding::reactive` (30 вызовов); guard `StateCell::schedule`; `on_remove` из сборки — в post-frame; вложенный `WriterSource::write` (c1); id графа `u64` + `checked_add` | R7 (2 строки), R8, R8b, R10, R11–R12 (вложенный `EventCx`) | `crates/flui-view/src/{reactive,context,owner}/**`, `crates/flui-view/tests/{writer_source.rs,ui/}`, `crates/flui-foundation/src/read_scope.rs`, `crates/flui-runtime/src/ui_realm/commands.rs`, `crates/flui-widgets/src/navigator/{local_history,modal_route}.rs`, `crates/flui-testing/src/lib.rs`, `crates/flui-testing/tests/owner_local_write_paths.rs` (новый), вызовы `.reactive()` в тестах, `changelog.d/` | — | [P] с T2, T4 | L | `cargo nextest run -p flui-view -p flui-runtime -p flui-testing -p flui-widgets`; `cargo nextest run -p flui-view -E 'test(=trybuild_ui::ui_tests)'`; `cargo test -p flui-testing --doc`; `cargo xtask check-changed` | слит ≤10-14; `on_remove` синхронно в сборке → `local_history_removal_writes_after_the_flush` теряет запись; `SignalWrite` через первую presentation → `cross_thread_signal_write_opens_the_owner_writer_source` видит запись не в том графе; `wrapping_add` счётчика → `exhausted_graph_ids_are_refused_for_good` | 4 | — |
| T4 | Ослабления bound'ов (строки 10, 22, 31, 39) | R2 (часть) | `crates/flui-widgets/src/animated/*`, `navigator/hero.rs`, `sliver_persistent_header.rs`, `crates/flui-view/src/view/render.rs`, `crates/flui-animation/src/` (tween, proxy), методы планировщика из строки 10, `changelog.d/` | T1 | [P] | L | `cargo nextest run -p flui-animation -p flui-widgets -p flui-scheduler`; `cargo xtask check-changed` | `!Send` значения в этих позициях компилируются на `main` (строки consumer T7) | 2 | — |
| T5 | C1: создание `send-flip/core`; коммит рецепта; снятие supertrait/alias (B-строки 5–8, 11–13, 15–19, 23–30, 32, 37) по `cargo xtask thread-boundary --report`, остальное — по ошибкам компилятора; контрактные строки ядра с `ignore` | R2, R3 | ~100 файлов во всех крейтах (один исполнитель) | T1, T2, T4 | — | L | `cargo xtask check-changed`; `cargo nextest run --workspace --run-ignored only -E 'test(/owner_|notifier_reentry|_during_build_is_refused/)'` — красный вывод в PR | ядро собирается; `ListenerCallback`, `StatusCallback` — `Rc`; каждая контрактная строка красная по assert | 4 | — |
| T6a | Планировщик owner-local: `UpdateScheduler` `!Send`, `frame_waker()`, один `PostFrameHandle`, `PostFrameScheduleError`, ticker; удалён `assert_impl_all!` (`post_frame.rs:304`) (строки 4–9, 35) | R4, R7, R11, R13 | `crates/flui-scheduler/**`, `crates/flui-app/src/app/runner/**`, `crates/flui-runtime/src/ui_realm/frame*.rs` | T5 | [P] | L | `cargo nextest run -p flui-scheduler -p flui-runtime -p flui-app`; `cargo xtask cross-typecheck` | строки в `post_frame_callback_ordering.rs`: post-frame под borrow очереди → `schedule` из callback даёт `BorrowMutError`; без catch вокруг callback → следующий кадр не наступает; заменённая очередь уничтожается под borrow → счётчик `Drop` ≠ 1 | 5 | — |
| T6b | Нотификаторы foundation: раунд по снимку, сильная ссылка на себя до конца раунда, первая паника — `resume_unwind` после раунда; `ViewKey`, удаление 7 alias'ов | R11, R12, R13 | `crates/flui-foundation/**` | T5 | [P] | L | `cargo nextest run -p flui-foundation`; `cargo xtask wasm-check` | таблицы в `tests/notifier.rs`: без catch раунда → второй слушатель не вызван; последняя паника вместо первой → не тот payload; обход живой карты под borrow → `BorrowMutError`; снимок уничтожается под borrow → `Drop` с `remove_listener` паникует; без сильной ссылки на себя → `Drop` до конца раунда | 3 | — |
| T6c | `AnimationController`, `Vsync` на `Rc<RefCell>`, borrow отпущен до пользовательского кода; `Simulation`; `AnimationError::MutatedDuringBuild` | R7, R8b, R11–R13 | `crates/flui-animation/**` | T5 | [P] | L | `cargo nextest run -p flui-animation` | строки в `tests/contracts/`: уведомление под borrow → подписчик читает контроллер, `BorrowError`; мутатор в `build` меняет значение → строка R8b; status-listener уничтожается под borrow при замене → `Drop` ≠ 1; паника status-listener без catch → строки R11 | 4 | — |
| T6d | `HitMetadata`, `CustomPainter` и 4 делегата без `Send`, `Rc`-слушатели `ScrollPosition`, hit-test, semantics | R2 | `crates/flui-rendering/**`, `crates/flui-objects/**`, `crates/flui-interaction/**` без `recognizers/**`, `crates/flui-semantics/**` | T5 | [P] | L | `cargo nextest run -p flui-rendering -p flui-objects -p flui-interaction -p flui-semantics`; `cargo test -p flui-objects --test render_object_harness` | `RENDER_OBJECT_TYPES` не меняется; строки R2 этих позиций (T7) компилируются | 4 | — |
| T6e | Контроллеры виджетов (без `TextEditingController`), `CupertinoTabController` на `Rc`, конструкторы `impl CustomPainter`/`impl Delegate + 'static`, отказ мутаторов в `build`; пакеты, examples, шаблоны | R7, R8b, R14 | `crates/flui-widgets/**` без `text/controller.rs`, `packages/**`, `examples/**`, `crates/flui-cli/src/templates` | T5 | [P] | L | `cargo nextest run -p flui-widgets -p flui-material -p flui-cupertino -p flui-devtools`; `cargo nextest run -p flui-cli -E 'test(/^cli_create::generated_/)'`; `cargo xtask facade-combos` | строки R8b в `flui-widgets/tests/scroll.rs`, `flui-material/tests/tabs.rs`, `flui-cupertino/tests/tab_scaffold.rs`; `TabController` уведомляет под borrow → `tab_controller_listener_writes_from_an_event_callback` падает с `BorrowError` | 4 | — |
| T7 | Закрытие: consumer R2/R4/R6/R15, trybuild R1 (3 НК + 6 РГ), строки R7 listener/status/post-frame, строка отчёта realm, целевой allowlist (`flip`, `owner-only`, `sendable`), R18, `changelog.d/send-flip.md`, ADR | R1–R7, R11, R14–R18 | `tests/facade_consumer.rs`, `tests/fixtures/thread_boundary_consumer.rs`, `tests/ui/thread_boundary/`, `crates/flui-testing/tests/{owner_local_write_paths,lifecycle_panic_containment}.rs`, allowlist, `crates/flui-rendering/ARCHITECTURE.md`, `design/`, `docs/adr/`, `crates/flui-sdk/tests/surface.rs` | T6a–T6e, T2, T3, teardown T8 | — | L | `cargo xtask check-changed`; `cargo xtask doc-strict`; `cargo xtask wasm-check`; `cargo xtask cross-typecheck`; `cargo nextest run -p flui -E 'group(trybuild) \| group(nested-cargo)'`; `cargo xtask changelog --check`; `cargo xtask docs-paths`; `rg "render.rs:451" -g '!docs/plans/**' -g '!docs/research/**'` пуст | «только `error!`» в `notify_listeners` → `listener_panic_reaches_the_realm_report` видит пустой отчёт; listener пишет через `SignalSender` → строка R7 видит запись на кадр позже; PR ядра готов к ревью | 4 | — |
| T8 | Нативный smoke Notes на SHA PR ядра | R14 | — | T7 | — | W | `cargo xtask device windows-notes` | датированный зелёный вывод в PR | 0,5 | — |
| T9 | `TextEditingController`: `Arc<Mutex>` → `Rc<RefCell>`; `with_inner_silent` (`text/controller.rs:994`) с выносом состояния | R12, R13 | `crates/flui-widgets/src/text/controller.rs` | T6e, text-ime T2a, T5 | — | L | `cargo nextest run -p flui-widgets -p flui-material` (`text_store_kit::assert_conforms`) | замыкание под borrow → `on_changed`, читающий контроллер, даёт `BorrowError`; на ядре после text-ime T5, иначе в `main` после ядра; мерж ядра не держит | 2 | — |

**Карточка T1 (ломающее, но окончательное: T6a его не переделывает).** `OwnerFrame` (`!Send`, `#[doc(hidden)]`,
поглощает `LocalPostFrameLane`; сильная ссылка — только у `RealmServices` и `HeadlessBinding`); `spawn_local` на
мёртвом `AsyncDriver` уничтожает future на owner и отдаёт отменённый `TaskToken` с `warn!`; опрос — в
`MidFrameMicrotasks` и в `pump_background` (`finish_async_pump()`, затем `poll_ready()`); безлейновые входы кадра
и `*_with_lane` удалены (~80 вызовов); drain `TaskStore` через `Option`/`ManuallyDrop`, первая паника сохраняется;
`FutureBuilder<K: Clone + PartialEq + Debug + 'static, T: 'static, E: 'static>`. Корневой trybuild
`tests/thread_boundary_ui.rs`: `[[test]]` в корневом `Cargo.toml`, член `binary_id(flui::thread_boundary_ui)` группы
`trybuild`. Новый ADR — §2, Proposed. **Контракт, красный до и зелёный после:** trybuild
`async_driver_stays_on_its_thread` (сегодня компилируется); `owner_local_future_completes_after_a_worker_wake`,
`future_builder_accepts_an_owner_local_future` (сегодня E0277); `late_completion_after_realm_drop_drops_captures_on_the_owner`;
`a_leaked_async_driver_holds_no_task_after_the_realm`; `frame_waker_wakes_the_realm_from_a_worker`. **Переносятся
без изменения утверждений** (в diff — только форма вызова входа кадра): `finish_async_pump_reissues_a_stranded_live_waiters_demand`,
`frame_scheduled_hook_fires_once_per_transition`, `lifecycle_reenable_edge_schedules_exactly_one_frame`,
`headless_wake_from_another_thread_is_polled_on_the_frame_thread`, таблица `crates/flui-scheduler/tests/wake_delivery.rs`.

## Требование → тест

| R | Тест или прогон | Задача | Вид |
|---|---|---|---|
| R1 | `tests/ui/thread_boundary/*_stays_on_its_thread`: НК `async_driver_`, `animation_controller_`, `scroll_controller_`, `cupertino_tab_controller_`; РГ `signal_`, `writer_source_`, `writer_`, `event_cx_`, `state_cell_`, `tab_controller_` | T1; T7 | trybuild |
| R2 | `owner_local_values_fit_every_flip_position` (через `flui` и через `flui-sdk`) | T7 (позиции T4–T6e) | consumer |
| R3 | `cargo xtask thread-boundary` внутри `doc-strict`; `--self-test` в `cargo test -p xtask` | T2; T7 (allowlist) | gate |
| R4 | `cross_thread_handles_stay_sendable`; `frame_waker_wakes_the_realm_from_a_worker` | T7; T1 | consumer, nextest |
| R5 | скан `unsafe impl` в `checks`; `cargo xtask deps` с банами | T2 | gate |
| R6 | `event_setters_infer_cx_through_the_facade` (РГ) | T7 | consumer |
| R7 | `owner_local_write_paths`: local history, cross-thread (T3); listener, status, post-frame (T7); строка `TabController` (T6e) | T3, T6e, T7 | nextest |
| R8 | trybuild `signal_write_through_build_context` + `ctx.reactive()`; `compile_fail`-doctest `HeadlessBinding` | T3 | trybuild, doctest |
| R8b | `state_cell_schedule_during_build_is_refused`; `controller_mutation_during_build_is_refused` (4 строки) | T3; T6c, T6e | nextest |
| R9 | `worker_write_is_visible_on_the_next_pump` (РГ); `owner_local_future_completes_after_a_worker_wake` | T1 | nextest |
| R10 | `late_completion_after_realm_drop_drops_captures_on_the_owner`; `exhausted_graph_ids_are_refused_for_good` (in-`src`) | T1; T3 | nextest |
| R11 | `owner_callback_panic_containment` (одиночный сбой, два в конкуренции, следующая операция) для listener, status, post-frame; `nested_write_panics_inside_an_event_cx`; `listener_panic_reaches_the_realm_report` | T6b, T6c, T6a; T3; T7 | nextest |
| R12 | `notifier_reentry` (6 строк); `nested_writer_source_inside_an_event_cx` | T6b, T6c; T3 | nextest |
| R13 | `owner_local_captures_drop_once_on_the_owner_thread` (замена, unmount, teardown) | T6a–T6c; T1 | nextest |
| R14 | `check-changed`; `cli_create::generated_*`; `external_notes_showcase_runs_through_the_facade`; smoke | T6e, T7; T8 | CI, W |
| R15 | `wasm_consumer_captures_rc_in_listener_and_post_frame` в `cargo xtask wasm-check`; `cargo xtask cross-typecheck` | T7 | wasm |
| R16–R18 | `cargo xtask changelog --check`; ревью ADR по списку; `cargo xtask docs-paths`, `rg "render.rs:451"` | T7 | gate, ревью |

Отклонение от design: таблица R7 разделена по владельцам файлов (`flui-testing` — без пакетов; строка
`TabController` — в `flui-material/tests/tabs.rs`), а матрицы R11–R13 — по крейтам семейств, чтобы T6x не
делили файлы.

## Владельцы общих файлов

| Файл | Владелец | Через владельца / порядок |
|---|---|---|
| `Cargo.lock`, `[workspace.dependencies]` | T2 (`rustdoc-types`) | руками не правится; на ребейзе ядра — версия `main`, затем `cargo metadata` |
| корневой `Cargo.toml` (`[[test]] thread_boundary_ui`) | T1 | фичи и `[[example]]` — persistence P1; порядок P1 → T1 |
| `deny.toml [bans]` | T2 | — |
| новый `ADR-XXXX` «UI surfaces are owner-local; one thread-boundary ledger» | T1 создаёт (§2, Proposed) | T7: §1, §3–§7, Accepted, amends ADR-0027 §2/§9; номер — оркестратор |
| ADR-0086 (Accepted + поправка §5), ADR-0091 (§2–§6 Accepted, §1 `Superseded-by`), ADR-0027 | T7 | номер ADR realm-model (D4) — оркестратор |
| ADR-0089 §6 | T2 | — |
| `.config/nextest.toml` | T1 (член группы `trybuild`) | прочие строки — teardown T9; consumer живёт в `facade_consumer` без правки |
| `crates/flui-scheduler/tests/main.rs` | T1 | ядро пишет в существующие модули |
| `crates/flui-testing/tests/main.rs` | teardown T8 | строка `mod owner_local_write_paths;` для T3 |
| `crates/flui-view/tests/main.rs`, facade `src/lib.rs` | persistence P1 | send-flip правок не вносит |
| `crates/flui-widgets/tests/main.rs`; `packages/flui-material/tests/main.rs` | focus-keyboard T1; text-ime T5 | send-flip пишет в существующие `future_builder.rs`, `scroll.rs`, `tabs.rs` |
| `crates/flui-sdk/tests/surface.rs` | T1 (`FrameWaker`), T7 (остальное) | после focus-keyboard T1 |
| `tests/facade_consumer.rs` | T7 | после persistence P1, P8 (ребейз) |
| `changelog.d/` | T1, T3, T4 — свои `<branch-slug>.md`; ядро — `send-flip.md` (T7) | строка «было → стало» на элемент контракта |
| `tools/xtask/allowlists/thread-boundary.toml` | T2 (сегодня) → T7 (цель) | PR в `main` между ними сам классифицирует своё вхождение |
| `crates/flui-testing/src/lib.rs`; `crates/flui-runtime/src/realm_services.rs` | T3 → T1; persistence P1 → T1 → teardown T8 | — |
| `crates/flui-widgets/src/text/controller.rs` | text-ime T2a → T5 → T9 | — |
| `crates/flui-runtime/src/ui_realm/{frame,pump}.rs` | T1 → text-ime T3 → T6a (ребейз ядра) → focus-keyboard T7 | — |

## Перебазирование ядра

Еженедельно (10-26, 11-02, 11-09, 11-16) ядро пересоздаётся на `origin/main`; механика повторяется рецептом,
а не переносится конфликтами. Первый коммит ядра — только рецепт, остальные — ручные (squash T5 и T6x).

```sh
git fetch origin && git switch -c send-flip/core-next origin/main
sg() { ast-grep run -l rust -U "$@" crates packages examples src tests; }
for m in add_listener add_status_listener; do sg -p "\$R.$m(Arc::new(\$F))" -r "\$R.$m(Rc::new(\$F))"; done
for m in painter foreground_painter; do sg -p "\$W.$m(Arc::new(\$P))" -r "\$W.$m(\$P)"; done
for t in Flow CustomSingleChildLayout CustomMultiChildLayout; do sg -p "$t::new(Arc::new(\$D), \$\$\$A)" -r "$t::new(\$D, \$\$\$A)"; done
sg -p '$C.local_post_frame_handle()' -r '$C.post_frame_handle()'; sg -p '$H.schedule_local($F)' -r '$H.schedule($F)'
git diff --name-only | xargs rg -l 'Rc::new' | xargs rg -L 'use std::rc::Rc|rc::\{?.*Rc' | xargs -r sed -i '0,/^use /s//use std::rc::Rc;\nuse /'
cargo fix --workspace --all-targets --allow-dirty && cargo fmt --all
git commit -am "workspace: rewrite listener and delegate call sites to owner-local values"
git cherry-pick <коммит рецепта на ядре>..send-flip/core    # ручные коммиты; конфликт — только в них
cargo xtask check-changed && cargo xtask thread-boundary --report   # новые вхождения из main → классифицировать
```

Повторный прогон рецепта на результате даёт пустой diff. **Решение оркестратора (2026-10-05): без
force-push.** Еженедельно `main` вливается в `send-flip/core` merge-коммитом; конфликты из call sites
снимаются повторным прогоном рецепта поверх merge (отдельный коммит). В `main` ядро попадает одним
squash-merge PR. Открытые T6x сливают `send-flip/core` в себя тем же способом. Ветку `send-flip/core-next`
не используем.

## Критический путь и календарь

T1 (6) → T5 (4) → T6a (5) → T7 (4) → ревью, T8, буфер (5) = **24 рабочих дня**; запас до 11-24 — 11 рабочих
дней (≈1,45 к пути). T2, T3, T4, T6b–T6e идут параллельно с непересекающимися файлами.

| Даты | Linux remote | Windows host |
|---|---|---|
| 10-06 – 10-13 | T1; T2; T3 (10-06 – 10-09, слить ≤10-14) | — |
| 10-14 – 10-16 | ревью T1, **слить ≤10-17**; T4; T5 с 10-14 на голове PR T1 | — |
| 10-19 | T5, `send-flip/core` от `main` с T1 | — |
| 10-20 – 10-26 | T6a (критический); T6b–T6e; ребейз 10-26 | — |
| 10-27 – 10-30 | T7 | — |
| 11-02 – 11-06 | ревью PR ядра; T9 после text-ime T5 (11-05 – 11-06) | T8 (~11-05) |
| ~11-09 | мерж ядра в `main` | — |

**Расхождение для оркестратора.** focus-keyboard и text-ime ждут ядро к 11-04, design — к ~11-09:
focus-keyboard T7, T9, T10, T12 сдвигаются на 3 рабочих дня (их риск 1). Предлагаю сдвинуть их календарь,
а не сжимать ревью ядра. T1 по design — к 10-14; жёсткий срок 10-17 задаёт persistence P4.

**Go/no-go 11-17.** «Go», если одновременно: PR ядра открыт и CI зелёный (включая `doc-strict`, `wasm-check`,
`cross-typecheck`, группы `trybuild` и `nested-cargo`); в PR есть вывод каждой мутации R7/R11–R13; T8 зелёный;
текст поправки ADR-0086 §5 в PR; нет блокирующих замечаний старше двух дней. Тогда окно мержа — до 11-24.
«No-go» окончателен для 0.2.0: с 11-18 зависимые (focus-keyboard T7, T9, T10, T12; facade-surface;
authoring-styles) идут на `main`. Поздний мерж ядра не делается, иначе оно столкнулось бы с ними в стабилизации.

## Отсечка 11-24

Если ядро не в `main` к концу 11-24 (no-go 11-17 или срыв после go):

1. **0.2.0 выходит без flip.** PR ядра закрывается без мержа; `send-flip/core` остаётся веткой для 0.3.
2. **Ничего не откатывается и ничего частичного не доливается.** В `main` остаются T1–T4: это согласованная
   модель. T5, отдельные T6x и T9 в `main` не идут: частичный flip запрещён (D3), а T9 без ядра сделал бы
   `!Send` один `TextEditingController`.
3. **Объявление для 0.3:** строка в `docs/ROADMAP.md` и в release notes 0.2.0 (docs-community): какие границы
   снимаются в 0.3; ADR-0086 и новый ADR остаются Proposed.
4. **Gate остаётся** храповиком на сегодняшней поверхности: до 0.3 в `main` не прибавляется ни одной
   `Send`-позиции класса flip. facade-surface снимает непереключённую поверхность.
