# Платформенный слой — задачи

- **Статус:** редакция 3 утверждена: ADR-0151, 0152, 0153, 0154, 0254 и 0255 приняты 2026-10-21;
  исполнение идёт, ход — в разделе «Ход исполнения»
- **Ограниченное утверждение:** LY8/LY9 исполняются вместе по
  [ADR-0172](../../../adr/ADR-0172-host-owned-system-preferences.md), используя
  текущие имена крейтов и owner-local runtime. Это не утверждает остальные задачи,
  переименование крейтов или шов возможностей. LY8/LY9 завершены: все пункты «Приёмки LY8/LY9»
  отмечены, ход — «Доведение до приёмки». Запуск на Mac — vanyastaff/flui#1840. Суть LY89
  (уведомления вместо опроса на macOS и Android, ноль пробуждений после Home на устройстве)
  сделана на текущей раскладке; за LY89 остаются headless-счётчик пробуждений в виртуальном
  времени и перенос в `preferences/`.
- **Дата:** 2026-10-20
- **Design:** [design.md](design.md); требования — [requirements.md](requirements.md)
- **База:** `main` @ `deb9faea2`
- **Итог:** 82 задачи LY20–LY102 одной упорядоченной серией (идентификатор LY77 не занят). Порядок
  задаёт колонка «Зависит от»; сроков, выпусков и оценок трудозатрат у задач нет, серия идёт до
  последней задачи. LY8/LY9 — работа вне серии, завершена.

## Правила исполнения

1. Задача = worktree под `.claude/worktrees/<имя>` = ветка = один PR. Агент работает в своём
   worktree и не трогает чужие.
2. Проверки по AGENTS.md «Checking a change»: `cargo fmt --all`, затем clippy heli
   (`--all-targets -- -D warnings`) и nextest на изменённых крейтах и на их зависимых, если
   изменился публичный API; docs-only задача — только `cargo heli xtask checks`. Cargo идёт через
   heli: `cargo heli <аргументы>`, задачи репозитория — `cargo heli xtask <задача>`. Набор
   `flui-native` исполняется на Linux: изменение хоста Windows или macOS требует
   `cargo heli nextest run -p flui-native` вручную на этой ОС (до LY20 крейт называется
   `flui-platform`).
3. Fix-тест падает при откате исправления (изолированный checkout). Перемещение без изменения
   поведения доказывается тем же набором тестов, зелёным до и после; список перенесённых тестов — в
   PR.
4. Имена кода и тестов без номеров задач и требований: ID живут только в файлах этой спеки
   (`cargo xtask markers`). Правки файлов — инструментами Read/Edit/Write; массовые механические
   правки — `git mv`, `sd`, `ast-grep`, `rg`, не скрипты на Python, Perl или PowerShell.
5. PR, меняющий поверхность для потребителя, добавляет `changelog.d/<branch-slug>.md`
   (`### Added|Changed|Deprecated|Removed|Fixed|Security`).
6. Новая публичная поверхность сливается вместе с первым production-потребителем (R10.4). Если
   потребитель — следующая задача, PR называет её.
7. Изменение поддержки платформы обновляет `docs/platform-parity.md` и `docs/gaps.md` в том же PR;
   ✅ — только после прогона на этой ОС.
8. Файлы активной ветки не трогаются до её слияния или без согласия владельца. Сейчас их держат:
   send-flip (ядро) — `flui-scheduler`, `flui-runtime/src/{realm_services,presentation}.rs`,
   `ui_realm/*`, `flui-view/src/owner/*`; text-ime и teardown — `text_store/*`, Win32
   `text_services`, Win32 `platform.rs`; persistence — доставка `Storage`; LY8/LY9 — предпочтения.
   Это зависимости по порядку и записаны в колонке «Зависит от», а не окна по датам.
9. **Где:** **L** — Linux CI; **W** — Windows нативно (локальный запуск, вывод в PR); **T** — только
   cross-typecheck или `wasm32` без запуска (так и пишется в PR); **Dev: <ОС>** — прогон на
   устройстве владельца: macOS, iOS, Android, Linux (Wayland, X11), Web (браузер); датированный
   вывод — в PR. «L» без остального — всё доказательство даёт Linux CI.
10. Каждый Rust-набросок в ADR и design собран в scratch-крейте, вывод сборки — в PR. Зелёный CI на
    head — условие слияния.

## Предпосылки

Серия начинается, когда:

1. владелец утвердил design.md и requirements.md редакции 3;
2. ADR-0151…0154, ADR-0254 (контракт окна: D4, D5, D10, D11, D12) и ADR-0255 (хосты Android, iOS
   и Web, мост к хосту: D14–D16, D19, D23) приняты вместе с этой редакцией, и `Superseded-by` в
   старых ADR (design §10) уже проставлены. Каждый записан как «не реализовано»: задачи ниже
   исполняют их.

Эти два пункта — вход серии, а не задачи в ней; на графике они узел PRE. Формы, которые ADR
оставляют задаче и которые задача называет в своём PR: набор материалов кроссплатформенной команды
фона (ADR-0254 п. 3), слот типизированных опций одной ОС в `WindowOptions` (п. 8), вход ядра для
`AndroidApp`.

## Граф

```mermaid
graph LR
  PRE["Предпосылки: design, ADR-0151…0154, 0254, 0255"]
  SF["send-flip (ядро)"]
  TI["text-ime, teardown"]
  SP8["ADR-0172: LY8, LY9"]
  PS["persistence"]
  DR["спека flui-driver"]

  subgraph rename["Переименование"]
    LY20; LY21
  end
  subgraph core["Устройство ядра-хоста"]
    LY22; LY23; LY24; LY25; LY26; LY27; LY28; LY29; LY30; LY31; LY32; LY33; LY34; LY35; LY36; LY37
  end
  subgraph contract["Контракт"]
    LY38; LY39; LY40; LY41; LY42; LY43; LY44; LY45; LY46; LY47; LY48; LY49
  end
  subgraph seam["Шов и крейты возможностей"]
    LY50; LY51; LY52; LY53; LY54; LY55; LY56; LY57; LY58; LY59; LY60; LY61
  end
  subgraph hosts["Хосты"]
    LY62; LY63; LY64; LY65; LY66; LY67; LY68; LY69; LY70; LY71; LY72; LY73; LY74; LY75; LY76
  end
  subgraph linux["Нативный Linux"]
    LY78; LY79; LY80; LY81; LY82; LY83; LY84; LY85; LY86; LY87
  end
  subgraph quality["Планка качества"]
    LY88; LY89; LY90; LY91; LY92; LY93; LY94; LY95; LY96; LY97; LY98; LY99; LY100; LY101; LY102
  end

  PRE --> LY20 --> LY21
  SF --> LY50
  TI --> LY47 & LY48
  SP8 --> LY22 & LY33 & LY45
  PS --> LY52
  DR --> LY99

  LY21 --> LY22 & LY23 & LY24 & LY39 & LY47 & LY48 & LY53
  LY24 --> LY22
  LY22 & LY23 --> LY25 --> LY26
  LY22 & LY26 --> LY27
  LY27 --> LY28 & LY30 & LY31 & LY32 & LY33
  LY28 --> LY29
  LY27 & LY28 & LY29 & LY30 & LY31 & LY32 & LY33 --> LY34
  LY34 --> LY35 & LY38 & LY49
  LY34 & LY39 --> LY36 --> LY37

  LY38 --> LY40
  LY40 & LY26 --> LY41 --> LY42 --> LY43
  LY41 --> LY44 & LY49
  LY44 --> LY45 & LY46

  LY38 & LY46 & LY47 --> LY50
  LY50 --> LY52 & LY54
  LY47 & LY48 & LY49 & LY50 --> LY55
  LY53 --> LY56
  LY50 --> LY51
  LY51 & LY53 & LY54 & LY56 --> LY57
  LY57 --> LY58 & LY59 & LY60
  LY55 & LY57 & LY60 --> LY61

  LY28 & LY33 & LY45 & LY46 & LY56 --> LY62 --> LY63
  LY50 & LY62 --> LY64
  LY32 & LY62 --> LY65
  LY62 & LY41 --> LY66
  LY57 & LY64 --> LY67
  LY30 & LY46 --> LY68 --> LY69
  LY32 & LY46 --> LY70
  LY29 & LY56 --> LY71
  LY28 & LY66 --> LY72
  LY57 & LY28 --> LY73
  LY28 & LY30 & LY46 & LY56 & LY66 --> LY74 --> LY75
  LY32 & LY75 --> LY76

  LY28 & LY31 & LY45 & LY46 --> LY78 & LY82
  LY29 & LY78 --> LY79
  LY32 & LY78 --> LY81
  LY30 & LY32 & LY50 & LY78 --> LY80
  LY29 & LY82 --> LY83
  LY30 & LY32 & LY50 & LY82 --> LY84
  LY79 & LY80 & LY81 & LY83 & LY84 --> LY85 --> LY86 --> LY87

  LY32 & LY34 --> LY88
  SP8 --> LY89
  LY33 & LY62 --> LY89
  LY26 & LY34 --> LY90
  LY46 & LY81 --> LY91
  LY41 & LY46 --> LY92
  LY57 & LY58 & LY59 & LY67 & LY73 --> LY93
  LY30 & LY32 & LY50 & LY69 --> LY94
  LY28 & LY81 --> LY95
  LY65 & LY70 & LY76 & LY85 & LY88 --> LY96
  LY41 & LY44 & LY45 --> LY97
  LY26 & LY34 --> LY98 --> LY99
  LY44 & LY50 --> LY100
  LY57 --> LY101\n  LY100 & LY101 --> LY102
```

## Задачи

### Переименование

| ID | Задача | Зависит от | Крейты и файлы | Гейты | Тесты | Где |
|---|---|---|---|---|---|---|
| **LY20** | **Переименование ядра:** `crates/flui-platform` → `crates/flui-native` (D21, design §9.1). `git mv` и `sd` по целому токену, длинное совпадение первым; без ручных правок, кроме ловушек: пакет, библиотека, пути, фичи (`flui-native/a11y`), `allowed-dependents`, `reach.tier.K.forbid` (называет `flui-native`; нового `flui-platform` в нём нет: контракт — tier C), `globals`, пути xtask, `package(flui-native)` в nextest, `--exclude flui-native` в `weekly.yml`, цели `tracing` и `RUST_LOG` (`flui_native=debug` в job’ах CI), имя потока `flui-native-bg`; trybuild `.stderr` перегенерируются (`TRYBUILD=overwrite`), не заменяются текстом. Не трогать: `flui_native_recording`, `flui-native-library-*`, архивный `docs/plans`, текст ADR-0151, выпущенные записи `CHANGELOG.md`, имя файла ADR-0082, Java-пакет `dev.flui.platform`, цель `"flui.platform"`. Тексты об отношении двух крейтов (README, ARCHITECTURE.md, `docs/crates.md`, `docs/architecture.md`, `docs/testing.md`, `design/*`) переписать по смыслу. Точные команды — в описании PR. Реэкспорты контракта через ядро читаются `flui_native::WindowOptions` до LY23 | PRE | весь workspace: корневой `Cargo.toml`; `crates/{flui-app,flui-runtime,flui-testing}`; `examples/web_demo`; `tools/xtask` (`tasks.rs`, `change_scope/lane_args.rs`, `workspace/reach*`, `globals*`, `platform_floor.rs`, `android_dex.rs`, allowlists `docs-paths`, `markers`, `testing`); `.github`; `.config/nextest.toml`; `clippy.toml`, `deny.toml`, `typos.toml`; `AGENTS.md`; `docs/*`, `design/*` | `workspace`, `reach`, `globals`, `checks`, `deps`, `cross-typecheck`, `wasm-check`, `key-vocabulary`, `platform-floor`, `android-dex`, `docs-paths`, `docs-links` | весь набор без правок смысла; `compile_fail` перегенерирован; повторный прогон команд PR на его выходе даёт пустой diff | L + T |
| **LY21** | **Переименование контракта:** `crates/flui-platform-api` → `crates/flui-platform` (D21, design §9.2). `flui_platform_api` → `flui_platform`; `SDK_SURFACE` в `tools/xtask/src/workspace/tiers.rs`; генератор `key_vocabulary` (пути записи `src/keyboard/{named_key,code}.rs` и строка `use …::keyboard::{name}` в сгенерированных документах: файлы под git совпадают с выводом генератора); `recording_vocabulary` (26 путей); `binary_id(flui-platform::compile_fail)` в nextest; фичи `flui-platform/serde` (`flui-interaction`, корневой `serde`); `documentation = "https://docs.rs/flui-platform"`; цели `tracing` `flui_platform_api::input_regions`; исключения путей в `typos.toml`; фикстуры `reach` в xtask; README и ARCHITECTURE.md контракта. Фасад `flui::platform` не меняется. Те же приёмы и те же запреты, что в LY20. Начинается с разовой проверки (не гейт): в неархивных файлах не осталось токенов `flui-platform` и `flui_platform` (кроме ADR-0151), иначе строка вроде `--exclude flui-platform` из ветки, написанной до LY20, молча нацелится на контракт; ветка, вливающаяся после LY21, применяет обе карты по порядку | LY20 | весь workspace (9 манифестов с `flui-platform-api = { workspace = true }`, 545 `.rs`-файлов с `flui_platform_api::`), `tools/xtask` (`workspace/tiers.rs`, `key_vocabulary.rs`, `recording_vocabulary.rs`, `workspace/reach*`), `.config/nextest.toml`, `typos.toml`, `docs/*` | те же, что у LY20 | весь набор; `key-vocabulary` и `recording-vocabulary` проверяют сгенерированное; пустой diff при повторном прогоне | L + T |

### Устройство ядра-хоста

Порядок: механизм хоста (LY22), основа `flui-native` (LY23–LY26), цикл каждой платформы в `owner/` (LY27), затем по одной фиче на задачу (LY28–LY33), уборка (LY34) и то, что стоит на готовой раскладке (LY35–LY37). Задачи переноса LY27–LY34 не меняют поведения: перенесённые тесты идут без правок (правило 3), а каталог входит в проверку `module-dag` в той задаче, которая его создаёт. В колонке «Задача» запись «A → B» называет сегодняшний путь относительно `crates/flui-native/src/` (файлы платформ лежат под `platforms/`, общий код — под `shared/`) и путь раскладки design §6; число в скобках — строки файла сегодня. Файл платформы, которого у платформы нет сегодня (drag на macOS и iOS, доступность Android, iOS и Web), не создаётся: позицию таблицы называет `Absent<F>`, пока задача хоста (LY62–LY76, LY78–LY84) не принесёт файл.

| ID | Задача | Зависит от | Крейты и файлы | Гейты | Тесты | Где |
|---|---|---|---|---|---|---|
| **LY22** [P] | **Механизм хоста** (D13, D30, R4.2, R4.3, R4.5, R4.11). `host.rs`: `trait Host` с ассоциированным типом на каждую позицию закрытого списка (`Loop`, `Window`, `Displays`, `Input`, `Text`, `Accessibility`, `Clipboard`, `Drag`, `Haptics`, `Preferences`, `Lifecycle`, `Chrome`, `Storage`; граница позиции — трейт её фичи) и `Absent<F>` — единственный тип «на этой платформе нет»: он отвечает `Unsupported { NotOnThisPlatform }` и реализует трейт каждой фичи. `platform.rs`: `HostPlatform<H: Host>` — единственная реализация `Platform` (проводка `AppHandler`, окон и дисплеев). Семь нынешних `impl Platform` (`WindowsPlatform`, `MacOSPlatform`, `IOSPlatform`, `AndroidPlatform`, `WebPlatform`, `WinitPlatform`, `HeadlessPlatform`) разбираются по позициям: объект позиции остаётся в своём файле, общая обвязка уходит в `HostPlatform<H>`. `hosts/{windows,apple,android,web,winit,headless}.rs` — `impl Host for <Os>` и конструктор, больше ничего; пока файлы лежат под `platforms/`, таблицы называют их сегодняшние типы, а LY27–LY33 меняют в таблицах пути, и позиция, которую сегодня держит loop хоста, получает свой тип, когда фича забирает своё состояние. Позиция, которой у платформы сегодня нет (drag на macOS и iOS, доступность Android, iOS и Web; ввод текста у winit есть — `WinitTextInput`), — `Absent<F>`, а не умолчание метода. `current_platform()` возвращает `Box<dyn Platform>`; трейт — таблица, не generic-ядро: модели фич им не параметризованы. `cargo xtask module-dag` распространён на `flui-native`: `owner/` и `version/` не зависят ни от одной фичи; файл платформы в фиче зависит от модели своей фичи, `owner/`, `version/` и моделей других фич, никогда от файла платформы другой фичи, от файла другой платформы и от `hosts/` (мультиплексоры ОС диспетчируют через трейты-хуки `owner/`, которые регистрирует таблица); `hosts/` зависит от всех фич, от него не зависит никто; исключения — `macos`/`ios` → `apple/mod.rs` и `wayland`/`x11` → `linux/mod.rs` той же фичи и, до LY87, `display/winit.rs` → `display/linux/` (на Linux снимок дисплеев берётся у композитора или X-сервера напрямую, мимо winit); каталог входит в проверку той задачей, которая его создаёт; `--self-test` сажает нарушение на каждое правило. Чистая часть файла платформы — модуль под `cfg(any(<os>, test))` с `forbid(unsafe_code)`, FFI — под `cfg(<os>)`; в scratch-крейте доказаны: посаженный `unsafe` в чистом модуле не компилируется, `const _: () = assert!(..)` в FFI вычисляется `cross-typecheck`, таблица без одного ассоциированного типа не компилируется. AGENTS.md «Definition of Done»: тесты моделей и чистых частей ядра-хоста — модулями рядом (`pub(crate)`), `tests/` хранит conformance и записи нативных трасс. `docs/extending.md`: строка «новая ОС» — слово из словаря std, файл в каждой фиче и таблица `hosts/<os>.rs`, которая начинается с `Absent<F>` во всех незаполненных позициях (строки parity новой платформы читаются из неё, и платформа готова, когда `Absent` не осталось и conformance-строки прошли на ней), нижняя граница в `platform-support.toml`, строки gaps, цель CI, записи `globals`; внешние хосты ждут встраивателя, а не разделения крейтов | LY21, LY24, LY9 | `crates/flui-native/src/{host.rs,platform.rs,hosts/*,lib.rs}`, `impl Platform` в `platforms/*`, `tools/xtask/src/module_dag*`, `crates/flui-native/Cargo.toml` (`[package.metadata.flui.modules]`), `AGENTS.md`, `docs/extending.md`, scratch-крейт для доказательств | `module-dag` (+ `--self-test`), `workspace`, `globals`, `docs-paths`, `checks` | самотест `module-dag`: по плантированному нарушению на каждое правило; trybuild: таблица без одного ассоциированного типа не компилируется (E0046); `rg "impl.* Platform for"` по ядру даёт одну строку; существующие наборы без правок; вывод scratch-доказательств в PR | L + W + T; Dev: macOS, iOS, Android, Web |
| **LY23** [P] | **Ядро перестаёт реэкспортировать имена контракта** (D12, R2.9). Удалить список из 64 имён в `traits/mod.rs` и около 50 в корне `lib.rs`; `flui-app` (около 249 мест), `flui-runtime` (26), `flui-testing`, `flui-sdk`, `examples`, `packages` импортируют контрактные имена из `flui_platform`, свои — из `flui_native`. В README и ARCHITECTURE.md обоих крейтов убрана фраза «реэкспортирует всё по старым путям» | LY21 | `crates/flui-native/src/{lib.rs,traits/mod.rs}`; `crates/{flui-app,flui-runtime,flui-testing,flui-sdk}`; `examples/*`; `packages/*` | `workspace`, `reach`, `deps` | компиляция workspace и существующий набор; `rg` по префиксу `flui_native::` с именами контракта (`WindowOptions`, `HostNotice`, `SessionEnd`, `WindowMode`, `WindowReveal`, `WindowCommand`) пуст | L + T |
| **LY24** [P] | **Мёртвое Linux:** удалить `LinuxPlatform` (274 строки, `unimplemented!()` в `run`, `quit`, `open_window`, …) и `LinuxWindowExt` (`window_ext.rs`, 510 строк, нет ни одной реализации) вместе с экспортами `lib.rs:214` и `platforms/mod.rs:59`. Живой слой `platforms/linux` сохраняется: `display/` (Wayland и X11 RandR), `host_notices/` (logind, memory pressure), `lifecycle_model/`, `x11_window_probe.rs`, `accessibility.rs` (AT-SPI). Единственный живой вызов `LinuxPlatform::is_wayland()` (`winit/display_linux.rs:127`) становится функцией этого слоя | LY21 | `crates/flui-native/src/{lib.rs,platforms/mod.rs}`, `platforms/linux/{mod.rs,window_ext.rs}`, `platforms/winit/display_linux.rs` | `workspace`, `globals`; `rg 'unimplemented!'` по `platforms/` | существующие тесты слоя Linux; `winit` по-прежнему выбирается на Linux | L |
| **LY25** | **`traits/` и `owner/`** (D13). `traits/platform.rs` → `platform.rs` рядом с `HostPlatform<H>` из LY22 (`Platform` запечатан приватным супертрейтом, D6; единственная реализация — `HostPlatform<H>`), `traits/host_window.rs` → `host_window.rs` (`HostWindow` запечатан), `traits/owner.rs` (992 строки) и `shared/{owner_signal,owner_slot,panic_boundary}` → `owner/` (`OwnerPlatform`, `SharedPlatform`, `PlatformProxy`, сигнал и слот owner-потока, граница паники); `traits/` исчезает; пять записей `globals` под `shared::` и пути `flui_native::traits::…` (65 мест в 31 файле) заменяются. `current_platform()` остаётся единственной лестницей выбора хоста в `lib.rs`; `owner/` входит в проверку `module-dag` | LY22, LY23 | `crates/flui-native/src/{lib.rs,platform.rs,host_window.rs,owner/*}`, `Cargo.toml` (`globals`), `crates/flui-app`, `tools/xtask/allowlists/*` | `module-dag`, `globals`, `workspace`, `reach` | trybuild: внешний `impl Platform` и `impl HostWindow` не компилируются (`tests/ui/*.stderr` перегенерированы); существующий набор | L + T |
| **LY26** | **Одна точка вызова пользовательского кода** (D13, R4.6). В `owner/` единственная функция берёт обработчик из слота, отпускает локи и заёмы, вызывает, возвращает на место (или оставляет замену, сделанную во время вызова) и держит границу паники: первый отказ не теряется (`thread::panicking()` после catch ложно). `shared/handlers.rs` (2 497 строк: `PlatformHandlers`, `WindowCallbacks`, `impl_window_callback_setters!`) переезжает в `owner/` и вызывает только через неё, до LY44 и LY45, которые его удаляют. Удалить шесть аренд (`RunningSessionEndHook`, `CallbackLease`, `InputLease`, `WindowEventLease`, `WindowEventHandlerLease`, `ExitPolicyHookLease`) и ручной take/restore (macOS `loop_control` 8 мест, iOS `platform.rs` 15, Web `platform.rs` 8). Исправить пользовательский код под локом: Android `mod.rs:1010` (`invoke_quit` под мьютексом обработчиков), headless `create_mock_window` (`WindowEvent::Created` под мьютексом состояния) и `Drop` заменённого обработчика под локом (Android `mod.rs:1098,1102,1116`, iOS `platform.rs:514,518,528`, headless `platform.rs:483,525,531,550,584`) | LY25 | `crates/flui-native/src/owner/*`, `platforms/{android/mod.rs,ios/platform.rs,web/platform.rs,macos/loop_control.rs,headless/platform.rs,winit/platform.rs,windows/*}` | `module-dag`, `globals`, `file-length`, `markers` | на каждом хосте: обработчик, который изнутри вызова ставит новый обработчик, завершает приложение, открывает окно; обработчик, чей `Drop` вызывает платформу; паника обработчика не теряет первый отказ; все падают без исправления | L + W + T; Dev: Android (выход по системе) |
| **LY27** | **`owner/`: цикл каждой платформы** (D13, R4.1, R4.8). Цикл, пробуждения и владение потоком выходят из файлов платформ в `owner/`; фич они не называют. Windows: цикл сообщений из `windows/platform.rs` (3 083; делится в пределах 3 000 строк, остальное уходит по фичам в LY28–LY33), `windows/platform/{wake_deadline,modal_frame_tests}.rs`, `owner_control.rs` и `owner_control/wake.rs` (584 + 49), `modal_loop.rs` (88), `com_apartment.rs` (162), `util.rs` (125; туда, где используется) и широковещательное окно — скрытый top-level приёмник из `windows/preferences/receiver.rs` (241), который делят предпочтения, дисплеи и lifecycle, поэтому он лежит в `owner/`, а не в фиче, и сообщает им через трейт подписчика, а не по имени фичи, — в `owner/windows.rs`. Там же — класс окна и сырая оконная процедура с трейтом-хуком сообщений, который реализуют файлы платформы фич (окно, ввод, текст, drag, lifecycle) и регистрирует `hosts/windows.rs`; viewport DirectManipulation создаётся тем же хуком при создании окна. Apple: `macos/{platform,loop_control,owner_lane,wake_pump}.rs` (443 + 666 + 267 + 292: цикл AppKit, делегат) и `ios/{platform,native_owner,scene,executor}.rs` (1 243 + 33 + 95 + 45: `UIApplicationMain`, делегат, сцены; `IOSSceneEvent` и `IOSSceneSessionId`, которые называет `flui-app`, остаются `pub` ядра) — в `owner/apple.rs`. Android: `android/mod.rs` (1 181: looper, `AndroidApp`, `JavaVM`), `android/java_view.rs` (693) и `shared/android_ndk` (513) — в `owner/android.rs`; путь `include_bytes!` на `classes.dex` и `tools/xtask/src/android_dex.rs` пересчитываются здесь. Web: `web/{platform,listener,executor}.rs` (488 + 53 + 36: цикл `requestAnimationFrame`) — в `owner/web.rs`. winit: `winit/{platform,control}.rs` (3 095 + 537; делится) и `winit/platform/real_loop_tests.rs` (1 998) — в `owner/winit.rs`. headless: ходы владельца из `headless/platform.rs` (2 454) — в `owner/headless.rs`. `redraw_poll.rs` (93) — в `owner/redraw_poll.rs`. Таблицы `hosts/*` называют новые типы цикла; записи `globals` переезжают; ARCHITECTURE.md приводится к новому устройству. Циклов Linux здесь нет: нативные Wayland и X11 приносят свои в LY78 и LY82 | LY22, LY26 | `crates/flui-native/src/owner/*`, `platforms/{windows,macos,ios,android,web,winit,headless}/*` (уходят перенесённые файлы), `shared/android_ndk.rs`, `redraw_poll.rs`, `hosts/*`, `Cargo.toml` (`globals`), `tools/xtask/src/android_dex.rs` | `module-dag`, `file-length`, `globals`, `android-dex`, `cross-typecheck`, `markers` | перенесённые тесты (`wake_deadline`, `modal_frame_tests`, `real_loop_tests`) без правок; `tests/` conformance и трассы Windows без правок; `cargo xtask android-dex` проверяет те же байты | L + W + T; Dev: macOS, iOS (Simulator), Android (ручной прогон ввода без изменения результата) |
| **LY28** [P] | **`window/`** (D13, R4.1, R4.8). Модель: `shared/window_state` (299; единственный `mode_of`) → `window/state.rs`, `shared/visibility` (130) → `window/visibility.rs`, `shared/window_installation` (92; его называет `flui-app`) → `window/installation.rs` (публичные имена — от корня крейта, `flui-app` называет `flui_native::WindowInstallation`; цикл iOS ждёт установку через свой хук `SceneInstallation`), `config.rs` (240) → `window/config.rs`. Windows: `windows/{window,window_ext,window_frame,window_limits,window_modes,window_placement,window_visibility,appearance}.rs` (3 176 + 368 + 291 + 192 + 94 + 342 + 256 + 58) и `shared/windows_window_limits` (333) → `window/windows/{mod,limits,window,set,proc,backdrop}.rs` (`mod.rs` — корень под `cfg(any(windows, test))`: чистые правила размеров `limits.rs` с `forbid(unsafe_code)` собираются и тестируются на любом хосте, FFI-половины — под `cfg(windows)`; тело окна делится на `window.rs` и `set.rs` (набор окон и расстановка) по пределу 3 000 строк; класс окна и сырая оконная процедура живут в `owner/windows.rs` (LY27), `proc.rs` реализует её хук для сообщений самого окна (`WindowProc`), сообщения ввода — хук `InputMessages` в `platforms/windows/input_messages.rs` до LY29, текст, drag и lifecycle — свои хуки в своих файлах, а `hosts/windows.rs` их регистрирует; каждый хук перечисляет свои сообщения (`WindowMessages::answers`), и наборы хуков таблицы не пересекаются). `shared/hwnd_affinity` (286) — машинерия владельца, не окна: правила привязки окна Win32 к потоку применяют контрол владельца и цикл → `owner/hwnd_affinity.rs` (под `cfg(any(windows, test))`). DirectManipulation (`windows/direct_manipulation.rs`, 758, его `tests.rs` и `shared/direct_manipulation`, 495) — источник ввода и переезжает в LY29. Apple: `macos/{window,window_tiling,window_ext,display_pass,liquid_glass}.rs` (2 994 + 413 + 306 + 85 + 243), `ios/{window,status_bar}.rs` (1 350 + 174) и `shared/{appkit_window_geometry,appkit_window_mode,screen_edges}` (372 + 409 + 43) → `window/apple/{mod,macos,ios,geometry,liquid_glass}.rs` и `window/apple/macos/{ext,delegate}.rs` (`macos.rs` держит окно с его командами и `display_pass`; публичное расширение `MacOSWindowExt` с раскладкой `tiling` и делегат `NSWindowDelegate` — отдельные файлы по пределу 3 000 строк; общее у macOS и iOS — `apple/mod.rs`). Android: `android/{window,safe_area,gesture_exclusion,system_chrome}.rs` (700 + 204 + 63 + 122) → `window/android.rs`. Web: `web/{window,registration}.rs` (881 + 531) → `window/web.rs`. winit: `winit/{window,window_plan}.rs` (914 + 831) → `window/winit.rs`. Linux (живой слой): `linux/x11_window_probe.rs` (387) с тестами и `linux/lifecycle_model/x11_window_mode.rs` (238) → `window/linux/x11.rs`. headless: `headless/window_policy.rs` (36) и `MockWindow` из `headless/platform.rs` → `window/headless.rs`. Системный chrome окна (`android/system_chrome.rs`, `ios/status_bar.rs`, `FakeSystemChrome`) живёт в тех же файлах: позицию `Chrome` таблицы называет `window/`. `liquid_glass` и `MacOSWindowExt` остаются внутри `window/apple/` до LY36. Шов: позиция окна называет общее окон платформы, `host::Window::Shared` (`WindowSet` Win32, конфигурация macOS, фабрика окон UIKit, рамка и вид оконного менеджера winit, модель окон headless, `()` на Android и Web); конструктор хоста строит его один раз, `HostPlatform::from_parts(shared, |shared| Loop::new(..))` (`try_from_parts` для цикла, который может не построиться), и цикл получает тот же экземпляр как хук, который сам объявляет. Хуки циклов объявляет `owner/`, реализует `window/`: `owner::handlers::{CallbackWindow, OpenWindows}`, `owner::windows::ClassWindows`, `owner::apple::macos::WindowRegistry`, `owner::apple::ios::{SceneWindow, SceneWindows, SceneConsumer, SceneInstallation}`, `owner::android::ActivityWindow`, `owner::winit::{LoopWindow, LoopWindows}` (цикл нумерует, регистрирует и доводит окно, хук только строит его из `WindowSeed`), `owner::headless::TrackedWindows`; `owner/` не называет `window/` и в тестах. Рёбра `window/` в `platforms/` и между файлами текста и окна одной платформы — датированные исключения `module-dag` (`exit = "ADR-0151"`), каждое уходит в задаче своей фичи (строки LY29, LY30, LY33); рёбра окна в файлы `accessibility/`, `haptics/` и `drag/` (LY32 влит раньше: окно делает объект фичи на окно) — тоже исключения, их снимает LY38, где оконные позиции таблицы становятся тем, что отдаёт окно. Исключения `retiring` LY32, ребро которых уходит с файлом окна, сняты: `drag::windows` → `platforms::windows::platform` и `haptics::windows` → `platforms::windows::platform` — `WindowRef` стал словарём владельца (`owner::windows::WindowRef`; `with_context` — метод окна в `window/windows/proc.rs`), `drag::web` → `platforms::web::registration` — хук `owner::web::HostDrag::register` получает регистрацию холста как `&mut dyn owner::web::CanvasListeners`, который реализует `window::web::Registration`; три ребра окна в `display/` (iOS, macOS, headless) уходят, когда хук дисплеев своего цикла несёт то, что окно читает. LY33 влит раньше, и окно называет его файлы платформ тоже через исключения: `window::windows` → `preferences::windows` (`mouse_geometry`), `window::windows` → `lifecycle::windows` (`messages`: разбор `WM_WTSSESSION_CHANGE`), `window::android` → `preferences::android` (`night_mode`, `pixel_ratio`, `geometry`); каждое уходит, когда фича отдаёт это через модель или хук `owner/`. Зонд X11 (`WinitLoop::x11_probe`, `WinitApp::refresh_x11_windows`) ушёл в окно: соединения и ячейка — `window/linux/x11.rs` за трейтом модели `WindowManagerView`, окна, которые менеджер окон изменил без события winit, отдаёт хук `LoopWindows::take_unreported`, а перечитывает их сам цикл; в `platforms/winit/linux_unreported.rs` осталась проверка дисплеев. Сигнальная половина правила Activity (`ActivitySignal` в `lifecycle::android::rule`, оконные ветки `ActivityTarget` у `AndroidLoop`, `publish_activity_fact`) в этой ветке не переехала в `window/android.rs`: это меняет путь событий Activity и требует своего прогона на устройстве; записи `globals` переезжают | LY27 | `crates/flui-native/src/window/*`, `platforms/{windows,macos,ios,android,web,winit,linux,headless}/*` (уходят перенесённые файлы), `shared/*` (уходят), `config.rs` (уходит), `tests/*` (уходят перенесённые), `hosts/*`, `Cargo.toml` (`globals`) | `module-dag`, `file-length`, `globals`, `cross-typecheck`, `markers` | перенесённые тесты правил (`hwnd_affinity`, `appkit_window_*`, `screen_edges`, `window_state`, `window_installation`) зелёные на Linux; `tests/` conformance и трассы Windows без правок | L + W + T; Dev: macOS, iOS (Simulator), Android |
| **LY29** | **`input/`** (D13, R4.1, R4.8). Модель: `shared/pointer_assembly/` (9 файлов, 2 729) → `input/assembly/`, `shared/{keys,keyboard_adapter}` (526 + 162) → `input/keys.rs`, `shared/input_availability` (152) → `input/availability.rs`, `shared/scroll` (38) → `input/scroll.rs`, `shared/events` (30: метка времени события и идентичность первичной мыши; её называет `flui-app` в `owner_dispatch.rs`) → `input/mod.rs`. Windows: `windows/events.rs` (1 734) и `windows/platform/{pointer_messages,keystroke_queue}.rs` (144 + 111) → `input/windows/{mod,raw}.rs`; `windows/direct_manipulation.rs` (758), его `tests.rs` и `shared/direct_manipulation` (495) → `input/windows/direct_manipulation.rs` (viewport на окно создаётся хуком `owner/windows.rs` при создании окна; его колбэки — источник pan и zoom для сборки указателя). Apple: `macos/{events,view}.rs` (524 + 755), `ios/events.rs` (284) и `shared/keys_macos` (401) → `input/apple.rs`. Android: `android/input.rs` (550) и `shared/{android_motion,android_keys,android_scroll}` (923 + 45 + 149) → `input/android.rs`. Web: `web/events.rs` (1 544) и `shared/web_pinch` (341) → `input/web.rs`. winit: `winit/events.rs` (1 670) → `input/winit.rs`; тест `winit/events/keyboard_tests.rs` (775) на таблицу `keys_macos` переезжает в `input/apple.rs`: файл платформы не называет другую платформу. headless: `HeadlessInputDevices` из `headless/platform.rs` → `input/headless.rs`. Форма доступа к `assembly/` для отдельного тестового бинаря со счётчиком аллокаций и признака `native-capture` решается в PR. Обратные рёбра, которые оставил LY28 (исключения `module-dag`), уходят здесь: хук `InputMessages` (`platforms/windows/input_messages.rs`: мышь, указатель, колесо, клавиатура, символы, раскладка) → `input/windows/`; `window::windows` → `platforms::windows::{events, direct_manipulation}` (позиции курсора и часы сообщений в процедуре окна; слот viewport тачпада в контексте окна), `window::apple::macos` → `platforms::macos::view` (content view принимает ввод), `window::apple::ios` → `platforms::ios::events`, `window::web` → `platforms::web::events` (слушатели ввода на canvas), а также `text::android` → `platforms::android::input` и `text::web` → `platforms::web::events` из LY30. Целевая форма: состояние фичи на окно (viewport, часы сообщений, слушатели) живёт у фичи по `WindowIdentity`, заводится её хуком `created` и снимается при уничтожении окна; окно его не строит и не держит, а `owner/` даёт хук уничтожения рядом с `created`, если его ещё нет. Ребро в файл LY33, которое снимает переезд ввода (через модель `preferences` или хук `owner/`): `platforms::windows::events` → `preferences::windows::mouse_geometry` (геометрия мыши для колеса и жестов). Снимает исключения `retiring` (LY32) `drag::windows` → `platforms::windows::events` (`MOUSE_BUTTONS`) и `drag::web` → `platforms::web::events` (`answer`, `pointer_position`): что drag берёт у ввода, становится моделью `input/`. Записи `globals` переезжают | LY28 | `crates/flui-native/src/input/*`, `platforms/{windows,macos,ios,android,web,winit,headless}/*` (уходят перенесённые файлы), `shared/*` (уходят), `tests/*` (уходят перенесённые), `crates/flui-app/src/app/runner/owner_dispatch.rs`, `Cargo.toml` (`globals`) | `module-dag`, `file-length`, `globals`, `cross-typecheck`, `markers` | перенесённые тесты (`pointer_assembly`, `direct_manipulation`, `android_motion`, `android_keys`, `web_pinch`, `input_capabilities`) рядом с моделью и чистыми частями; записи нативных трасс проходят без правок | L + W + T; Dev: macOS, iOS, Android, Web |
| **LY30** [P] | **`text/`** (D13, R4.1, R4.8). Модель: `shared/ime_text` (45; зеркало текстового поля Android и Web) → `text/ime.rs`, `text_sizing.rs` (202) → `text/sizing.rs`. Windows: `windows/text_services/{mod,document,input_scope,probe,tests}.rs` (4 534, из них тесты 1 361) и `shared/text_geometry` (143) → `text/windows/{mod.rs,tsf/}`. Apple: `macos/text_input.rs` (781) → `text/apple/macos.rs`, `ios/text_sizing.rs` (58) → `text/apple/ios.rs`. Android: `android/{text_input,text_sizing}.rs` (516 + 619) и `shared/android_ime` (399) → `text/android.rs`. Web: `web/text_input.rs` (1 080) и `shared/web_text` (105) → `text/web.rs`. headless: `FakeTextInput` из `headless/platform.rs` → `text/headless.rs`. winit отдаёт `WinitTextInput` (разрешение IME и область курсора) → `text/winit.rs`, позиция `Text` его таблицы называет его; позиция `Text` получает трейт фичи текста вместо маркера `host::Text`. Тесты `android_ime`, `web_text`, `text_input_mapping` — рядом с моделью и чистыми частями; записи `globals` переезжают. LY30 влит раньше LY28, и с окном его связывают рёбра между файлами одной платформы (исключения `module-dag`): окно строит текстовый ввод и снимает размер текста (`window::{windows, apple::macos, apple::ios, android, web, winit, headless}` → `text::*`: контекст Win32 держит соединение TSF, окно Android — поле IME и захват размера текста), а текст находит своё состояние в окне (`text::windows` → `window::windows`: `WindowRef`, `with_window_context`; `text::android` → `window::android`: работа метода ввода находит поле на окне; `text::web` → `window::web`: `EditorSlot`, `Listener`, `FocusSync`, `effective_focus`). Целевая форма та же, что у остальных фич: текст держит своё состояние на окно по `WindowIdentity`, заводит его в хуке `created` (как `Tsf` уже активирует TSF) и снимает при уничтожении окна; масштаб окна доходит до него через модель окна, а не через контекст | LY27 | `crates/flui-native/src/text/*`, `platforms/{windows,macos,ios,android,web,headless}/*` (уходят перенесённые файлы), `shared/*` (уходят), `text_sizing.rs` (уходит), `tests/*` (уходят перенесённые), `Cargo.toml` (`globals`) | `module-dag`, `file-length`, `globals`, `cross-typecheck` | перенесённые тесты `android_ime`, `web_text`, `text_input_mapping` зелёные на Linux; Win32 `text_services/{probe,tests}.rs` без правок смысла — W | L + W + T; Dev: macOS, Android, Web |
| **LY31** [P] | **`display/`** (D13, R4.1, R4.8). Модель: `shared/display_source` (460: реестр идентичностей и последний снимок, которые держит каждый нативный источник) → `display/source.rs`. Apple: `shared/{apple_display_identity,display_link_rate,display_period}` (253 + 304 + 61), `macos/display.rs` (272) и `ios/display.rs` (298) → `display/apple/{mod,macos,ios}.rs` (общее у macOS и iOS — `apple/mod.rs`). Windows: `windows/display.rs` (572), `windows/platform/display_message_tests.rs` (151) и `shared/windows_display` (184) → `display/windows.rs`. Android: `android/display.rs` (576) и `shared/android_display` (317) → `display/android.rs`. Web: `web/{display,display_events,features}.rs` (375 + 706 + 125) и `shared/{dppx_query,refresh_estimate,screen_identity,web_feature}` (102 + 560 + 187 + 165) → `display/web.rs`. winit: `winit/display_{linux,macos,windows}.rs` (151 + 70 + 65) → `display/winit.rs` (`display_linux.rs` читает источник Linux из `display/linux/`: единственное ребро между файлами платформ, его называет `module-dag` поимённо до LY87). Linux (живой слой): `linux/display/*` (Wayland `wl_output` и `zxdg_output_v1`, X11 RandR; 11 файлов, около 3 500 строк) → `display/linux/`. headless: `headless/display.rs` (191) → `display/headless.rs`. Тесты правил и источников (`windows_display`, `refresh_estimate`, `screen_identity`, `dppx_query`, `display_link_rate`, `display_period`, `android_display`, `apple_display_identity`, `linux/display/*/tests.rs`) — рядом; записи `globals` переезжают. Проверка дисплеев winit на Linux из `platforms/winit/linux_unreported.rs` (`WinitApp::sync_unannounced_displays`, `moves_unannounced`: окно Wayland переходит на другой выход без события winit) → `display/winit.rs` | LY27 | `crates/flui-native/src/display/*`, `platforms/{windows,macos,ios,android,web,winit,linux,headless}/*` (уходят перенесённые файлы), `shared/*` (уходят), `tests/displays.rs`, `Cargo.toml` (`globals`) | `module-dag`, `file-length`, `globals`, `cross-typecheck`, `wasm-check` | перенесённые тесты правил и источников; `tests/displays.rs` без правок смысла; `wasm-test` и `web_browser` без правок смысла | L (+ wasm32) + W + T; Dev: macOS, iOS, Android, Web (Chrome) |
| **LY32** [P] | **`clipboard/`, `drag/`, `accessibility/`, `haptics/`** (D13, R4.1, R4.8). `clipboard/`: `shared/clipboard_lock` (88; общий замок Win32 и winit с `arboard`) → `lock.rs`; `windows/clipboard.rs` (456) → `windows.rs`; `macos/clipboard.rs` (245) и `ios/clipboard.rs` (71) → `apple.rs`; `web/clipboard.rs` (60) → `web.rs`; `winit/clipboard.rs` (128) → `winit.rs`; headless-приёмник в памяти → `headless.rs`; внутрипроцессная заглушка Android остаётся в его таблице до LY50. `drag/`: `windows/drag.rs` (785), `windows/drag/{com,tests}.rs` (557 + 1 395) и `shared/ole_drag_drop` (453) → `windows.rs`; `android/drag.rs` (556) и `shared/android_drag` (307) → `android.rs`; `web/drag.rs` (642) и `shared/html_drag_drop` (296) → `web.rs`; `winit/data_transfer.rs` (484) → `winit.rs`; `headless/drag.rs` (469) → `headless.rs`. `accessibility/`: `shared/accessibility_bridge` (114) → `bridge.rs`; `windows/accessibility.rs` (210) → `windows.rs`; `macos/accessibility.rs` (227) → `apple/macos.rs`; `linux/accessibility.rs` (189; AT-SPI, живой слой) → `linux.rs`; `FakeAccessibility` из `headless/platform.rs` → `headless.rs`. `haptics/`: `shared/haptic_degradation` (1 242) → `degradation.rs`; `windows/haptics.rs` (768) → `windows.rs`; `macos/haptics.rs` (58) и `ios/haptics.rs` (452) → `apple/{macos,ios}.rs`; `android/haptics.rs` (519) → `android.rs`; `web/haptics.rs` (103) → `web.rs`; `FakeHaptics` (headless-приёмник, LY48) → `headless.rs`. Позиции, которых у платформы нет сегодня (drag на macOS и iOS, доступность Android, iOS и Web, haptics Linux и winit), в таблицах — `Absent<F>`. Записи `globals` переезжают | LY27 | `crates/flui-native/src/{clipboard,drag,accessibility,haptics}/*`, `owner/*` (трейты хуков, которыми цикл держит объекты фич, и хост-трейты, которые их делают; состояние drag headless), `hosts/*`, `platforms/{windows,macos,ios,android,web,winit,linux,headless}/*` (уходят перенесённые файлы), `shared/*` (уходят), `tests/*` (уходят перенесённые), `Cargo.toml` (`globals`) | `module-dag`, `file-length`, `globals`, `cross-typecheck`, `markers` | перенесённые тесты (`ole_drag_drop`, `android_drag`, `html_drag_drop`, `haptic_degradation`) рядом с моделью и чистыми частями; тесты слоя Linux (AT-SPI) и `headless_drag` без правок | L + W + T; Dev: macOS, Android, Web |
| **LY33** [P] | **`preferences/` и `lifecycle/`** (D13, R4.1, R4.8). `preferences/`: `shared/preference_read` (196; допуск опроса, общий для ограниченных сэмплеров) → `read.rs`; Windows: `windows/preferences.rs` (457), `windows/preferences/{languages,tests}.rs` (180 + 279) и `windows/platform/preference_retry_tests.rs` (242) → `windows/` (приёмник широковещательных сообщений уже в `owner/windows.rs`, LY27: предпочтения подписываются на него через `owner/`); Apple: `macos/preferences.rs` (115) и `ios/preferences.rs` (128) → `apple/{macos,ios}.rs`; Android: `android/preferences.rs` (445) и `shared/android_text_weight` (82) → `android.rs`, а помощник Activity `with_activity` из него → `owner/android/activity.rs`, и с ним уходит исключение `module-dag` `display::android` → `platforms::android::preferences`; Web: `web/preferences.rs` (83) → `web.rs`; Linux: наблюдение портала Settings из `winit/preferences.rs` (239; D-Bus) → `linux.rs` (им пользуются winit, Wayland и X11); headless: производитель из LY8 в `headless/*` → `headless.rs`. `lifecycle/`: `shared/system_notice` (321; путь, которым уведомления любого хоста доходят до приложения) → `notice.rs`; Windows: `windows/host_events.rs` (481) и `shared/{windows_host_messages,windows_memory_watch}` (376 + 207) → `windows.rs`; Apple: `macos/system_notices.rs` (246), `shared/{terminate_gate,dispatch_memory_pressure}` (277 + 76) и `shared/apple_scene_error` (15; сбой уничтожения сцены UIKit) → `apple/{macos,ios}.rs`; Android: `shared/android_lifecycle` (446) и `android/memory.rs` (37) → `android.rs`; Web: `web/page_events.rs` (87) → `web.rs`; Linux (живой слой): `linux/host_notices/*` (logind, давление памяти), `linux/lifecycle_model/{mod,logind,memory_pressure}.rs` и `winit/platform/linux_notices.rs` (272) → `linux.rs`; headless: `HeadlessHostEvents` → `headless.rs`. Записи `globals` переезжают. Что переехало на деле: `shared/preferred_locales` → `preferences/locales.rs` (модель: его читают Web, Linux, macOS, iOS и Android); один файл на платформу, прежние файлы — его встроенные модули: `preferences/android.rs` (`configuration`, правила `contrast`, `text_weight`), `preferences/web.rs` (`browser`, правило `media` из `shared/browser_preferences`), `lifecycle/windows.rs` (`host_events`, правила `messages`, `memory_watch`; тесты `HostEvents` — `lifecycle/windows/tests.rs`), `lifecycle/apple/macos.rs` (`notices`, правило `memory_pressure`); Linux — `lifecycle/linux.rs` (входящий ящик и источник) и `lifecycle/linux/{logind,pressure}.rs`, каждый со своим правилом (`signals`, `triggers`), `logind/bus_tests.rs`; `preferences/linux.rs` — наблюдение портала без типов winit, связка с winit — свой блок `cfg(feature = "winit-backend")`, Wayland и X11 (LY78, LY82) запускают его так же. `shared/system_notice` → `lifecycle/notice.rs` за трейтом `owner::notices::NoticeChannel`, канал цикла делает таблица. Правило Activity (`shared/android_lifecycle`, `android/memory.rs`) → `lifecycle/android.rs` за хуком лупера `ActivityCallbacks`, словарь колбэков — `owner/android/activity_callbacks.rs`; цикл — позиция lifecycle, у macOS, iOS, Android и winit на Linux через одну обобщённую реализацию в `lifecycle/mod.rs`. В `owner/` остались `shared/terminate_gate` → `owner/apple/terminate_gate.rs` и `shared/apple_scene_error` → `owner/apple/ios/scene_error.rs` (ответ и сцены цикла). `winit/platform/linux_notices.rs`: доставка → `lifecycle::linux`, `WinitLoop::session_end` → `owner/winit/session_end.rs`, остаток (зонд X11 и проверка дисплеев) → `platforms/winit/linux_unreported.rs` (LY28, LY31). Окно (LY28, влито позже) читает эти фичи через исключения `module-dag`: `window::windows` → `preferences::windows` (геометрия мыши), `window::windows` → `lifecycle::windows` (правило `messages` для своей подписки на `WM_WTSSESSION_CHANGE`: каждое окно само регистрируется на уведомления сессии и по блокировке скрывает свою поверхность), `window::android` → `preferences::android` (ночной режим, плотность, геометрия касаний) | LY27, LY9 | `crates/flui-native/src/{preferences,lifecycle}/*`, `platforms/{windows,macos,ios,android,web,winit,linux,headless}/*` (уходят перенесённые файлы), `shared/*` (уходят), `Cargo.toml` (`globals`, features) | `module-dag`, `file-length`, `globals`, `markers` | перенесённые тесты правил и шины logind (`bus_tests`); `linux_preferences`, `preferences`, `windows_host_events` без правок смысла | L + W + T; Dev: macOS, Android, Linux (ручной прогон под X11) |
| **LY34** | **`version/`; `shared/` и `platforms/` исчезают** (D13, R4.1, R4.4). `shared/os_version/` (689; пороги и гейты версий ОС, ADR-0244) и `ios/os_version.rs` (18) → `version/gate.rs` (модули `windows`, `apple`, `android` внутри, под `cfg(any(<os>, test))`); генерируемый `shared/platform_floor.rs` → `version/floor.rs`, а `tools/xtask/src/platform_floor.rs` пишет новый путь; `version/` входит в проверку `module-dag` и не зависит от фич. `storage/` (`file_store.rs`, `page_store.rs`, `os_error.rs`, `user_dirs.rs`) остаётся как есть. Удаляются `shared/mod.rs` (29 строк `cfg`) и `platforms/mod.rs`; на верхнем уровне, кроме механизма хоста (`host.rs`, `platform.rs`, `host_window.rs`), остаются `error.rs`, `table_test.rs` и `executor.rs` с `task.rs`, которые удаляет LY61. `flui-app` называет установку окна от корня крейта (`flui_native::WindowInstallation`, LY28), так что `shared::window_installation` и псевдоним для него не остаются. 21 файл `tests/` с `flui_native::shared::*` исчезает: тесты правил — модулями рядом с моделями и чистыми частями, conformance и трассы — через публичный API. Записи `globals` (19, ключ — путь модуля) и пути в allowlists xtask переехали; `module-dag` охватывает все каталоги ядра; ключ `retiring` и каждое исключение в `platforms::*` удаляются (свои снимают LY28, LY29 и LY33; среди остальных `display::apple::ios` → `platforms::ios::os_version`, когда чтение версии становится `version/gate.rs`). Поправка 2026-10-22 (строка писалась до исключений LY28–LY33, выход части которых — LY38, задача после LY34): исключений в `platforms::*` и ключа `retiring` нет; остаются набор LY38 (32: оконные позиции через таблицу хоста, общие виды AppKit и UIKit, `text::web` → `window::web` и десять рёбер окна и другой фичи одной платформы от LY28, LY30, LY31 и LY33, которые строка LY38 называет с формой снятия) и поимённое ребро `display::winit` → `display::linux`, которое живёт до LY87; путь `include_bytes!` на `classes.dex` и `android_dex.rs` уже указывают на `owner/android/java_view.rs` (LY27), а `android-dex` подтверждает, что ни один путь не ведёт в `platforms/` или `shared/`; ARCHITECTURE.md ядра описывает новое устройство. Тесты `owner/`, которые ещё называют хосты или фичи (унаследованы от `main`: `owner/windows/tests.rs` — `display::windows`, хост-трейт таблицы `hosts::windows` и `WindowsPlatform`; `owner/winit/real_loop_tests.rs` — `WinitPlatform` и `preferences::windows`; тест текстового хоста в `owner/mod.rs` — `HeadlessPlatform`), переходят на заглушки `owner/` (как `owner::winit::testing::QuietHost`) или переезжают к фичам, которые проверяют | LY27, LY28, LY29, LY30, LY31, LY32, LY33 | `crates/flui-native/src/{version/*,hosts/*}`, `shared/*` и `platforms/*` (удаляются), `tests/*`, `Cargo.toml` (`globals`), `tools/xtask/src/platform_floor.rs`, `tools/xtask/allowlists/*` | `module-dag`, `globals`, `platform-floor`, `android-dex`, `file-length`, `markers`; `rg 'unimplemented!'` по `src/` | перенесённые тесты; `tests/` не называет `flui_native::shared` и `flui_native::platforms`; записи нативных трасс без правок | L + T |
| **LY35** | **Слияния дублей** (D13, R4.7). Внутри моделей фич. Режим окна из флагов — одно определение вместо четырёх: `mode_of` в `window/state.rs` остаётся, а `mode_of` в `window/apple/geometry.rs` (было `appkit_window_mode.rs`), `window_mode` в `window/linux/x11.rs` (было `x11_window_mode.rs`) и `public()` в `window/windows/mod.rs` (было `windows/window.rs`) уходят. Период обновления — один конструктор из Гц, мГц и интервала с одной проверкой (конечное, больше нуля) и один издатель в `display/source.rs` вместо `display::android::sample::refresh_period` (`display/android.rs`), `display::apple::link_rate::ScreenPeriods::period_of` и `display::apple::period::core_graphics_refresh_period` (оба в `display/apple/mod.rs`), `refresh_period_for_screen` в `display/apple/macos.rs`, `refresh_period` в `display/linux/wayland_model.rs`, `display::windows::sample::{rational_refresh_period, frequency_period}` (`display/windows.rs`) и `display::web::refresh_estimate` (`display/web.rs`). `Location` клавиши — одна таблица в `input/keys.rs` вместо `keyboard_adapter` (6 мест), `keys` (4), `input/web.rs` (4; было `web/events.rs`) и `input/windows/` (1; было `windows/events.rs`). Шаг колеса — по одному правилу в `input/scroll.rs` вместо `scroll`, `winit/events.rs::wheel_step`, `web/events.rs` и `android_scroll`; видимость — одно правило в `window/visibility.rs` | LY34 | `crates/flui-native/src/{window,display,input}/*` | `module-dag`, `globals`, `file-length` | табличные тесты `mapping` (одни для всех потребителей); записи нативных трасс проходят без изменений | L + W + T |
| **LY36** | **OS-типы `pub(crate)`** (D11, R4.10, R3.9). Скрыть: `win32::{HWND,DWMWINDOWATTRIBUTE,DwmExtendFrameIntoClientArea,DwmSetWindowAttribute,MARGINS}`, `WindowsWindow::hwnd`, `toggle_fullscreen_for_hwnd`, `WindowsAccessibility::new(HWND)`, `MacOSPlatform::app`, `AndroidPlatform::{new(AndroidApp),app}`, `WebWindow::canvas`, реэкспорты `accesskit` через `PlatformAccessibility`, `FakeAccessibility::published`, `liquid_glass`, а также имена, которые не называет никто вне крейта и которых нет в сигнатурах публичных трейтов (`WindowConfiguration`, `FullscreenMonitor`, `Headless*`); `PlatformReadyCallback` становится `AppHandler::ready` (LY45); остальные безымянные снаружи имена (`PlatformExecutor`, `WaitError`, `PlatformResult`) уходят вместе с `Platform::background_executor` в LY61. `AndroidApp` flui-app передаёт через вход ядра, форму которого называет PR. `as_any` переезжает с `PlatformWindow` на `HostWindow`; расширения ОС реализованы поверх него и возвращают `Option`. Гейт API-замыкания проверяет rustdoc-JSON ядра с `--document-hidden-items`; его `entries` держит ровно вход Android, которого требует эта строка, `traits` — методы реализаций `PlatformAccessibility` из `flui-semantics`. Часть R4.10 про тестовые двойники (`FakeAccessibility`, `FakeTextInput`, `MockWindow`, `HeadlessPlatform` и прочие `Headless*` в корне) — LY100 (двойники за фичей `scripted`) и LY48 | LY34, LY39 | `crates/flui-native/src/{lib.rs,host_window.rs,hosts/*,owner/*,window/*,accessibility/*}`, `crates/flui-platform/src/platform_window.rs` (удаляется `as_any`), `crates/flui-app/src/app/runner/*`, `examples/*` | гейт API-замыкания (расширен на ядро), `workspace`, `cross-typecheck` | rustdoc-JSON: пусто; расширение Windows на окне headless даёт `None` | L + W + T |
| **LY37** | **Escape hatch** (D11, R3.9). Фасад публикует `flui::native::<os>` за `cfg` и фичей `native` (Evolving): `WindowsWindowExt`, `MacOSWindowExt` как расширения поверх `dyn HostWindow` (форма LY36: один метод `win32()`/`appkit()` даёт вид окна ОС `Win32Window`/`AppKitWindow` или `None`), чьи методы принимают и возвращают только типы FLUI (вкладки macOS, уровень, тема и углы Windows; прогресс на панели задач — `flui-system`, LY57; материалы одной ОС, ADR-0254 п. 3: Liquid Glass macOS возвращается здесь из git (`window/apple/liquid_glass.rs` до LY36) методами `AppKitWindow`, общий словарь материалов — команда окна LY43; заглушки, отвечавшие успехом без действия, удалены LY36), а сырые handle окна и дисплея отдают только через трейты `raw-window-handle` (ADR-0089 §2); опции создания одной ОС — типизированное значение этой ОС в слоте `WindowOptions` (ADR-0254 п. 8; форму слота называет PR); примеров `windows11_demo` и `resize_jitter_probe` нет с #1827, переводить нечего; строка в `docs/extending.md` и в `tests/surface.rs` | LY36 | `src/*` (фасад), `crates/flui-platform/src/window.rs` (слот), `crates/flui-native/src/window/{windows,apple}/*` (расширения), `examples/*`, `docs/extending.md` | `workspace`, `reach`, `facade-combos`, `doc-strict` | чужой хост → `None`/`Unsupported`, не паника; doctest каждого расширения; фича `native` выключена по умолчанию | L + W + T |

### Контракт

| ID | Задача | Зависит от | Крейты и файлы | Гейты | Тесты | Где |
|---|---|---|---|---|---|---|
| **LY38** | **Один словарь отсутствия; закрытость как решение** (D5, D6, R3.7, R2.5). Модуль `unsupported`: `Unsupported { reason }` (конструируется, не пишется литералом; конструктор с именем возможности `Unsupported::of::<C>` добавляет LY50), `UnsupportedReason { NotOnThisPlatform, NotOnThisDevice, NotDeclared { what }, Unavailable, NotRegistered, NoWindow }` (`NotOnThisDevice` — ОС или железо этого устройства не умеет, номер версии не называется), `Availability`. Встраивается в `WindowCommandError`, `WindowShowError`, `CursorError`, `DragError`, исход haptics, `StorageError`, `PreferenceQueryError`, `TextStoreHostError` вместо девяти unit-вариантов. `Absent<F>` (`host.rs`, LY22) отвечает `Unsupported { NotOnThisPlatform }` в каждой позиции, где сегодня отвечает словарём позиции; оконные позиции таблицы (`Text`, `Accessibility`, `Haptics`, `Chrome`) становятся тем, что отдаёт окно: у `HostWindow::accessibility` и `PlatformWindow::{text_input, haptics, system_chrome}` нет умолчаний, окно без позиции отдаёт `Absent` своей таблицы. Так снимаются рёбра файлов окна в фичи, которые оставили LY32 и LY28 (окно делает объект позиции, называя файл платформы фичи): Win32 — `accessibility::windows`, `haptics::windows` и `drag::windows` (слот и отзыв drop target в контексте окна); macOS — `accessibility::apple`, `haptics::apple`, `text::apple` (текстовый ввод над content view); iOS — `haptics::apple`; Android — `haptics::android`, `drag::android` (источник drag); Web — `haptics::web`, `text::web` (текстовый ввод над canvas); winit — `accessibility::linux`, `text::winit`; headless — `accessibility::headless`, `haptics::headless`, `drag::headless` (источник drag и сторона теста), `text::headless` (двойник текстового ввода). Источник drag и цель Win32 и Web окно уже получает через хук владельца (`HostDrag`). Здесь же уходят рёбра общего вида Apple, оставленные LY29: content view AppKit и касания UIKit становятся мультиплексорами владельца, к которым ввод, текстовый ввод и окно подключаются хуками и держат каждый своё состояние окна по `WindowIdentity` (ADR-0151 §10) — `text::apple::macos` ↔ `input::apple::macos`, `input::apple::macos` → `window::apple::macos`, `window::apple::macos` → `input::apple::macos`, `window::apple::ios` → `input::apple::ios`; нужен прогон на Mac и iPhone. И ребро `text::web` → `window::web`: текстовый ввод регистрирует свой редактор и слушателей через хуки холста (`CanvasRegistration`), как ввод и drag, и держит их по `WindowIdentity`. Здесь же уходят десять рёбер окна и другой фичи одной платформы, которые оставили LY28, LY30, LY31 и LY33 (поправка 2026-10-22 при LY34). Формы снятия три: (a) объект позиции окно получает через таблицу хоста (рёбра оконных позиций выше); (b) фича держит своё состояние окна по `WindowIdentity` и заводит и снимает его своими хуками окна; (c) факт дисплеев, предпочтений или lifecycle окно получает через хук `owner/`. По форме (b): `text::windows` ↔ `window::windows` (соединение TSF — у текста, заводится и снимается его хуком класса, масштаб окна текст слышит шагом окна), `text::android` ↔ `window::android` (поле, которое правит метод ввода, и захват размера текста — у текста, через хуки окна лупера Android), `window::apple::ios` → `text::apple::ios` (захват размера текста по трейтам окна — у текста, через хуки окна цикла UIKit). По форме (c): `window::windows` → `lifecycle::windows` (блокировку и разблокировку сессии окно получает фактом от хука владельца и само не разбирает `WM_WTSSESSION_CHANGE`), `window::android` → `preferences::android` (ночной режим, плотность и геометрию касаний — хуком лупера Android, как их прочёл источник предпочтений), `window::apple::macos` → `display::apple::macos` (границы экрана для `bounds_of` — хуком дисплеев цикла macOS), `window::apple::ios` → `display::apple` (`LinkPeriod` и `change_screen` — хуком дисплеев цикла iOS, как он несёт идентичность экрана), `window::headless` → `display::headless` (разложенные дисплеи — хуком дисплеев цикла headless; отказ `MockWindow::simulate_move_to` — ошибкой самого окна). `reason` каждого исключения в `[package.metadata.flui.modules]` говорит то же; снятие требует прогонов на macOS, iOS и Android. `clippy::exhaustive_enums` включён на уровне deny; каждое из 26 закрытых перечислений получает `#[non_exhaustive]` или `#[allow]` с причиной (W3C-закрытые `KeyState`, `Location` — кандидаты на `allow`). Версия ОС в контракт не попадает (ADR-0244) | LY34 | `crates/flui-platform/src/{unsupported.rs,window.rs,window_state.rs,drag.rs,haptics/*,storage.rs,preferences/geometry.rs,text_store/host.rs,input_capabilities/*,…}`, хосты и потребители ошибок; `crates/flui-native/src/{window,text,display,preferences,lifecycle,owner}/*` (рёбра окна), `crates/flui-native/Cargo.toml` (исключения `module-dag`) | `workspace`, `reach`, `clippy` (`exhaustive_enums`), `module-dag` | таблица «каждая ошибка несёт причину»; conformance окна и haptics; существующие | L + T; Dev: macOS, iOS, Android |
| **LY39** [P] | **Гейт API-замыкания** (ADR-0089 §6, R2.6). Новая задача xtask по rustdoc-JSON: публичные сигнатуры контракта называют upstream-типы только из ADR-0089 §2; `ui-events`, `keyboard-types`, `dpi` в списке исключений поимённо со ссылкой на спеку pointer-vocabulary, список только сокращается; тот же механизм принимает проверку OS-типов в ядре (LY36). В CI — отчёт `cargo-semver-checks` по контракту (R2.7) | LY21 | `tools/xtask/src/api_closure*` (новый), `.github/workflows/ci.yml`, манифесты контракта (`reach-forbid`) | сама задача: `api-closure` (+ `--self-test`) | самотест: посаженный `ui-events` в публичной сигнатуре и посаженный OS-тип в ядре; исключение, которое больше не нужно, — ошибка | L |
| **LY40** | **Снимок окна: полнота** (D4, R3.1). `WindowState` получает все оконные факты, которые сегодня отдают геттеры или парные `on_*`: размеры и масштаб (`WindowMetrics`), внешний вид, дисплей, safe-area insets, состояние исполнения, фокус, видимость, наведение; каждый факт сохраняет правило своей записи (тема — пользовательская, как применена к окну, ADR-0222; дисплей — по идентичности снимка дисплеев, ADR-0236; состояние исполнения не зависит от фокуса и видимости, ADR-0035; нерисуемое значение — нулевой размер, неконечный или неположительный масштаб — не публикуется); `WindowCapabilities` входит в `WindowState` (токен привязывается к ревизии, LY42); `activates` и сходные признаки — `Availability`, а не `bool`. Одно `on_state_changed` без данных; хост публикует факты одной ревизией и доставляет изменение синхронно внутри OS-колбэка там, где его даёт ОС (Win32 modal loop). Геттеры живут до LY41 (аддитивный шаг) | LY38 | `crates/flui-platform/src/{window_state.rs,platform_window.rs}`, `crates/flui-native/src/window/*` | `workspace`, `reach`, `cross-typecheck` | conformance окна: снимок согласован с readback ОС на каждом хосте; Windows — живой resize в modal loop | L + W + T |
| **LY41** | **Потребители на снимок; удаление вторых истин** (D4, D10, R3.1). Удалить геттеры `is_focused`, `is_visible`, `physical_size`, `logical_size`, `scale_factor`, `metrics`, `appearance`, `display_id`, `safe_area_insets`, `execution_state`, `is_active`, `is_hovered`, `capabilities`; `mouse_position` и `modifiers` (положение и модификаторы берутся из потока ввода); регистрации `on_resize`, `on_active_status_change`, `on_visibility_status_change`, `on_hover_status_change`, `on_appearance_changed`, `on_display_changed`, `on_safe_area_change`, `on_execution_state_change` (остаются шесть регистраций по обязательствам, их заменит LY44); потребитель, которому нужно знать, какой факт изменился, сравнивает новый снимок с прежним (равенство ревизий); методы-близнецы `show`, `activate`, `close` и `WindowShowError`; `WindowEvent` и `Platform::on_window_event`. `flui-app` (`direct.rs`, `runner/{window_install,desktop,secondary_window,android,ios,web}.rs`) подписывается один раз на запуск; `presentation.rs` читает снимок; 15 реализаций `PlatformWindow` вне ядра (ADR-0254) теряют методы; команды show, activate, close идут только через `WindowControl` | LY40, LY26 | `crates/flui-platform/src/{platform_window.rs,window.rs}`, `crates/flui-native/src/window/*`, `crates/flui-app/src/app/**`, `crates/flui-runtime/src/presentation.rs`, `crates/flui-interaction`, `crates/flui-testing`, тестовые двойники в `flui-*` | `workspace`, `reach`, `markers` | headless: смена фокуса видна одним `state_changed` и одним снимком; путь вето закрытия (ADR-0123) через `WindowControl`; `rg` по удалённым именам пуст | L + W + T |
| **LY42** | **`AdmittedCommand`** (D4, R3.2). `WindowCommand::admit(&WindowCapabilities)` возвращает `AdmittedCommand`, который строит только `admit`; `apply` принимает его; ошибки разделены: `AdmissionError { Unsupported, InvalidGeometry }` для вызывающего, `ApplyError { Closed, Superseded, Platform }` для хоста, где `Superseded` — возможности сменили ревизию между допуском и применением на следующем owner-ходе, команду надо допустить заново (гонка «допустили на ревизии N, применили на N+1» не прячется в `Platform` и отличима от сбоя ОС); токен привязан к ревизии снимка, а ОС оставляет за собой последнее слово через `Platform` и следующий снимок. Место вызова — `WindowControl::submit` в `flui-app`: допуск при подаче против возможностей снимка, применение на следующем ходе владельца в порядке подачи; `submit` возвращает `Result<(), SubmitError>` (`Admission(AdmissionError)` или `Closed`); `WindowCommand::Close` заменяет метод `close` | LY41 | `crates/flui-platform/src/{window_state.rs,platform_window.rs}`, `crates/flui-app/src/app/window_control.rs`, файлы платформ в `window/` (`apply`) | `workspace`, `reach` | trybuild: `AdmittedCommand` нельзя построить снаружи и голую `WindowCommand` нельзя отдать в `apply`; conformance «`Unsupported` ровно тогда, когда возможность ложна»; смена возможностей между допуском и применением даёт `Superseded` | L + W + T |
| **LY43** | **Покадровые глаголы и материал фона** (D4, D10, R3.6). `set_frame_rate_hint` и `set_input_regions` возвращают `Result<(), Unsupported>` и имеют `Availability` в возможностях окна (регионы — в `InputCapabilities`); одна кроссплатформенная команда материала фона вместо `set_background_appearance` с `Availability` в возможностях (набор материалов называет PR: общий словарь всех хостов; материалы одной ОС достижимы через `flui::native::<os>`, LY37, ADR-0254 п. 3); `WindowBackgroundAppearance` (в т. ч. `MicaBackdrop`, `MicaAltBackdrop`) удалён (ADR-0254 п. 3, 8). Windows: отказ `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE)` становится `Unsupported(NotOnThisDevice)` вместо записи в журнал; `liquid_glass` удалён LY36 (его не достигал ни один путь) и возвращается из git в LY37 как методы вида `appkit()`, не в этой строке | LY42 | `crates/flui-platform/src/{platform_window.rs,window.rs,window_state.rs}`, `crates/flui-native/src/window/{windows,apple}/*`, `crates/flui-app` | `workspace`, `reach`, `cross-typecheck` | conformance; тест на таблице версий (`Since<F>`): ниже порога — `NotOnThisDevice`; Windows — живая смена материала | L + W + T |
| **LY44** | **`WindowHandler` при открытии** (D4, R3.3). `open_window(options, Rc<dyn WindowHandler>)`; обязательства: ввод с результатом доставки (`DispatchEventResult`, включая отложенные формы), изменение состояния, запрос кадра, запрос закрытия с решением, закрыто, смена поверхности; каждый метод получает доказательство owner-потока. Отложенное открытие (`PlatformProxy::open_window`) несёт `Send`-фабрику, собирающую обработчик на owner. Удалить `WindowCallbacks`, `impl_window_callback_setters!`, 14 `on_*` с инертными умолчаниями, очередь раннего ввода и снимок «ещё не зарегистрировано» (ADR-0213 §3), защёлку `WindowCallbacks::clear()` (ADR-0063 §5). Хосты держат обработчик в слоте и вызывают его через точку LY26; `flui-app` не регистрирует колбэки. `WindowOptions::input_latency`, которую сегодня никто выше хоста не задаёт, проводится из конфигурации окна приложения (это её потребитель; D10, R3.10), Web и Android читают значение | LY41 | `crates/flui-platform/src/platform_window.rs`, `crates/flui-native/src/{owner/*,window/*}`, `crates/flui-app/src/app/**`, тестовые двойники | `workspace`, `reach`, `globals`, `markers` | conformance: первое событие после `open_window` доходит; повторный вход из каждого метода (как в LY26); вето закрытия; отложенный ввод сохраняет `DEFERRED` | L + W + T; Dev: Android |
| **LY45** | **`AppHandler` при `run`** (D12, R3.4). `run(handler)`: `AppHandler` — трейт ядра (`flui-native`), его реализует только `flui-app`; обязательства без умолчаний: `ready(OwnerPlatform)` (bootstrap, который нёс `PlatformReadyCallback`), `quit_requested`, `reopen`, `session_end` (`SessionEndAnswer`), `host_notice`, `exit_policy`, `wake_deadline`; `on_preferences_changed` нет; принятое событие не теряется при замене или отсутствии обработчика; `PlatformHandlers` удалён. `on_open_urls` и `on_keyboard_layout_change` регистрациями не заменяются: обязательство `opened(urls, files)` добавится к трейту вместе с фичей активации (инертной реализации сейчас нет, правило 4 design), а `keyboard_layout` и его событие становятся `pub(crate)` ядра (LY55). Доставка предпочтений сохраняет правила ADR-0172 (R9.7) | LY44, LY8 | `crates/flui-native/src/{platform.rs,owner/*,lifecycle/*,preferences/*}`, `crates/flui-app/src/app/runner/host.rs` | `workspace`, `reach`, `globals` | headless: каждое событие доходит один раз; замена обработчика в вызове; отсутствие обработчика не теряет принятое | L + W + T |
| **LY46** | **`!Send` окно и `SurfaceTarget`** (D4, R3.5). `PlatformWindow` теряет `Send + Sync`; поток растеризации получает `SurfaceTarget: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static`, отвечающий `HandleError::Unavailable` после закрытия, с обязательным `pre_present_notify` (поток растеризации зовёт его перед каждым present; на Wayland он взводит frame callback); `unsafe impl Send` хоста остаётся только на нём, а `SAFETY` называет инвариант (что держит цель и как проверяется жизнь окна), не операцию (macOS); правила `Unavailable` по хостам из ADR-0063 п. 5 (Win32 — проба идентичности, AppKit — флаг закрытия, Android — только для завершённого окна, headless — всегда) переезжают на цель. `PendingWindow::wait()` отдаёт рабочему потоку идентификатор, а не `Arc<dyn HostWindow>`. Рассказ в документации `request_redraw` (около 50 строк) превращается в ошибку компиляции; `thread_local!` в `flui-app/src/app/runner/host.rs` теряет свою причину и удаляется, если другой не найдено. Заменяет ADR-0063 в части `Send + Sync` окна | LY44 | `crates/flui-platform/src/platform_window.rs`, `crates/flui-native/src/{owner/*,window/*}`, `crates/flui-engine`, `crates/flui-rendering`, `crates/flui-app/src/app/runner/*`, реализации `PlatformWindow` (семь хостов и 15 вне ядра, ADR-0254) | `workspace`, `reach`, `globals` | `static_assertions::assert_not_impl_any!`; trybuild; `SurfaceTarget` после закрытия; Win32: `state_changed` синхронно в OS-колбэке живого resize | L + W + T |
| **LY47** [P] | **Машинерия text store — к потребителю** (D7, R2.4). `LockArbiter`, `CompositionLedger`, `EditGeneration`, `byte_range`, `committed_text` (около 1,6 тыс. строк политики) → `flui-interaction::text_input`; `flui-sdk` реэкспортирует их авторам текстовых виджетов. В контракте остаются трейты, значения, `CommitGate`, `OwnerCalls`, `commit_composition_in_place`, `project_ime_event`. `lock.rs` делится по потребителю. Заменяет закрепление ADR-0142 | LY21, text-ime и teardown | `crates/flui-platform/src/text_store/*`, `crates/flui-interaction/src/text_input/*`, `crates/flui-widgets/src/text/*`, `crates/flui-sdk/src/lib.rs`, `tests/lock_gate.rs` | `workspace`, `reach`, `deps` | тесты арбитра и журнала переезжают во `flui-interaction`; строка `tests/surface.rs` | L + T |
| **LY48** [P] | **Тестовые двойники — из контракта** (D9, R2.2). `InMemoryTextStore` (736 строк) → `flui-testing` (комплект `text_store_kit`); `FakeHaptics` делится на две роли, как `InMemoryClipboard` (D9): headless-приёмник в ядре (`pub(crate)`; headless-хост не может зависеть от `flui-testing`) и сценарный двойник во `flui-testing`. `InMemoryClipboard` делится в LY50 | LY21, text-ime и teardown | `crates/flui-platform/src/{text_store/in_memory.rs,haptics/fake.rs}`, `crates/flui-testing/src/*`, `crates/flui-native/src/haptics/headless.rs` | `workspace`, `testing`, `reach` | `text_store_kit::assert_conforms`; существующие тесты; Win32 `text_services/{probe,tests}.rs` — W | L + W |
| **LY49** | **Только для хоста — в ядро** (D10, R2.2). Таблицы предложений `OfferTable`, `OfferRecord`, `TransferCompleter`, `TransferUri`, `TransferImage` (вариант без потребителя выше хоста удаляется вместе с типом) → модель `drag/` (`offers.rs`); `device_to_logical`, `logical_to_device`, `offset_from_coords`, `delta_offset_from_coords`, `DeviceInputRegion`, `ProjectedInputRegions` → модель `input/` (`regions.rs`); удалить `KeyText`. В контракте остаются `PlatformInput`, `DispatchEventResult`, `DragDropEvent`, `InputRegion`, `InputRegionKind` и словарь data transfer | LY34, LY41 | `crates/flui-platform/src/{data_transfer.rs,input.rs,input_regions.rs,keyboard/*}`, `crates/flui-native/src/{input,drag}/*` | `workspace`, `reach`, `key-vocabulary`, `recording-vocabulary` | существующие drag-тесты (headless, OLE, HTML, Android); таблица ввода | L + W + T |

### Шов и крейты возможностей

| ID | Задача | Зависит от | Крейты и файлы | Гейты | Тесты | Где |
|---|---|---|---|---|---|---|
| **LY50** | **Шов возможностей с первым потребителем — clipboard** (D8, D17, ADR-0152, R5.1–R5.5, R5.7, R5.9). Контракт, модуль `capability`: `Capability` (маркер, `Handle: Clone + 'static`, `NAME`), `CapabilityProvider<C>` (dyn-compatible), `ProviderContext` (`owner()` — `Send`-poster на owner-поток; `window()` — `Option<WindowRef>`), `WindowRef` (id, raw-handles, масштаб), `Completion<T>`, `CapabilityRegistrar` (им `flui-app` регистрирует встроенные провайдеры). `flui-runtime`: реестр per-realm (параметр конструктора realm, `BuildOwner`, `capability_erased`, `LifecycleContextExt`), конфликт и повтор `NAME` → `AppRunError::CapabilityConflict` до первого окна. `ClipboardHandle` во `flui-interaction` (owner-local, на `Completion`); `EditableText` копирует и вставляет через него. OS-объекты clipboard живут в ядре (owner-local, чтение через колбэк), провайдеры шва собирает `flui-app` поверх них: маркер лежит во `flui-interaction`, выше ядра, и ядро провайдеров не держит. Объекты: Windows, macOS, iOS (`UIPasteboard`), Android (`ClipboardManager` через JNI вместо внутрипроцессной заглушки), Web (асинхронный `navigator.clipboard`), winit (arboard), headless (в памяти, `pub(crate)`). Удалить `Clipboard`, `ClipboardItem`, `InMemoryClipboard`, `Platform::{clipboard,write_to_clipboard,read_from_clipboard}`, `clipboard_handle`, `test_clipboard()` в runtime; во `flui-testing` — сценарный провайдер clipboard на минимальном сценарном примитиве фичи `testing` контракта (упорядоченные ответы на запросы, разрешение на границе хода владельца; полный примитив — LY54). `Unsupported::of::<C>` с именем возможности добавляется здесь. AGENTS.md «Extending FLUI», `docs/extending.md` (строка «Platform capability») и `design/architecture.md` получают описание шва. Типы разрешений приходят с первым потребителем (LY64). Заменяет ADR-0039 §5 (`Clipboard` остаётся `Send + Sync`). Сливается в один день с LY51: шов не попадает на `main` без потребителя (ADR-0152 п. 4) | LY38, LY46, LY47, send-flip (ядро) | `crates/flui-platform/src/{capability.rs,clipboard.rs}`, `crates/flui-runtime/src/{capability/*,realm_services.rs,presentation.rs}`, `crates/flui-view/src/{owner/*,element/behavior.rs,context/*}`, `crates/flui-widgets/src/text/editable_text.rs`, `crates/flui-interaction/src/clipboard.rs`, `crates/flui-app/src/app/runner/host.rs`, `crates/flui-app/src/app/capability_providers*` (встроенные провайдеры), `crates/flui-native/src/clipboard/*`, `crates/flui-testing` | `workspace`, `reach`, `globals`, `testing` | ADR-0152 §4: копирование и вставка в headless; пустой реестр отвечает `NotRegistered`, не паникой; `NotOnThisPlatform` от провайдера доходит до виджета без изменений; две возможности с одним `NAME` отказывают до окна; результат, доставленный ОС не на owner-потоке, доходит до виджета на границе хода владельца через poster; trybuild E0599 (`BuildContext`), E0277 (`ClipboardHandle: Send` и `Completion<T>: Send`), E0639 (литерал `Unsupported`); dyn-пин; Windows — W | L + W + T; Dev: Android, Web (браузер) |
| **LY51** | **`Plugin` и регистрация приложением** (D17, R5.8). `Plugin` и `CapabilityRegistrar` в контракте; `Application::plugin` и `Application::capability`; `AppRunError::CapabilityConflict`; реэкспорт в `flui-sdk` и строка в `tests/surface.rs`; два fixture-пакета вне зависимостей фреймворка: на `flui-sdk` + контракте и лист-крейт только на контракте. Сливается в один день с LY50 (посадка шва, ADR-0152 п. 4); потребитель в production — `flui-system` (LY57), PR называет его | LY50 | `crates/flui-platform/src/capability.rs`, `crates/flui-app/src/app/application.rs`, `crates/flui-sdk/src/lib.rs`, `tests/fixtures/*` | `workspace`, `reach` (fixture не видит хост) | два плагина на одну возможность и повтор `NAME` отказывают `Application::run` до окна; оба fixture регистрируют и получают свою возможность | L |
| **LY52** | **Storage через шов** (D8, R5.9). Трейт `Storage` остаётся в контракте; встроенный провайдер без окна поверх `FileStore` (и IndexedDB на wasm32); `HostStorage` из `flui-app/src/app/storage_host.rs` становится провайдером; headless `MemoryStorage` во `flui-testing`; `LifecycleContext::storage` удалён (заменяет пункт ADR-0133 о методе) | LY50, persistence | `crates/flui-view/src/persist/*`, `crates/flui-app/src/app/storage_host.rs`, `crates/flui-native/src/storage/*`, `crates/flui-testing/src/storage.rs` | `workspace`, `reach` | сохранение и загрузка без окна; `NotRegistered`; существующие persist-тесты | L + T |
| **LY53** [P] | **Класс `capability` в гейтах** (ADR-0154, R5.10, R5.11). kind-правила `cargo xtask workspace`: крейт возможности зависит от контракта (и `flui-sdk`, если несёт виджеты), не зависит от других крейтов возможностей, ни один крейт фреймворка не зависит от него, кроме необязательной фичи фасада (LY101); reach-таблица; запрет `build.rs`, запускающего `javac`/`swiftc`/`d8`; версия крейта — с поездом, пока версия контракта с поездом (ADR-0153, ADR-0154 п. 1). Сливается в день LY57: правило приходит в PR первого крейта (ADR-0154 п. 12) | LY21 | `tools/xtask/src/workspace/*`, корневой `Cargo.toml` (легенда tier-kind) | `workspace` (+ `--self-test`) | самотест: посаженное нарушение на каждое правило | L |
| **LY54** | **Сценарный примитив** (ADR-0154, R5.13). Расширяет минимальный примитив из LY50. В фиче `scripted` контракта (крейт возможности не может зависеть от `flui-testing`): упорядоченный скрипт ответов на запросы, переключатели службы, отзывы разрешений, внедрённые ошибки, потоки событий в виртуальном времени; всё разрешается на границе хода владельца. `flui-testing`: «поставить провайдер для `C`» без ручной сборки runtime; сценарный провайдер clipboard из LY50 переводится на полный примитив (13+ тестов runtime и 2 теста app, которые держал `InMemoryClipboard`) | LY50 | `crates/flui-platform/src/capability/*`, `crates/flui-testing/src/capability.rs`, `crates/flui-runtime` (тесты), `crates/flui-app` (тесты) | `testing`, `workspace` | сценарий «запрос 1 → отказ, запрос 2 → разрешено, через 3 с отзыв» на виртуальном времени; примитив не доступен без фичи | L |
| **LY55** | **Контракт без `tracing` и `parking_lot`; `Platform` без мёртвых методов** (D10, D12, R2.2, R3.8). `tracing`: остатки в `text_store/projection.rs` и `text_store/host.rs` — логирует вызывающий, причина в возвращаемом значении; `parking_lot`: `DisplayRegistry` на `std::sync::Mutex` (отравление — по `docs/PANIC-POLICY.md`); убрать устаревший комментарий манифеста; `reach-forbid` += `tracing`, `parking_lot`. `Platform`: удалить `active_window`, `compositor_name`; `keyboard_layout` и его событие, которых выше хоста никто не читает, — `pub(crate)` ядра (их используют раскладка клавиш и TSF), публичными их сделает фича меню, когда появится потребитель (подписи сочетаний) | LY47, LY48, LY49, LY50 | `crates/flui-platform/{Cargo.toml,src/display.rs,src/text_store/*}`, `crates/flui-native/src/platform.rs`, файлы платформ | `workspace`, `reach`, `deps` | существующие display-тесты; публичная поверхность контракта по гейту API-замыкания | L + T |
| **LY56** | **Декларации возможностей → манифесты** (D18, R5.12, R6.2). `flui-cli`: строгая схема `[package.metadata.flui.capability]` с фичами; генератор по `cargo metadata` (транзитивные крейты, включённые фичи) пишет `AndroidManifest` (базу FLUI-владельца и блоки крейтов между маркерами), `Info.plist`, entitlements, `PrivacyInfo.xcprivacy` (объединение по приложению), `appxmanifest`; таблица приложения переопределяет usage-строки и отключает возможность; режим verify перечисляет недостающие ключи в проектах, которыми CLI не владеет; `flui add <capability>` (фича фасада, `cargo add` без фасада, декларации, строка регистрации); `NotDeclared` — ответ провайдера, не отказ ОС. База манифеста Android — шаблон под владением FLUI (содержимое — LY62, LY63). Ключи схемы фиксирует первый крейт с декларацией — `flui-system` (например `<queries>` Android для URL и share); сливается в день LY57 (ADR-0154 п. 12) | LY53 | `crates/flui-cli/src/{build,add,manifest}*`, `crates/flui-cli/templates/platforms/*`, fixture-крейты | `workspace`, `checks` | golden-тесты генератора на fixture-крейтах (включая транзитивный и фичу); тест verify; тест `NotDeclared` | L |
| **LY57** | **`flui-system`: крейт и Windows** (D18, R5.14). Новый крейт возможностей: маркеры и handle служб (диалоги, URL, файлы, reveal, trash, share, message box, автозапуск, глобальные сочетания, прогресс и значок на панели задач, jump list, пути и сведения об ОС; трей приходит вместе с фичей меню: без модели меню это только значок), `Plugin`, декларации, симулирующий провайдер на LY54, общая conformance-таблица. Провайдеры Windows: файловые диалоги (код `prompt_for_*` и OLE переезжает из хоста), URL, открыть путь, reveal, trash, message box, share (где ОС даёт; иначе `Unsupported` с причиной), автозапуск, глобальные сочетания, прогресс и значок, jump list (`ICustomDestinationList`; хосту не нужен), пути и сведения об ОС; родитель диалога — из `WindowRef`; провайдер не имитирует успех, возможность с деградирующей гарантией сообщает уровень на handle | LY51, LY53, LY54, LY56 | `packages/flui-system/*` (новый), `docs/crates.md`, корневой `Cargo.toml` | `workspace`, `reach`, `globals`, `deps` | conformance-таблица против симулятора в CI; Windows против реальной ОС; `NotDeclared`, `NotOnThisPlatform` | L + W |
| **LY58** [P] | **`flui-system`: macOS** (D18). Провайдеры на objc2: `NSOpenPanel`/`NSSavePanel`, `NSWorkspace` (URL, открыть путь, reveal), trash через `NSFileManager`, `NSAlert`, `NSSharingServicePicker`, `SMAppService`, глобальные сочетания, прогресс и значок Dock, пути | LY57 | `packages/flui-system/src/macos/*` | `workspace`, `cross-typecheck` | таблица против симулятора; macOS — T | L + T; Dev: macOS |
| **LY59** [P] | **`flui-system`: Linux** (D18, D19). Порталы через `ashpd`/`zbus`: FileChooser, OpenURI, OpenDirectory (reveal), Trash, GlobalShortcuts, Background (автозапуск); прогресс — сигнал Unity LauncherEntry; message box и share — `Unsupported` с причиной, где портала нет; пути — XDG. Родитель портала из `WindowRef`: `x11:<XID>`; для Wayland — после LY81 | LY57 | `packages/flui-system/src/linux/*` | `workspace`, `reach`, `deps` | таблица против симулятора; портал-заглушка на сессионной шине в CI | L; Dev: Linux |
| **LY60** [P] | **`flui-system`: Web** (D18). Файловые диалоги (`showOpenFilePicker`, где есть, иначе `<input type=file>` и загрузка), `window.open`, share через `navigator.share`, message box через `<dialog>`; reveal, trash, автозапуск, глобальные сочетания, прогресс, jump list — `Unsupported(NotOnThisPlatform)`; пути — `Unsupported` | LY57 | `packages/flui-system/src/web/*` | `workspace`, `wasm-check` | таблица против симулятора; `wasm-test`; браузерные тесты | L (wasm32); Dev: Web |
| **LY61** | **`Platform` без сервисов и исполнителя** (D12, R3.8). `open_url`, `reveal_path`, `open_path`, `prompt_for_paths`, `prompt_for_new_path`, `app_path` — удалены (живут в `flui-system`); `background_executor`, `BackgroundExecutor`, `Task`, `TaskLabel`, `PathPromptOptions`, `PlatformExecutor` — удалены, внутренние пользователи (winit, Linux notices, macOS, Windows, headless, Web, Android, iOS) переведены на собственные нити и каналы хоста. Заменяет ADR-0039 §2 | LY55, LY57, LY58, LY59, LY60, LY67, LY73 | `crates/flui-native/src/{platform.rs,executor.rs,task.rs,owner/*,lifecycle/*}` | `workspace`, `reach`, `globals` | существующие наборы; нить `flui-native-bg` исчезает из тестов | L + W + T |

### Хосты

| ID | Задача | Зависит от | Крейты и файлы | Гейты | Тесты | Где |
|---|---|---|---|---|---|---|
| **LY62** | **Android: `FluiActivity`** (D14, R6.1, ADR-0255). `dev.flui.platform.FluiActivity extends NativeActivity`, `hasCode="true"`, готовый dex в APK. Java-очередь пересылает только записи, у которых есть потребитель в серии: `onActivityResult` (потребитель — `launch_for_result` из LY64 для выбора файла в `flui-system`), `onTrimMemory` (host notice), `onMultiWindowModeChanged`, `onTopResumedActivityChanged`, `onUserLeaveHint` (факты состояния окна); Rust разбирает очередь на owner-потоке и будит его (ADR-0249); очередь буферизует до `android_main`. `onNewIntent` и `onRequestPermissionsResult` не пересылаются: их потребители — фича активации и первая возможность с разрешением ОС (см. «Чего нет в этой серии»). Dex грузит загрузчик Activity: `InMemoryDexClassLoader`, `DexClassLoader` и `code_cache_dex` удалены, библиотека больше не встраивает dex (`include_bytes!` уходит), класс ищется `loadClass`, не `FindClass`; `FluiHost.PROTOCOL` сверяется с константой Rust до `RegisterNatives`, несовпадение называет оба значения. `flui build` кладёт `classes.dex` в APK из каталога крейта той версии, которую разрешает lockfile приложения (`cargo_metadata`, без `javac` и `d8`); база манифеста: `hasCode`, имя Activity, `enableOnBackInvokedCallback`, `configChanges`, `launchMode="singleTop"` (новый intent не запускает вторую Activity и второй runtime), `<queries>`, тема. `onTrimMemory` доходит до приложения как host notice (`AppHandler`). Заменяет ADR-0247 п. 1, 2, 9 | LY28, LY33, LY45, LY46, LY56 | `crates/flui-native/android/java/*`, `classes.dex(.toml)`, `crates/flui-native/src/{owner,window,lifecycle}/android.rs` (`java_view.rs` — в `owner/android.rs`), `crates/flui-cli/src/build/android_package.rs`, `crates/flui-cli/templates/platforms/android/*` | `android-dex`, `cross-typecheck`, `workspace` | рукопожатие (несовпадение — ошибка запуска с причиной, не `UnsatisfiedLinkError`); очередь буферизует до `android_main`; golden базы манифеста | L + T; Dev: Android (ручной прогон на Galaxy S21 и на образе API 23–25; записи очереди, пришедшие до `android_main`) |
| **LY63** | **Android: упаковка и версия** (D14, D23, R6.2). `targetSdk` закреплён в `platform-support.toml`, `android_package.rs` берёт его оттуда (не «последний установленный»); путь Gradle: исходники хоста пишутся в генерируемый каталог с заголовком или `sourceSets` указывает на крейт, правила R8 `dev.flui.**`, недостающий `proguard-rules.pro`; список `configChanges` проверен прогоном (смена темы, масштаба шрифта, локали не пересоздаёт Activity); `windowSoftInputMode` выбран по прогону | LY62 | `platform-support.toml`, `crates/flui-cli/src/build/android_package.rs`, `crates/flui-cli/templates/platforms/android/*`, `tools/xtask/src/platform_floor.rs` | `platform-floor`, `workspace`, `checks` | упаковщик берёт закреплённый `targetSdk`; golden манифеста и `build.gradle.kts` | L; Dev: Android (`adb shell cmd uimode night yes`, `settings put system font_scale 1.3`: один `android_main`) |
| **LY64** | **Android: мост результатов activity** (D19, R5.6). `launch_for_result(intent)` возвращает `Completion` и маршрутизируется токеном на вызов, не целым числом, общим для всех крейтов; результат приходит из записи `onActivityResult` очереди LY62; `JavaVM` и текущая Activity — обёртки сырых указателей в контексте провайдера, не `static` (каждый крейт выбирает свою версию `jni`); мост сообщает о замене Activity (Android пересоздаёт её). Потребитель — выбор файла через Storage Access Framework в `flui-system` (LY67). `request_permissions`, `PermissionState`, `Grant`, `Denial` и `Scope` в серию не входят: их форму фиксирует ADR-0152 п. 2.8, а типы приходят с первой возможностью, которой они нужны (уведомления или геолокация) | LY50, LY62 | `crates/flui-platform/src/capability.rs`, `crates/flui-native/android/java/*`, `crates/flui-native/src/owner/android.rs` | `workspace`, `globals`, `android-dex` | токен результата; отказ, отмена пользователем, повторное присоединение после пересоздания Activity | L + T; Dev: Android |
| **LY65** [P] | **Android: доступность** (D14, R6.3). Низкоуровневый `accesskit_android::Adapter` вместо `InjectingAdapter`: около 70 строк Java в `FluiView` (делегат и провайдер узлов); hover из нативной очереди через `Adapter::on_hover_event`; запрос дерева будит owner, ответ не позже следующего обновления дисплея; реализация `PlatformAccessibility`; идентификатор ADR-0240 → `viewIdResourceName` | LY32, LY62 | `crates/flui-native/android/java/*`, `crates/flui-native/src/accessibility/android.rs`, `crates/flui-semantics` | `android-dex`, `cross-typecheck`, `workspace` | юнит-тесты моста (потоки, пробуждение); Android 6 и 15 | L + T; Dev: Android (TalkBack) |
| **LY66** [P] | **Android: insets и inset клавиатуры** (D14, R6.4). `setOnApplyWindowInsetsListener` и `WindowInsetsAnimation.Callback` (API 30) вместо опроса `getRootWindowInsets` (сегодня `safe_area.rs`, после LY28 — в `window/android.rs`), каждое изменение постится на owner; уровень API выше нижней границы — гейт `Since<F>` (ADR-0244 п. 4). Safe area и inset клавиатуры доходят через снимок окна до `MediaQuery` (padding и view insets — потребитель); поле inset клавиатуры вносит эта задача, iOS и Web пишут то же поле (LY72, LY74). Регистрация «назад» (`OnBackInvokedCallback`) в серию не входит: производитель бита «можно ли вернуться» — навигатор, вне серии, а регистрация хоста без производителя — неподключённая поверхность (см. «Чего нет в этой серии») | LY62, LY41 | `crates/flui-native/android/java/*`, `crates/flui-native/src/window/android.rs`, `crates/flui-platform/src/window_state.rs`, `crates/flui-runtime` (`MediaQuery`) | `android-dex`, `workspace`, `cross-typecheck` | юнит-тесты на правилах insets; счётчик `WindowResized` при анимации клавиатуры | L + T; Dev: Android |
| **LY67** | **`flui-system`: Android** (D18). Выбор файла через `launch_for_result` (`ACTION_OPEN_DOCUMENT`, `ACTION_CREATE_DOCUMENT`), URL (`ACTION_VIEW`), share (`ACTION_SEND`), message box (`AlertDialog` на главном потоке Java), пути; reveal, trash, автозапуск, глобальные сочетания, прогресс, jump list — `Unsupported` | LY57, LY64 | `packages/flui-system/src/android/*` | `workspace`, `cross-typecheck` | таблица против симулятора; Android — T | L + T; Dev: Android |
| **LY68** | **iOS: доказательство `UITextInput`** (D15, R6.5). Минимальный невидимый responder-view на `define_class!` с `UITextInput` (около тридцати методов, токенизатор, input delegate), составление текста через CJK-клавиатуру (японская, китайская) на Simulator; результат и вывод прогона — в PR. Если не складывается, меняется ADR-0255, а не задача | LY30, LY46 | `crates/flui-native/src/text/apple/ios.rs` (спайк), `crates/flui-native/Cargo.toml` (признаки `objc2-ui-kit`) | `cross-typecheck` | прогон составления текста с CJK на Simulator | T; Dev: iOS (Simulator) |
| **LY69** | **iOS: ввод текста** (D15). `UITextInput` и `UITextInteraction` в отдельном невидимом responder-view, backed тем же text-store host, что на других ОС; соответствие `TextInputPurpose` клавиатуре; кандидат-панель CJK; размещение каретки | LY68 | `crates/flui-native/src/text/apple/ios.rs` | `cross-typecheck`, `module-dag` | правила соответствия — на Linux; составление текста — Simulator | T; Dev: iOS |
| **LY70** [P] | **iOS: доступность** (D15). `accesskit_ios::Adapter` (низкоуровневый), вызовы с главного потока; идентификатор ADR-0240 → `accessibilityIdentifier`; две версии objc2 в графе допустимы (пути через сырые указатели) | LY32, LY46 | `crates/flui-native/src/accessibility/apple/ios.rs` | `cross-typecheck`, `deps` | правила узлов на Linux; VoiceOver — прогон | T; Dev: iOS (VoiceOver) |
| **LY71** [P] | **iOS: Info.plist** (D15, ADR-0255 п. 14). Генерируемый `Info.plist` получает ключи, у которых в серии есть потребитель: `UIApplicationSupportsIndirectInputEvents` (указатель на iPad) и ориентации iPad (геометрия окна); URL-типы и usage-строки пишет генератор LY56 из деклараций крейтов, когда они есть. Колбэки сцены для URL, user activity и shortcut в серию не входят: их потребитель — фича активации (см. «Чего нет в этой серии») | LY29, LY56 | `crates/flui-cli/src/build/ios_package.rs`, `crates/flui-cli/templates/platforms/ios/*` | `workspace`, `checks` | golden Info.plist | L; Dev: iOS (iPad Simulator: указатель и ориентации) |
| **LY72** [P] | **iOS: клавиатура** (D15). Уведомления клавиатуры (граница iOS 13; `UIKeyboardLayoutGuide` от iOS 15 — уточнение за гейтом `Since<F>`, ADR-0244 п. 4) публикуют inset клавиатуры в снимке окна, откуда его читает `MediaQuery` (view insets). Краевой свайп «назад» в серию не входит (см. «Чего нет в этой серии») | LY28, LY66 | `crates/flui-native/src/window/apple/ios.rs` | `cross-typecheck` | правила анимации и inset — на Linux | T; Dev: iOS |
| **LY73** | **`flui-system`: iOS** (D18). Выбор документа (`UIDocumentPickerViewController`), URL, share (`UIActivityViewController`), message box (`UIAlertController`), пути; reveal, trash, автозапуск, глобальные сочетания, прогресс, jump list — `Unsupported` | LY57, LY28 | `packages/flui-system/src/ios/*` | `workspace`, `cross-typecheck` | таблица против симулятора; iOS — T | T; Dev: iOS |
| **LY74** | **Web: корень DOM и страница** (D16, R6.6). Хост создаёт и владеет корнем DOM: canvas, хост ввода текста, хост семантики (в этом порядке); страница с собственным canvas работает как прежде. `flui-cli` генерирует страницу заново при каждой сборке (`<base href>` для размещения под подпутём, `viewport-fit=cover`, manifest, импорт модуля), без отдельных JavaScript-файлов. Safe area — `env(safe-area-inset-*)` (нужен `viewport-fit=cover`); inset клавиатуры — `visualViewport` (`interactive-widget` не используется: Safari его не даёт); публикация в снимке окна (поле вносит LY66). Согласовано со спекой web-canvas-input-ownership | LY28, LY30, LY46, LY56, LY66 | `crates/flui-native/src/{window,text,owner}/web.rs`, `crates/flui-cli/templates/platforms/web/*`, `crates/flui-cli/src/build/web.rs` | `wasm-check`, `workspace` | wasm-тесты корня; страница с и без своего canvas | L (wasm32); Dev: Web |
| **LY75** | **Web: textarea — основа** (D16). Шрифт не менее 16 px на iOS (иначе Safari увеличивает страницу), `enterkeyhint` из назначения поля, решение об автозаполнении; фокус внутри пользовательского жеста; `EditContext` остаётся улучшением Chrome | LY74 | `crates/flui-native/src/text/web.rs` | `wasm-check`, `workspace` | wasm- и браузерные тесты атрибутов | L (wasm32); Dev: Web (Safari iOS, Firefox) |
| **LY76** | **Web: ARIA DOM** (D16). `PlatformAccessibility` для Web: `TreeUpdate` через `accesskit_consumer` в параллельный ARIA DOM в хосте семантики (последним в корне); включается лениво скрытой фокусируемой кнопкой-заглушкой; текстовое поле семантики служит элементом редактирования, когда доступность включена (экран-ридер и IME видят один элемент); текст поля canvas доступен вспомогательным технологиям при любом пути редактирования; идентификатор ADR-0240 → `data-flui-id` | LY32, LY75 | `crates/flui-native/src/accessibility/web.rs`, `crates/flui-semantics` | `wasm-check`, `workspace` | правила отображения узлов — на хосте; браузерные тесты | L (wasm32); Dev: Web (скринридер) |

### Нативный Linux

| ID | Задача | Зависит от | Крейты и файлы | Гейты | Тесты | Где |
|---|---|---|---|---|---|---|
| **LY78** | **Wayland: основа** (D20, R7.1). Соединение, привязки глобалов, цикл событий на owner-потоке с пробуждением (по образцу ADR-0249), окно `xdg-shell`, снимок и команды окна, дисплеи на живом слое Linux (`display/linux/`), таблица `Wayland` в `hosts/linux.rs` (тип на каждую позицию, чего нет — `Absent<F>`) и `HostWindow`, цель поверхности для wgpu (raw-handles), выбор хоста в `current_platform()`. `zbus` и `accesskit_unix` становятся безусловными зависимостями Linux (из `winit-backend` уходят), условие `accessibility::{bridge, linux}` в `accessibility/mod.rs` — `target_os = "linux"` (ADR-0151 §15 с поправкой 2026-10-22) | LY28, LY31, LY45, LY46 | `crates/flui-native/src/{owner,window}/linux/wayland.rs`, `hosts/linux.rs`, `lib.rs`, `accessibility/mod.rs`, `Cargo.toml` (фичи `wayland`, зависимости Linux) | `module-dag`, `globals`, `file-length`, `deps` | conformance окна на безголовом композиторе; правила — на хосте | L; Dev: Linux (Wayland) |
| **LY79** | **Wayland: ввод** (D20). Клавиатура (xkb, раскладка), указатель, касание, перо (`tablet-v2`), жесты pinch и pan (`pointer-gestures`), форма курсора (`cursor-shape`), сборка через `pointer_assembly` | LY29, LY78 | `crates/flui-native/src/input/linux/wayland.rs` | `module-dag`, `file-length` | `assert_backend_conforms` (ввод); записи нативных трасс Wayland | L; Dev: Linux (Wayland, сенсорный ввод или перо при наличии) |
| **LY80** | **Wayland: IME, clipboard, drag and drop** (D20, D8). `text-input-v3` и text-store host; провайдер clipboard и primary selection через `wl_data_device`; DnD как цель и источник | LY30, LY32, LY50, LY78 | `crates/flui-native/src/{text,clipboard,drag}/linux/wayland.rs` | `module-dag`, `file-length` | conformance clipboard и DnD | L; Dev: Linux (Wayland) |
| **LY81** | **Wayland: окно и экран** (D20). Декорации (`xdg-decoration`, запасные клиентские), дробный масштаб, видимость и перекрытие, темп по `frame`-колбэкам, перенос окна между выходами одного масштаба, экспорт `xdg-foreign` для родителя порталов, AT-SPI (живой слой) | LY32, LY78 | `crates/flui-native/src/{window,display}/linux/*`, `accessibility/linux.rs` | `module-dag`, `file-length` | conformance окна и дисплеев | L; Dev: Linux (Wayland, два выхода) |
| **LY82** [P] | **X11: основа** (D20). Соединение, цикл событий на owner-потоке, окно, EWMH-состояние и команды, дисплеи (RandR, живой слой), таблица `X11` в `hosts/linux.rs` и `HostWindow`, цель поверхности, AT-SPI. Если LY82 сливается раньше LY78, безусловные `zbus` и `accesskit_unix` и условие `target_os = "linux"` в `accessibility/mod.rs` — его (строка LY78) | LY28, LY31, LY45, LY46 | `crates/flui-native/src/{owner,window}/linux/x11.rs`, `hosts/linux.rs`, `Cargo.toml` (фича `x11`) | `module-dag`, `globals`, `file-length`, `deps` | conformance окна на Xvfb с оконным менеджером | L; Dev: Linux (X11) |
| **LY83** | **X11: ввод** (D20). XInput2 (касание, перо, плавная прокрутка), XKB, указатель, курсор | LY29, LY82 | `crates/flui-native/src/input/linux/x11.rs` | `module-dag`, `file-length` | `assert_backend_conforms` (ввод); записи нативных трасс X11 | L; Dev: Linux (X11) |
| **LY84** | **X11: IME, clipboard, drag and drop** (D20, D8). XIM/IBus для IME и text-store host; провайдер clipboard и primary (селекции, INCR); XDND как цель и источник | LY30, LY32, LY50, LY82 | `crates/flui-native/src/{text,clipboard,drag}/linux/x11.rs` | `module-dag`, `file-length` | conformance clipboard и DnD | L; Dev: Linux (X11) |
| **LY85** | **Conformance на обоих и parity** (D20, R7.1). `assert_backend_conforms` и записи нативных трасс на Wayland и X11; `docs/platform-parity.md` и `docs/gaps.md` (запись 5) обновлены по исполненным прогонам; ✅ только после них | LY79, LY80, LY81, LY83, LY84 | `crates/flui-native/tests/*`, `docs/platform-parity.md`, `docs/gaps.md` | `platform-test`, `docs-paths` | вся conformance-таблица на двух хостах | L; Dev: Linux (Wayland и X11) |
| **LY86** | **CI на нативном Linux** (D20, R7.2). Job CI исполняет нативный хост (Wayland и X11) вместо winit | LY85 | `tools/xtask/src/tasks.rs`, `.github/workflows/*` | `ci-verify` | job CI зелёный на нативном хосте | L |
| **LY87** | **Удаление winit** (D20, R7.3). Удалить файлы `winit` в фичах (`owner`, `window`, `input`, `display`, `clipboard`, `drag`) и блоки `cfg(feature = "winit-backend")` в файлах платформ фич (в `display/windows.rs` и `display/apple/macos.rs` это реализации `owner::winit::HostDisplays`), таблицу `hosts/winit.rs`, фичу `winit-backend` и зависимости (`winit`, `arboard`, `ui-events-winit`, `crossbeam-channel`, `zbus` и `futures-util`, если больше никому не нужны), `[[test]]` и `[[example]]` с `required-features = ["winit-backend"]`, `WinitPlatform` в xtask, пропуски в `deny.toml`, разделы документов; общие модули, которыми пользовался только winit, упрощаются; `preferences/linux.rs` и `lifecycle/linux.rs` остаются: ими пользуются нативные хосты | LY86 | `crates/flui-native/{Cargo.toml,src/{owner,window,input,display,clipboard,drag}/winit.rs,src/hosts/winit.rs,examples/*,tests/*}`, `crates/flui-app/Cargo.toml`, `tools/xtask/*`, `deny.toml`, `docs/*` | `deps`, `workspace`, `globals`, `checks`, `docs-links` | существующие наборы; `rg winit` пуст вне архивных корней и ADR | L |

### Планка качества

Задачи D24 и D25: качества, которые должны быть у каждой ОС сразу. Они не вводят новых пунктов
закрытого списка, а доводят его существующие области до планки с гейтом или прогоном (R11).

| ID | Задача | Зависит от | Крейты и файлы | Гейты | Тесты | Где |
|---|---|---|---|---|---|---|
| **LY88** [P] | **Доступность всегда включена** (D24, R11.1). Удалить фичу `a11y` из `flui-native` (`a11y = [dep:accesskit_unix, dep:accesskit_windows, dep:accesskit_macos]`), `flui-app` (`a11y = ["flui-native/a11y"]`) и фасада: адаптеры AccessKit становятся безусловными зависимостями своих ОС; AT-SPI на Linux идёт по D-Bus, который нативному хосту нужен и так (logind, порталы). Мобильные адаптеры (LY65, LY70) и ARIA DOM (LY76) фичи не имеют с самого начала. Убрать `a11y` из `feature-matrix`, `facade-combos`, README и документов. Отступление (решение координатора 2026-10-22, ADR-0151 `Amended`): адаптер AT-SPI приходит с хостом Linux — пока это winit, `accesskit_unix` включает `winit-backend` (его `flui-app` включает на Linux всегда), условие `accessibility::{bridge, linux}` — `all(target_os = "linux", feature = "winit-backend")`, и тест моста AT-SPI идёт с `winit-backend` (в CI — `--all-features`); безусловной зависимостью Linux вместе с `zbus` его делает LY78 | LY32, LY34 | `crates/flui-native/Cargo.toml`, `crates/flui-app/Cargo.toml`, корневой `Cargo.toml`, `crates/flui-native/src/accessibility/*`, `docs/*` | `feature-matrix`, `facade-combos`, `deps`, `workspace`, `cross-typecheck` | `rg` по `feature = "a11y"` пуст; тесты моста доступности идут без `--features`; адаптеры собираются на каждой ОС | L + T |
| **LY89** | **Ноль CPU в простое: предпочтения без опроса** (D25, R11.2). Предпочтения macOS и Android приходят уведомлениями вместо таймера в 500 мс: macOS — `NSWorkspace` и `NSNotificationCenter` (параметры отображения доступности, локаль), Android — `ContentObserver` и `ComponentCallbacks2`/`ConfigChanged` через `FluiView` и очередь LY62; цикл Android после Home не просыпается без события (ADR-0249). Счётчик пробуждений цикла — тест на headless в виртуальном времени и измерение на устройстве | LY8, LY9, LY33, LY62 | `crates/flui-native/src/preferences/{apple,android}*`, `crates/flui-native/android/java/*` | `cross-typecheck`, `android-dex`, `globals` | простой N секунд в виртуальном времени — ноль пробуждений; смена предпочтения — ровно одно; тест падает при возврате таймера | L + T; Dev: macOS и Android (пробуждения за минуту после Home) |
| **LY90** [P] | **Гейт границы паники** (D25, R11.3). Новая задача xtask: по AST каждая `extern "C"` и `extern "system"` функция и каждый OS-колбэк в FFI-модулях файлов платформ проходит `owner::panic_boundary`; текущие 210 мест сверяются, пропущенные исправляются; allowlist пуст | LY26, LY34 | `tools/xtask/src/panic_boundary*` (новый), `crates/flui-native/src/{owner,window,input,text,display,clipboard,drag,accessibility,haptics,preferences,lifecycle}/*` | сама задача: `panic-boundary` (+ `--self-test`), `markers` | самотест: посаженная `extern "C"` без границы не проходит; существующие тесты границы | L |
| **LY91** | **Один таймлайн и обратная связь о показе кадра** (D25, R11.4, R11.5). `EventTime` — единственная шкала ввода, кадров и показа. Контракт получает обратную связь «кадр показан в момент T» на этой шкале (форму называет PR); источники: Windows — DXGI frame statistics, macOS и iOS — `addPresentedHandler`, Wayland — `wp_presentation_time` (LY81), Android — frame timelines; где ОС не даёт (Web: метки `requestAnimationFrame` — начало кадра, не показ; X11) — `Unsupported` с причиной. Frame clock (`flui-scheduler`, `flui-runtime`) использует её для планирования кадра; headless-инъекция | LY46, LY81 | `crates/flui-platform/src/*` (значение обратной связи), `crates/flui-native/src/{window,display}/*`, `crates/flui-scheduler`, `crates/flui-runtime` | `workspace`, `reach`, гейт API-замыкания, `cross-typecheck` | headless-инъекция: метка показа доходит до frame clock и сравнима с меткой ввода; conformance-строка на каждой ОС | L + W + T; Dev: macOS, iOS, Android, Linux (Wayland), Web |
| **LY92** | **HDR, широкий охват, профиль дисплея** (D25, R11.6). Контракт получает `SurfaceColorSpace`, запрос headroom и событие смены профиля дисплея (в снимке дисплеев); поверхность `flui-engine` читает их при настройке. Источники по возможностям ОС: Windows — цветовое пространство DXGI, macOS — `CAMetalLayer` с EDR, Android — HDR-возможности дисплея и `ColorMode`, Wayland — протокол управления цветом, Web — `matchMedia` по `dynamic-range` и `color-gamut`; где ОС не даёт — `Unsupported` | LY41, LY46 | `crates/flui-platform/src/display*`, `crates/flui-native/src/display/*`, `crates/flui-engine/src/*` (настройка поверхности) | `workspace`, `reach`, гейт API-замыкания, `cross-typecheck` | headless-инъекция смены профиля; conformance; `Unsupported` с причиной | L + W + T; Dev: Windows (HDR-монитор), macOS, Android |
| **LY93** | **Файловый доступ с учётом песочницы** (D25, R11.7). Выбор файла возвращает доступ, которым процесс сможет пользоваться и после перезапуска: security-scoped bookmarks (macOS, iOS), URI Storage Access Framework (Android), портал документов (Flatpak), пути с учётом AppContainer и MSIX (Windows); `data_dirs` знает контейнер. Слой не отдаёт путь, который процесс не может открыть | LY57, LY58, LY59, LY67, LY73 | `packages/flui-system/*`, `crates/flui-native/src/storage/*` | `workspace`, `reach` | таблица `flui-system` на симулируемой песочнице: путь вне песочницы не отдаётся; доступ переживает перезапуск | L + T; Dev: Flatpak, macOS в песочнице, Android |
| **LY94** | **Защищённый ввод и скрытые записи буфера** (D25, R11.8). Назначение поля «пароль» включает `EnableSecureEventInput` (macOS), `isSecureTextEntry` (iOS), `IME_FLAG_NO_PERSONALIZED_LEARNING` (Android); `ClipboardHandle` получает способ записать скрытую запись: Windows — `ExcludeClipboardContentFromMonitorProcessing` и `CanIncludeInClipboardHistory`, macOS — `org.nspasteboard.ConcealedType`, Android — `ClipDescription.EXTRA_IS_SENSITIVE`; `EditableText` со скрытым вводом пользуется ими | LY30, LY32, LY50, LY69 | `crates/flui-interaction/src/clipboard.rs`, `crates/flui-widgets/src/text/editable_text.rs`, `crates/flui-native/src/{text,clipboard}/*` | `workspace`, `reach`, `cross-typecheck` | таблицы «назначение поля → флаги ОС» на хосте; скрытая запись в headless-провайдере; копирование из скрытого поля не пишет обычную запись | L + T; Dev: macOS, iOS, Android, Windows |
| **LY95** [P] | **Активация по правилам ОС** (D25, R11.9). Wayland — `xdg-activation`: токен берётся из последнего пользовательского ввода, `WindowCommand::Activate` без токена просит внимание, а не крадёт фокус (`Emulated` в `WindowCapabilities.activates`); Windows — право на передний план передаётся явно при активации второго запуска или ссылкой, без слепого `SetForegroundWindow` | LY28, LY81 | `crates/flui-native/src/window/{windows,linux}/*` | `workspace`, `cross-typecheck` | тест соответствия токена и порядка команд; Windows — окно не отбирает фокус у другого приложения | L + W; Dev: Linux (Wayland) |
| **LY96** | **Conformance с настоящей ассистивной технологией** (D25, R11.10). Сценарии `os`-lane драйвера (ADR-0238) читают приложение через Narrator (Windows), VoiceOver (macOS, iOS), Orca (Linux, AT-SPI) и TalkBack (Android): имя и роль кнопки, переключатель, поле ввода, список, действие; результат записывается в `docs/platform-parity.md` | LY65, LY70, LY76, LY85, LY88 | `crates/flui-driver/*` (lane `os`), `docs/platform-parity.md` | `docs-paths` | сценарий conformance на каждой ОС; запись прогона | Dev: Windows, macOS, iOS, Linux, Android |
| **LY97** | **Записи нативного ввода: IME, lifecycle, предпочтения, дисплеи** (D25, R11.11). `native_recording` и `recording_vocabulary` расширяются на композицию IME, lifecycle (состояние окна, host notices), предпочтения и дисплеи; `--record` `os`-lane драйвера пишет их на Windows, macOS, Android и Web; headless воспроизводит детерминированно | LY41, LY44, LY45 | `crates/flui-native/tests/native_recording/*`, `crates/flui-testing/src/*`, `tools/xtask/src/recording_vocabulary.rs`, `crates/flui-driver/*` | `recording-vocabulary`, `workspace` | replay-тест на каждый вид записи на headless; `assert_backend_conforms` по ним | L + W; Dev: macOS, Android |
| **LY98** | **Журнал нативного ввода в ядро** (D28, R11.13). `tests/native_recording/{codec,mod}.rs` → `src/owner/journal/`; схема записи (заголовок `flui_native_recording`, версия, семейства событий) объявляется во `flui-protocol`; фича `native-capture` сливается с `driver`; golden-записи в `tests/` читаются через `flui-protocol`; `record.start{native}` драйвера пишет этим кодеком | LY26, LY34 | `crates/flui-native/src/owner/journal/*`, `crates/flui-protocol/src/`, `crates/flui-native/Cargo.toml` | `workspace`, `reach`, `globals` | golden-записи без правок; запись во время работы на headless; `flui-driver` читает запись без ребра к ядру | L |
| **LY99** | **Conformance как сценарии драйвера** (D27, R11.12). Строки conformance, которым нужна ОС, — сценарии `flui-driver` в его крейте; тот же сценарий на headless в CI; отчёт драйвера пишет lane и результат в формате, из которого обновляется строка parity. Совместно со спекой flui-driver (её задачи владеют самим драйвером) | LY98; спека flui-driver | `crates/flui-driver/*`, `crates/flui-native/tests/backend_conformance/*` | `checks` | один сценарий прошёл на headless и на Windows (`os`-lane) | L + W |
| **LY100** | **Фича `scripted` и один двойник окна** (D29, R11.14). Сценарный примитив контракта — под `scripted`; `flui-testing` и `flui-app[driver]` включают её; 15 реализаций `PlatformWindow` вне ядра сводятся к `flui_testing::MockWindow`; двойники ядра в корне (`FakeAccessibility`, `FakeTextInput`, `MockWindow`, `HeadlessPlatform` и прочие `Headless*`) уходят за `scripted` — часть R4.10, которую LY36 оставил | LY44, LY50, LY54 | `crates/flui-platform/src/capability/`, `crates/flui-native/src/lib.rs` (двойники в корне), `crates/flui-testing/src/`, `crates/flui-app/Cargo.toml`, тесты `flui-interaction`, `flui-runtime`, `flui-app` | `testing` (ADR-0243), `workspace` | `rg "impl .*PlatformWindow for"` вне ядра и kit — ноль; сценарий диалога через драйвер на headless | L |
| **LY101** | **Фасад: `flui::services::<name>` за фичами** (D31, R5.15, ADR-0154 п. 8). Фасад `flui` получает по фиче на каждый официальный крейт возможности: сейчас `system` для `flui-system` — фича подключает крейт необязательной зависимостью и реэкспортирует его как `flui::services::system`; каждый следующий крейт возможности добавляет свою фичу тем же образцом в своём PR (`flui = { features = ["location", "bluetooth"] }` даёт `flui::services::location::*`). Крейты остаются отдельными: несовместимое изменение в одном задевает только приложения, включившие его фичу. `flui add <capability>` (команда и декларации — LY56) включает фичу фасада, а не добавляет зависимость на крейт напрямую. Kind-правило `workspace` допускает ребро «фасад → крейт возможности» только необязательное, за фичей, и требует фичу фасада у каждого официального крейта возможности | LY57 | `Cargo.toml` (фичи фасада), `src/lib.rs` (`pub mod services`), `crates/flui-cli/src/add*`, `tests/surface.rs`, `docs/crates.md`, `tools/xtask/src/workspace/*` | `workspace` (+ `--self-test`), `facade-combos`, `feature-matrix`, `doc-strict` | `tests/surface.rs`: `flui::services::system` есть ровно при включённой фиче; `facade-combos` собирает каждую фичу отдельно и все вместе; golden-тест `flui add` на fixture-крейте включает фичу фасада и пишет декларации; самотест: обязательная зависимость фасада на крейт возможности и такая же зависимость другого крейта фреймворка отказывают | L + T |
| **LY102** | **Контракт по областям** (D26, R2.10). Модули `flui-platform` группируются: `window/` (опции, состояние, команды, возможности, размещение, трейт окна, `SurfaceTarget`, яркость), `input/`, `text/`, `display/`, `preferences/`, `transfer/`, `haptics/`, `capability/`, `menu/` (резерв), `unsupported`, `notice`, `system_chrome`, `storage`, `target_platform`; корневые реэкспорты имён, которыми пользуются фасад и `flui-sdk`, остаются (`flui::platform::WindowOptions` не меняется) | LY100, LY101 | `crates/flui-platform/src/**`, `docs/crates.md` | `workspace`, `doc-strict`, `docs-paths` | `cargo public-api diff`: корневые пути без изменений; workspace собирается | L |

## Ход исполнения

- [x] **LY20** — ветка `platform/flui-native`. Команды: `git mv crates/flui-platform
  crates/flui-native`; затем по файлам, которые находит `rg -l --hidden` вне `target/`, `.git/`,
  `Cargo.lock`, `docs/plans/**`, текста ADR-0151, `CHANGELOG.md` и `*.stderr`: `sd -s` по порядку
  `flui-platform-api` → заглушка, `flui_platform_api` → заглушка, `flui-platform` → `flui-native`,
  `flui_platform` → `flui_native`, заглушки обратно; `cargo metadata` обновляет `Cargo.lock`;
  `TRYBUILD=overwrite cargo test -p flui-native --test compiler_guards` перегенерирует пять
  `.stderr`; `cargo fmt --all` и `taplo fmt`. По смыслу переписаны: описание ядра в
  `docs/architecture.md`, `docs/crates.md` и комментарий корневого `Cargo.toml`; ADR-0152, 0153,
  0154 и 0254, где `flui-platform` означает будущий контракт, возвращены к этому имени. 17 путей
  ADR-0151 на дерево `083b0f75d` внесены в allowlist `docs-paths` с причиной. Проверки на
  Windows: `cargo heli xtask checks`, `workspace`, `reach`, `deps`, `wasm-check` — зелёные;
  clippy `--workspace --all-targets -D warnings` — зелёный; `cargo check` под
  `aarch64-linux-android` — зелёный; nextest `-p flui-native -p flui-app -p flui-runtime
  -p flui-testing` — 388 прошли; doctest'ы `flui-native` и `flui-app` — 19 прошли. Повтор
  команд на результате ничего не меняет (`rg` по `flui-platform` вне исключений и имени контракта
  пуст). `cross-typecheck` для Apple на этой машине не собирается без `cargo-zigbuild`
  (C-сборка `psm`, `objc2-exception-helper`); job CI прошёл для macOS, iOS, Android и Windows
  (#1829).
- [x] **LY21** — ветка `platform/flui-platform`. Начальная проверка: вне архива, текста ADR-0151 и
  `CHANGELOG.md` токены `flui-platform`/`flui_platform`, называющие ядро, не остались (совпали только
  строки ADR-0152, 0153, 0154 и 0254, уже означавшие контракт, и фрагмент changelog LY20). Команды:
  `git mv crates/flui-platform-api crates/flui-platform`; `sd -s` `flui-platform-api` →
  `flui-platform`, `flui_platform_api` → `flui_platform` с исключениями LY20 и фрагментом
  `changelog.d/platform-flui-native.md`; `cargo metadata`; `TRYBUILD=overwrite cargo test -p
  flui-platform --test compile_fail` (снимки не изменились); `cargo fmt --all`, `taplo fmt`. По
  смыслу: ADR-0152, 0153, 0154 и 0254 называют итоговые имена; README и `docs/crates.md` —
  «no OS code»; причина allowlist ADR-0151 покрывает оба переименования, счёт остался 17. Отдельно:
  снимок `remaining_recognizer_details_are_non_exhaustive.stderr` у `flui-interaction` отставал от
  входа с #1811 и перегенерирован. Проверки на Windows: `xtask checks` (с `key-vocabulary` и
  `recording-vocabulary`), `workspace`, `reach`, `deps`, `wasm-check`, clippy `--workspace
  --all-targets -D warnings` — зелёные; nextest по контракту и девяти зависимым — 759 из 760 до
  исправления снимка, trybuild `flui-interaction` после него — зелёный; doctest'ы десяти крейтов — 321
  прошли. Повтор команд ничего не меняет.
- [x] **LY24** — ветка `platform/dead-linux`. Удалены `LinuxPlatform` (заглушка с
  `unimplemented!()` в конструкторе и большинстве методов) и `LinuxWindowExt` (`window_ext.rs`, без
  реализаций) с экспортами из `lib.rs` и `platforms/mod.rs`, а с ним и `bitflags` из Linux-зависимостей
  ядра (первый прогон CI: `cargo shear`). Живой слой `platforms/linux` (`display`, `host_notices`,
  `lifecycle_model`, `x11_window_probe`, `accessibility`) остался; его модуль описывает, что в нём и
  кто им пользуется. `is_wayland` стал `Protocol::current()` в `linux/display/protocol.rs`, им
  пользуется `winit/display_linux.rs`; `is_x11` не звал никто. Строка про `LinuxPlatform` ушла из
  незавершённого фрагмента changelog дисплеев Linux. Проверки: `rg 'unimplemented!'` по
  `platforms/` — ноль; `xtask checks`, `workspace`, `deps` — зелёные; clippy `-p flui-native -p
  flui-app --all-targets -D warnings` — зелёный; ядро под `x86_64-unknown-linux-gnu`
  (`winit-backend,a11y`) — `cargo check` зелёный; nextest `-p flui-native` — 213 прошли.
- [x] **LY23** — ветка `platform/contract-paths`. Ядро не реэкспортирует контракт: из
  `traits/mod.rs` ушли 64 имени `flui_platform`, `NamedKey` и три имени
  `flui_semantics::platform`, из корня `lib.rs` — модуль `data_transfer`, имена предпочтений,
  передачи данных, `CursorIcon` и контрактные имена списка `traits`; корень оставляет только свои
  (`Platform`, `HostWindow`, owner-типы, хосты, ошибки). Пути переписал одноразовый инструмент на
  `syn` (вне репозитория): лист `use`-дерева с корнем `flui_native`/`flui_native::traits` (в
  `crates/flui-native/src` — `crate`/`crate::traits`) и контрактным именем уходит в
  `flui_platform::…`, `flui_semantics::platform::…` или `flui_platform::data_transfer`; встроенные
  пути и doc-ссылки — тем же отображением. Руками: относительные `super::` в `traits/`, `$crate::`
  в `impl_window_callback_setters`, doc-пример `lib.rs`, ARCHITECTURE.md и README обоих крейтов.
  Проверки: `cargo check --workspace --all-targets` на Windows; ядро под `aarch64-apple-darwin` и
  `x86_64-unknown-linux-gnu` (`winit-backend,a11y`); ядро и `flui-app` под `aarch64-linux-android`;
  `flui-app`, `flui-runtime`, `flui-testing` под Linux и macOS (`CC`/`AR` из LLVM для `psm`);
  `wasm-check`; clippy `--workspace --all-targets -D warnings`; `xtask checks`, `workspace`,
  `reach`, `deps` — зелёные; nextest пяти изменённых крейтов — 422 прошли; doctest'ы — 80 прошли;
  `rg` по `flui_native::` с контрактными именами пуст; `xtask doc-strict` зелёный после правки
  относительной doc-ссылки в `traits/owner.rs`, которую нашёл CI. iOS локально не собирается
  (`objc2-exception-helper` требует SDK), его проверяет CI.
- [x] **LY39** — ветка `platform/api-closure`. `cargo xtask api-closure` читает rustdoc JSON
  (nightly, `rustup run`) каждого пакета с `tier-kind = "stable"` (`flui`, `flui-platform`,
  `flui-protocol`) через таблицы `index`, `paths`, `external_crates` и все `"id"` элемента;
  blanket и synthetic impl пропускаются; разрешено ADR-0089 §2, `serde`/`schemars` — только внутри
  impl их трейта; `[package.metadata.flui.api-closure] exceptions` (с ADR в `exit`, неиспользуемое —
  находка) и `deny` (режим для OS-типов ядра, LY36). Исключений `ui-events`, `keyboard-types`, `dpi`
  не понадобилось: контракт их уже не называет, они вошли в его `reach-forbid`. Первый прогон нашёл
  два множества на `bitflags` (`TransferActions`, `PointerKindSet`) — они стали своими (`flag_set!`),
  `bitflags` ушёл из контракта; ADR-0211 п. 12 — `Superseded-by` ADR-0151 §5, ADR-0089 — поправка.
  CI: job `doc` ставит nightly, гоняет гейт и `--self-test`, затем отчёт `cargo semver-checks` по
  `flui-platform` относительно базы PR (R2.7, не гейт). Проверки: самотест — 4 случая; `cargo test
  -p xtask` — 45; nextest контракта, ядра, interaction, widgets и testing — 548; doctest'ы
  контракта — 46; clippy, `checks`, `workspace`, `reach`, `deps`, `actionlint`, `zizmor` — зелёные.
- [x] **LY22** — ветка `platform/host-mechanism`. `host.rs`: `Host` (`NAME` и 13 позиций),
  трейты позиций и `Absent<F>` с маркерами `host::position::*`; `platform.rs`: `HostPlatform<H>` —
  единственная реализация `Platform` (`rg "impl.* Platform for"` по ядру — одна строка), у
  `Platform` не осталось тел по умолчанию. Семь типов `<Os>Platform` стали `<Os>Loop` (позиция
  `Loop` и её состояние), публичные имена — `HostPlatform` над таблицами `hosts/{windows,apple,
  android,web,winit,headless}.rs`; конструкторы — в таблицах, прочие публичные методы (тестовые
  ручки headless, `app()`, `on_scene_event`, `run_event_loop`) — на `HostPlatform<H: Host<Loop =
  …>>` в файле платформы. Решения, которых строка не называла: (1) позиции уровня приложения —
  статические функции над loop'ом хоста `O`: состояние сегодня живёт в loop'е, и где у позиции нет
  своего объекта, таблица называет сам loop; LY27–LY33 дают такой позиции свой тип (абзац над
  таблицей дополнен); (2) `Absent<F>` отвечает словарём каждой позиции на сегодня (пустая передача,
  `HapticOutcome::Unavailable`, неизвестные предпочтения — как `Platform::preferences` у хоста без
  наблюдения, окно не открывается — `OpenWindowError::Backend`, сброс регистрации); перевод на
  `Unsupported { NotOnThisPlatform }` записан в строку LY38; (3) оконные позиции (Text,
  Accessibility, Haptics, Chrome) таблица называет, но их связь с тем, что отдаёт окно, и снятие
  умолчаний `HostWindow::accessibility` и `PlatformWindow::{text_input, haptics, system_chrome}` —
  тоже LY38 (строка дополнена), трейт позиции `Text` вместо маркера — LY30; (4) модуль `host`
  доступен снаружи (`#[doc(hidden)]`) ради R4.11: trybuild-крейт вне ядра реализует `Host` без
  одной позиции; построить `HostPlatform` снаружи нельзя; (5) у winit ввод текста есть
  (`WinitTextInput`: IME и область курсора), так что позиция `Text` у него не `Absent` (строки
  LY22 и LY30 поправлены); (6) исполнитель, путь приложения, открытие URL и путей, диалоги и имя
  композитора — трейт `host::Services` loop'а до LY61, без тел по умолчанию: каждый хост пишет свой
  ответ, инертный тоже; (7) `platforms::android` стал `pub(crate)`. Изменённые внутренние тесты:
  юнит-тесты, которые называли старую структуру (`WindowsPlatform`, `WinitPlatform`, headless),
  переименованы или идут через `owner()`; `tests/android_exit_path.rs` читает исходник `run`, и его
  якорь следует новой сигнатуре. Clipboard Android — внутрипроцессная заглушка `MockClipboard`,
  которую LY32 оставляет в таблице до LY50. Таблицы: `Absent` —
  drag macOS и iOS, accessibility Android, iOS, Web (и без `a11y` на Windows, macOS, winit), Text
  iOS, haptics winit, chrome Windows, macOS, Web, winit. `module-dag` получил ось фич
  (`features`, `platforms`, `groups`, `base`, `hosts`; самотест — 52 находки), `flui-native`
  объявлен с одним `hosts`, посаженное `crate::hosts::…` в файле платформы отказывает; скан больше не
  считает `let`, макрос-оператор и выражение под `cfg(test)` production-кодом. trybuild
  `host_table_names_every_position`: таблица без `Storage` — ровно E0046. Scratch-крейт: посаженный
  `unsafe` в модуле `cfg(any(target_os = "macos", test))` с `forbid(unsafe_code)` не компилируется
  в тестовой сборке; `const _: () = assert!(..)` в FFI-модуле `cfg(target_os = "macos")` с неверным
  размером падает (E0080) под `cargo check --target aarch64-apple-darwin` — так, как его проверяет
  job CI `cross-typecheck` (локальный `xtask cross-typecheck` требует `cargo-zigbuild`), — и не
  собирается на Windows. AGENTS.md
  «Definition of Done» и `docs/extending.md` (строка «Платформа») дополнены. Проверки на Windows:
  clippy `-p flui-native -p flui-app --all-targets -D warnings`, с `winit-backend,a11y`; nextest
  `-p flui-native -p flui-app` — 263 прошли, с `winit-backend` — 243; `wasm-check`, `wasm-test` (48
  утверждений), `doc-strict`, `checks`, `globals` (55 записей), `module-dag` (+ самотест); clippy Android (`aarch64-linux-android`),
  macOS lib (и с `winit-backend,a11y`), iOS lib (`DOCS_RS=1`: `objc2-exception-helper` без SDK).
  WSL Ubuntu 24.04: clippy `winit-backend,a11y`, `linux_preferences` — 16 случаев. Galaxy S21:
  проба собирается, рисует, `MediaQuery` получает отступы и живую смену `animator_duration_scale`
  (Full → Reduce → Full). Браузер: `flui build web` пробы — платформа стартует, canvas-окно, WebGPU
  инициализирован, ошибок нет. macOS и iOS — в очереди vanyastaff/flui#1840.
- [x] **LY25** — ветка `platform/owner-module`. `traits/` исчез: `Platform`,
  `PlatformReadyCallback`, `PlatformExecutor`, `PathPromptOptions` — в `platform.rs` рядом с
  `HostPlatform<H>`; `HostWindow` и `OwnerThreadToken` — в `host_window.rs`; `traits/owner.rs` и
  `shared/{owner_signal,owner_slot,panic_boundary}` — в `owner/{mod,signal,slot,panic_boundary}.rs`
  (`git mv`). Корень крейта экспортирует те же имена, новых публичных нет; путей `traits::` вне ADR,
  `docs/plans` и выпущенных фрагментов changelog не осталось. `Platform` и `HostWindow` запечатаны
  приватным супертрейтом с `#[diagnostic::on_unimplemented]`; окна реализуют печать через
  crate-private маркер `CoreHostWindow` и одну обобщённую реализацию с
  `#[diagnostic::do_not_recommend]`, чтобы текст ошибки не зависел от цели и фич. trybuild
  `platform_and_host_window_are_sealed`: внешние `impl Platform` и `impl HostWindow` со всеми
  методами дают ровно по одной ошибке печати (одинаково на Windows и Linux, с `winit-backend` и без).
  Случаи удалённых имён импортируют из корня (E0432); случай `owner_thread_token_has_no_default`
  получил по снимку на написание `Default`: rustc пишет `std::default::Default` на Windows и
  `Default` на Linux, и снимок с Windows с LY20 не совпадал в Linux-job trybuild (Linux-снимок
  совпадает и с `winit-backend,a11y`; macOS не проверен — trybuild в CI идёт на Linux).
  `owner_thread_token_is_private` теперь доказывает, что корень не реэкспортирует токен (E0432):
  модуля `traits`, через который `pub(crate)`-реэкспорт давал E0603, нет. `globals`:
  `owner::slot::{LAST_SLOT_KEY, OWNER_VALUES}`; из пяти записей под `shared::` переехали две,
  три (`clipboard_lock::{CLIPBOARD, reentrancy::SESSION_OPEN}`, `pointer_assembly::capture::INSTALLED`)
  принадлежат буферу обмена и вводу и переедут с ними (LY32, LY29); `module-dag`: `base = ["owner"]`.
  Известные пределы: печать `Platform` держится и на том, что `HostPlatform::from_loop` —
  `pub(crate)` (внешний `Host` реализуем, но платформу из него не построить); `owner/signal.rs`
  называет `shared::{InputWake, WindowCallbacks}` — оконную машинерию: когда она уйдёт в `window/`
  (LY28), `owner/` получит трейт-хук вместо этого ребра (ADR-0151 §10). Решения:
  `panic_boundary` — `pub(crate)`, его тест переехал к модулю (AGENTS.md DoD); doctest удалённых
  имён ушёл с `traits/mod.rs` (их держат четыре trybuild-случая). Проверки на Windows: clippy
  `-p flui-native -p flui-app -p flui-testing --all-targets -D warnings` и с `winit-backend,a11y`;
  nextest `-p flui-native -p flui-app` — 263, с `winit-backend` — 243; `compiler_guards`;
  `wasm-check`; clippy Android, macOS lib, iOS lib (`DOCS_RS=1`); `doc-strict`, `checks`,
  `module-dag` (243 модуля, 1 base), `globals` (55), `workspace`, `reach`, `file-length`. WSL:
  clippy `winit-backend,a11y`, `linux_preferences` — 16, юнит-тесты `owner::` — 4,
  `compiler_guards` — зелёный.
- [x] **LY26** — ветка `platform/owner-call-point`. `shared/handlers.rs` → `owner/handlers.rs`
  (`git mv`, 2 499 → 2 260 строк); точка вызова — `owner/call_point.rs` (384 строки и тесты рядом).
  Форма: трейт `HandlerSlot { take, restore, replace }` — каждый метод берёт и отпускает лок или
  заём своего слота и не запускает пользовательский код; `call(&slot, |handler| ..) -> Called<R>`
  (`Vacant`, `Returned(R)`, `Panicked(Failure)`) берёт обработчик, вызывает его без локов в
  `catch_unwind` и возвращает на место, если за время вызова не поставили новый (тогда новый
  остаётся, вызванный уходит в `retire`); `Called::{or_resume, or_contain}` решают, поднять панику
  дальше (headless, winit, FIFO окна, Web, Android) или погасить её (выход и конец сессии Win32,
  AppKit, UIKit, проходы ввода, немедленные события iOS); `install` и `retire` — замена и сброс вне
  локов (паника деструктора погашена, во время раскрутки значение удерживается). Первый отказ:
  полезная нагрузка — паника самого обработчика; обработчик, вытесненный во время упавшего вызова,
  удерживается (`mem::forget`, ADR-0127 §2) в одном месте — `call`; `thread::panicking()` для этого
  не читается; брошенный `Failure` удерживает нагрузку. Слоты: `Field` — поле состояния за
  `Mutex`/`RefCell` (трейт `Guarded`), `restoring_while` — macOS (`accepts_windows`) и сцена iOS
  (`signal.accepting`); `SharedField` — хук `Arc` (wake deadline) вызывается клоном и остаётся
  установлен; `Once` — одноразовый вызов (quit); `Mutex<Option<H>>` — слот окна; в
  `owner/handlers.rs` — `Latched` (защёлки окна `cleared`/`lifecycle_closed` читаются под локом
  слота), `InputSlot` (слот владельца) и `Closing` (`FnOnce` `on_close` в конверте). Удалены:
  `RunningSessionEndHook`, `CallbackLease`, `InputLease`, `WindowEventLease`,
  `WindowEventHandlerLease`, `ExitPolicyHookLease`, а также `TakenSessionHook` (headless),
  `Walk` уведомлений (`shared/system_notice.rs`, общий для всех хостов), `with_leased_handler` и
  `DeadlineQuery` Win32, `invoke_and_drop_owner_callback`, `PlatformHandlers::{invoke_*,
  run_session_end}`; ручной take/restore macOS (`ask_session_end`, `drain_reopens`, `evaluate`,
  `finish`), iOS (`emit_scene`, `announce_session_end`, `invoke_quit`, сброс потребителя сцены,
  `arm_deadline`, регистрации) и Web (`dispatch_window_event`, `report_session_phase`, `quit`).
  Исправлено: Android `invoke_quit` под мьютексом обработчиков; headless `create_mock_window`
  больше не объявляет `Created` — это делают `open` и `resolve_next` после лока; `Drop`
  заменённого обработчика под локом — Android, iOS, headless, а также найденные сверх строки:
  winit (все регистрации и обе аренды, отдававшие вытесненный обработчик в замыкании
  `with_state`), регистрации macOS, сеттеры макроса окон (все хосты, кроме Win32 и Web), хук конца
  сессии и хук уведомлений, заменившие себя изнутри вызова; Win32 и macOS роняли вытесненный хук
  после погашенной паники. Решения, которых строка не называла: (1) уведомления системы идут через
  ту же точку — это обработчик приложения; (2) регистрации после `clear()` по-прежнему принимаются
  и срабатывают один раз (поведение не менялось); (3) headless сверяет пустоту окон и после хука
  выхода: окно, открытое самим хуком, теперь удерживает платформу (раньше она выходила); (4) quit
  Win32 в `RunGuard` — одноразовый вызов с погашенной паникой, его захваты после паники
  удерживаются; (5) `PlatformHandlers` не перестраивался, корень получил `InputCallback` и
  `InputWake` вместо пути `shared::`. `owner/signal.rs` называет теперь
  `super::handlers::{InputWake, WindowCallbacks}` — ребро внутри `owner/`; оконная машинерия
  остаётся в `owner/` до LY28 (и LY44/LY45 её удаляют). Тесты: юнит `owner::call_point::tests` —
  11; `tests/handler_reentry.rs` (модуль `tests/main.rs`) — таблицы headless (6 случаев) и Win32
  (4), `replaced_winit_handlers_reach_the_platform`; в крейте
  `winit_handlers_run_outside_the_state_lock`; строка `a_hook_that_replaced_itself_and_failed_is_retained`
  в `native_host_events`; wasm `web_session_end_hook_reenters_the_platform`. Каждый случай идёт в
  рабочем потоке с таймаутом 10 с: регрессия — это дедлок, а не зависание. Против исходников базы
  (`git restore --source=<база>` для `src/`): headless — все 6 случаев падают (5 — «deadlocked — a
  handler ran, or a replaced one was dropped, under a lock the platform holds», 1 — «the failed
  hook's captures are retained, not dropped»), так же в WSL; winit — «winit application handlers:
  deadlocked …» (Windows и WSL); Win32 — «a failing exit-policy hook keeps the first failure» и
  строка `native_host_events` падают с «the failed hook's captures are retained, not dropped»;
  внутренний тест winit с возвращёнными арендами — «deadlocked: a handler ran, or was dropped,
  under the state lock». Не падают на базе (регрессионные, дефекта там не было): Win32 «a
  window-event handler installs, opens and quits», «replaced application handlers…», «replaced
  window callbacks…» (Win32 отпускал `RefCell` и временный гард до вызова) и wasm-тест Web.
  Мутации точки: `retire` вместо удержания после отказа роняет
  `a_failing_call_retains_the_handler_it_displaced` и headless-случай отказа; возврат поверх нового
  обработчика роняет `a_handler_installed_during_a_call_replaces_the_one_that_ran`. Проверки на
  Windows: clippy `-p flui-native -p flui-app -p flui-testing --all-targets -D warnings` и
  `-p flui-native --all-targets --features winit-backend,a11y`; nextest `-p flui-native -p
  flui-app` — 273 (2 пропущены), с `winit-backend` — 255; `wasm-check`, `wasm-test` (49
  утверждений); clippy Android (`-p flui-native -p flui-app --all-targets`), macOS lib, iOS lib
  (`DOCS_RS=1`); `doc-strict`, `checks`, `module-dag` (244 модуля, 1 base), `globals` (55),
  `file-length`, `markers`, `workspace`, `reach`. WSL Ubuntu 24.04: clippy `winit-backend,a11y` и
  без фич (`-p flui-native -p flui-app`); nextest `-p flui-native` (`FLUI_HEADLESS=1`) — 168; с
  `winit-backend` под xvfb — 254 из 255: `a_randr_reconfiguration_on_a_real_server_is_published`
  падает и на базе (RandR в xvfb этого окружения); `linux_preferences` — 16. Galaxy S21 (Android
  15, проба `flui create --local`): выход по системе (Back завершает Activity) — `Destroy`, цикл
  завершён, службы приложения остановлены за ~40 мс, без ANR и паники; повторный запуск в том же
  процессе — новый `android_main`, `MediaQuery` приходит. Предупреждение `close_this_window:
  dispatch refused` при этом выходе было и до LY26 (та же проба на дереве LY22). macOS и iOS —
  только проверка типов, их места покрыты общей точкой и её тестами. После ревью: Win32
  `RunGuard` отдаёт набор обработчиков `retire` после quit (раньше последний `Rc` падал без
  границы, в т. ч. во время раскрутки) — строка `a handler dropped at teardown is contained`
  (без исправления: «a handler's destructor escaped the teardown»); строка headless `an exit-policy
  hook that opens a window keeps the platform` закрепляет решение (3) (без повторной проверки окон:
  «the window the hook opened keeps the platform running»); `PlatformHandlers` ушёл из публичного
  API (его поля обходили точку вызова), публичная doc `set_exit_policy_hook` больше на него не
  ссылается. Покрытие по хостам — честно: headless — все пять видов, каждый падает без
  исправления; Win32 — все пять, без исправления падают строки паники и teardown, остальные — страж
  регрессий (на Win32 этих дефектов не было); Web — замена, quit, `Drop` (страж); winit — замена,
  `Drop`, паника (юнит); Android, iOS, macOS — исправления через общую точку и её юнит-тесты,
  проверка типов, прогон Android на S21; запуск macOS и iOS — vanyastaff/flui#1840. Строке «все
  падают без исправления» отвечают headless, winit и Win32-строки паники и teardown. Известный
  дефект до LY26, не исправленный здесь: в `SystemNotices::dispatch` уведомление из другого потока
  между выходом из цикла и сбросом `delivering` ждёт следующего уведомления — чинится отдельной
  задачей вне серии.
- [x] **LY27** — ветка `platform/owner-loops`. Цикл каждой платформы — модуль `owner/` (тела
  перенесены без правок, кроме путей, `use` и видимости; `git mv`, где файл переезжает целиком;
  публичные пути не изменились, changelog не нужен). Windows: `windows/platform.rs` разделён —
  цикл сообщений, `WindowsLoop`, `WindowIdentity`, класс окна и его сырая процедура → `owner/windows.rs`
  (1 431 строка), контекст окна, шлюзы `with_window_context`/`teardown_route` и тело процедуры
  остаются в `platforms/windows/platform.rs` (1 659) до LY28; `platform/{wake_deadline,
  modal_frame_tests}.rs`, `modal_loop.rs`, `com_apartment.rs` → `owner/windows/`;
  `owner_control.rs` и `owner_control/wake.rs` → `owner/windows/control.rs`, `control/wake.rs`;
  `preferences/receiver.rs` → `owner/windows/broadcast.rs`; `util.rs` удалён: класс и курсор — в
  `owner/windows.rs`, `get_x/y_lparam` — в `events.rs`, `hiword` — в `platform.rs`, пересчёт в
  device-пиксели и `USER_DEFAULT_SCREEN_DPI` — в `window_placement.rs`, `SIZE_*` читаются из
  `shared::visibility`, шесть мёртвых функций удалены. Apple: `owner/apple.rs` (корень),
  `macos/platform.rs` → `owner/apple/macos.rs` (454) и `macos/{loop_control,owner_lane,wake_pump}.rs`
  → `owner/apple/macos/`, `ios/platform.rs` → `owner/apple/ios.rs` (1 307) и
  `ios/{native_owner,scene,executor}.rs` → `owner/apple/ios/`; реализации позиций, которые лежали в
  файлах цикла (окно, дисплеи, предпочтения macOS; окно, дисплеи, буфер обмена, предпочтения iOS),
  — в `platforms/{macos,ios}/positions.rs`, как у winit (иначе `macos/window.rs` превысил бы 3 000);
  `IOSSceneEvent`, `IOSSceneSessionId`, `IOSSceneAttachmentId`, `IOSExecutor` реэкспортирует
  `platforms::ios`. Android: `android/mod.rs` → `owner/android/looper.rs` (1 198; новый
  `platforms/android/mod.rs` — корень фич, 29 строк), `java_view.rs` → `owner/android/java_view.rs`,
  `shared/android_ndk.rs` → `owner/android/ndk.rs`; `owner::android` собирается под
  `cfg(any(target_os = "android", test))`, чтобы тесты `ndk` шли на хосте; `MockClipboard` —
  `platforms/android/clipboard.rs`. Путь `include_bytes!` не изменился (глубина каталога та же),
  `android_dex.rs` теперь называет встраивающий файл (`EMBEDDER`) и требует, чтобы единственный
  `include_bytes!` в нём вёл к `android/classes.dex`. Web: `web/platform.rs` → `owner/web.rs` (409),
  `listener.rs`, `executor.rs` → `owner/web/`. winit: `winit/platform.rs` → `owner/winit.rs` (2 764
  production-строки из 2 997), `control.rs` и `platform/real_loop_tests.rs` → `owner/winit/`;
  `platform/positions.rs` и `platform/linux_notices.rs` (фичи) → `platforms/winit/`. headless: ходы
  владельца (`HeadlessLoop`, `HeadlessState`, `TestExecutor`, `HeadlessOwnerTurns`,
  `HeadlessExitReevaluation`, `HeadlessDeferredWindowOpens`, отложенные хуки владельца, три ручки
  `HostPlatform`) → `owner/headless.rs` (773); двойники фич остаются в `headless/platform.rs` (1 735).
  `redraw_poll.rs` → `owner/redraw_poll.rs`. Таблица Windows называет то, через что `owner/`
  диспетчирует: `owner::windows::WindowsHost` — `const WINDOW_HOOKS: WindowHooks` (`&'static
  [&'static dyn WindowMessages]`) и `fn broadcast_subscribers(&OwnerObjects<'_>) -> Vec<Rc<dyn
  Subscriber>>`, оба в порядке, в котором их слышит сообщение; `hosts/windows.rs` реализует его для
  `Windows` и строит цикл `WindowsLoop::with_config::<Windows>(config)`. Трейты-хуки:
  (1) `owner::windows::WindowMessages` — `fn created(&self, &CreatedWindow)` и `unsafe fn
  message(&self, HWND, u32, WPARAM, LPARAM) -> Option<LRESULT>` (оба с пустым умолчанием).
  Процедура класса — `window_proc::<H>`: набор хуков — тип, она держит границу паники и предлагает
  сообщение `H::WINDOW_HOOKS` по порядку — первый `Some` завершает, `None` передаёт дальше,
  неотвеченное уходит в `DefWindowProcW`; созданное окно объявляет себя тем же хукам через
  состояние владельца (`OwnerShares::window_hooks`). Класс Windows держит один на процесс, поэтому
  регистрация запоминает своего хоста (`static WINDOW_CLASS: OnceLock<Result<TypeId, String>>`,
  запись `globals` класса `process`), и цикл другого хоста получает `PlatformError::Init`, а не
  тихо чужие хуки; неудачная регистрация теперь ответ и всех последующих вызовов (раньше — только
  первого). Реализуют `platforms::windows::WindowProc` (прежнее тело процедуры, отвечает на всё:
  окно, ввод, текст, drag, lifecycle, включая умолчание) и `platforms::windows::Touchpad`
  (только `created`: viewport DirectManipulation, прежде прямой вызов из `install_native`;
  порядок создания тот же — TSF, drop target, viewport, показ); монолит `WindowProc` и
  `created`-only `Touchpad` делит по фичам LY28 (и LY29–LY33). (2)
  `owner::windows::broadcast::Subscriber` — `attach`, `detach`, `answer_after_panic(&self, u32) ->
  Option<LRESULT>` (пустые по умолчанию) и `message(&self, HWND, u32, WPARAM, LPARAM) ->
  Option<LRESULT>`. Приёмником владеет контроль владельца (поле контекста, после источника
  предпочтений: тот закрывает приём при сбросе, и сообщение во время разрушения окна никого не
  будит), собранный из списка `Receiver::new(impl IntoIterator<Item = Rc<dyn Subscriber>>)`,
  который даёт таблица: `PreferenceBroadcasts` источника предпочтений (`WM_SETTINGCHANGE` с
  `SPI_SETWORKAREA`, `WM_DISPLAYCHANGE`), затем `HostEvents` lifecycle. Приёмник сам отвечает на
  `WM_NCCREATE`/`WM_NCDESTROY`/`WM_CLOSE`, остальное говорит подписчикам по порядку, пока один не
  ответит, держа их `Rc<[…]>`, а не заём контекста; паника подписчика погашена, и сообщение
  получает ответ, который на этот случай даёт подписчик (`answer_after_panic`: lifecycle разрешает
  запрос конца сессии), иначе 0 — `WM_QUERYENDSESSION` больше не в приёмнике. Класс, заголовок и
  логи приёмника — его собственные (`FLUIBroadcasts_…`, «FLUI system broadcasts», «broadcast
  receiver»; `tests/windows_host_events.rs` ищет окно по новому заголовку). Дисплеи делят
  подписчика и флаг приёма с предпочтениями: свой подписчик дисплеев — не малая правка (порядок
  `WM_SETTINGCHANGE` и время жизни приёма меняются), его берёт LY31 вместе с состоянием дисплеев.
  winit на Windows строит свой приёмник для своего источника предпочтений (поле
  `preference_broadcasts`, сбрасывается после источника). `platforms/android/window.rs:626`
  (`app.create_waker().wake()` в `request_redraw`) — использование пробуждения `AndroidApp`, а не
  машинерия пробуждений: остаётся в окне. Что `owner/` ещё называет по имени (строка хука не
  требует): состояние фич, которое держит цикл, пока фича его не заберёт (LY28–LY33) — у Win32
  `WindowMap`/`WindowsWindow`, `WindowsDisplays`, `OleTransfer`, `PreferenceSource`, `HostEvents`
  в контексте владельца (`OwnerObjects` отдаёт их таблице); процедура окна владельца диспетчирует
  `START_DRAG`/`MODAL_FRAME` drag по имени; `redraw_deadline_windows` и `modal_loop` (`impl
  WindowContext`) читают контекст окна; Apple — окна, дисплеи, предпочтения, наблюдатели и
  источники уведомлений macOS, `IOSWindow` (сцены), `IosDisplays`, `IOSPreferences`,
  `IOSClipboard`; Android — окно, ввод, текст, дисплеи, предпочтения, память, drag, а `java_view`
  (мультиплексор колбэков `FluiView`) — drag и `preferences::with_activity`; Web — буфер обмена,
  дисплеи, предпочтения, drag, наблюдатели; winit — окна, перевод ввода, источники дисплеев и
  предпочтений (в т. ч. Win32 и macOS), `linux_notices`; headless — `MockWindow`,
  `create_mock_window`, `DragDouble`, `HeadlessDisplays`. Решения: (1) `owner/<os>` — корень и
  подмодули, а не один файл: переносы остаются переименованиями, `owner/windows` и `owner/winit`
  укладываются в 3 000; (2) `globals`: место трамплина — платформа, её каталог в `platforms/` или
  модуль в `owner/` (группа `apple` раскрыта в `macos`/`ios`, собственный модуль группы,
  `owner/apple.rs`, — ничей), по-прежнему один на платформу; ADR-0097 получил строку `Amended`
  со ссылкой на ADR-0151 §10, как этот репозиторий записывает поправки, следующие из другого
  принятого ADR; (3) `WindowsLoop::new` удалён: таблица строит цикл через `with_config`;
  (4) маркеры `US2`/`US3` headless удалены вместе с записью allowlist; (5) `android_dex.rs` называет
  встраивающий файл (`EMBEDDER`), а проверку ведёт `embedding_failure`. Записи `globals`
  перекодированы (9), `REGISTER_WINDOW_CLASS` стал `WINDOW_CLASS`, итого 55. Тесты:
  `modal_frame_tests` без правок, `wake_deadline` и `real_loop_tests` — только пути (doc модуля
  последнего и строка, шлющая настройку через приёмник winit); сверх путей — приёмник строится
  списком в `hosted_receiver`, `a_receiver_the_os_keeps_stays_registered_and_inert` и
  `a_delegate_that_outlives_its_source_is_inert`, `preference_retry_tests` и
  `display_message_tests` шлют сообщения в окно приёмника из `OwnerShares` и получили импорты
  `Platform`, `PlatformError`, `WM_QUIT` (их давал `use super::*` из старого `platform.rs`);
  `tests/android_exit_path.rs` читает `owner/android/looper.rs`; conformance и трассы Windows не
  тронуты. Новые: `owner::windows::tests::a_window_message_reaches_the_hooks_in_order`
  (первый `Some` выигрывает, `None` передаёт, без ответа — `DefWindowProcW`, каждый хук узнаёт о
  созданном окне), `a_second_host_is_refused_the_window_class`,
  `broadcast_subscribers_hear_a_message_in_order` (порядок и ответ при панике), строка
  `trampoline_limits` с трамплином в `owner/apple.rs`, `a_backend_that_embeds_another_dex_fails`;
  каждый падает со сломанным правилом (мутации: возврат последнего ответа хука — «left: (0,
  ["first", "second"])», принятие чужого хоста — тест отказа падает, ответ при панике всегда 0 —
  «left: (0, ["first"])», модуль группы как слот — нет находки `owner::apple::APPLE`,
  `embedding_failure`, всегда `None`, — падает `android_dex_contract`). Ревью (отдельный агент,
  старые и новые тела построчно): тело процедуры совпадает побайтно (1 034 строки), приёмник — по
  каждому сообщению; ревью PR: хуки — тип хоста вместо `static` с `expect`, подписчиков называет
  таблица, тест `android-dex` и модуль группы в `globals`. Проверки на Windows (после rebase на
  `origin/main` @ `0fdbd53ae`): `cargo fmt --check`; clippy `-p flui-native -p flui-app -p
  flui-testing --all-targets -D warnings`, `-p flui-native --all-targets --features
  winit-backend,a11y` и `-p xtask --all-targets`; nextest `-p flui-native -p flui-app` — 278 (2
  пропущены), с `winit-backend` — 260; nextest `-p xtask` — 45; `wasm-check`, `wasm-test` (49
  утверждений, до ревью); clippy Android (`-p flui-native -p flui-app --all-targets`), macOS lib (и
  с `winit-backend,a11y`), iOS lib (`DOCS_RS=1`); `android-dex` (те же байты, встраивание
  проверено), `doc-strict`, `checks`, `module-dag` (251 модуль, 1 base) и самотест (52),
  `globals` (55) и самотест (15), `file-length`, `markers`, `testing`, `docs-paths`,
  `workspace`, `reach`; rustdoc `--document-private-items` с `winit-backend,a11y`. WSL Ubuntu
  24.04: clippy `winit-backend,a11y` и без фич (`-p flui-native -p flui-app`); nextest `-p
  flui-native` (`FLUI_HEADLESS=1`) — 170; до ревью с `winit-backend` под xvfb — 256 из 257,
  падает `a_randr_reconfiguration_on_a_real_server_is_published` (как на базе); `cargo test
  --features winit-backend` под xvfb — lib 188 из 198: 9 тестов реального цикла winit/X11 и RandR
  падают, когда идут в одном процессе (winit даёт один цикл на процесс), так же на базе (архив
  `019ecee7b`: 186 из 196), по одному и под nextest проходят; остальные бинари и doctest'ы
  зелёные; `linux_preferences` — 16. Galaxy S21 (Android 15, проба `flui create --template
  counter --local`, сборка с этими правками): три касания «Increment» — счётчик 3, свайп и `input
  text` счётчик не меняют, ошибок и паник нет; системный Back — `Destroy`, «Android platform event
  loop finished» (модуль `flui_native::owner::android::looper`), службы остановлены; повторный
  запуск в том же процессе — новый `android_main`, счётчик с нуля, касание — 1; Home, затем `am
  kill` (выход по системе: процесс убит сигналом 9) и холодный запуск — новый процесс, два касания
  — 2, Back — чистое завершение; ANR, `FATAL` и предупреждений `flui` нет, настройки устройства не
  менялись. Не запускалось: `cross-typecheck` локально (нет `zig`; его job — в CI); Mac и iPhone —
  в очереди vanyastaff/flui#1840.
- [x] **LY30** — ветка `platform/text`. Фича текста — каталог `text/` (`git mv`, тела без правок,
  кроме путей, `use` и видимости; правки ревью PR — в решениях): модель — `shared/ime_text` →
  `text/ime.rs` (с `diff`/`TextDiff` из правил Web, тесты — `text/ime/tests.rs`,
  `mirror_text_table`), `text_sizing.rs` → `text/sizing.rs`, `text/mod.rs` (трейт позиции и
  корень); Windows — `windows/text_services/*` → `text/windows/tsf/*`, `shared/text_geometry` —
  модуль `geometry` в `text/windows/mod.rs` (тесты `text/windows/geometry/tests.rs`, 2), там же хук
  класса окна `Tsf`; Apple — `macos/text_input.rs` → `text/apple/macos.rs`, `ios/text_sizing.rs` →
  `text/apple/ios.rs`, корень группы `text/apple/mod.rs`; Android — один файл `text/android.rs`:
  `keys` (правило клавиши), `ime` (из `shared/android_ime`, тесты `text/android/ime/tests.rs`,
  `android_ime_adapter`, 14 случаев), `input` (из `android/text_input.rs`), `sizing` (из
  `android/text_sizing.rs`, тесты `text::android::sizing::tests` с
  `android_capture_delivery_receipt`, трюк `#[path]` ушёл); Web — один файл `text/web.rs`: `ime`
  (из `shared/web_text`, тесты `text/web/ime/tests.rs`, `web_text_table`, 2 строки; случаи
  `TextDiff` — в `mirror_text_table`), `input` (из `web/text_input.rs`); `FakeTextInput` →
  `text/headless.rs`, `WinitTextInput` (и `new`) → `text/winit.rs`; Win32 `tsf/{probe,tests}.rs` —
  только пути. Решения: (1) платформа — один файл `text/<os>.rs`, его чистая половина — модуль в
  нём (`cfg(any(<os>, test))`, `forbid(unsafe_code)`), тесты — дочерний `tests.rs`; каталог —
  только у Windows (`tsf/`, пять файлов) и группы `apple`; (2) `diff` и `TextDiff` — модель
  `text::ime`: зеркало Android сравнивает отчёты тем же `diff`, файл платформы не называет файл
  другой; (3) позиция `Text` — трейт `text::TextInput { fn window_text_input(&self) ->
  Option<WindowTextInput>; }`, `pub enum WindowTextInput { Ime(Arc<dyn PlatformTextInput>),
  Store(Rc<dyn TextStoreHost>) }` с `ime(self)` и `store_host(self)`; в таблицах Windows
  `Rc<TextServices>`, Android `Rc<AndroidTextInput>`, Web `Rc<WebTextInput>` (`Store`), macOS
  `Arc<MacOSTextInput>`, winit `Arc<WinitTextInput>`, headless `Arc<FakeTextInput>` (`Ime`),
  `Absent<position::Text>` — `None`. Production-вызывающие — окна всех платформ:
  `PlatformWindow::text_input` и `HostWindow::text_store_host` отдают свой ввод через этот метод.
  Пути `HostPlatform` → `H::Text` нет (окно строит ввод само): «таблица строит, окно держит
  `H::Text`» — LY38; trybuild `host_table_names_every_position` без правок; (4) Windows:
  `WINDOW_HOOKS = [Tsf, WindowProc, DropTargetRegistration, Touchpad]`, порядок создания прежний —
  TSF, drop target, viewport, показ; `DropTargetRegistration` (`platforms/windows/drag.rs`) —
  переходный хук, регистрирующий OLE drop target, уходит с drag в LY32. `CreatedWindow` несёт
  `identity: WindowIdentity` (`Tsf::created` строит `WindowRef::new(hwnd, identity)`, поиска по
  hwnd нет); `window_created(hooks, &window, alive) -> Result<(), WindowGone>` проверяет окно после
  каждого хука и на первом мёртвом останавливается, открытие отвечает `OpenWindowError::Backend`.
  TSF узнаёт о сдвиге в своём `message` (`WM_MOVE`, `WM_EXITSIZEMOVE` → `window_moved`, ответ
  `None`, сообщение идёт дальше в `WindowProc`). В `WindowProc` остались `WM_DPICHANGED` (TSF
  узнаёт о сдвиге после того, как окно применило новый масштаб: `GetTextExt` читает его синхронно,
  а хук `Tsf` стоит до `WindowProc`) и деактивация в `WM_DESTROY` (между close-колбэками и ретайром
  контекста, ADR-0135 §3). `WM_IME_*` нет (TSF, IMM32 не используется — ADR-0135 §7). ADR-0135 —
  строка `Amended` с инвариантом, §6 называет `text::windows::geometry`; (5) Android:
  `owner/android` не называет предметов текста. Хук `owner::android::java_view::InputMethod: Send
  + Sync` — `hold(generation, &str)`, `edited(generation, EditorState) ->
  Option<InputMethodReply>`, `editor_action(action, uptime_ms) -> InputMethodReply`,
  `cursor_updates(mode) -> InputMethodReply`, `pen_down(&AndroidWindow, Point<f64>)`;
  `InputMethodReply { composing: Option<bool>, work: WindowWork }`, `WindowWork = Box<dyn
  FnOnce(&AndroidWindow) + Send>`. Его создаёт таблица (`AndroidHost::input_method() -> Box<dyn
  InputMethod>`, `AndroidLoop::new::<Android>`), реализует `text::android::ViewInputMethod`
  (`Mutex<Mirror>`). `JavaView` ставит флаг составления из ответа и шлёт
  `ViewEvent::RunOnWindow(work)`; цикл выполняет работу над текущим окном (без окна — отброшена, как
  прежде); `ViewEvent::{CursorUpdates, EditorAction}` и `AndroidWindow::editor_action` ушли (коммит
  composition и клавиши действия — в `ViewInputMethod::editor_action`). `EditorState` (текст,
  выделение, составление) — тип `owner::android`, передаётся целиком; `Mirror::report(generation,
  &EditorState) -> Option<ImeEdit>` возвращает правку, применяет её работа ответа. Стилус: цикл
  проверяет `StylusHandwriting` и зовёт `pen_down` для касания пером, `AndroidTextInput::offer_pen`
  решает по своему фильтру. Ребро `owner/android` → `AndroidWindow` (`WindowWork`, `pen_down`)
  остаётся: окно в `platforms/android/window.rs` — уйдёт с окном в LY28; (6) размер текста: трейт
  модели `NativeTextSize { fn query(&self, TextSizeRequest) -> Result<TextSize,
  TextSizingCaptureError>; }`, `CapturedTextSizing` держит `Arc<dyn NativeTextSize>`, реализуют
  `text::android::sizing` и `text::apple::ios` (`Snapshot`) — модель не называет файлов платформ;
  `OwnerPlatform::capture_text_sizing(&window)` → `flui_native::capture_text_sizing(&owner,
  &window)` (функция фичи, `owner/` не называет текста), `flui-app` переведён, фрагмент
  `changelog.d/platform-text.md` (`### Changed`); (7) для conformance-таблицы в `tests/` публичны и
  скрыты `text::web::ime` и `text::android::keys` (`key_composition`), их doc говорит «пока таблица
  не дойдёт до хостов через публичный API» — LY34; `text::android::ime` — `pub(crate)`; (8)
  `forbid(unsafe_code)` — `text::ime`, `android::{keys, ime}`, `web::ime`, `windows::geometry`;
  `expect(unsafe_code)` — `tsf`, `apple::{macos, ios}`, `impl WindowMessages for Tsf`;
  `expect(deprecated)` — `apple::macos` (`objc2::runtime::Protocol`); (9) записей `globals` у
  текста нет (`SHUTDOWN_IN_APARTMENT`, `BEFORE_TSF_ACTIVATION` — `cfg(test)`), итого 55; (10)
  `pub(super)` → `pub(crate)` у того, что текст называет в `platforms/`: `WindowRef::{new, hwnd,
  with_context}`, `ViewContext::text_input`, `get_context`, `with_view_context`,
  `dispatch_input_event`, `handle_input_event`, `route_on_owner` macOS, `KeyRoute::{composing,
  deliver}`, `Listener::install{,_in_phase}`, `EditorSlot::set`, `FocusSync::sync` Web,
  `Acquisition` Android; (11) скрытые `shared::{android_ime, ime_text, text_geometry, web_text}`
  и `host::Text` ушли; ADR-0090, 0247, 0255 цитируют новые пути, ADR-0135 и 0142 — тесты
  `text::windows::tsf::tests`. `module-dag`: черри-пик механизма `retiring` (`1ca40f30b`, он же
  `8ed043411` на `platform/display`; выпадет при rebase после LY31), `features = ["text"]`,
  `retiring = ["platforms"]`, 8 исключений (`exit = "ADR-0151"`, `since = "2026-10-22"`):
  `text::android` → `platforms::android::input` (`editor_action_keys`, LY29) и
  `platforms::android::window` (`AndroidWindow`, LY28); `text::apple::macos` →
  `platforms::macos::view` (методы `NSTextInputClient` на классе view, LY29) и
  `platforms::macos::window` (`route_on_owner`, LY28); `text::web` → `platforms::web::events`
  (`KeyRoute`, `is_focus_handoff`, LY29), `platforms::web::registration` (`EditorSlot`,
  `Listener`, LY28) и `platforms::web::window` (`FocusSync`, LY28); `text::windows` →
  `platforms::windows::platform` (`WindowRef`, `with_window_context`, LY28). Тесты ревью:
  `owner::windows::tests::a_window_a_hook_destroyed_is_announced_to_no_later_hook` (хуки `[First,
  Destroys, Second]`, жив ли — `IsWindow`; без проверки после каждого хука падает «left: ["first
  created", "destroys created", "second created"]»), `a_window_message_reaches_the_hooks_in_order`
  обновлён, `a_window_destroyed_during_activation_fails_the_open` зелёный. Проверки на Windows
  (ветка @ `dd7d318c3`): `cargo fmt --check`; clippy `-p flui-native -p flui-app -p flui-testing
  --all-targets -D warnings` и `-p flui-native --all-targets --features winit-backend,a11y`, `-p
  xtask --all-targets`; nextest `-p flui-native -p flui-app` — 280 (2 пропущены), с
  `flui-native/winit-backend` — 307 (2 пропущены); clippy Android (`-p flui-native -p flui-app
  --all-targets`), macOS lib (и с `winit-backend,a11y`), iOS lib (`DOCS_RS=1`), wasm32 lib;
  `wasm-check`, `wasm-test` (49 утверждений); `module-dag` (259 модулей, 1 фича с 8 файлами
  платформ, 1 base, 469 рёбер, 8 исключений) и самотест (54); `globals` (55) и самотест (15);
  `file-length`, `markers`, `testing` (73 в 60 записях), `docs-paths` (330), `checks`,
  `doc-strict`, `workspace`, `reach`, `android-dex`. WSL Ubuntu 24.04 (cargo без heli): nextest
  `-p flui-native` (`FLUI_HEADLESS=1`) — 171; до ревью — clippy и с `winit-backend` под xvfb 257
  из 257. Galaxy S21 (Android 15), проба `flui create --template counter --local` с `RawTextField`
  и эхом его `on_changed`, сборка из ветки после ревью: касание поля — Samsung Keyboard (русская
  раскладка); `adb shell input text hello` — «hello» в поле и эхе; пробел, «п», «р», «и» —
  составление «при» подчёркнуто, эхо его не содержит; подсказка «привет» — в поле и эхе «hello
  привет »; «м», «и», «р» и Done во время составления — «hello привет мир» в поле и эхе; Back ×2 —
  «Android platform event loop finished», `FATAL`, паник и ANR нет. Предупреждение «a composition
  could not be committed in place» на выходе есть и на базе `e86d0d5b4` — не регрессия. Стилус
  (`offer_pen`) не проверен: у S21 нет S Pen. Проба удалена, настройки устройства не менялись. Web
  (прогон координатора в браузерной панели, проба для wasm32 из ветки @ `812c24249`): «hello web» —
  в поле и эхе; три Backspace, затем « мир» — «hello  мир» в обоих; ошибок в консоли нет; набор
  `web_browser` не запускался — на хосте нет chromedriver. Не запускалось: `cross-typecheck`
  локально (нет `zig`; job — в CI); Mac и iPhone — в очереди vanyastaff/flui#1840.
- [x] **LY32** — ветка `platform/services` (на `main` @ `f4af625af`, затем слияния с `56ad79c96`, LY31, и `a0dd4ba01`, LY33). Четыре каталога фич, модель рядом с файлом на
  платформу, платформа — один файл. `clipboard/`: `lock.rs` (модель: замок сессий Win32, общий у
  Win32 и `arboard`), `windows.rs`, `apple/{macos,ios}.rs`, `android.rs` (внутрипроцессная
  заглушка; её делает `AndroidHost` таблицы до LY50), `web.rs`, `winit.rs`, `headless.rs`
  (`host::Clipboard` для `InMemoryClipboard` контракта). `drag/`: модель `offers.rs` (`format_of`,
  `payload_of_bytes`: правила Android брали их у веб-модуля правил — ребро между платформами);
  `windows.rs` с детьми `com.rs` (COM-объекты OLE), `tests.rs` (набор на настоящих окнах) и
  `crossing/tests.rs`; `android.rs` с `clip/tests.rs`; `web.rs` с `mapping/tests.rs`;
  `winit.rs`, `headless.rs`. `accessibility/`: `bridge.rs`, `windows.rs`, `apple/macos.rs`,
  `linux.rs` (AT-SPI), `headless.rs` (`FakeAccessibility` из `headless/platform.rs`).
  `haptics/`: `degradation.rs`, `windows.rs`, `apple/{macos,ios}.rs`, `android.rs`, `web.rs`,
  `headless.rs` (реэкспорт `FakeHaptics` контракта до LY48). `Absent`: drag macOS и iOS,
  accessibility Android, iOS, Web (и везде без `a11y`), haptics winit (он же хост Linux).
  Переносы — `git mv`; тела — без правок, кроме путей, `use`, видимости и того, что перечислено в
  решениях: новые функции — `ArboardClipboard::or_inert`, `AndroidTransfer::for_view`,
  `HeadlessDragSource::of`, `HeadlessDragAndDrop::of`, реализации `HostDrag` (`OleTransfer`,
  `WebTransfer`) и `CanvasDrag` (`HtmlCanvasDrag`), позиции clipboard и drag, перешедшие в
  циклы, `WindowsLoop::with_drag` (только тесты); разбиения — `ModalFrames` из
  `WindowsLoop::deliver_modal_frames`, `owner/modal_pace.rs` из `owner/windows.rs`, `OsDrags` из
  `DragDouble`, `drag/offers.rs` из модуля правил Web, `FakeAccessibility` из
  `headless/platform.rs`; три файла платформ drag — слияние корня, FFI-модуля и модуля правил.
  Решения, которых строка не называла:
  (1) `owner/` не называет ни одной из четырёх фич, а объект фичи цикл держит типизированным
  полем: рядом с циклом объявлен трейт-хук того, что цикл и его окна просят у объекта, и
  хост-трейт, функция которого его делает; таблица называет позицией сам цикл (`type Drag =
  WindowsLoop`), цикл отвечает из поля — таблица не может назвать один тип и построить другой, и
  не может объект пропустить; так же LY31 называет цикл позицией дисплеев: Windows называет цикл
  для дисплеев и drag, Web и winit — ещё и для буфера обмена (у headless дисплеи — цикл,
  буфер и drag — типы фич над состоянием, которое цикл держит сам). Win32 — `WindowsHost::drag() -> Arc<dyn HostDrag>`, `HostDrag:
  OwnerMessages + DataTransferSource` с `drag_source(WindowRef)` и `register_target(WindowRef)`
  для окна; хуки окна владельца — этот транспорт. Web — `WebHost::{clipboard, drag}`,
  `HostDrag::register(canvas, callbacks, registration) -> Box<dyn CanvasDrag>` (`source`,
  `arm`, `retire`). winit — `WinitHost::{clipboard, drag}` с `FileDrops` (шесть методов
  транспорта, `DataTransferSource` — супертрейт, позиция отдаёт upcast). Android —
  `AndroidHost::{clipboard, transfer}` (в трейте рядом с `input_method` LY30, `displays` LY31 и `preferences`, `notices`, `activity` LY33), `transfer(view)`
  делает транспорт `ViewTransfer` (`drag`, `drop_clip`, `set_own`, `take_local_drop`) для
  `FluiView`. Циклы берут хост: `WebLoop::new::<Web>()`, `WinitLoop::new::<Winit>()`,
  `AndroidLoop::new::<Android>(app)` (форма LY31). headless держит состояние drag
  симулированной системы, как остальную систему: `owner::headless::OsDrags` (открытые
  предложения, сессия) и `os_drag_start`; `DragDouble`, источник окна и `HeadlessDragAndDrop`
  играют поверх него, цикл строится в один шаг. У AppKit, UIKit и Win32 буфер без состояния —
  позиция делает ручку на вызов и остаётся типом фичи. Окна Win32 и Web больше не называют
  `drag`: источник и цель они берут у `HostDrag` цикла, поэтому `Win32DragSource`,
  `register_target`, `WebDragSource`, запись холста и `drag::web::register` закрыты в своих
  файлах. (2) `OwnerMessages::{attach, destroyed}` обязательны. `WM_NCDESTROY` освобождает
  контекст и говорит каждому хуку `destroyed`, каждый шаг в своей границе паники: паника первого
  хука больше не лишает второго (тест ниже падает с одной границей вокруг цикла). (3) Кадры
  модального цикла: `owner::windows::modal_loop::ModalFrames` (обработчики, карта окон, счёт
  кадров, последняя доставка) с `deliver` и `start`; одну держит окно владельца для цикла OLE
  (`OwnerWindow` — `Weak<ModalFrames>`), свою — каждое окно для циклов `DefWindowProc`, как
  раньше каждое держало свою доставку. Темп — `owner/modal_pace.rs` под `cfg(any(windows, test))`,
  рядом с `redraw_poll`, чистый, `forbid(unsafe_code)`. (4) Файл платформы drag собирается под
  `cfg(any(<os>, test))`, его сторона ОС — под `cfg(<os>)`, корень ожидает `unsafe_code` только
  на своей ОС; решения без ОС — встроенный модуль файла с `forbid(unsafe_code)`, названный по
  тому, что решает: `crossing` (что пересекает OLE: форматы, байты, `DROPEFFECT`, конец
  модального цикла), `clip` (что клип предлагает, держит и отвечает), `mapping` (типы и эффекты
  `DataTransfer`); их элементы — `pub(super)`. Константы, которые читает только сторона ОС, живут
  в ней (имя CF_HTML, действия `DragEvent`, тип `Files`), неиспользуемый `ACTION_DROP` удалён.
  (5) Drag-сообщений у `WindowProc` нет; отзыв drop target в `WM_DESTROY` (между освобождением
  тачпада и колбэками закрытия) остался в окне. Регистрация при создании — хук класса окна
  LY30, `DropTargetRegistration`, переехавший в `drag/windows.rs`: порядок хуков тот же (`Tsf`,
  `WindowProc`, `DropTargetRegistration`, `Touchpad`; создание — TSF, drop target, viewport),
  хук находит окно по `CreatedWindow::identity` (`WindowRef`), берёт транспорт из контекста окна
  — теперь `Arc<dyn HostDrag>` цикла, а не `OleTransfer` — и регистрирует через
  `HostDrag::register_target`; окно, которое хук убил, следующие хуки не слышат
  (`window_created`, `WindowGone`). (6) Рёбра фич в `platforms/` названы механизмом LY31,
  пришедшим с LY30 (`retiring = ["platforms"]`; десять исключений LY30, LY31 и LY33 рядом с
  пятью этими), по исключению на ребро
  (`exit = "ADR-0151"`, задача — словами в `reason`: идентификаторы задач
  в TOML запрещает `markers`). Кто снимает: LY28 — `drag::windows` →
  `platforms::windows::platform` (`WindowRef`, контекст окна: `drop_target`, `pointer_registry`,
  колбэки), `haptics::windows` → `platforms::windows::platform` (`WindowRef`), `drag::web` →
  `platforms::web::registration` (`Registration`); LY29 — `drag::windows` →
  `platforms::windows::events` (`MOUSE_BUTTONS`), `drag::web` → `platforms::web::events`
  (`answer`, `pointer_position`); шестое, `haptics::android` → `platforms::android::preferences`
  (`with_activity`), снято слиянием с LY33: помощник — `owner::android::activity`; LY34 снимает ключ `retiring`.
  Хуки drag владельца называют `WindowRef` и `Registration` (`owner` → `platforms`, как
  `WindowMap`), это записано в строке LY28. Обратные рёбра — из `platforms/` в фичи — `retiring`
  как источник не проверяет; их снимает подключение оконных позиций через таблицу (LY38, список в
  его строке): окно Win32 → `accessibility::windows`, `haptics::windows`, контекст окна Win32 →
  `drag::windows` (слот и отзыв drop target); macOS → `accessibility::apple`, `haptics::apple`;
  iOS → `haptics::apple`; Android → `haptics::android`, `drag::android`; Web → `haptics::web`;
  winit → `accessibility::linux`; headless → `accessibility::headless`, `haptics::headless`,
  `drag::headless`. Расширения видимости `pub(super)` → `pub(crate)` ради этих рёбер: в окне и
  вводе — `WindowRef` и его `new`, `hwnd`, `with_context`, `WindowContext::drop_target`, `mod
  events` и `events::{SURFACE, MOUSE_BUTTONS}` Win32; `mod events` и `events::{answer,
  pointer_position}`, `mod registration`, `Registration` и `Registration::listen` Web; в фичах —
  `revoke_target` и `WindowDropTarget` (был `pub(in crate::platforms::windows)`),
  `WindowsHaptics::new`, `AndroidHaptics::new`, `IOSHaptics::new`, `AndroidDragSource::{new,
  retire}`. Остальные COM-объекты, их `new` и `leave` — `pub(in crate::drag::windows)` и
  `pub(super)`, как было относительно старого модуля. (7) Публичные пути
  `platforms::<os>::{WindowsClipboard, …}`, скрытые `shared::{ole_drag_drop, android_drag,
  html_drag_drop}` удалены, `shared::haptic_degradation` → `haptics::degradation` (скрытый: его
  читает `backend_conformance`), фрагмент `changelog.d/platform-services.md`; имена корня
  (`FakeAccessibility`, `FakeHaptics`, `HeadlessDragAndDrop`) те же, `flui-app` не тронут.
  `globals`: `clipboard::lock::{CLIPBOARD, reentrancy::SESSION_OPEN}`,
  `clipboard::windows::owner_window::OWNER` (итого 55); `module-dag`: `features = ["clipboard",
  "drag", "accessibility", "haptics"]`, `retiring = ["platforms"]`, 5 исключений (решение 6);
  allowlist `testing` — путь `drag/windows/tests.rs`, `docs-paths` — ADR-0247 называет
  `tests/android_drag.rs` (запись не переписывается); `.config/nextest.toml` называет
  `src/clipboard/lock.rs`. Тесты: `ole_drag_drop_contracts` (`drag/windows/crossing/tests.rs`),
  `android_drag_mapping` (`drag/android/clip/tests.rs`), `html_drag_drop_table`
  (`drag/web/mapping/tests.rs`), `haptic_degradation_table` и
  `stale_haptic_source_plays_on_the_window_actuator` — модули `tests.rs` рядом, правки — пути,
  импорты, `crate::table_test::run_table`; `ole_drag_and_drop_on_real_windows`
  (`drag/windows/tests.rs`) — смысл тот же, случаи, которые ведут транспорт сами, строят
  платформу над своей ручкой (`platform_over` через `WindowsLoop::with_drag`), окно владельца —
  через `OwnerPost::hwnd`; `a_replaced_wake_hook_gets_its_modal_frame` — те же три доставки через
  `ModalFrames::deliver`; `the_modal_loop_waits_for_the_next_deadline_within_a_frame` — в
  `owner::modal_pace`; `headless_drag` и тест AT-SPI — без правок. Новые:
  `every_owner_hook_hears_the_window_go_after_one_panics` (без раздельных границ второй хук не
  слышит — проверено откатом), `a_media_type_offers_its_transfer_format` и
  `bytes_carry_the_payload_of_their_format` (рядом с `drag::offers`). Проверки на слиянии с
  `main` @ `a0dd4ba01` (LY33). Windows: `cargo fmt --check`; clippy `-p flui-native -p flui-app
  --all-targets -D warnings` и `-p flui-native --all-targets --features winit-backend,a11y`;
  nextest `-p flui-native -p flui-app` — 286 (2 пропущены), `-p flui-native --features
  winit-backend` — 270 (1), `-p xtask` — 45 (до слияний); clippy Android (`-p flui-native -p
  flui-app --all-targets`), macOS `--lib` (и с `winit-backend,a11y`) и iOS `--lib`
  (`DOCS_RS=1`): с `--all-targets` тесты для Apple с этого хоста не собираются
  (dev-зависимость `flui-testing` → `flui-rendering` → `stacker` → `psm` требует C-компилятор
  цели); `wasm-check`, `wasm-test` (50 утверждений); `module-dag` (300 модулей, 8 фич с 59
  файлами платформ, 1 base, 412 рёбер, 15 исключений) и самотест, `globals` (55),
  `file-length`; на слиянии с LY31 ещё `markers`, `testing`, `docs-paths`, `workspace`, `reach`,
  до слияний — `checks`, `doc-strict` и rustdoc `--document-private-items` без фич и с
  `winit-backend,a11y` под `-D warnings`. WSL Ubuntu 24.04: nextest `-p flui-native`
  (`FLUI_HEADLESS=1`) — 183; командой CI без `WAYLAND_DISPLAY` и `XDG_SESSION_TYPE`
  (`FLUI_HEADLESS=1 xvfb-run -a -s '-screen 0 1280x1024x24 -noreset' cargo nextest run -p
  flui-native --locked --all-features --no-fail-fast`) — 263 из 263. Galaxy S21 (Android 15),
  2026-10-22, на слиянии с LY33 (проводка Android при слиянии изменилась: один `AndroidHost`,
  конструктор лупера): проба `flui create --template counter --local` из этого дерева, в её
  `lib.rs` (вне репозитория) — касание с `HapticsHandle::perform(LightImpact, Immediate)`,
  `Draggable` с `DragTrigger::LongPress` и `DropTarget`, `RawTextField`. Касание «Increment» —
  счётчик 1, исход хаптики `Submitted`; `input draganddrop` с долгим нажатием по карточке на цель
  — «dropped 1»; в поле «clip»: Ctrl+A, Ctrl+C, End, Ctrl+V — «clipclip» (заглушка буфера);
  `logcat -c` не вызывался, настройки устройства не менялись. Та же проба дала то же на слиянии с
  LY31 и до rebase на LY30; предупреждение `flui_interaction::haptics: a haptic outcome was not
  delivered before its presentation closed` (тикеты `FramePresented` подъёма и броска drag),
  замеченное до rebase, есть и на базе `e86d0d5b4` с той же пробой. Не запускалось:
  `cross-typecheck` (нет `zig`; job CI), набор `web_browser` (нет `chromedriver`; браузерная проба
  — у координатора), Mac и iPhone — в очереди vanyastaff/flui#1840.
- [x] **LY31** — ветка `platform/display`. Фича дисплеев — каталог `display/`; публичные пути не
  изменились (типы headless реэкспортирует `platforms::headless`), changelog не нужен, `flui-app`
  не тронут. Перенос — `git mv`; тела менялись только там, где это названо ниже (решения 1–6).
  Модель: `shared/display_source.rs` → `display/source.rs`. Один файл на платформу (R4.1, R4.2,
  ADR-0151 §10): половины — встроенные модули, чистая — `cfg(any(<os>, test))` с
  `forbid(unsafe_code)`, та, что зовёт ОС, — `cfg(<os>)`; дочерний файл — только механизм одного
  объекта ОС, разбиение по пределу 3 000 строк или тесты модуля. Windows: `display/windows.rs`
  (1 018) — `sample` (было `shared/windows_display.rs`), `source` (`windows/display.rs`),
  `broadcasts` (новый); тесты сообщений — дочерний `display/windows/message_tests.rs` (было
  `display_message_tests.rs`). Apple: `display/apple/mod.rs` — чистые `screen` (ключи и границы
  экрана: `shared/apple_display_identity.rs` и `device_bounds_of_screen` из
  `shared/appkit_window_geometry.rs`), `period` (`shared/display_period.rs`) и `link_rate`
  (`shared/display_link_rate.rs`); `{macos,ios}/display.rs` → `display/apple/{macos,ios}.rs`.
  Android: `display/android.rs` — `sample` (`shared/android_display.rs`) и `source`
  (`android/display.rs`). Web: `display/web.rs` (2 277) — чистые `capability` (`shared/web_feature.rs`),
  `dppx_query`, `refresh_estimate`, `screen_identity` (из `shared/`), `source` (`web/display.rs`
  и `web/features.rs`: `BrowserFeatures` и вызовы по доказательствам) и `events`
  (`web/display_events.rs`). winit: `display_linux.rs` → `display/winit.rs`. Linux:
  `linux/display/` → `display/linux/` остаётся каталогом: одним файлом он перешёл бы предел и без
  тестов (3 001 строка); дети — соединения протоколов (`wayland`, `x11`, `session`), чистые модели
  (`wayland_model`, `x11_model`, `view`, `protocol`), источник (`source`) и тесты; модуль `#[path]`
  для тестов не на Linux исчез (каталог собирается под `cfg(any(linux, test))`). headless:
  `headless/display.rs` → `display/headless.rs`.
  `display` вошёл в `features` `module-dag`; правила держатся как есть. `module-dag` получил ключ
  `retiring` (`platforms` в `flui-native`): модуль под ним — свой узел, фича называет его только
  через поимённое исключение (правило «names a retiring module», самотест — 54 находки: посаженное
  нарушение, допущенное исключением ребро и `retiring` с неизвестным именем); без этого исключение
  для ребра в `platforms/` было бы «stale», ведь правила уровня крейта его допускают. `exceptions` —
  три записи, выход у всех ADR-0151, что их снимает — в `reason` словами (номер задачи в TOML —
  маркер процесса): `display::winit` → `display::linux` (§11, уходит с winit, LY87);
  `display::android` → `platforms::android::preferences` (`with_activity`; уходит с LY33, которая
  переносит помощник в `owner/android/activity.rs`); `display::apple::ios` →
  `platforms::ios::os_version` (`read`, расширенный до `pub(crate)` ради дисплеев; уходит с LY34,
  `version/gate.rs`). Других рёбер из `display/` в `platforms/` нет (`--print-edges`); в `shared/` —
  `system_notice::StaleOnWake` (будущая модель lifecycle) и `os_version` (будущий `version/`), оба
  разрешены.
  Решения. (1) `owner/` не называет дисплеи: каждый модуль владельца объявляет `HostDisplays` — что
  цикл и открытые им окна спрашивают у дисплеев, — а таблица делает объект через трейт хоста
  владельца: `WindowsHost::displays` (добавлен), `MacosHost`, `IosHost`,
  `AndroidHost::displays(api, library, owner)`, `WebHost`, `WinitHost` (новые). Позиция таблицы —
  сам цикл, как `type Input = WindowsLoop`: `type Displays` = `WindowsLoop`, `MacOSLoop`, `IOSLoop`,
  `AndroidLoop`, `WebLoop`, `WinitLoop`, `HeadlessLoop`, и `impl host::Displays<L> for L` лежит рядом
  с циклом и читает через хук, так что позиция и хук не расходятся. Тела — в реализациях хуков;
  собственные методы источников — только `new` и чтение ОС. Каждый наблюдатель, которого источник
  запускает для цикла, возвращается как `owner::watch::Watch` (`#[must_use]`), и цикл держит его до
  остановки, так что раскрутка не пропускает остановку: экранные уведомления AppKit и UIKit,
  слушатели браузера, обратный вызов частоты Android (`HostDisplays::observe` вместо
  `watch_refresh_rate` и `stop_watching`; наблюдение снимается на потоке looper в конце цикла) и поток
  соединения Linux под winit (`observe` вместо `stop()`; каскад `cfg` при остановке
  `owner/winit.rs` ушёл). macOS держит наблюдение в `NativeOwner<Watch>` (`owner/apple/native_owner.rs`,
  был `owner/apple/ios/native_owner.rs`, теперь общий для обоих циклов): значение отпускается на
  главном потоке, где бы цикл ни упал, и `SAFETY` у `Send`/`Sync` `MacOSLoop` это называет.
  (2) headless — переделка вынужденная: на `main` цикл держал `HeadlessDisplays` из
  `platforms::headless`, а `owner/` не может назвать фичу (base ниже features), и сам тип был только
  обёрткой снимка, чей `host::Displays` читал `owner.displays`. Цикл держит `DisplaySnapshot`,
  раскладка спецификаций — функция `display::headless::laid_out`, `HeadlessHost::displays`
  раскладывает дисплей по умолчанию; `with_displays`, `display_controls` и
  `HeadlessDisplayControls` переехали в `display/headless.rs`, а вопросы двойников окна к снимку —
  расширяющий трейт `LaidOutDisplays`. (3) winit на Windows и macOS: строка кладёт
  `display_{windows,macos}.rs` в `display/winit.rs`, но они называли `display::windows` и
  `display::apple::macos` — ещё два ребра между файлами платформ, которых строка не считала. Вместо
  этого источники Win32 и AppKit реализуют `owner::winit::HostDisplays` рядом с хуком своего цикла
  (`cfg(feature = "winit-backend")`; туда ушли дисплей окна по сырому handle, пересэмплирование,
  подписчик и наблюдатель экранов), а `display/winit.rs` — источник Linux над `display::linux`,
  единственное поимённое ребро. `owner::winit::HostDisplays` — по сути три хука под `cfg`, и
  `resample` на Windows и macOS не читает аргументов: эти источники читают ОС мимо winit и только
  помечают себя устаревшими, а цикл событий нужен лишь источнику Linux, который следует за
  соединением winit. Три трейта с выбором по `cfg` были бы тем же с большим числом имён, а всё это
  уходит с winit (LY87), поэтому трейт остался одним, а его документация говорит, чей какой метод.
  (4) `LoopTurn`, `poll_timeout` и их тест лежали в `shared/android_display.rs`, но их зовёт цикл
  Android: в `display::android` они дали бы ребро `owner` → фича, поэтому они — правило цикла,
  `owner/android/poll.rs`; ADR-0249 называет новый путь. (5) Подписчик дисплеев, отложенный LY27:
  `DisplayBroadcasts` слышит `WM_DISPLAYCHANGE` и `WM_SETTINGCHANGE` с `SPI_SETWORKAREA`, помечает
  мониторы устаревшими и будит владельца. Windows не читает результата этих широковещательных
  сообщений, поэтому подписчики дисплеев и предпочтений только наблюдают (отвечают `None`), и
  порядок в таблице смысла не несёт; отвечает только lifecycle (`HostEvents`: запрос завершения
  сеанса и `WM_POWERBROADCAST`). Флаг приёма — свой: закрывается в `detach`, который приёмник зовёт
  перед `DestroyWindow`; пометка устаревания ставится всегда. Приёмник winit на Windows (поле
  `broadcasts`) строится и тогда, когда предпочтения не открылись, — с теми подписчиками, что есть.
  Правка файла предпочтений минимальна: `Invalidation` теряет `displays` и `notify_displays`,
  `PreferenceBroadcasts` — ветки дисплеев и ответ, `PreferenceSource::new(&signal)`; их тесты теряют
  аргумент дисплеев и вызов `notify_displays`. (6) iOS: `ScreenIdentity` — словарь хука в
  `owner::apple::ios`, номер экрана — `ScreenNumber`. Запрос частоты кадров display link
  (`request_frame_rate`, `set_frame_rate_range`) переехал из окна в источник: окно получает дисплеи
  как `Arc<dyn HostDisplays>` и не может назвать `IosDisplays`, а правило диапазона (версия iOS,
  `frame_rate_preference`) — у источника. (7) `SystemNotices::mark_held_stale_on_wake(Weak<dyn
  StaleOnWake>)`: цикл держит дисплеи трейт-объектом, а `mark_stale_on_wake` принимает
  `&Arc<T>` с известным `T`; обобщённая форма теперь зовёт новую. (8) `device_bounds_of_screen` и его
  тест — из `shared/appkit_window_geometry.rs` в `display::apple::screen` (кроме дисплеев его никто
  не звал); `nearest_monitor` Windows — метод хука, `Option<NonZeroIsize>` (`None` — монитора нет),
  процедура окна зовёт его через контекст, а `last_monitor` окна стал `Cell<Option<NonZeroIsize>>`.
  (9) `iOS os_version::read` стал `pub(crate)`. Записи `globals` не менялись (статиков в дисплеях
  нет, 55); путь в `tools/xtask/allowlists/testing.toml` (тесты источника Linux) перекодирован.
  Остаются рёбра в чужие файлы платформ, которые `module-dag` увидит, когда фича войдёт в проверку
  (сейчас `platforms/` — уровень крейта): окно → дисплеи — `platforms/macos/window.rs` →
  `display::apple::macos::bounds_of` (NSRect в `Bounds`, 3 места), `platforms/ios/window.rs` →
  `display::apple::{screen::change_screen, link_rate::LinkPeriod}`, `platforms/web/events.rs` →
  `display::web::PixelRatioWatch`, `platforms/headless/platform.rs` → `display::headless::
  {DisplayChangeError, LaidOutDisplays}` — их снимает LY28; ребро дисплеи → предпочтения
  (`with_activity`, JNI-помощник, которым пользуются и `owner::android::java_view`, haptics, memory)
  названо исключением выше.
  Тесты: перенесённые без правок смысла (`windows_display`, `monitor_key_table`,
  `display_message_tests` — `display_change_message_invalidates_the_snapshot_and_wakes_the_owner`,
  `work_area_change_invalidates_the_snapshot`, `apple_display_identity` и строка экранных границ,
  `display_period`, `display_link_rate`, `android_display_table`,
  `a_paused_loop_blocks_until_it_is_woken`, правила Web, модели Linux и `source/tests.rs`,
  `x11/tests.rs`, тесты `DisplaySource`); `tests/displays.rs`, `wasm-test` и `web_browser` не
  тронуты. Новые: `display_broadcasts_mark_the_monitors_stale_and_wake_until_detached` (и что
  подписчик не отвечает), `the_displays_and_the_preferences_hear_every_change_in_either_order`
  (приёмник в обоих порядках), `a_display_change_reaches_the_displays_when_the_preferences_cannot_open`
  (winit на Windows). Мутации: предпочтения отвечают на `WM_SETTINGCHANGE` — падает тест порядка
  («preferences first: the displays did not hear the work-area change»); дисплеи отвечают на
  рабочую область — падают он («displays first: the preferences did not hear the work-area
  change») и тест подписчика (left `Some(LRESULT(0))`, right `None`); приёмник winit только при
  открытых предпочтениях — падает тест winit («the displays' receiver stands without the
  preferences»); `detach`, не закрывающий приём, — «a detached subscriber woke the owner».
  Проверки на Windows: `cargo fmt --check`; clippy `-p flui-native --all-targets -D warnings` с
  `winit-backend` и без; nextest `-p flui-native` — 235 (1 пропущен), с `winit-backend` — 263
  (1 пропущен); `wasm-check`, `wasm-test`; clippy Android (`--all-targets`), macOS lib (и с
  `winit-backend,a11y`), iOS lib (`DOCS_RS=1`); `module-dag` и самотест, `globals` и самотест,
  `file-length`, `markers`, `testing`, `docs-paths`, `checks`, `doc-strict`, `workspace`, `reach`.
  WSL Ubuntu 24.04: clippy `-p flui-native -p flui-app --all-targets` с `winit-backend,a11y` и без;
  nextest `-p flui-native` (`FLUI_HEADLESS=1`) — 170. Под xvfb
  `a_randr_reconfiguration_on_a_real_server_is_published` падал в WSL и на ветке, и на `main` @
  `0f52e4e32` («the wake read the server again», left 1,
  right 0) — артефакт WSLg: его `WAYLAND_DISPLAY` направлял источник дисплеев к компоновщику
  WSLg, а не к Xvfb. С `WAYLAND_DISPLAY` и `XDG_SESSION_TYPE` убранными, как на раннере CI, и
  командой CI (`FLUI_HEADLESS=1 xvfb-run -a -s '-screen 0 1280x1024x24 -noreset' cargo nextest
  run -p flui-native --locked --all-features --no-fail-fast`) набор проходит без падений.
  Слияние `origin/main` @ `f4af625af` (фон холста по pixel ratio и фича `text`), merge-коммитом:
  `features` = `display` и `text`, один ключ `retiring`, исключения обеих фич (11); один
  `AndroidHost` с `input_method` и `displays`; таблицы Apple и winit берут текст из `text`, дисплеи —
  из `display`; поведение фона холста не тронуто, только тип дисплеев окна
  (`Arc<dyn HostDisplays>`). После слияния: `cargo fmt --check`; clippy `-p flui-native -p
  flui-app --all-targets -D warnings` с `winit-backend` и без, Android, macOS (и с
  `winit-backend,a11y`), iOS; nextest `-p flui-native` — 237 (1 пропущен), с `winit-backend` — 265;
  `wasm-check`; `wasm-test` проходит, среди его тестов
  `web_window_backing_store_follows_a_pixel_ratio_change_without_a_resize`; `module-dag` (267
  модулей, 2 фичи с 19 файлами платформ, 11 исключений) и самотест, `globals` и самотест,
  `file-length`, `markers`, `testing`, `docs-paths`, `workspace`, `reach`; WSL — clippy с
  `winit-backend,a11y` и без, nextest headless — 171; полный набор командой CI без
  `WAYLAND_DISPLAY` — без падений, 20 прогонов подряд (и столько же на `main` @ `f4af625af`).
  Устройства. Galaxy S21 (Android 15, проба `flui create --template counter --local`, собрана со
  слитой ветки; лог последующих ревизий и обратного вызова частоты временно поднят до `info`, правка
  не закоммичена): обратный вызов частоты зарегистрирован через `Watch` и сообщает 8333333 →
  16666666 → 8333333 нс; ревизии снимка 1–3 — 1080×2400 с периодом 8333, 16666 и 8333 мкс, поворот
  в ландшафт — ревизия 4, 2400×1080, id 1 тот же, обратно — ревизия 5, 1080×2400; Back — «Destroy —
  shutting down», «Android platform event loop finished», предупреждения «refresh-rate callback
  outlived its thread» нет (наблюдение снято на потоке looper); `FATAL`, ANR и паник нет;
  `accelerometer_rotation` и `user_rotation` восстановлены (1 и 0). Web (панель браузера, проба с
  `platform/display` @ `1dd51754c`): окно 896×545 при dpr 1 → 600×500 при dpr 2 — страница
  переложена, холст 600×500, ошибок в консоли нет, как на `main` @ `0f52e4e32`; живая смена dpr не
  масштабировала буфер холста и на том `main` — это чинит слитый #1859, его тест проходит выше;
  перезагрузка при dpr 2 давала 1200×1000. Не запускалось: `web_browser` — нужен ChromeDriver под
  Chrome 155, его здесь нет; `cross-typecheck` (нет `zig`; job в CI); смена масштаба или
  разрешения на Windows — системная настройка, её не меняли, опора — тесты; Mac и iPhone — в
  очереди vanyastaff/flui#1840.
- [x] **LY28** — ветка `platform/window`, поверх LY30, LY31, LY32 и LY33. Окно — фича `window/`: тела
  перенесены `git mv`, где файл переезжает целиком; правки — пути, `use`, видимость, разрезы и
  хуки, названные ниже. Ветку пришлось сложить в один коммит кода и один коммит спецификации:
  LY30, LY31, LY32 и LY33 влились посреди ревью, и перенос по коммитам давал по 31 конфликту на каждом шаге.
  Модель: `shared/window_state` → `window/state.rs`, `shared/visibility` → `window/visibility.rs`
  (у правил не было тестов — добавлены четыре: `a_win32_show_message_reports_unless_the_window_is_minimized`,
  `a_win32_size_message_reports_only_the_minimize_edges`, `a_win32_window_starts_as_visible_as_its_style`,
  `appkit_occlusion_reports_the_visible_bit_once`; `SIZE_MAXSHOW`/`SIZE_MAXHIDE` остались
  константами теста, который проверяет, что эти значения ничего не сообщают), `config.rs` →
  `window/config.rs` (`flui_native::config` — `pub use window::config`),
  `shared/window_installation` → `window/installation.rs`; имена установки публичны от корня крейта
  (`flui_native::{WindowInstallation, PendingWindowInstallation, InstallationCancelled}`),
  `flui-app` называет их там, псевдонима в `shared` нет — `changelog.d/platform-window.md`;
  `tests/window_installation.rs` (`window_installation_contract`, на `HeadlessPlatform`) проверяет
  установку через эти имена.
  Файлы платформ (R4.2: корень под `cfg(any(<os>, test))`, чистая половина с `forbid(unsafe_code)`
  собирается и тестируется на любом хосте, FFI — под `cfg(<os>)`; `#[path]` нет): Windows —
  `window/windows/{mod (37), limits (339), window (2 183), set (714), proc (2 651), backdrop (783)}.rs`:
  `limits` — чистые правила размеров (`window_limits_in_device_pixels`), `window` и `set` — тело окна
  и набор окон с расстановкой (вместе они перешли бы предел), `proc` — хук `WindowProc` с контекстом
  окна, `backdrop` — `WindowsWindowExt` и материалы (строка называет `window/windows/backdrop.rs`).
  Apple — `window/apple/{mod, geometry (843),
  liquid_glass, ios (1 723), macos (2 253)}.rs` и `macos/{ext (1 058), delegate (651)}.rs`: команды,
  `display_pass` и `tiling` вернулись в `macos.rs` и `ext.rs`; `delegate` — класс
  `NSWindowDelegate`, механизм одного объекта ОС; `ext` (публичный `MacOSWindowExt` с
  `tiling`) — единственный разрез по пределу (`macos.rs` с ним — 3 311 строк); `liquid_glass` —
  `window/apple/liquid_glass.rs`, где строка оставляет его до LY36. Android — `window/android.rs`
  (1 215, `safe_area`, `gesture_exclusion`, `system_chrome` внутри), Web — `window/web.rs` (1 533,
  регистрация canvas внутри), winit — `window/winit.rs` (2 068, `mod plan`), Linux —
  `window/linux/x11.rs` (чистый `mode` под `cfg(any(all(linux, winit-backend), test))`: тесты
  `x11_window_state_parse`, `net_wm_state_reads_only_known_atoms`, `wm_state_property_words` идут на
  любом хосте) и `x11/probe.rs` (FFI, тесты в `probe/tests.rs`), headless — `window/headless.rs`.
  `shared/hwnd_affinity` — машинерия владельца: `owner/hwnd_affinity.rs` под
  `cfg(any(windows, test))` (`refusal_precedence_is_gone_then_foreign_thread_then_class_then_slot`).
  Шов: позиция окна называет общее окон платформы, `host::Window::Shared` (`WindowSet`,
  `MacOSWindows`, `IOSWindows`, `WinitWindows`, `HeadlessWindows`, `()` на Android и Web);
  `HostPlatform::from_parts(shared, |shared| Loop::new::<H>(..))` (и `try_from_parts`) строит его
  один раз и отдаёт циклу тот же `Arc` как хук — два экземпляра невозможны. Хуки объявляет
  `owner/`: `CallbackWindow`, `OpenWindows` (`handlers`), `ClassWindows`, `WindowRegistry`,
  `SceneWindow`, `SceneWindows`, `SceneConsumer`, `SceneInstallation`, `ActivityWindow`,
  `LoopWindow`, `LoopWindows` (цикл выделяет номер, регистрирует и доводит окно; `build` только
  строит его из `WindowSeed`), `TrackedWindows`; `callbacks()` и `is_empty` — по одному разу в
  `CallbackWindow`/`OpenWindows`. R4.3 и в тестах: тесты `owner/` держат свои заглушки (`NoWindows`,
  `EmptyHost`, `NoNotices` в `owner::headless`); таблица X11 окна — `window/linux/x11/tests.rs`
  (R4.11: в `hosts/` только таблица и конструктор) со своей таблицей `X11Windows` над циклом
  winit с `owner::winit::testing::QuietHost`, чьи хуки ничего не сообщают (`NoDisplays`,
  `NoNotices`, `NoPreferences`, `NoHostNotices`, `NoFileDrops`, `InMemoryClipboard`); прогон окна
  под живым циклом — `run_window_scenario`, и `install_test_owner_signal` с `wait_for_running`
  переехали туда же из `real_loop_tests`. Тесты `owner/`, которые от `main` называют хосты и фичи,
  переносит LY34 (строка это называет). Win32: каждый хук
  перечисляет свои сообщения (`WindowMessages::answers(msg, wparam)`), диспетчер проверяет это
  `debug_assert`; `WM_TIMER` процедуры окна отвечает только своему таймеру модального цикла;
  hit-test и таймер тачпада — `Touchpad::message`, горячая клавиша полноэкранного режима —
  `WindowProc` (`InputMessages` просит окно о его клавишах), `InputMessages` — только ввод.
  `hosts::windows::tests::the_window_hooks_answer_disjoint_messages` перебирает все сообщения,
  таймеры и системные команды и падает, если два хука таблицы
  (`[Tsf, WindowProc, InputMessages, DropTargetRegistration, Touchpad]`) отвечают на одно; без
  проверки номера таймера в `WindowProc` он падает на таймере тачпада (проверено: с
  `WM_TIMER => true` тест падает). LY30, LY31 и LY33: работа метода
  ввода Android идёт против `&dyn ActivityWindow` и находит поле через `as_any` у
  `window::android::AndroidWindow`; поле IME ушло из `ActivityWindow`; маршрут сеттеров текста
  macOS — прямо `owner::apple::macos::exec_on_owner` (ребро в окно не нужно); окна читают дисплеи
  через хуки `HostDisplays` своих циклов, а уведомления — через `owner::notices::NoticeChannel`
  (`WindowSeed::notices`); зонд X11 из `platforms/winit/linux_unreported.rs` — в окне, там осталась
  проверка дисплеев; окна, которые менеджер окон изменил без события winit, отдаёт
  `LoopWindows::take_unreported() -> Vec<u32>`, а `WinitLoop::refresh_unreported` перечитывает свои
  окна сам, без замка состояния. iOS: `SceneConsumer::connected(..) -> Result<Box<dyn
  SceneInstallation>, BootstrapError>` — принятое подключение всегда несёт то, что цикл ждёт
  от установки, и цикл хранит его до опроса; «`Ok` без установки» не выражается. Цикл UIKit
  собирается только под iOS, поэтому правило несёт тип, а не тест. LY32: `WindowRef` — словарь
  владельца (`owner::windows::WindowRef`; `with_context` — метод окна в `window/windows/proc.rs`),
  его называют хуки drag и haptics Win32; `owner::web::HostDrag::register` получает регистрацию
  холста как `&mut dyn owner::web::CanvasListeners` с одним методом
  `listen(target, kind, Rc<dyn Fn(Event)>)`: слушатель держит единственную сильную ссылку на
  тело (`debug_assert`), одно выделение на слушатель; его реализует `window::web::Registration`,
  и слушатели ввода из `platforms/web/events.rs` идут через него же; цикл Android не называет
  источник drag окна — `ActivityWindow::drag_ended(result)`; `ModalFrames` держит окна как
  `Weak<dyn ClassWindows>`; `platforms/linux/` ушёл целиком, `platforms/web` без своего `unsafe`
  больше не ждёт `unsafe_code`. Сняты три исключения LY32: `drag::windows` →
  `platforms::windows::platform`, `haptics::windows` → `platforms::windows::platform`,
  `drag::web` → `platforms::web::registration`. Рёбра — датированные исключения `module-dag`
  (`exit = "ADR-0151"`, в `reason` — что их снимает): 6 из `window::*` в `platforms::*`, 12 из
  `window::*` в `accessibility::*`, `haptics::*` и `drag::*` (окно делает объект фичи на окно; их
  снимает LY38, когда оконные позиции доходят до окна через таблицу хоста, — так говорят и
  `reason` всех двенадцати), 7 из `window::*` в `text::*` и 3 обратно, 3 из `window::*` в `display::*` (iOS —
  `LinkPeriod` и `change_screen`, macOS — `bounds_of`, headless — `LaidOutDisplays`), 3 в
  `preferences::*` и `lifecycle::*` (геометрия мыши Win32, правило `messages` для
  `WM_WTSSESSION_CHANGE`, ночной режим, плотность и геометрия касаний Android); строки LY29, LY30,
  LY33 и LY38 называют, чем каждое уходит. Не сделано здесь: сигнальная половина правила Activity (`ActivitySignal`, оконные ветки
  `ActivityTarget`, `publish_activity_fact`), которую строка LY28 после LY33 относит к окну, осталась
  у lifecycle и лупера — её переезд меняет путь событий Activity и требует своего прогона на
  устройстве. Отступления: (1) `x11_window_without_a_probe_does_not_claim_to_observe_minimizing`
  — правка только в том, где лежит ячейка, помнящая неудавшийся probe
  (`window_shared().window_manager` вместо поля цикла); утверждения те же; (2) публичный
  `MacOSWindow::new` стал `pub(crate)` (брал внутреннюю карту окон). `globals`: 4 записи окна
  переехали, итого 55. ADR-0084, 0090, 0254, 0255 называют новые пути и строки; запись замера в
  ADR-0082 и ссылка на модуль Linux-заглушки — в `docs-paths` (6); `docs/architecture.md` называет
  Linux-файлы фич вместо `platforms/linux/`. Проверки (Windows): `cargo fmt`; clippy `-D warnings`
  `-p flui-native -p flui-app --all-targets` с `winit-backend,a11y` и без; nextest
  `-p flui-native -p flui-app` — 293 (2 пропущены), с `winit-backend` — 323 (2); clippy Android
  `--all-targets`, macOS lib (и с `winit-backend,a11y`), iOS lib (`DOCS_RS=1`), wasm32 lib;
  `wasm-check`, `wasm-test` — 50; `module-dag` (298 модулей, 9 фич с 69 файлами платформ, 356 рёбер,
  41 исключение) и самотест (54), `globals` (55) и самотест (15), `file-length`, `markers`,
  `testing`, `docs-paths`, `checks` (с `android-dex`), `doc-strict`, `workspace` (28 крейтов,
  229 рёбер), `reach`. WSL Ubuntu 24.04 (`WAYLAND_DISPLAY` снят): clippy `-p flui-native -p flui-app
  --all-targets` (и с `a11y`), nextest под xvfb — 313 (1 пропущен; семь тестов
  `window::linux::x11::tests` на `QuietHost`), rustdoc `-D warnings --document-private-items` для
  `flui-native` с `winit-backend,a11y` и без, `xtask doc-strict` (задача `doc` в CI). Ссылки,
  цель которых есть не на каждом хосте сборки документации, записаны кодом (`visibility` окна;
  `source` дисплеев и модели Linux — их сборка без признаков падала и на `main`), а
  `PlatformWindow::close`/`window_handle` в `owner/winit.rs` — полными путями `flui_platform`.
  iOS — только clippy lib (`DOCS_RS=1`), устройства и симулятора нет. Galaxy S21, 2026-10-22,
  Android 15, проба `flui create --template empty` с `SafeArea` и `RawTextField`: поле под строкой
  состояния; клавиатура Samsung, ввод «ар1» — составление подчёркнуто, «Done» закрывает его
  (действие редактора дошло до поля); поворот в альбом и обратно — окно пересобрано по размеру, вставки
  слева (вырез) и справа (панель навигации), текст и каретка на месте; Back — IME скрыт, второй Back —
  «Destroy — shutting down», цикл завершён; `FATAL`, паник и ANR нет; после переноса на LY33 —
  повторный короткий прогон той же пробы (ввод «а1» составляется, «Done», Back, цикл завершён);
  после переноса на LY32 — ещё один (поле под строкой состояния, `adb input text a1` дошёл до поля
  с кареткой в конце, «Done», Back, Back — «Destroy — shutting down»; `FATAL`, паник и ANR в
  процессе пробы нет; ручного drag на устройстве не было, `drag_ended` покрыт только сборкой);
  поворот возвращён (`accelerometer_rotation 1`, `user_rotation 0`), проба и снимки удалены. Не запускалось:
  `cross-typecheck` локально (нет `zig`; job в CI), `web_browser` (нет ChromeDriver), clippy wasm32
  `--all-targets` — находки в `tests/wasm32/web_page_lifecycle.rs` и в мёртвом под wasm коде
  `owner::call_point`/`owner::panic_boundary`, файлах, которых ветка не трогает; Mac и iPhone — в
  очереди vanyastaff/flui#1840.
- [x] **LY33** — ветка `platform/preferences-lifecycle`, поверх LY31 (`platform/display` @
  `365b28b82`, в нём `main` @ `f4af625af` с LY30). `preferences/` и `lifecycle/` — каталоги фич
  (`git mv`, где файл переезжает целиком; публичные пути не изменились: `HeadlessPreferences` и
  `HeadlessHostEvents` реэкспортирует `platforms::headless`, changelog не нужен). Один файл на
  платформу: прежние файлы платформы — встроенные модули её файла, дочерний файл — только
  механизм одного объекта ОС или тесты модуля. `preferences/`: модель `read`
  (`shared/preference_read`) и `locales` (`shared/preferred_locales`); `windows`
  (+`windows/{languages,tests,retry_tests}`), `apple/{macos,ios}`, `android` (модули
  `configuration`, правила `contrast` и `text_weight`), `web` (`browser`, правило `media`),
  `linux` (портал Settings из `winit/preferences.rs`; наблюдение не называет типов winit, связка с
  winit — свой блок `cfg(feature = "winit-backend")`, так что Wayland и X11 в LY78/LY82
  запускают его тем же `PreferenceSource::new`), `headless`. `lifecycle/`: `notice` (канал из
  `shared/system_notice`), `windows` (модули `host_events`, правила `messages` и `memory_watch`;
  тесты `HostEvents` — `windows/tests.rs`), `apple/macos` (`notices`, правило
  `memory_pressure`), `android` (правило колбэков Activity и чтение `lastTrimLevel`), `web`,
  `linux` (входящий ящик и источник; `linux/{logind,pressure}.rs`, в каждом его правило —
  `signals` и `triggers`, `logind/bus_tests.rs`), `headless`. Чистый модуль — `cfg(any(<os>,
  test))` и `forbid(unsafe_code)`, FFI — `cfg(<os>)` со своим `expect(unsafe_code)`. Форма связи
  цикла и таблицы — та же, что у LY27, LY30 и LY31: один трейт хоста на модуль `owner/`, его
  методы реализует таблица, хук-трейты объявлены рядом с циклом, реализуют их файлы фич, а цикл —
  позиция таблицы, так что таблица не назовёт один тип и не построит другой. Методы LY33:
  `WindowsHost::{preferences, notices, lifecycle}` (из `OwnerSeeds`), `MacosHost::{preferences,
  notices, notice_sources}`, `IosHost::{preferences, notices}`, `AndroidHost::{preferences,
  notices, activity}`, `WebHost::{preferences, page}`, `WinitHost::{notices, preferences,
  host_notices}`, `HeadlessHost::notices`. Хуки: `owner::notices` — `NoticeChannel { set_hook,
  mark_held_stale_on_wake, dispatch }` (`mark_stale_on_wake::<T>` — на `dyn NoticeChannel`),
  `StaleOnWake`, `SystemNoticeHook` и `LifecycleHooks { notices, set_session_end_hook,
  release_session_end, set_reopen_hook }`; `owner::sources::SystemPreferenceSource` (без `Any`;
  `Any` — у `OwnerPreferences` Windows ради `kept` в тестах) и трейты циклов над ним
  (`OwnerPreferences`, `UIKitPreferences`, `ActivityPreferences`, `PortalPreferences`);
  `OwnerLifecycle`; `ActivityCallbacks`. `lifecycle::notice::SystemNotices` реализует канал,
  тела — в реализациях трейтов. Источник, который цикл запускает позже, — объект с методом
  запуска: `NoticeSources::listen() -> Watch` (`SystemNoticeSources`, после бутстрапа),
  `PageLifecycle::listen(report) -> Watch` (`PageSession`), а у winit, чей сигнал появляется
  только при запуске, — `PreferenceStart::start` (`WinitSettings` в `preferences::{windows,
  apple::macos, linux}`) и `HostNoticeStart::start` (`WinitNotices`); указателей на функции и
  псевдонимов нет, `owner::watch::Watch` — файл LY31. Позиции: цикл — позиция предпочтений
  везде, кроме headless, и позиция lifecycle везде, кроме headless и winit на Windows и macOS
  (`Absent<Lifecycle>`); у headless позиции — его тестовые ручки (`HeadlessPreferences`,
  `HeadlessHostEvents`). Циклы, которые слышат lifecycle в своих колбэках (macOS, iOS, Android,
  winit на Linux), реализуют `LifecycleHooks`, и одна обобщённая реализация в
  `lifecycle/mod.rs` (`impl<L: LifecycleHooks> host::Lifecycle<L> for L`) делает их позицией;
  Windows и Web отвечают сами (`owner/windows.rs`, `owner/web.rs`). Источники winit и позиция
  предпочтений winit — `owner/winit/system_sources.rs` (`owner/winit.rs` иначе перешёл бы 3000
  строк), lifecycle winit на Linux — `owner/winit/session_end.rs`. Правило Activity:
  `ActivityLifecycle` (состояние между колбэками, `pressure_for_trim_level`) и `last_trim_level`
  (JNI) — `lifecycle::android` (`rule` — чистый модуль), хук лупера `ActivityCallbacks { fn
  react(&self, ActivityEvent) -> Reaction }` реализует `lifecycle::android::ActivityRule`;
  словарь (`ActivityEvent`, `ActivitySignal`, `Reaction`, `ActivityTarget`) — в
  `owner::android::activity_callbacks`, его оконная половина уходит в LY28 (строка LY28). В
  `owner/` остались `terminate_gate` и `scene_error` (ответ и сцены цикла), `with_activity`
  (`owner/android/activity.rs`; им пользуется и `display::android`, исключение LY31 на
  `platforms::android::preferences` снято), `observation` AppKit. Видимость: `LOW_MEMORY` и пять
  ручек `*_for_test` у `HostEvents` — `pub(super)` (ручки под `cfg(test)`),
  `invalidate_for_retry_test` — приватная, `LoopControl::{session_end_released, set_reopen}` —
  `pub(super)`, типы источников фич — `pub(crate)`. `platforms/winit/linux_notices.rs`
  переименован в `linux_unreported.rs` (зонд X11 → LY28, проверка дисплеев → LY31).
  `module-dag`: `features = ["display", "text", "preferences", "lifecycle"]`, исключения — 10 от
  LY30 и LY31 (у LY33 своих нет: источник предпочтений Windows больше не держит дисплеи);
  входящие рёбра из `platforms/` в файлы LY33 названы в строках LY28 и LY29. `Absent<Lifecycle>`
  у winit на Windows и macOS сбрасывает каждую регистрацию с её захватами сразу (тест
  `winit_lifecycle_registrations_drop_their_captures_without_a_source`); наблюдаемой разницы у
  `flui-app` нет. Web-позиция клонирует `Rc` источника: `Weak` пришлось бы поднять до сильной
  ссылки на время опроса, это то же самое. Тесты: перенесённые — только пути и импорты (у
  `hosted_receiver` в приёмнике один подписчик `HostEvents`: тесты шлют `LOW_MEMORY` и `WM_NULL`,
  подписчик предпочтений на них не отвечает; тест LY31
  `the_displays_and_the_preferences_hear_every_change_in_either_order` берёт источник по новому
  пути); новый — `winit_lifecycle_registrations_drop_their_captures_without_a_source`. Проверки
  (2026-10-22, после rebase): `cargo fmt`; clippy `-D warnings` на Windows (`-p flui-native -p
  flui-app --all-targets`; `-p flui-native --all-targets --features winit-backend`), Android
  (`--all-targets`), macOS (`--all-targets`, и с `winit-backend`), iOS lib (`DOCS_RS=1`), wasm32
  lib; clippy wasm32 `--all-targets` падает и на `origin/main` @ `f4af625af` тем же — 31 ошибка в
  `tests/wasm32/web_page_lifecycle.rs` и 6 мёртвого кода в `owner::{call_point,
  panic_boundary}` под `test`; nextest Windows `-p flui-native` — 237 (1 пропущен), с
  `winit-backend` — 266 (1 пропущен); `wasm-check`, `wasm-test`; `module-dag` (287 модулей,
  4 фичи с 34 файлами платформ, 1 base, 430 рёбер, 10 исключений) и самотест (54); `globals`
  (55), `file-length`, `markers`, `testing`, `docs-paths`, `checks`, `doc-strict`, `workspace`,
  `reach`. WSL Ubuntu 24.04: clippy без фич и с `winit-backend`; nextest `-p flui-native`
  (`FLUI_HEADLESS=1`) — 180; с `winit-backend` под xvfb — 258 из 258, среди них
  `linux_preferences`, `lifecycle::linux::logind::bus_tests` и `owner::winit::session_end`.
  Linux: `linux_preferences` идёт со стенд-ином портала на частной шине D-Bus, не с живым
  `xdg-desktop-portal` (в WSL его нет). logind в WSL есть (systemd): проба вне репозитория
  (`WinitPlatform` под xvfb, `on_host_notice` печатает) в сессии (`XDG_SESSION_ID`) — задержка
  `FLUI application … shutdown:sleep … delay` видна в `systemd-inhibit --list`, `loginctl
  lock-session` → `SessionLocked`, `unlock-session` → `SessionUnlocked` через живой цикл winit, до
  rebase и после; вне сессии — «no logind session found», уведомления блокировки выключены, как и
  задумано; сна и выключения в WSL не вызывали. Galaxy S21 (Android 15, проба `flui create
  --template counter --local` с подписчиком `subscribe_notices`, печатающим в журнал; до rebase
  05:32–05:34 и после 06:34–06:35, одинаково): `am send-trim-memory RUNNING_LOW`, Home —
  `MemoryPressure(Moderate)` один раз (на `Pause`), на `Stop` — только `SessionEnded`; возврат —
  ничего; `RUNNING_CRITICAL`, Home — `MemoryPressure(Critical)` один раз, затем `SessionEnded`;
  Back — `Pause`, `SessionEnded`, `Destroy`, цикл закончился; новый запуск, Home —
  `SessionEnded`; `am kill` фонового процесса («kill background») — без колбэков, конец сессии
  уже был объявлен на `Stop`. Проба удалена, настройки устройства не менялись. macOS: перенос
  наблюдателя AppKit, `terminate_gate`, источников уведомлений (`NoticeSources::listen` после
  бутстрапа) и запуска предпочтений winit (`WinitSettings`) должен покрыть прогон в очереди
  vanyastaff/flui#1840. Не запускалось: `cross-typecheck` локально (нет `zig`; его job — в CI),
  `web_browser` (нет браузерного драйвера).
- [x] **LY29** — ветка `platform/input`, на `main` @ `78c487b24` (LY28). Ввод — фича `input/` (`git mv`, где
  файл переезжает целиком; правки — пути, `use`, видимость, разрезы и хуки ниже). Модель:
  `assembly/` (9 файлов, тесты `pointer_assembly_contracts` из `tests/pointer_assembly.rs` — в
  `assembly/tests.rs`; `assembly/touchpad.rs` — показания viewport тачпада как данные, их пишет
  журнал `native-capture`: модель не называет файл платформы; модель запрещает `unsafe`),
  `keys.rs` (с `keyboard_adapter` внутри как `keys::adapter` под
  `cfg(any(android, winit-desktop))` и кодами кнопок мыши `MOUSE_BUTTONS: [MouseButtonCodes; 5]`,
  которые раньше держал `platforms::windows::events`; их читает и drag), `availability.rs`
  (`every_backend_states_its_availability` из `tests/input_capabilities.rs` — в
  `availability/tests.rs`), `scroll.rs` (`pub(crate)`: `tests/` его не называет), `mod.rs`
  (`event_timestamp_ns`, публичен от корня крейта: `flui_native::event_timestamp_ns`, его зовёт
  `flui-app` в `owner_dispatch.rs`; `primary_mouse_info` стал `pub(crate)`; `shared` больше не
  публичен — `changelog.d/platform-input.md`). Файлы платформ (каждая чистая половина —
  `forbid(unsafe_code)`): `windows/{mod (2 138), direct_manipulation (1 180)}.rs` — `mod.rs`:
  хук класса `InputWindows` (сообщения ввода, hit test и таймер тачпада) с состоянием окна
  `InputWindow` и встроенным модулем `raw` (разбор сообщений, бывший `events.rs` с
  `pointer_messages`; его тесты — `raw/tests.rs`); `direct_manipulation.rs` — viewport тачпада,
  механизм одного объекта ОС, с чистым `manipulation` (декодер `shared/direct_manipulation`, тесты
  из `tests/direct_manipulation.rs`) и своими тестами в `direct_manipulation/tests.rs`; `apple.rs`
  (2 048: чистый `keys` из `keys_macos`, `macos` — класс content view и его события, `ios` —
  касания); `android.rs` (1 968: чистые `keys`, `motion`, `scroll` с тестами рядом;
  `AndroidInput`); `web.rs` (1 859: чистые `keys` и `pinch`; `events`, `WebInput`); `winit.rs`
  (2 019, `WinitInput`); `headless.rs` (`HeadlessInputDevices`, `AttachedDevices`).
  `#[path]` нет. Каталоги `platforms/android/` и `platforms/web/` ушли (страж `atomics` — в
  `owner/web.rs`), `shared/` держит только `os_version` и `platform_floor`. Доступ к `assembly/`:
  модуль остаётся `#[doc(hidden)] pub` — записи нативных трасс и conformance в `tests/` декодируют
  через реестр, а `pointer_assembly_allocates_nothing_per_event` — отдельный бинарь со своим
  глобальным аллокатором, `pub(crate)` ему недоступен; `native-capture` пишет через
  `input::assembly::capture`. Так же публичны и скрыты правила, которыми декодирует таблица
  `tests/`: `availability`, `keys`, `apple::keys`, `android::keys`, `web::keys`,
  `windows::direct_manipulation::manipulation`; `android::motion` и `web::pinch` стали
  `pub(crate)`. Хуки (owner/ не называет фичу): Win32 — хуки класса стали экземплярами:
  `WindowsHost::window_class() -> WindowClass { hooks: Rc<[Rc<dyn WindowMessages>]>, input:
  Rc<dyn WindowInputs> }`, класс делается один раз на поток владельца, и процедура окна
  (`window_proc::<H>`, её Win32 зовёт с одним дескриптором) находит его через батут Win32
  `owner::windows::CLASS_ON_THREAD` (ADR-0097, `window_class::<H>()`). Каждый хук держит своё
  состояние окна сам, по `WindowIdentity`: заводит в `created`, снимает в `destroyed` (окно
  объявляет его на `WM_DESTROY`) и на `WM_NCDESTROY` для окна, чьё уничтожение не объявлено;
  указателей Rust в свойствах и слотах окна нет; сообщение, шаг или чтение для окна, которого хук
  не держит, — явный `None`. Хуки слышат шаги окна `WindowMessages::step` (`WindowStep`: размер,
  свёртывание, потеря фокуса, отрисовка, модальный цикл размера, DPI — `Rescaled { dpi }`, смена
  настройки, клавиша, которую приложение пропустило). Хук ввода `InputWindows` (с тачпадом в
  одном хуке: его `answers` — и `DM_POINTERHITTEST`, и таймер насоса; порядка между хуками нет)
  делает `InputWindow` в `created` (часы сообщений, реестр указателей, viewport тачпада, hover,
  модификаторы, мёртвые клавиши, суррогат) и выставляет возможности окна; окно, drag и haptics
  читают его только через `WindowInputs` (`hovered`, `modifiers`, `press_kind`, `pen_in_range`,
  каждое `Option`) из `owner::windows::window_inputs()`; контекст окна этих полей не держит.
  Пропущенная клавиша — шаг, на который окно отвечает своей горячей клавишей. Интервал и
  прямоугольник двойного щелчка — одним чтением источника предпочтений
  (`OwnerPreferences::mouse_gestures(dpi) -> GesturePreferences`; им же пользуется снимок
  предпочтений и геометрия жестов окна). Наборы ответов не менялись; окно получает ввод с viewport
  до регистрации drop target; `window_created` по-прежнему останавливается на первом мёртвом
  окне. Android —
  `AndroidHost::input` (`ActivityInput`: события очереди, сброс касаний, коэффициенты прокрутки),
  winit — `WinitHost::input` (`LoopInput`), headless — `HeadlessHost::devices`
  (`SimulatedDevices`), Web — `WebHost::input` (`HostInput`): окно регистрирует ввод canvas через
  хук цикла в ту же регистрацию, что и drag (`CanvasListeners` стал `CanvasRegistration`:
  слушатели, флаг приёма только для чтения `Accepting`, таймеры, `on_retire(Box<dyn FnOnce()>)` —
  ввод сам держит захваченные указатели и отпускает их, когда регистрация уходит, опека захвата,
  область фокуса), ввод отвечает маршрутом клавиш `CanvasKeys`, который окно отдаёт текстовому
  вводу; собственные слушатели окна (фокус, раскладка, оформление, жизненный цикл страницы,
  полноэкранный режим, потеря контекста) перешли в `window/web.rs`; наблюдение за плотностью
  пикселей для раскладки окна — хук дисплеев `HostDisplays::watch_pixel_ratio(&Accepting, ..)`.
  Отступление от строки: она относила `answer` и `pointer_position`, которые берёт drag, к модели
  ввода, но это чтение DOM (`web_sys`), а модель — общая часть платформ; они у владельца веба,
  `owner::web::event` (вместе с `client_position`), их читают ввод и drag. Позиция ввода таблицы называет цикл (`type Input = <Os>Loop`),
  его `host::Input` реализует файл ввода. Правило «клавиша принадлежит методу ввода» —
  `input::{android,web}::keys::key_composition` (`text::android::keys` ушёл, `text::web::ime`
  стал приватным; `editor_action_keys` — в модуле `input` у `text::android`). Тесты: перенесённые
  меняют только пути и импорты, кроме разрезов: вход `keyboard_tests` звал и контракт указателя
  winit — тот стал отдельным `#[test]` `translated_events_meet_the_pointer_field_contract` в
  `input/winit.rs`; сам `keyboard_tests` разделён, файл Apple не называет winit: проверка кодов
  winit `every_winit_keycode_maps_to_a_canonical_code` — в `input/winit.rs`, сверка трёх таблиц
  `physical_keys_agree_across_backends` — в `tests/backend_conformance.rs` (обёртка
  `winit_events_translate_to_the_cross_backend_contract` ушла); `web_text_table` распался на
  `the_purpose_picks_the_input_mode` (`text::web::ime::tests`) и
  `a_key_of_the_input_method_is_marked_composing` (`input::web::keys::tests`);
  `a_key_during_composition_is_the_input_methods` (Android) переехал из `text::android` в
  `input::android::keys::tests` вместе с правилом. Новые: `two_windows_keep_their_own_input`,
  `a_windows_input_is_gone_once_it_is_destroyed` (падает, если `InputWindows::forget` оставляет
  запись), `a_window_the_input_never_saw_has_no_input`, `the_mouse_buttons_carry_the_win32_codes`;
  `a_window_step_and_its_destruction_reach_every_hook_in_order` (проверял только цикл) снят.
  Записи нативных трасс и conformance в `tests/` проходят без правок, кроме путей импорта. `module-dag`: `input` в `features`; снято 11
  исключений (`drag::web`, `drag::windows`, `text::android`, `text::apple::macos`, `text::web`,
  `window::apple::ios`, `window::apple::macos`, `window::web` в `platforms::*`, три
  `window::windows` → `platforms::windows::{direct_manipulation, events}` и
  `preferences::windows`), добавлено 5: класс content view AppKit делят ввод, текст и окно, и класс
  UIKit view держит касания — `input::apple::macos` → `text::apple::macos` и
  `window::apple::macos`, `text::apple::macos` → `input::apple::macos`, `window::apple::macos` →
  `input::apple::macos`, `window::apple::ios` → `input::apple::ios`; это не снятие, а перенос
  путей: их снимает строка LY38 — content view AppKit и касания UIKit становятся мультиплексорами
  владельца с хуками фич (ADR-0151 §10), нужен прогон на Mac; туда же отнесено `text::web` →
  `window::web` (`is_focus_handoff`), причины исключений говорят то же. Прямые рёбра окна в
  `text::*` (macOS, Web, winit, headless) говорят, что уходят через таблицу хоста, и записаны в
  строку LY38. Итого 35 (было 41). `globals`: `FIRST_MOTION_SEEN` — у `AndroidInput::motion`,
  записи захвата и класса view переехали, добавлен батут Win32 `owner::windows::CLASS_ON_THREAD`,
  итого 56; запись `testing` для насоса тестов тачпада — по новому пути. ADR-0143, 0213, 0217 и 0255 называют пути `input/`;
  `ARCHITECTURE.md` — инвариант фичи ввода, состояния и шаги окна Win32, хуки ввода всех циклов.
  Проверки (Windows): `cargo fmt`; clippy `-D warnings` `-p flui-native -p flui-app --all-targets`
  с `winit-backend,a11y` и без; nextest `-p flui-native -p flui-app` — 300 (2 пропущены), с
  `winit-backend,a11y` — 332 (2); clippy Android `--all-targets`, macOS lib (и с
  `winit-backend,a11y`), iOS lib (`DOCS_RS=1`), wasm32 lib; `wasm-check`, `wasm-test` — 50 (44 у
  `flui-native`); `module-dag` (299 модулей, 10 фич с 77 файлами платформ, 339 рёбер,
  35 исключений) и самотест (54), `globals` (56) и самотест (15), `file-length`, `markers`,
  `testing`, `docs-paths`, `checks` (с `android-dex`), `doc-strict`, `workspace` (28 крейтов,
  229 рёбер), `reach`. WSL Ubuntu 24.04 (`WAYLAND_DISPLAY` и `XDG_SESSION_TYPE` сняты): clippy
  `-p flui-native -p flui-app --all-targets` (и с `winit-backend,a11y`), nextest с
  `FLUI_HEADLESS=1` под xvfb — 318 (1 пропущен), `--all-features` под xvfb (команда CI) — 275,
  rustdoc `-D warnings --document-private-items` для `flui-native` с `winit-backend,a11y` и без.
  На X-сервере WSLg (`DISPLAY=:0`) два теста окна X11, которых ветка не трогает, падают на его
  менеджере окон; под xvfb проходят. Galaxy S21, 2026-10-22, Android 15, проба `flui create
  --template empty` с `GestureDetector` (tap, long press, drag, transform) и временными строками
  `PROBE` в `AndroidInput::{motion, key}`: tap — Down/Up и `on_tap`; свайп — ход и `on_end` drag со
  скоростью −1 138 px/с по вертикали в логических пикселях; удержание 1,5 с — `on_long_press`;
  `KEYCODE_A`, `KEYCODE_1`, `KEYCODE_DPAD_DOWN` — `KeyA`, `Digit1`, `ArrowDown`, Down и Up, без
  композиции; после возврата — tap и горизонтальный свайп, Back — `GoBack`, Pause, «Destroy —
  shutting down», цикл завершён; `FATAL` и паник нет. Касание, удержанное через Home, дало ANR через
  10 с: цикл не читает очередь ввода, пока Activity на паузе (`if resumed { process_input_events() }`
  было и до ветки), и отмену системы для убранного окна никто не принимает — отдельная задача
  (предложена задачей сессии), запись в `docs/gaps.md`, п. 15. Pinch через `adb` не воспроизвести
  (нет мультитача), он покрыт только тестами модели. Строки `PROBE` удалены, проба удалена,
  настройки устройства не менялись. Правки по ревью проводку ввода Android не трогают (только
  `forbid(unsafe_code)` на чистых половинах), повтор на S21 не нужен. Не запускалось:
  `cross-typecheck` локально (нет `zig`; job в CI), `web_browser` (нет ChromeDriver; регистрация
  холста перестроена — браузерный прогон по фокусу, видимости, клавиатуре, указателю и колесу
  делается на итоговом коммите); Mac и iPhone — в очереди vanyastaff/flui#1840 (перенос класса
  content view и его методов, касаний UIKit).
- [x] **LY34** — ветка `platform/version`, на `main` @ `e89a8bc19` (после LY29, `76e511106`). Каталогов `shared/` и
  `platforms/` нет (`git mv`, где файл переезжает целиком). `version/`: `shared/os_version/{mod,
  android, apple, windows}.rs` и `platforms/ios/os_version.rs` → `version/gate.rs` (модули
  `windows`, `apple`, `android` внутри, каждый под `cfg(any(<os>, test))` и с `forbid(unsafe_code)`;
  чтение системы — `WindowsBuild::read` в соседнем модуле `read_build` под `cfg(windows)` с
  `expect(unsafe_code)` (тест `the_running_build_is_read`) и новый `AppleVersion::read` вместо
  свободной `os_version::read`, под `cfg(target_os = "ios")` в `apple` (`NSProcessInfo` читается без
  `unsafe`); правила Windows
  теперь собираются и тестируются на любом хосте, нижняя граница Windows в `floor.rs` — под
  `cfg(any(windows, test))`), генерируемый `shared/platform_floor.rs` → `version/floor.rs`
  (`tools/xtask/src/platform_floor.rs` пишет новый путь); `version` — второй `base` `module-dag`.
  `version/` не называет фич и в тестах: таблица `os_version_gate_table` разделена по месту
  объявления маркеров через `version::gate::testing::row` — в `version::gate` остаются маркеры
  Android, строки NDK — `ndk_gate_table` в `owner::android::ndk`, строка дисплейного линка —
  `frame_rate_range_gate_table` в `display::apple::link_rate`, а маркер `QdcVirtualModeAware`
  объявлен в чистом `display::windows::sample` рядом с флагами `QueryDisplayConfig`, которые он
  выбирает (`display_config_flags` — в `display::windows::source`): строка —
  `virtual_mode_gate_table` на любом хосте, флаги — `windows_build_below_18362_omits_the_virtual_mode_flag`;
  сами строки те же. Остаток `platforms/`: `platforms/headless/platform.rs`
  — `TextStoreHostFactory` → `owner::headless` (его держит состояние владельца), `with_text_store_host`
  → `text::headless`, `with_os_drag` и `drag_and_drop` → `drag::headless`; `platforms/winit/linux_unreported.rs`
  (`WinitApp::sync_unannounced_displays`, `moves_unannounced`) → `owner/winit.rs` (решение цикла: какие
  окна смотреть снова; строка LY31 относила это к `display/winit.rs`, но метод — цикла и читает его
  хук дисплеев); `platforms/{windows,macos,ios,winit,headless}/mod.rs` — публичные имена от корня
  крейта (`WindowsWindow`, `WindowsWindowExt`, `WindowsBackdrop`, …, модуль `win32` — из
  `window/windows/mod.rs`; `MacOSWindow`, `MacOSWindowExt`, …; `IOSWindow`, `IOSSceneEvent`,
  `IOSSceneSessionId`, `IOSSceneAttachmentId`, `IOSExecutor`; `WinitWindow`), `flui-app` называет
  `flui_native::{IOSSceneEvent, IOSSceneSessionId}`, `tests/` — `flui_native::WindowsWindow`; описания
  macOS и iOS — в документации `MacOSPlatform` и `IOSPlatform` (`hosts/apple.rs`); `shared/mod.rs` и
  `platforms/mod.rs` удалены — `changelog.d/platform-version.md`. Скрытые публичные
  `text::{android, web}` стали `pub(crate)` (LY30 оставлял их публичными до LY34 ради таблицы
  `tests/`, правила которой LY29 перенёс в `input`); `text::ime` тоже стал `pub(crate)` под
  `cfg(any(target_os = "android", target_arch = "wasm32", test))` — его называют только текстовые
  хосты Android и Web (`scalar_spans` — только Web). Верхний уровень `src/`: `lib.rs`, `host.rs`, `platform.rs`,
  `host_window.rs`, `error.rs`, `table_test.rs`, а также `executor.rs` и `task.rs`, которые удаляет
  LY61 (строка называет их там); каталоги `hosts`, `owner`, `version`, `storage` и десять фич.
  `module-dag`: ключ `retiring` убран и из xtask (узел, правило, его посаженные находки самотеста);
  `features` — десять фич, `base = ["owner", "version", "storage"]` (`storage` не называет фич, а его
  `DataDirs` называют циклы); новый ключ `crate-level` перечисляет модули верхнего уровня (`host`,
  `platform`, `host_window`, `error`, `executor`, `task`; `table_test` — модуль тестов, `lib.rs` —
  не модуль), и модуль верхнего уровня без роли — находка `undeclared` (самотест: посаженный
  `shared` без роли и имя `gone` в `crate-level`, которого нет, — 54 находки). Снято 2 исключения в `platforms::*`
  (`display::apple::ios` → `platforms::ios::os_version`, `window::headless` →
  `platforms::headless::platform`), осталось 33: (1) `display::winit` → `display::linux` — до LY87;
  (2) 22 из набора LY38 — окно делает объект оконной позиции (Win32 → `accessibility`, `drag`,
  `haptics`; macOS → `accessibility`, `haptics`, `text`; iOS → `haptics`; Android → `drag`,
  `haptics`; Web → `haptics`, `text`; winit → `accessibility::linux`, `text::winit`; headless →
  `accessibility`, `drag`, `text`), общие виды AppKit и UIKit (`text::apple::macos` ↔
  `input::apple::macos`, `input::apple::macos` → `window::apple::macos`, `window::apple::macos` →
  `input::apple::macos`, `window::apple::ios` → `input::apple::ios`) и `text::web` → `window::web`;
  (3) ещё 10 из набора LY38 (решение координатора: строка LY38 их берёт и называет форму снятия,
  `reason` каждого говорит то же словами): по форме «состояние фичи на окно по `WindowIdentity`, через
  её хуки» — `text::windows` ↔ `window::windows` (соединение TSF), `text::android` ↔
  `window::android` (поле IME и захват размера текста), `window::apple::ios` → `text::apple::ios`
  (захват размера текста); по форме «факт через хук `owner/`» — `window::windows` →
  `lifecycle::windows` (блокировка сессии), `window::android` → `preferences::android` (ночной
  режим, плотность, геометрия касаний), `window::apple::macos` → `display::apple::macos` (границы
  экрана), `window::apple::ios` → `display::apple` (`LinkPeriod`, `change_screen`),
  `window::headless` → `display::headless` (разложенные дисплеи; отказ `simulate_move_to` — ошибкой
  окна). Итого остаются набор LY38 — 32 — и ребро LY87. Их снятие меняет пути событий на macOS, iOS
  и Android и требует прогонов на устройствах. `globals`: записей под `shared::`/`platforms::` к этой задаче не осталось (56);
  правило размещения трамплина знает только модуль цикла в `owner/` (каталог бэкендов из xtask
  удалён, тест размещения и фикстура самотеста переписаны: трамплин в файле фичи — не на месте,
  второй в подмодуле цикла — второй трамплин), ADR-0097 получил строку `Amended`. `android-dex`
  проверяет то же встраивание (`owner/android/java_view.rs`). Тесты `owner/` не называют хостов и
  фич: Win32 — заглушки `owner::windows::testing` (`QuietHost<C>` с хуками класса `C: ClassHooks`,
  по умолчанию `NoHooks`, `NoPreferences`, `NoLifecycle`, `NoDisplays`, `NoDrag`, `NoInputs`,
  `NoWindows`), хост тестов диспетчеризации — `QuietHost<FirstThenSecond>` с двумя своими хуками;
  `NoNotices` один на все циклы — `owner::notices::testing` (его берут headless, Win32 и winit); `every_owner_hook_hears_the_window_go_after_one_panics`
  — `NoDisplays` вместо `WindowsDisplays`; `a_replaced_wake_hook_gets_its_modal_frame` — считает кадры,
  которые доставка просит у заглушки `ClassWindows`, вместо окна `WindowsPlatform` (те же три
  доставки, тело изменено); `a_second_host_is_refused_the_window_class` остаётся в
  `owner::windows::tests` на двух хостах-заглушках: класс регистрирует `QuietHost`, а
  `QuietHost<FirstThenSecond>` получает отказ `Init`. winit — `owner::winit::testing`:
  `QuietHost<D>` (дисплеи `D`, по умолчанию `NoDisplays`) на всех целях winit, таблица `QuietTable`,
  позиция окна которой не открывает окон, `quiet_platform` и `quiet_platform_over::<H>()`; на ней
  три теста `owner/winit.rs` (тест смены дисплеев — на `QuietHost<ChangingDisplays>`, дисплеи которого
  будят владельца на `WM_DISPLAYCHANGE`; пересылающих хостов поверх `QuietHost` нет), четыре
  `session_end` и `a_wake_while_the_loop_reads_gets_a_turn_that_reads_again`. Тесты реального цикла,
  открывающие окна, — к фичам: `winit_lane_dropped_after_delivery_unwinds_and_leaves_the_window_gone`,
  `programmatic_close_runs_the_full_teardown_and_exits_the_loop` и
  `wayland_window_refuses_what_a_compositor_does_not_let_a_client_do` → `window/winit/real_loop_tests.rs`,
  `windows_winit_wheels_preserve_raw_units_and_observe_system_policy` и
  `windows_winit_cold_preferences_recover_on_an_idle_owner_turn` → `preferences/windows/winit_tests.rs`;
  их правка — `testing::install_control_lane` (полоса управления, которая будит цикл через прокси, и
  её установка; отдаёт `ControlSender` и `LaneReceiver` и заменяет три копии этой настройки),
  `WinitApp::for_test` вместо литерала, `ControlSender::request_quit_for_test` и
  `WinitLoopState::{window_ids_for_test, cursor_positions_for_test}`; утверждения те же.
  `control_lane`, `ControlReceiver`, `request_quit`, `install_control_lane` и поля `window_id_map`,
  `cursor_positions` закрыты, как на `main`.
  `the_owner_platform_reads_a_window_s_text_store_host` → `text::headless::tests`;
  `idle_display_check_covers_every_wayland_window` — только путь. Чего тесты `owner/` больше не
  проходят: `a_replaced_wake_hook_gets_its_modal_frame` не идёт через `WindowSet::redraw_for_deadline`
  (`window/windows/set.rs`) — его держат `test_window_lifecycle_contract` (случаи сроков),
  `deadlines_are_delivered_while_the_window_is_moved` и `a_second_window_repaints_while_the_modal_loop_rests`,
  и с пустым телом `redraw_for_deadline` падают все три. `a_display_change_reaches_the_displays_when_the_preferences_cannot_open`
  на `ChangingDisplays` проверяет половину цикла (приёмник стоит с одним подписчиком дисплеев, когда
  источник предпочтений не открылся, и доставляет ему `WM_DISPLAYCHANGE`), но не доходит до хука
  `broadcasts` дисплеев Win32 под winit, который на `main` проходил через `WinitPlatform`; его держит
  новый `the_winit_loop_s_displays_hear_a_display_change` (`display::windows::broadcasts`): хук,
  отдающий подписчика на чужом сигнале, роняет его, а тест `owner/` проходит. Самого подписчика
  держат `display_broadcasts_mark_the_monitors_stale_and_wake_until_detached` и
  `the_displays_and_the_preferences_hear_every_change_in_either_order`. «21 файл `tests/`» строки: на
  `main` модуль `shared` уже закрыт (`mod shared;`), и ни один файл `tests/` его не называл — файлы,
  которые считала строка, ушли раньше; LY34 правит в `tests/` три строки пути
  (`flui_native::platforms::windows::WindowsWindow` → `flui_native::WindowsWindow` в `contract.rs`,
  `windows_host_events.rs`, `windows_window_commands.rs`). Записи нативных трасс и conformance
  не тронуты. CI на ветке: `cli_run::android_run_builds_the_selected_device_native_abi` убит на 30 с
  в job `test`. Причина — `flui run --device <serial>` опрашивал все платформы, а обнаружение
  браузеров на Linux запускает каждый установленный браузер с `--version` (до 10 с на браузер);
  на `main` в CI тест шёл 6,8–13,9 с. Воспроизведено на WSL: четыре браузера в `PATH`, каждый
  отвечает через 9 с, — `origin/main` (`c7980c845`) убит на 30,1 с, ветка проходит за 0,24 с.
  `resolve_target` (`crates/flui-cli/src/commands/run.rs`) сначала ищет точный серийный номер среди
  устройств Android (id браузера — `browser:<name>`, серийный номер он взять не может), иначе
  собирает список `flui devices`, опрашивая каждую платформу один раз; фикстура кладёт в `PATH`
  `google-chrome`, который отмечает свой запуск, и тест требует, чтобы его не запускали (с откатом
  исправления падает); `changelog.d/platform-version.md` — `### Fixed`. `feature-matrix (3/5)`
  (`--no-default-features --features a11y` на Linux: мёртвый код в `accessibility::{bridge, linux}`)
  был и на ветке: адаптер AT-SPI собирается там, где его называют, —
  `all(target_os = "linux", feature = "a11y", any(feature = "winit-backend", test))`, Linux-ветвь
  `bridge` — с тем же условием; когда LY88 уберёт `a11y`, условие остаётся осмысленным без него. Документы: ARCHITECTURE.md ядра открывается устройством крейта (верхний уровень,
  каталоги, роли `module-dag`, что осталось в исключениях), инвариант версий называет
  `version::gate` и три таблицы; `docs/architecture.md`, `docs/crates.md`, `docs/extending.md`,
  README ядра, ADR-0244 и ADR-0255 называют новые пути, причины в `clippy.toml` — `version::gate`;
  ADR-0082 своих путей не переписывает, `docs-paths` даёт ему 9 вместо 6. Проверки (Windows, после
  правок ревью): `cargo fmt`; clippy `-D warnings` `-p flui-native -p flui-app -p flui-cli
  --all-targets` с `winit-backend,a11y` и без (без — вместе с `-p xtask`); nextest `-p flui-native
  -p flui-app -p flui-cli` — 361 (2 пропущены), с `winit-backend,a11y` — 394 (2); `-p xtask` — 45;
  мутации: пустое тело `redraw_for_deadline` и чужой сигнал в хуке `broadcasts` роняют свои тесты;
  clippy Android `--all-targets`, macOS lib `-p flui-native` (и с `winit-backend,a11y`), iOS lib
  `-p flui-native` (`DOCS_RS=1`), wasm32 lib; `wasm-check`, `wasm-test` — 50; `module-dag` (291
  модуль, 10 фич с 77 файлами платформ, 3 base, 321 ребро, 33 исключения) и самотест (54), `globals`
  (56) и самотест (15), `platform-floor`, `file-length`, `markers`, `testing`, `docs-paths`,
  `doc-strict`, `workspace` (28 крейтов, 229 рёбер), `reach`, `checks` (с `android-dex` и taplo); `rg
  'unimplemented!' crates/flui-native/src` и `rg 'flui_native::(shared|platforms)' crates tests`
  пусты. Случай `press_captures_the_mouse_until_the_last_release` из `test_window_lifecycle_contract`
  однажды упал на фикстуре клавиатуры потока (утверждение `GetKeyState` при живом вводе на общем
  рабочем столе) и прошёл повторно отдельно и в полном прогоне. WSL Ubuntu 24.04 (`WAYLAND_DISPLAY`
  и `XDG_SESSION_TYPE` сняты): clippy `-p flui-native -p flui-app -p flui-cli --all-targets` (и с
  `winit-backend,a11y`); проходы `feature-matrix` по ядру с `--no-default-features`: `a11y`, `a11y
  --tests`, `winit-backend`, `winit-backend,a11y` и он же с `--tests`; nextest `FLUI_HEADLESS=1`
  под xvfb с `winit-backend,a11y` `-p flui-native -p flui-app -p flui-cli` — 383: 379 прошли, четыре
  `flui-cli` `agent::tests::*` падают только от выставленного скриптом `CARGO_TARGET_DIR` (сессия
  пишет файл туда, процессы тестов делят его; с `--target-dir` — 5 из 5), `--all-features` под xvfb
  (команда CI) — 278, без признаков и без дисплея — 198, тест моста AT-SPI с одним `a11y` — 1;
  rustdoc `-D warnings --document-private-items` с `winit-backend,a11y` и без.
  `x11_window_takes_commands_and_publishes_what_the_server_holds` на X-сервере WSLg падает и на
  `origin/main` (сервер не держит позицию окна), под xvfb проходит. Не запускалось: `cross-typecheck` (нет `zig`; job в CI); clippy
  `flui-app` под macOS и iOS (сборочный скрипт `psm` требует C-компилятора Apple и `xcrun`); Mac и
  iPhone: перенос чтения версии iOS в `AppleVersion::read`, публичные имена macOS и iOS от корня и
  тесты реального цикла на macOS (там они `ignore`) проверены только сборкой.
- [x] **LY36** — ветка `platform/os-types`, на `main` @ `fc8f8ebaa` (LY34). Ни один публичный путь
  ядра, скрытые включая, не называет OS-тип, AccessKit или tokio ни на одной цели, где ядро строит
  хост (R4.10). `as_any` — с `PlatformWindow` на `HostWindow` (обязательный, без тела): каждое окно
  ядра реализует его рядом с прочим `HostWindow`, двойники `PlatformWindow` (`flui-app`,
  `flui-runtime`, `flui-interaction`, `flui-testing`, тесты ядра) его теряют, тесты, приводившие
  headless-окно, держат `Arc<dyn HostWindow>` (`headless_host_window`, `test_host_window`); doc
  контракта — compile_fail E0599 на `window.as_any()` у `dyn PlatformWindow`. Расширения ОС:
  `WindowsWindowExt` и `MacOSWindowExt` реализованы для `dyn HostWindow + '_` поверх `as_any`, берут
  `&self` и отвечают `Option` — `None` для окна другого хоста, у которого ничего не просят (сеттеры
  — `Option<()>`); тела — `pub(crate)` методы `WindowsWindow` и `MacOSWindow`, заглушки, не
  трогающие окно (прогресс на панели задач, тень, анимации, tiling), — связанные функции;
  `add_tab_to_window(&self, &dyn HostWindow) -> Option<bool>` отвечает `Some(false)`, если другое
  окно не AppKit. Скрыто: модуль `win32` удалён (его не называл никто); `WindowsWindow`,
  `MacOSWindow`, `IOSWindow`, `WinitWindow` ушли из корня — по правилу строки «не называет никто вне
  крейта», как только `tests/` перешли на `raw-window-handle` (номинально они `pub` в
  `pub(crate)`-модулях: тип позиции `Host::Window` в публичной реализации); `WindowsWindow::hwnd`,
  `toggle_fullscreen_for_hwnd`, `WindowsAccessibility::new` — `pub(crate)`; `HostPlatform::app`
  macOS и Android и `WebWindow::canvas` удалены (вызывающих нет); `BackgroundExecutor::handle` —
  `pub(crate)` под `cfg(all(linux, winit-backend))`, его зовут источники Linux; `liquid_glass` — типы
  не публичны, `set_liquid_glass*` и `clear_liquid_glass` ушли из `MacOSWindowExt` в `pub(crate)`
  методы окна с `expect(dead_code)` (после ревью удалены, см. ниже);
  `WindowConfiguration` — `pub(crate)` под `cfg(any(windows, macos))` с одним полем, которое читает
  хост (`fullscreen_hotkey`, F11), `FullscreenMonitor` и `resize_debounce_ms` удалены (их не читал
  ни один хост), `with_config` Windows и macOS — тоже; `FakeAccessibility::{published,
  request_action}` удалены (называли `accesskit`, вызывающих нет): двойник считает опубликованное
  (`published_count`), а не хранит деревья, слушатель действий держит до замены и не зовёт; из
  корня ушли `FakeSystemChrome`, `IOSExecutor` и `FakeHaptics` (это `flui_platform::haptics::
  FakeHaptics`, один путь — ADR-0151 §9). Остались: `HeadlessDeferredWindowOpens`
  (`enable_deferred_window_open`, его зовёт `flui-app`), `HeadlessInputDevices`,
  `HeadlessWindowPolicy`, `DisplayChangeError` (`tests/`), `IOSSceneAttachmentId` (поле
  `IOSSceneEvent`), `InputCallback`, `InputWake` (`WindowCallbacks`), `InstallationCancelled`;
  `PlatformReadyCallback` — LY45; `PlatformExecutor`, `WaitError`, `PlatformResult`, `Task`,
  `TaskLabel`, `PathPromptOptions` — LY61. Вход Android: `flui_native::android_platform(app:
  android_activity::AndroidApp) -> Box<dyn Platform>` (`cfg(target_os = "android")`, `#[must_use]`),
  рядом с `current_platform()` и `headless_platform()`; `AndroidPlatform` (псевдоним, `new`, `app`)
  ушёл, `HostPlatform<Android>::new` — `pub(crate)`; других типов `android_activity` вход не
  называет, фасад новых не получает (`run_app_android` берёт тот же `AndroidApp`), а
  `current_platform()` на Android отвечает ошибкой с именем входа. `tests/` берут `HWND` как
  встраивающий код — через `raw-window-handle` (`win32_handle` в `tests/main.rs`; `contract.rs`,
  `windows_window_commands.rs`, `windows_host_events.rs` меняют только приведение на этот хелпер и
  импорты); тесты в крейте — пути `crate::window::windows::WindowsWindow` и
  `crate::haptics::headless::FakeHaptics`. Примеров `windows11_demo` и `resize_jitter_probe` нет с
  #1827 — переводить нечего (строка LY37 их тоже называет). Гейт API-замыкания, решения сверх
  строки: (1) `targets` — rustdoc-JSON по сборке на цель (`features` или `all-features`): Linux,
  Windows, macOS — все фичи, iOS, Android, wasm32 — по умолчанию; хостом CI одним `HWND` не увидеть
  никогда; находка называет сборку; rustdoc идёт с `DOCS_RS=1`, как сборка docs.rs
  (`objc2-exception-helper` на iOS не компилирует C); (2) `entries` — одному элементу `deny`-пакета
  можно назвать один запрещённый крейт (`flui_native::android_platform` → `android_activity`),
  ненужная запись — находка; (3) под `deny` метод реализации трейта другого крейта workspace —
  сигнатура этого трейта, за неё отвечает его крейт: окна и `Absent<position::Accessibility>`
  (скрытый публичный `host`) реализуют `PlatformAccessibility` из `flui-semantics`, чьи методы
  называют дерево AccessKit; заголовок реализации и её ассоциированные типы проверяются; (4)
  `workspace` не знал ключа `api-closure` (LY39 таблиц не заводил) — добавлен. `deny` ядра — 37
  крейтов (`windows`, `windows_core`, семейство `objc2`, `core_foundation`, `core_graphics`,
  `dispatch{,2}`, `block2`, `android_activity`, `ndk{,_sys}`, `jni{,_sys}`, `libc`, `web_sys`,
  `js_sys`, `wasm_bindgen`, `winit`, `ui_events_winit`, `arboard`, `x11rb`, `wayland_*`, `rustix`,
  `zbus`, `accesskit*`, `tokio`). CI: job `doc` ставит nightly с пятью целями, таймаут 40 мин.
  ADR-0151 — `Amended` (вход Android в §8, подтверждение гейтом по целям), ADR-0089 — проверка
  называет новые случаи самотеста; `docs/architecture.md`, README и ARCHITECTURE.md ядра (инвариант
  «No public path names an OS type»), строка материала фона в `docs/platform-parity.md`;
  `changelog.d/platform-os-types.md`, а из фрагмента LY34 убраны имена, которых больше нет.
  Исключения `module-dag` не менялись: 33. Тесты: строка
  `the_windows_extension_answers_only_for_a_win32_window` в `native_window_commands` (headless —
  `None` у `backdrop`, `set_backdrop`, `set_corner_preference`, `is_dark_mode`, `dpi`,
  `convert_point_to_device`, `set_taskbar_progress`; Win32 — `dpi() > 0`, `Some(())`, `Some(_)`):
  без исправления не компилируется, с `expect` в приведении (как `unwrap` winit) падает —
  «failing cases: ["the_windows_extension_answers_only_for_a_win32_window"]»; xtask —
  `a_method_of_a_workspace_trait_is_that_traits_signature_under_a_deny_list`,
  `an_entry_admits_its_item_and_crate_until_no_build_needs_it`,
  `an_entry_one_build_needs_is_needed_and_each_build_reports_its_own` (без пропуска метода трейта
  workspace падает первый, без поиска записи — два других), самотест — 6 случаев (+2). Гейт до
  правок (Windows, `nightly-2026-10-12`, хост, пробный `deny`) — 23 находки: `win32` (5), `hwnd`,
  `toggle_fullscreen_for_hwnd`, `FakeAccessibility` (3), `Absent<Accessibility>::publish`,
  `BackgroundExecutor::handle` и 11 сеттеров `WindowCallbacks` с `parking_lot`, который в итоговый
  `deny` не вошёл; после — пусто на всех шести целях (`1.99.0` с `RUSTC_BOOTSTRAP=1`: у nightly на
  этой машине только цель Windows), переименованная запись даёт находку сборки Android и «names no
  `android_activity` item». Проверки (Windows): `cargo fmt`; clippy `-D warnings` `-p flui-platform
  -p flui-native -p flui-app --all-targets`, `-p flui-native -p flui-app --all-targets --features
  flui-native/winit-backend,flui-native/a11y`, `-p flui-runtime -p flui-interaction -p flui-testing
  -p flui --all-targets`, `-p xtask --all-targets`; nextest `-p flui-native -p flui-app -p
  flui-platform` — 343 (2 пропущены), `-p flui-native -p flui-app` с `winit-backend,a11y` — 337 (2),
  `-p xtask` — 48; doctest'ы `flui-platform` и `flui-native`; clippy Android `-p flui-native -p
  flui-app --all-targets`, macOS lib (и с `winit-backend,a11y`), iOS lib (`DOCS_RS=1`), wasm32 lib;
  `wasm-check`, `wasm-test` — 51; `checks` (taplo, `android-dex`, `module-dag` — 290 модулей, 33
  исключения, — и самотест, `globals` — 56 — и самотест, `file-length`, `markers`, `testing`,
  `docs-paths`, `workspace` — 28 крейтов, 229 рёбер, `reach`), `doc-strict`; `api-closure -p flui -p
  flui-platform -p flui-protocol` на `nightly-2026-10-12` — чисто. WSL Ubuntu 24.04 (`WAYLAND_DISPLAY`
  и `XDG_SESSION_TYPE` сняты): clippy `-p flui-platform -p flui-native -p flui-app --all-targets` и с
  `winit-backend,a11y` (первый прогон нашёл `BackgroundExecutor::handle` у источников Linux — отсюда
  `pub(crate)` выше); nextest `FLUI_HEADLESS=1` `-p flui-native -p flui-app -p flui-platform` — 360:
  358, два теста X11 (`x11_window_asked_for_a_mode_before_its_first_frame_opens_in_that_mode`,
  `x11_window_takes_commands_and_publishes_what_the_server_holds`) падают на X-сервере WSLg, как в
  LY29 и LY34; `--all-features` под xvfb (команда CI) — 278 из 278; rustdoc `-D warnings
  --document-private-items` без фич и с `winit-backend,a11y`; `api-closure` трёх Stable-пакетов и
  самотест на nightly — чисто, 6 случаев (nightly в WSL поставил сам `rustup +nightly target list`;
  целей, кроме Linux, у него нет). Устройства не нужны: путь Android меняет только имя входа (тот же
  `HostPlatform::<Android>::new`), его держит clippy Android. Не запускалось: `cross-typecheck` (нет
  `zig`; job в CI); `api-closure -p flui-native` хостом Linux на nightly — его первый прогон в job
  `doc`; Mac и iPhone (расширение macOS поверх `dyn HostWindow`, `liquid_glass`, корень iOS) — в
  очереди vanyastaff/flui#1840.
  После ревью PR #1874 и слияния с `main` @ `59e63b27c` (LY88: фичи `a11y` нет; конфликтов кода не
  было, кроме строки LY88 в `tests/contract.rs`, которая приводила окно к `WindowsWindow`, — теперь
  `win32().is_some()`). Расширения ОС — по одному методу: `WindowsWindowExt::win32() ->
  Option<Win32Window<'_>>`, `MacOSWindowExt::appkit() -> Option<AppKitWindow<'_>>` (`#[must_use]`);
  37 однострочных пересылок `Option<()>` ушли, тела — методы видов, отказать по чужому хосту они
  не могут; `AppKitWindow::add_tab_to_window -> Result<(), TabJoinError>` (`NotAppKit`,
  `SameWindow`, `NotOnTheMainLane`, `Closed`). Удалены заглушки, отвечавшие успехом без действия:
  Windows — прогресс на панели задач (с `TaskbarProgressState`), тень, анимации; macOS — tiling
  (`enable_tiling`, `disable_tiling`, `is_tiling_enabled` и пять типов модуля `tiling`); удалён
  `liquid_glass` (≈330 строк `unsafe` AppKit, недостижимых, под `expect(dead_code)`), вернётся из git
  в LY37 методами вида `appkit()` (ADR-0254 п. 3: материалы одной ОС — через `flui::native::<os>`;
  строки LY37 и LY43 это называют). `HostWindow::as_any` — одно тело через
  печать (`Sealed::any` в обобщённой реализации над `CoreHostWindow: 'static`), девять ручных тел
  ушли; doc больше не утверждает, что снаружи нечего привести (`MockWindow` публичен).
  `FakeAccessibility` бросает слушатель действий сразу (вне замка). Ошибки и doc не называют
  удалённые `MacOSPlatform::with_config` и `AndroidPlatform`. Гейт: список `allow` вместо `deny`
  (`cursor_icon`, `parking_lot`, `raw_window_handle`, `smallvec`, `web_time` — что API называет
  сегодня на шести целях): запретный список открыт по умолчанию (`windows-sys`, `objc2-metal`,
  `objc2-core-graphics` в lockfile его миновали), разрешающий — закрыт; изъятие для методов трейта
  workspace — поимённое (`traits`: `flui_semantics::platform::PlatformAccessibility` →
  `accesskit`, ненужное — находка); таблица читается с `deny_unknown_fields`; упавший rustdoc
  отсылает к своим ошибкам выше. CI: job `doc` упал на nightly rustdoc `-D warnings` под
  `x86_64-pc-windows-msvc` — две избыточные ссылки в `input/windows/direct_manipulation.rs`
  (у Linux они не разрешались, теперь путь — подпись ссылки); таймаут `doc` возвращён к 25 мин
  (`main` — 13 мин, шесть сборок ядра — около минуты каждая). Строки: LY36 называет `entries` и
  `traits` и отдаёт LY100 часть R4.10 про двойники (строка LY100 и таблица трассировки — тоже),
  R4.10 в `requirements.md` называет единственный вход Android; LY37 — форма видов, примеров нет,
  заглушек нет, Liquid Glass возвращается в ней методами `AppKitWindow`; LY43 — общий словарь
  материалов в команде окна, материалы одной ОС — через `flui::native::<os>` (ADR-0254 п. 3,
  ADR не менялся: это решение владельца). Тест таблицы чужих хостов —
  `tests/os_extensions.rs`, свой харнесс (`libtest-mimic`, один поток: winit строит цикл только
  на главном потоке): `the_windows_extension_has_no_view_of_another_hosts_window` — окна headless и
  winit (`winit-backend`) без вида Win32, окно Win32 с видом (`dpi() > 0`); с `expect` в приведении
  падает на главном потоке; `the_macos_extension_has_no_view_of_another_hosts_window` — только
  проверка типов (cross-typecheck). xtask: `an_allow_list_reports_every_crate_it_does_not_name`,
  `an_admitted_trait_lets_its_implementations_methods_name_its_crate`,
  `a_misspelled_key_of_the_table_is_an_error_not_a_narrower_build`, тесты записей переведены на
  `allow`; самотест — 7 случаев (+ ОС-крейт, которого нет ни в одном списке; допущенный, другой и
  отсутствующий трейт). Проверки после слияния (Windows): `cargo fmt`; clippy `-D warnings` `-p
  flui-platform -p flui-native -p flui-app --all-targets` и с `flui-native/winit-backend`, `-p
  flui-runtime -p flui-testing -p flui-interaction -p flui -p xtask --all-targets`; nextest `-p
  flui-platform -p flui-native -p flui-app -p xtask` — 393 (2 пропущены), `-p flui-native -p
  flui-app` с `winit-backend` — 338 (2), `-p flui-runtime -p flui-testing -p flui` — 138;
  doctest'ы `flui-platform`, `flui-native`; clippy Android `--all-targets`, macOS lib (и с
  `winit-backend`), iOS lib (`DOCS_RS=1`), wasm32 lib; `wasm-check`, `wasm-test` — 51; `checks`
  (`module-dag` — 288 модулей, 33 исключения; `globals` — 56), `doc-strict`; `api-closure` всех
  четырёх пакетов на шести целях с `RUSTDOCFLAGS=-D warnings` (`1.99.0`, `RUSTC_BOOTSTRAP=1`) —
  чисто, самотест — 7; nightly `2026-10-12` — Stable-пакеты чисто, rustdoc ядра под Windows со
  всеми фичами и `-D warnings` — чисто. WSL (без `WAYLAND_DISPLAY`): clippy с `winit-backend` и без,
  `--all-features` под xvfb — 279 из 279, rustdoc `-D warnings --document-private-items` с
  `winit-backend` и без, самотест гейта на nightly — 7.
- [x] **LY88** — ветка `platform/a11y-always`, на `main` @ `fc8f8ebaa` (LY34). Фичи `a11y` нет ни в
  `flui-native`, ни в `flui-app`, ни в фасаде. `accesskit_windows` и `accesskit_macos` — безусловные
  зависимости своих ОС: каждое окно Win32 и AppKit строит адаптер, таблица Win32 называет
  `WindowsAccessibility` без двойника `Absent`, таблица Apple — `MacosAccessibility` на macOS и
  `Absent` на iOS. Отступление от строки (решение координатора, записано поправкой `Amended` к
  ADR-0151 §15 и в строках LY88, LY78 и LY82): адаптер AT-SPI приходит с хостом Linux. Linux-хоста,
  кроме winit, нет, без `winit-backend` адаптер никто не называет, и безусловная зависимость собирала
  бы около 66 крейтов (zbus, atspi), которых не зовёт ни один код; поэтому `accesskit_unix` приносит
  `winit-backend` рядом с `zbus` (его `flui-app` включает на Linux всегда, так что AT-SPI есть в
  каждой сборке приложения), а условие `accessibility::linux` и Linux-ветви `bridge` —
  `all(target_os = "linux", feature = "winit-backend")`, записанное один раз, в
  `accessibility/mod.rs` (файлы winit говорят только `target_os = "linux"`: они сами под
  `winit-backend`). `build.rs` у ядра нет, и заводить его ради псевдонима `cfg` — вернуть сборочный
  скрипт, удаление которого записано в ARCHITECTURE.md. LY78 (или LY82, если раньше) делает
  `accesskit_unix` и `zbus` безусловными и условие — `target_os = "linux"`. Тест моста AT-SPI идёт с
  `winit-backend` (в CI — `--all-features`), а не без признаков. `cargo shear` находок по
  зависимостям не даёт (12 его предупреждений — о файлах trybuild, не о зависимостях).
  Остальные условия: `accessibility::{apple, windows}` — `cfg(target_os = "macos")` и `cfg(windows)`,
  таблица winit — `UnixAccessibility` на Linux и `Absent` на Windows и macOS, поля и
  `HostWindow::accessibility` окон Win32, AppKit и winit (Linux) — без `cfg(feature)`,
  `owner::panic_boundary::retain_off_owner_resource` — `any(windows, macos, test)`. xtask:
  `facade-combos` — 9 сочетаний (без `--features a11y`), `cross-typecheck` включает из объявленных
  только `testing`. Документы: README (абзац вместо строки таблицы фич), документация фасада и
  `flui-native`, `docs/testing.md` (`cross-typecheck`), `docs/platform-parity.md` (заметка «Tree and
  actions»; клетки те же: прогона со скринридером нет), `docs/gaps.md` (запись 2),
  попутная правка документации `PlatformWindow::request_redraw` в `flui-platform` (слушатель
  активации AccessKit принадлежит `flui_runtime`, не `flui_app`, и в сборке по умолчанию он теперь
  достижим), ARCHITECTURE.md ядра (абзац о зависимостях адаптеров); таблицы истории и шапки
  «Last updated» в `docs/platform-parity.md` и `docs/gaps.md` получили строку этого изменения;
  `changelog.d/platform-a11y-always.md` — `### Removed` (имя по ветке, как велит
  `changelog.d/README.md`). ADR-0084 своих строк про `a11y` не переписывает: это запись; `rg
  'feature = "a11y"'` вне `docs/plans` и ADR-0084 пуст. Тесты: новый
  `window::winit::real_loop_tests::a_linux_winit_window_hands_out_its_at_spi_bridge` открывает окно
  winit на настоящем цикле (Xvfb в CI, `--all-features`) и требует его мост AT-SPI; с выключенным
  `HostWindow::accessibility` окна winit он падает, с исправлением прошёл 5 раз из 5.
  `test_window_lifecycle_contract` получил пункт 9 — окно Win32 (проверка типа
  `WindowsWindow`, так что двойник headless, у которого мост есть всегда, его не проходит) отдаёт мост
  UI Automation; с выключенным `HostWindow::accessibility` окна Win32 (то, что давала сборка по
  умолчанию с `a11y` выключенной) он падает. Окно macOS тест не открывает (там он `ignore`), окно
  winit на Linux вне `run()` не открывается — его держит тест реального цикла. Исключений
  `module-dag` по-прежнему 33. Проверки (Windows): `cargo fmt`; clippy `-D
  warnings` `-p flui-native -p flui-app -p flui-platform --all-targets` и `-p flui-native -p
  flui-app` с `winit-backend`, `-p xtask`; nextest `-p flui-native -p flui-app` — 304 (2 пропущены),
  с `winit-backend` — 337 (2), `-p xtask` — 45; `facade-combos` (9); clippy macOS lib `-p
  flui-native` (и с `winit-backend`) и `--all-targets` (`clang` из LLVM), Android `--all-targets` `-p
  flui-native -p flui-app`, iOS lib (`DOCS_RS=1`), wasm32 lib; `wasm-check`, `wasm-test` — 51;
  `checks` (с `android-dex` и taplo), `module-dag` (291 модуль, 321 ребро, 33 исключения) и самотест
  (54), `globals` (56) и самотест (15), `file-length`, `markers`, `testing`, `docs-paths`,
  `doc-strict`, `workspace` (28 крейтов, 229 рёбер), `reach` (68 корневых сборок), `deps --only
  policy`. WSL Ubuntu 24.04 (`WAYLAND_DISPLAY` и `XDG_SESSION_TYPE` сняты): clippy `-p flui-native -p
  flui-app --all-targets` и с `winit-backend`; проходы `feature-matrix` (cargo-hack в WSL нет, его
  `--each-feature` повторён руками): `flui-native` — по умолчанию, `--no-default-features` и каждая
  из `desktop`, `winit-backend`, `web`, `wayland`, `x11`, `native-capture`, каждый и с `--tests
  --benches --examples` (16), `flui-app` — 6, фасад — те же 9 сочетаний; nextest без признаков и без
  дисплея `-p flui-native` — 199 (в первой редакции, с dev-зависимостью; см. «После ревью»), `-p
  flui-app` под xvfb — 45 (1 пропущен), `--all-features` под xvfb (команда CI) — 278, `winit-backend`
  под xvfb — 322 (1); в первом прогоне `logind_lock_and_unlock_reach_the_owner` не дождался события
  за 5 с, отдельно прошёл 5 раз из 5 и в повторе всего набора; rustdoc `-D warnings
  --document-private-items` с `winit-backend` и без. Устройства: Android и Web не тронуты (адаптеров
  нет); Mac — сборка адаптера AppKit без фичи проверена только clippy, прогон в
  vanyastaff/flui#1840. Не запускалось: `cross-typecheck` (нет `zig`; job в CI). После ревью
  (vanyastaff/flui#1871): dev-зависимость `accesskit_unix` и ветвь `test` условия сняты, поправка
  ADR-0151, тест реального цикла, пункт 9 только для окна Win32, строки истории документов.
  Проверки: `cargo fmt`; Windows — clippy `-p flui-native -p flui-app --all-targets` и с
  `winit-backend`, nextest — 304 (2 пропущены) и с `winit-backend` — 337 (2;
  `haptics::windows::tests::a_stalled_pen_worker_plays_only_the_newest_request_and_never_late`
  однажды не уложился в 5 с под нагрузкой хоста и прошёл повторно); clippy macOS lib (и с
  `winit-backend`) и `--all-targets`, Android `--all-targets`, iOS lib (`DOCS_RS=1`), wasm32 lib;
  WSL — clippy с `winit-backend` и без, 16 проходов `feature-matrix` по ядру, nextest без признаков
  и без дисплея — 198 (тест моста AT-SPI теперь идёт с `winit-backend`), `--all-features` под xvfb
  — 279 (с новым тестом), новый тест под xvfb — 5 из 5; `checks`, `module-dag`, `facade-combos`,
  `deps --only policy`, `docs-paths`, `markers`.
- [x] **LY90** — ветка `platform/panic-boundary-gate`, на LY34 (`dbbb8d7d9`); слиты `fc8f8ebaa`,
  LY88 (`ccda248ee`, признака `a11y` больше нет), `59e63b27c` и LY35 (`5cf18f936`). Граница —
  одна функция: `owner::panic_boundary::os_entry<A: Copy + Debug, R>(entry: &'static str, args: A,
  after_panic: AfterPanic<A, R>, body: impl FnOnce() -> R) -> R`,
  `AfterPanic::{Answer(fn(A) -> R), Abort}`.
  `args` — то, с чем ОС вызвала вход (сообщение и окно, делегат): лог паники несёт их (кроме
  ZST) и текст паники под именем входа, ответ строится из них. Ответ — `fn`-указатель: ничего не
  захватывает и создаётся только после паники (ошибка COM читает `GetErrorInfo` потока в момент
  создания). Порядок: лог (погашен — подписчик лога пользовательский), удержание нагрузки, ответ;
  `Abort` и ответ, который сам паникует, завершают процесс (`end_process`), так что первый отказ
  всегда в логе раньше. `Abort` — где состояние может быть порвано посреди диспетчеризации:
  процедура окна Win32 (в логе снова `msg` и `hwnd`), хук cloak (`hwnd`), `configuration` и
  `init` UIKit, страж полосы владельца macOS; на Android и Linux его нет
  (`cfg_attr(…, expect(dead_code))`). Шаги внутри входа гасятся порознь, каждый с текстом паники
  под именем входа: `contain_os_entry(entry, body) -> Option<R>` (Windows, macOS) — каждый
  подписчик широковещательного приёмника, каждый хук окна владельца, провайдер drag, ветка `Now`
  `loop_control`; `contain_owner_callback` тоже логирует текст. Ответы после паники:
  широковещательный приёмник — TRUE на `WM_QUERYENDSESSION` (и запрос, оставленный без ответа
  паникующим подписчиком, разрешается: паника не держит выход из сеанса), иначе 0; окно
  владельца — 0; `applicationShouldTerminate:` — `TerminateNow`, пока идёт завершение сеанса
  (`terminate_gate::session_ending`), иначе Cancel; `windowShouldClose:` NO; reopen и
  `didFinishLaunching` false; диапазоны `NSTextInputClient` `{NSNotFound, 0}`; COM —
  `E_UNEXPECTED`; перечисления — FALSE; делегаты WinRT — `Ok(())`; JNI — `T::default()`. Шесть
  входов — константа (тело — литерал или путь, код не выполняется): `GiveFeedback`,
  `accepts_first_responder`, `is_opaque`, `accepts_touch_events`, `character_index_for_point`,
  `takes_focus`. TSF: `TextServices::enter` возвращает гард `Depth`, `leave(entered)` его
  отпускает — глубина снимается и при панике мимо отравления документа (раньше оставалась 1, и
  каждая следующая операция хоста вставала в очередь навсегда). Тела GCD — `move`: захваты
  уничтожаются внутри границы. Wake pump: тик с поколением (`Tick`), страж
  `StopUnlessFinished(Option<Tick>)` снимает `pending_at` через `compare_exchange`, только если
  хук не запланировал новый тик до паники (старое пробуждение не стирает новое обязательство).
  Адаптеры (8, по виду объекта ОС): `com_method` (drag, input scope, TSF без состояния),
  `com_hresult` (drag), `TsfStore::document_method`, `Handler::com_method` (DirectManipulation),
  `jni_native`, `Self::scene_event` (сцены UIKit). Гейт — `cargo xtask panic-boundary`
  (`tools/xtask/src/panic_boundary{.rs,/scan.rs,/fixture.rs,/tests.rs}`); дерево модулей, cfg с
  тремя значениями, атрибуты и источники `Disk`/`Memory` — общий `tools/xtask/src/source_tree.rs`,
  им же читает `globals`. Код под cfg, ложным в любой сборке (`test`), не читается. Точки входа:
  fn с чужим ABI — свободная, метод `impl` или умолчание трейта (процедуры, колбэки
  перечисления, JNI, методы `ClassBuilder`, NDK); метод `define_class!` с
  `#[unsafe(method(…))]`/`method_id`; метод `impl I…_Impl for …` (vtable COM); методы
  `ActionHandler`/`ActivationHandler`/`DeactivationHandler` (AccessKit зовёт их из своих входов
  UIA, NSAccessibility, AT-SPI); замыкание, отданное ОС, — любой литерал замыкания в вызове
  функции или конструктора крейта привязок ОС (`windows`, `objc2*`, `block2`, `dispatch*`, `jni`,
  `ndk`, `android_activity`, …) по пути или по импортированному имени, так что незнакомая функция
  ОС не проходит молча; конструкторы блоков и делегатов по имени (14 типов) — где их прячет
  glob-импорт; методы GCD по имени (`exec_async`, `exec_sync`, `exec_after`, `after`,
  `barrier_async`, `barrier_sync`), их тело обязано быть `move`; синхронный `autoreleasepool` —
  не вход. Правило: тело — одно выражение: константа, вызов `os_entry` (`…panic_boundary::os_entry`
  или импортированный оттуда) или адаптера — свободной функции того же файла, определённой один
  раз на уровне модуля и не затенённой импортом (`name(…)`), либо метода того же файла,
  определённого один раз на `Self` или названном типе (`Self::name(…)`, `Type::name(…)`); вызов
  `self.name(…)` — не адаптер (тип получателя гейту не виден). Аргументы вычисляются до границы,
  поэтому каждый — литерал, путь, замыкание, кортеж, ссылка, поле, приведение, скобки от них или
  `AfterPanic::Answer(…)` от такого. Идентичность: `fn os_entry` один, в `owner::panic_boundary`;
  другой элемент, модуль или импорт под именем `os_entry`/`panic_boundary` — находка. Тело
  `define_class!` разбирается (атрибуты класса до `struct Name;` отброшены: `#[ivars = Weak<T>]`
  не синтаксис атрибута Rust); в токенах других макросов определение `extern fn`, конструктор
  блока или делегата и метод GCD — находка. Вид находки — `Problem::{Unguarded, Argument,
  Identity, Unread}`. Allowlist нет. Пределы (в доке гейта и ARCHITECTURE.md): методы ОС
  узнаются по имени — новый метод, берущий замыкание, вносится в список GCD, конструктор за
  glob-импортом — в список типов колбэков; захваты блока или делегата живут, пока ОС его держит, и
  уничтожаются вне любого вызова, поэтому это значения без пользовательского `Drop`. Вне охвата по
  виду: замыкания `wasm_bindgen` (wasm32 всегда `panic=abort`), процедуры winit и
  android-activity (своя граница: winit поднимает панику в `run_app`, android-activity —
  `abort_on_panic`), ABI плагинов `flui-hot-reload` (`flui_scene_build`, `flui_app_build` из
  `scene_plugin!`/`app_plugin!`: их зовёт процесс-хост в плагине, вне `flui-native`; граница там
  нужна та же, но своя — крейт строится на `flui-sdk`, отдельной задачей), тестовый код (паника
  заглушки роняет процесс теста). Самотест — 30 посаженных находок, по одной на правило и вид
  (среди них: метод с чужим ABI в `impl` и умолчание трейта, адаптер, затенённый импортом,
  `self.name`, метод чужого типа, вычисление внутри `AfterPanic::Answer`, тело `exec_after` без
  `move`, блок и метод GCD в токенах макроса, незнакомая функция ОС с замыканием), и тихие случаи
  (`#[cfg(test)]`, блок `extern {}`, тип указателя, константное тело, `autoreleasepool`,
  адаптеры); таблица `panic_boundary_contract` — 13 строк. Подключён в `checks`
  (`panic-boundary --self-test`, `panic-boundary`), значит и в job `checks` CI. Сверка «210 мест»:
  это число строк с `panic_boundary|catch_unwind` в `crates/flui-platform/src` на `b4ff641e5`
  (импорты, тесты, внутренние гашения), а не мест входа. Перепись гейта — 169 точек входа: 57
  extern fn, 26 методов Objective-C, 56 методов COM, 7 обработчиков AccessKit, 23 замыкания; 163 —
  через `os_entry` (67 из них — через 8 адаптеров), 6 — константы; 205 файлов. До исправления 63
  гасили всё тело, 12 — часть, 94 — ничего. Без границы были: Win32 — процедура окна-владельца
  буфера обмена, `enum_proc` мониторов, `first_visible`, `low_memory_fired`; COM — 12 методов OLE
  drag (`GetDataHere`, `GetCanonicalFormatEtc`, `SetData`, три `*Advise`, четыре
  `IEnumFORMATETC`, `QueryContinueDrag`, `GiveFeedback`), `OnViewportUpdated`, 8 методов TSF с
  постоянным ответом (без проверки отравления — ответ не читает документ) и 5 методов input scope;
  AccessKit — 7 (Windows 2, macOS 2, Linux 3); AppKit — 41 (14 методов вида ввода, 11
  `NSTextInputClient`, 15 делегата окна, `takes_focus`); GCD macOS — тик wake pump (хук дедлайна
  поднимал панику через `or_resume` в трамплин `extern "C"` — abort) и пробуждение повтора
  предпочтений; UIKit — 14 (`configuration`, `init` делегата сцены, hover, четыре touches,
  display link, два метода контроллера, четыре блока Core Haptics). Частично: 6 входов iOS через
  `with_platform` (заём `DELEGATE_STATE` вне гашения), 4 метода вида iOS (pin и super вне
  гашения), `OnContentUpdated` (`GetContentTransform` до границы), status bar iOS. Ещё: `dealloc`
  вида macOS гасит разбор контекста отдельно, и super `dealloc` идёт и после его паники;
  завершение перезапуска Core Haptics снимает `restart_in_flight` до лога; `exec_async_guarded`
  и исполнитель iOS логируют через `os_entry`; `LoopControl::after` стал `run_after` (имя `after`
  — метод GCD). Тесты: `owner::panic_boundary::tests::an_os_entry_logs_the_first_failure_then_answers`,
  `an_os_entry_answers_from_its_arguments_and_logs_them` (без аргументов в логе — падает),
  `a_contained_callback_is_logged_with_its_text` (без текста — падает),
  `a_panicking_log_stays_inside_the_entry` (лог вне гашения — падает);
  `owner::windows::tests::a_failing_subscriber_is_logged_and_a_session_end_still_granted` (с
  прежним общим гашением — ответ 0, выход из сеанса отклонён); строка TSF
  `a_tsf_call_failing_past_its_poisoning_leaves_no_depth` (с ручными enter/leave — глубина 1);
  `accessibility::windows::tests::a_panicking_listener_stays_inside_its_handler` и
  `accessibility::linux::…` (с откатом обработчика — «a listener's panic unwound out of its
  handler»); только macOS — `owner::apple::macos::wake_pump::tests::a_tick_whose_hook_panics_leaves_no_tick_queued`,
  `a_tick_whose_hook_rearms_then_panics_keeps_the_newer_tick`,
  `owner::apple::terminate_gate::tests::a_session_end_is_under_way_from_its_announcement_to_its_answer`
  (здесь модули Apple не собираются в тестах: `psm` требует `cc`); xtask `panic_boundary_contract`
  и самотест, мутации ветвей гейта (разрешение адаптера мимо импорта, `move`, `Answer`, ABI в
  `impl`) — «MISSED». Исключения `module-dag` — 33, `globals` — 56, как было. Проверки на Windows:
  `cargo fmt`; clippy `-D warnings` `-p flui-native -p flui-app --all-targets` и с
  `flui-native/winit-backend`, `-p xtask --all-targets`; nextest `-p flui-native -p flui-app` — 317
  (2 пропущены), с `winit-backend` — 350 (2), `-p xtask` — 46; clippy Android `--all-targets`
  (`-p flui-native -p flui-app`), macOS lib `-p flui-native` и с `winit-backend` (`flui-app` под
  macOS требует `cc`: `psm` через `flui-rendering`), iOS lib (`DOCS_RS=1`), wasm32 lib;
  `wasm-check`, `wasm-test` — 51; `checks` целиком: `module-dag` (295 модулей, 325 рёбер, 33
  исключения) и самотест (54), `globals` (56) и самотест (15), `panic-boundary` (169) и самотест
  (30), `file-length`, `markers`, `testing`, `docs-paths`, `workspace`, `reach`, `changelog`;
  `doc-strict`; rustdoc `--document-private-items -D warnings`. WSL Ubuntu 24.04
  (`WAYLAND_DISPLAY`, `XDG_SESSION_TYPE` сняты): clippy с `winit-backend` и без; nextest
  `FLUI_HEADLESS=1` — 335: 334, падает `x11_window_takes_commands_and_publishes_what_the_server_holds`
  на X-сервере WSLg (как на `origin/main`, см. LY34; в прогоне до слияния LY35 однажды упал и
  `x11_window_asked_for_a_mode_before_its_first_frame_opens_in_that_mode`, отдельно он прошёл 5
  из 5); `--all-features` под xvfb — 291 из 291; `accessibility::linux` и `owner::panic_boundary`
  с `winit-backend` — 7; rustdoc с `winit-backend` и без. Не запускалось: Mac и iPhone — 41 вход
  AppKit, 14 UIKit и блоки только собраны (библиотека), тесты wake pump и terminate gate не
  собирались, в очереди vanyastaff/flui#1840 (пункт 4 `docs/gaps.md`);
  `cross-typecheck` (нет `zig`; job в CI); устройство Android (JNI-методы меняют только имя входа
  в логе и ответ по умолчанию). Браузер: проверять нечего — Web входов не имеет.
- [x] **LY35** — ветка `platform/duplicates` (vanyastaff/flui#1873), начата на вершине LY34
  `dbbb8d7d9`, перенесена на `main` @ `fc8f8ebaa` после его слияния (дерево то же), после ревью —
  слияние `main` @ `ccda248ee` (LY88). Слияния дублей внутри моделей фич; `flui-app` не тронут.
  `changelog.d/platform-duplicates.md`: `### Added` — `RefreshPeriod::from_secs_f64` в контракте,
  `### Changed` — ось колеса без движения `+0.0` на каждом хосте.
  Режим окна. Одно правило — `window::state::mode_of(ModeFlags)` (свёрнуто, затем полноэкранно,
  затем развёрнуто); `ModeFlags { minimized, fullscreen, maximized }` — что оконная система говорит
  об окне. Ушли `mode_of` AppKit (`window/apple/geometry.rs`), `window_mode` X11
  (`window/linux/x11.rs`) и `NativeMode::public()` Win32; каждый читает свой отчёт во флаги, и только:
  winit — три запроса как есть; X11 — `mode::mode_flags(wm_state, net)` (`WM_STATE` решает
  свёрнутость, `_NET_WM_STATE_HIDDEN` — только без него, развёрнуто — по обеим осям); AppKit —
  `WindowFlags::mode_flags()` (зум — только у окна, меняющего размер); Win32 — `NativeMode::flags()`.
  `NativeMode` переехал из `proc.rs` в чистую половину `window::windows::mode` (встроенный модуль
  `mod.rs`, `forbid(unsafe_code)`, вне Windows — для тестов, с `allow(dead_code, reason)`).
  Win32 держит один `enum`, но свёрнутое из полноэкранного окно несёт два факта, и какой сильнее —
  это и есть один порядок `mode_of`; прямое отображение `enum` в `WindowMode` повторило бы его, поэтому
  `flags()` (это сказано в его документации). Расхождение копий: AppKit ставил полноэкранность выше
  свёрнутости (строка `FMZR` → `Fullscreen` в `flags_become_a_mode`), остальные — свёрнутость выше.
  Порядок один — у `mode_of`; закреплённое поведение AppKit сохранено его чтением флагов:
  `isMiniaturized` считается только вне полноэкранного режима (AppKit не сворачивает полноэкранное окно,
  а сначала выходит из него — так строит запросы и `steps_to`); строка `FMZR` не правлена. winit на
  macOS для тех же фактов давал `Minimized` и даёт; AppKit такого сочетания не сообщает.
  Видимость. Одно правило — `window::visibility::ReportedVisibility` (`report`, и `report_in` для
  окна, держащего его в `Cell`): рантайм слышит ответ, только когда он меняется. Его держат окна winit
  (`Mutex`; ответ читается под замком, как раньше), Win32 (`report_in`), AppKit (в состоянии окна) и
  Web (`report_in`; проверка приёма — до правила, как раньше); ушли `visibility_edge` и три сравнения
  `replace(..) != visible`. Правила одной платформы ушли из модели в её файл: `WM_SIZE`,
  `WM_SHOWWINDOW` и стиль создания — `window::windows::surface` (`initial_visibility`,
  `size_visibility`, `show_window_visibility`, `SIZE_*`, `WS_VISIBLE_STYLE_BIT`; три теста перенесены
  с именами), бит `NSWindowOcclusionStateVisible` — его двойник `window::apple::geometry::surface`
  (`occlusion_state_is_visible`, тест `appkit_occlusion_reports_the_visible_bit`). Android
  (`publish_activity_fact`, факт lifecycle) и headless (`simulate_visibility`, двойник) фильтра не
  имели и не получили.
  Период обновления. «Один конструктор … с одной проверкой» строки — конструкторы контракта:
  `RefreshPeriod::{from_hz, from_ratio, from_millihertz}` и новый `from_secs_f64` для измеренного
  интервала (общий код ушёл вниз; тест `measured_interval_is_the_period` в
  `refresh_value_classification_table`); одна их проверка отвергает не конечное, не большее нуля и
  вне 1 мс…1 с. Каждый источник отдаёт отчёт своей ОС прямо конструктору своей единицы. Один издатель —
  `display::source::MeasuredPeriod`: `offer` публикует измерение, только когда оно выходит из полосы
  (`outside_band`, доля опубликованного; первое — всегда), `departs` спрашивает без публикации,
  `forget`; публиковать в обход полосы нечем. Ушли `display::android::sample::refresh_period` (вызов
  на месте), модуль `display::apple::period` (`core_graphics_refresh_period`),
  `refresh_period_for_screen` (macOS читает `refresh_rate_of_screen -> Option<f64>` и отдаёт её
  `from_hz`), `refresh_period` Wayland (вызов на месте) и
  `display::windows::sample::{rational_refresh_period, frequency_period}` — их место заняло одно
  правило Windows, `display_period(target_rate, mode_frequency)` (рациональная частота пути, иначе
  `dmDisplayFrequency`, читаемая только тогда; её 0 и 1 — «по умолчанию оборудования»). Ссылка
  дисплея iOS (`LinkPeriod::observe`) и оценка браузера (`RefreshEstimator`) строят период через
  `from_secs_f64` и публикуют через `offer`; полосы прежние (5 % и 3 %), согласие последних интервалов
  и устойчивость оценки судятся тем же `outside_band`. Отступления от строки:
  `ScreenPeriods::period_of` период не строит, а выбирает, чей опубликованный период несёт экран
  (первого живого окна), — это учёт iOS, он остался, а строитель `link_rate` (`LinkPeriod::observe`)
  переведён; `refresh_estimate` остался как оценщик (усечённое среднее, выбросы), ушли его построение
  периода и полоса. headless (`DisplaySpec::with_refresh_period(Duration)`) остался на
  `TryFrom<Duration>`: это период, заданный тестом, не отчёт ОС. Расхождений копии не имели — все шли
  через конструкторы контракта; полоса браузера считается теперь в секундах, а не в миллисекундах
  (округление расходится лишь на самой границе, тесты её не задевают); 1 Гц из `dmDisplayFrequency`
  неизвестен только у Windows, сохранено.
  `Location` клавиши. Одна таблица — `input::keys::LOCATIONS`, строки — `LocationRow { location,
  dom_number, codes }`: номер, которым DOM называет место, и коды клавиш этого места. `location_of_number`
  (`pub(crate)`, только DOM) читает веб, `location_for_code` — Win32 и AppKit; посторонний `WM_CHAR`
  Win32 — именованная константа `CHARACTER_WITHOUT_A_KEY` вместо литерала. Адаптер keyboard-types
  (winit, Android) сопоставляет `keyboard_types::Location` по вариантам, исчерпывающим `match`
  (вариант, добавленный keyboard-types, не соберётся, пока его не назовут), и таблица `mapping`
  сверяет его со строками. Ушли `match` веба (4) и литерал Win32; `location_for_code` стал поиском
  по таблице. Расхождений нет: неизвестный номер DOM — `Standard`, как было.
  Шаг колеса. Одно правило — `input::scroll::wheel_step(unit, x: WheelAmount, y: WheelAmount) ->
  Result<ScrollDelta, InputValueError>` с `WheelAmount::{offset, content}` (`WheelDirection`):
  количества платформы в выбранной ею единице, каждая ось поворачивается в направление FLUI. Ушли
  `from_win32_wheel`, `from_win32_hwheel` и `from_web`; потребители — Win32 (`ContactRegistry::wheel`
  с `WHEEL_DETENT`), Web (чистая половина `input::web::wheel::step`: `deltaMode` → единица), winit
  (`scroll_step`, был `wheel_step`; `ScrollStep { delta, precision }`), Android (`AxisPolicy::delta`)
  и AppKit (не в строке, то же правило; шаг переехал в чистую половину `input::apple::wheel::step` по
  R4.2). Расхождения: (1) неконечное значение — winit и AppKit давали нулевой шаг в единице события,
  Web и Android отбрасывали событие, у Win32 его не бывает (`i16`): правило отвечает ошибкой, и каждый
  потребитель решает, как раньше; (2) ось без движения — Win32 давал `+0.0`, Web пропускал знак нуля
  из `WheelEvent` (`deltaX: -0` → `-0.0`), winit, AppKit, Android (`-x` от нуля) и вертикальное колесо
  Win32 с нулевым расстоянием давали `-0.0`: правило поворачивает вычитанием из нуля, а оставленное
  значение складывает с нулём, так что ось без движения везде `+0.0` (так закрепляет запись
  `win32_sent_mouse_and_wheel`: `Detents(0, -1)`, `Detents(1, 0)`); прочие значения те же бит в бит.
  Это видно потребителю (`signum()` такой оси был `-1.0`), отсюда `### Changed`.
  Тесты — по одной таблице `mapping` на правило, в модели, и каждая читает строки через пути
  потребителей, что собираются на хосте:
  `window::state::mapping::every_window_system_decides_the_mode_alike` (8 строк × флаги winit,
  свойства X11, сырые флаги AppKit с его режимом в строке, где свёрнутое окно ещё и полноэкранно, режим
  Win32), `window::visibility::mapping::every_change_is_reported_once` (правила Win32 `WM_SIZE` и
  `WM_SHOWWINDOW` и бит перекрытия AppKit через правило; winit и Web читают ответ через FFI и
  сообщают его как есть), `display::source::mapping::every_display_source_reads_a_cadence_alike`
  (11 каденций × Android через `RawDisplay::info`, Windows через `display_period`, Wayland через
  `WaylandOutputs::apply` и `views`, RandR через `RandrState::views`, ссылка дисплея через
  `LinkPeriod::observe`, браузер через `RefreshEstimator`; у macOS чтение — FFI, столбца нет),
  `a_measured_period_is_published_outside_its_band`, `macos_shim_keeps_the_period_it_reports_today`
  (перенесён из удалённого `display::apple::period` с именем, его цитирует ARCHITECTURE.md),
  `input::keys::mapping::every_backend_reads_a_key_at_one_location` (7 клавиш × скан-код Win32,
  код AppKit, номер DOM, адаптер keyboard-types при `winit-backend`),
  `input::scroll::mapping::every_backend_steps_a_turn_alike` (7 поворотов, среди них ось `-0` DOM, ×
  Win32, DOM, Android, AppKit, winit при `winit-backend`; бит в бит) и
  `a_turn_that_is_not_finite_steps_nothing`. В них слиты `window_mode_precedence` и строки
  `android_refresh_value_table`; `windows_zero_or_one_hz_is_unknown` и `rational_refresh_converts_exactly`
  идут через `display_period` (добавлен откат пути без частоты на частоту режима). Мутации — таблица
  падает, когда потребитель расходится: `NativeMode::flags` без «свёрнуто из полноэкранного» — «Win32:
  minimized from fullscreen»; X11 без полноэкранности при развёрнутости — «X11: fullscreen that kept its
  maximized flag»; AppKit, считающий `isMiniaturized` и в полноэкранном режиме, — «AppKit: minimized
  from fullscreen»; `report`, всегда отвечающий `Some`, — «an unchanged visible window»; инвертированный
  бит AppKit — «AppKit: an unchanged visible window»; `size_visibility` без сворачивания — «Win32
  WM_SIZE: hidden, then shown»; Wayland и RandR, читающие миллигерцы как герцы, — «Wayland: 60 Hz is
  None», «RandR: 60 Hz is None»; `LinkPeriod`, строящий период как герцы, — «display link: 60 Hz is
  None»; `offer` без полосы — «inside the band»; Android и winit с прежним `-x` — «a notch down stepped
  … x: -0.0»; AppKit, не поворачивающий ось, — «AppKit: two notches right»; правило без `+ 0.0` — «DOM: a
  still axis reported as negative zero»; адаптер, не читающий место, — «winit and Android: the left
  Shift». Мутация `size_visibility`, отвечающая на каждое восстановление, таблицу не роняет: сообщения
  те же, повтор снимает правило (её держит `a_win32_size_message_reports_only_the_minimize_edges`).
  Потребители, чей путь — только FFI (чтение CoreGraphics, клавиатура и колесо Web, ответ видимости
  winit и Web), зовут правило или конструктор напрямую. Записи нативных трасс и conformance не тронуты
  и проходят.
  Ревью (vanyastaff/flui#1873): `Refresh` удалён (он лишь пересылал конструкторам контракта и держал
  вторую копию их проверки), `from_secs_f64` добавлен в контракт; `MeasuredPeriod::offer` вместо
  безусловного `publish`, `outside_band` — одна формула полосы вместо `differs` оценки; столбцы
  CoreGraphics и миллигерц больше не зовут конструктор сами — Wayland и RandR идут через модели;
  таблица видимости идёт через читателей Win32 и AppKit; шаг AppKit — в чистой половине и в таблице;
  исчерпывающий `match` keyboard-types вернулся, `LOCATIONS` — именованные строки, `location_of_number`
  — `pub(crate)`; `+ 0.0` для оставленной оси и строка `-0` DOM; таблица режима кормит AppKit сырыми
  флагами; `report_in` — одна последовательность `Cell` для Win32 и Web; бит перекрытия AppKit —
  `apple::geometry::surface`; константа `CHARACTER_WITHOUT_A_KEY`. Слияние `main` с LY88: конфликт
  только в этом разделе (обе ветки дописали запись), признака `a11y` в изменениях ветки не было.
  `module-dag`: исключений 33, новых нет (`window::apple::ios` → `display::apple` остаётся: окно iOS
  называет `LinkPeriod` и `change_screen`); `globals` — 56, без изменений. Документы: ARCHITECTURE.md
  ядра называет одно правило каждой пары, конструкторы контракта и что читает каждая таблица; раздел
  периода macOS — `refresh_rate_of_screen` и `RefreshPeriod::from_hz`.
  Проверки после ревью и слияния (Windows): `cargo fmt`; clippy `-D warnings` `-p flui-native -p
  flui-app --all-targets` с `winit-backend` и без, `-p flui-platform --all-targets`; nextest
  `-p flui-native -p flui-app -p flui-platform` — 349 (2 пропущены), `-p flui-native -p flui-app` с
  `winit-backend` — 343 (2); clippy Android `--all-targets`, macOS lib (и с `winit-backend`), iOS lib
  (`DOCS_RS=1`), wasm32 lib; `wasm-check`, `wasm-test` — 51 (45 у `flui-native`); `module-dag`
  (295 модулей, 10 фич с 77 файлами платформ, 3 base, 324 ребра, 33 исключения) и самотест (54),
  `globals` (56) и самотест (15), `file-length`, `markers`, `testing`, `docs-paths`, `doc-strict`,
  `workspace` (28 крейтов, 229 рёбер), `reach` (68 корневых сборок), `checks` (с `android-dex` и taplo).
  WSL Ubuntu 24.04 (`WAYLAND_DISPLAY` и `XDG_SESSION_TYPE` сняты): clippy `-p flui-native -p flui-app
  -p flui-platform --all-targets` и `-p flui-native -p flui-app` с `winit-backend`; nextest
  `FLUI_HEADLESS=1` под xvfb с `winit-backend` `-p flui-native -p flui-app` — 329 (1 пропущен),
  `--all-features` под xvfb (команда CI) — 285, без признаков (`FLUI_HEADLESS=1`) — 204; rustdoc `-D
  warnings --document-private-items` с `winit-backend` и без. Galaxy S21, 2026-10-22, Android 15, два
  коротких прогона пробы `flui create --template counter` (до ревью и после слияния, чтение частоты
  Android перешло на `from_hz` на месте); лог обратного вызова частоты и ревизий снимка временно поднят
  до `info` (правка не закоммичена): обратный вызов сообщает 8333333 ↔ 16666666 нс (адаптивная частота
  при простое и касаниях), ревизии снимка 1–7 и 1–5 — 1080×2400 с периодом попеременно 8333 и
  16666 мкс; Back — «Destroy — shutting down», «Android platform event loop finished», предупреждения
  «outlived its thread» нет; `FATAL`, паник и ANR нет; настройки устройства не менялись, пробы удалены.
  Браузер — у координатора, проба `inputprobe` вне репозитория: последняя клавиша и её место, шаг
  колеса, видимость и lifecycle. Не запускалось: `cross-typecheck` (нет `zig`; job в CI),
  `web_browser` (нет ChromeDriver); Mac и iPhone — в очереди vanyastaff/flui#1840 (чтение флагов и бита
  перекрытия AppKit, шаг колеса AppKit, чтение CoreGraphics, `LinkPeriod` iOS) — только сборкой.
- [x] **LY37** — ветка `platform/escape-hatch`, начата на вершине LY36 `0f1b0ac06`, перенесена на
  `main` @ `8ff5a40d0` (LY36, дерево то же), затем на `main` @ `7eb6bdb7a` (#1899–#1907; конфликты
  только в таблицах истории `docs/platform-parity.md` и `docs/gaps.md`, обе строки сохранены).
  Фасад: фича `native` (выключена по умолчанию, Evolving, ADR-0089 п. 5) и `flui::native` —
  `HostWindow`, `flui::native::windows` под `cfg(windows)` и `flui::native::macos` под
  `cfg(target_os = "macos")`; фасад не зависит от ядра (`allowed-dependents` ядра — только
  `flui-app`), поэтому путь идёт через `flui_app::native`, а в ядре расширения ОС ушли из корня в
  `flui_native::os::{windows, macos}` (модуль `os` — `crate-level` в таблице `module-dag`).
  Без фичи модуля нет: doctest фасада `compile_fail,E0432` на `use flui::native::HostWindow`, с
  фичей — тот же doctest компилируется. `facade-combos` — 10 сочетаний (`--features native`).
  Слот опций одной ОС (ADR-0254 п. 8), форма: `WindowOptions::os: OsWindowOptions`,
  `#[non_exhaustive] enum { None (по умолчанию), Windows(Win32WindowOptions) }`, `with_os(impl
  Into<OsWindowOptions>)`; типы — в контракте рядом со слотом, чистые данные на всех целях (кросс-
  платформенный код задаёт их без своего `cfg`); читает только хост своей ОС (`_ =>` по умолчанию),
  чужой хост (headless, winit, другая ОС) открывает окно без них, и это видно: расширение этой ОС
  на таком окне отвечает `None`. Первое значение — `Win32WindowOptions { corner_preference:
  WindowsCornerPreference, theme: WindowsTheme }`, по умолчанию то, что окно Win32 получало
  жёстко (углы `Round`, тема пользователя): углы и тема — то, что хост задавал при создании без
  выбора; Mica не тронута (материал фона — LY43). `WindowCornerPreference` переехал из ядра в
  контракт как `WindowsCornerPreference`, `WindowsTheme` — тоже, без DWM-преобразований (они —
  приватные функции хоста). Слот достижим из приложения: `AppConfig::os_window_options` и
  `with_os_window_options`, `From<&AppConfig> for WindowOptions` его переносит. Решение записано в
  ARCHITECTURE.md контракта («One operating system's creation options are one typed value»).
  Материал macOS (бывший Liquid Glass) вернулся из git методами вида:
  `AppKitWindow::set_backdrop(MacOSBackdrop) -> Result<(), MacOSBackdropError>` и
  `clear_backdrop`. Решения сверх строки: (1) имя по тому, что рисуется: код до LY36 ставил
  `NSVisualEffectView` с его материалами и называл это «приближением до API Liquid Glass»;
  настоящий Liquid Glass — `NSGlassEffectView` (macOS 26, есть в `objc2-app-kit` 0.3.2), ему нужен
  шлюз версии macOS, которого в `version/` нет, и прогон на Mac — следующая работа, не эта строка;
  (2) убрано то, что принималось и не делалось: `blur_radius`, `tint`, `vibrancy` (у
  `NSVisualEffectView` их нет), `is_available()` (запуск `sw_vers`, к тому же ложно отвечал «нет» на
  macOS ниже 26, где материалы работают), дубль `ControlCenter` (тот же `hudWindow`, что
  `Prominent`), `BlendingMode::WithinWindow` (у фона окна позади ничего нет) и прозрачный заголовок
  (`FullSizeContentView` менял область содержимого, которую снимок окна не публиковал, а
  `clear_liquid_glass` его не снимал); (3) метод делает работу или говорит, почему нет:
  `Closed`, `NotOnTheMainLane` (вид AppKit живёт на главном потоке), `NoEffectView` (`init`
  вернул nil); значения материала, смешивания и состояния — типы `objc2-app-kit`
  (`NSVisualEffectMaterial` и др., `NSInteger`), а не `usize` старого кода, который не прошёл бы
  проверку кодировок objc2 в debug; +1 эффект-вида и FLUI-вида держат `Retained`. Файл
  `window/apple/liquid_glass.rs` не восстановлен: перечисление, ошибка и два метода — в
  `window/apple/macos/ext.rs` рядом с видом. Вход ОС (`os_entry`) не добавлен: методы зовут AppKit,
  а не принимают вызовы от него; `panic-boundary` — те же 169 входов.
  Приложению `dyn HostWindow` сегодня не отдаёт ни один путь фасада (окно получают раннеры
  `flui-app`): `flui::native` достижим из кода, открывающего окна через ядро (тесты, doctest'ы);
  метод доступа к окну хоста из `WindowHandle` или `PresentationAddress` — следующая работа,
  естественно — после LY44, которая переделывает владение окнами в раннерах.
  Документы: строка в `docs/extending.md`, `docs/platform-parity.md` (материал фона macOS ❌ → 🟡,
  macOS 28% → 29%, общий ~30%; заголовок и история), `docs/gaps.md` (записи 4 и 20, заголовок и
  история), `docs/crates.md`, README и документация фасада (таблица фич), ARCHITECTURE.md ядра;
  `changelog.d/platform-escape-hatch.md` (`### Added`, `### Changed`). Исключения `module-dag` не
  менялись: 33.
  Тесты: `tests/os_extensions.rs` ядра — новый случай
  `a_win32_window_opens_with_its_options_and_another_host_without_them` (окна headless и winit с
  опциями Win32 открываются, вида Win32 нет; окна Win32 с `DoNotRound`/`Dark` и
  `RoundSmall`/`Light` читают углы и тему из DWM): без чтения опций хостом падает на `left: Round,
  right: DoNotRound`, без темы из опций — на `left: Dark, right: Light`;
  `an_appkit_backdrop_goes_behind_the_content_and_comes_away` — только сборкой (Mac, #1840).
  Фасад — `tests/surface.rs` (`required-features = ["native"]`): каждый элемент `flui::native` по
  пути фасада, `native_extensions_answer_none_on_a_foreign_host` (имя из подтверждения ADR-0254)
  и `a_win32_window_narrows_through_the_facade` — настоящее окно Win32, суженное через
  `flui::native::windows`, читает опции создания и меняет углы, тему и Mica Alt.
  `flui-app` — строка `one_systems_options_are_kept` в `config_to_options_keeps_every_field`: без
  переноса слота падает «failing cases: ["one_systems_options_are_kept"]». Doctest'ы:
  `WindowsWindowExt`, `MacOSWindowExt` (на окне headless — `None`), `OsWindowOptions`.
  Гейт API-замыкания: фасад не получил таблицы `targets` — гейт не идёт за реэкспортом элемента
  workspace, так что сборка фасада с `native` на Windows и macOS проверяла бы только сами
  реэкспорты, а содержимое `os::*` проверяет гейт ядра на шести целях; разовый прогон с временной
  таблицей (`flui` на Windows с `native`) и `-p flui-native -p flui-platform -p flui-protocol` на
  `1.99.0` с `RUSTC_BOOTSTRAP=1` и `RUSTDOCFLAGS=-D warnings` — 4 пакета чисто.
  Проверки (Windows): `cargo fmt`; clippy `-D warnings --all-targets` всего workspace
  (`--affected`: изменился корневой `Cargo.toml`), `-p flui-native -p flui-app` с
  `winit-backend`, `-p flui --features native`; nextest `-p flui-platform -p flui-native -p
  flui-app` — 363 (2 пропущены): 362 и `trybuild_ui::ui_tests`, с `winit-backend` — 359: 358 и он
  же; его единственное расхождение — фикстура только для Windows
  `tests/ui/windows/owner_thread_token_has_no_default.rs`, `.stderr` которой ждёт
  `std::default::Default`, а rustc 1.99 печатает `Default` (файлов этой строки не касается, на
  `main` так же; CI её не собирает); `os_extensions` — 2; фасад с `native` — 9; doctest'ы
  `flui-native` (4 + 3), `flui-platform` (40 + 9), фасада без `native` (3 + 5, `compile_fail`) и с
  ним (4 + 4); clippy для macOS lib `-p flui-platform -p flui-native` и `flui-native` с
  `winit-backend`, iOS lib (`DOCS_RS=1`), wasm32 lib, Android `-p flui-native -p flui-app
  --all-targets`; `flui-app` и фасад под Apple здесь не собираются (`psm` требует C-компилятор
  цели) — их держит `cross-typecheck`; `wasm-check`, `wasm-test` — 50; `checks` (`module-dag` —
  296 модулей, 33 исключения, и самотест; `globals` — 56; `panic-boundary` — 169 входов, без
  изменений; `workspace` — 28 крейтов, 229 рёбер; `reach` — 69 сборок; `file-length`, `markers`,
  `testing`, `docs-paths`, `changelog`), `doc-strict`, `facade-combos` — 10. WSL Ubuntu 24.04
  (`WAYLAND_DISPLAY` и `XDG_SESSION_TYPE` сняты): clippy `-p flui-platform -p flui-native -p
  flui-app`, с `winit-backend`, фасад с `native`; nextest `FLUI_HEADLESS=1` — 379: 378,
  `x11_window_takes_commands_and_publishes_what_the_server_holds` падает на X-сервере WSLg, как в
  LY29, LY34 и LY36; `--all-features` под xvfb — 297 из 297; rustdoc `-D warnings
  --document-private-items` без фич и с `winit-backend`, фасад с `native`. Устройства не нужны
  (Android не затронут). Окно Win32, суженное через `flui::native::windows`, — тест фасада
  `a_win32_window_narrows_through_the_facade` на этой машине (Windows 11, сборка 26300; DWM
  отдаёт углы и тему обратно). Не запускалось: `cross-typecheck` (нет `zig`; job в CI); Mac —
  `flui::native::macos`, материал фона и `an_appkit_backdrop_goes_behind_the_content_and_comes_away`
  в очереди vanyastaff/flui#1840.
  После ревью PR #1910 (коммиты поверх, без перебазирования); где сказанное выше расходится с
  этим абзацем, верен он. (1) Liquid Glass — настоящий: `MacOSBackdrop` — `Material(MacOSMaterial)`
  (материалы `NSVisualEffectView`, как были) или `Glass(LiquidGlass { style: GlassStyle, tint:
  Option<GlassTint>, corner_radius: Option<f64> })` — то, что применяет `NSGlassEffectView`
  (`style`, `tintColor`, `cornerRadius`; сообщения вручную, `NSInteger` и `CGFloat`). Класс ищется по
  имени при вызове (`AnyClass::get(c"NSGlassEffectView")`), без шлюза версии и `sw_vers`; до macOS 26
  — `MacOSBackdropError::NotOnThisSystem`. `GlassTint::new` принимает только конечные каналы в
  `0..=1`, радиус не конечный или отрицательный — `InvalidCornerRadius`. Код — в своём файле
  `window/apple/macos/backdrop.rs` (механизм одного объекта ОС, правило 1); `MainThreadMarker`
  берётся внутри маршрутизированного замыкания (`expect("BUG: …")`) и передаётся каждому шагу;
  содержимое окна, которое не FLUI-вид и не эффект/стекло, держащее его (`isKindOfClass:` и
  `superview`/`contentView`), — `ForeignContentView`, а не сообщение неизвестному селектору;
  `Retained::retain` вида FLUI — `expect("BUG: …")`, ошибкой `Closed` он больше не притворяется.
  Стекло каждый раз — новый вид (вход `None` — умолчание AppKit), материал поверх материала
  меняется на месте. (2) Долговременная проверка R3.9: гейт API-замыкания не идёт за реэкспортом
  элемента workspace (его «Limit», `tools/xtask/src/api_closure.rs`), так что таблица фасада
  проверяла бы только сами реэкспорты; вместо неё — fixtures trybuild фасада `tests/ui/native/`
  (`host_window_needs_native` — без фичи не компилируется, с ней компилируется;
  `windows_extension_needs_windows` и `macos_extension_needs_macos` — с фичей вне своей ОС не
  компилируются, на своей — компилируются; `.stderr` для Windows-фикстуры снят на Linux), doctest
  `compile_fail,E0432` (job `doc-test`, фасад без фич) и `flui/native` в `TEST_FEATURES` xtask
  (`test` и `test-trybuild` в CI идут с ним, так что `tests/surface.rs` и фикстуры исполняются);
  OS-типы в сигнатурах ядра держит гейт ядра на шести целях. (3) Путь приложения к окну:
  `flui_app::native::with_window(address, f) -> Option<R>` (фасад — `flui::native::with_window`):
  адрес — тот, к которому разрешается запрос окна или который называет запрос закрытия, как у
  `window_handle`; раннер в `presentation_window` (единственное место, где результат
  `open_window` становится окном UI runtime) кладёт `Weak<dyn HostWindow>` в `AppRuntime`, а поиск
  идёт от окна очереди команд адреса по адресу выделения (`Arc::as_ptr`), не по `WindowId`, который
  ОС переиспользует; `f` вызывается вне заимствований runtime, на чужом потоке, по пустому адресу
  и после ухода презентации — `None`. Виджету, у которого есть только `WindowHandle`, адрес пока
  не достаётся — это записано в документации `flui::native` (обработчик окна при открытии, LY44).
  (4) `flui_app::native` — за новой фичей `native` у `flui-app` (выключена — модуля и списка окон
  нет), фича фасада — `native = ["flui-app/native"]`. (5) Каждая команда и запрос `Win32Window`
  отвечает `Result<_, Win32WindowError>`: `Closed` и `NotOnTheOwnerThread` (сначала проверяется,
  что окно всё ещё владеет HWND и вызов на его потоке, через `with_window_context_checked`, так что
  чужой HWND с тем же значением не тронут), `Refused { request, hresult }` (в т. ч. углы на Windows
  10), `Unrecognized { request, value }`, `NotOnThisHost` (значение контракта новее хоста);
  переименованы `set_snap_layouts`, `snap_layouts`, `set_blur_behind`, `dark_mode`; удалён дубль
  `set_caption_color`. (6) Слот — структура: `WindowOptions::os_options: OsWindowOptions { windows:
  Option<Win32WindowOptions> }` (`with_os_options`, `OsWindowOptions::with_windows`), то же имя в
  `AppConfig` (`os_options`, `with_os_options`); опции двух ОС больше не вытесняют друг друга.
  (7) Типы — в публичном модуле контракта `flui_platform::os_options`, не в корне; все
  `#[non_exhaustive]`. ADR-0089 даёт уровень крейту, не модулю (§1: обещание следует за элементом),
  так что они Stable; выбрана наименее обязывающая форма — `#[non_exhaustive]`, построение от
  `Default` и `with_*`, никаких преобразований в значения ОС; это записано в документации модуля и
  в ARCHITECTURE.md контракта. (8) В `backdrop.rs` Win32 атрибут DWM — константа `Attribute<T>`,
  связывающая ординал крейта `windows` (`DWMWA_*`) с типом значения (обязательство `unsafe`
  выполняется в одном месте, при объявлении); углы и материал — по одной таблице
  (`DWMWCP_*`, `DWMSBT_*`), читаемой в обе стороны; модуль `dwm_attributes` и ручные числа ушли.
  `WindowsBackdrop` — `#[non_exhaustive]`, `Auto` добавлен, `Tabbed` (тот же `DWMSBT_TABBEDWINDOW`,
  что `MicaAlt`) удалён. (9) Тесты: `os_extensions` — `a_win32_window_opens_with_its_options`
  (повтор «чужого хоста» удалён), новый `a_win32_view_refuses_another_thread_and_a_closed_window`
  (с обходом проверки HWND падает на `left: Ok(()), right: Err(NotOnTheOwnerThread)`),
  `an_appkit_backdrop_goes_behind_the_content_and_comes_away` проверяет класс содержимого,
  материал (`material`), стиль стекла и `contentView`, что вид FLUI — подвид эффекта и первый
  ответчик, и что снятие возвращает его содержимым (только сборкой, #1840); `flui-app` —
  `with_window_reaches_the_host_window_of_its_presentation_and_no_other` (без регистрации окна в
  `presentation_window` падает на `left: None, right: Some(true)`) и строка
  `every_systems_options_are_kept`; фасад — `with_window_answers_none_where_nothing_is_presented`;
  в `backdrop.rs` Win32 — `each_mapping_is_one_table_read_both_ways`. Документы: parity (Liquid
  Glass 🟡 до прогона, #1840), gaps (записи 4 и 20), extending, ARCHITECTURE.md ядра и контракта,
  README, `docs/testing.md` (`flui/native` в области тестов), changelog.
  Проверки после ревью (Windows): `cargo fmt`, `taplo fmt`; clippy `-D warnings --all-targets`
  всего workspace, `-p flui-app -p flui` с `flui/native`, `-p flui-native -p flui-app` с
  `winit-backend` и `flui-app/native`; macOS lib `-p flui-platform -p flui-native` и с
  `winit-backend`, iOS lib (`DOCS_RS=1`), wasm32 lib `-p flui-native -p flui-app` с `native`,
  Android `--all-targets` с `native` (тесты под macOS здесь не собираются: `psm` требует
  C-компилятор цели); nextest `-p flui-platform -p flui-native -p flui-app` с `native` — 364 из
  364 (`trybuild_ui` ядра исключён: расхождение `std::default::Default` прежнее), с
  `winit-backend` — 360 из 360, фасад с `native` — 10 из 10 (`thread_boundary_ui` с фикстурами
  `native` на Windows), `xtask` — 50; doctest'ы ядра (4 + 3), контракта (40 + 9), фасада без
  `native` (3 + 5) и с ним, вместе с `flui-app` (4 + 4, 3 + 1); `checks` (`module-dag` — 296
  модулей, 33 исключения; `globals` — 56; `panic-boundary` — 169 входов: методы зовут AppKit и
  DWM, входов ОС не прибавилось), `facade-combos` — 10, `doc-strict`, `wasm-check`, `wasm-test` —
  51. WSL Ubuntu 24.04 (`WAYLAND_DISPLAY` и `XDG_SESSION_TYPE` сняты): clippy `-p flui-platform
  -p flui-native -p flui-app` с `flui-app/native`, с `winit-backend`, фасад с `native`; nextest
  `FLUI_HEADLESS=1` с `native` — 380: 379 и прежний X11 на WSLg; `--all-features` под xvfb — 297
  из 297; фасад с `native` — 8 из 8 (с `thread_boundary_ui`: Windows-фикстура не компилируется,
  macOS-фикстура не компилируется), `thread_boundary_ui` без фич — пройден; rustdoc `-D warnings
  --document-private-items` с `native` и с `winit-backend`, фасад с `native`. Mac — Liquid Glass,
  материал и тест `an_appkit_backdrop_goes_behind_the_content_and_comes_away` — в очереди
  vanyastaff/flui#1840; `cross-typecheck` — в CI.
  Слияние `main` @ `9d091b190` (LY38) слиянием, без перебазирования: конфликты — корень
  `flui-native/src/lib.rs` (LY38 добавил `UnknownDisplay` к реэкспорту `window::headless`, ветка
  убрала оттуда расширения ОС: оставлены оба) и раздел хода исполнения (записи LY37 и LY38 обе);
  parity и gaps слились сами. Словарь LY38 принят: `MacOSBackdropError::{Closed, NotOnThisSystem}`
  и `Win32WindowError::{Closed, NotOnThisHost}` стали `Unsupported(Unsupported)` — закрытое окно
  `NoWindow`, нет `NSGlassEffectView` — `NotOnThisDevice` (сборка ОС без класса), значение
  контракта новее хоста — `NotOnThisPlatform`; `NotOnTheOwnerThread`, `Refused`, `Unrecognized`,
  `NotOnTheMainLane`, `NoBackdropView`, `ForeignContentView`, `InvalidCornerRadius` остались своими:
  это не отсутствие. Линты macOS-теста, которые нашёл `cross-typecheck (aarch64-apple-darwin)`:
  чтения дерева AppKit — типизированный API `objc2-app-kit` (`NSView::window`,
  `NSWindow::{contentView, firstResponder}`, `NSObjectProtocol::isKindOfClass`,
  `Retained::downcast::<NSVisualEffectView>()` и `material()`), `unsafe` осталось в трёх функциях
  с `#[expect(unsafe_code, reason = …)]` и `SAFETY:` на блоке — `Retained::retain` вида FLUI из
  raw handle, `-[NSView superview]` (в `objc2-app-kit` он unsafe) и свойства `NSGlassEffectView`
  (`style`, `contentView`), которых `objc2-app-kit` без своей фичи не даёт; лишний импорт
  `HasWindowHandle` ушёл, `match` на `Option` стал `if let … else`, сравнения видов —
  `std::ptr::from_ref` (`borrow_as_ptr`). Тесты macOS теперь проверяются здесь: clippy
  `-p flui-native --all-targets --target aarch64-apple-darwin` (и с `winit-backend`), а также
  `-p flui -p flui-app -p flui-native -p flui-platform --all-targets` с фичами job'а, — с C-сборкой
  `psm` через LLVM (`CC_aarch64_apple_darwin=clang`, `AR_…=llvm-ar`,
  `CFLAGS_…=--target=arm64-apple-macos11`): чисто; `cargo-zigbuild` job'а здесь нет.
- [x] **LY38** — ветка `platform/unsupported`, от `main` @ `fc8f8ebaa` (LY34), `main` @ `7eb6bdb7a`
  влит слиянием (LY35, LY36, LY88, LY90 и #1899–#1907). Восемь коммитов по шагам строки: словарь,
  встраивание и потребители, оконные позиции, рёбра Win32 и headless, рёбра Apple, закрытые
  перечисления, conformance окна, архитектура. При слиянии методы `FLUIContentView` (ввод, отрисовка,
  внешний вид, `NSTextInputClient`, `dealloc`) прошли через `os_entry` с ответами, которые LY90 дал им в
  окне; дублёры окна потеряли `as_any` (его даёт печать).
  **Словарь.** `flui_platform::unsupported`: `UnsupportedReason` (`#[non_exhaustive]`: `NotOnThisPlatform`,
  `NotOnThisDevice`, `NotDeclared { what }`, `Unavailable`, `NotRegistered`, `NoWindow`; номера версии ОС
  нет, ADR-0244), `#[non_exhaustive] Unsupported { pub reason }` — строится `Unsupported::new(reason)`,
  литерал вне крейта не собирается (doctest `compile_fail,E0639`); `is_retryable` — только `Unavailable`.
  `Availability { Native, Emulated, Unavailable }` переехал в корень контракта (из
  `input_capabilities`; `flui-sdk` держит старый путь под `unstable-input`), закрыт с причиной.
  **Встраивание.** `WindowCommandError`, `WindowShowError`, `CursorError` (стал `#[non_exhaustive]`),
  `DragError`, `HapticOutcome`, `StorageError` (`Unavailable` и `LockUnsupported` слились;
  блокировки без ОС — `NotOnThisDevice`), `PreferenceQueryError`, `TextStoreHostError` несут
  `Unsupported(Unsupported)` вместо unit-вариантов; у каждого `unsupported() -> Option<Unsupported>` и
  `From<Unsupported>`. В ядре — `OpenWindowError::Unsupported` и `PlatformError::Unsupported`; `Absent<F>`
  отвечает `NotOnThisPlatform` в каждой позиции. Причины по месту: перо Win32 вне зоны — `Unavailable`,
  перо без контроллера — `NotOnThisDevice`; браузер без вибрации — `NotOnThisDevice`, до активации —
  `Unavailable`; закрытое окно TSF/Web — `NoWindow`; незарегистрированное — `NotRegistered`.
  **Оконные позиции.** `Text`, `Accessibility`, `Haptics`, `Chrome` таблицы — `for_window(&Ref)`: каждая
  делает часть окна, пока оно открывается, из того, чем позиция окна называет его для фич
  (`host::Window::Ref`, словарь владельца: `owner::windows::WindowRef`,
  `owner::apple::macos::ContentViewRef`, `owner::apple::ios::ViewRef`, `owner::android::ActivityRef`,
  `owner::web::CanvasRef`, окно winit, `()` headless). `HostPlatform::from_parts(|positions| shared, …)`
  строит `host::WindowPositions` (по функции на позицию) из таблицы и отдаёт в `Shared` позиции окна,
  так что окно не может сделать другой тип, чем называет таблица. Окно держит сделанное, отдаёт через
  `PlatformWindow::{text_input, haptics, system_chrome}` и `HostWindow::{accessibility,
  capture_text_sizing, text_store_host}` и при уходе зовёт `TextInput::detach` и
  `WindowAccessibility::detach` (у мостов ещё `focus_changed`: ключевое окно AppKit для VoiceOver).
  Позиции делают четыре части окна вместе (`WindowPositions::for_window` → `host::WindowFacets`), окно
  держит их одним значением; окно AppKit ставит content view до того, как они сделаны, и мост больше не
  слот, заполняемый после конструктора. Умолчаний нет у `HostWindow::{accessibility,
  text_store_host, capture_text_sizing}` и `PlatformWindow::{text_input, haptics, system_chrome,
  drag_source, show}`: окна Android, iOS и Web сами отвечают `show` — `NotOnThisPlatform`, окна AppKit,
  UIKit и winit сами отвечают `drag_source` — `None`; внешняя реализация `HostWindow` кроме печати
  получает E0046 (написать методы с токеном она не может). Умолчания `PlatformWindow`, которые
  остались: регистрации `on_*`, `pre_present_notify`, `set_input_regions`, `set_frame_rate_hint` и
  геттеры — их снимают LY41–LY44. Окно без позиции отдаёт `Absent` своей таблицы: мост, который
  никогда не активен, вместо `None`. Android: метод ввода держит поле каждого окна по
  `ActivityWindowId` (`ActivityWindowIds` — `checked_add`, тест
  `activity_window_identities_never_repeat_or_wrap`), источник drag отдаёт хук лупера `ViewTransfer`,
  ночной режим, плотность и геометрию касаний — `ActivityPreferences`; Web: редактор холста — хук
  `owner::web::CanvasEditor`, слушатели — `CanvasListener` владельца.
  **Win32 (форма b и c).** У окна один масштаб: `owner::windows::WindowScale` (DPI, который процедура
  меняет на `WM_DPICHANGED`; засевается один раз при открытии — DPI от системы, иначе DPI масштаба
  размещения), его читают контекст окна, TSF, drop target и ввод, копий нет; `WindowStep::Rescaled`
  больше не несёт DPI. Каждый хук держит свои окна в `owner::windows::WindowTable` (положить при
  объявлении, читать только для окна, которое называет ссылка, забрать при уничтожении или
  `WM_NCDESTROY` и завершить с отпущенной таблицей): сессия, drop target, TSF и ввод. TSF: хук `Tsf`
  держит соединение окна, окно отдаёт его через `WindowClass.text` (`WindowTexts`,
  `owner::windows::window_texts`: ручка текста — `Send + Sync`, хук — `Rc` owner-потока, поэтому ручка
  достаёт его через класс своего потока, и поток без класса соединения не находит), сдвиг полей после
  прямоугольника нового DPI — новый шаг `ScaleApplied`, деактивация — новый шаг `Closed` между
  колбэками закрытия и уходом контекста (ADR-0135 §3, поправлен). Drop target: хук
  `DropTargetRegistration` держит регистрацию, которую возвращает `HostDrag::register_target`
  (`HostDrag` приходит в `CreatedWindow.drag`), и отзывает её; второй карты целей у транспорта нет;
  цель держит колбэки и `WindowScale` окна и не трогает контекст. Сессия: хук
  `lifecycle::windows::SessionWindows` подписывает каждое окно на `WTS` и отдаёт блокировку шагом
  `SessionLocked`, окно `WM_WTSSESSION_CHANGE` не разбирает. **headless (c):** лупер отдаёт окну
  разложенные дисплеи фактами (`HeadlessState::{window_display, display_bounds, holds_display}`) и
  источник drag через `HeadlessHost::drag_source`; `MockWindow::simulate_move_to` отказывает
  `UnknownDisplay` самого окна.
  **Apple.** `FLUIContentView` — владельца (`owner::apple::macos::content_view`): регистрирует все методы
  сам и раздаёт их частям вида, которые делает таблица (`MacosHost::view_hooks`): `ViewInput` ввода
  (`ViewInputs`) и `ViewText` текстового ввода (`TextView`; методы `NSTextInputClient` переводят
  аргументы AppKit в значения Rust), маршрутизирует нажатие между ними и входит в проход отрисовки
  (`owner::apple::macos::display_pass`, переехал из окна) для кадра `drawRect:`. `MacOSTextInput` доходит
  до `TextView` вида через полосу владельца; `bounds_of` — владельца; масштаб, который вид держал и не
  читал, убран. iOS: касания вида — `ViewTouches` ввода (`IosHost::view_touches`), каденс линка и экран —
  хук дисплеев лупера (`observe_link`, `move_window`, `forget_link`, `LinkPeriod` каждого окна держат
  дисплеи); текстовая позиция iOS — `IosTextSizing` (захват размера текста по трейтам вида), окно
  оставляет себе проверки допуска вокруг него. ADR-0071 поправлен.
  **Исключения `module-dag`:** 33 → 1, осталось `display::winit` → `display::linux` (до LY87); комментарий
  над списком переписан.
  **Закрытые перечисления.** `#![deny(clippy::exhaustive_enums)]` в контракте; из 24 найденных (26 строки
  — с `Availability` и `UnsupportedReason`) `#[non_exhaustive]` — `WindowAppearance`, `WindowEvent`
  (их сообщает бэкенд приложению); остальные 22 закрыты с причиной (23 `allow` вместе с
  `Availability`): набор задан тем, что они называют (`KeyState`, `Location`, `KeyRepeat`,
  `ImeComposition`, `PointerRole`, `ButtonChange`, блокировка TSF — `LockKind`, `LockTiming` (флаг
  `TS_LF_SYNC`), `LockGrant`, `LockOutcome`, — `PointMode`, `Brightness`, `TransferUri`, `OsDragEnd`,
  `OsOffer`, `HapticEvent`), открытый конец — `Custom` (`TransferFormat`, `TransferPayload`), или новый
  вариант обязан обработать каждый потребитель (`PlatformInput`, `DragDropEvent`, `OsDragStart`,
  `WindowBackgroundAppearance` — его отображает каждый бэкенд запечатанного окна).
  **Сверх строки.** Актуатор iOS делается вместе с окном, а не при первом обращении; у TSF новые шаги
  окна `ScaleApplied` и `Closed`; `ActivityWindowIds` Android с `checked_add`; `Persisted` отвечает
  `NotRegistered`, когда хранилище не зарегистрировано. Причина выбрана по месту и по смыслу
  `UnsupportedReason`: временно и повтор может помочь — `Unavailable` (перо Win32 вне зоны, браузер до
  активации, drag без нажатой кнопки мыши); нет у этого устройства — `NotOnThisDevice` (перо без
  контроллера, браузер без вибрации, файловая система без блокировок); нет на платформе ни на одном
  устройстве — `NotOnThisPlatform` (`Absent`, `show` на Android, iOS и Web, OS-drag пером или касанием
  на Win32: цикл OLE ведёт только мышь); окна нет — `NoWindow`; не зарегистрировано — `NotRegistered`.
  **Conformance.** `capabilities_are_honest` ещё требует, чтобы отказ в команде вне возможностей не
  просил повтора (`Unavailable`).
  Тесты (новое поведение; «падает без» проверено откатом): `every_error_carries_its_reason`
  (`flui-platform/tests/unsupported.rs`, строки по всем восьми носителям и исходу haptics;
  `StorageError::unsupported`, всегда `None`, роняет его), doctest `compile_fail,E0639`,
  `a_drop_target_follows_its_window_to_a_new_scale` и
  `a_rescaled_window_answers_layout_at_its_new_scale` (до общего масштаба роняли пустой
  `rescale_target` и пропуск `services.rescale`),
  `a_move_to_a_display_the_platform_does_not_hold_is_refused`,
  `a_window_that_refuses_for_now_what_it_never_takes_is_caught` (без новой ветки проверки роняет),
  `activity_window_identities_never_repeat_or_wrap`; по ревью — таблица `absent_positions_say_not_here`
  (`flui-native/tests/absent.rs`: `OpenWindowError` и `PlatformError` с `NotOnThisPlatform`, мост, который
  никогда не активен и не держит слушателей; все три строки падают при другой причине у `Absent` и при
  удержанном слушателе), `a_window_opens_at_one_dpi_its_hooks_share` (правило засева и одно значение
  для хуков), строка `a_window_hands_its_drop_target_and_text_services_its_one_scale` таблицы OLE (при
  открытии и на 120 или 144 DPI окно, drop target и TSF читают одно; открыть окно не на 100% хост не
  даёт, поэтому засев при открытии проверяет правило), строки причин в
  `the_os_loop_carries_only_a_drag_a_mouse_button_holds` и
  `a_drag_the_os_loop_cannot_carry_stays_in_its_window` (без кнопки — `Unavailable`, повторяемо). Блокировка сессии через
  хук — существующий `windows_window_commands` (сообщение окну), порядок TSF — существующие тесты `tsf`.
  **Ревью (vanyastaff/flui#1909).** Не сделано, с причиной: (1) `PlatformWindow::{haptics,
  system_chrome}` остаются `Option`: ADR-0151 §5 и ADR-0254 п. 7 держат грань сверх обязательного ядра
  за `Option` (открытый трейт, двойники); отсутствие возможности говорит объект грани своим
  `Unsupported`, и каждое окно ядра его отдаёт; `None` у `text_input` — модель pull (TSF), а не
  отсутствие. (2) `Availability` в `WindowCapabilities` (последняя часть R3.7) — строка LY40
  («`activates` и сходные признаки — `Availability`, а не `bool`») и LY43 (подсказка частоты кадров,
  регионы ввода, материал фона). (3) Импорты `crate::text::{TextInput, WindowTextInput}` и
  `crate::input::availability` в файлах окон — модели других фич, которые R4.3 разрешает (запрещены
  файлы платформ других фич); `module-dag` их пропускает по тому же правилу. (4) Потребитель
  `unsupported()` в production — очередь команд окна `flui-app` пишет причину и `is_retryable`;
  остальные носители читают: `AdmissionError::Unsupported` у `WindowControl::submit` — LY42, причина
  провайдера до виджета без изменений — LY50.
  Проверки на слитом состоянии: Windows — clippy рабочего пространства (`--all-targets -D warnings`),
  nextest `flui-native`, `flui-app`, `flui-platform`, `flui-testing`, `flui-runtime` — 498/498;
  `flui-native` и `flui-app` с `winit-backend` — 359/359; trybuild рабочего пространства по одному
  потоку — 9/9; `xtask checks` целиком (в нём `module-dag` — одно исключение, `globals`,
  `panic-boundary` — 165 входов, самотесты), `doc-strict`, `wasm-check`, `wasm-test`. clippy
  `-D warnings`: `aarch64-apple-darwin` (lib, обычный и `winit-backend`), `aarch64-apple-ios` (lib,
  `DOCS_RS=1`), `aarch64-linux-android` (`flui-native`, `flui-app`, `--all-targets`),
  `wasm32-unknown-unknown`. Linux (WSL Ubuntu 24.04): clippy четырёх крейтов `--all-targets` и
  `flui-native --all-features`, nextest под xvfb — 443/443, `--all-features` — 444/444, rustdoc
  `-D warnings`, `api-closure -p flui-platform` чист.
  Устройство (2026-10-22, S21, до слияния): окно, текстовый ввод и haptics после снятия рёбер — haptic
  `Submitted`, ввод текста доходит; ночной режим Dark → Light → восстановлен; поворот — 853×384 с
  отступами t24 b0 l28 r48 против портретных t28 b48, первый кадр после поворота приходит с размером
  раньше отступов; `view_insets` IME на Android нет нигде. Не запускалось: Mac и iPhone
  (мультиплексоры AppKit и UIKit, фокус VoiceOver) — в очереди vanyastaff/flui#1840; тесты и doc macOS
  (нужен C-тулчейн), `cross-typecheck`; браузер — у координатора (присоединение и отсоединение
  редактора холста, исходы haptics `NotOnThisDevice`/`Unavailable`, отсоединение при закрытии окна).
- [x] **LY40** — ветка `platform/window-state`, начата на вершине LY38 `45809e5ea`, перенесена на
  `main` @ `9d091b190` (LY38) и `f5da90a90` (LY37), конфликт только в журнале
  `docs/platform-parity.md`. Аддитивно: геттеры и парные `on_*` живут до LY41.
  **Контракт.** `WindowState` получил `metrics: Option<WindowMetrics>`, `appearance`, `safe_area`,
  `execution`, `hovered`, `capabilities` и `with_*` к каждому (дисплей, фокус и видимость были);
  `new` — `None`, `Light`, ноль, `Running`, `false`, `WindowCapabilities::NONE`. Правило «нерисуемое
  не публикуется» — в `successor`: образец без `metrics` (нулевой размер, неконечный или
  неположительный масштаб) сохраняет метрики снимка, так что такой отчёт без других перемен
  ревизии не даёт; `None` — только «ещё ни разу не было рисуемой площади». `is_active` отдельного
  факта не получил: ADR-0254 §1 его не называет, его умолчание — фокус; `mouse_position` и
  `modifiers` — поток ввода. `WindowCapabilities`: все 15 полей — `Availability` (и наблюдения
  `observes_*`, `places_at_open`: «отчёт делает ОС, хост или никто»), `NONE`/`ALL` — через
  `every`, `availability(&WindowCommand) -> Availability` (`Normal` — всегда `Native`), `permits`
  остался (`is_available`); `Availability::is_available` новый; `expect(struct_excessive_bools)`
  ушёл. `admit` отказывает ровно при `Unavailable`, `Emulated` допускается. Хосты: Win32 —
  `enters_fullscreen: Emulated` (полноэкранность — окно без рамки во весь монитор), прочее
  `Native`; AppKit и Web — как были, `Native`; winit — `Native`, Wayland и X11 без пробы —
  `Unavailable` там, где был `false`; Android и iOS — `NONE`. Каждый набор назван по полю, так что
  новая возможность контракта у хоста `Unavailable`, пока `apply` её не делает.
  **Публикация.** У `WindowCallbacks` — `StatePublisher` (`Arc<dyn Fn() + Send + Sync>` над слабой
  ссылкой на окно, `set_state_publisher`, сбрасывается в `clear`): каждая доставка факта (resize и
  масштаб, фокус, видимость, наведение, внешний вид, дисплей, safe area, исполнение; iOS —
  `dispatch_metrics_resize` и немедленный lifecycle кроме поверхности) сначала зовёт его без
  замков набора, так что снимок перечитан и `on_state_changed` поставлен в очередь раньше
  колбэка факта, в том OS-колбэке, где факт пришёл; наблюдение, фиксируемое в точке доставки
  (фокус и видимость Web), публикуется сразу после фиксации. `publish_state` — `pub(crate)` для
  факта без своего колбэка (iOS: размер при нерисуемых метриках, шаг lifecycle — вместо
  безусловного `dispatch_state_changed`). Наведение — одно правило для всех хостов: что сказал
  последний `dispatch_hover_status_change` (`WindowCallbacks::hovered`), кто бы его ни сообщил
  (ввод Win32 и Web, tracking area AppKit, курсор winit), — без ребра от ввода к окну. Хосты ставят
  издателя при открытии: headless (`publish_window_state`, свободная функция над моделью), Win32
  (`with_window_context` → `publish_window_state`), winit (`refresh_state`), AppKit, UIKit, Android
  (`publish_state`), Web (`publisher().publish()`). Выборки: метрики — клиентская область Win32 в
  пикселях устройства при масштабе записи, `inner_size` winit, область контента AppKit при
  `backingScaleFactor`, метрики вида UIKit, `ANativeWindow` Android, буфер холста Web; внешний вид
  — тема пользователя, как применена к окну (ADR-0222): Win32 — то, что последний
  `WM_SETTINGCHANGE` прочёл (`ctx.appearance`), winit — `appearance_of` (тема winit, иначе портал),
  AppKit — `effectiveAppearance` окна, UIKit и Android — их отчёт, Web — `prefers-color-scheme`;
  исполнение — Android `resumed` (`execution_of`), UIKit lifecycle, Web Page Lifecycle, headless —
  `Detached` после закрытия. Сверх строки: (1) снимок Win32 невидим под маскировкой DWM или при
  заблокированной сессии (`OwnerTracking::occluded`), как уже говорил геттер `is_visible` и
  колбэк видимости, — иначе потребитель LY41 видел бы скрытое окно видимым; `settle_visibility`
  помечает снимок устаревшим и тогда, когда отчёт видимости не меняется; (2) winit объявляет снимок
  до отчёта видимости, который тянет свёрнутость X11 (`follow_wm_minimized` после
  `dispatch_state_changed`): прежний порядок давал гонку, которую прогон под xvfb поймал в
  `x11_window_takes_commands_and_publishes_what_the_server_holds` («a notification»); (3) поле
  `shell` окна winit удалено (читалось только выборкой); (4) `MockWindow::simulate_hover` — двойник
  ОС для наведения, как прочие `simulate_*`; закрытие headless публикует скрытое и `Detached` одной
  ревизией до `on_close`.
  **Conformance.** `flui_testing::window_conformance`: `Readback { metrics, focused, visible }` и
  обязательный `WindowFixture::readback` (что говорит оконная система своими средствами, `None` —
  не читается), проверка `the_snapshot_agrees_with_the_window_system` — при открытии и после
  каждой принятой команды факты снимка равны прочитанному, а `capabilities` снимка — окна.
  Headless-фикстура читает модель через геттеры (LY41 переведёт её на модель, когда геттеров не
  станет). Win32 впервые проходит весь набор против настоящей системы: `the_win32_window_conforms`
  (`GetClientRect` и `GetDpiForWindow`, `GetFocus`, `IsWindowVisible` и `DWMWA_CLOAKED`). Находка:
  окно Win32 без рамки не может быть изменяемым (`WS_THICKFRAME` уходит с рамкой), и
  `SetResizable(true)` на нём сходился бы только после `SetDecorated(true)`; порядок образцовых
  команд — рамка, затем изменяемость (комментарий в коде). Живой resize:
  `a_live_resize_publishes_inside_its_modal_loop` — клавиатурный цикл `SC_SIZE`, стрелки и Enter
  из другого потока; каждый `on_state_changed` внутри цикла читает снимок, чьи метрики равны
  клиентской области и DPI Win32 на этом шаге, до возврата из цикла. winit conformance не
  запускалась: харнесс реального цикла (`real_loop_tests`) гоняет сценарий в рабочем потоке, а
  `apply` и `settle` фикстуры требуют хода владельца между командами.
  Тесты (новое поведение; «падает без» проверено откатом): `flui-platform` —
  `each_window_fact_advances_the_revision`, `a_report_that_is_not_drawable_keeps_the_metrics`,
  `admit_refuses_exactly_when_the_capability_is_unavailable` (переименован из `…_is_false`, строка
  `Emulated` допускается); `flui-native` — `window_facts_arrive_only_in_the_snapshot` (таблица
  `window_state_contract`: фокус, видимость, наведение, внешний вид, safe area, исполнение,
  дисплей, масштаб, размер — одна ревизия, `State` раньше `Fact`, повтор не публикует, NaN-масштаб
  и пустая площадь метрик не трогают, `capabilities` снимка — окна; с выключенным издателем падают
  наведение, внешний вид, safe area, исполнение и масштаб: «heard [Fact, State]»),
  `the_win32_window_conforms` и `a_live_resize_publishes_inside_its_modal_loop`
  (`native_window_commands`; без метрик в выборке Win32 падают оба); `flui-testing` —
  `a_snapshot_without_the_windows_facts_is_caught`, а без метрик в выборке headless падает
  `the_headless_window_conforms_under_every_capability_set`. Записи нативных трасс и conformance
  ввода не тронуты.
  `module-dag`: 1 исключение, без изменений; `globals` — 56. Документы: ARCHITECTURE.md ядра
  (снимок со всеми фактами и `Availability`, новый пункт «каждый колбэк факта идёт после снимка»,
  iOS публикует из метрик и lifecycle), `docs/platform-parity.md` (свидетельство Windows у
  «Minimize, maximize, restore, state readback», строка журнала), `changelog.d/platform-window-state.md`.
  Проверки (Windows, после переноса на `f5da90a90`): `cargo fmt`; clippy `-D warnings` рабочего
  пространства `--all-targets` и `-p flui-native -p flui-app` с `winit-backend`; nextest
  `flui-platform`, `flui-native`, `flui-app`, `flui-testing`, `flui-runtime`, `flui-interaction`,
  `flui-view` — 689 из 690 (16 пропущены), `flui-native` и `flui-app` с `winit-backend` — 364 из
  365 (2); упал один trybuild `owner_thread_token_has_no_default`: rustc печатает `Default`, а
  `.stderr` (с LY25) ждёт `std::default::Default`; до переноса на LY37 тот же код ветки давал
  687 из 687 и 362 из 362, в WSL он проходит, файл не тронут — расхождение пришло с `main`; doctest'ы
  `flui-platform`, `flui-native`, `flui-testing`; clippy Android `-p flui-native -p flui-app
  --all-targets`, macOS lib (и с `winit-backend`), iOS lib (`DOCS_RS=1`), wasm32 lib (`flui-native`,
  `flui-app`); `wasm-check`, `wasm-test` — 51; `checks` (`module-dag` и самотест — 296 модулей,
  1 исключение; `globals` и самотест — 56; `file-length`, `markers`, `testing`, `docs-paths`,
  `workspace` — 28 крейтов, 229 рёбер; `reach` — 68 сборок; `android-dex`, taplo), `doc-strict`.
  WSL Ubuntu 24.04 (`WAYLAND_DISPLAY` и `XDG_SESSION_TYPE` сняты): clippy `-p flui-platform -p
  flui-native -p flui-app -p flui-testing --all-targets`, с `winit-backend` и `flui-native
  --all-features`; nextest `FLUI_HEADLESS=1` без признаков — 318, под xvfb с `winit-backend` — 446
  (1 пропущен), `--all-features` под xvfb — 300; X11-тесты пять раз подряд после правки порядка —
  17 из 17; rustdoc `-D warnings --document-private-items` без признаков и с `winit-backend`;
  `api-closure -p flui-platform` — чисто. Устройства не нужны (строка — L + W + T): Android меняет
  выборку и издателя, их держит clippy Android. Не запускалось: Mac и iPhone (выборка AppKit с
  `effectiveAppearance` и метриками, издатели AppKit и UIKit) — в очереди vanyastaff/flui#1840;
  `cross-typecheck` (нет `zig`; job в CI); winit conformance (см. выше); браузер — у координатора:
  снимок холста после наведения мыши, смены `prefers-color-scheme`, `freeze`/`resume` страницы и
  изменения масштаба страницы несёт эти факты новой ревизией.
  **Ревью (vanyastaff/flui#1911), коммиты поверх.** (1) Паника в `on_state_changed` больше не
  теряет колбэк факта: издатель (`StatePublisher`) только читает окно и отвечает `Publication`
  (`Changed`, `Unchanged`, `Held`), ничего не объявляя, а `WindowCallbacks` кладёт
  `on_state_changed` и колбэк факта в одно событие очереди `Published { changed, facts }`, каждый
  в своей изоляции; первая ошибка всплывает после обоих (`resurface_first`). Для фактов вне
  очереди окна (safe area, немедленный lifecycle и resize iOS) — `publish_then`, то же правило.
  Тест `a_panicking_state_callback_still_delivers_the_resize` (`window_state_contract`): без
  изоляции объявления падает. (2) Win32: пока окно держит публикацию (`WM_DPICHANGED`, переход в
  полноэкранный режим), издатель отвечает `Held`, факт ждёт в `WindowCallbacks::held` и уходит за
  публикацией, снимающей удержание (`announce` в `window_state`); `on_resize` больше не обгоняет
  `on_state_changed`. Тест `a_dpi_change_announces_the_snapshot_before_the_resize`: с `Unchanged`
  вместо `Held` падает (как и `a_state_read_inside_a_callback_never_sees_a_half_made_change`,
  которая теперь ждёт, что колбэк внутри удержанного изменения читает целое новое окно, а не
  прежнее). Вне owner-потока факт на Win32 не доставляется: все факты идут из оконной процедуры;
  отказ контекста оставляет снимок устаревшим, следующее чтение владельца публикует.
  (3) Издатель Win32 читает контекст только с `WindowIdentity` своего окна: переиспользованный
  `HWND` ничего чужого не публикует. (4) `a_live_resize_publishes_inside_its_modal_loop` ловит
  порядок (`on_resize` после снимка с его метриками) и метрики против readback Win32 (без метрик в
  выборке падает); `Readback` — без `Default`, поля `focused`, `active`, `visible` обязательны
  (`bool`), `metrics: Option` — «нет рисуемой площади». (5) Новый факт `WindowState::active`
  (`with_active`): Win32 — `WM_ACTIVATE` (окно, чьё собственное всплывающее окно держит
  клавиатуру, активно и без фокуса), прочие хосты понятия, отдельного от фокуса, не имеют (ключевое
  окно AppKit, `Focused` winit, фокус холста, сцена, Activity) — у них это фокус. Тест
  `activation_is_a_fact_apart_from_focus` (без публикации в `WM_ACTIVATE` падает), строка
  `activation` в `each_window_fact_advances_the_revision`, `active` в `Readback`.
  (6) `WindowCallbacks::publish_through(Weak<W>, fn(&W) -> Publication)` вместо пяти одинаковых
  замыканий (winit, AppKit, UIKit, Android, Web; headless — через `SnapshotSource`); у каждого хоста
  `read_state`. (7) Факты сверх геометрии — один тип `window::state::ReportedFacts` с `onto`; его
  строит каждый хост (у Android — `Kept`, что окно хранит из прежних отчётов; у winit —
  `reported_facts`). (8) Doc Android на месте, тест переименован в
  `each_window_fact_reaches_the_snapshot_before_its_callback` (ADR-0254 называет
  `window_facts_arrive_only_in_the_snapshot` для состояния после LY41, когда колбэков фактов не
  станет), `dispatch_hover_status_change` закрыт по `lifecycle_closed`, комментарии «`false`» →
  `Unavailable`. Сверх строки ещё раз: порядок образцовых команд conformance (рамка, затем
  изменяемость: окно Win32 без рамки не бывает изменяемым) и невидимость снимка Win32 под
  маскировкой DWM или при заблокированной сессии. Conformance «снимок против readback» не
  запускалась на winit, AppKit, UIKit, Web и Android — записано в `docs/gaps.md` (запись 9).
  Изменения форм для LY41: `StatePublisher` возвращает `Publication`, `WindowCallbacks::{announce,
  publish_through}`, `WindowState::active`, поля `Readback` обязательны. Прогон поймал взаимную
  блокировку winit: выборка дважды брала `is_focused` в одном выражении (временный замок жив до
  конца оператора) — флаг читается один раз. Проверки: `cargo fmt`; clippy рабочего пространства
  и `flui-native`/`flui-app` с `winit-backend` (Windows); nextest семи крейтов — 689 из 690, с
  `winit-backend` — 364 из 365 (оба раза падает только trybuild `owner_thread_token_has_no_default`
  с `main`, см. выше); clippy Android `--all-targets`, macOS `--all-targets` (`clang` и `llvm-ar`
  LLVM) обычный и с `winit-backend`, iOS lib, wasm32 lib; `wasm-check`, `wasm-test` — 51; `checks`,
  `doc-strict`; WSL — clippy без признаков, с `winit-backend` и `--all-features`, nextest 318, 446
  (1 пропущен), 300, rustdoc.
- [x] **LY41** — ветка `platform/snapshot-consumers`, начата на вершине LY40, после слияния
  vanyastaff/flui#1911 перенесена на `main` @ `006d3d7b1` (дерево LY40 в `main` то же, что
  `5fbcbb117`). **Удалено.** Из `PlatformWindow` — геттеры `is_focused`, `is_visible`,
  `physical_size`, `logical_size`, `scale_factor`, `metrics`, `appearance`, `display_id`,
  `safe_area_insets`, `execution_state`, `is_active`, `is_hovered`, `capabilities`,
  `mouse_position`, `modifiers`; восемь парных `on_*`; `show`, `activate`, `close` и
  `WindowShowError`; `WindowEvent` и `Platform::on_window_event`; `WindowControl::capabilities` и
  `WindowHandle::capabilities` (возможности — поле снимка). В ядре ушли слоты фактов и их
  `dispatch_*`, событие `Published`, `LifecycleEvent`, `deliver_status_observation`, отчёты
  видимости (`window::visibility` целиком, `ReportedVisibility` и `follow_wm_minimized` winit,
  правила поверхности Win32 `windows::surface`, `visibility_muted`, `last_monitor`), глобальные
  события окон. Остаются шесть регистраций по обязательствам (`on_input`, `on_request_frame`,
  `on_state_changed`, `on_close`, `on_should_close`, `on_surface_status_change`) — их заменит LY44.
  **Добавлено сверх строки** (факта или команды в снимке не хватало): (1) `WindowCommand::Close` и
  `WindowCapabilities::closes` — закрытие стало командой здесь, а не в LY42 (иначе `close` нельзя
  удалить): `Native` на Win32, macOS, iOS, winit и headless, `Emulated` на Web (холст уходит),
  `Unavailable` на Android (окно одно, приложение его не закрывает; строка «Open and close a
  window» в `docs/platform-parity.md` это называет); (2) `WindowState::is_seen` — «видно и не
  свёрнуто», одно чтение вместо правил поверхности, им пользуются жизненный цикл `flui-app` и
  presentation; (3) `HeadlessWindowSystem` и `MockWindow::window_system` — что держит оконная
  система headless, readback conformance-фикстуры вместо геттеров; (4) `ActivityWindow::scale`
  (Android, `pub(crate)`) вместо геттера масштаба.
  **Потребители.** `flui-app`: одна подписка на окно (`WindowInstall::observe_window`), которая
  сравнивает снимок с прежним (`RuntimeEvent::of_state_change`) и отдаёт `WindowStateChanged`,
  `Resized`, `SafeAreaChanged`, `AppearanceChanged`, `WindowExecution`, `WindowFocus`,
  `WindowVisibility(is_seen)`, `WindowHover`; смена дисплея или масштаба будит owner, чтобы
  снимок дисплеев перечитался; `SurfaceGeometry::of` читает размеры и масштаб (до первых рисуемых
  метрик — масштаб 1); закрытие, показ и активация — команды (`Activate` с `Unsupported`
  игнорируется). `flui-runtime` (presentation, media query, кадр, предпочтения, запись), drag
  `flui-interaction`, `flui-view`, `flui-testing` (conformance читает возможности из снимка,
  закрывает командой, когда `closes` есть), `examples/{scene_render,web_demo}` — на снимке. Около
  30 тестовых двойников потеряли методы; двойники, чьи `capabilities()` были `ALL`, перенесли это
  в снимок. **Поведение, найденное по пути.** Web: окно перечитывает состояние на каждом
  пробуждении owner'а, иначе переход на другой экран не доходил до `display` снимка
  (`web_window_names_the_display_the_snapshot_describes` падал); winit/Linux: окно публикует схему
  портала, которой его засеяли при открытии (`color-scheme` в `linux_preferences` падал под
  xvfb); `StateChanged`, поставленный в очередь внутри упавшего колбэка, доставляется — ревизия уже
  опубликована (`window_callback_unwind` теперь ждёт этого).
  Тесты (откат проверен): `window_facts_arrive_only_in_the_snapshot` (`window_state_contract`,
  имя, которое цитирует ADR-0254) — каждый факт даёт одну ревизию и ровно одно `on_state_changed`,
  услышанное до любого чтения `state()`; без публикации в `report_hover` падает «hover: heard
  []»; `a_state_change_survives_a_failed_callback` (юнит в `owner::handlers`) — без доставки
  `StateChanged` при восстановлении падает «the state notice was lost»; `compile_fail` doctest на
  `PlatformWindow` (`window.is_focused()`, E0599). Переписаны на снимок: `displays.rs`,
  `contract.rs` (`a_dpi_change_publishes_the_new_scale_once`,
  `moves_within_one_monitor_keep_the_display`), `windows_window_commands.rs`
  (`a_dpi_change_announces_one_whole_snapshot`), `real_loop_tests.rs` (программное закрытие не
  спрашивает `on_should_close`), X11 (`is_seen` вместо отчётов видимости), wasm32 и браузерные.
  `module-dag`: 1 исключение, без изменений; `globals` — 56; запись `platform_window.rs` в
  `markers.toml` удалена (маркеров не осталось). Документы: ARCHITECTURE.md ядра (пункт «снимок —
  единственный источник факта» вместо «колбэк факта после снимка», Web на пробуждении, дисплей
  Win32 и macOS, тема, видимость), `flui-platform` (новый пункт), `flui-runtime`, README двух
  крейтов, `docs/architecture.md`, `docs/platform-parity.md`, `changelog.d/platform-snapshot-consumers.md`.
  Проверки: `cargo fmt`; clippy `-D warnings` рабочего пространства `--all-targets` обычный и с
  `flui-native/winit-backend`; nextest `flui-native`, `flui-app`, `flui-platform` — 366, с
  `winit-backend` — 401 (2 пропущены; оба раза падает только trybuild
  `owner_thread_token_has_no_default` с `main`, как в LY40), `flui-runtime`, `flui-testing`,
  `flui-view`, `flui-interaction`, `flui` — 327 (14); doctest'ы `flui-platform`, `flui-native`,
  `flui-testing`, `flui-view`; `.stderr` `platform_and_host_window_are_sealed` перегенерирован
  (сдвинулись только номера строк); clippy Android `-p flui-native -p flui-app --all-targets`,
  macOS lib `flui-native` обычный и с `winit-backend`, iOS lib (`DOCS_RS=1`), wasm32 (`flui-native`,
  `flui-web-demo` `--all-targets`); `wasm-check`, `wasm-test` — 51; `checks`, `doc-strict`, `reach`.
  WSL Ubuntu 24.04 (`WAYLAND_DISPLAY` и `XDG_SESSION_TYPE` сняты): clippy `-p flui-native -p
  flui-app -p flui-platform -p flui-testing --all-targets` и с `winit-backend`; nextest
  `FLUI_HEADLESS=1` — 212, под xvfb с `winit-backend` — 340 (1 пропущен), `--all-features` — 296;
  rustdoc `-D warnings --document-private-items` без признаков и с `winit-backend`. Не
  запускалось: `flui-app` под macOS и iOS (на этом хосте нет `cc` для `psm`), Mac и iPhone — в
  очереди vanyastaff/flui#1840; `cross-typecheck` (нет `zig`); полный nextest рабочего
  пространства (виджеты и пакеты — только clippy); браузер — у координатора: перенос окна
  страницы на другой экран меняет `display` снимка, `freeze`/`resume` и фокус холста видны одним
  `on_state_changed` каждое.
  **Исправления после ревью (vanyastaff/flui#1912).** (1) Подписка окна в `flui-app`
  (`observe_window`) больше не теряет факты после паники подписчика: снимок ведёт
  `RuntimeEvent::follow` (база сдвигается до доставки, чтобы перемена, сделанная подписчиком, шла
  против этого снимка; снимок без метрик сохраняет прежние, так что Some→None→Some тех же метрик
  не даёт resize), доставку — `RuntimeEvent::deliver_each`: каждое событие в своей изоляции,
  первая паника всплывает после всех. Тест `window_state_follow_contract` (таблица в
  `owner_dispatch/state_follow_tests.rs`: каждый факт — ровно его событие, активация одна — только
  `state`, свёртывание — `WindowVisibility(false)`, цикл метрик, перемена до чтения seed ничего
  не даёт, перенос на дисплей или новый масштаб просит выборку, паника доставки не теряет поздние
  факты). Откат проверен: без ветки `is_seen` (сравнение `visible`) падают три случая; без
  сохранения метрик — цикл; без изоляции доставки — паника. Тест крейтовый, а не в `tests/`:
  `RuntimeEvent` и `observe_window` закрыты внутри раннера, и публичного входа, который запускает
  раннер на управляемом headless-окне, нет. (2) Отказ закрытия не теряется:
  `CloseRequestError::Refused { address, error }` у `request_presentation_close` (Android:
  `closes` — `Unavailable`); тест `a_window_that_cannot_close_says_so` (`close_request_matrix`)
  падает без него («Ok»). Окна, которые раннер отбрасывает (неудачная или отменённая установка,
  открытие, которого никто не ждёт), закрываются через `runner::discard_window`, который пишет
  отказ в журнал: слушателя у отказа нет. (3) Win32: `WindowCommand::Close` с чужого потока шлёт
  свой `CLOSE_WINDOW` (`WM_APP + 4`, с идентичностью окна), владелец уничтожает окно без вопроса
  вето; раньше уходил `WM_CLOSE`, который спрашивал вето. Тест
  `a_close_from_another_thread_asks_no_veto` (`native_window_commands`) с `WM_CLOSE` вместо
  нового сообщения падает. Путь сброса окна в `Drop` с чужого потока по-прежнему шлёт `WM_CLOSE`
  (сброс — не команда). (4) Вето закрытия через `WindowControl` уже покрыто:
  `commands_after_a_close_that_goes_ahead_are_dropped_and_after_a_veto_apply`
  (`window_handle_contract`, `flui-app`) — `request_close` с обработчиком `KeepOpen` оставляет окно
  и применяет остальные команды. (5) Мелкое: `StatePublisher` Web делит одну выборку (`sample`);
  перечитывание окна на пробуждении owner'а Web изолировано от паники колбэка, как хук конца
  сессии; `WindowCallbacks::announce` удалён (хосты зовут `dispatch_state_changed` только при
  новой ревизии); `HeadlessWindowSystem::active` удалено — активация headless и есть фокус,
  фикстура читает `focused`. Чтение возможностей клонирует снимок (строку заголовка и малые
  поля) — дёшево, API не растёт. (6) Документы, которые называли удалённое (`close()`,
  `on_window_event`), исправлены; `rg` по удалённым именам вне `docs/plans`, ADR и changelog пуст
  (оставшиеся совпадения — `WindowEvent` winit и чужие `close()`). `docs/gaps.md`: запись 9 —
  Android не закрывает окно, запись 4 — браузерный прогон снимка не выполнен. Тест фасада
  `tests/surface.rs` (`flui/native`, Windows) закрывает окно командой — его ловил
  `cross-typecheck`. Проверки: clippy `-D warnings` рабочего пространства `--all-targets` с
  `flui-native/winit-backend` и `flui/native`; clippy и nextest `-p flui --features native` — 10;
  nextest `flui-native`, `flui-app`, `flui-testing` — 389, с `winit-backend` — 424 (2 пропущены;
  падает только trybuild `owner_thread_token_has_no_default` с `main`); clippy wasm32 lib,
  `wasm-test` — 51; `checks`, `doc-strict`; WSL — clippy и rustdoc с `winit-backend`.
- [x] **LY42** — ветка `platform/admitted-command`, начата на вершине LY41 `87977afc3`, после слияния
  vanyastaff/flui#1912 перенесена на `main` @ `c50202c7a` (дерево то же). **Контракт.**
  `WindowCommand::admit(self, &WindowCapabilities) -> Result<AdmittedCommand, AdmissionError>` —
  единственный конструктор `AdmittedCommand` (поля закрыты); `PlatformWindow::apply(AdmittedCommand)
  -> Result<(), ApplyError>`. `AdmissionError { Unsupported, InvalidGeometry }` (`unsupported()`),
  `ApplyError { Closed, Superseded, Platform { message } }`, `SubmitError { Admission, Closed }`
  (`unsupported()`); `WindowCommandError` удалён (и из `flui-sdk::platform`, где теперь
  `AdmissionError` и `SubmitError`). Ревизия возможностей живёт в самих `WindowCapabilities`
  (закрытое поле, `revision()`): её ведёт только `WindowState::successor` — ревизия образца
  игнорируется, при смене любого поля возможностей она становится ревизией нового снимка, при смене
  другого факта не двигается. Равенство и хеш возможностей сравнивают только поля доступности
  (деструктуризация: новое поле не скомпилируется, пока его нет в сравнении) — иначе снимок X11,
  чьи возможности пересмотрены, не был бы равен тем же возможностям, собранным руками (поймал
  прогон под WSLg). `AdmittedCommand::against(&WindowCapabilities) -> Result<WindowCommand,
  ApplyError>` — единственный способ вынуть команду: другая ревизия или возможности, которые её
  больше не допускают, — `Superseded`. **Хосты.** Каждый `apply` (Win32, AppKit, UIKit, Android,
  Web, winit, headless) открывает токен против возможностей последнего опубликованного снимка
  (`WindowStateCell::published_capabilities`), не своей константы: смена, которую снимок
  опубликовал (наблюдение свёртывания X11 у winit), даёт `Superseded`. Ветки «команда, которой хост
  не знает» — `ApplyError::Platform` (`window::state::not_carried_out`). Сверх строки: числа Win32
  за пределом устройства (`device_pair`) — теперь `Platform`, а не `InvalidGeometry`: допуск
  проверил конечность и положительность, предел — свойство Win32. **Вызывающие.** Очередь
  `flui-app` хранит `AdmittedCommand`; на `Superseded` она допускает команду заново против снимка
  на этом ходе и применяет её на месте (`window_control::apply_admitted`), отказ при повторном
  допуске пишется с причиной. Свои команды раннера (показ и активация главного окна, закрытие
  отброшенного окна, `request_close`) идут через `window_control::apply_now`; `CloseRequestError`
  разделён: `Refused { error: AdmissionError }` (Android) и новый `Failed { error: ApplyError }`.
  Закрытие при выходе цикла AppKit и харнесс-закрытие winit пишут отказ в журнал вместо `let _`.
  Headless: возможности — часть модели оконной системы, `MockWindow::simulate_capabilities`.
  Тестам и фикстурам — `flui_testing::window_commands::{AdmitAndApply, CommandRefusal}`
  (`admit_and_apply`, `close_now`: уже закрытое окно — норма, иной отказ — паника); около 80
  тестовых `let _ = window.apply(Close)` стали `close_now()`. **Conformance**
  (`flui_testing::window_conformance`): `capabilities_are_honest` — допуск отказывает
  `Unsupported` (не повторяемым) ровно при `Unavailable`, и окно применяет всё допущенное;
  `bad_numbers_are_refused` — допуском; закрытое окно отвечает `ApplyError::Closed`. Фикстура
  `RefusesForNow` невыразима (отказ по возможности больше не делает окно) и заменена на
  `ClaimsWhatItCannot` (снимок заявляет `ALL`, окно — `NONE`).
  Тесты (откат проверен, кроме оговорённого): `flui-platform` — trybuild
  `admitted_command_has_one_constructor` (с открытыми полями фикстура компилируется — тест падает)
  и `apply_takes_only_an_admitted_command` (E0308 на типе параметра; откат сигнатуры `apply` не
  прогонялся — он ломает весь крейт), `compile_fail` doctest на `PlatformWindow::apply` с голой
  командой; `the_capabilities_revision_moves_only_with_the_capabilities` и
  `a_token_opens_only_under_the_capabilities_it_was_admitted_against` (без штампа ревизии в
  `successor` падают оба, без сравнения ревизии в `against` — второй); строка `Close` в
  `admit_refuses_exactly_when_the_capability_is_unavailable`; `flui-native` —
  `a_command_admitted_before_a_capability_change_is_superseded` (`window_state_contract`; падает и
  без штампа, и без сравнения ревизии); `flui-app` — `a_command_superseded_in_the_queue_is_admitted_again`
  (`window_handle_contract`; без повторного допуска в очереди и без штампа падает);
  `flui-testing` — `a_window_that_claims_what_it_cannot_do_is_caught` (если честность не применяет
  допущенное, первым ловит не `capabilities_are_honest`). `submit_reports_a_refusal_and_a_closed_window`
  ADR-0254 покрыт существующими `a_refused_command_is_never_queued` и `a_closed_window_answers_closed`.
  `module-dag` без изменений, `globals` без изменений. Документы: ARCHITECTURE.md `flui-platform`
  и `flui-native`, `changelog.d/platform-admitted-command.md`; `docs/platform-parity.md` и
  `docs/gaps.md` не тронуты — поведение платформ не изменилось.
  Проверки (Windows): `cargo fmt`; clippy `-D warnings` рабочего пространства `--all-targets`
  обычный и с `flui-native/winit-backend,flui/native`; nextest `flui-platform`, `flui-native`,
  `flui-app`, `flui-testing`, `flui-view`, `flui-runtime`, `flui-interaction`, `flui-sdk` — 695 из
  696 (16 пропущены), `flui-native` с `winit-backend` и `flui-app` — 361 из 362 (оба раза падает
  только trybuild `owner_thread_token_has_no_default` с `main`), `flui --features native` — 10;
  trybuild `flui-platform` — 3 из 3; clippy Android `-p flui-native -p flui-app --all-targets`,
  macOS `--all-targets` (`flui-native`, `flui-app`, `flui-platform`, `flui-testing`; clang и
  llvm-ar LLVM) обычный и с `winit-backend`, iOS lib `flui-native` (`DOCS_RS=1`; `flui-app` под
  iOS не собирается: `psm` без `cc`), wasm32 lib `flui-native`, `flui-app`; `checks`,
  `doc-strict`, `wasm-check`, `wasm-test` — 51. WSL Ubuntu 24.04 (`WAYLAND_DISPLAY` и `XDG_SESSION_TYPE` сняты): clippy четырёх
  крейтов `--all-targets`, с `winit-backend` и `flui-native --all-features`; nextest под xvfb без
  признаков — 443 (1 пропущен), с `winit-backend` — 341 (1), `--all-features` — 296; rustdoc
  `-D warnings --document-private-items` без признаков и с `winit-backend`. Без xvfb (дисплей WSLg
  с оконным менеджером) X11-тест позиции открытия видит рамку менеджера — окружение, не код.
  Не запускалось: Mac и iPhone — в очереди vanyastaff/flui#1840;
  `cross-typecheck` (нет `zig`); браузер — у координатора: `WindowCommand::Close` холста и
  полноэкранный режим через допуск ведут себя как прежде.
  **Исправления после ревью (vanyastaff/flui#1914).** (1) Закрытие из `Drop` и уборки не паникует:
  `AdmitAndApply::close_for_cleanup` пишет отказ в журнал (уже закрытое окно — норма), `close_now`
  остался там, где отказ — проваленный тест, и его doc отсылает к `close_for_cleanup` и
  `admit_and_apply`; `ClosePointerTarget` (`contract.rs`), `CanvasWindow` и `HostedCanvas`
  (`web_browser.rs`, теперь всегда доходит до `owner.quit()`) закрывают через неё, TSF сначала
  проверяет список упавших случаев, потом закрывает. Тест `window_commands` (`flui-testing`):
  `a_refused_close_fails_the_test_only_where_asked_to`,
  `a_cleanup_while_unwinding_keeps_the_first_failure` (охрана, чьё закрытие отказано, в `Drop`
  при раскрутке; с паникой в `close_for_cleanup` бинарник падает с ABORT),
  `a_closed_window_is_closed_for_both`. (2) Очередь `flui-app` не теряет работу за отказанным
  закрытием: `close_with_veto` заканчивает пачку только на `Ok`, `Failed { Closed }` и
  исчезнувшем окне; `Refused` и прочие `Failed` пишутся в журнал, остальное применяется. Тест
  `a_refused_close_keeps_the_work_queued_behind_it` (`window_handle_contract`) падает, если
  `close_with_veto` снова отвечает `true`. (3) Ревизию проверяет conformance хоста: новая
  проверка `a_command_admitted_before_a_revision_is_superseded` и
  `WindowFixture::revise_capabilities` (по умолчанию `false` — проверка пропускается; headless
  переключает `places_at_open` через `simulate_capabilities`). Фикстура
  `Fault::IgnoresTheToken` (применяет то, что окно допускает сейчас) ловится только ею
  (`a_window_that_ignores_the_token_is_caught`); если headless сверяет токен со своей
  константой, падает всё conformance. Без проверки ревизии в `against` (только владелец)
  падают `a_command_superseded_in_the_queue_is_admitted_again`,
  `a_command_admitted_before_a_capability_change_is_superseded`,
  `a_token_opens_only_under_the_capabilities_it_was_admitted_against` и conformance headless.
  (4) Токен привязан к окну: `CapabilitiesOwner` (`NonZeroU64`) в `WindowCapabilities`,
  ставится `WindowStateCell::new` из одного счётчика процесса
  (`window::state::NEXT_CAPABILITIES_OWNER`, `try_update` с `checked_add`, новая запись `globals`
  по ADR-0097), `successor` его сохраняет; `against` сравнивает владельца и ревизию. Токен
  другого окна и токен, допущенный против возможностей, собранных руками (`ALL`, `NONE`), —
  `Superseded`. Тесты `a_token_opens_only_on_the_window_it_was_admitted_for` (`flui-platform`) и
  `a_command_admitted_for_another_window_is_superseded` (`flui-native`): без сравнения владельца
  падают оба. `AdmittedCommand::revision` удалён (его не читал ни один путь);
  `WindowCapabilities::revision` читают тесты трёх крейтов и `against`. (5) Один путь
  «допустить и применить»: `WindowCommand::admit_and_apply(self, &W) -> Result<(),
  CommandRefusal>` в `flui-platform` (при `Superseded` допускает ещё раз); `CommandRefusal` один,
  там же. Им пользуются `request_close`, показ главного окна, `discard_window`, очередь (на
  `Superseded`), закрытие при выходе AppKit, харнесс winit и `AdmitAndApply` `flui-testing`;
  `apply_now` и `CommandRefusal` `flui-app` удалены (в `flui-runtime/src/testing.rs` — двойник
  `apply`, не допуск). (6) Типы ошибок: `ApplyError::Platform(PlatformFailure)`,
  `PlatformFailure { OutOfRange, Failed { message } }`; Win32 за пределом устройства —
  `OutOfRange` (с `Failed` падают `geometry_win32_cannot_hold_is_refused` и
  `a_position_beyond_the_screens_does_not_overflow`); `loop_control.rs` и харнесс winit пишут
  `CommandRefusal` как есть. (7) Без клона: `ApplyError::Superseded { command }` отдаёт команду
  назад, очередь допускает её заново (`command.admit_and_apply(window)`). `ApplyError`,
  `CommandRefusal` и `CloseRequestError` больше не `Eq` (`WindowCommand` несёт `f64`).
  (8) Контракт `against` — тождество (владелец и ревизия); проверка `permits` удалена: в одном
  окне одна ревизия — один набор возможностей. `.stderr` `admitted_command_has_one_constructor`
  перегенерирован (третье закрытое поле `owner`).
  Проверки после исправлений: `cargo fmt`; clippy `-D warnings` рабочего пространства обычный и с
  `flui-native/winit-backend,flui/native`; nextest восьми крейтов — 696 из 697, `flui-native` с
  `winit-backend` и `flui-app` — 361 из 362 (оба раза падает только trybuild
  `owner_thread_token_has_no_default` с `main`), `flui --features native` — 10; trybuild
  `flui-platform` — 3 из 3; clippy Android `--all-targets`, macOS `--all-targets` обычный и с
  `winit-backend`, iOS lib, wasm32 lib; `checks` (`globals` — 57), `doc-strict`, `wasm-check`, `wasm-test` — 51.
  WSL: clippy без признаков, с `winit-backend` и `--all-features`; nextest под xvfb — 444, 341,
  296; rustdoc обоих вариантов. Не запускалось: Mac и iPhone
  (vanyastaff/flui#1840), `cross-typecheck`, браузер.
- [x] **LY44** — ветка `platform/window-handler`, начата на вершине LY42 `9ae010429`, после слияния
  vanyastaff/flui#1914 перенесена на `main` @ `620d37f82`. **Контракт.** `flui-platform`:
  `WindowHandler` (`!Send`), шесть обязательных методов по обязательствам — `input` →
  `DispatchEventResult`, `state_changed`, `frame_requested(FrameRequest)`, `close_requested` →
  `CloseDecision { Close, KeepOpen }`, `closed`, `surface_changed(SurfaceChange { Release,
  Rebuild })`; каждый получал `OwnerThread` — доказательство потока владельца (после ревью удалено,
  пункт 1 ниже). `PlatformWindow` потерял `on_input`,
  `on_state_changed`, `on_request_frame`, `on_close`, `on_should_close`,
  `on_surface_status_change`. **Ядро.** `open_window(options, Rc<dyn WindowHandler>)` у `Platform`,
  `OwnerPlatform` и `Window::open` каждого хоста; `HostPlatform` отдаёт снимок возможностей первым
  `input` до возврата из открытия (winit — в конце `create_window_now`, iOS — в `create`).
  `WindowCallbacks`, `impl_window_callback_setters!`, инертные умолчания, очередь раннего ввода,
  снимок «ещё не зарегистрирован» (ADR-0213 §3) и защёлка `clear()` (ADR-0063 §5) удалены; вместо них
  `owner/window_delivery.rs` — `WindowDelivery`: обработчик в `OwnerSlot`, причинная FIFO окна,
  вызов через точку вызова LY26 (клон `Rc` на время вызова, обработчик остаётся на месте и может быть
  вызван изнутри себя). Доставка изнутри метода ждёт его конца, вложенный ввод отвечает отложенной
  формой (`DEFERRED`, клавиша — `DEFERRED_UNHANDLED`), вложенный `close_requested` — `KeepOpen` без
  вопроса; паника метода не теряет хвост очереди — он доставляется, каждый в своей защите, первая
  паника возобновляется после; паника на снимке сдерживается, ввод за ним доставляется; `closed` —
  последнее обязательство; `clear` встаёт в очередь, если она осушается (закрытие изнутри метода), и
  клон, оказавшийся последним, уходит в точку вызова, которая роняет его вне локов. Запечатанный
  `HostWindow` отдаёт доставку (`Sealed::delivery`). Отложенное открытие:
  `PlatformProxy::open_window(WindowRequest)`, `WindowRequest { options, handler:
  WindowHandlerFactory }` — `Send`-фабрика, которую `build_on_owner` выполняет на владельце (паника
  фабрики — `OpenWindowError::Backend`, полезная нагрузка удерживается); блокирующее открытие winit
  не с потока владельца — `Unavailable` (обработчик `!Send`). iOS спрашивает обработчик окна сеанса у
  потребителя сцены до создания окна; ошибка — сцена отклонена. **`flui-app`.** Каждое окно
  открывается с `WindowRoutes` (`runner/window_routes.rs`): маршрут каждого обязательства
  ставится, когда появляется представление; ввод до маршрута отвечает результатом по умолчанию,
  возможности сохраняются и отдаются маршруту ввода; `closed`, услышанное раньше маршрута, запускает
  его сразу; маршрут, который запаниковал, остаётся. `RouteBook` в `AppRuntime` по `WindowId`.
  Регистраций колбэков нет. `AppConfig::input_latency` (`with_input_latency`) попадает в
  `WindowOptions`, их читают Web и Android. **Тестовый набор.** `flui_testing::window_handler` —
  `RecordingWindowHandler` (`react_to_*`, `calls`, `inputs`, `count`) и `HandlerCall`; открытия
  окон в тестах всех крейтов перешли на него.
  Тесты (откат проверен): `window_handler_obligation_table` (`flui-native`, юнит) — без доставки
  хвоста после паники, без отложенного ответа вложенному вводу, с `true` на вложенный вопрос
  закрытия, без проверки `closed` падает; `input_capabilities_contracts` —
  `the_first_event_after_open_reaches_the_handler` (без снимка при открытии падают он и ещё пять
  строк), `a_panic_on_the_snapshot_loses_no_input` (снимок через `or_resume` вместо сдерживания),
  `a_native_event_during_a_callback_is_queued_not_dropped` (ответ по умолчанию вместо `DEFERRED`),
  `input_raised_from_input_is_delivered_after_it`; `handler_reentry` — каждый из шести методов
  открывает окно, меняет своё и завершает приложение изнутри вызова (headless; Win32 — пять, без
  `surface_changed`), `a_handler_dropped_at_close_reaches_the_platform`: с локом состояния окна,
  удержанным на `dispatch_close`, headless-случай виснет и падает по таймауту;
  `panics_in_callbacks_and_tasks_leave_the_dispatcher_usable` — запрос за упавшим кадром доставлен,
  вложенное закрытие отвечает `KeepOpen`, закрытие из упавшего метода завершается и окно отпускает
  обработчик (без доставки хвоста и с `Some(handler)` в `Shared::restore` падает);
  `window_routes_contract` (`flui-app`; без немедленного запуска услышанного `closed` падает);
  строка `the_input_latency_is_kept` в `config_to_options_keeps_every_field` (без переноса из
  конфига падает); trybuild `ui_pass/window_handler_holds_rc.rs`. Удалены строки, чьё состояние
  сочли невыразимым: Win32 `deadline_window_without_frame_callback_does_not_spin` и
  `deadline_hidden_window_without_frame_callback_does_not_spin` (после ревью возвращены в новой
  форме, пункт 3 ниже), строки очереди раннего ввода и
  замены колбэка ввода. `globals` — 57, `module-dag` `flui-native` — 300 модулей, 323 ребра.
  Документы: ARCHITECTURE.md `flui-platform` и `flui-native`, README `flui-native`,
  `docs/architecture.md`, `docs/gaps.md` (имя метода), `changelog.d/platform-window-handler.md`;
  `docs/platform-parity.md` не тронут — поддержка платформ не изменилась.
  Проверки (Windows): `cargo fmt`; clippy `-D warnings` рабочего пространства `--all-targets`
  обычный и с `flui-native/winit-backend,flui/native`; nextest `flui-platform`, `flui-native`,
  `flui-app`, `flui-testing`, `flui-view`, `flui-runtime`, `flui-interaction`, `flui-sdk` — 702 из
  703 (16 пропущены), `flui-native` с `winit-backend` и `flui-app` — 367 из 368 (оба раза падает
  только trybuild `owner_thread_token_has_no_default` с `main`), `flui --features native` — 10;
  trybuild `flui-platform`; clippy Android `-p flui-native -p flui-app --all-targets`, macOS
  `--all-targets` (clang и llvm-ar LLVM) обычный и с `winit-backend`, iOS lib `flui-native`
  (`DOCS_RS=1`), wasm32 lib четырёх крейтов; Linux кросс-clippy с Windows (clang LLVM)
  `--all-targets` пяти крейтов без признаков, с `winit-backend` и `flui-native --all-features`,
  rustdoc `--document-private-items -D warnings` Linux обоих вариантов и Windows; `checks`,
  `doc-strict`, `wasm-check`, `wasm-test` — 51. WSL в первом прогоне не запускался (вызов `wsl`
  был не той формы); его догнал прогон ниже.
  **Исправления после ревью (vanyastaff/flui#1915)**, слиянием, без перебазирования; `main` с
  vanyastaff/flui#1916 влит (`79cd14897`).
  (0) **X11 под xvfb (CI `test`, `--all-features`).** Команда, применённая с рабочего потока
  (`x11_window_takes_commands_and_publishes_what_the_server_holds`), публиковала снимок на том
  потоке, а обработчик живёт в owner-слоте — `state_changed` скрытия терялся. Доставка с чужого
  потока теперь встаёт в FIFO окна и просит ход владельца (`WindowDelivery::resume_through`,
  подключено в winit и headless; без него — ждёт следующей доставки владельца, с
  предупреждением); ввод оттуда отвечает `DEFERRED`. Строка
  `a_delivery_raised_off_the_owner_thread_waits_for_an_owner_turn` (без постановки в очередь
  падает: «each one asked for an owner turn, left 1 right 2»).
  (1) **`OwnerThread` удалён, методы `WindowHandler` без аргумента-платформы.** ADR-0254 §4 даёт
  каждому методу `&OwnerPlatform`, но тот же ADR (раздел «Decision») велит контракту называть
  только свои типы и отдаёт `OwnerPlatform` хост-крейту (`flui-native`, ADR-0151 §9), а
  `flui-runtime`, который реализует обработчик, по ADR-0083 не имеет ребра к хосту: форму ADR не
  выполнить, не ломая слои. Решение для владельца — в отчёте координатору.
  (2) **Ничего до маршрута не теряется.** `WindowRoutes` держит весь ввод по порядку (ответ
  `DEFERRED`), последний `state_changed`, последний запрос кадра и последнюю смену поверхности и
  отдаёт их при установке маршрута; `close_requested` без маршрута — `KeepOpen`.
  `WindowRoutes::of` — `expect("BUG: …")` (кроме двойников окна в юнит-тестах). Строки
  `window_routes_contract`: `the_input_route_hears_everything_before_it_in_order` и
  `state_frame_and_surface_wait_for_their_routes` (без удержания ввода и без удержания
  поверхности падают соответственно).
  (3) **Спин дедлайна.** `frame_requested` отвечает `FrameOutcome { Produced, Declined }`;
  счётчик кадров Win32 растёт только от `Produced` (или паники). Строки
  `deadline_window_declining_its_frames_does_not_spin` и
  `deadline_hidden_window_declining_its_frames_does_not_spin` в `test_window_lifecycle_contract`
  (со счётом каждого вызова: 26 153 и 81 690 перевзводов за 300 мс) и строка
  `a_frame_counts_when_the_handler_runs`.
  (4) **`close_requested` в очереди.** `dispatch_close_request(close_later)`: при простое FIFO
  спрашивает сразу и возвращает ответ; во время метода или с чужого потока — `KeepOpen` сейчас,
  вопрос после метода, при `Close` окно закрывает отложенное закрытие хоста (Win32 — `CLOSE_WINDOW`
  с идентичностью, winit и headless — командой, AppKit — `close`); после одобренного запроса
  следующие в очереди не задаются. Строки: `a_close_request_is_the_handlers_to_decide`,
  `a close request raised inside input waits for it` в headless и Win32 таблицах
  `handler_reentry`, `a_nested_close_request_keeps_the_window` (без очереди падают все три
  таблицы; без пропуска после одобренного — счёт вопросов 2 вместо 1).
  (5) **Паника снимка возобновляется** после ввода, который он предварял (паника самого ввода
  сдерживается, не подменяет первую); iOS `dispatch_surface_immediate` возвращает панику, вызов
  доделывает `closed`/релиз или запуск кадров и возобновляет. Строки
  `a_panicking_snapshot_resumes_after_the_input_it_precedes` и
  `a_panic_on_the_snapshot_is_resumed` (с `or_contain` падают обе). iOS только типизирован.
  (6) iOS: маршруты сопоставляются по `IOSSceneSessionId` (теста нет: `flui-app` под iOS не
  собирается из-за `psm`); `Shared` → `ResidentHandler`; `SAFETY:` в Win32-строках называют
  инвариант. (7) ADR-0213 §3 и ADR-0063 §5 помечены `Superseded-by ADR-0254`, в заголовке
  ADR-0063 — защёлка `clear()`; ADR-0237 решение 2 — `state_changed` обработчика; `docs/gaps.md`
  (запись 9): Web и Android не спрашивают `close_requested`; changelog — форма `PlatformProxy`,
  `Unavailable` winit вне владельца, `FrameOutcome`, очередь закрытия, доставка с чужого потока.
  Проверки: Windows — clippy рабочего пространства обычный и с
  `flui-native/winit-backend,flui/native`; nextest восьми крейтов — 702 из 703, с `winit-backend`
  — 367 из 368 (падает только trybuild `owner_thread_token_has_no_default`, как на `main`);
  clippy Android `--all-targets`, macOS `--all-targets` обычный и с `winit-backend`, iOS lib,
  Linux кросс-clippy (обычный, `--all-features`); `checks` (`globals` — 57), `doc-strict`,
  `wasm-check` (теперь с `clippy -p flui-native --all-targets` для wasm32). WSL Ubuntu 24.04
  (`WAYLAND_DISPLAY`, `XDG_SESSION_TYPE` сняты): clippy `flui-native`, `flui-app`,
  `flui-platform`, `flui-testing` без признаков, с `winit-backend` и `flui-native
  --all-features`; nextest под xvfb `flui-native` — 217, с `winit-backend` — 300,
  `--all-features` — 301, `flui-app` с `winit-backend` — 47, всё зелёное. Не запускалось:
  Android-устройство, Mac и iPhone (vanyastaff/flui#1840), браузер, `cross-typecheck`.
  **Одобрение владельца (2026-10-27).** Оба отступления от ADR-0254 §4 одобрены: методы
  `WindowHandler` без `&OwnerPlatform` и `frame_requested -> FrameOutcome { Produced, Declined }`;
  ADR-0254 дополнен на месте (строка `Amended: 2026-10-27`, текст и таблица §4). Запрос закрытия
  до маршрута теперь не забывается: один флаг, маршрут спрашивается ровно один раз при установке,
  `Close` закрывает окно командой (`a_close_request_before_its_route_is_asked_once` в
  `window_routes_contract`; без флага падает). Доставка с чужого потока теперь будит владельца на
  всех хостах: Win32 — `DELIVER_QUEUED` окну по его идентичности, AppKit, UIKit, Android — сигнал
  владельца их цикла (`WindowDelivery::resume_through_signal`, им же пользуется headless).
  Тест Win32 `a_delivery_raised_on_another_thread_reaches_the_handler_with_no_other_input`
  (`window/windows/delivery_tests.rs`; без пробуждения падает: «the state change never reached
  the idle window's handler»); AppKit, UIKit, Android только типизированы. Один прогон
  `flui-native` уронил `native_window_commands` (Win32, фокус), три повтора и полный перезапуск
  зелёные.
- [x] **LY49** — ветка `platform/host-only-core`, начата на вершине LY41
  (`origin/platform/snapshot-consumers` @ `87977afc3`), после слияния vanyastaff/flui#1912 перенесена
  на `main` @ `c50202c7a` (дерево то же). **В ядро, `pub(crate)`.** Модель `drag/offers.rs`
  (теперь без `cfg`, элементы — под `cfg` своих потребителей): `OfferTable` (слот хранит
  `Arc<[RepresentationDescriptor]>`; `len`/`is_empty` без потребителя удалены), `TransferCompleter`
  (`channel()` и `complete`; `is_cancelled` никто не звал — удалён) и `TransferUri`. Модель
  `input/regions.rs`: `DeviceInputRegion`, `ProjectedInputRegions` (`sent()` — только тестам);
  тесты проекции переехали `git mv` из `flui-platform/tests/input_regions.rs` в
  `input/regions/tests.rs` (правки — пути и импорты). **В контракте.** `TransferRequest::pending(ClaimHandle)`
  вместо `channel()`: слот `claim_slot` держит хост, его обёртка — `TransferCompleter` ядра.
  **Удалено.** `KeyText` (`Key::Character(String)`, непустоту держит `Key::character`);
  координатные помощники — ни одного вызова, перенос дал бы мёртвый код; `OfferRecord` —
  его `payload` читал один winit, теперь замороженный список держит сессия winit
  (`frozen: Option<TransferPayload>`); `TransferImage` и варианты `TransferPayload::{UriList, Image}`
  — выше хоста их никто не читал (вариант уходит вместе с типом). **Решения сверх строки.**
  (1) Раз `TransferUri` в ядре, список URI доходит до приложения как `Custom` типа `text/uri-list`
  (RFC 2483, строки через CRLF, локальный путь — `file:` URI по RFC 8089 с процент-кодированием;
  диск `C:\a` ↔ `file:///C:/a`, UNC `\\h\s\a` ↔ `file://h/s/a`, относительный путь пропускается),
  изображение — `Custom` своего контейнера. Хост пишет список (`TransferUri::list_payload`:
  `CF_HDROP` Win32, файлы winit, URI клипа Android) и читает отданный источником
  (`TransferUri::list_of`: `CF_HDROP` наружу, клип Android); Web отдаёт строку браузера как есть.
  (2) `owner/headless` больше не называет таблицу предложений (база не называет фичу): цикл держит
  её через хук `SimulatedOffers`, который делает таблица (`HeadlessHost::offers`), как
  `SimulatedDevices`; реализация — `drag/headless.rs`. (3) `MockWindow::input_region_updates` и
  `HeadlessWindow::input_region_updates` (`flui-testing`) отдают пары (вид, прямоугольник в
  пикселях устройства); тестовый хост `flui-testing` проецирует сам (`device_rect_covering` и тот же
  ключ сравнения), ядро ему недоступно. Тестовые двойники выше хоста (`flui-interaction`
  `drag_sessions`, `flui-widgets` `anchored_drag`/`nested_scroll`, `flui-app`
  `data_transfer_transport`, `flui-platform` `drag_plan`/`transfer_request`) чеканят id сами
  (`DataTransferId::new_gen`, счётчик без повторов) и завершают запросы через `claim_slot`.
  Пересечение с LY42: одна строка документации `PlatformWindow::set_input_regions`
  (`platform_window.rs`). **Тесты** (`drag::offers::tests`): `a_uri_list_is_delivered_as_file_uris`,
  `a_served_uri_list_reads_as_paths_and_uris`, `a_retired_offer_never_aliases_the_next`.
  Откат проверен: с `file://{path.display()}` вместо кодирования падают первые два; без разбора
  `file:` в `list_of` — `a_served_uri_list_reads_as_paths_and_uris`,
  `drag::windows::crossing::tests::ole_drag_drop_contracts` и `ole_drag_and_drop_on_real_windows`;
  без сдвига поколения — `a_retired_offer_never_aliases_the_next`. Существующие drag-тесты
  (headless, OLE, HTML, Android, winit) зелёные, утверждения о типизированных `UriList`/`Image`
  переписаны на байты. `module-dag`: 1 исключение, без изменений. Документы: ADR-0038 (словарь,
  `pending`, место таблицы), ARCHITECTURE.md ядра (модели `drag::offers`, `input::regions`,
  `HeadlessHost::offers`), `flui-platform`, `flui-rendering`;
  `changelog.d/platform-host-only-core.md`; `platform-parity`/`gaps` без изменений (поддержка по
  платформам та же).
  Проверки (Windows): `cargo fmt`; clippy `-D warnings` рабочего пространства `--all-targets`
  обычный и с `flui-native/winit-backend,flui/native`; nextest `flui-native`, `flui-platform` —
  324, `flui-native`, `flui-app` с `winit-backend` — 366 (2 пропущены; оба раза падает только
  trybuild `owner_thread_token_has_no_default`, как в LY40/LY41), `flui-interaction`, `flui-app`,
  `flui-testing`, `flui-widgets` — 351 (3), `flui` с `native` — 10; doctest'ы `flui-platform`,
  `flui-native`; clippy Android `-p flui-native -p flui-app --all-targets`, wasm32 lib, iOS lib
  (`DOCS_RS=1`), macOS `--all-targets` обычный и с `winit-backend`; `wasm-check`, `wasm-test` — 51;
  `checks` (с `key-vocabulary`, `recording-vocabulary`, `markers`, `testing`), `doc-strict`,
  `module-dag`, `workspace`, `reach`. WSL Ubuntu 24.04 (`WAYLAND_DISPLAY` и `XDG_SESSION_TYPE`
  сняты, cargo без heli — в WSL его нет): clippy `-p flui-native -p flui-platform -p flui-testing
  -p flui-app --all-targets` обычный и с `winit-backend`; nextest `FLUI_HEADLESS=1` (те же крейты и
  `flui-interaction`) — 580, падает только
  `x11_window_takes_commands_and_publishes_what_the_server_holds`: без xvfb он ходит в X-сервер
  WSLg, который не двигает окно (кода окна ветка не трогает); под xvfb `--all-features`
  `flui-native` — 300 из 300; rustdoc `-D warnings --document-private-items` без признаков и с
  `winit-backend`. Не запускалось: `flui-app` под macOS и iOS, Mac и iPhone — в очереди
  vanyastaff/flui#1840; `cross-typecheck` (нет `zig`); Android-устройство (поведение drop на
  Android меняется только формой списка URI, тест `drag::android::clip::tests` на хосте); браузер —
  у координатора: drop файла или ссылки на холст отдаёт `Custom` `text/uri-list` строкой браузера,
  исходящий drag со списком URI кладёт ту же строку в `DataTransfer`.
  **Исправления после ревью (vanyastaff/flui#1913).** Правка ADR-0038 на месте (без `UriList` и
  `Image`, вместо них `Custom` `text/uri-list` и закодированные байты изображения) одобрена
  владельцем 2026-10-27. (1) `TransferRequest::pending()` возвращает пару с `TransferCompleter`:
  ручка, взятая до запроса, больше невозможна (паника на ошибку вызывающего ушла), и
  `TransferCompleter` со `is_cancelled` остался в контракте — четыре ручные копии (ядро,
  `transfer_request.rs`, `data_transfer_transport.rs`, `drag_sessions.rs`) свёрнуты в него.
  Тест — `compile_fail` doctest у `pending` (ручку в него не передать); с прежней подписью
  `pending(handle)` он падает. (2) `DeviceInputRegion` — снова данные контракта, с единственной
  проекцией `DeviceInputRegion::project` (`NotProjected`: пусто, не конечно, вне `i32`); её зовут
  `ProjectedInputRegions` ядра (и логирует) и тестовый хост; `input_region_updates` обоих отдают
  `DeviceInputRegion`. Тест `the_headless_window_drops_input_regions_no_host_could_send`
  (`flui-testing`, `headless_host`): с прежней проекцией тестового хоста падает. (3) `file:` URI:
  написание пути — параметр `PathStyle` (`Windows` у `CF_HDROP`, `Posix` у Android, `HOST` у
  winit), так что Android и Linux не пишут `\\host\share`, а обратная косая в имени POSIX
  экранируется; `\\?\C:\…` и `\\?\UNC\…` пишутся как путь, который они называют; `%` берёт ровно
  две шестнадцатеричные цифры; путь не в Unicode (непарный суррогат, не-UTF-8) не пишется, как и не
  читается. Таблицы: `a_path_is_written_as_its_file_uri`, `a_served_file_uri_reads_as_its_path`,
  `a_written_path_reads_back`, `a_path_that_is_not_unicode_is_left_out`. Откат проверен: с
  `from_str_radix` падает строка `%+1`; с POSIX, написанным как Windows, — строка `a\b`; без
  веток `\\?\` — `\\?\C:\`; с `to_string_lossy` вместо `to_str` — тест суррогата. (4) `KeyText`
  возвращён (закрытое поле, строит только `Key::character`); `compile_fail` doctest
  `Key::Character(String::new())` падает с `Character(String)`. (5) ADR-0038: ADR-0151 в
  `Amended by:`, `TransferCompleter::is_cancelled` снова наблюдает отмену; ADR-0151 получил запись
  «Amended: 2026-10-27» — `TransferCompleter`, `DeviceInputRegion` и `KeyText` остаются в
  контракте. (6) Строка «Чего нет в этой серии»: таблицы предложений переехали (в `docs/gaps.md`
  такой строки нет). (7) `OsDragProvider` и `OsDragEndHook` (ADR-0151 §7) не названы ни в одной
  строке `tasks.md` — владельца нет, не тронуты.
  Проверки: clippy `-D warnings` рабочего пространства `--all-targets` обычный и с
  `flui-native/winit-backend,flui/native`; nextest `flui-native`, `flui-platform`, `flui-app`,
  `flui-testing`, `flui-interaction`, `flui-widgets` с `winit-backend` — 713 (4 пропущены; падает
  только trybuild `owner_thread_token_has_no_default`); doctest'ы `flui-platform`, `flui-native` —
  61; clippy Android `--all-targets`, wasm32, iOS lib, macOS `--all-targets` обычный и с
  `winit-backend`; `wasm-test` — 51; `checks`, `doc-strict`, `module-dag`, `workspace`, `reach`.
  WSL: clippy обычный и с `winit-backend`; nextest `FLUI_HEADLESS=1` — 582 из 583 (тот же
  `x11_window_takes_commands_and_publishes_what_the_server_holds` против X-сервера WSLg), под
  xvfb `--all-features` — 302 из 302; rustdoc с закрытыми элементами обычный и с `winit-backend`.
  Не запускалось: Mac и iPhone (vanyastaff/flui#1840), `cross-typecheck`, Android-устройство,
  браузер.
- [x] **LY43** — ветка `platform/frame-verbs`, от `main` @ `620d37f82` (LY42). **Контракт.**
  `PlatformWindow::set_frame_rate_hint(FrameRateHint)`, `set_input_regions(&[InputRegion])` и новый
  `set_material(WindowMaterial)` возвращают `Result<(), Unsupported>` и не имеют умолчаний (все
  внешние реализации, около 20 двойников тестов, пишут их сами; обёртки делегируют). Это глаголы, не
  `WindowCommand` (ADR-0254 п. 3, вариант 8 отвергнут). `WindowCapabilities` получил
  `frame_rate_hint` и `material` (`honoured` — 18 полей); регионы ввода отвечают
  `InputCapabilities::input_regions`, как было. `WindowBackgroundAppearance` (с `MicaBackdrop`,
  `MicaAltBackdrop`, `Transparent`, `Blurred`) и `set_background_appearance` удалены (их не звал ни
  один путь). **Набор материалов — решение PR:** `WindowMaterial { Opaque, System }` (закрыт с
  причиной: новый материал — работа каждого хоста). `System` — материал ОС для фона долгоживущего
  окна (Mica на Windows 11); `Opaque` — без материала. Меньший честный набор: каждый хост, у
  которого материал не `Unavailable`, рисует оба; Mica Alt и Acrylic остаются в
  `flui::native::windows` (`Win32Window::set_backdrop`), материалы `NSVisualEffectView` и Liquid
  Glass — в `flui::native::macos`. **Хосты.** Win32: `WindowSet` читает сборку один раз
  (`WindowsPlatform::at_build`, `new` — с `WindowsBuild::read()`), шлюз `SystemBackdrop`
  (`Since<F>`, сборка 22621, чистый модуль `window::windows::material`) решает и возможность, и
  дойдёт ли глагол до DWM; ниже порога и при отказе `DwmSetWindowAttribute` — `Unsupported` с
  `NotOnThisDevice` вместо записи в журнал; закрытое окно — `NoWindow`. Возможности окна Win32
  теперь несёт `OwnerTracking` (контекст окна хранит их с открытия). Окно Win32 по-прежнему
  открывается на Mica, теперь через тот же шлюз (`apply_windows_features` без ручных ординалов);
  частота кадров и регионы ввода — `NotOnThisPlatform`. Android: `HostDisplays::takes_frame_rate_requests`
  (разрешился ли `ANativeWindow_setFrameRate`) даёт `frame_rate_hint` в возможностях окна и
  платформы; ниже — `NotOnThisDevice`; регионы ввода до API 29 — `NotOnThisDevice`. iOS: регионы
  ввода как были (закрытое окно — `NoWindow`, не главный поток — `Unavailable`), частота и
  материал — `NotOnThisPlatform`. macOS, Web, winit: все три — `NotOnThisPlatform`; материал macOS
  через контракт не подключён (без прогона на Mac и с ошибками `NotOnTheMainLane`/`ForeignContentView`,
  которых `Unsupported` не выразит) — запись 20 в `docs/gaps.md`. Headless: глаголы отвечают
  опубликованным возможностям (снимок берётся до замка модели) и доступности ввода
  (`HeadlessWindowSystem::input_regions`), записи — `frame_rate_hints()`, `materials()`.
  **Вызывающие.** Presentation (`hand_input_regions`) и хост дисплеев `flui-app` (`request_rate`)
  пишут отказ в `trace`: возможность уже сказала то же. **Conformance.** Новая проверка
  `per_frame_verbs_report_unsupported_exactly_when_unavailable` (имя из подтверждения ADR-0254) и
  обязательный `WindowFixture::input_regions`; headless — ещё набор «без регионов ввода», Win32 —
  таблица `input::availability::windows`.
  Тесты (откат проверен для каждого): `system_backdrop_gate_table` (с `Native` ниже порога
  падает); `window_material_on_win32` — живое окно на этой машине (Windows 11, сборка 26300),
  readback — атрибут DWM: `below_the_gate_a_material_is_not_on_this_device` (сборка 22000
  внедрена: возможность `Unavailable`, ответ `NotOnThisDevice`, атрибут не тронут; падает, если
  набор читает настоящую сборку), `from_the_gate_on_each_material_reaches_the_desktop_window_manager`
  (открыто на `DWMSBT_MAINWINDOW`, затем `Opaque`→`NONE`, `System`→`MAINWINDOW`, `Opaque`→`NONE`,
  после закрытия `NoWindow`; падает, если глагол отвечает `Ok` без вызова DWM),
  `a_refusal_of_the_desktop_window_manager_is_not_on_this_device` (падает с прежним проглатыванием
  отказа); `flui-testing` — `a_window_whose_verbs_ignore_their_capabilities_is_caught` (без новой
  проверки падает), а с прежним инертным headless падает
  `the_headless_window_conforms_under_every_capability_set`. `.stderr`
  `platform_and_host_window_are_sealed` перегенерирован (сдвиг строк). `module-dag`: 1 исключение,
  без изменений; `globals` — 57. Документы: ARCHITECTURE.md контракта и ядра, `docs/platform-parity.md`
  (заметки без смены ячеек: Windows остаётся 🟡 — прозрачности нет), `docs/gaps.md` (запись 20),
  `changelog.d/platform-frame-verbs.md`.
  Проверки (Windows): `cargo fmt`; clippy `-D warnings` рабочего пространства `--all-targets`
  обычный и с `flui-native/winit-backend,flui/native`; nextest `flui-platform`, `flui-native`,
  `flui-testing`, `flui-app`, `flui-runtime`, `flui-interaction` — 643 (2 пропущены), с
  `winit-backend` (`flui-native`, `flui-app`) — 370; оба раза падает только trybuild
  `owner_thread_token_has_no_default` с `main`; фасад с `native` — 10; doctest'ы контракта и ядра;
  clippy Android `-p flui-native -p flui-app --all-targets`, wasm32 lib, iOS lib (`DOCS_RS=1`),
  macOS `--all-targets` (`flui-native`, `flui-app`, `flui-platform`, `flui-testing`; clang и
  llvm-ar LLVM) обычный и с `winit-backend`; `checks`, `doc-strict`, `wasm-test` — 51.
  WSL Ubuntu 24.04 (`WAYLAND_DISPLAY` и `XDG_SESSION_TYPE` сняты): clippy четырёх крейтов
  `--all-targets`, с `winit-backend` и `flui-native --all-features`; nextest `FLUI_HEADLESS=1` — 450
  из 451 (прежний `x11_window_takes_commands_and_publishes_what_the_server_holds` против X-сервера
  WSLg), под xvfb `--all-features` — 303 из 303; rustdoc `-D warnings --document-private-items`
  без признаков и с `winit-backend`.
  Не запускалось: Mac и iPhone (vanyastaff/flui#1840), `cross-typecheck` (нет `zig`),
  Android-устройство (частота кадров меняет только форму ответа), браузер.
  **Исправления после ревью (vanyastaff/flui#1917).** Набор материалов `{ Opaque, System }`
  утверждён владельцем. Слиты `main` @ `dcb188cb8` (LY44: `open_window(options, handler)`,
  `WindowDelivery` вместо `WindowCallbacks`) и `6b45a8bb5` (#1919–#1921, AppKit conformance) —
  слиянием. (1) Материал — команда, как решает ADR-0254 п. 3: `WindowCommand::SetMaterial`,
  допуск по `WindowCapabilities::material`, отказ ОС — `ApplyError::Platform`; глагол
  `set_material` удалён, `set_frame_rate_hint` и `set_input_regions` остались глаголами.
  Новое в контракте: `WindowCapabilities::unavailable_on_this_device` и `refusal` — возможность,
  которой нет у этого устройства, допуск отказывает с `NotOnThisDevice` (Win32 ниже 22621), прочие
  — `NotOnThisPlatform`; `WindowHandle::set_material` (`flui-view`) — путь виджета через очередь
  окна. (2) В документации `WindowCapabilities`: `Unavailable` ровно тогда, когда допуск
  отказывает; отказ ОС после допуска — сбой, не отсутствие. iOS `set_input_regions` с любого
  потока: края доходят до корневого контроллера через главную очередь (`StatusBar::defer_edges`,
  контроллер читает последние края при обновлении, так что ранний блок не возвращает старые).
  (3) Тест отказа DWM: инъекция исхода DWM (`material_applied`) — отказ даёт `ApplyError::Platform`;
  закрытое окно — `ApplyError::Closed` (команда, допущенная до закрытия). (4) Android: чистая
  модель `display::android::sample::FrameRateRequests` (разрешение `ANativeWindow_setFrameRate`
  → `Availability`), хук `HostDisplays::frame_rate_hint`. (5) Заглушки двойников —
  макрос `flui_platform::window_takes_no_frame_verbs!` (модуль `testing`, ADR-0243): шесть
  двойников и fixture trybuild. (6) Один шлюз `DWMWA_SYSTEMBACKDROP_TYPE`: команда, открытие на Mica,
  `Win32Window::set_backdrop` и `backdrop` берут доказательство `Since<SystemBackdrop>`, ниже
  шлюза вид Win32 отвечает `NotOnThisDevice`. (7) Документация `request_rate`/`hint_for`.
  (9) ADR-0151 и ADR-0082 называют `WindowCommand::SetMaterial` вместо живого
  `WindowBackgroundAppearance` (учёт решения ADR-0254 п. 3). (10) macOS: `System` —
  материал `windowBackground` `NSVisualEffectView` за видом FLUI, `Opaque` — `clear_backdrop`,
  возможность `Native` для окон главной полосы; закрытое окно — `Closed`, прочие ошибки AppKit —
  `ApplyError::Platform`. Комментарий владельца после прогона на Mac (M1, macOS 27, 2026-10-27):
  `WindowFixture::input_regions` добавлен в AppKit-фикстуру `tests/appkit_window.rs` (#1921 уже в
  `main`), там же readback материала и окно с обработчиком (фикстура звала `open_window` с одним
  аргументом); iOS `set_frame_rate_hint` идёт через `CADisplayLink` окна (`High` — лучшая частота
  экрана, иначе без предпочтения; чистое правило `link_rate::frame_rate_preference` с подсказкой),
  возможность `Native`; macOS `set_frame_rate_hint` остаётся `Unavailable` — кадры хоста идут не от
  display link — и отвечает `NotOnThisDevice`, а не `NotOnThisPlatform`; вариант причины «этот хост
  не делает» в контракте отсутствует — вопрос владельцу. Conformance: `SetMaterial` в образцовых
  командах, `Readback::material` и сходимость материала.
  Тесты (откат проверен для каждого): `system_backdrop_gate_table` (без
  `unavailable_on_this_device` ниже шлюза и с причиной, игнорирующей устройство, падает);
  `window_command_validate` (строка `SetMaterial` и проверка `NotOnThisDevice` на каждую команду;
  падает при причине без устройства); `window_material_on_win32` — `below_the_gate_…` (падает,
  если набор читает настоящую сборку и если `set_backdrop` без шлюза),
  `from_the_gate_on_…` (падает, если Win32 игнорирует `SetMaterial`), `a_refusal_…_is_a_platform_failure`
  (падает при проглатывании отказа); `the_win32_window_conforms` (сходимость материала: падает
  при игнорировании); `the_frame_rate_capability_follows_the_request` (Android, падает при
  инвертированном `if`); `the_link_asks_for_what_the_window_hints` (iOS, падает, если подсказка
  игнорируется); `a_material_goes_through_the_window_queue` (`flui-app`) и conformance headless
  (падают, если headless игнорирует `SetMaterial`). iOS `defer_edges` и macOS-материал — только
  сборкой (Mac).
  Проверки после исправлений и слияний (Windows): `cargo fmt`; clippy `-D warnings` рабочего
  пространства `--all-targets` обычный и с `flui-native/winit-backend,flui/native`; nextest
  `flui-platform`, `flui-native`, `flui-testing`, `flui-app`, `flui-runtime`, `flui-interaction`,
  `flui-view` — 698 из 700 (16 пропущены): trybuild `owner_thread_token_has_no_default` с `main` и
  `open_real_window` — проходит отдельно и в прогоне одного `flui-native` (291 из 292), падает
  только под нагрузкой семи крейтов с настоящими окнами; с `winit-backend` — 372 из 374 (те же);
  doctest'ы; `.stderr` `platform_and_host_window_are_sealed` перегенерирован; clippy Android
  `--all-targets`, iOS lib (`DOCS_RS=1`), macOS `--all-targets` обычный и с `winit-backend`;
  `wasm-check`, `wasm-test` — 51; `checks`, `doc-strict`. WSL Ubuntu 24.04: clippy трёх вариантов;
  nextest `FLUI_HEADLESS=1` — 453 из 454 (прежний X11 на WSLg), xvfb `--all-features` — 305 из
  305; rustdoc `--document-private-items` нашёл ссылку на удалённый `set_material`
  (исправлено). Не запускалось: Mac и iPhone для новых путей (материал macOS, частота кадров iOS,
  края iOS из чужого потока) — запрошено у владельца; `cross-typecheck`; браузер.
  **Дополнение к LY43 и словарю LY38** — ветка `platform/not-yet-on-this-host` (владелец одобрил
  2026-10-27). Новая причина `UnsupportedReason::NotYetOnThisHost`: ОС это умеет, хост FLUI ещё
  нет; не повторяемая. Отличает «на этой платформе никогда» (`NotOnThisPlatform`) и «у этого
  устройства нет» (`NotOnThisDevice`) от «FLUI ещё не сделал». `unavailable_on_this_device`
  обобщён в `WindowCapabilities::unavailable(command, absence)`: тип `CapabilityAbsence`
  (`NotOnThisPlatform`, `NotOnThisDevice`, `NotYetOnThisHost`, байт на слот) не пускает
  повторяемые и оконные причины; `refusal` отвечает им; равенство сравнивает отсутствие только у
  недоступных слотов; слоты объявлены один раз макросом, `slot()` сопоставляет команды
  исчерпывающе. Подсказка частоты и входные области — глаголы, свою причину отвечают сами (их
  ничто не допускает по возможностям). Поправки ADR-0152, ADR-0254 §6 (следуют решению
  владельца), ADR-0151 §4. Шлюз версии — `NotOnThisDevice`, с версии — `NotYetOnThisHost`:
  материал Android (`BackgroundBlur`, API 31, и `isCrossWindowBlurEnabled` из выборки
  дисплеев), курсор Android (`PointerIcons`, API 24), указатель iPad (`PointerInteraction`,
  iOS 13.4, только iPad), частота кадров macOS (`ViewDisplayLink`, macOS 14; новый
  `MacOSVersion` с полом из `platform-support.toml`, читается в `MacosDisplays`). Материал iOS и
  AppKit вне главной полосы — `NotYetOnThisHost`; значение `Win32Window` новее хоста — тоже.
  Правила вынесены в чистую модель окна `window::capabilities`, которую дисплеи не называют
  (они отдают версию и факты). Оставлены `NotOnThisPlatform` (входные области контракта —
  исключение системных жестов, не сквозной ввод): Win32, AppKit и веб входные области, winit на
  всех целях (Linux тоже — прошлое изменение отменено), частота кадров winit на Linux и Windows
  (на macOS — ответ AppKit), Win32 частота кадров, drag пером и касанием в OLE, хаптика Linux,
  `Absent`, неизвестный `WriteMode`, headless. Тесты: `every_error_carries_its_reason`
  (`each reason answers retry and message`); `window_command_validate` (каждая команда с
  `NotOnThisDevice` и `NotYetOnThisHost`, `an_absence_left_in_an_available_slot_is_no_difference`,
  `no_absence_is_retryable`); `native_capability_absences`,
  `native_window_gate_table`, строки `PointerIcons` и `BackgroundBlur` в `os_version_gate_table`.
  Каждый падает при откате своей правки (проверено). На устройствах iOS, Android и macOS пути не
  запускались.
- [x] **LY45** — ветка `platform/app-handler`, от `main` @ `e93d756f4` (LY44 влит), `main` с
  vanyastaff/flui#1917 (LY43) влит слиянием. **Ядро.** `AppHandler` (`platform.rs`): `ready(OwnerPlatform)
  -> Result<(), BootstrapError>`, `quit_requested`, `reopen`, `session_end(phase) -> SessionEndAnswer`,
  `host_notice`, `exit_policy -> bool`, `wake_deadline -> Option<Instant>`; каждый метод, кроме
  `ready`, получает `&OwnerPlatform` (ADR-0254 §4), умолчаний нет, трейт не `Send`.
  `Platform::run(self: Box<Self>, Rc<dyn AppHandler>)`. Удалены `PlatformReadyCallback`,
  `PlatformHandlers`, `SessionEndHook`, `on_quit`, `on_reopen`, `on_session_end`, `on_host_notice`,
  `on_open_urls`, `set_exit_policy_hook`, `set_wake_deadline_hook` (у `Platform`, `OwnerPlatform`,
  `SharedPlatform`, `host::Loop`, `host::Lifecycle`, `LifecycleHooks`), `NoticeChannel::set_hook`,
  `SharedField` и `Once` точки вызова, учёт идентичности хука в дедлайне Win32 (у прогона один
  обработчик). `on_keyboard_layout_change` остаётся регистрацией до LY55 (хранится рядом с
  загрузкой цикла: `HeadlessState::keyboard_layout_changed`, Win32 `AppHooks`).
  **Доставка** — `owner/app_delivery.rs`, `AppDelivery` у каждого цикла: обработчик в owner-слоте
  (`bind` при `run`, на потоке владельца), платформа и owner-хуки — слабо (платформа владеет циклом,
  который владеет доставкой; цикл держит их сильно на время `run`), `OwnerPlatform` чеканится на
  каждый вызов. Сообщения (выход, reopen, notice) идут одной FIFO: принятое до `bind`, во время
  `ready` или другого метода, с чужого потока — ждёт и доставляется по порядку на владельце
  (ход сигнала владельца, `resume_through_signal`, на всех хостах); выход доходит один раз за прогон.
  Вопросы (конец сеанса, политика выхода, дедлайн) задаются сразу, повторно-входно; сообщение,
  поднятое во время ответа, ждёт ответа. Без обработчика хост отвечает сам (`Proceed`, свой
  умолчательный выход, без дедлайна). Паника метода сдерживается, хвост доставляется, первая
  паника возвращается. `release` в конце прогона: при раскрутке обработчик удерживается навсегда,
  а не остаётся в таблице потока (иначе его захваты падали бы при выходе потока —
  `deadline_query_unwind_retains_hostile_captures`). `SystemNotices` строится поверх доставки
  (`H::notices(&app)`), помечает устаревшее при пробуждении и передаёт. Хосты: Win32 (`RunGuard`
  сообщает выход и отпускает; хуки объявлены до охраны), winit (связывание в `resumed`), AppKit
  (`LoopControl` и `WakePump` поверх доставки; reopen сохраняет очередь и забор выхода), UIKit,
  Android, Web, headless. `HeadlessHostEvents::reopen` — новый ввод для тестов.
  **`flui-app`.** `app_handler(bootstrap)` (`runner/host.rs`): `ready` — загрузка раннера; конец
  сеанса и notice — во все UI runtime; выход, reopen, политика выхода и дедлайн — по маршрутам
  `AppRoutes` (`route_quit`, `route_reopen`, `route_exit_policy`, `route_wake_deadline`), которые
  ставит загрузка; каждая установка владельца начинает маршруты заново, обработчик отвечает только
  за свою установку. Регистраций колбэков у раннеров нет. Без маршрута: выход и reopen никуда,
  выход из цикла запрещён, дедлайна нет; прямой режим ставит `route_exit_policy(|| true)`.
  **Решение сверх строки (для владельца):** `flui-app` не использует `&OwnerPlatform` из методов —
  его маршруты читают TLS `APP_RUNTIME`, как раньше хуки.
  **Тесты** (откат проверен): `app_handler_obligation_table` (`tests/app_handler.rs`; строки
  `ready`, выход, reopen, конец сеанса с ответом, notice, политика выхода, сообщение до
  обработчика, паника метода, дедлайн на Win32); `app_delivery_contracts` (юнит, 10 строк);
  `handler_reentry` — каждый метод `AppHandler` открывает окно, сообщает notice и выходит изнутри
  вызова, прогон внутри обработчика связывает свой (вместо «замены обработчика»: заменить его
  API нет); `explicit_quit_reenters_and_reaches_the_handler_outside_the_state_lock`,
  `winit_exit_policy_runs_outside_the_state_lock`, `a_due_deadline_gets_one_modal_frame_per_generation`,
  `web_session_end_handler_reenters_the_platform`. Откаты: без очереди до `bind` падают «a report
  before ready waits for it», «a failed bootstrap keeps the reports» и «a report before the handler
  is heard after ready»; без доставки хвоста после паники — «a panicking method …» в обеих
  таблицах; без «выход один раз» — по строке в обеих таблицах и `handler_reentry` (headless-тест
  выхода виснет); без ожидания ответа на вопрос — «a report raised inside a question …»; с
  `retire(binding)` вместо выемки обработчика — `deadline_query_unwind_retains_hostile_captures`
  (abort процесса); с хуками Win32 после охраны — «a failing exit policy keeps the loop» и «the wake
  deadline is asked and kept». Удалены строки замены хуков (замены нет) и строки очереди
  `SystemNotices` (очередь теперь в доставке, её строки — в `app_delivery_contracts`).
  Тесты, вызывавшие `quit()` внутри загрузки и ждавшие хука там же, выполняют сценарий после неё
  (`after_bootstrap`). `.stderr` `host_table_names_every_position` и
  `platform_and_host_window_are_sealed` перегенерированы (сдвиг строк).
  Документы: ARCHITECTURE.md ядра, README ядра, `docs/architecture.md`, `docs/gaps.md` (запись 13),
  `docs/platform-parity.md` (заметка о диплинках, ячейки без изменений),
  `changelog.d/platform-app-handler.md`.
  `module-dag` `flui-native` — 302 модуля, 324 ребра, 1 исключение; `globals` — 57; из
  `markers.toml` ушла запись маркера `slice` в `platform.rs`.
  Проверки (Windows): `cargo fmt`; clippy `-D warnings` рабочего пространства `--all-targets`
  обычный и с `flui-native/winit-backend,flui/native`; nextest `flui-native`, `flui-app`,
  `flui-platform`, `flui-testing`, `flui-runtime`, `flui-view`, `flui-interaction`, `flui-sdk` —
  701 из 703 (падают trybuild `owner_thread_token_has_no_default`, как на `main`, и реальное окно
  `open_real_window` — повтор зелёный), `flui-native` и `flui-app` с `winit-backend` — 365 из 366
  (тот же trybuild), `flui --features native` — 10; trybuild `flui-native` (`.stderr` двух
  фикстур); clippy Android `--all-targets` (`flui-native`, `flui-app`), macOS `--all-targets`
  обычный и с `winit-backend` (clang и llvm-ar LLVM), iOS lib `flui-native` (`DOCS_RS=1`), wasm32
  lib `flui-native`, `flui-app`, `flui-platform`, `flui-web-demo`; `checks`, `doc-strict`,
  `wasm-check`, `wasm-test` — 49. WSL Ubuntu 24.04 (`WAYLAND_DISPLAY`, `XDG_SESSION_TYPE`
  сняты): clippy четырёх крейтов без признаков, с `winit-backend` и `flui-native --all-features`;
  nextest под xvfb `FLUI_HEADLESS=1`: `flui-native` — 215, с `winit-backend` — 297,
  `--all-features` — 298, `flui-app` с `winit-backend` — 47; rustdoc `--document-private-items -D
  warnings` обоих вариантов. Не запускалось: Android-устройство, Mac и iPhone (AppKit reopen и
  насос дедлайна, UIKit — только типизированы; vanyastaff/flui#1840), браузер (проверить: `pagehide`
  и `pageshow` доходят до `AppHandler::session_end`, выход отпускает окно), `cross-typecheck`.

  **Исправления после ревью** (#1926). (1) Выход, поднятый внутри метода обработчика, на вебе и
  Android больше не теряется: `AppDelivery::release`, вызванный во время доставки, ждёт её конца
  и отпускает обработчик после того, как она доставила очередь (строка «a quit raised as the
  loop ends inside a method is heard once» в `app_delivery_contracts`, из `host_notice` и из
  `ready`). (2, 3, 5) `AppRoutes` принадлежат `RunnerHandler` (`Rc<RefCell<AppRoutes>>`);
  `AppRuntime` хранит только слабые ссылки `run_routes` на маршруты живых запусков, а функции
  `route_*` пишут в самый внутренний живой. Поколение установки и `try_borrow` ушли: вложенный
  `run` не стирает маршруты внешнего, проваленная загрузка не отвечает чужими. Таблица
  `app_handler_routes` в `flui-app` («a nested run leaves the outer run's routes», «a failed
  bootstrap answers with no other run's routes»). Маршрут выхода рабочего стола и дедлайна
  по-прежнему сверяют `loop_identity`: главное окно на поток одно. (4) `exit_policy` отвечает
  `ExitAnswer { End, KeepRunning, HostDecides }`; без маршрута `flui-app` отвечает
  `HostDecides`, и хост решает как без обработчика (Win32, AppKit, winit — выход, headless —
  нет); прямой режим — `End`. `ExitAnswer` одобрен владельцем 2026-10-27, поправка ADR-0254 §4
  записана. Строки «an exit policy left to the host is the host's» и «a run
  that routes no exit leaves it to the host». (6) R9.7: настройки идут прежним путём ADR-0172
  через запуск с `AppHandler` — `a_panicking_preference_delivery_does_not_stop_later_changes`,
  `a_replacement_platform_starts_a_new_preference_owner`,
  `the_platform_preference_source_reaches_installed_consumers` (`flui-app`, через
  `app_handler`) и `a_change_with_no_turn_callback_reaches_the_next_one_registered`
  (`flui-native/tests/preferences.rs`). (7) Один набор: `flui_native::testing::app` (`AppLog`,
  `AppCall`, `on_ready`, `with_log`, `run_keeping_owner`), `tests/support/app.rs` удалён;
  Win32 `handlers` → `app_hooks`, проверка выхода получает `&AppDelivery`; документы о
  реализациях `AppHandler` исправлены (ADR-0151 §9, ADR-0254 §4); ADR-0039 §1 и §6, ADR-0044
  §7, ADR-0093 отмечены как замещённые ADR-0254 решением 4; статус ADR-0254 — реализован
  частично. Каждый новый тест падает при откате (проверено: отложенное отпускание; старая
  маршрутизация из `HEAD` — падают все три строки `app_handler_routes`). macOS: ответ
  `applicationShouldTerminate:` на выход из сеанса не изменился — `TerminateGate` отвечает
  `Now` или `Later` после объявления, `Cancel` только обычному выходу
  (`loop_control.rs` `terminate_requested`, `terminate_gate.rs` `terminate_requested`).
- [ ] **LY63**, часть `configChanges` — ветка `android/config-changes`. Шаблон `flui create`
  объявляет все изменения, которые хост читает на `ConfigChanged`:
  `orientation|screenSize|smallestScreenSize|screenLayout|density|uiMode|fontScale|fontWeightAdjustment|keyboard|keyboardHidden|navigation`
  (тест `the_android_activity_handles_the_configuration_changes_the_host_follows`). `locale` и
  `layoutDirection` не объявлены: хост Android не читает языки, а цепочка резервных шрифтов
  выбирается по локали один раз при скане шрифтов среды; они входят в список вместе с этим
  чтением. Упаковщик без
  Gradle отказывает `android.jar` ниже API 31 (флаг `fontWeightAdjustment`). Ночной режим хост
  читает из Java `Configuration.uiMode`: нативный `AConfiguration` android-activity после
  изменения на месте остаётся прежним. Прогон на Galaxy S21 (Android 15, проба `flui create
  --local`): тема в обе стороны, `font_scale` 1.3, `font_weight_adjustment` 300, `wm density`
  480, поворот — один `android_main`, значения в `MediaQuery`. Остаток LY63: `targetSdk` из `platform-support.toml`, путь
  Gradle, правила R8, `windowSoftInputMode`, golden манифеста и `build.gradle.kts`, список в
  генерируемом манифесте `FluiActivity` (LY62).
- [x] **LY46** — ветка `platform/surface-target`, от `main` @ `136947eff` (LY43, LY44), слит `main`
  @ `e617f109a` (#1923, #1924, #1925). **Контракт.** `PlatformWindow` без `Send + Sync`; окна
  отдаются как `Rc<dyn HostWindow>`/`Rc<dyn PlatformWindow>`, слабые ссылки — `std::rc::Weak`.
  Новое: `SurfaceTarget: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static` с
  обязательным `pre_present_notify`; `PlatformWindow::surface_target()` (обязательный);
  `NoNativeSurface` — цель окна без нативной поверхности (doctest контракта, двойники).
  `PlatformWindow::pre_present_notify`, `window_handle`, `display_handle` и
  `HasWindowHandle`/`HasDisplayHandle` для `dyn PlatformWindow`/`dyn HostWindow` удалены. Рассказ
  `request_redraw` (≈50 строк, «нарушено на двух путях») заменён двумя фразами: вызов с чужого
  потока не компилируется. **Решение сверх строки (для владельца):** два пути с чужого потока
  (`FrameWakeHandle` `flui-app` и слушатель активации AccessKit в `flui-runtime`, а также
  `request_frame` презентации, который зовут сигналы, записанные на рабочих потоках) нуждались в
  законном маршруте. Выбран `RedrawRequester` (`Send + Sync`, `request_redraw`) и обязательный
  `PlatformWindow::redraw_requester()`; любое `Fn() + Send + Sync` — запросчик. Альтернатива —
  глагол `PlatformProxy::request_redraw(WindowId)` по ADR-0045 п. 5 — требовала бы поиск окна по
  идентификатору в каждом транспорте и недоступна `flui-runtime` (ADR-0083). **Ядро.**
  `owner::window_delivery::Retirement` — признак начатого вывода окна (ставит
  `WindowDelivery::clear`, то есть `WM_DESTROY`, `windowWillClose:`, завершение закрытия winit,
  выход лупера Android, сцена UIKit, отставка холста, закрытие headless); `owner::slot::{OwnerCell,
  OwnerMap, OwnerWeak}` — хранение `!Send` окон в `Send + Sync` структурах хоста, доступное только
  потоку владельца; `WindowDelivery::publish_through_owner`. Цели и запросчики по хостам: Win32
  `Win32Surface` (HWND и адрес контекста числами, `Retirement` + проба идентичности
  `teardown_route`; `unsafe impl` не нужен), winit `WinitSurface` (держит `Arc<Window>`,
  `Retirement`; `pre_present_notify` взводит frame callback Wayland) и `WinitRedraw` (слабо),
  Android `AndroidSurface` (`native_window()` на каждый запрос: `Unavailable` только для
  завершённого окна) и `AndroidRedraw`, web `WebSurface` (единственный `unsafe impl Send + Sync`
  вне Apple, под `cfg(not(target_feature = "atomics"))`, SAFETY называет инвариант), headless
  (всегда `Unavailable`, счёт `pre_present_notifies`). AppKit: `unsafe impl Send/Sync` у
  `MacOSWindow` и `route_on_owner` удалены, `MacOSSurface` (указатель вида, флаг закрытия,
  `Retirement`, флаг ставит и `Drop`) с SAFETY-инвариантом, `MacOSRedraw` с чужого потока ставит
  перерисовку в ленту владельца (`exec_async_on_owner`) и не блокирует; реестр окон — `OwnerMap`,
  отложенная перерисовка держит `OwnerWeak` вместо ключа-указателя. UIKit: `IOSSurface` с
  инвариантным SAFETY, `IOSRedraw` — флаг спроса для `CADisplayLink`. Таблицы окон хостов: Win32
  `WindowSet` — `OwnerCell`, winit `windows` — `OwnerMap`, Android — `ActivityWindow: Send + Sync`
  (окно конкретно `Send`, приложению отдаётся клон в `Rc`). `PendingWindow`: `wait()` отдаёт
  `WindowId`, `try_take` и future — `OpenedWindow` (идентичность; окно — только на владельце,
  слабо); headless держит окно отложенного открытия до закрытия (`keep_deferred`), как
  нативные хосты. **`flui-app`, `flui-runtime`.** Рендерер строится из `window.surface_target()`,
  `install_pre_present_hook` берёт цель; `FrameWakeHandle`, пробуждение UI-рантайма, слушатель
  активации и `request_frame` презентации зовут `redraw_requester()`; `CloseRequestRouter` — `Rc`.
  `thread_local!` в `host.rs` оставлен: причина другая — регистрации уровня приложения
  (`on_wake`, `on_host_notice`, `on_session_end`, хуки политики выхода и дедлайна) остаются
  `Send` до `AppHandler` (LY45); документация поля переписана. Пример `web_demo` держит окно в
  обработчике до закрытия (`on_quit` требует `Send`). **Тесты** (откат проверен для каждого):
  trybuild `a_platform_window_cannot_cross_a_thread` (`flui-platform/tests/compile_fail`;
  с возвращёнными `Send + Sync` — mismatch) и `assert_not_impl_any!` в
  `tests/surface_target.rs` (с `Send + Sync` не компилируется), там же
  `a_window_with_no_native_surface_hands_out_no_handle`; Win32 в `native_window_commands`:
  `a_surface_target_is_unavailable_once_its_window_closed` (без проверки жизни в цели падает:
  «a closed window's target hands out no handle») и
  `a_redraw_asked_from_another_thread_reaches_the_window` (с пустым запросчиком падает);
  `main_window_installer_matrix` (без `keep_deferred` падает: «coalesced intents invoke one
  installer, left 0»). Синхронный `state_changed` в живом resize Win32 — существующая строка
  `a_live_resize_publishes_inside_its_modal_loop`, зелёная. Перенос тестов: двойники окна
  получили запросчик и цель, сценарии X11 водят окно через `ScenarioWindow` (вызовы уходят на
  владельца ходом `OwnerSignal`); переименованы `a_close_command_asks_no_veto` (было
  `a_close_from_another_thread_asks_no_veto`) и `a_win32_view_refuses_a_closed_window` (было
  `a_win32_view_refuses_another_thread_and_a_closed_window`), поток-переход из них убран —
  тип его запрещает. **Документы:** ARCHITECTURE.md `flui-platform`, `flui-native`, README
  `flui-native`, `docs/architecture.md`, ADR-0058 (`SurfaceTarget::pre_present_notify`),
  комментарии `flui-engine` и `flui-scheduler`, `changelog.d/platform-surface-target.md`;
  `docs/platform-parity.md`/`docs/gaps.md` без изменений (поддержка по платформам та же, запись
  gaps о `SurfaceTarget` стала верной).
  Проверки (Windows): `cargo fmt`; clippy `-D warnings` рабочего пространства `--all-targets`
  обычный и с `flui-native/winit-backend,flui/native`; nextest `flui-platform`, `flui-runtime`,
  `flui-interaction`, `flui-testing`, `flui-app`, `flui-view`, `flui-sdk` — 418 из 418 (15
  пропущены), `flui-native`, `flui-engine` — 358 из 359 (падает только trybuild
  `owner_thread_token_has_no_default`, как на `main`); clippy Android `--all-targets`, wasm32
  lib, iOS lib (`DOCS_RS=1`), macOS `--all-targets` обычный и с `winit-backend`; `checks`,
  `doc-strict`, `wasm-check`, `wasm-test` — 51, `globals` — 57, `module-dag` — 1 исключение.
  WSL Ubuntu 24.04: clippy трёх вариантов чистый; nextest под xvfb `--all-features` — 308 из 308,
  `flui-native`+`flui-app` с `winit-backend` — 354 из 354; без xvfb — 662 из 663 (прежний
  `x11_window_takes_commands_and_publishes_what_the_server_holds` против X-сервера WSLg).
  Живой пример: `counter` на Windows (DX12, RTX 3070 Ti) — поверхность создана из цели, кадры
  рисуются, три нажатия дают «3», `WM_CLOSE` завершает процесс без предупреждений.
  Не запускалось: Mac и iPhone (vanyastaff/flui#1840; список в PR), `cross-typecheck`,
  Android-устройство, браузер.
  **Решение владельца (2026-10-27):** `RedrawRequester` одобрен; ADR-0254 получил строку
  `Amended` с причинами, ADR-0045 §5 помечен как заменённый в части ADR-0254.
  **Исправления после ревью (#1927).** Слит `main` @ `c2f90d5d8` (LY45, `AppHandler`) и
  сессия Mac (`9f8bc89c8`, тест `a_redraw_from_another_thread_is_queued_without_waiting`).
  ADR: статус ADR-0254 описывает сделанное (решения 1–5), устаревший текст ADR-0063, ADR-0082,
  ADR-0135 исправлен. Цели поверхности: AppKit и UIKit держат вид (`MainRetainedView`: retain на
  главном потоке, release на главном — с чужого потока через главную очередь,
  `main_release::release_on_main`), вне главного потока — `Unavailable`; Android держит счётную
  ссылку `NativeWindow` и вне лупера отвечает `Unavailable` (раньше указатель временного
  `NativeWindow` мог пережить его). Документация `SurfaceTarget`/`RedrawRequester`: где делается
  поверхность, что цель держит живым, отставка вместо «флага закрытия». `NoNativeSurface`
  перенесён в `flui_platform::testing` (ADR-0243). `OpenedWindow::window()` отдаёт
  `OpenedWindowReach { Open, Closed, OtherThread }`, сообщение `opened_window` в `flui-app`
  различает оба отказа. `OwnerSlot`, сброшенный на чужом потоке, оставляет значение владельцу
  (кладбище сирот, хоронится при следующем обращении владельца к любому слоту), `OwnerCell`
  привязан к потоку, который его создал; `request_redraw` Win32 и winit проверяют отставку.
  Строка соответствия `a_surface_target_is_send_sync_and_unavailable_after_close`
  (`flui-testing`, с `pre_present_notify` раз за кадр; фикстура объявляет
  `has_native_surface`), прогнана на headless, Win32, winit; AppKit — на Mac. Отчётные метки
  «(issue #1043)» убраны из `flui-app` и `examples/scene_render.rs`. Мёртвый код wasm32 в
  `flui-app` (`install_exit_policy_hook`, `has_pending_installs`, `PrimarySurfaceRestored` и др.,
  11 предупреждений) есть на `main` @ `c2f90d5d8` и только в `check --all-targets` для wasm32,
  который CI не запускает; `wasm-check` чист на обеих ветках — не тронут. Тесты (откат проверен
  для каждого): `a_value_dropped_elsewhere_is_released_in_a_main_turn` (с release на месте
  падает), `a_value_whose_slot_dropped_elsewhere_is_dropped_at_the_owners_next_use` (без записи
  в кладбище падает), `an_owner_map_belongs_to_the_thread_that_made_it`,
  `a_winit_surface_target_answers_until_its_window_closes` (без проверки отставки в цели winit
  падает), `a_surface_target_that_blocks_or_withholds_is_caught` (ловит цель, которая блокирует
  или не отдаёт описатель), переписанный Win32 `a_surface_target_is_unavailable_once_its_window_closed`
  (проверки при удерживаемом окне, последний шаг проверяет исчезновение слабой ссылки).
  Проверки (Windows): `cargo fmt`; clippy рабочего пространства `--all-targets` обычный и с
  `flui-native/winit-backend,flui/native`; nextest `flui-platform`, `flui-runtime`,
  `flui-interaction`, `flui-testing`, `flui-app`, `flui-view`, `flui-sdk` — 419 из 419;
  `flui-native`, `flui-engine` — 354 из 357: trybuild `owner_thread_token_has_no_default` (как на
  `main`), `ole_drag_and_drop_on_real_windows` и `open_real_window` падают только рядом с другими
  тестами реальных окон, по одному зелёные; с `winit-backend` — 374 из 376 (тот же trybuild и
  `app_handler_obligation_table` из LY45, по одному зелёный). `.stderr`
  `platform_and_host_window_are_sealed` обновлён (строки сдвинулись). clippy с
  `flui-native/testing,flui-native/winit-backend` для Android, iOS, macOS (и macOS без признаков);
  `checks`, `doc-strict`, `wasm-check`. WSL: clippy трёх вариантов (в том числе `-p flui-native
  --all-targets --features winit-backend` и `--all-features`), rustdoc; xvfb nextest 306 из 306
  и 353 из 353; без xvfb — 661 из 662 (прежний случай X-сервера WSLg).

## Отступление от порядка, решённое владельцем

- 2026-10-21: LY22 ждёт свои зависимости (LY24, LY9, text-ime, teardown), хотя ни одна ветка не
  держит их файлы; первыми идут LY24 и LY23.
- 2026-10-21, позже: LY8/LY9 доводятся до приёмки, затем LY22. Зависимость LY22 от text-ime и
  teardown снята: это черновики от 2026-10-05, их файлы не держит ни одна ветка, а сами спеки
  пишутся заново на новом устройстве ядра. У LY47 и LY48 она остаётся.

## Работа вне серии: LY8 и LY9

Эти две задачи ведутся по [ADR-0172](../../../adr/ADR-0172-host-owned-system-preferences.md)
независимо от серии и сохраняют свой статус. Пути названы по текущим именам крейтов; после LY20 и
LY21 читать их как `crates/flui-native/…` и `crates/flui-platform/…`. Задачи, которые трогают те
же файлы, ждут слияния соответствующих частей LY8/LY9: LY22 (разбор `impl Platform` каждого
хоста), LY33 (файлы предпочтений) и `AppHandler` (LY45).

| ID | Задача | Зависит от | Крейты и файлы | Гейты | Тесты | Где |
|---|---|---|---|---|---|---|
| **LY8** | `SystemPreferences` (валидированный снимок), принадлежащий host источник и headless-производитель; `flui-app` засевает runtime и рассылает через typed FIFO; `MediaQuery` (text, contrast, bold, locales, motion); `AccessibilityFeatures` удаляется с миграцией потребителей. Сливается вместе с LY9 и production-потребителями | ADR-0172 | `flui-platform-api/src/preferences.rs`, `flui-platform/src/{traits/platform.rs, platforms/headless/*}`, `flui-app/src/app/{runtime.rs, runner/*}`, `flui-runtime/src/{owner/,media_query_root.rs}`, `flui-widgets/src/media_query.rs`, `flui-semantics/src/accessibility.rs` | `workspace`, `reach`, `globals`, thread-boundary; changelog | начальное значение до пользовательского окна; поздние runtime/presentation; FIFO и failure/recovery; реальный text consumer и revert proof | L |
| **LY9** [P] | `GestureSettings` через существующий `GestureArenaScope`, lifecycle consumers и active-sequence policy; native producers Windows, Android, AppKit, iOS, web; winit/Linux сохраняет неизвестные поля вместо фиктивных наблюдений | вместе с LY8; consumer — с владельцем interaction | `flui-interaction/src/{settings,binding}.rs`, `flui-widgets/src/`, `flui-platform/src/platforms/{windows,android,web,macos,ios,winit}/` | `cross-typecheck`, `wasm-check`; Windows native execution | реальные double tap, drag, long press, fling, wheel; DPI-проекция; Windows refresh и teardown; overrides; unavailable platforms явно compile-only | L/W/T |

## Приёмка LY8/LY9

Незавершённые пункты требуют проверки полной цепочки. Нативная проба,
исправление существующего линейного текста или чтение getter не закрывают задачу.

- [x] Валидация значений и единиц: неизвестно/не поддержано/ошибка обновления;
  диапазоны и промежуточная арифметика; motion duration scale отдельно от rate;
  mouse rectangle отдельно от touch slop; wheel disabled/lines/characters/page.
  Неизвестно и не поддержано: `unavailable_observations_do_not_claim_system_defaults`,
  `unsupported_features_are_unobserved`, случай `unsupported` Linux; ошибка обновления:
  `the_platform_preference_source_reaches_installed_consumers`,
  `deferred_native_reads_preserve_accepted_preferences`, `bounded_native_read_admission`;
  диапазоны: `invalid_text_observations_cannot_enter_a_snapshot` и случай
  `text-scale-out-of-range` (портал сообщает 100 и −1 — ни одно не публикуется, и ход владельца
  между отказом и следующим значением видит «нет наблюдения»); арифметика:
  `an_exhausted_revision_refuses_without_changing_or_queueing`,
  `native_touch_geometry_projects_each_context_and_refuses_overflow`, строка `drag_metric_is_a_half_extent_of_the_same_magnitude`
  (`i32::MIN` отклоняется); масштаб длительности против скорости:
  `normal_and_preserve_use_distinct_duration_timelines` (Normal — `clock_rate × controller_rate /
  scale`, Preserve — без масштаба); прямоугольник мыши против slop касания:
  `native_mouse_geometry_preserves_full_area_and_half_extents`; колесо:
  `wheel_observations_keep_lines_pages_and_characters_apart`,
  `vertical_wheel_count_is_lines_until_the_page_sentinel` (0 → `Lines(0)`, только `u32::MAX` →
  `Page`), `horizontal_wheel_characters_are_never_a_page`.
- [x] Источник без пользовательского окна: регистрация до sample, invalidation
  во время sample, commit до wake, повтор/замена/отсутствие/panic callback,
  поздний callback после shutdown и новая инкарнация host.
  Регистрация до первого чтения: изменение, принятое до первого хода, доходит —
  `a_change_before_the_first_owner_turn_reaches_consumers_once`; на Android `FluiView`
  регистрирует наблюдателей до отчёта (`onAttachedToWindow`). Invalidation во время чтения:
  `invalidation_during_read_requires_another_observation` (Windows); у macOS и Android чтение и
  уведомление идут на одном потоке владельца, поэтому уведомление не может прийти во время
  чтения — ОС ставит его в очередь цикла, и отдельной строки нет. Commit до доставки:
  `a_revision_is_accepted_before_the_turn_that_delivers_it` (ход, доставляющий ревизию, уже
  видит её принятой; валится, если принять после доставки). Повтор:
  `preference_failure_retries_without_a_user_window`, случай `recovery`. Замена:
  `a_replaced_wake_hook_is_the_only_one_later_changes_reach` (и захваты заменённого хука
  освобождаются там, где могут снова войти в сигнал владельца). Отсутствие: до `start` —
  `a_change_before_the_wake_hook_exists_reaches_consumers_once`; после `start` —
  `a_change_with_no_turn_callback_reaches_the_next_one_registered` (`OwnerSignal::drive_in`
  оставляет долг, пока callback нет, и регистрация планирует ход). Panic:
  `a_panicking_preference_delivery_does_not_stop_later_changes` (паника потребителя не гасит wake
  presentation; ход, чей хук запаниковал, не повторяется — иначе вечно паникующий хук зациклил
  бы владельца, — и принятое значение доставляет следующий ход владельца без нового изменения),
  `panic_preserves_refresh_and_releases_the_reader`. Поздний callback:
  `browser_motion_observations_wake_replace_and_retire_their_owner`,
  `a_delegate_that_outlives_its_source_is_inert`, `a_receiver_the_os_keeps_stays_registered_and_inert`,
  случай `late-proxy`, `obsolete_native_observation_cannot_update_a_replacement_host`; новая
  инкарнация: `a_replacement_platform_starts_a_new_preference_owner`. «Один раз» в именах строк
  app — одна пересборка потребителя; повторную доставку той же ревизии отклоняет
  `an_older_or_repeated_preference_revision_is_ignored`.
- [x] Один и два runtime, поздний runtime, поздний presentation, закрытый sibling,
  отказ первого получателя и доставка остальным; старый queued revision не
  переписывает новый seed. Соседние обновления не склеиваются (ADR-0172 п. 3 это разрешает, но не
  требует): каждая принятая ревизия — отдельная работа в очереди владельца.
  `preference_fanout_survives_a_failing_runtime`, `preferences_wake_each_isolated_window`,
  `text_scale_reaches_a_late_runtime_and_its_later_presentation`,
  `preference_fanout_passes_a_closed_sibling`, `an_older_preference_revision_cannot_replace_a_newer_seed`,
  `an_older_or_repeated_preference_revision_is_ignored`; обновления предпочтений не склеиваются
  вовсе: `two_preference_deliveries_for_one_runtime_never_merge` и
  `queued_preferences_are_not_coalesced_across_an_input` (A, ввод, B, ввод — каждый ввод видит свои).
- [x] Text/RichText/EditableText: styled spans разных размеров, unstyled text,
  layout/intrinsics/hit testing/caret, live change и subtree override. Политика
  sizing зафиксирована по действительным producer mappings без заявления native
  parity из одного scalar; собственные authored стили не масштабируются дважды.
  Строки `the_system_text_scale_grows_each_span_once`,
  `a_field_measures_and_hit_tests_the_current_text_scale` вместе с прежними
  `media_text_scaling_changes_the_laid_out_text`,
  `a_text_scale_change_relayouts_a_preserved_text_subtree` и
  `inherited_text_sizing_updates_editable_glyphs_and_caret`; политика — таблица
  в `crates/flui-native/ARCHITECTURE.md` («Native text sizing…»).
- [x] GestureArenaScope и mounted consumers: double tap/drag/long press/fling/wheel,
  настройки на текущую и следующую последовательность, per-kind fallback,
  авторские overrides, обратная смена настройки.
  Локальная gesture/wheel приёмка: `admitted_gesture_settings_contract`,
  `gesture_lifecycle_matrix`, `pointer_and_gesture_recognition`,
  `scroll_physics_and_activity`, `navigator_and_overlay`, `owner_metrics_contract`
  и `frame_pacing_and_pump_matrix`; детали и native ограничения —
  [приёмка I11](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/plans/specs/flui-interaction/tasks.md#текущая-локальная-приёмка-i11-gesturewheel).
  All-features clippy/compile-fail и Win32 wheel smoke прошли. Заключительный
  `check-changed` прошёл на `38f9d5238` относительно `b357bc903`; CI ещё ожидается.
  Остальные пункты LY8/LY9 этой отметкой не закрываются.
- [x] Motion и остальные значения имеют production-потребителей; политика
  приложения остаётся runtime-owned. Существующий animation spec сверяется перед
  реализацией, отдельный SystemMotion producer не появляется.
  Масштаб текста и вес — `RichText`, `EditableText`, `Icon`; движение — `MotionClock` и
  `MediaQuery::motion_of` (swipe, `page_view`, `shimmer`, индикаторы, переходы, material и
  cupertino); высокий контраст — выбор темы в `flui-material` и цвета `flui-cupertino`; тактильный
  отклик — `ctx.haptics()` в `draggable` и `anchored_drag`; языки — `WidgetsApp`; жесты —
  распознаватели через `GestureArenaScope`; колесо — `Scrollable` и `InteractiveViewer`.
  `native_mouse_geometry`/`native_touch_geometry` читает только снимок — так задумано. Отдельного
  производителя `SystemMotion` нет; политика `set_motion_override` принадлежит runtime
  (`system_motion_change_reaches_media_query_and_the_clock`,
  `motion_policy_resolves_preference_against_the_system`).
- [x] Windows native query/refresh через настоящий owned receiver без изменения
  пользовательских настроек, zero-window lifetime и teardown. Искусственно
  адресованное сообщение доказывает refresh/routing, а не происхождение от ОС.
  Для остальных платформ записываются точные build/execute ограничения.
  `windows_reads_preferences_before_a_user_window_exists`,
  `setting_messages_wake_the_owner_without_a_user_window` и `preference_receiver_retirement`;
  ограничения по ОС — таблица в записи **PR 9**.
- [x] Полный поиск и миграция AccessibilityFeatures, старых источников и exports;
  SDK/facade, docs, optional features, cross-typecheck, meaningful revert proofs,
  self-review и заключительный `cargo xtask check-changed`. CI — отдельное доказательство.
  `AccessibilityFeatures` нигде не осталось; старые хуки и экспорты — **PR 2**; хост winit
  проверен с фичей и без на каждой цели; у каждой новой строки есть мутация, которая её валит
  (записи PR); ревью всей стопки против «Recurring defects» нашло девять дефектов, все исправлены
  до слияния (записи PR 5a, 6, 8, 9). Заключительный прогон на Windows вместо снятого
  `check-changed`: `xtask platform-test` (216 и 242 пройдены), `xtask gate` — проверки и clippy
  рабочего пространства зелёные, `doc-strict` падает только на ссылках `shared/os_version`,
  которые уже есть на `main` и ломаются лишь на Windows; `xtask cross-typecheck` требует `zig` и
  идёт в CI.

### Доведение до приёмки

Аудит кода 2026-10-21 по каждому пункту выше дал порядок PR:

1. **Хост и runtime** — пункты 1–3 там, где их проверяет Linux CI.
2. **Миграция** — пункт 8: устаревшие `handle_*_changed` и `did_change_*` в `flui-view::binding`,
   экспорт типов предпочтений из `flui-sdk` и фасада, фрагмент changelog для публичных
   `OwnerHost::{update_preferences, preferences}` и `RuntimeHostServices::with_preferences`.
3. **Текст** — пункт 4: hit testing и `RichText` при масштабе текста, политика sizing.
4. **Нативные источники** — пункты 2 и 6 по ОС: Web (`prefers-contrast`, `navigator.languages` и
   `languagechange`), Linux-портал (контраст, масштаб текста, языки), macOS (уведомления вместо
   опроса раз в 500 мс; контраст, языки), iOS (контраст, языки, масштаб текста), Android
   (`ComponentCallbacks` и `ContentObserver` вместо опроса; `fontScale`, языки, контраст); строки
   parity и запись 23 gaps.
5. **Windows и итог** — пункт 7: отказ `DestroyWindow` и отзыва токенов, ограничения сборки и
   исполнения по ОС; заключительный прогон.

- [x] **PR 1** — ветка `platform/preferences`. Headless-источник `HeadlessPreferences`
  (`HeadlessPlatform::with_preferences`, `preference_source`): значение до первого окна, смена через
  пробуждение владельца, отказ чтения сохраняет принятое, неизвестное значение возвращает базу
  потребителя — `the_platform_preference_source_reaches_installed_consumers` (без источника падает
  на первом утверждении). Строки `owner_metrics_contract`: `preference_fanout_passes_a_closed_sibling`,
  `an_older_preference_revision_cannot_replace_a_newer_seed` (публикация runtime за очередью
  ревизий отказывает `Busy`; runtime, построенный после, стартует с новейшей),
  `text_scale_reaches_a_late_runtime_and_its_later_presentation`. В `preferences_contract` —
  `wheel_observations_keep_lines_pages_and_characters_apart`. Тестовый хук runtime берёт ревизию
  через `checked_add`. Длительности жестов контракт хранит как наблюдение: потребитель считает
  дедлайны через `checked_add` (`arm_deadline(Duration::MAX)` и долгое нажатие с `Duration::MAX`
  уже в тестах `flui-interaction`). Соседние обновления предпочтений не склеиваются вовсе, поэтому
  порядок относительно ввода и кадров сохраняется.
- [x] **PR 2** — ветка `platform/preferences-migration`. Из `WidgetsBindingObserver` убраны
  `did_change_{locales,text_scale_factor,platform_brightness,accessibility_features}`, из
  `WidgetsBinding` — питавшие их `handle_*_changed`, из `RendererBinding` — пустые
  `handle_text_scale_factor_changed` и `handle_platform_brightness_changed`: ни один хост их не
  вызывал, предпочтения доходят до виджетов только через `MediaQuery` из снимка хоста.
  `binding_observer_retirement` ведёт повторный вход из `Drop` через смену фокуса окна.
  `flui_sdk::platform` экспортирует остальной словарь предпочтений (девять типов, список в
  `the_public_surface_is_the_measured_list`, тождество с фасадом в
  `the_re_exports_are_the_facades_types`); фасад их уже экспортировал. Фрагмент changelog
  описывает публичные `OwnerHost::{update_preferences, preferences}` и
  `RuntimeHostServices::with_preferences`.
- [x] **PR 3** — ветка `platform/preferences-text`. Системный масштаб, поданный через runtime без
  собственного провайдера теста: `the_system_text_scale_grows_each_span_once` — спаны разных
  размеров, текст без стиля, фиксированный спан посередине, ambient-стиль `Text` масштабируется один
  раз, фиксированный ambient-стиль держит поддерево; intrinsics совпадают с layout, снятие
  наблюдения возвращает исходную геометрию. `a_field_measures_and_hit_tests_the_current_text_scale`
  — intrinsics поля и касание после живой смены в обе стороны. Мутации: фиксированный спан в
  выравнивании ответов и один множитель на все спаны валят первую строку; вторая падает, когда
  смена не доходит до layout ни через виджет, ни через конвейер (каждый путь в одиночку закрывает
  другой). Windows публикует один `TextScaleFactor`, а WinUI масштабирует крупный текст меньше по
  недокументированной кривой, поэтому линейное применение не заявлено как parity (таблица в
  `flui-native/ARCHITECTURE.md`, строка parity, запись 14 gaps). Фрагмент
  `inherited-text-scale.md` исправлен: недопустимый масштаб отклоняется при построении, а не
  заменяется на 1.0.

Шаг 4 идёт отдельным PR на каждую ОС в порядке списка: Web, Linux, macOS, iOS, Android.

- [x] **PR 4** — ветка `platform/preferences-web`. Хост держит по запросу на каждое различаемое
  значение (`prefers-reduced-motion`, `prefers-contrast` more/less/no-preference, `forced-colors`
  active/none) и читает `navigator.languages` (пустой список — `navigator.language`); `change`
  каждого запроса и `languagechange` окна будят владельца, тот читает всё заново вне займа хоста.
  Правила — в модулях `browser_preferences` (неизвестная браузеру функция остаётся ненаблюдённой)
  и `preferred_locales` (BCP 47 в порядке приоритета, неразбираемый тег пропускается; его
  переиспользуют источники Apple, Android и Linux), таблицы `browser_media_contract` и
  `preferred_locale_tags_contract`. Node-зонд в `tests/wasm32.rs` отвечает на все запросы,
  `navigator.languages` и события окна; `browser_contrast_and_languages_reach_their_owner` ведёт
  смены контраста и языков до владельца и снятия (без регистрации `languagechange` падает на
  «languagechange wakes the owner»). В настоящем браузере не запускалось.
- [x] **PR 5** — ветка `platform/preferences-linux`. Источник портала Settings хоста winit читает
  `contrast` из `org.freedesktop.appearance` (без ключа — GNOME `high-contrast`) и GNOME
  `text-scaling-factor`, который GTK применяет ко всем размерам одинаково (поэтому линейная
  политика здесь — родная); отсутствие обоих ключей — ненаблюдение, без повторов раз в секунду.
  Языки — из окружения, как читает gettext (`preferred_locales::from_posix_environment`: список
  `LANGUAGE`, если локаль сообщений из `LC_ALL`/`LC_MESSAGES`/`LANG` не `C`/`POSIX`), один раз
  при старте. Прогон в WSL Ubuntu 24.04 под `xvfb-run`, каждый случай в своей
  `dbus-run-session` с подменным порталом: все 15 случаев `linux_preferences`, из них новые
  `contrast`, `gnome-contrast`, `text-scale` (живая смена через `SettingChanged`), языки
  `[fr-CA, en]` в первом наблюдении каждого случая, `unsupported` без контраста и масштаба. Без
  `contrast` в фильтре `SettingChanged` случай `contrast` падает по сторожу через 30 с. В сессии
  GNOME не запускалось.
- [x] **PR 5a** — ветка `platform/preferences-locale-scripts`, по ревью: модификатор письменности в
  имени POSIX называет письменность (`sr_RS@latin` → `sr-Latn-RS`, `@cyrillic`, `@devanagari`),
  остальные модификаторы и кодировка отбрасываются; строки в
  `a_posix_environment_is_read_as_gettext_reads_it`.
- [x] **PR 6** — ветка `platform/preferences-macos`. Опрос раз в 500 мс снят: источник делится на
  `Send + Sync` `PreferenceSource` (сигнал и `ReadAdmission`) и главнопоточные
  `PreferenceObservers`, которыми владеет хост (`MacOSPlatform` и цикл winit). Владельца будят три
  уведомления: параметры отображения рабочего стола (уменьшение движения и, новое, повышение
  контраста), смена текущей локали (новое: `NSLocale.preferredLanguages` через `preferred_locales`)
  и активация приложения — двойной щелчок не уведомляет, его меняют в System Settings, поэтому он
  перечитывается при возвращении. Неудачное или размотанное чтение ставит одно пробуждение на свой
  срок повтора (`ReadAdmission::retry_deadline`), иначе к нему никто бы не вернулся. Наблюдатели
  кончаются раньше источника — в `MacOSPlatform` порядком полей, в цикле winit при остановке.
  Обёртка уведомления вынесена из `system_notices` в `observation`; её `new` безопасна, обработчик
  обязан быть `Send + Sync`. Только проверка типов (`aarch64-apple-darwin`, с хостом winit и без,
  `--all-targets`); запуск на Mac — vanyastaff/flui#1840.
- [x] **PR 7** — ветка `platform/preferences-ios`. Источник UIKit наблюдает ещё
  `UIAccessibilityDarkerSystemColorsStatusDidChangeNotification` и
  `NSCurrentLocaleDidChangeNotification` и читает `UIAccessibilityDarkerSystemColorsEnabled` и
  `NSLocale.preferredLanguages`. Масштаб текста на iOS — точные ответы `UIFontMetrics` по профилю
  (ADR-0183), скаляра в снимке нет; уведомление о размере контента по-прежнему будит владельца.
  Проверка типов — в CI (`cross-typecheck`, `aarch64-apple-ios`, нужен Apple SDK); не запускалось.
- [x] **PR 8** — ветка `platform/preferences-android`. `FluiView` регистрирует `ContentObserver` на
  масштаб длительности анимаций, тактильный отклик, задержку долгого нажатия и высококонтрастный
  текст, слушатель контраста `UiModeManager` с API 34 и `onApplyWindowInsets`; каждый приходит
  владельцу событием вида и перечитывает предпочтения, коэффициенты прокрутки или отступы, наряду с
  `ConfigChanged` и `Resume`. Каденция 500 мс остаётся, только пока вида нет в окне;
  `poll_timeout` блокирует возобновлённый цикл без отсчёта, пока его не разбудят. Новое: высокий
  контраст (`shared/android_contrast`: уровень контраста или высококонтрастный текст) и языки
  (`LocaleList` с API 24, гейты `LocaleList` и `SystemContrast`). Запуск на Galaxy S21
  (Android 15 / API 35, зонд, логирующий каждую пересборку `MediaQuery` и каждый оборот цикла):
  0 оборотов за 10 с в простое — возобновлённым и после Home; `animator_duration_scale 0`,
  `high_text_contrast_enabled 1`, `contrast_level 0.5` и поворот экрана дошли до `MediaQuery` без
  перезапуска (контраст — за 0,22 с до пересоздания Activity обновлением оверлея Samsung), возврат
  каждого вернул прежнее значение. Запись 23 gaps закрыта. После ревью: вид наблюдает только пока
  прикреплён к окну (регистрация в `onAttachedToWindow` и один отчёт после неё, снятие при
  откреплении и в `detach`), а если Java-установка прошла, но удержать вид не вышло, нативная
  сторона отцепляет его до освобождения хэндла; долг повтора после неудачного чтения
  (`ReadAdmission::retry_deadline`) ограничивает ожидание цикла; готовность вида берётся до
  чтения; запрос отступов, пока ответ в пути, повторяется после его усвоения; задержка долгого
  нажатия читается из настройки, а не из копии `ViewConfiguration`. Повторный прогон на том же
  S21: `long_press_timeout 1000` → 1 с и обратно 500 мс, контраст, высококонтрастный текст,
  движение и поворот доходят вживую; 0 оборотов цикла за 15 с простоя после установления.
- [x] **PR 9** — ветка `platform/preferences-windows`. Снятие приёмника получает вызовы
  уничтожения окна и снятия класса параметрами; `preference_receiver_retirement` проваливает
  `DestroyWindow` на настоящем скрытом окне: окно остаётся, класс не снимается, контекст (и
  разделяемая им инвалидация) жив, поздний `WM_SETTINGCHANGE` ничего не помечает и никого не
  будит. `a_delegate_that_outlives_its_source_is_inert`: делегат WinRT держит только инвалидацию
  источника, которую тот закрывает до отзыва, — переживший источник делегат (как при неудачном
  отзыве) инертен, а преемник будит; сам отказ отзыва не вызывается — WinRT не даёт его
  спровоцировать. Мутации: освобождение контекста при отказе, снятие класса после отказа и
  источник, не закрывающий приём при сбросе, валят каждую свою строку.

  Что собрано и что исполнено для LY8/LY9:

  | Среда | Собрано | Исполнено |
  |:--|:--|:--|
  | Windows | clippy, nextest | на этой машине: источник без окна, адресованный `WM_SETTINGCHANGE`, снятие с отказами |
  | Linux | кросс-clippy с хостом winit и без | WSL Ubuntu 24.04, `xvfb-run`, подменный портал в `dbus-run-session` (16 случаев с **PR 10**) |
  | macOS | кросс-clippy `aarch64-apple-darwin` | нет; vanyastaff/flui#1840 |
  | iOS | `cross-typecheck` в CI (нужен Apple SDK) | нет; vanyastaff/flui#1840 |
  | Android | кросс-clippy, dex | Galaxy S21, Android 15 / API 35 |
  | Web | wasm32 clippy, `wasm-check` | Node против подменной границы браузера; не в браузере |
  | Headless | — | CI Linux |
- [x] **PR 10** — ветка `platform/preferences-acceptance`: строки, которых не хватало приёмке по
  сверке её пунктов с тестами. Linux: случай `text-scale-out-of-range` (портал сообщает 100 до окна
  и −1 вживую — владелец видит «нет наблюдения», затем 1,5; ход владельца запускается после того,
  как источник обработал −1, и до появления 1,5, поэтому источник, который зажимает значение
  вместо отказа, падает и на засеве, и вживую — 3 прогона из 3). Отсутствие хука после `start`:
  `OwnerSignal::drive_in` больше не снимает долг при пустом слоте, регистрация планирует ход —
  `a_change_with_no_turn_callback_reaches_the_next_one_registered`. Commit до доставки:
  `a_revision_is_accepted_before_the_turn_that_delivers_it`. Единицы: строки `normal_and_preserve_use_distinct_duration_timelines`
  со скоростями часов и контроллера; `native_unit_conversions` на Windows (`wheel_preferences` и
  `drag_extent` вынесены из нативного чтения без изменения поведения). Runtime:
  `an_older_or_repeated_preference_revision_is_ignored`,
  `an_exhausted_revision_refuses_without_changing_or_queueing`,
  `two_preference_deliveries_for_one_runtime_never_merge`,
  `queued_preferences_are_not_coalesced_across_an_input`. App: четыре строки `owner_dispatch_matrix`
  — изменение до первого хода, до появления хука, замена хука, паника доставки. Каждую строку валит
  своя мутация (сохранение порога ревизии, насыщение вместо отказа, слияние или вытеснение
  предпочтений в очереди, хук без восстановления после паники, зажим масштаба и т. д.).

## Чего нет в этой серии

Часть функций из закрытого списка ядра (R1.4) на части ОС ещё отсутствует. Каждая — отдельная
спека функции, ранжированная в [docs/gaps.md](../../../gaps.md). Серия даёт
каждой её место в контракте и ядре (D2): пункт закрытого списка, раскладку «каталог фичи с
файлом на платформу» и таблицу хоста, в которой новая позиция не даёт ни одному хосту её
пропустить, `WindowHandler` и `AppHandler`, шов и мост к хосту — и не реализует её. Это не
откладывание содержимого этой спеки: всё, что названо в R1–R11, делают LY20–LY102. Ниже — то, чего R1–R11 не называют. Публичной поверхности под эти функции серия не
вносит (R10.4): модуль `menu`, например, приходит вместе с первой реализацией и потребителем
(design §5).

| Функция | Запись в gaps.md | Что даёт серия | Что остаётся спеке функции |
|---|---|---|---|
| Нативные меню: строка меню, контекстные меню, меню Dock (его отдаёт app delegate) | 11 | пункт D2; резерв модуля `menu` (design §5); `AppHandler` для команд; `keyboard_layout` и его событие остаются `pub(crate)` ядра (LY55), публичными их сделает эта фича; позицию в `Host` и каталог фичи приносит сама фича (таблица без новой позиции не компилируется, поэтому ни один хост её не пропустит) | модель меню, реализация на каждой ОС, прогоны; трей в `flui-system` приходит вместе с этой фичей: без модели меню он только значок (D18) |
| Дочерние, владеемые и всплывающие окна; свой заголовок (hit-testing заголовка, snap layouts) | 10 | пункты D2 «дочерние и всплывающие окна» и «декорации и свой заголовок»; `WindowOptions`, `WindowCommand` и `Availability` в `WindowCapabilities` (LY38–LY43) | родитель и владелец в опциях, hit-testing заголовка, snap, окна за границей окна приложения |
| Системная «назад» как функция (спека «system back») | 3 | ничего на хосте: производитель бита «можно ли вернуться» — навигатор, вне серии, а регистрация хоста без производителя — неподключённая поверхность; место — `WindowHandler`, `AppHandler` и очередь LY62 | регистрация `OnBackInvokedCallback` на Android (API 33, `OnBackAnimationCallback` с API 34, только пока приложение может вернуться), краевой свайп iOS (`UIScreenEdgePanGestureRecognizer`), History API Web с приёмом «одной записи» и ответом dev-сервера для путей без файла (ADR-0255 п. 3, 14, 19), бит «можно ли вернуться», виджеты, навигатор, прогоны по ОС |
| Клавиатура в раскладке: избегание клавиатуры, прокрутка к полю | 1, 16 | хосты публикуют inset клавиатуры в снимке окна (LY66, LY72, LY74) | поведение виджетов, прокрутка, прогоны |
| Перетаскивание наружу и приём от других приложений | 8 | словарь `drag` и `data_transfer` остаётся в контракте (design §5); таблицы предложений переехали в ядро (LY49) | нативные источники и цели (в т. ч. `NSDraggingDestination`), прогоны с другими приложениями |
| Богатый clipboard: HTML, изображения, файлы | 6 | `ClipboardHandle` на шве, текст на каждой ОС, где она его даёт (LY50, LY80, LY84) | форматы, согласование представлений |
| Активация (спека «activation»): URL-схемы, ассоциации файлов, единственный экземпляр, тёплые ссылки | 13 | место — `AppHandler` (LY45), очередь Activity LY62, манифест с `launchMode="singleTop"`; `AppHandler` не получает пустого обязательства `opened` | обязательство `opened(urls, files)` в `AppHandler` и маршрутизатор, `onNewIntent` Android, колбэки сцены iOS (`scene:willConnectToSession:options:`, `scene:openURLContexts:`, `scene:continueUserActivity:`, `windowScene:performActionForShortcutItem:`), регистрация схем и ассоциаций в упаковке, передача аргументов единственному экземпляру, источники на десктопе и Web |
| Разрешения ОС: запрос, состояние, объём | — (крейты возможностей: уведомления, геолокация) | `launch_for_result` и очередь результатов Activity (LY62, LY64); `onRequestPermissionsResult` не пересылается | `request_permissions`, `PermissionState`, `Grant`, `Denial` и `Scope` в форме ADR-0152 п. 2.8 приходят с первой возможностью, которой они нужны |
| Текстовые сервисы: орфография, системное edit-меню, автозаполнение | 18 | IME и text store (D2); `UITextInput` с `UITextInteraction` на iOS (LY68, LY69); textarea Web (LY75) | сами сервисы |
| Внешний вид окна: набор материалов, прозрачность, значок, исключение из захвата | 20 | команда материала фона с `Availability` (LY43) | набор материалов на каждой ОС, прозрачность, значок, захват |
| Нативные виды внутри UI: web view, карта, видео | 19 | ничего (решение владельца; триггеры ухода с `NativeActivity` — в ADR-0255) | всё |

Записи 1, 2, 5 и 7 серия закрывает по мере исполненных прогонов (R10.2); запись 17 — в части
`flui-system` (без трея); записи 23–27, 30 и 31 (планка качества) — задачами LY89–LY95, запись 28 — в
части проводки `input_latency` (LY44). Остальные записи, не названные выше (в их числе 29 —
упаковка, подпись, нотаризация), остаются своими записями: задачи серии обновляют их строки, когда
меняется поддержка.

## Инструкция для открытых веток (LY20 и LY21)

Переименование — не инструмент `xtask`, а команды в описании PR (D21). После каждого шага:

1. `git fetch origin && git merge origin/main`.
2. Повторить на ветке команды `git mv` и `sd` из описания PR для файлов, которые ветка добавила или
   изменила (`git diff --name-only $(git merge-base HEAD origin/main)`); `.stderr` trybuild —
   `TRYBUILD=overwrite`, не текстовой заменой.
3. `cargo heli xtask checks`, clippy и nextest на изменённых крейтах.

Порядок шагов не менять: шаг LY21 на ветке — только после того, как ветка влила LY20. Ветка,
написанная до LY20 и влитая после LY21, применяет обе карты по порядку: сначала карту LY20, затем
карту LY21; иначе строка вроде `--exclude flui-platform` молча нацелится на контракт. Ветка, которая
не может влить LY20 сразу, сообщает владельцу спеки platform-layer; LY21 сдвигается целиком, а не
по частям.

## Проверка требований

| Требование | Задачи |
|---|---|
| R1.1, R1.2 | LY20, LY21 |
| R1.3 | уже верно (`allowed-dependents`); LY20 и LY21 сохраняют |
| R1.4 | LY57–LY61, LY67, LY73 (сервисы вне списка уходят в `flui-system`); ADR-0151 §3 |
| R1.5 | раздел «Чего нет в этой серии»; правило исполнения 7 в каждой задаче |
| R2.1 | LY47, LY48, LY49, LY55 |
| R2.2 | LY47, LY48, LY49, LY50, LY55 |
| R2.3 | LY38, LY40–LY50 (по модулям design §5) |
| R2.4 | LY47 |
| R2.5 | LY25 (запечатывание `Platform`, `HostWindow`), LY38 (`exhaustive_enums`) |
| R2.6, R2.7 | LY39 |
| R2.8 | уже верно; `workspace` сохраняет |
| R2.9 | LY23 |
| R3.1 | LY40, LY41 |
| R3.2 | LY42 |
| R3.3 | LY44 |
| R3.4 | LY45 |
| R3.5 | LY46 |
| R3.6 | LY43 |
| R3.7 | LY38, LY40 |
| R3.8 | LY41 (`on_window_event`), LY45, LY50 (clipboard), LY55, LY61 |
| R3.9 | LY36, LY37 |
| R3.10 | LY44 |
| R4.1 | LY22 (`Host`, `HostPlatform`, `hosts/`), LY25, LY27–LY34 |
| R4.2, R4.3 | LY22 (механизм), LY27–LY33 (применение), LY78, LY82 (новые хосты) |
| R4.4 | LY22, LY34 |
| R4.5 | LY22, LY27–LY34 |
| R4.6 | LY26; LY44 и LY45 входят в ту же точку |
| R4.7 | LY35 |
| R4.8 | LY27–LY33, LY78, LY82 |
| R4.9 | LY24; LY34 (проверка по всему `src/`) |
| R4.10 | LY36, LY100 |
| R4.11 | LY22; сохраняют LY27–LY34, LY78, LY82, LY87 |
| R5.1 | LY50 |
| R5.2 | LY50 |
| R5.3 | LY50 |
| R5.4 | LY50, LY54 |
| R5.5 | LY50, LY52 |
| R5.6 | LY64 |
| R5.7 | LY50, LY51 |
| R5.8 | LY51 |
| R5.9 | LY50, LY52 |
| R5.10 | LY53, LY57 |
| R5.11 | LY53, LY56 |
| R5.12 | LY56 |
| R5.13 | LY54, LY57 |
| R5.14 | LY57–LY60, LY67, LY73 |
| R5.15 | LY101 (фасад); LY57 — первый крейт возможности |
| R2.10 | LY102 (контракт по областям) |
| R6.1 | LY62, LY64 |
| R6.2 | LY56, LY62, LY63 |
| R6.3 | LY65 |
| R6.4 | LY66 |
| R6.5 | LY68–LY73 |
| R6.6 | LY74–LY76 |
| R6.7 | LY63 |
| R6.8 | LY62, LY64, LY66, LY71, LY72 (границы названы в строках задач); раздел «Чего нет в этой серии» |
| R7.1 | LY78–LY85 |
| R7.2 | LY85, LY86 |
| R7.3 | LY87 |
| R8.1–R8.5 | LY20, LY21; R8.4 также LY23 (фраза о реэкспорте) |
| R9 | LY8, LY9 (вне серии, ADR-0172); LY45 сохраняет R9.7 при переносе доставки в `AppHandler` |
| R10.1 | каждая задача (правило 5; перечень ADR — design §10) |
| R10.2 | правило 7 в каждой задаче; LY85 |
| R10.3 | правило 2 и перекладка `globals` в LY25, LY27–LY34 |
| R10.4 | правило 6; LY51, LY64 (PR называет потребителя) |
| R10.5 | правило 9 в каждой задаче |
| R10.6 | правило 8 и колонка «Зависит от» |
| R10.7 | правило 10; LY22 |
| R10.8 | правило 3 в каждой задаче |
| R10.9 | правило 4 в каждой задаче |
| R11.1 | LY88 |
| R11.2 | LY89 |
| R11.3 | LY90 |
| R11.4, R11.5 | LY91 |
| R11.6 | LY92 |
| R11.7 | LY93 |
| R11.8 | LY94 |
| R11.9 | LY95 |
| R11.10 | LY96 |
| R11.11 | LY97 |
| R11.12 | LY99 |
| R11.13 | LY98 |
| R11.14 | LY100 |

Каждая задача LY20–LY102, кроме не занятой LY77, названа хотя бы в одной строке.
