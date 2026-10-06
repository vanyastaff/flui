# text-ime — задачи (уровень 1)

- **Статус:** черновик
- **Дата:** 2026-10-05
- **Design:** [design.md](design.md) (W1–W11, спайк, график); [requirements.md](requirements.md);
  уровень 0 — [../release/tasks.md](../release/tasks.md). `teardown/tasks.md` ещё нет: порядок
  закрытия и владелец lifecycle-контракта — `teardown/design.md` («Работы» 2, 6, 10).
- **Порядок работы:** ветка и worktree на задачу (`cargo xtask worktree new ime/<slug>`), один PR,
  `cargo xtask check-changed` зелёный. **A** — Windows-хост, критический путь; **B** — удалённая
  Linux-сессия (cfg-free и headless); **C** — нативные прогоны. Win32 в CI только clippy:
  Win32-строки — датированный локальный прогон, вывод в PR. ID R/F/D/T — только в этом
  каталоге, не в коде, именах тестов и коммитах.

## Решение: заморозка контракта

Контракт делится по тому, что проверяет спайк. **T2a (store)** — `settle` в `LockArbiter`, канал
отказа `CommitGate`, `committed_text`, поколение контроллера — от TSF не зависит и идёт
**параллельно T1** с 10-06: его ждёт порядок с `send-flip` (слияние ≤10-17). **T2b (хост)** —
`TextStoreHost` через `OwnerPlatform`/`HostWindow`, `TextInputBackend`, `CompositionEnd` —
**следует за первой неделей T1** (черновик ADR с 10-13, слияние после PASS пунктов (1) и (7)):
форма `Abandoned` и очередь по `entry_depth` — ровно то, что спайк проверяет, и заморозка до
них означала бы вторую. Тесты первыми: первый коммит PR — сигнатуры с инертными телами (no-op;
`todo!`/`unimplemented!` запрещены clippy) и тесты, красные по assertion или stderr
`compile_fail`; вывод красного прогона — в PR.

## Граф зависимостей

```text
          10-06        10-16    10-23              11-04  11-09      11-20 go/no-go   12-15 RC
A  T1 спайк ──────────▶ T2b ───▶ T6 Win32 (test-first) ───────────▶ │ ─▶ T9 отложенное ─▶ (T10)
   (жёстко 10-26)  │      │                                         │
                   │      └──────────────┐                          │
B  T2a store ──────┼▶ T4 tsf_acp ────────┴▶ T3 runtime ‖ T5 виджет ─▶ T7 Notes ─▶ (T8, если нет C)
   (≤10-17)        │  (после mapping-PR T1)  (≤11-04, send-flip)  (≤11-10)
C                  └─────────── классы окон IME ──▶ T8 device windows-ime ─────▶ T10 R24
T4 → T6. Внешние: send-flip (T2a ≤10-17, T5 ≤11-04, затем переписывание контроллера); teardown
2, 6 → T3; T5 → teardown 10 (R21); persistence R3 → строка R8 в T7; T4 → focus-keyboard R9.
```

## Задачи

| ID | Работа (design) | Требования | Исп. | Зависит | [P] | Нед | Статус |
|---|---|---|---|---|---|---|---|
| T1 | спайк TSF (W1) | R12, R16–R18 (доказательство), D3, D5 | A | VM владельца | ‖ T2a | 1,5 | — |
| T2a | контракт store, поправка ADR-0090 (W3) | D2, R9-основа, F2, F3, п. 7 | B | — | ‖ T1, T2b | 1,25 | — |
| T2b | контракт хоста, узкий ADR (W2) | D1, R17 (виджет), F1, F5, F7 | A | T1 п. (1),(7) | — | 1,0 | — |
| T3 | runtime: долг якоря, gate, хуки (W4) | R6, F5, F8, п. 10 | B | T2b; teardown 2, 6 | ‖ T5, T6 | 0,75 | — |
| T4 | `shared::tsf_acp`, `VK_PROCESSKEY` (W6a) | R11, R14 (Linux), R18 (Linux) | B | mapping-PR T1 | ‖ T2b | 0,5 | — |
| T5 | виджет: подтверждение композиции (W5) | R4, R6, R8, R9, R10, R15, F1, F2, F3 | B | T2a, T2b | ‖ T3, T6 | 1,0 | — |
| T6 | Win32 production (W6) | R2, R16–R18, R21, F1, F3, F5, F7, F8 | A | T1, T2b, T4 | ‖ T3, T5, T8 | 2,4 | — |
| T7 | строки Notes + consumer (W8) | R1, R3, R4, R6, R8, R23 | B | T3, T5 | — | 0,75 | — |
| T8 | `cargo xtask device windows-ime`, шаг R22 (W9) | R7, R22, R24 (инструмент) | C (B после 11-20) | T1 (классы окон) | ‖ T6 | 1,5 | — |
| T9 | после go/no-go (W10) | R13, R14 (Win32), R19, R20, F4, F6 | A | T6, go | — | 0,7 | — |
| T10 | нативный прогон R24, `docs/BETA.md` (W11) | R7, R22, R24 | C (A) | все, RC | — | 0,5 | — |

Итого ≈ 11,85 инж.-нед. Тесты задачи — её строки в «Требование → тест»; «готово» для фикса —
и прогон с откатом production-ханка в отдельном worktree: строка падает по названной причине.

### T1 — спайк (эксперимент), A, 10-06 → 10-16, жёстко 10-26
- **Объём.** Ветка `ime/tsf-spike`, без merge до ADR. Настоящий модуль
  `crates/flui-platform/src/platforms/windows/text_services/` (активация, пустой документ,
  `Document` на фокус, `TsfStore`: `RequestLock`, чтения, `SetText`, `InsertTextAtSelection`,
  `GetTextExt`/`GetScreenExt`/`GetWnd`, `AdviseSink`, composition sink, `SessionSlot`,
  `com_entry`, очередь по `entry_depth`) и `range_rect_to_screen` в `shared/text_geometry.rs`.
  Выбрасываемый зонд **≤150 строк**: `#[ignore]`-тест открывает окно `WindowsPlatform` и
  подключает store с запаздывающей раскладкой (обёртка над `InMemoryTextStore`: `NoLayout` для
  текста текущей сессии, раскладка по тику); поле смещено от начала клиентской области.
- **Файлы:** `platforms/windows/text_services/**`, `shared/{text_geometry.rs,mod.rs}`,
  `crates/flui-platform/Cargo.toml` (`Win32_UI_TextServices`), `tests/{text_input_mapping.rs,main.rs}`.
- **PASS — все пункты, каждый может упасть** (VM, MS-IME ja-JP новый и «previous version», 100 %
  и 150 %): (1) `Activate` → `S_OK`, `TfClientId`; явный `SetFocus` меняет `GetFocus` менеджера;
  (2) `toukyou`, Space, Enter → в store `東京`, композиции нет; журнал `RequestLock` →
  `OnLockGranted` → `SetText`/`OnStartComposition`…`OnEndComposition`; (3) при закрытом gate —
  `TS_S_ASYNC`, после якоря текст тот же, без потерь и дублей; (4) классы окон кандидатов и
  подсказок (`TextInputHost.exe` XAML, старый IME) по перечислению top-level окон; rect окна
  пересекает полосу ±1 строка под rect `GetTextExt` на первом нажатии и после Space; xcap;
  (5) реакция на `TS_E_NOLAYOUT` записана; (6) приходит ли `VK_PROCESSKEY`, нужен ли
  `ITfKeystrokeMgr`; (7) `TerminateComposition` при открытом gate — `Committed`, при закрытом —
  `Abandoned`; (8) ни одного abort, `#[implement]` с `Rc`-полями компилируется.
- **Отрицательные контроли:** IME выключен — гейт (4) окна не находит; rect `GetTextExt` умышленно
  сдвинут — гейт (4) падает. **FAIL:** нет (1)–(4) к 10-26 → эскалация с журналом; D3 — 11-20.
- **Выживает:** `text_services` (база T6), `range_rect_to_screen` с таблицей, классы окон IME
  (в T8). В конце T1 — **mapping-PR** только с `text_geometry` и строками R12 (Linux).
- **Проверка:** `cargo test -p flui-platform --lib text_services -- --ignored --nocapture` (VM);
  `cargo nextest run -p flui-platform text_input_mapping`; `cargo xtask cross-typecheck`.
- **Готово:** отчёт по (1)–(8) и контролям (вывод, скриншоты, сборка Windows, версия IME) в issue
  фичи; mapping-PR слит; строка R12 падает при усечении вместо `floor`.

### T2a — контракт store, B, 10-06 → 10-16, слить ≤10-17
- **Файлы:** `crates/flui-platform-api/src/text_store/lock.rs` и файл `CommitGate` (`settle`,
  `defer_failure`/`take_failure`); `crates/flui-widgets/src/text/{controller.rs,text_store.rs}`
  (`committed_text`, поколение), `editable_text.rs` (только `report_if_changed`);
  `crates/flui-widgets/src/form/form_field.rs`; `packages/flui-material/src/text_form_field.rs`;
  `crates/flui-testing` `text_store_kit` (v2); поправки ADR-0090 и ADR-0030 §6; `changelog.d/`.
- **Проверка:** `cargo nextest run -p flui-platform-api -p flui-widgets -p flui-testing -p flui-material`;
  `cargo xtask check-changed`; `cargo xtask checks`. **Готово:** строки красные на коммите
  контракта (`on_changed` под `Held`, preedit в `on_changed`); кейсы v1 kit не изменены.

### T2b — контракт хоста, A, черновик с 10-13, слить ≤10-23
- **Файлы:** `crates/flui-platform-api/src/text_store/{host.rs,mod.rs}`;
  `crates/flui-platform/src/traits/owner.rs` (`text_store_host`, запечатанный метод `HostWindow`
  с токеном); `crates/flui-interaction/src/text_input.rs` (`TextInputBackend`,
  `complete_composition`, удаление `active_store`); `crates/flui-runtime/src/presentation.rs`;
  `crates/flui-app/src/app/runner/` (`presentation_window`); `crates/flui-testing/src/{realm.rs,widgets/harness.rs}`
  (записывающий хост, `store_host_calls`); новый ADR; `changelog.d/`.
- **Тесты сверх таблицы:** `compile_fail`-doctest — метод `HostWindow` не вызвать без
  `OwnerPlatform`; сценарий #1052 строкой в `crates/flui-interaction/tests/text_input_retirement.rs`.
- **Проверка:** `cargo nextest run -p flui-interaction -p flui-widgets -p flui-testing -p flui-runtime -p flui-app`;
  `cargo test --doc -p flui-platform`; `cargo xtask check-changed`; `cargo xtask cross-typecheck`.
  **Готово:** ADR принят, номер назначен; Win32 пока отвечает `None`; T1 PASS по (1) и (7).

### T3 — runtime, B, 10-23 → 10-29
- **Файлы:** `crates/flui-runtime/src/ui_realm/{frame.rs,pump.rs,input.rs}` и close-путь;
  таблица в `ui_realm/tests/presentation_text_input.rs`.
- **Внешнее:** `CloseReason`/guard (teardown, работа 2), доставка закрытия (работа 6). Если не
  слиты к 10-23 — хук закрытия отдельным PR после них; pointer-down их не ждёт.
- **Проверка:** `cargo nextest run -p flui-runtime`; `cargo xtask check-changed`. **Готово:** F8,
  F5 (runtime) и строка «realm извлечён» падают с откатом.

### T4 — `shared::tsf_acp` и `VK_PROCESSKEY`, B, 10-16 → 10-21
- **Файлы:** `crates/flui-platform/src/shared/{tsf_acp.rs,keys.rs,mod.rs}`; строки в
  `crates/flui-platform/tests/text_input_mapping.rs` (после mapping-PR T1).
- **Проверка:** `cargo nextest run -p flui-platform text_input_mapping`; `cargo xtask check-changed`.
  **Готово:** без типов `windows`; строки красные на `main` (`Unidentified`, `S_OK`).

### T5 — виджет: подтверждение композиции, B, 10-23 → 11-04
- **Файлы:** `crates/flui-widgets/src/text/editable_text.rs` (blur и dispose: `take()` отдельным
  оператором; paste/undo; unmount-страховка; путь `Abandoned`); `crates/flui-widgets/tests/editable_text.rs`
  (модуль `text_store`); `packages/flui-material/tests/` (+ `mod` в `main.rs`).
- **Проверка:** `cargo nextest run -p flui-widgets -p flui-material`; `cargo xtask check-changed`.
  **Готово:** слит ≤11-04 (затем переписывание `send-flip`); F2, R8, R10 падают с откатом.

### T6 — Win32 production (test-first), A, 10-23 → 11-09
- **Файлы:** `platforms/windows/text_services/**` (из T1, таблица
  `the_text_services_bridge_honours_the_tsf_contract` в `text_services/tests.rs`: mock-sink
  `ITextStoreACPSink`, mock `TsfThread`); `platforms/windows/platform.rs` (`WindowContext`,
  `WM_DESTROY`, `VK_PROCESSKEY`; ≤3000 строк); `crates/flui-platform/Cargo.toml`; при
  необходимости корневой `Cargo.toml`, `Cargo.lock`; `changelog.d/`.
- **Проверка:** `cargo nextest run -p flui-platform` на Windows (вывод в PR); `cargo xtask cross-typecheck`;
  `cargo xtask check-changed`; ручной `toukyou` в Notes на VM.
- **Готово:** строки написаны до кода и красные на коммите контракта; F7 и
  `on_changed_unfocus_inside_request_lock` падают с откатом; `windows::*` не выходит из крейта.

### T7 — Notes и consumer, B, 11-04 → 11-10
- **Файлы:** `tests/fixtures/notes_flow.rs`, `examples/two_screens/` (черновик читает
  `committed_text`); consumer `external_notes_showcase_runs_through_the_facade`.
- **Внешнее:** черновик persistence R3 (без него строка R8 проверяет состояние формы).
  **Проверка:** `cargo nextest run -p flui notes`; `cargo xtask check-changed`. **Готово:** R3,
  R6, R8 падают с откатом своих ханков (T5, T3).

### T8 — `cargo xtask device windows-ime`, C (или B после 11-20), 1,5 нед
- **Файлы:** `tools/xtask/src/device/{windows_ime.rs,windows_notes.rs,plan.rs,mod.rs}`,
  `tools/xtask/Cargo.toml`, `Cargo.lock`. Шаги — R24 (классы окон из T1, ±1 строка, у нижнего
  края — над полем, xcap), `alt_tab_mid_composition`, шаг R22 в `windows-notes`; exit 2 без языка.
- **Проверка:** `cargo xtask device windows-ime` на VM; отрицательный контроль на `4915054c8`;
  `cargo xtask cross-typecheck`. **Готово:** обе стороны контроля в PR; #1065 закрыт после R22.

### T9 — после go/no-go, A, 11-20 → 11-27 (только при «go»)
`text_services/**`, `shared/tsf_acp.rs`, `editable_text.rs` (`layout_changed`, часть #1091, после `send-flip`); проверка как T6.

### T10 — нативный прогон R24, C (A), 12-15 → 12-19
- `device windows-ime` и `windows-notes` на SHA RC (VM ja-JP обязательно, zh-CN и «previous
  version» желательно; хост 100 %, 150 %, второй монитор) → запись в `docs/BETA.md` (дата, SHA,
  сборка, версия IME, вывод; «no IME» снимается). Exit 2 — «не проверено», не PASS.

## Требование → тест

| R/F | Тест | Уровень | Задача |
|---|---|---|---|
| R1 | `ime_preedit_is_underlined_at_the_caret_and_hides_it_on_request` | Notes | T7 |
| R2 | `a_composition_update_replaces_the_whole_preedit_and_places_the_caret` | Win32-unit | T6 |
| R3 | `ime_commit_inserts_the_conversion_and_notifies_once` | Notes | T7 |
| R4 | `cancelled_composition_keeps_the_committed_text_without_on_changed` (+ строка Notes) | виджет | T5, T7 |
| R5 | `composition_over_a_selection_replaces_the_selection` | kit v2 | T2a |
| R6 | `pointer_down_mid_composition_commits_before_the_save_handler`; `pointer_down_inside_the_composing_field_commits` | Notes; виджет | T7, T5 (хук T3) |
| R7 | шаг `alt_tab_mid_composition` | нативный | T8, T10 |
| R8 | `route_change_mid_composition_commits_the_preedit_into_the_draft`; `a_conversion_queued_for_an_unmounted_field_is_dropped` | Notes; виджет | T7, T5 |
| R9 | `autovalidation_ignores_a_whitespace_preedit_until_commit` | material | T5 (основа T2a) |
| R10 | `paste_and_undo_mid_composition_commit_first`; `a_refused_termination_still_commits_before_paste`; `the_enter_that_commits_does_not_submit` | виджет | T5 |
| R11 | `a_processkey_keydown_produces_no_key` | Linux | T4 |
| R12 | `screen_rects_follow_scale_and_round_outwards` | Linux | T1 |
| R13 | `scroll_move_and_dpi_change_send_layout_change_before_the_next_query` | Win32-unit | T9 |
| R14 | `split_surrogate_offsets_are_invalid_positions`; `astral_and_cluster_commits_count_utf16` | Linux; Win32-unit | T4; T9 |
| R15 | `rtl_commit_in_an_ltr_field_keeps_logical_order_and_a_rect` | виджет | T5 |
| R16 | `a_window_activates_the_thread_manager_and_a_focused_field_gets_a_document` | Win32-unit | T6 |
| R17 | `field_focus_moves_the_tsf_document_focus`; `focus_gain_and_loss_reach_the_store_host` | Win32-unit; виджет | T6; T2b |
| R18 | `the_bridge_reproduces_the_kit_conversion_script`; `reads_outside_a_lock_answer_no_lock`; `a_lock_inside_the_frame_answers_async`; `lock_flags_and_errors_translate_to_tsf_codes` | Win32-unit; Linux | T6; T4 |
| R19 | `ime_window_messages_reach_the_default_procedure` | Win32-unit | T9 |
| R20 | `reconversion_requests_leave_the_document_unchanged`; `query_only_insert_answers_from_the_selection` | Win32-unit; Linux | T9 |
| R21 | `failed_activation_falls_back_to_wm_char_and_warns_once` | Win32-unit | T6 |
| R22 | шаг `title_copy_paste_round_trips_through_the_clipboard` | нативный | T8, T10 |
| R23 | строки R1, R3, R4, R6, R8 + `flui::facade_consumer` | Notes | T7 |
| R24 | `cargo xtask device windows-ime` + отрицательный контроль на `4915054c8` | нативный | T8, T10 |
| F1 | `a_grant_for_a_replaced_document_answers_unexpected` | Win32-unit + виджет | T6, T5 |
| F2 | `on_changed_runs_after_the_lock_is_released`; `set_text_from_on_changed_lands_after_the_commit` | виджет | T2a; T5 |
| F3 | `a_panicking_on_changed_keeps_the_commit_and_reports_through_the_realm`; `a_panic_after_the_grant_returns_ok_and_the_queue_drains` | виджет; Win32-unit | T5; T6 |
| F4 | `a_full_deferred_queue_answers_fail_and_recovers_after_the_anchor` | Win32-unit | T9 |
| F5 | `closing_the_window_mid_composition_releases_tsf_objects_in_order`; `an_accepted_close_commits_the_preedit_before_the_guard` | Win32-unit; runtime | T6; T3 |
| F6 | масштаб в строках `screen_rects_follow_scale_and_round_outwards` + строка DPI в R13 | Linux; Win32-unit | T1; T9 |
| F7 | `tsf_reentry_from_terminate_composition_commits_once`; `on_changed_unfocus_inside_request_lock`; `a_nested_slot_entry_answers_unexpected` | Win32-unit | T6 |
| F8 | `a_grant_deferred_by_a_panicking_frame_runs_without_another_frame` | runtime | T3 |

Сверх R/F (design): `an_app_edit_during_a_lock_is_not_overwritten` (T2a), `tsf_entries_while_the_realm_is_checked_out_run_no_user_code` (T3), `a_poisoned_document_is_replaced_then_falls_back_after_three` (T6).

## Владельцы общих файлов

| Файл | Владелец | Правило |
|---|---|---|
| `crates/flui-platform/Cargo.toml`, фича `windows` `Win32_UI_TextServices` | T1 (ветка) → сливает T6 | Фичи `windows` задаются в манифесте крейта, корень держит только версию `0.62` |
| `Win32_UI_Input_Ime` | никто | **Не включается** (R19, design «Клавиши»); если спайк (6) потребует — правка design, решение владельца |
| корневой `Cargo.toml` `[workspace.dependencies]`; `Cargo.lock` | T6 (если `#[implement]` потребует `windows-core`), T8 (xcap, перечисление окон) | `cargo xtask deps` зелёный; `Cargo.lock` только регенерирует cargo, при конфликте rebase — пересборка |
| `docs/adr/ADR-XXXX-*` (новый узкий ADR) | T2b | Номер назначает оркестратор (индекса нет, уникальность — `cargo xtask workspace`) |
| `docs/adr/ADR-0090-*`, `ADR-0030-*` | T2a | Поправка §1–§3; `Superseded-by` в ADR-0030 §6 |
| `crates/flui-platform/tests/main.rs`, `src/shared/mod.rs` | T1 (mapping-PR) → T4 | Одна строка `mod text_input_mapping;`; T4 и T9 только добавляют строки таблицы |
| `packages/flui-material/tests/main.rs` | T5 | Одна строка `mod` |
| `crates/flui-platform-api/src/text_store/mod.rs` | T2b | T2a трогает только `lock.rs`/файл gate |
| `.config/nextest.toml` | не меняется | `compile_fail`-doctest вместо нового trybuild-бинаря; `#[ignore]`-зонд в конфиг не входит |
| `changelog.d/` | T2a, T2b, T6 | Каждый — свой `<branch-slug>.md`; `CHANGELOG.md` не трогается |
| `docs/BETA.md` | T10 | Одна датированная запись |

## Критический путь и календарь

| Даты | A | B | C |
|---|---|---|---|
| 10-06 – 10-16 | T1 (цель 10-16, **жёстко 10-26**) | T2a (слить ≤10-17) | VM готова к 10-08 |
| 10-13 – 10-23 | T2b (черновик ADR с 10-13) | T4 (10-16 – 10-21, после mapping-PR) | — |
| 10-23 – 11-09 | T6 | T3 ‖ T5 (10-23 – 11-04), T7 (11-04 – 11-10) | T8 (если есть C) |
| 11-09 – 11-20 | ручной `toukyou`, правки; **11-20 go/no-go** | правки | — |
| 11-20 – 12-19 | T9 (до 11-27) | T8 (если нет C) до ~12-01 | T10 на SHA RC, 12-15 – 12-19 |

Критический путь: T1 → T2b → T6, 10-06 → ~11-09 (4,9 нед), запас ≈1,7 нед до 11-20.
**Go/no-go 2026-11-20 (D3):** зелёные R2, R3, R16, R17, R18 (Win32-строки — датированный прогон
на Windows, R3 — Notes в CI) и датированный ручной прогон `toukyou` → `東京` в Notes на VM с
Microsoft IME ja-JP. **No-go:** владелец выбирает между scope и датой; запасной вариант — релиз
с «no IME» и проверенной деградацией R21 (строка в T6 готова к 11-09 при любом исходе).

**Риски по датам.** (1) Спайк на жёстком сроке 10-26 оставляет T2b + T6 (3,4 нед) ровно до
11-20, без запаса; FAIL (1)–(4) → D3 на 11-20 без IME. (2) `send-flip`: T2a ≤10-17, T5 ≤11-04,
отсечка `send-flip` 11-24; если контроллер переписан раньше — перенос T2a/T5, +0,5 нед B.
(3) Хук закрытия T3 ждёт teardown 2 и 6; teardown 10 ждёт T5. (4) B загружен ≈4,25 нед до
11-10; без третьего исполнителя T8 идёт 11-20 → ~12-01: к RC успевает, к go/no-go — ручной
прогон. (5) Win32 в CI только clippy; потеря VM или хоста останавливает T1, T6, T10.

## Предусловия владельца

- **К 10-08 (до пункта (2) спайка):** Hyper-V VM Windows 11 со снимком; Microsoft IME ja-JP
  (новый и «Use previous version of Microsoft IME»), zh-CN Pinyin; тулчейн по `rust-toolchain.toml`.
- **К 10-23:** принять узкий ADR (T2b) и поправку ADR-0090 (T2a); подтвердить новую формулировку
  R8 (программный unmount подтверждает preedit, design Open Q2). **К 11-09:** третий исполнитель
  для T8. **11-20:** go/no-go. **К T10:** хост 100 %/150 % и второй монитор с другим DPI (D4).
