# Решения по открытым вопросам архитектурного ревью

Состояние на main @ cab06137d (2026-09-25). Документ закрывает §14 «Вопросы к владельцу» (flui-global-architecture.md:720-735). По каждому вопросу работали три судьи (engineer, ecosystem_author, owner) и отдельный проверяющий. Ниже итог после сверки решений между собой. Если проверяющий находил серьёзную (major) проблему, решение изменено, и причина указана.

Условные обозначения: `arch` = flui-global-architecture.md, `plan` = plan.md, `roadmap` = roadmap.md (все в scratchpad). Остальные пути даны от корня репозитория D:\flui. **Гипотеза** означает, что утверждение не проверено командой или пробой.

## Сводка

| # | Вопрос | Решение | Уверенность | Главный довод |
|---|--------|---------|-------------|---------------|
| 1 | Где живут официальные пакеты (D7) | **Один воркспейс.** `packages/` станет каталогом его членов, а не вложенным воркспейсом. Внешнего автора имитируют гейт зависимостей и `cargo package --workspace` на каждом PR. Пакет уходит в отдельный репо только по записанному триггеру. **Изменено:** джоба «strip path, собрать против опубликованного поезда» отменена | 0.82 | 55% коммитов каталога (68 из 123) трогают и ядро. Поезда на crates.io ещё нет. `cargo package --workspace` уже сейчас доказывает сборку «как снаружи» |
| 2 | `flui-sdk` вне Stable | **Отдельный крейт Evolving `0.N`, host-free, привязанный к поезду, фасад его не реэкспортирует.** **Изменено:** добавлен страж `links = "flui_train"`, и цена lockstep для сторонних пакетов записана явно | 0.80 | 127 крейтов против 191. D=статус-кво ломает манифест каждого пакета при любой смене топологии. Без `links` две версии поезда дают E0308 вместо ошибки резолвера |
| 3 | Исключения P10 | **Единственное поимённое исключение: raw-window-handle, и только через `HasWindowHandle`/`HasDisplayHandle`/`HandleError`.** serde и cursor-icon идут в список «разрешённые 1.x». Словарь доступности свой (`SemanticsRole`/`SemanticsAction`, `#[non_exhaustive]`), **не** зеркало AccessKit 1:1. Escape-модули Evolving добавляются **только вместе с потребителем**; сейчас это один `flui_sdk::gpu` за фичей `wgpu-30`. **Изменено:** пин отображения делается тестом по `ALL`, а не exhaustive match | 0.75 | accesskit выпустил 13 ломающих релизов за 2024–2026, у Role/Action нет `#[non_exhaustive]`. Зеркало из 182 ролей переносит этот churn, а не останавливает его |
| 4 | Exit B0 | **Три класса пунктов.** [G] гейты подключены и умеют падать. [S] структурные инварианты зелёные. [R] долг заморожен ratchet-ом. Счётчик крейтов убран, мёртвый `just ci` заменён. **Изменено:** набор forbid-reach для K, определение «одной транзакции», reach на `cargo metadata` | 0.80 | Критерий «forbid-reach wgpu» уже зелёный (wgpu=0) и ничего не доказывает. Нужный набор красный сегодня только через `flui-interaction → flui-platform` |
| 5 | Где реактивное ядро (D4) | **Поэтапно.** Шаг 1: шов внутри flui-view (`ReadScope`, `RebuildSink`, non-Clone драйверы). Шаг 2a: обобщить читателей на фазы внутри flui-view. Шаг 2b: вынести в `flui-reactive` (V) в одном PR с первым render-подписчиком. **Изменено:** 2 шага стали 3, два драйвера вместо одного, другой первый потребитель, фича `signals` снимается до 2b | 0.75 | Inherent-метод `Signal::get(cx)` требует графа и `Signal<T>` в одном крейте. 15 против 27 инвалидируемых крейтов. P8 требует второго потребителя |
| 6 | Фасад без Material | **`default = []`, у фасада нет фич `material`/`cupertino`, `flui create` добавляет `flui-material` явно.** **Изменено:** причина в семвере и порядке поезда, а не в цикле Cargo. «Независимая каденция» отменена: Material выпускается в том же прогоне, что поезд | 0.82 | Stable-фасад, который публично реэкспортирует Evolving-пакет, делает каждый мажор Material мажором `flui` |
| 7 | Колбэки и `Writer` (W5) | **Типизированный `&mut EventCx<'_>` на событийных колбэках, выдаваемых фреймворком.** `WriterSource` берётся из `LifecycleContext`. Это единый механизм, которым любой виджет (каталог и сторонний) открывает `EventCx` для пользовательского колбэка. Отдельного «люка для чужих `Fn()`» в W5 нет. **Изменено** (проверка `holds: false`): gesture arena не трогаем, П4 чиним отдельно и раньше, StateCell остаётся рантайм-тиром | 0.62 | Запись в build становится E0061 на этапе компиляции. Сигналов в продакшене 0, а W5 и так ломает все `Send + Sync` сеттеры |
| 8 | Windows IME + Narrator как H0-гейт | **Нет.** В H0 гейтом становится headless-кит соответствия text-store. Автоматическая Windows-линия (UIA). Живая сессия остаётся в exit B1, свежесть определяется изменениями путей и релизом. **Изменено:** контракт text-store должен включать запись и асинхронную блокировку (TSF), а не только чтение | 0.80 | В Win32 нет ни одной строки IME-кода. Шов плагинов не касается IME. Human-сессия не может быть merge-гейтом по AGENTS.md |

---

## 1. Где живут официальные пакеты (D7)

**Контекст.** В `arch:177` предложен вложенный воркспейс `packages/` с `[patch]` и CI против опубликованного поезда. `plan:31` говорит «`flui-*` в отдельных репо, один релизный поезд». ADR-0028 (Accepted) держит Material/Cupertino на слое L7 внутри воркспейса.

**Варианты.** A: один воркспейс, `packages/` как члены плюс гейт. B: вложенный воркспейс (D7 как написан). C: отдельные репо (plan.md). D: A сейчас, проверка против поезда после первой публикации, вынос по триггеру.

**Решение: D, изменённое проверкой.**
- `packages/` это каталог членов **корневого** воркспейса: один `Cargo.lock`, один `cargo metadata`, path-зависимости с точным пином поезда. Физический перенос (`git mv crates/flui-material packages/flui-material` и т.п.) делается **вместе с** переводом пакета на `flui-sdk` (W3), а не отдельным PR ради перемещения.
- Метаданные `[package.metadata.flui] tier-kind = "official"`.
- **Гейт в `cargo xtask workspace`** получается обобщением уже существующего механизма `allowed-dependents` (tools/xtask/src/workspace.rs:9-13, 231-257; crates/flui-material/Cargo.toml:87-88), ключом служит `tier-kind`. Параллельного правила не заводим.
  - Прямое направление: official зависит только от `flui-sdk`, `flui-platform-api`, `flui-protocol` и от объявленных рёбер official→official. До W3 работает режим allowlist: сейчас там 9 ядровых крейтов Material, плюс `flui-view/runtime-internals` у hot-reload. Allowlist может только сокращаться, у каждой записи срок «W3». Когда `flui-sdk` появится, режим становится строгим.
  - Обратное направление строгое с первого дня: крейт ядра не называет official-крейт ни в каком виде, включая optional и dev. Исключения перечисляются поимённо, у каждого указана причина и срок:
    - `flui-testing` dev → `flui-devtools` (crates/flui-testing/Cargo.toml:106). Выход: тест observation-seam переезжает в flui-devtools. Иначе при любом выносе devtools рвётся dev-цикл devtools↔testing.
    - `flui-cli` dev → `flui-hot-reload` (crates/flui-cli/Cargo.toml:114). Срок: W3.
    - `flui-app` optional → `flui-hot-reload` (crates/flui-app/Cargo.toml:65,108). Удаляется в W3 по D15.
  - Исключения для flui-widgets **не нужны**: строки crates/flui-widgets/Cargo.toml:85,182 это комментарии, ребро идёт в обратную сторону. Все три судьи прочитали эти строки неверно.
- **Доказательство паритета с внешним автором на каждом PR** (главное изменение): xtask-команда, которая запускает `cargo package -p flui-sdk -p <official...> --offline` (или `--workspace`). Каждый член собирается из упакованного tarball против tarball-ов соседей во временном локальном реестре. Так ловятся пропущенные `include`, фичи, существующие только через path, и ошибки пинов. Команда доступна с W3 и входит в `checks` или в тяжёлую CI-джобу.
- `cargo-semver-checks` на `flui-sdk` работает в advisory-режиме, начиная с первой публикации (сравнение с последней опубликованной версией). Это модель внешнего автора, который застрял на старом sdk.
- **Out-of-tree фикстура.** Сторонний пакет-пример лежит вне `members` и зависит только от `flui-sdk = "=<train>"` через `[patch]` на путь. Он использует публичные точки расширения: кастомный виджет, расширение темы, плагин.
- **Триггеры выноса в отдельный репо** (записываются в ADR проверяемыми критериями):
  - каденция пакета расходится с поездом два поезда подряд или больше;
  - у пакета есть внешний мейнтейнер;
  - доля core-coupled коммитов пакета ниже порога за N месяцев (метод `git log`, как в исследовании).
  Вынесенный репо версионируется lockstep с поездом. Первые кандидаты: OS-плагины и a2ui. Material и Cupertino выносятся последними.

**Почему не другие.**
- B до первой публикации равносилен path-зависимостям с накладными расходами: двойная компиляция ядра (два fingerprint в пробе), точный пин `=0.2.0-dev` (проба показала, что `"0.2"` не матчит prerelease), change_scope не видит второй корень (tools/xtask/src/change_scope/classify.rs:325-350), нужны новая CI-джоба и две суперсессии ADR.
- C требует двусторонних пинов и бота-роллера, как во Flutter. Flutter сам консолидировал репозитории: plugins→packages в 2023, engine→flutter в 2024.
- Scheduled-джоба «strip path deps, собрать против последнего опубликованного поезда» из исходного D **отменена**. При lockstep-пинах main пинит следующую dev-версию, которой нет на crates.io (проба probe-train: `no matching package ... location searched: crates.io index`). Если переписать пины вниз, получится сборка main-Material против старого sdk, то есть комбинация, которую ни один пользователь не получит. Пару «Material N + sdk N» проверяет `cargo publish` в момент релиза.

**Доказательства.**
- `curl https://crates.io/api/v1/crates/flui-*` → «does not exist».
- Cargo.toml:106 `0.2.0-dev`.
- git-замер: 123 коммита, 68 затрагивают другие crates/.
- probe-packages: `[patch]` работает только на корне воркспейса, два fingerprint.
- probe-train: `cargo package --workspace --allow-dirty --offline` → `Unpacking ... (registry target\package\tmp-registry) ... Compiling ... Finished`.
- Прецеденты: Dioxus (packages/ в одном воркспейсе, DioxusLabs/sdk отдельно в lockstep), Linebender (xilem+masonry вместе, vello/parley отдельно), Flutter (.ci/flutter_*.version, bin/internal/flutter_packages.version).

**Что нашла проверка и как учтено.**
- (major) Post-publish джоба сломана самой конструкцией. Заменена на `cargo package` на каждом PR плюс semver-checks.
- (major) Доказательство «как снаружи» доступно до публикации. Окно риска до первого релиза закрыто.
- (minor) Исключение для flui-widgets основано на комментарии. Удалено, гейт строится обобщением `allowed-dependents`.
- (minor) Пропущенные рёбра testing→devtools, cli→hot-reload, app→hot-reload, runtime-internals. Внесены в allowlist со сроками.

**Правки миграции и планов.**
- W1 (трек A): `tier-kind` в манифестах. Гейт «ядро не называет official» строгий, с перечисленными исключениями.
- W3: создание `flui-sdk` → перевод Material/Cupertino/devtools/hot-reload на sdk с переносом в `packages/` в том же PR. Allowlist становится строгим. Появляются команда `cargo xtask package-check` (рабочее имя) и out-of-tree фикстура.
- После первой публикации: semver-checks advisory на `flui-sdk`.
- `arch:177` заменить на: «**Официальные пакеты:** `packages/` в этом репозитории как члены корневого воркспейса (не отдельный воркспейс). Паритет с внешним автором обеспечивают гейт `tier-kind = official` в `cargo xtask workspace` и `cargo package` официальных пакетов вместе с `flui-sdk` на каждом PR. Отдельный репо только по триггеру из ADR тиров, с lockstep-версиями.»
- `arch` таблица D7 (:599): решение «один воркспейс + гейт + `cargo package`; вынос по триггеру»; отвергнутые варианты «вложенный воркспейс (нет поезда, двойная сборка), отдельные репо сейчас».
- `arch:178`: «Новая проверка `cargo xtask workspace`: обобщение `allowed-dependents` по `tier-kind`; ни один крейт ядра не называет official-крейт, даже опционально или в dev; исключения поимённо, с причиной и сроком.»
- `plan:31`: «Официальные пакеты (`flui-*` в `packages/` этого репо, один воркспейс и один релизный поезд; отдельный репо только по записанному триггеру, версии lockstep): …»

**ADR.** «ADR-NNNN: Уровни поставки и официальные пакеты». Supersedes: ADR-0028 (в части размещения L7; остальное, то есть развязка Material/Cupertino от ядра, сохраняется). ADR-0041 не суперсидится: модель слоёв остаётся, добавляется `tier-kind`. Если топология тиров меняет перечень слоёв, ADR-0041 правится отдельной строкой «Amended-by».

---

## 2. `flui-sdk` как отдельный Evolving-крейт

**Контекст.** `arch:14,169,209` предлагает host-free sdk, но `arch:204` реэкспортирует его из Stable-фасада, что противоречит :209. `arch:205` гейтит `unstable` Cargo-фичей.

**Варианты.** A: отдельный train-locked крейт без реэкспорта. B: всё в Stable-фасаде. C: Evolving-модуль в фасаде за opt-in. D: без sdk, пакеты точно пинят внутренние крейты (статус-кво).

**Решение: A, изменённое проверкой.**
- Отдельный крейт `flui-sdk`, тир K, host-free: без flui-app, flui-engine, wgpu в нормальном замыкании. Версия `0.N`, бамп на каждом поезде безусловно. Публикуется в том же автоматическом прогоне, что поезд, включая патчи. Внутренние крейты пинятся точно.
- Две части:
  1. **Stable-замыкание:** реэкспорты целыми модулями по тем же путям, что у фасада (форма `pub use flui_x as x`). Нет обёрток и newtype, тип-идентичность сохраняется. Проверка идентичности: тест, что `flui_sdk::<m>::T` и `flui::<m>::T` являются одним и тем же типом.
  2. **Evolving:** только именованные модули `paint`, `pipeline`, `hooks`, `gpu`. Экспозиция пакета к ним считается grep-ом `flui_sdk::(paint|pipeline|hooks|gpu)`.
- **Фасад не реэкспортирует sdk.** `arch:204` удаляется. `arch:205` (`#[cfg(feature = "unstable")]`) заменяется на `--cfg flui_unstable` или удаляется: проба 2 показала утечку через унификацию фич.
- **Страж одного поезда (новое):** `links = "flui_train"` с тривиальным `build.rs` в одном нижнем крейте, от которого зависит всё на поезде (кандидат: `flui-foundation`). Проба probe-sdklock показала: без `links` приложение на `flui = "1"` и пакет на `flui-sdk = "0.1"` молча резолвятся в две копии internal (`0.2` и `0.3`), и сборка падает с E0308. С `links` резолвер выбирает один поезд. Требование на W3: регистровый тест (локальный реестр), что несовпадение поездов даёт выбор старого поезда или ошибку резолвера, но никогда не E0308. Тот же страж нужен самому Stable-фасаду: его caret-совместимость 1.x держится только внутри одного поезда (minor-находка проверки).
- **Потолок Evolving-поверхности.** На W3 замер через rustdoc-JSON (см. согласованность: инструмент общий с P10). Если сверх хуков больше ~30 элементов, решение пересматривается, потому что sdk превращается во второй фасад. Гипотеза: 12–20 элементов.
- **Выпуск (graduation) после H3:** элемент переезжает в Stable-модуль `flui`, если пережил N поездов без изменений и у него есть второй потребитель. sdk сохраняет реэкспорт по старому пути. `gpu` и dev-хуки могут оставаться Evolving бессрочно.
- До W3 sdk не создаётся. Статус-кво D остаётся временным, и никакая документация не предлагает сторонним авторам зависеть от внутренних крейтов.

**Почему не другие.**
- B замораживает ~12 недоказанных внутренностей (DrawOp, Canvas, RenderUpdateImpact, LocalPostFrameHandle, FrameSnapshot, InputEpochId, RenderPhysicalShape, RenderTable, TranslationFraction, ElementId::new, RebuildReason, observe) и тянет хост в каждый пакет.
- C: Cargo-фича протекает (проба 2), а cfg навязывает RUSTFLAGS каждому потребителю Material.
- D кодирует топологию в манифесте каждого пакета. Удаление flui-tree и flui-localizations или добавление flui-reactive и flui-runtime ломало бы все внешние манифесты: паттерн bevy_egui, где на каждый релиз Bevy нужен новый релиз плагина.

**Доказательства.**
- `cargo tree` с дедупом: 127 (flui-material) против 191 (flui).
- 172 точных пина `=0.2.0-dev`, из них 14 у flui-material.
- Проба 1 (E0308 `multiple different versions of crate core`), проба 2 (утечка фичи), probe-sdklock (с `links` и без).
- cargo-semver-checks #638 (ложные срабатывания на кросс-крейтовых реэкспортах).
- Прецеденты: wgpu-core в lockstep, accesskit_consumer с собственным счётчиком, Masonry как крейт для авторов библиотек.

**Что нашла проверка и как учтено.**
- (major) «Несовпадение поездов = ошибка резолвера» неверно для межминорных поездов. Добавлен страж `links`.
- (major) Lockstep-цена для пакетов только на Stable-поверхности не записана. Записана как принятая цена: минорный апгрейд `flui` в приложении ждёт переиздания пакетов. Смягчение для первой стороны уже есть: официальные пакеты в том же воркспейсе (решение 1) и публикуются тем же прогоном, так что лага нет. Для третьих сторон решение отложено (см. «Что требует владельца», п. 3).
- (minor) Проблема фасада та же. Покрыта тем же стражем.
- Не проверено: свободно ли имя `flui-sdk` на crates.io (cratesio MCP не подключился). Проверить до W3.

**Правки.**
- `arch:204`: удалить строку `pub use flui_sdk as sdk; …`.
- `arch:205`: заменить на `#[cfg(flui_unstable)] pub mod unstable; // RUSTFLAGS=--cfg flui_unstable; Cargo-фича протекает через унификацию`.
- `arch:209`: убрать «фасад пинит точную версию и в своём мажоре не обещает стабильность `flui::sdk`», вместо этого: «Фасад от sdk не зависит. sdk привязан к поезду (`0.N` на каждый поезд, тот же прогон публикации). Один поезд в графе гарантирует `links = "flui_train"`.»
- `arch:176`: дописать «плюс `links = "flui_train"` в нижнем крейте поезда».
- W3: создание sdk, страж `links`, регистровый тест, замер поверхности.
- `plan:57` (уровни API): «Evolving (`flui-sdk` для авторов пакетов, Material/Cupertino детали, devtools-протокол, плагины)».

**ADR.** «ADR-NNNN: Тиры API и `flui-sdk`» (общий ADR тиров, который уже упоминается в D17). Supersedes: нет. Фиксирует: тиры, sdk, страж поезда, правило выпуска, потолок поверхности.

---

## 3. Исключения P10 (апстрим-типы в Stable)

**Контекст.** `arch:90` (P10 по каденции мажоров), `arch:102` (serde и rwh как исключения), `arch:292` (свои Role/Action «1:1 с AccessKit»), `arch:314` (gpu в Evolving).

**Варианты.** A: зеркало AccessKit 1:1. B: строго свой словарь, без escape-модулей. C: версионированные реэкспорты. D: B плюс версионированные escape-модули в Evolving sdk.

**Решение: B-ядро плюс D-механизм «по требованию».** Engineer и ecosystem_author выбрали D, owner выбрал B. Разногласие только в том, выпускать ли escape-модули заранее. Сведено так:
- **Stable:** только свои типы.
  - `SemanticsRole` (33) плюс флаги и `SemanticsAction` (~24) получают `#[non_exhaustive]` и переносятся в `flui-protocol`.
  - Правило имён: «имя AccessKit или ARIA, если такое понятие есть». Это руководство, а не контракт 1:1.
  - `PlatformAccessibility::publish(TreeUpdate)` остаётся во внутреннем `flui-platform`.
- **Ввод:** собственные `PointerEvent`/`KeyEvent`/`Key`/`NamedKey`/`Code`/`Modifiers`/`ScrollDelta`/`PointerId` в `flui-platform-api`. `NamedKey` и `Code` генерируются xtask-ом из исходников keyboard-types (~307 и ~216 вариантов). Тест round-trip по каждому варианту плюс diff-проверка при бампе keyboard-types. Новые W3C-клавиши приходят как `Unidentified`, пока их не приняли, и это записано в ADR. Глоб `pub use keyboard_types::*` из ui-events не должен попасть ни на один Stable-путь.
- **Единственное поимённое исключение:** raw-window-handle 0.6, только через `HasWindowHandle`/`HasDisplayHandle` и `HandleError`. Цена записывается в ADR: «rwh 0.7 = мажор `flui-platform-api`». `fn raw_window_handle(&self) -> RawWindowHandle` (crates/flui-platform/src/window.rs:196) заменить до заморозки.
- **Разрешённые 1.x (это не исключения):** serde, cursor-icon.
- **Никогда в Stable:** accesskit, ui-events, keyboard-types, dpi, wgpu, kurbo, peniko, parley, fontique, cosmic-text, android_activity.
- **Escape-модули Evolving (механизм D):**
  - Сейчас только `flui_sdk::gpu` за фичей `wgpu-30`. У него есть потребитель (внешний GPU-контент, `arch:314`). `pub use ::wgpu` (crates/flui-engine/src/lib.rs:229) убирается с любых путей, достижимых из Stable.
  - `flui_sdk::a11y::accesskit_025` (хук ролей и свойств) и `flui_sdk::input::ui_events_03` добавляются **только в PR с потребителем**, аддитивно в минорной версии. Хук применяется последним, трогает только свойства, которые владеющая модель оставила пустыми, и не может менять идентичность узла или структуру дерева.
  - Реэкспорты accesskit из flui-testing (a11y.rs:36) уходят в Evolving/internal. Stable-хелперы тестов проверяют свои `SemanticsRole`/`SemanticsAction`.
  - `android_activity` (crates/flui-app/src/lib.rs:116) уходит в Evolving-модуль `native`.
- **Пины отображения (исправлено проверкой):**
  - Исходящее направление (свой enum → accesskit) после `#[non_exhaustive]` в другом крейте требует `_`-ветки. Поэтому пин делается тестом: сгенерированный `const ALL: &[SemanticsRole]` (и то же для Action), каждый вариант кроме `None` отображается в `Some(accesskit role)`, каждое действие имеет входящий источник или помечено «FLUI-only».
  - Входящее направление: exhaustive match без `_` только в `semantics_action_for` (accesskit_translation.rs:316) и `semantics_action_args_for` (:359, по `ActionData`).
  - Exhaustive match на 182 роли `accesskit::Role` **не делаем**: продакшен-кода, который по ним матчит, нет.
- **Гейт замыкания.** Команда `cargo xtask api-closure` (рабочее имя) на обходе rustdoc-JSON. Denylist по путям перечисленных апстримов, allowlist `raw_window_handle`, `serde`, `cursor_icon`, охват `flui`, `flui-platform-api`, `flui-protocol`. Перед включением: проба в scratchpad с подложенным `pub fn f() -> accesskit::Role`, гейт обязан его поймать. cargo-public-api и cargo-semver-checks на хосте не установлены, rustdoc-JSON требует nightly (гипотеза). Тулчейн nightly для rustdoc-JSON заводится один раз и общий с semver-checks H3 и замером sdk (решение 2).

**Почему не другие.**
- A: зеркало наследует переименования и удаления (StaticText→Label, InlineTextBox→TextRun, ToggleButton, удалённые deprecated-роли), дублирует свой словарь из 33 ролей и добавляет ~150 вариантов, которые никто не производит (P9).
- C: несовместим с заморозкой H3. Egui и Masonry стабильности не обещают.
- Чистый B без механизма оставил бы авторов кастомных виджетов с ролями вне 33 без выхода. Поэтому механизм D записан, но каждый модуль появляется только вместе с потребителем.

**Доказательства.**
- crates.io, число ломающих релизов 2024-01…2026-09: accesskit 13, ui-events 4 (с 2025-05), wgpu 11, rwh 0 (milestone v0.7 закрыт пустым 2026-02-10), parley/fontique 11, keyboard-types 1, serde и cursor-icon 0.
- accesskit-0.25.0: 182 Role, 22–23 Action, один `#[non_exhaustive]` на весь файл.
- Slint 1.x: `raw-window-handle-06` стабилен, `unstable-wgpu-30` и `unstable-fontique-011` нет.
- iced_core: свои keyboard/mouse/touch.

**Что нашла проверка и как учтено.**
- (major) `#[non_exhaustive]` снимает компиляторный пин исходящего отображения. Пин переведён на тест по `ALL`.
- (minor) Пин по `accesskit::Role` бесполезен. Оставлены только Action/ActionData.
- (minor) keyboard-types сам `#[non_exhaustive]`. Генерация плюс diff-проверка, в ADR записан путь через `Unidentified`.
- (minor) Инструментов нет на хосте. Гейт условный, сначала проба, затем rustdoc-JSON в xtask.

**Правки.**
- `arch:292`: «`flui-protocol` задаёт собственные `#[non_exhaustive]` `SemanticsRole`/`SemanticsAction` (плюс флаги); имена следуют AccessKit/ARIA там, где понятие существует; отображение в AccessKit внутреннее, пинится тестом по `ALL` (исходящее) и exhaustive match по `accesskit::Action`/`ActionData` (входящее). Роли вне словаря идут через Evolving-хук `flui_sdk::a11y::accesskit_NNN`, который появляется с первым потребителем.»
- `arch:102`: «C: контракты (STABLE; raw-window-handle как единственное поимённое исключение; serde и cursor-icon разрешены как 1.x)».
- `arch:90`: в столбце «Проверка» заменить «cargo-public-api снимок транзитивного замыкания» на «`cargo xtask api-closure` (обход rustdoc-JSON; denylist/allowlist), после пробы с подложенным нарушением».
- `arch:314`: «Интероп с wgpu живёт в `flui_sdk::gpu` за фичей `wgpu-NN`».
- Миграция:
  - W1: `#[non_exhaustive]` и тест `ALL`, замена `_` в :316/:359.
  - Волна выделения `flui-platform-api`: собственные типы ввода (генерация), window.rs:196.
  - W3: вынести `pub use ::wgpu` и `android_activity`.
  - До H3: `api-closure` в `checks`.

**ADR.** Часть ADR тиров (решение 2), раздел «P10: апстрим-типы». Supersedes: нет. Для ADR-0030 правка идёт по решению 8/D10, не здесь.

---

## 4. Exit B0

**Контекст.** `roadmap:617` «just ci зелёный на чистом Mac без обходов; cargo build --workspace с README без lld; 26 крейтов; ни одного файла > 3000 строк в flui-app и flui-widgets». justfile в репо нет. В `arch:172` счётчик назван случайностью.

**Варианты.** A: минимальная замена. B: 10 пунктов, всё в ноль. C: «гейты подключены, структура зелёная, долг заморожен». D: оставить счётчики.

**Решение: C, изменённое проверкой.** Exit B0 записывается так:

**[G] Гейты подключены и умеют падать.**
- Закреплённый список in-process проверок (тест в tools/xtask/src/tasks/checks.rs:138-160) содержит: `workspace` (тиры), `reach`, `globals`, `module-dag`, `markers`, `file-length`, «ядро не называет official».
- У каждого гейта есть `--self-test` на подложенном нарушении (прецедент `wgsl --self-test`, checks.rs:108-109).
- Гейт без self-test и без записи в закреплённом списке к B0 не засчитывается.

**[S] Структурные инварианты зелёные.**
1. **Тир-гейт** (`cargo xtask workspace`, код в tools/xtask/src/workspace.rs, не в tasks/). У каждого манифеста есть `tier = V|C|S|R|K|H|pkg`, `tier-kind` и порядок внутри тира. Нормальные и build-рёбра идут вниз или вбок только к более раннему крейту в порядке. Dev-рёбра проходят через `allowed-dev-dependents`: у flui-view есть dev-цикл с flui-testing (crates/flui-view/Cargo.toml:63-67). Крейт без тира валит гейт. Тир-гейт проверяет только прямые рёбра, транзитивное отсутствие проверяет `reach`, они не пересекаются.
2. **`cargo xtask reach`** (новый, в `checks`). Работает по резолвленному графу `cargo metadata --locked` без фильтра таргета (эквивалент `--target all`), по **всем** комбинациям фич фасада: замер 0.39 с на граф, урезать не нужно. Сопоставление по имени пакета или glob, а не через `cargo tree -i` (тот падает на отсутствующем пакете и на нескольких версиях).
   - **Набор forbid для K (исправлен):** `flui-platform`, `winit`, `android-activity`, `ndk`, крейт `windows` (не `windows-sys`), `objc2-app-kit`, `objc2-ui-kit`, `wgpu`, `flui-engine`, `flui-app`.
   - Generic FFI (`jni`, `windows-sys`, `core-foundation`, голый `objc2`) разрешены или идут в allowlist рёбер с причиной. Пример: `reqwest → rustls-platform-verifier → jni` через фичу `network-images` (crates/flui-widgets/Cargo.toml:177, crates/flui-assets/Cargo.toml:69).
   - Набор для `pkg`: тот же плюс OS-крейты.
   - Сегодня гейт красный **только** через `flui-interaction → flui-platform`, это проверено `--prune flui-platform`. Поэтому B0 закрывается только вместе с D1a.
   - Три `TREE_FACTS` hot-reload (tools/xtask/src/tasks/facade.rs:53-92) переезжают в reach в том же PR.
3. **`cargo xtask module-dag -p flui-widgets`** зелёный. Тот же механизм закрепляет решение 5, шаг 1: модуль reactive без `crate::`-импортов.
4. **Одна транзакция кадра** (если W2 остаётся в B0, иначе пункт дословно переходит в exit B1). Определение **через тип, а не через имена функций** (исправлено). Точки входа, которые ведут фазы кадра, недостижимы вне `flui-runtime`: `drive_frame_with_lane`, `handle_begin_frame`/`handle_draw_frame` планировщика и frame-entry view/binding. Механизм: `pub(crate)`, sealed-токен или capability-тип. Если тип невозможен, syn-скан с self-test, который разрешает вызовы только в flui-runtime. Проверка «нет `pub fn pump_frame`» отменена: её обходит переименование, а `HeadlessBinding::pump_frame` (crates/flui-testing/src/lib.rs:955-1079) не вызывает ни одну из прежних двух функций.

**[R] Долг заморожен (ratchet, только сокращается).**
- Allowlist-ы `globals` (syn-скан без `#[cfg(test)]`), `file-length` (≤3000 строк, модель rustc tidy), `undocumented_unsafe`, `multiple-versions`.
- Каждый засевается сканом **в том же PR**, что и гейт. Регекс-оценки (24 `thread_local!`, 89 static) засевом не служат.
- `realm_dispatch.rs` (7149 строк) должен уйти из allowlist в ходе работ по ui_realm в B0.
- Perf: счётчики и записанный baseline существуют. `perf --check` в B0 не блокирует, блокирует на exit B1.

**[P] Гигиена.**
- `cargo xtask ci` зелёный на `macos-latest` (заменяет мёртвый `just ci`).
- `cargo build --workspace` по README без lld.
- Версия воркспейса `0.2.0` (сейчас Cargo.toml:106 `0.2.0-dev`).
- Число крейтов только отчётный факт таблицы тиров, не цель.

**Порядок и WIP:** тир-гейт и reach → markers и file-length → globals и module-dag. Одновременно открыт один PR с гейтом.

**Почему не другие.** A выхолощен: wgpu=0 уже сегодня. B тянет содержательную работу W5 и измерительный проект в B0. D держит случайный счётчик.

**Доказательства.**
- `cargo tree -p flui-interaction -e normal -i flui-platform --depth 1`.
- `--prune flui-platform -i jni` → `rustls-platform-verifier ← reqwest ← flui-assets ← flui-widgets`.
- `time cargo tree -p flui … --all-features --target all` → 0.386 s.
- `ls justfile` → нет.
- В main.rs:35-105 нет команд reach/globals/module-dag/perf/markers/file-length.
- Прецеденты: rustc tidy LINES=3000, cargo-deny `wrappers`, Flutter analyze.dart, Nx depConstraints.

**Что нашла проверка и как учтено.**
- (major) Набор K красный и после D1a из-за jni через reqwest. Набор исправлен.
- (major) «Одна транзакция» проверялась по неверным функциям. Определение через capability.
- (minor) `cargo tree -i` непригоден. Reach строится на `cargo metadata`.
- (minor) Опасение по времени снято. Путь к коду исправлен, dev-рёбра учтены.

**Правки.**
- `roadmap:617` заменить на:
  «**[G]** закреплённый список `checks` содержит workspace-tiers, reach, globals, module-dag, markers, file-length, core-names-no-official; у каждого `--self-test`. **[S]** `cargo xtask workspace` (тиры) 0 находок; `cargo xtask reach` зелёный с набором K = {flui-platform, winit, android-activity, ndk, windows, objc2-app-kit, objc2-ui-kit, wgpu, flui-engine, flui-app} по всем комбинациям фич фасада (требует D1a); `cargo xtask module-dag -p flui-widgets` зелёный; [если W2 в B0] точки входа фаз кадра достижимы только из flui-runtime. **[R]** allowlist-ы globals/file-length/undocumented-unsafe/multiple-versions засеяны сканом и только сокращаются; perf baseline записан (неблокирующий). **[P]** `cargo xtask ci` зелёный на macos-latest; `cargo build --workspace` по README без lld; версия 0.2.0.»
- `roadmap:304` «26 крейтов вместо 28» → «крейты соответствуют таблице тиров (число не цель)».
- `roadmap:718` «just ci зелёный» → «`cargo xtask ci` зелёный».
- `arch:172`: последнее предложение заменить на «Счётчик крейтов убран из exit B0 (решение по §14 п. 4).»
- W1 трек A: перечень гейтов с self-test. Шаги в `.github/workflows` (macOS-прогон, неблокирующий perf) идут отдельным PR с подписью владельца.

**ADR.** Не требуется: это критерий вехи, а не кросс-крейтовый контракт. Определение «одной транзакции» записывается в ADR про flui-runtime (W2), если он пишется.

---

## 5. Где живёт реактивное ядро (D4)

**Контекст.** Граф сейчас в одном файле crates/flui-view/src/reactive/mod.rs (1159 строк). Он зависит только от `flui_foundation::{ElementId, RebuildReason}`, smallvec, thiserror, `crate::owner::ExternalBuildScheduler` и `crate::BuildContext`. `arch:596` (D4) выбирает `flui-reactive` (V).

**Варианты.** A: `flui-reactive` сейчас. B: модуль в foundation. C: остаётся в flui-view, render подписывается через стёртый трейт. D: поэтапно, крейт появляется вместе со вторым потребителем.

**Решение: D, изменённое проверкой (три шага, два драйвера).**
- **Шаг 1** (начало W3, внутри flui-view, без нового крейта):
  - `Signal::get/with/try_*` принимают `&dyn ReadScope`, `BuildContext: ReadScope`. Апкаст работает на 1.98.1 (probe-reactive).
  - **Поверхность `ReadScope` только для чтения:** регистрация чтения и проверка идентичности графа (`fn scope(&self) -> ScopeRef<'_>`). Владеющий хендл `Reactive` она **не** возвращает (иначе противоречит D4 «`BuildContext::reactive()` удалён»). Сам `BuildContext::reactive()` удаляется в срезе W5 вместе с `Writer` (решение 7). До того момента запись идёт по-старому.
  - `ExternalBuildScheduler` заменяется одно-методным `RebuildSink`.
  - **Драйверы по фазам:** non-Clone `ElementDriver` (у `BuildOwner`: begin/end_element_build, register_element_reader, release_element, set_scheduler) и non-Clone `RenderDriver` (регистрация и брекетинг layout/paint), который realm выдаёт из того же графа и передаёт `PipelineOwner`. Realm чеканит ровно два драйвера. compile_fail-доктесты показывают, что ни `ReadScope`, ни клон хендла не вызывает хуки.
  - Шаг закреплён `module-dag` (решение 4): в модуле reactive ноль `crate::`-импортов.
  - **Фича `signals` снимается в шаге 1** (D4 п. 5): модуль и `ReadScope` безусловны, потому что supertrait нельзя загейтить `cfg`. Шаг 1 зависит от go/no-go ADR-0074, записанного в Cargo.toml:642-645 («до #1090 field-mask registry»). Если go/no-go не пройден, шаг 1 ждёт.
- **Шаг 2a** (внутри flui-view): обобщить читателей до enum `Element(ElementId) | Layout(RenderId) | Paint(RenderId)` (сегодня жёстко `ElementId`, mod.rs:136,143,150,191,469). Добавить фазовые гарды: запись во время layout или paint отклоняется или откладывается, по образцу `WrittenDuringBuild`. Тесты на каждый гард.
- **Шаг 2b** (W3): один PR переносит модуль в `flui-reactive`. Параметры крейта: тир V, internal, `publish = false` до решения о публикации, зависимости foundation + smallvec + thiserror + tracing, ARCHITECTURE.md, переписанный комментарий Cargo.toml:71-77. В том же PR появляется **первый render-подписчик**: поле RenderObject, которое читается в paint через `PaintContext: ReadScope` и пишется вне фаз кадра. RenderObject без `Send` (render_object.rs:178), `PipelineOwner` тоже `!Send`. К PR прилагается тест, который падает без изменения и показывает сам repaint, а не только наличие подписки.
  - **ScrollPosition не первый потребитель:** он пишется из `perform_layout` (viewport.rs:1057,1104,1827-1828), а политики записи во время layout пока нет.
  - **CustomPainter:** трейт `Send + Sync` (custom_painter.rs:105), поэтому нужно явное изменение API. Например, хранить `Send` `SignalSlot`/`SignalSender`, а не `Signal`. Отдельный пункт W3 или W5.
  - Адаптер `Listenable` поверх графа живёт в `flui-reactive`. `Arc`/`Mutex` нотифаер в foundation остаётся для `Send + Sync` пользователей до флипа `!Send` в W5 и затем **удаляется** (пункт W5).
- **Если к концу W3 render-потребителя нет**, граф остаётся в flui-view, и D4 правится явно.
- **Критерий свёртки в foundation:** warm-edit (тронуть reactive, замерить `cargo check -p flui-app`, сравнить с той же правкой в foundation). Никогда не `--timings` одного юнита: сам крейт проверяется за 0.16 с. Замер делается один раз в worktree в PR шага 2b. Там же проверяется гипотеза «у Cargo нет early cutoff».

**Почему не другие.**
- C не даёт render/animation назвать `Signal<T>` (inherent-impl только в определяющем крейте) и навсегда оставляет две системы наблюдения.
- B пересобирает 27 крейтов вместо 16 в окно частых правок, включая engine (72k строк), platform (46k) и interaction (40k), и кладёт понятия жизненного цикла элемента в нижний слой. Хуки всё равно становятся публичными.
- A создаёт крейт без второго потребителя (P8, дефект «unwired surface»).

**Доказательства.**
- `cargo tree -i` (счёт включает сам крейт): foundation 27, view 12, rendering 14, animation 14, объединение view/rendering/animation 15. Значит «инвалидируется за правку»: **27 (foundation) против 16 (flui-reactive + 15)**.
- Правка D4: foundation имеет 20 прямых зависимых, а не 17.
- Оценка по строкам: ~547k против ~335k. Это оценка, не замер.
- probe-reactive: тесты проходят, 5 неиспользуемых хуков как признак будущей публичной поверхности.
- Прецеденты: Leptos `reactive_graph` ниже tachys, sycamore-reactive, floem_reactive. Bevy, Slint и GPUI (модуль ядра) подходят как конечное состояние стабильного API.

**Что нашла проверка и как учтено.**
- (major) Шаг 2 не механический, граф жёстко про элементы. Разбит на 2a и 2b.
- (major) Одного `ReactiveDriver` у `BuildOwner` render-фазе не хватает. Введены два драйвера.
- (major) Оба названных потребителя плохие. Выбран другой первый потребитель.
- (major) Порядок фичи `signals` не записан. Фича снимается в шаге 1, с зависимостью от go/no-go.
- (minor) `ReadScope` отдавал `Reactive`. Поверхность стала только для чтения.
- (minor) Счёт крейтов. Исправлен, правило счёта записано.

**Правки.**
- `arch:596` (D4), столбец решения: «`flui-reactive` (V, internal) появляется в PR с первым render-подписчиком (шаги 1 → 2a → 2b); `ReadScope` только для чтения; `ElementDriver`/`RenderDriver` non-Clone; подписчики `Element|Layout|Paint`; адаптер Listenable в flui-reactive, Arc-нотифаер foundation удаляется в W5; сигналы безусловны с шага 1». Отвергнутый вариант: «ядро в foundation (27 против 16 крейтов на правку; 20 прямых зависимых)».
- W3: шаги 1/2a/2b. Критерий свёртки warm-edit.
- W5: удалить Arc-нотифаер foundation. API CustomPainter для сигналов.
- `arch:726` (§14 п. 5): закрыть со ссылкой сюда.

**ADR.** «ADR-NNNN: Размещение реактивного графа и фазовые подписчики». Supersedes: нет. Правит формулировку ADR-0074 «lives beside BuildOwner» строкой Amended-by. FOUNDATIONS C1 правится по тексту D4.

---

## 6. Фасад без Material по умолчанию

**Контекст.** Cargo.toml:598 `default = ["material"]`, :605-606. src/lib.rs:148-149 реэкспорт, :267-276 Material в prelude. README.md:44-66 обещает Material-first. Шаблон crates/flui-cli/src/templates/counter.rs:22 пишет `flui` без фич, но на :80 и :115 использует Theme и ElevatedButton.

**Варианты.** A: `default = []`, фич нет, `flui create` добавляет `flui-material`. B: оставить default. C: A плюс мета-крейт позже. D: переименование.

**Решение: A, изменённое проверкой.**
- `flui` получает `default = []`, фич `material`/`cupertino`/`devtools`/`hot` нет (`arch:189`). `pub use flui_material as material` и Material-половина prelude удаляются.
- **Правильная причина** (исправлено по сравнению с `arch:19,684`): цикл Cargo возникает, только если пакет зависит от фасада (проба A). При Material на `flui-sdk` default-фича собралась бы (проба B, exit 0). Реальные причины:
  1. публичная зависимость Stable-крейта от Evolving-пакета: мажор Material становится мажором `flui`;
  2. порядок публикации поезда;
  3. гейт «ядро не называет official» остаётся без исключений.
- **«Независимая каденция» отменена** (major-находка проверки). При точных пинах новый `flui` и старый `flui-material` не резолвятся вместе (проба probe-q6/E, `failed to select a version`). Смягчения:
  - (а) Material в том же воркспейсе и публикуется тем же прогоном поезда (решение 1), так что пара «последний flui + последний flui-material» совместима всегда;
  - (б) `links = "flui_train"` (решение 2) заставляет резолвер выбрать один поезд, а не собрать две копии;
  - (в) `flui create` и README пинят известную совместимую пару;
  - (г) проверка `cargo package` (решение 1) покрывает эту пару до публикации.
  Caret-пины на внутренние крейты **не** вводим: это ломает тип-идентичность (решение 2).
- **Атомарный PR:**
  - шаблон counter пишет `flui-material = <ver>` и импорт;
  - в `flui-material` добавляется модуль `prelude` (его сейчас нет, crates/flui-material/src/lib.rs:110-157) с набором из src/lib.rs:267-276 без реэкспорта имён flui_widgets, чтобы не было неоднозначности глобов, плюс доктест пары глобов;
  - тест в flui-cli генерирует counter и делает `cargo check`;
  - Material-примеры переезжают в `packages/flui-material/examples`, каталожно-нейтральные остаются в корне. Корневой dev-dep на flui-material **не** держим;
  - переписать `cargo xtask facade-combos`;
  - **строка .github/workflows/ci.yml:924** (`--features material --example sliver_demo`) и комментарий :1012-1019. Это правка workflows, нужны подпись владельца и метка `full-ci`;
  - список документов строится командой `rg 'flui::(material|cupertino)|features.*(material|cupertino)'` по всему репо, кроме docs/archive. Известные места: README.md:44-66, src/lib.rs:38-64/148-149/267-276, ARCHITECTURE.md, book/src/**, docs/{FOUNDATIONS,getting-started,testing}.md, tests/facade_smoke.rs (14 мест), `[[test]]` Cargo.toml:741-773, строка AGENTS.md про `required-features`;
  - раздел «Design systems» в документации крейта на docs.rs;
  - CHANGELOG с заметкой о миграции `flui::material::` → `flui_material::`.
- **Последовательность:** фасад теряет фичу только когда `flui-material` можно подключить напрямую из проекта вне воркспейса (path/git), то есть после W3-перевода на sdk. Иначе у авторов H0–H1 пропадает путь к Material.
- Мета-крейт (`flui-kit`) откладывается до свидетельств, что двухстрочная установка мешает.

**Почему не другие.**
- B дешёв сейчас и дорог потом: удалить default-фичу после Stable-публикации значит сломать semver. Плюс нужно исключение в гейте.
- D отдаёт самое заметное имя волатильному крейту.

**Доказательства.**
- Пробы A–E в scratchpad/probe-q6.
- Cargo SemVer reference (публичные зависимости).
- Flutter 3.47 (`material_ui`/`cupertino_ui`), Dioxus (default без дизайн-системы), Slint (смена стиля по умолчанию анонсировалась).

**Что нашла проверка и как учтено.**
- (major) Независимая каденция ложна. Отменена, смягчения выше.
- (major) Пропущена ci.yml:924. Добавлена.
- (minor) Нет `flui_material::prelude`. Добавить.
- (minor) Неполный список документов. Заменён командой.

**Правки.**
- `arch:19` п. 7 переписать: «Никаких фич фасада, которые тянут пакеты. Причина: Stable-фасад не может публично зависеть от Evolving-пакета (мажор пакета стал бы мажором `flui`), и публикация `flui` не должна ждать пакета. Цикл Cargo возникает только при ребре пакет→фасад, которое D17 запрещает.»
- `arch:684`: убрать «Исправлено: фичи сняты» как следствие цикла, дать ту же формулировку.
- `arch:609` (D17): отвергнутый вариант дополнить «default-фича `material` (собирается при пакетах на sdk, но делает Material публичной зависимостью Stable)».
- W3: PR фасада после перевода Material на sdk.
- README: двухстрочный quick start и `flui create`.

**ADR.** Часть ADR тиров (D17), раздел «Фасад». Supersedes: нет. ADR-0028 уже суперсидится по решению 1.

---

## 7. Сигнатуры колбэков и `Writer` (W5)

**Контекст.** Сегодня `Signal::set(self, r: &Reactive, v)` (mod.rs:774), `Reactive: Clone` и выдаётся любым контекстом. Защита только рантаймовая (`WrittenDuringBuild`). 92 публичных `on_*`, ~432 места вызова, ~160+ мест с listener/post-frame. Продакшен-пользователей сигналов 0.

**Варианты.** A: `&mut EventCx` везде. A+: A плюс `WriteHandle` из `LifecycleContext`. B: ambient scope. C: только гард, realm из хендла.

**Решение: A с одним механизмом `WriterSource`. Изменено проверкой (`holds: false`).**
- **Событийные колбэки, которые выдаёт фреймворк** (публичные `on_*` каталога, классифицированные как event), получают `&mut EventCx<'_>`: borrowed, создаётся на каждый диспатч, `Deref<Target = Writer>`. Изначально он минимален: Writer плюс id realm-а. spawn, focus и команды добавляются только когда конкретному сеттеру нужны, с записью в ADR. Позиции в дереве нет (`arch:438`).
- **`WriterSource`** берётся из `LifecycleContext` (ADR-0078), `!Send`, привязан к realm-у. Это **единственный** способ открыть `EventCx`. Им пользуются:
  - (а) виджеты каталога, чтобы обернуть колбэки распознавателей;
  - (б) **сторонние виджеты** с собственными `on_changed` (условие ecosystem_author);
  - (в) внутренний путь `Spawner`-продолжений и `UiCommand::SignalWrite`.
  Ограда `disallowed_methods` «каталог не пользуется люком» **снимается**: механизм общий, а не люк. Отдельного публичного «WriteHandle для чужих `Fn()`» в W5 нет. Он добавляется аддитивно при первом реальном same-thread `!Send` потребителе.
- **Gesture arena не меняем** (major-находка проверки). `GestureArenaMember::accept_gesture/reject_gesture/poll_deadline` и алиасы `Rc<dyn Fn(Details)>` (tap.rs:91, drag.rs:113-121) остаются как есть. Виджет захватывает `WriterSource` (он `!Send`, а алиасы `Rc`, так что это легально) и вызывает `source.write(|cx| user_cb(cx, details))`. Следствия: flui-interaction не зависит от flui-reactive, решение 7 не зависит от момента выноса крейта в решении 5, custom-recognizer API не ломается.
- **Listenable, animation-status, post-frame** сегодня `Send + Sync` (notifier.rs:46,78; animation.rs:11; frame.rs:729). cx они получают **только в том же срезе**, где снят их `Send`. До этого запись оттуда идёт через `WriterSource`, захваченный после флипа, или через `SignalSender`. Платформенные хуки (`Send` до W1b, platform.rs:319…698) работают только через `SignalSender`.
- **Query-колбэки** не получают Writer: генераторы маршрутов, drag will-accept, всё, что возвращает `EventPropagation` или `bool`. Классификация 92 сеттеров оформляется таблицей в ADR, с командой grep. В таблице есть столбец «место вызова» (event / listener / post-frame / **build**).
- **Колбэки, которые виджет вызывает из build** (например, `AnimatedSize::build` вызывает `on_end()`, animated_size.rs:201-209), до смены сигнатур переносятся в status-listener или post-frame. К этому нужен тест, падающий, если `on_end` всё ещё вызывается в build.
- **StateCell и RebuildHandle** остаются **рантайм-тиром с гардом**, `&mut Writer` они не принимают. `StateCell` уже представляет собой capability из `init_state` (`bind`, state_cell.rs:189) и работает несвязанным. Гард ADR-0074 распространяется на `StateCell::schedule` во время build. Типизирующее утверждение ADR ограничено записями в `Signal`.
- **П4 чиним отдельно и раньше W5:** `UiCommand::SignalWrite` маршрутизируется по `SignalSlot.graph` (mod.rs:78-82) в realm-владелец. Сейчас он пишет в граф основной презентации (commands.rs:449-455), и `ForeignGraph` уже ловит несовпадение (mod.rs:267-273). Нужен падающий multi-window тест. Типизированный cx П4 **не** чинит «по построению» и межреалмовую безопасность на этапе компиляции не даёт. Аргумент в пользу A только один: отклонение записи в build на этапе компиляции.
- `BuildContext::reactive()` и публичный `BuildOwner::reactive()` (build_owner.rs:973) удаляются в срезе W5.
- Хелпер `callback(|cx| ..)` поставляется вместе со сменой сигнатур (проба a2: ошибка HRTB «Fn is not general enough»). Строка в AGENTS.md или в документации для авторов виджетов. compile_fail-доктесты: `&mut Writer` не уходит в `'static` (a5), у build нет пути к Writer.
- **Порядок срезов** (по одному открытому):
  1. фикс П4;
  2. типы, хелпер, `WriterSource`;
  3. сигнатуры `Signal::set/update`, удаление `reactive()`;
  4. codemod `flui migrate` по крейтам (пилот flui-cupertino, 9 мест), каждый раз `check-changed` зелёный.
- **Триггер отката на C** записан заранее: если пилот покажет, что HRTB-трение или рост шаблонов заметно хуже пробы, переключаемся на C до 1.0.
- Гард ADR-0074 и ADR-0075 остаётся авторитетным. В ADR Writer называется «сужением, а не заменой» (проба `a_nested_sync_callback_during_build_still_needs_guard`).

**Почему не другие.**
- B требует той же проводки, что A, но падает в рантайме (NoScope) и добавляет TLS против курса на сокращение глобалов.
- C остаётся запасным вариантом: дёшев сейчас, дорог после 1.0.
- A+ с отдельным люком в исходной форме: у люка почти нет компилируемых применений до флипов `!Send`, а ограда каталога конфликтует с обёрткой распознавателей.

**Доказательства.**
- probe-writer: a1/a3/a7/a8/a9/a10 компилируются; a2 падает с HRTB; a4 E0061; a5 lifetime; a6/c1 `!Send`; d1 требует аннотаций; e1 owned-токен утекает.
- probe-q7-verify: E0277 при захвате `!Send` в `add_listener`.
- Прецеденты: GPUI `Fn(&Event, &mut Window, &mut App)`, Xilem `&mut State`. Compose backwards-write и SwiftUI «Modifying state during view update» показывают, что рантайм-модель оставляет этот класс ошибок живым.

**Что нашла проверка и как учтено.**
- (major) Arena. Не трогаем, обёртка через `WriterSource`.
- (major) П4 не требует типов. Отдельный ранний фикс, довод убран.
- (major) `WriteHandle` без применений. Отдельный люк снят, есть один механизм `WriterSource`.
- (minor) StateCell. Рантайм-тир.
- (minor) Вызовы из build. Столбец в таблице и перенос.
- (minor) Нет механизмов ограды и тиринга. Ограда снята, Evolving-статус задаётся ADR тиров (решение 2), а не несуществующим атрибутом.

**Правки.**
- `arch:266`: «Запись: событийные колбэки фреймворка получают `&mut EventCx<'_>` (Deref к `Writer`); виджеты открывают его через `WriterSource` из `LifecycleContext`; gesture arena не меняется; listener/post-frame получают cx в срезе флипа `!Send`; StateCell остаётся рантайм-тиром под гардом; П4 чинится маршрутизацией по `SignalSlot.graph` до W5. Writer является сужением, гард авторитетен.»
- `arch:560` (W5): добавить срезы 1–4 и триггер отката. В W2 или W3 фикс П4.
- `arch:727` (§14 п. 7): закрыть.

**ADR.** «ADR-NNNN: Запись в сигналы: EventCx и WriterSource». Supersedes: нет. Amends ADR-0074 (граница рантайм-гарда, StateCell) и ADR-0078 (новый capability `WriterSource` на `LifecycleContext`).

---

## 8. Живое свидетельство Windows IME + Narrator

**Контекст.** В Win32 нет IME-кода: grep WM_IME/ITextStore/ITfThreadMgr/Imm по crates/ пуст, нет override `text_input()`, нет фич Ime/TextServices в crates/flui-platform/Cargo.toml:89-105. docs/BETA.md:122 держит Windows в статусе **experimental**. `plan:63` ставит японский IME и Narrator в exit B1.

**Варианты.** A: жёсткий H0-гейт. B: только B1, без изменений. C: свежесть < 30 дней. D: гибрид.

**Решение: D, изменённое проверкой.**
1. **H0-гейт по контракту, в CI:** headless-кит соответствия pull text-store (D10). Публичный, версионируемый test-support API, который внешние крейты запускают на своих виджетах. Встроенный TextField проходит **тот же** кит. **Контракт text-store: чтение + правка + блокировка** (исправлено):
   - UTF-16 ACP-смещения;
   - глаголы записи (`SetText`/`InsertTextAtSelection`-эквиваленты);
   - модель блокировки или edit-session, в которой блокировку можно выдать позже (асинхронно, `TS_S_ASYNC`).
   Кит работает как TSF-подобный харнесс: запрашивает блокировки асинхронно и правит со стороны IME, а не только задаёт запросы по диапазону. Покрываются суррогаты, графемы, диапазоны композиции. С приходом TSF-бэкенда добавляются unit-тесты против mock `ITextStoreACP` (как Chromium mock_tsf_bridge).
2. **Автоматическая Windows-линия.**
   - Сначала одноразовый прогон на ветке: `cargo xtask device windows-a11y` на `windows-latest`. С **пред-проверкой**, отделяющей ограничение хоста от регрессии: окно пробы найдено и видимо, desktop-сессия есть (`OpenInputDesktop`/`GetForegroundWindow`). Если пред-проверка не прошла, результат CANNOT_VERIFY, иначе FAIL. Формулировку «сопоставить exit 2 с провалом» убираем: это уже так (plan.rs:249-256).
   - При успехе шаг кладётся в `gpu-test` (там уже есть сборка wgpu и WARP, ci.yml:953) или в отдельную тяжёлую джобу из `needs` и `HEAVY_JOBS`, а не в `platform-windows`: сборка `a11y_probe` release с фасадом резко меняет профиль той джобы. Время джобы замеряется.
   - `windows-input` и будущий `cargo xtask device windows-ime` (romaji виртуальными клавишами при открытом IME, чтение через UIA TextPattern) идут на хосте мейнтейнера или в Hyper-V VM.
   - Сначала проверяется, что путь `KEYEVENTF_UNICODE` в desktop-mcp (device.rs:538-570) обходит IME (гипотеза).
3. **Живая сессия** (японский IME «toukyou» → 東京 через композицию и конверсию, Narrator читает метку и поле) остаётся **exit B1**. Она записывается в `docs/evidence/windows.toml` (commit, OS, версия IME, команда, результат, артефакт) и рендерится в BETA.md (P5). UIA-клиент не засчитывается как половина «Narrator». Пока записи нет, строка Windows остаётся experimental с явным «нет IME», и никакие тексты не заявляют паритет IME или a11y.
4. **Свежесть по изменениям и релизу.**
   - Запись устаревает, если после записанного коммита менялись `crates/flui-platform/src/platforms/windows/**`, трейт text-input/text-store, мост семантики/UIA или пути текстовой раскладки, влияющие на диапазоны каретки.
   - «Свежая» означает: записанный коммит является предком коммита релиза, и между ними не менялись trigger-пути. Сами файлы evidence из trigger-набора исключены (исправлено: старое условие «на коммите релиза» было круговым).
   - Проверка идёт **локально** в `cargo xtask release-check` до тега, где есть полная история. Если записанного коммита нет в истории, это явная ошибка, а не pass. Альтернатива: `fetch-depth: 0` в release.yml, это правка workflows с подписью владельца.
   - Календарный потолок (~90 дней) и NVDA speech-spy не вводим, пока не доказан дрейф.

**Почему не другие.**
- A невыполним сегодня и ставит шов плагинов (clipboard/haptics/dialogs, `arch:304,633`) в очередь за TSF (W1–W2) и Parley (W4). Human-сессия не может быть merge-гейтом по AGENTS.md.
- B оставляет контракт непроверенным и даёт записи тихо устаревать.
- C означает ~12 ручных сессий в год, сигнал слабый, статус будет мигать.

**Доказательства.**
- grep без совпадений по IME.
- `Get-WinUserLanguageList` → en-US, ru (нет ja-JP).
- Narrator.exe есть, API снятия речи нет.
- AccessKit гоняет UIA против реального HWND на windows-latest.
- Chromium tsf_text_store_unittest.
- boringcactus 2025: из 43 крейтов все три проверки прошли только Dioxus, Slint, Tauri.
- Flutter Windows на IMM32 без живого гейта (#182876, #191196).

**Что нашла проверка и как учтено.**
- (major) Кит тестировал бы read-only контракт, которого TSF недостаточно. Контракт расширен до чтения + правки + блокировки, это правка `arch:284`.
- (minor) Путаница CANNOT_VERIFY. Добавлена пред-проверка.
- (minor) Стоимость джобы. Размещение в gpu-test или отдельной джобе.
- (minor) Мелкая история в release.yml. Проверка идёт локально.
- (minor) Круговое условие. Определение через предка.

**Правки.**
- `arch:284`: «IME: pull-модель: синхронная поверхность text-store на owner-потоке **для чтения и правки** (текст в диапазоне, выделение, composing, rect для диапазона, индекс по точке, замена диапазона/вставка в выделение) в UTF-16-смещениях, с моделью блокировки/edit-session, допускающей отложенную выдачу (как `ITextStoreACP::RequestLock` + `TS_S_ASYNC`). Реализует `TextFieldState` + Parley; публичный кит соответствия гоняется в CI. Windows с первого дня использует TSF + UIA TextPattern/ValuePattern.»
- `plan:60`: «платформы со свежим свидетельством (1 → 4 → 6); свежесть = записанный коммит является предком релиза и trigger-пути платформы не менялись».
- `plan:63`: без изменений (Windows + Narrator + японский IME остаются в exit B1). Дописать: «; H0-гейт по IME является китом соответствия text-store, а не живой сессией».
- `roadmap:351`/`:489`: добавить «запись в `docs/evidence/<platform>.toml`, свежесть по trigger-путям, проверяет `cargo xtask release-check`».
- Миграция по одному WIP за раз:
  - H0 вместе с D10: спецификация и headless-кит;
  - одноразовый windows-latest прогон (подпись владельца);
  - P5: evidence TOML и staleness;
  - W1–W2→W3: TSF mock-тесты и windows-ime.

**ADR.** Суперсессия ADR-0030 §1 (уже запланирована в D10), дополненная контрактом «чтение + правка + блокировка» и китом соответствия. Supersedes: ADR-0030 (§1). Политика свидетельств оформляется документом P5 (docs/BETA.md), не ADR.

---

## Согласованность между решениями

1. **Размещение пакетов ↔ стабильность sdk ↔ фасад ↔ циклы (1, 2, 6).**
   - Все три решения исходят из того, что каденция lockstep. Один воркспейс (1), sdk `0.N` на каждом поезде (2), Material публикуется тем же прогоном (6).
   - Фраза «независимая каденция пакетов» удалена из всех трёх. Она противоречила точным пинам (проба probe-q6/E) и отмене post-publish джобы (1).
   - Циклов нет по построению: пакеты зависят от sdk, а не от фасада (D17). Фасад не называет official (6). Гейт «ядро не называет official» строгий (1). Поэтому снятие default-фичи в (6) мотивировано семвером и порядком поезда, а не циклом.
   - Единый страж одного поезда `links = "flui_train"` (2) защищает и пары фасад+sdk, и пары фасад+Material (6).
2. **Проверки паритета (1, 2).** Исходные тексты 1 и 2 ссылались на «CI против последнего опубликованного поезда» (D7). Это заменено одним механизмом: `cargo package` официальных пакетов вместе с sdk на каждом PR, плюс semver-checks advisory на sdk после публикации, плюс out-of-tree фикстура.
3. **Инструменты rustdoc-JSON (2, 3, H3).** Замер Evolving-поверхности sdk (2), гейт замыкания P10 (3) и semver-checks (H3) требуют rustdoc-JSON, вероятно на nightly (гипотеза). Заводится один закреплённый nightly-тулчейн для этих задач, одним PR с `cargo xtask doctor`, а не три разных механизма.
4. **Реактивный крейт ↔ форма Writer (5, 7).**
   - Writer больше не обязан жить ниже flui-interaction: arena не меняется, распознаватели оборачивает виджет через `WriterSource` (7). Поэтому (7) не зависит от того, вынесен ли граф в `flui-reactive` к W5. Если вынесен, `Writer`/`WriterSource` лежат в flui-reactive. Если нет (условие 5), лежат в flui-view.
   - `ReadScope` в (5) только для чтения. `BuildContext::reactive()` удаляется в срезе W5 (7), что совпадает с D4.
   - Драйверы (5: `ElementDriver`/`RenderDriver`) и `WriterSource` (7) являются разными capability с разными правами. Хуки жизненного цикла не открывают запись, `WriterSource` не открывает хуки.
   - Флип `!Send` в W5 одновременно разблокирует cx для listener-ов (7) и удаление Arc-нотифаера foundation (5).
5. **Выход B0 ↔ гейты ↔ Windows (4, 1, 8).**
   - «Ядро не называет official» (1) входит в [G] B0 (4).
   - `tier = pkg` / `tier-kind` из (1) входит в тир-гейт (4). allowlist Material до W3 является ratchet класса [R].
   - `module-dag` (4) закрепляет шаг 1 из (5).
   - В B0 **нет** живого Windows-свидетельства (8): оно принадлежит exit B1. Кит соответствия text-store (8) является H0-гейтом через тесты, а не пунктом B0.
   - Изменения `.github/workflows` из всех решений требуют подписи владельца по AGENTS.md: macOS-прогон и perf (4), ci.yml:924 (6), шаг windows-a11y (8), при необходимости `fetch-depth` (8). Их лучше собрать в один-два workflow-PR.
6. **P10 ↔ sdk ↔ фасад (2, 3, 6).** Stable-фасад содержит только Stable-замыкание: нет sdk (2), нет Material (6), нет апстрим-типов кроме rwh (3). Evolving-escape-модули живут только в `flui_sdk` (3), и это единственный потребитель Evolving-механики из (2).
7. **Последовательность волн.**
   - W1: гейты B0, `#[non_exhaustive]` + тест `ALL`, фикс П4 (7, может быть W2).
   - W3: sdk + `links` + перевод пакетов с `git mv` + `cargo package`-проверка + фасад `default = []` + реактивные шаги 1/2a/2b.
   - W5: Writer/EventCx + `!Send` + удаление Arc-нотифаера.
   - B1: живая Windows-сессия.
   - Противоречий в порядке нет. Единственная жёсткая цепочка: снятие фичи `signals` (5, шаг 1) зависит от go/no-go ADR-0074, и от него же зависит W5 (7).

## Что всё ещё требует решения владельца

1. **W2 (одна транзакция кадра) в B0 или в B1?** От этого зависит пункт [S]4 exit B0. **По умолчанию:** оставить в B0, как в таблице §11 архитектуры, но формулировка пункта уже готова к дословному переносу в B1, если B0 затягивается.
2. **Escape-модули P10 заранее или по требованию (3).** Engineer и ecosystem_author выбрали D (заранее), owner выбрал B (по требованию). **По умолчанию:** по требованию, кроме `flui_sdk::gpu` (потребитель есть). Хук ролей a11y выпускается первым, как только кастомному виджету понадобится роль вне 33.
3. **Лаг сторонних пакетов при lockstep sdk (2).** Минорный апгрейд `flui` в приложении ждёт переиздания сторонних пакетов. **По умолчанию:** принять до H3 (сторонних пакетов пока нет) и вернуться к вопросу на H3. Вариант на будущее: host-free Stable-крейт для пакетов, которым нужна только Stable-поверхность, с caret-требованием.
4. **Форма Writer (7), уверенность 0.62.** Панель единогласна за типизацию, но проверка опровергла два из четырёх доводов. **По умолчанию:** A с `WriterSource`, с пилотом codemod на flui-cupertino и записанным триггером отката на C. Здесь нужен вкус владельца: насколько важна эргономика `move ||` против ошибок компиляции для агентного кода.
5. **Правки `.github/workflows`**: ci.yml:924, macOS-прогон `cargo xtask ci`, неблокирующий perf, windows-a11y шаг, возможный `fetch-depth` в release.yml. **По умолчанию:** одобрить пакетом из двух PR с меткой `full-ci`, сначала одноразовый пробный windows-latest прогон на ветке.
6. **Системная настройка хоста:** включить ja-JP в Windows (сейчас en-US и ru). Агенты этого не делают. **По умолчанию:** включить перед работой над `windows-ime` (W3).
7. **Имя `flui-sdk` на crates.io** не проверено: cratesio MCP не подключился. **По умолчанию:** проверить до W3. Если имя занято, использовать `flui-package-sdk`.

Непроверенное, что не должно попасть в ADR как факт без перепроверки:
- стоимость двойной компиляции на реальном воркспейсе (только двухкрейтовая проба);
- лаг экосистемы Bevy 2–8 недель (вторичный источник);
- warm-edit выигрыш flui-reactive (оценка по строкам);
- проход windows-a11y и windows-input на hosted runner;
- обход IME путём `KEYEVENTF_UNICODE`;
- требование nightly для rustdoc-JSON-инструментов;
- текст ошибки резолвера для конфликтующих `=`-пинов через реестр (воспроизведена только в probe-sdklock с directory-source).
