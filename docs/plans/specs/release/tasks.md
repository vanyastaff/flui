# Первый релиз FLUI на crates.io — статус

- **Статус:** черновик вместе с [requirements.md](requirements.md). Даты нет: релиз ждёт ворот
  готовности. Из пяти ворот не пройдено ни одно; рендер, цена контрола и имена крейтов в работе,
  снимок API и тестовый API авторов не начаты.
- **Сверено с:** `main` @ `e13b7ce` (2026-11-12): спеки фич, [`docs/gaps.md`](../../../gaps.md),
  статусы ADR, открытые issue `priority: critical` и `priority: high`. Сверка чтением кода, не
  запуском.

Статусы: `не начато` → `в работе` → `ждёт прогона` (код и headless-тесты на main, остался
нативный прогон) → `готово`. Работа без своей спеки ведётся тредом проекта, который назван в
столбце «Кто ведёт»; спека фичи ведёт свои задачи сама, здесь только её итог.

## Ворота готовности

| Ворота | Состояние на main | Статус |
|---|---|---|
| Рендер (render-scaling G1–G7) | [render-scaling/tasks.md](../render-scaling/tasks.md): сделаны RS1, RS3–RS8, RS14, RS15a, RS15c1, RS15c2; дальше RS8c, RS19, RS9, RS16, RS15b. У ADR-0304, ADR-0305 и ADR-0306 остаются пункты в `docs/gaps.md`: движок рисует версию сцены целиком, ни один runner не вызывает `GpuContext::bootstrap`, команды хранят свою `Matrix4` | в работе |
| Цена контрола | `RawButton` и `InkWell` на узле ADR-0322; `RawButton` поверх `Text` добавляет 2 элемента, 1 узел рендера, 35 аллокаций и 4630 байт (`perf_raw_button_over_text`). Остальные контролы Material и все Cupertino на обёртках; цель записи — 3 КБ | в работе |
| Снимок публичного API (R3) | `cargo xtask api-closure` проверяет чужие типы в сигнатурах Stable-крейтов только с фичами по умолчанию на хосте (ADR-0089 §6); снимка поверхности нет, фасад реэкспортирует внутренние крейты целиком | не начато |
| Тестовый API авторов (R19) | `src/testing.rs` отдаёт `HeadlessBinding` (колонка «Not» в `AGENTS.md`); живут четыре драйвера: `HeadlessBinding`, `widgets::lay_out`, `widgets::harness::mount`, `HeadlessHost`. Тест шаблона `flui create` (`crates/flui-cli/src/templates/counter.rs`) считает координаты кнопки сам | не начато |
| Имена крейтов (R21) | на crates.io занято одно имя, `flui-cli`; публикуемые манифесты — `rg -l '^publish = \["crates-io"\]' --glob '**/Cargo.toml'`. Заглушки публикуются с машины владельца после того, как он подтвердит список | в работе |

## Требования

| Требование | Состояние на main | Что осталось | Кто ведёт | Статус |
|---|---|---|---|---|
| R1 consumer по реестру | `acceptance_app_runs_through_the_facade` и `external_consumers_extend_and_test_through_the_facade` (`tests/facade_consumer.rs`) собирают внешний проект по path | staging-реестр из `.crate`-файлов и consumer по нему | конвейер публикации, после R2 | не начато |
| R2 публикация | `flui-native` собирается на опубликованных `accesskit_windows` 0.35.1, `accesskit_ios` 0.2.2, `accesskit_android` 0.9.1, `third_party/` нет; чего в них нет, отвечает отказом: курсор скринридера на iOS и Android `AssistiveFocusSupport::Unreported` (`each_native_adapter_says_whether_it_reports_the_assistive_cursor`), нет `ScrollPattern` на Windows и `viewIdResourceName` на Android ([gaps](../../../gaps.md#accessibility-bridge-fidelity-properties-and-announcements)); `cargo package` не проверяется | запросы в AccessKit (подаёт владелец, срок 2026-12-01); проверка `cargo package` всех публикуемых крейтов в порядке зависимостей | тред «Подготовить публикацию `flui-native` без форков» | в работе |
| R3 снимок API | см. ворота | снимок поверхности фасада в CI; корни крейтов реэкспортируют выбранные имена; `api-closure` по фичам; `pub unsafe` конструкторы ID (`crates/flui-foundation/src/id.rs`) не видны через фасад | тред «Снять снимок публичного API» | не начато |
| R4 граф по умолчанию | `ordinary_facade_graph_excludes_test_support` есть; `crates/flui-app/Cargo.toml` `default = ["desktop", "http"]`, у фасада фичи `http` нет, поэтому на Linux в граф попадают `ureq`, `rustls`, `aws-lc-sys` | фича `http` фасада выключена по умолчанию; HTTP и TLS в запрещённом списке теста; одна версия `zbus` | тред «Убрать лишнее из графа и фич» | не начато |
| R5, R6 persistence | [persistence/tasks.md](../persistence/tasks.md): P1–P6, P10, P11 | P12, нативный прогон через `flui-driver` FD11 | спека `persistence` | ждёт прогона |
| R7 IME | [text-ime/tasks.md](../text-ime/tasks.md): T1–T6, T9 | T11 (строки IME на приложении приёмки), T10 (нативное доказательство после FD11) | спека `text-ime` | в работе |
| R8 клавиатура | [focus-keyboard/tasks.md](../focus-keyboard/tasks.md): T1, T3, T5–T8, T10 | T2, T4, T9, T11–T13, T17 в PR; T14–T16; T15 ждёт FD11 | спека `focus-keyboard` | в работе |
| R9 readback | readback есть только у композитного слоя (`tests/composited_layer_update_readback.rs`); экраны приложения приёмки не сверяются | readback экранов приёмки через путь окна; файлы движка заняты render-scaling, поэтому после ворот рендера | после ворот рендера | не начато |
| R10, R11 teardown и паники | [teardown/tasks.md](../teardown/tasks.md): всё, кроме T19 | T19, нативный прогон через лейн `os` драйвера (FD11) | спека `teardown` | ждёт прогона |
| R12 скринридер | дерево доступности публикуется без фичи на трёх десктопах; датированного прогона Narrator нет | прогон Narrator на Windows | владелец (устройство), сценарий драйвера | ждёт прогона |
| R13 бюджеты | гейты `cargo xtask perf --check` держат аллокации, байты и узлы; бюджетов времени кадра, памяти и старта на эталонной машине нет | замер и публикация бюджетов, профиль фаз кадра | тред perf-гейтов, после ворот рендера | не начато |
| R14 платформы | Windows: нативные строки идут через `flui-driver` (FD11 не сделан; `cargo xtask device` удалён); Linux: нативный хост, CI; статусы — [`platform-parity.md`](../../../platform-parity.md) | нативный прогон приёмки на Windows на SHA релиза | тред драйвера, владелец (устройство) | не начато |
| R15 CI | CI main и ночные прогоны сейчас красные: #2454, #2455 | зелёные 7 дней до тега; версия во всех манифестах; `CHANGELOG.md` из `changelog.d/` | координатор перед тегом | не начато |
| R16 issue | таблица ниже | закрыть исправленные; остальные в release notes | координатор | в работе |
| R17 книга | 529 строк в `book/src`; фрагменты `rust,ignore` учат удалённым формам (`column!`, сигналы в `init_state`); учебника нет | включения из компилируемых файлов и сборка в CI; учебник по приложению приёмки после R14, R15 и R19 | тред «Сделать книгу компилируемой и написать учебник» | не начато |
| R18 форма виджета | конструктор с setter'ами, дети как `ViewSeq` (`Column::new((a, b))`), макросы `column!` удалены; ребёнок передаётся позиционно в одних конструкторах и `.child()` в других, `on_press` рядом с `on_pressed`, `cx` рядом с `ctx` | правило и табличный тест; правки по нему | тред «Привести имена и форму child к одному правилу»; контролы Material — тред перевода контролов | не начато |
| R19 тестовый API | см. ворота | один вход `flui::testing` на локаторах драйвера; лишние драйверы из фасада; шаблон `flui create`; диагностика переполнения Flex (`crates/flui-objects/src/layout/flex.rs`) после ворот рендера | тред «Дать авторам один тестовый API» | не начато |
| R20 агент | `flui run --agent`, `flui mcp`, `flui record` (`crates/flui-cli/src/commands/`); `an_agent_reads_and_taps_the_app_run_with_agent_through_mcp` (`crates/flui-cli/tests/cli_agent.rs`) на тестовом приложении; `an_agent_presses_the_counter_on_a_live_device` (`cli_live_agent.rs`) на счётчике в ночных прогонах | CI-строка с MCP-сессией (locate, act, inspect, snapshot) на приложении приёмки headless; ночной прогон macOS или пометка «без прогона»; README: experimental | тред драйвера | в работе |
| R21 имена | см. ворота | подтверждение списка владельцем; публикация заглушек | тред «Занять имена крейтов на crates.io» | в работе |
| R22 пустые фичи | у фасада пустые `localizations` и `gpu-readback-tests`; четыре пустые `signals` (ADR-0085 §5 в `docs/gaps.md`) | удалить или дать действие | тред «Убрать лишнее из графа и фич» | не начато |
| R23 профиль шаблонов | в шаблонах `flui create` только `[profile.release]` | dev-профиль в шаблонах; базовая линия холодной сборки | тред «Убрать лишнее из графа и фич» | не начато |
| R24 один вход | три входа: `run_app`, `run_app_android`, `AppLaunch` (`src/lib.rs`); API нескольких окон в prelude | один `run_app`; окна за нестабильной фичей | тред «Свести запуск к одному `run_app`» | не начато |
| R25 навигация | пункт ADR-0093 в `docs/gaps.md`: Router, `push_for_result`, `WidgetsApp::router` есть; именованные маршруты и двери `NavigatorHandle` живы, роутера в `MaterialApp` и `CupertinoApp` нет | достроить ADR-0093 | тред «Оставить одну форму навигации» | в работе |
| R26 закрытые enum | `#[non_exhaustive]` на части публичных `enum`; линта нет | правило и clippy-линт | тред снимка API | не начато |
| R27 состояние виджета | запись [Widget states still fill their handles in `init_state`](../../../gaps.md#widget-states-still-fill-their-handles-in-init_state); `StateCell` рядом с `Signal` (решение 7 аудита) | перевести состояния каталогов на `create_state`; решить судьбу `StateCell` | тред инвентаря | в работе |

## Открытые issue critical и high

Сверка с main 2026-11-12 чтением кода. «Закрыть» — код на main уже делает то, что просит issue,
или работу заменил названный ADR; закрывает владелец или координатор.

| Issue | О чём | Состояние на main | Для релиза |
|---|---|---|---|
| #1043 (critical) | время жизни нативного окна в `Renderer` | `Renderer::new(target: impl WindowTarget)`, поверхность через `SurfaceFactory` и `SurfaceLease` (`crates/flui-engine/src/renderer.rs`), `create_surface_unsafe` нет | закрыть |
| #551 (critical) | `OwnerPlatform` и власть цикла событий | `OwnerPlatform` в `flui-app`; ADR-0039, ADR-0083, ADR-0254 | закрыть с указателем |
| #566 (critical) | трекер Runtime.1 | зонтик над #551, #558–#565 | пересобрать или закрыть с указателем на эту спеку |
| #563 (critical) | инфраструктура соответствия native и headless | частично: `HeadlessHost`, записи и повтор в `flui-testing`; прогоны в записи [Most implemented rows never ran on their environment](../../../gaps.md#most-implemented-rows-never-ran-on-their-environment) | release notes (матрица платформ) |
| #546 (critical) | корпус верности виджетов Flutter 3.44 | чек-лист вехи, зависит от #540, #542 | release notes |
| #1092 | раскладка клавиш winit | winit удалён из workspace, нативные хосты в `flui-native` | закрыть как устаревший |
| #1040 | устаревшие уведомления фокуса при повторном входе | очередь `pending_focus_transitions` (`crates/flui-interaction/src/focus/manager.rs`) | закрыть |
| #542 | именованные маршруты, PopScope с результатом | заменён ADR-0093 (R16) | закрыть с указателем |
| #558 | жизненные циклы Task, Worker, Service | ADR-0049; сервисы и протокол закрытия есть; `ProcessWorker` отложен | закрыть с указателем |
| #1037 | частичный damage по идентичности слоя | на нативных хостах есть (ADR-0304); веб рисует кадр целиком (`DirectSink`) | release notes (веб) |
| #1039 | локальные линейные коммиты топологии рендера | квадратичное удаление ушло; глобальный флаг `needs_render_reorder` и обход леса остались (`crates/flui-view/src/tree/element_tree.rs`) | release notes (производительность) |
| #1041 | обход всей арены при заимствовании грязных поддеревьев | `collect_disjoint_mut` обходит slab на каждый корень (`crates/flui-rendering/src/storage/tree.rs`) | release notes (производительность) |
| #1091 | изменения геометрии текста без полного relayout | канала нет | после релиза, новый API |
| #540 | выделение, многострочность, скрытый ввод в `EditableText` | выделение, скрытие и ручки есть; многострочного поля нет ([No text input on mobile and the Web](../../../gaps.md#no-text-input-on-mobile-and-the-web)) | release notes |
| #559 | поток растра и политика задержки | ADR-0045 заменён ADR-0305; остаток — пункт ADR-0305 в `docs/gaps.md` | release notes (растр синхронный) |
| #560 | рантайм под управлением хоста, interop с wgpu | нет; запись [Add-to-app](../../../gaps.md#add-to-app-no-flui-view-inside-an-existing-native-app) | вне scope |
| #562 | ворота последовательного и параллельного исполнения | отложено до параллельной раскладки (D5) | вне scope |
| #564 | эталонное многооконное приложение | нет | вне scope |
| #565 | выпуск проверенного публичного API Runtime.1 | покрывается R3 и R19 | закрыть с указателем на эту спеку |

## Требование → доказательство

| Требование | Тест или прогон | Статус |
|---|---|---|
| R4 | `ordinary_facade_graph_excludes_test_support` | есть, без HTTP и TLS в запрещённом списке |
| R5, R6 | строки persistence на приложении приёмки (`tests/fixtures/acceptance_app/persistence.rs`) | есть headless |
| R10, R11 | строки teardown на приложении приёмки (`tests/fixtures/acceptance_app/teardown.rs`) | есть headless |
| R20 | `an_agent_reads_and_taps_the_app_run_with_agent_through_mcp`, `an_agent_presses_the_counter_on_a_live_device` | есть на тестовом приложении и счётчике; строки на приложении приёмки нет |
| остальные | из `tasks.md` спек фич и PR тредов | — |

## Карточки (мелкие правки без спеки)

| Карточка | Доказательство | Статус |
|---|---|---|
| Док `flui-widgets` описывает `bon`-builder'ы, которых нет | `crates/flui-widgets/src/lib.rs:32` | не начато |
| Rebuild любого UI-рантайма будит только последнее открытое окно (выведено чтением 2026-10-05, не перепроверено) | `crates/flui-app/src/app/runtime.rs`, `wake_frame` | сверить |

Номера ADR-0128–ADR-0134 и ADR-0137–ADR-0141, которые прежняя редакция этой спеки
резервировала, не заняты и не переиспользуются.
