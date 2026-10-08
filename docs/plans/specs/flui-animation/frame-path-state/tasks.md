# frame-path-state — задачи

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Design:** [design.md](design.md); требования — [requirements.md](requirements.md)
- **База:** `main` @ `9a4daa3ed` (F1, F2 — от `main` после Q0; F3–F5 — от `send-flip/core` после T5)

## Правила

- Задача = worktree = PR; `cargo xtask check-changed` зелёный до ревью; ID задач и требований —
  только в этом каталоге. `CARGO_BUILD_JOBS=6`, `NEXTEST_TEST_THREADS=4`; бенчи — один прогон на хост.
- **Контракт первым коммитом** (F1): тесты, красные по названной причине; вывод красного прогона —
  в PR. Строки, которые сегодня виснут, идут через `child_process::run_rows` (зависание = падение).
- **Мутация** для каждой строки DoD — названная правка production-кода в изолированном checkout;
  строка обязана упасть по названной причине, вывод — в PR.
- F2 сливается **до** того, как listener-delivery (A) и controller-robustness (B) правят
  внутренности `controller/*`, `vsync.rs`, `proxy.rs`, `switch.rs`, `curved.rs`: они пишут через
  `mutate`/`commit`/`share.rs` и не добавляют `Mutex`.

## Граф

```text
Q0 ─► F1 ─► F2 ─┬─► A, B (main)
send-flip T4, T5, T6b ─┴─► F3 (= send-flip T6c, core) ─┬─► [P] F4
                                                      └─► [P] F5 ─► бенч «после» в PR ядра
```

## Задачи

| ID | Задача | Требования | Файлы | Зависит | P |
|---|---|---|---|---|---|
| F1 | Контракт и «до»: таблицы `reads_inside_every_callout`, `frame_path_reentry` (строки R5.1–R5.9, кроме требующих `!Send`), `nested_commit_is_not_overwritten`, `prop_listener_reads_see_one_commit`; `[[test]] frame_path_allocation` (`steady_state_frame_allocates_nothing`, `paint_reads_allocate_nothing`); строка `animated_opacity_paint_reads_the_cached_alpha`; бенч `frame_path` и таблица «до» | R2.2–R2.4, R3, R4.1, R5, R6, R7 | `crates/flui-animation/tests/{main.rs,frame_path.rs,frame_path_allocation.rs}`, `crates/flui-animation/Cargo.toml` (`[[test]]`, `[[bench]]`), `crates/flui-animation/benches/frame_path.rs`, `crates/flui-objects/tests/render_object_harness.rs` | Q0 | — |
| F2 | Форма хранения без flip: приватный `share.rs`; `ControllerCore` с `mutate`/`commit`/`Published`; обёртки копируют указатель родителя и отпускают guard до вызова; обход детей `Vsync` курсором без `Vec`; `walk_probe`/`has_running` читают `Published` | R2.2, R2.3, R4.1–R4.3 | `crates/flui-animation/src/{share.rs,controller/*,vsync.rs,proxy.rs,switch.rs,curved.rs,animation.rs,lib.rs}` (строка `mod share`) | F1 | — |
| F3 | Flip (текст send-flip T6c заменяется этой задачей): `share.rs` → `Rc`/`RefCell`/`Cell`; `Vsync`, `VsyncRegistration` — `rc`; `ParentSubscription` — `Cell<Option<Box<dyn FnOnce()>>>`; `animate_to_curved(curve: impl Curve + 'static)` (строка 21); `AnimationError::MutatedDuringBuild` из send-flip без изменений; модуль `thread_affinity`, `owner_local_parent_composes`, оставшиеся строки R5 | R1, R2.1, R3, R5, R8.1 | `crates/flui-animation/**` | F2, send-flip T4, T5, T6b; желательно B (удалён `Ticker`) | — |
| F4 | Render objects: кэш alpha `Rc<Cell<u8>>`, ложный комментарий о другом потоке удалён | R7 | `crates/flui-objects/src/proxy/animated_opacity.rs`, `crates/flui-objects/src/sliver/sliver_animated_opacity.rs` (по согласованию с send-flip T6d) | F3 | [P] |
| F5 | ADR (номер — оркестратор), поправки ADR-0064 §4 и ADR-0125; rustdoc-контракт `Animation::value/status`, `velocity`; удалён абзац-исключение `controller.rs:227-230`; «Thread Safety» в `README.md` и `docs/ARCHITECTURE.md`; таблица `docs/PERFORMANCE.md` из бенча; фрагмент `changelog.d` | R6, R8.2 | `docs/adr/`, `crates/flui-animation/{README.md,docs/*,src/animation.rs,src/controller/*}`, `changelog.d/` | F3 | [P] |

## Проверка и Definition of Done

**F1.** `cargo nextest run -p flui-animation`; `cargo test -p flui-objects --test render_object_harness`;
`cargo bench -p flui-animation --bench frame_path -- --save-baseline before`. Красные по причине:
`custom_train_reads_switch` — зависание (`switch.rs:444-451`); `steady_state_frame_allocates_nothing`
— ненулевой счёт (`vsync.rs:352-356`, `:442-450`). Остальные строки зелёные и помечены в PR как guard
с мутацией из requirements. Таблица «до» (1 000 / 10 000) — в PR.

**F2.** Те же команды; `cargo xtask check-changed`. Строки F1 зелёные, кроме требующих `!Send`.
Мутации: вернуть `collect::<Vec<_>>()` детей → `steady_state_frame_allocates_nothing` красный;
читать родителя Switch под guard → `custom_train_reads_switch` виснет; вызвать кривую внутри
`mutate` → строки `Curve::transform` в `reads_inside_every_callout` виснут (F2) / `BorrowMutError` (F3). Поведение
прочих тестов крейта не меняется (без правки assert).

**F3.** `cargo nextest run -p flui-animation -p flui-objects -p flui-widgets`;
`cargo test -p flui-animation --doc`; `cargo xtask wasm-check`; `cargo xtask check-changed`.
Красный первый коммит: `assert_not_impl_any!` в `thread_affinity` не компилируется на `Arc`-хранении;
`owner_local_parent_composes` — E0277 (родитель `!Send`). Мутации: раздача внутри `mutate`
→ `restart_from_listener` с `BorrowMutError`; `commit` после раздачи → `nested_commit_is_not_overwritten`
красный; `generation.wrapping_add` → `run_generation_exhaustion_is_permanent` красный;
`Debug` через `borrow()` → строка `debug_inside_listener` паникует. `rg 'Arc<dyn (Animation|Simulation)'
crates packages examples src` пуст.

**F4.** `cargo test -p flui-objects --test render_object_harness`; мутация: `paint_effects` читает
`animation.value()` → `animated_opacity_paint_reads_the_cached_alpha` красный. `RENDER_OBJECT_TYPES`
не меняется.

**F5.** `cargo xtask checks` (docs-paths, docs-links, markers, changelog);
`cargo bench -p flui-animation --bench frame_path -- --baseline before` на том же хосте, что F1:
каждая строка «после» быстрее, таблица в PR и в `PERFORMANCE.md`; `frame_path_allocation` — 0.

## Карточка для [../tasks.md](../tasks.md)

После F3: убрать data-plane shuttles navigator (`transition_route.rs` `pending_statuses`,
`hero_flight.rs` `settled_status`) — слушатель может захватывать owner-local состояние. Меняет
момент применения статуса (сейчас — в build), поэтому отдельная задача с тестом порядка в
`crates/flui-widgets/tests/transition_route.rs`.

## Решения владельца

1. F3 исполняется как send-flip T6c в ядре: исправление доходит до `main` со слиянием ядра
   (~11-09); промежуточных атомиков в `main` нет.
2. A и B ждут слияния F2 (оценка 2–3 дня) перед правкой внутренностей контроллера и обёрток.
