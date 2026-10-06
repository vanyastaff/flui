# Релиз FLUI для сообщества — карта фич и календарь (уровень 0)

- **Статус:** черновик вместе с [requirements.md](requirements.md).
- **Обновлено:** 2026-10-05

Статусы фич: `—` (спеки нет) → `спека` → `контракт заморожен` → `в реализации` →
`проверено` → `смержено`.

## Что уже есть (проверено чтением, не запуском)

- Notes (`examples/two_screens`) работает только через facade. Router: три маршрута.
  Есть `Form`/`TextFormField`, ленивый `ListView::builder` на 10 000 строк и `FutureBuilder`
  с Retry и отменой при unmount.
- Headless-acceptance: `notes_public_input_flow_matrix` в `tests/fixtures/notes_flow.rs`.
  Запускается внешним consumer через `external_notes_showcase_runs_through_the_facade`,
  но только на wide/full/extended-линиях CI.
- Нативная acceptance: `cargo xtask device windows-notes`. Запускается вручную
  (SendInput + UIA), без IME, Narrator и GPU-пикселей.
- Нет: persistence/restoration API, IME в showcase, GPU readback Notes, учёта утечек при
  teardown, consumer по реестру, release tooling (`release-check`, `package-check`,
  `api-closure`).

## Карта фич

| Фича (`docs/plans/specs/<feature>/`) | Требования | Зависит от | Блокеры и issues | Статус |
|---|---|---|---|---|
| `persistence`: хранение и восстановление состояния, реальный IO в loading/error/retry | R5, R6 | — | нет ADR; новый межкрейтовый контракт | — |
| `router-restore`: стек маршрутов как список путей, deep link при старте | R5 | `persistence` (контракт) | ADR-0093 шаг 8 | — |
| `focus-keyboard`: полный обход с клавиатуры, активация | R8 | — | #1040; активация RawButton с клавиатуры | — |
| `text-ime`: композиция IME в Notes, headless-строка композиции, нативный backend | R7 | `focus-keyboard` (частично) | ADR-0090 §3 (TSF), #1091, #1092 | — |
| `render-proof`: GPU readback экранов Notes, жизненный цикл native window в Renderer | R9 | — | #1043 (critical) | — |
| `teardown`: учёт `Drop`, закрытие окна, паника в callback | R10, R11 | — | #1147, #1066, #1195, #1165 | — |
| `a11y-native`: скринридер на сертифицируемых платформах | R12 | `focus-keyboard` | #675, #1075 (Linux) | — |
| `send-flip`: `!Send` и сигнатура event-callback | R2 (предусловие) | — | W5-A4/W5-A5; критический путь (D1 = публикация) | — |
| `facade-surface`: снимок публичного API, курируемая поверхность | R3, R4 | `send-flip` | ADR-0089 Proposed | — |
| `publish-pipeline`: package/staging-реестр/consumer по реестру/порядок публикации | R1, R2, R15 | `facade-surface` | `.github/workflows`, нужно подтверждение владельца | — |
| `measurements`: бюджеты кадра, памяти, старта | R13 | `persistence` | `refresh_period` не в facade | — |
| `ci-coverage`: Notes acceptance на линии по умолчанию, красный weekly | R15 | — | `.github/workflows`, нужно подтверждение владельца | — |
| `authoring-styles`: три стиля описания виджетов из одного определения | R18 | `send-flip` (сигнатуры callback'ов), `facade-surface` | нет ADR; документация `column!` обещает несуществующий struct-литерал | — |
| `layout-diagnostics`: ошибки раскладки видны (переполнение, flex без ограничения, `Expanded` не под тем родителем) | R19 | — | `flex.rs:529-535`, `:658`; `effects.rs:719` без вызова | — |
| `testing-dx`: finder'ы, tap/enter_text по виджету | R19 | — | сейчас только `dispatch_pointer_down(x, y)` | — |
| `docs-community`: tutorial, README-матрица, docs.rs, шаблоны issue, обновление BETA | R16, R17 | всё остальное | — | — |

Первый полный поток showcase (цель октября): `persistence` + `router-restore` + `teardown` +
`render-proof` + `focus-keyboard`, через facade, headless и нативно на Windows.

## Календарь

| Период | Что |
|---|---|
| 10-05 – 10-11 | Утверждение уровня 0 (D1 решён 10-05). Requirements для `persistence`, `teardown`, `render-proof`, `focus-keyboard`. Тикет на красный weekly |
| 10-12 – 10-25 | Design и заморозка контрактов первого потока; реализация `[P]`-задач. Спеки `text-ime`, `send-flip`, `router-restore` |
| 10-26 – 10-31 | Первый полный поток Notes: сохранение, перезапуск, восстановление; headless + Windows native |
| 11-01 – 11-30 | `text-ime`, recovery (`teardown` R11), `a11y-native`, `send-flip` → `facade-surface`, `measurements`, нативные прогоны |
| 12-01 – 12-15 | Стабилизация, `publish-pipeline` (staging-реестр), consumer, docs, RC на SHA |
| 12-16 – 12-31 | Только исправления по RC; публикация и тег после подтверждения владельца |

Если срок под угрозой, владелец выбирает между сужением scope и сдвигом даты. Критерии
корректности не снижаются.

## Требование → доказательство

| Требование | Тест или прогон | Статус |
|---|---|---|
| R4 | `ordinary_facade_graph_excludes_test_support` | есть |
| остальные | заполняются из `tasks.md` спек уровня 1 | — |

## Карточки (мелкие фиксы без полной спеки)

| Карточка | Доказательство | Статус |
|---|---|---|
| Доки `WindowPolicy::SeparateRealms` обещают, что медленное окно не задержит соседа; все realm'ы на одном потоке | `crates/flui-app/src/app/runtime.rs:441-446`; `realm-model/experiment.md` §2 | — |
| Rebuild любого realm'а будит только последнее открытое окно (выведено чтением, не запуском) | `runtime.rs:468-482`, `desktop.rs:709`; `realm-model/experiment.md` §1 | — |
| Документация `column!` показывает несуществующий struct-литерал и метку `FR-034`; гейт `markers` не ловит `FR-NNN` | `crates/flui-view/src/macros/mod.rs:1-30` | — |
| Закрыть issue, исправленные в коде: #1092 полностью, #1187 с пометкой | ревью `teardown` и `focus-keyboard` | ждёт владельца |
| `docs/FOUNDATIONS.md:103`, `crates/flui-widgets/src/lib.rs:32` описывают `bon`-builder'ы, которых нет | `authoring-styles/requirements.md` | — |
