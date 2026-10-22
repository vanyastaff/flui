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
| **LY28** [P] | **`window/`** (D13, R4.1, R4.8). Модель: `shared/window_state` (299; единственный `mode_of`) → `window/state.rs`, `shared/visibility` (130) → `window/visibility.rs`, `shared/window_installation` (92; его называет `flui-app`) → `window/installation.rs`, `config.rs` (240) → `window/config.rs`. Windows: `windows/{window,window_ext,window_frame,window_limits,window_modes,window_placement,window_visibility,appearance}.rs` (3 176 + 368 + 291 + 192 + 94 + 342 + 256 + 58) и `shared/{windows_window_limits,hwnd_affinity}` (333 + 286) → `window/windows/{mod,proc,limits,backdrop}.rs` (`window.rs` делится на `mod.rs` и `proc.rs` в пределах 3 000 строк; класс окна и сырая оконная процедура живут в `owner/windows.rs` (LY27), `proc.rs` реализует её хук для сообщений самого окна, ввод, текст, drag и lifecycle — свои хуки в своих файлах, а `hosts/windows.rs` их регистрирует). DirectManipulation (`windows/direct_manipulation.rs`, 758, его `tests.rs` и `shared/direct_manipulation`, 495) — источник ввода и переезжает в LY29. Apple: `macos/{window,window_tiling,window_ext,display_pass,liquid_glass}.rs` (2 994 + 413 + 306 + 85 + 243), `ios/{window,status_bar}.rs` (1 350 + 174) и `shared/{appkit_window_geometry,appkit_window_mode,screen_edges}` (372 + 409 + 43) → `window/apple/{mod,macos,ios,geometry,liquid_glass}.rs` (`macos/window.rs` делится с запасом от предела; общее у macOS и iOS — `apple/mod.rs`). Android: `android/{window,safe_area,gesture_exclusion,system_chrome}.rs` (700 + 204 + 63 + 122) → `window/android.rs`. Web: `web/{window,registration}.rs` (881 + 531) → `window/web.rs`. winit: `winit/{window,window_plan}.rs` (914 + 831) → `window/winit.rs`. Linux (живой слой): `linux/x11_window_probe.rs` (387) с тестами и `linux/lifecycle_model/x11_window_mode.rs` (238) → `window/linux/x11.rs`. headless: `headless/window_policy.rs` (36) и `MockWindow` из `headless/platform.rs` → `window/headless.rs`. Системный chrome окна (`android/system_chrome.rs`, `ios/status_bar.rs`, `FakeSystemChrome`) живёт в тех же файлах: позицию `Chrome` таблицы называет `window/`. `liquid_glass` и `MacOSWindowExt` остаются внутри `window/apple/` до LY36; записи `globals` переезжают | LY27 | `crates/flui-native/src/window/*`, `platforms/{windows,macos,ios,android,web,winit,linux,headless}/*` (уходят перенесённые файлы), `shared/*` (уходят), `config.rs` (уходит), `tests/*` (уходят перенесённые), `hosts/*`, `Cargo.toml` (`globals`) | `module-dag`, `file-length`, `globals`, `cross-typecheck`, `markers` | перенесённые тесты правил (`hwnd_affinity`, `appkit_window_*`, `screen_edges`, `window_state`, `window_installation`) зелёные на Linux; `tests/` conformance и трассы Windows без правок | L + W + T; Dev: macOS, iOS (Simulator), Android |
| **LY29** | **`input/`** (D13, R4.1, R4.8). Модель: `shared/pointer_assembly/` (9 файлов, 2 729) → `input/assembly/`, `shared/{keys,keyboard_adapter}` (526 + 162) → `input/keys.rs`, `shared/input_availability` (152) → `input/availability.rs`, `shared/scroll` (38) → `input/scroll.rs`, `shared/events` (30: метка времени события и идентичность первичной мыши; её называет `flui-app` в `owner_dispatch.rs`) → `input/mod.rs`. Windows: `windows/events.rs` (1 734) и `windows/platform/{pointer_messages,keystroke_queue}.rs` (144 + 111) → `input/windows/{mod,raw}.rs`; `windows/direct_manipulation.rs` (758), его `tests.rs` и `shared/direct_manipulation` (495) → `input/windows/direct_manipulation.rs` (viewport на окно создаётся хуком `owner/windows.rs` при создании окна; его колбэки — источник pan и zoom для сборки указателя). Apple: `macos/{events,view}.rs` (524 + 755), `ios/events.rs` (284) и `shared/keys_macos` (401) → `input/apple.rs`. Android: `android/input.rs` (550) и `shared/{android_motion,android_keys,android_scroll}` (923 + 45 + 149) → `input/android.rs`. Web: `web/events.rs` (1 544) и `shared/web_pinch` (341) → `input/web.rs`. winit: `winit/events.rs` (1 670) → `input/winit.rs`; тест `winit/events/keyboard_tests.rs` (775) на таблицу `keys_macos` переезжает в `input/apple.rs`: файл платформы не называет другую платформу. headless: `HeadlessInputDevices` из `headless/platform.rs` → `input/headless.rs`. Форма доступа к `assembly/` для отдельного тестового бинаря со счётчиком аллокаций и признака `native-capture` решается в PR; записи `globals` переезжают | LY28 | `crates/flui-native/src/input/*`, `platforms/{windows,macos,ios,android,web,winit,headless}/*` (уходят перенесённые файлы), `shared/*` (уходят), `tests/*` (уходят перенесённые), `crates/flui-app/src/app/runner/owner_dispatch.rs`, `Cargo.toml` (`globals`) | `module-dag`, `file-length`, `globals`, `cross-typecheck`, `markers` | перенесённые тесты (`pointer_assembly`, `direct_manipulation`, `android_motion`, `android_keys`, `web_pinch`, `input_capabilities`) рядом с моделью и чистыми частями; записи нативных трасс проходят без правок | L + W + T; Dev: macOS, iOS, Android, Web |
| **LY30** [P] | **`text/`** (D13, R4.1, R4.8). Модель: `shared/ime_text` (45; зеркало текстового поля Android и Web) → `text/ime.rs`, `text_sizing.rs` (202) → `text/sizing.rs`. Windows: `windows/text_services/{mod,document,input_scope,probe,tests}.rs` (4 534, из них тесты 1 361) и `shared/text_geometry` (143) → `text/windows/{mod.rs,tsf/}`. Apple: `macos/text_input.rs` (781) → `text/apple/macos.rs`, `ios/text_sizing.rs` (58) → `text/apple/ios.rs`. Android: `android/{text_input,text_sizing}.rs` (516 + 619) и `shared/android_ime` (399) → `text/android.rs`. Web: `web/text_input.rs` (1 080) и `shared/web_text` (105) → `text/web.rs`. headless: `FakeTextInput` из `headless/platform.rs` → `text/headless.rs`. winit отдаёт `WinitTextInput` (разрешение IME и область курсора) → `text/winit.rs`, позиция `Text` его таблицы называет его; позиция `Text` получает трейт фичи текста вместо маркера `host::Text`. Тесты `android_ime`, `web_text`, `text_input_mapping` — рядом с моделью и чистыми частями; записи `globals` переезжают | LY27 | `crates/flui-native/src/text/*`, `platforms/{windows,macos,ios,android,web,headless}/*` (уходят перенесённые файлы), `shared/*` (уходят), `text_sizing.rs` (уходит), `tests/*` (уходят перенесённые), `Cargo.toml` (`globals`) | `module-dag`, `file-length`, `globals`, `cross-typecheck` | перенесённые тесты `android_ime`, `web_text`, `text_input_mapping` зелёные на Linux; Win32 `text_services/{probe,tests}.rs` без правок смысла — W | L + W + T; Dev: macOS, Android, Web |
| **LY31** [P] | **`display/`** (D13, R4.1, R4.8). Модель: `shared/display_source` (460: реестр идентичностей и последний снимок, которые держит каждый нативный источник) → `display/source.rs`. Apple: `shared/{apple_display_identity,display_link_rate,display_period}` (253 + 304 + 61), `macos/display.rs` (272) и `ios/display.rs` (298) → `display/apple/{mod,macos,ios}.rs` (общее у macOS и iOS — `apple/mod.rs`). Windows: `windows/display.rs` (572), `windows/platform/display_message_tests.rs` (151) и `shared/windows_display` (184) → `display/windows.rs`. Android: `android/display.rs` (576) и `shared/android_display` (317) → `display/android.rs`. Web: `web/{display,display_events,features}.rs` (375 + 706 + 125) и `shared/{dppx_query,refresh_estimate,screen_identity,web_feature}` (102 + 560 + 187 + 165) → `display/web.rs`. winit: `winit/display_{linux,macos,windows}.rs` (151 + 70 + 65) → `display/winit.rs` (`display_linux.rs` читает источник Linux из `display/linux/`: единственное ребро между файлами платформ, его называет `module-dag` поимённо до LY87). Linux (живой слой): `linux/display/*` (Wayland `wl_output` и `zxdg_output_v1`, X11 RandR; 11 файлов, около 3 500 строк) → `display/linux/`. headless: `headless/display.rs` (191) → `display/headless.rs`. Тесты правил и источников (`windows_display`, `refresh_estimate`, `screen_identity`, `dppx_query`, `display_link_rate`, `display_period`, `android_display`, `apple_display_identity`, `linux/display/*/tests.rs`) — рядом; записи `globals` переезжают | LY27 | `crates/flui-native/src/display/*`, `platforms/{windows,macos,ios,android,web,winit,linux,headless}/*` (уходят перенесённые файлы), `shared/*` (уходят), `tests/displays.rs`, `Cargo.toml` (`globals`) | `module-dag`, `file-length`, `globals`, `cross-typecheck`, `wasm-check` | перенесённые тесты правил и источников; `tests/displays.rs` без правок смысла; `wasm-test` и `web_browser` без правок смысла | L (+ wasm32) + W + T; Dev: macOS, iOS, Android, Web (Chrome) |
| **LY32** [P] | **`clipboard/`, `drag/`, `accessibility/`, `haptics/`** (D13, R4.1, R4.8). `clipboard/`: `shared/clipboard_lock` (88; общий замок Win32 и winit с `arboard`) → `lock.rs`; `windows/clipboard.rs` (456) → `windows.rs`; `macos/clipboard.rs` (245) и `ios/clipboard.rs` (71) → `apple.rs`; `web/clipboard.rs` (60) → `web.rs`; `winit/clipboard.rs` (128) → `winit.rs`; headless-приёмник в памяти → `headless.rs`; внутрипроцессная заглушка Android остаётся в его таблице до LY50. `drag/`: `windows/drag.rs` (785), `windows/drag/{com,tests}.rs` (557 + 1 395) и `shared/ole_drag_drop` (453) → `windows.rs`; `android/drag.rs` (556) и `shared/android_drag` (307) → `android.rs`; `web/drag.rs` (642) и `shared/html_drag_drop` (296) → `web.rs`; `winit/data_transfer.rs` (484) → `winit.rs`; `headless/drag.rs` (469) → `headless.rs`. `accessibility/`: `shared/accessibility_bridge` (114) → `bridge.rs`; `windows/accessibility.rs` (210) → `windows.rs`; `macos/accessibility.rs` (227) → `apple/macos.rs`; `linux/accessibility.rs` (189; AT-SPI, живой слой) → `linux.rs`; `FakeAccessibility` из `headless/platform.rs` → `headless.rs`. `haptics/`: `shared/haptic_degradation` (1 242) → `degradation.rs`; `windows/haptics.rs` (768) → `windows.rs`; `macos/haptics.rs` (58) и `ios/haptics.rs` (452) → `apple/{macos,ios}.rs`; `android/haptics.rs` (519) → `android.rs`; `web/haptics.rs` (103) → `web.rs`; `FakeHaptics` (headless-приёмник, LY48) → `headless.rs`. Позиции, которых у платформы нет сегодня (drag на macOS и iOS, доступность Android, iOS и Web, haptics Linux и winit), в таблицах — `Absent<F>`. Записи `globals` переезжают | LY27 | `crates/flui-native/src/{clipboard,drag,accessibility,haptics}/*`, `platforms/{windows,macos,ios,android,web,winit,linux,headless}/*` (уходят перенесённые файлы), `shared/*` (уходят), `tests/*` (уходят перенесённые), `Cargo.toml` (`globals`) | `module-dag`, `file-length`, `globals`, `cross-typecheck`, `markers` | перенесённые тесты (`ole_drag_drop`, `android_drag`, `html_drag_drop`, `haptic_degradation`) рядом с моделью и чистыми частями; тесты слоя Linux (AT-SPI) и `headless_drag` без правок | L + W + T; Dev: macOS, Android, Web |
| **LY33** [P] | **`preferences/` и `lifecycle/`** (D13, R4.1, R4.8). `preferences/`: `shared/preference_read` (196; допуск опроса, общий для ограниченных сэмплеров) → `read.rs`; Windows: `windows/preferences.rs` (457), `windows/preferences/{languages,tests}.rs` (180 + 279) и `windows/platform/preference_retry_tests.rs` (242) → `windows/` (приёмник широковещательных сообщений уже в `owner/windows.rs`, LY27: предпочтения подписываются на него через `owner/`); Apple: `macos/preferences.rs` (115) и `ios/preferences.rs` (128) → `apple/{macos,ios}.rs`; Android: `android/preferences.rs` (445) и `shared/android_text_weight` (82) → `android.rs`; Web: `web/preferences.rs` (83) → `web.rs`; Linux: наблюдение портала Settings из `winit/preferences.rs` (239; D-Bus) → `linux.rs` (им пользуются winit, Wayland и X11); headless: производитель из LY8 в `headless/*` → `headless.rs`. `lifecycle/`: `shared/system_notice` (321; путь, которым уведомления любого хоста доходят до приложения) → `notice.rs`; Windows: `windows/host_events.rs` (481) и `shared/{windows_host_messages,windows_memory_watch}` (376 + 207) → `windows.rs`; Apple: `macos/system_notices.rs` (246), `shared/{terminate_gate,dispatch_memory_pressure}` (277 + 76) и `shared/apple_scene_error` (15; сбой уничтожения сцены UIKit) → `apple/{macos,ios}.rs`; Android: `shared/android_lifecycle` (446) и `android/memory.rs` (37) → `android.rs`; Web: `web/page_events.rs` (87) → `web.rs`; Linux (живой слой): `linux/host_notices/*` (logind, давление памяти), `linux/lifecycle_model/{mod,logind,memory_pressure}.rs` и `winit/platform/linux_notices.rs` (272) → `linux.rs`; headless: `HeadlessHostEvents` → `headless.rs`. Записи `globals` переезжают | LY27, LY9 | `crates/flui-native/src/{preferences,lifecycle}/*`, `platforms/{windows,macos,ios,android,web,winit,linux,headless}/*` (уходят перенесённые файлы), `shared/*` (уходят), `Cargo.toml` (`globals`, features) | `module-dag`, `file-length`, `globals`, `markers` | перенесённые тесты правил и шины logind (`bus_tests`); `linux_preferences`, `preferences`, `windows_host_events` без правок смысла | L + W + T; Dev: macOS, Android, Linux (ручной прогон под X11) |
| **LY34** | **`version/`; `shared/` и `platforms/` исчезают** (D13, R4.1, R4.4). `shared/os_version/` (689; пороги и гейты версий ОС, ADR-0244) и `ios/os_version.rs` (18) → `version/gate.rs` (модули `windows`, `apple`, `android` внутри, под `cfg(any(<os>, test))`); генерируемый `shared/platform_floor.rs` → `version/floor.rs`, а `tools/xtask/src/platform_floor.rs` пишет новый путь; `version/` входит в проверку `module-dag` и не зависит от фич. `storage/` (`file_store.rs`, `page_store.rs`, `os_error.rs`) остаётся как есть. Удаляются `shared/mod.rs` (29 строк `cfg`) и `platforms/mod.rs`; на верхнем уровне остаются `error.rs` и `table_test.rs`. 21 файл `tests/` с `flui_native::shared::*` исчезает: тесты правил — модулями рядом с моделями и чистыми частями, conformance и трассы — через публичный API. Записи `globals` (19, ключ — путь модуля) и пути в allowlists xtask переехали; `module-dag` охватывает все каталоги ядра, списка переходных исключений нет (поимённое ребро `display/winit.rs` → `display/linux/` живёт до LY87); путь `include_bytes!` на `classes.dex` и `android_dex.rs` уже указывают на `owner/android/java_view.rs` (LY27), а `android-dex` подтверждает, что ни один путь не ведёт в `platforms/` или `shared/`; ARCHITECTURE.md ядра описывает новое устройство | LY27, LY28, LY29, LY30, LY31, LY32, LY33 | `crates/flui-native/src/{version/*,hosts/*}`, `shared/*` и `platforms/*` (удаляются), `tests/*`, `Cargo.toml` (`globals`), `tools/xtask/src/platform_floor.rs`, `tools/xtask/allowlists/*` | `module-dag`, `globals`, `platform-floor`, `android-dex`, `file-length`, `markers`; `rg 'unimplemented!'` по `src/` | перенесённые тесты; `tests/` не называет `flui_native::shared` и `flui_native::platforms`; записи нативных трасс без правок | L + T |
| **LY35** | **Слияния дублей** (D13, R4.7). Внутри моделей фич. Режим окна из флагов — одно определение вместо четырёх: `mode_of` в `window/state.rs` остаётся, а `mode_of` в `window/apple/geometry.rs` (было `appkit_window_mode.rs`), `window_mode` в `window/linux/x11.rs` (было `x11_window_mode.rs`) и `public()` в `window/windows/mod.rs` (было `windows/window.rs`) уходят. Период обновления — один конструктор из Гц, мГц и интервала с одной проверкой (конечное, больше нуля) и один издатель в `display/source.rs` вместо `display/android.rs::refresh_period`, `display/apple/ios.rs::period_of` (было `display_link_rate`), `display/apple/macos.rs::refresh_period_for_screen`, `display/linux/wayland_model.rs::refresh_period`, `display_period`, `windows_display` и `refresh_estimate`. `Location` клавиши — одна таблица в `input/keys.rs` вместо `keyboard_adapter` (6 мест), `keys` (4), `input/web.rs` (4; было `web/events.rs`) и `input/windows/` (1; было `windows/events.rs`). Шаг колеса — по одному правилу в `input/scroll.rs` вместо `scroll`, `winit/events.rs::wheel_step`, `web/events.rs` и `android_scroll`; видимость — одно правило в `window/visibility.rs` | LY34 | `crates/flui-native/src/{window,display,input}/*` | `module-dag`, `globals`, `file-length` | табличные тесты `mapping` (одни для всех потребителей); записи нативных трасс проходят без изменений | L + W + T |
| **LY36** | **OS-типы `pub(crate)`** (D11, R4.10, R3.9). Скрыть: `win32::{HWND,DWMWINDOWATTRIBUTE,DwmExtendFrameIntoClientArea,DwmSetWindowAttribute,MARGINS}`, `WindowsWindow::hwnd`, `toggle_fullscreen_for_hwnd`, `WindowsAccessibility::new(HWND)`, `MacOSPlatform::app`, `AndroidPlatform::{new(AndroidApp),app}`, `WebWindow::canvas`, реэкспорты `accesskit` через `PlatformAccessibility`, `FakeAccessibility::published`, `liquid_glass`, а также имена, которые не называет никто вне крейта и которых нет в сигнатурах публичных трейтов (`WindowConfiguration`, `FullscreenMonitor`, `Headless*`); `PlatformReadyCallback` становится `AppHandler::ready` (LY45); остальные безымянные снаружи имена (`PlatformExecutor`, `WaitError`, `PlatformResult`) уходят вместе с `Platform::background_executor` в LY61. `AndroidApp` flui-app передаёт через вход ядра, форму которого называет PR. `as_any` переезжает с `PlatformWindow` на `HostWindow`; расширения ОС реализованы поверх него и возвращают `Option`. Гейт API-замыкания проверяет rustdoc-JSON ядра с `--document-hidden-items` | LY34, LY39 | `crates/flui-native/src/{lib.rs,host_window.rs,hosts/*,owner/*,window/*,accessibility/*}`, `crates/flui-platform/src/platform_window.rs` (удаляется `as_any`), `crates/flui-app/src/app/runner/*`, `examples/*` | гейт API-замыкания (расширен на ядро), `workspace`, `cross-typecheck` | rustdoc-JSON: пусто; расширение Windows на окне headless даёт `None` | L + W + T |
| **LY37** | **Escape hatch** (D11, R3.9). Фасад публикует `flui::native::<os>` за `cfg` и фичей `native` (Evolving): `WindowsWindowExt`, `MacOSWindowExt` как расширения поверх `dyn HostWindow`, чьи методы принимают и возвращают только типы FLUI (прогресс на панели задач, материал, вкладки macOS), а сырые handle окна и дисплея отдают только через трейты `raw-window-handle` (ADR-0089 §2); опции создания одной ОС — типизированное значение этой ОС в слоте `WindowOptions` (ADR-0254 п. 8; форму слота называет PR); примеры `windows11_demo`, `resize_jitter_probe` переходят на `flui::native::windows`; строка в `docs/extending.md` и в `tests/surface.rs` | LY36 | `src/*` (фасад), `crates/flui-platform/src/window.rs` (слот), `crates/flui-native/src/window/{windows,apple}/*` (расширения), `examples/*`, `docs/extending.md` | `workspace`, `reach`, `facade-combos`, `doc-strict` | чужой хост → `None`/`Unsupported`, не паника; doctest каждого расширения; фича `native` выключена по умолчанию | L + W + T |

### Контракт

| ID | Задача | Зависит от | Крейты и файлы | Гейты | Тесты | Где |
|---|---|---|---|---|---|---|
| **LY38** | **Один словарь отсутствия; закрытость как решение** (D5, D6, R3.7, R2.5). Модуль `unsupported`: `Unsupported { reason }` (конструируется, не пишется литералом; конструктор с именем возможности `Unsupported::of::<C>` добавляет LY50), `UnsupportedReason { NotOnThisPlatform, NotOnThisDevice, NotDeclared { what }, Unavailable, NotRegistered, NoWindow }` (`NotOnThisDevice` — ОС или железо этого устройства не умеет, номер версии не называется), `Availability`. Встраивается в `WindowCommandError`, `WindowShowError`, `CursorError`, `DragError`, исход haptics, `StorageError`, `PreferenceQueryError`, `TextStoreHostError` вместо девяти unit-вариантов. `Absent<F>` (`host.rs`, LY22) отвечает `Unsupported { NotOnThisPlatform }` в каждой позиции, где сегодня отвечает словарём позиции; оконные позиции таблицы (`Text`, `Accessibility`, `Haptics`, `Chrome`) становятся тем, что отдаёт окно: у `HostWindow::accessibility` и `PlatformWindow::{text_input, haptics, system_chrome}` нет умолчаний, окно без позиции отдаёт `Absent` своей таблицы. `clippy::exhaustive_enums` включён на уровне deny; каждое из 26 закрытых перечислений получает `#[non_exhaustive]` или `#[allow]` с причиной (W3C-закрытые `KeyState`, `Location` — кандидаты на `allow`). Версия ОС в контракт не попадает (ADR-0244) | LY34 | `crates/flui-platform/src/{unsupported.rs,window.rs,window_state.rs,drag.rs,haptics/*,storage.rs,preferences/geometry.rs,text_store/host.rs,input_capabilities/*,…}`, хосты и потребители ошибок | `workspace`, `reach`, `clippy` (`exhaustive_enums`) | таблица «каждая ошибка несёт причину»; conformance окна и haptics; существующие | L + T |
| **LY39** [P] | **Гейт API-замыкания** (ADR-0089 §6, R2.6). Новая задача xtask по rustdoc-JSON: публичные сигнатуры контракта называют upstream-типы только из ADR-0089 §2; `ui-events`, `keyboard-types`, `dpi` в списке исключений поимённо со ссылкой на спеку pointer-vocabulary, список только сокращается; тот же механизм принимает проверку OS-типов в ядре (LY36). В CI — отчёт `cargo-semver-checks` по контракту (R2.7) | LY21 | `tools/xtask/src/api_closure*` (новый), `.github/workflows/ci.yml`, манифесты контракта (`reach-forbid`) | сама задача: `api-closure` (+ `--self-test`) | самотест: посаженный `ui-events` в публичной сигнатуре и посаженный OS-тип в ядре; исключение, которое больше не нужно, — ошибка | L |
| **LY40** | **Снимок окна: полнота** (D4, R3.1). `WindowState` получает все оконные факты, которые сегодня отдают геттеры или парные `on_*`: размеры и масштаб (`WindowMetrics`), внешний вид, дисплей, safe-area insets, состояние исполнения, фокус, видимость, наведение; каждый факт сохраняет правило своей записи (тема — пользовательская, как применена к окну, ADR-0222; дисплей — по идентичности снимка дисплеев, ADR-0236; состояние исполнения не зависит от фокуса и видимости, ADR-0035; нерисуемое значение — нулевой размер, неконечный или неположительный масштаб — не публикуется); `WindowCapabilities` входит в `WindowState` (токен привязывается к ревизии, LY42); `activates` и сходные признаки — `Availability`, а не `bool`. Одно `on_state_changed` без данных; хост публикует факты одной ревизией и доставляет изменение синхронно внутри OS-колбэка там, где его даёт ОС (Win32 modal loop). Геттеры живут до LY41 (аддитивный шаг) | LY38 | `crates/flui-platform/src/{window_state.rs,platform_window.rs}`, `crates/flui-native/src/window/*` | `workspace`, `reach`, `cross-typecheck` | conformance окна: снимок согласован с readback ОС на каждом хосте; Windows — живой resize в modal loop | L + W + T |
| **LY41** | **Потребители на снимок; удаление вторых истин** (D4, D10, R3.1). Удалить геттеры `is_focused`, `is_visible`, `physical_size`, `logical_size`, `scale_factor`, `metrics`, `appearance`, `display_id`, `safe_area_insets`, `execution_state`, `is_active`, `is_hovered`, `capabilities`; `mouse_position` и `modifiers` (положение и модификаторы берутся из потока ввода); регистрации `on_resize`, `on_active_status_change`, `on_visibility_status_change`, `on_hover_status_change`, `on_appearance_changed`, `on_display_changed`, `on_safe_area_change`, `on_execution_state_change` (остаются шесть регистраций по обязательствам, их заменит LY44); потребитель, которому нужно знать, какой факт изменился, сравнивает новый снимок с прежним (равенство ревизий); методы-близнецы `show`, `activate`, `close` и `WindowShowError`; `WindowEvent` и `Platform::on_window_event`. `flui-app` (`direct.rs`, `runner/{window_install,desktop,secondary_window,android,ios,web}.rs`) подписывается один раз на запуск; `presentation.rs` читает снимок; 15 реализаций `PlatformWindow` вне ядра (ADR-0254) теряют методы; команды show, activate, close идут только через `WindowControl` | LY40, LY26 | `crates/flui-platform/src/{platform_window.rs,window.rs}`, `crates/flui-native/src/window/*`, `crates/flui-app/src/app/**`, `crates/flui-runtime/src/presentation.rs`, `crates/flui-interaction`, `crates/flui-testing`, тестовые двойники в `flui-*` | `workspace`, `reach`, `markers` | headless: смена фокуса видна одним `state_changed` и одним снимком; путь вето закрытия (ADR-0123) через `WindowControl`; `rg` по удалённым именам пуст | L + W + T |
| **LY42** | **`AdmittedCommand`** (D4, R3.2). `WindowCommand::admit(&WindowCapabilities)` возвращает `AdmittedCommand`, который строит только `admit`; `apply` принимает его; ошибки разделены: `AdmissionError { Unsupported, InvalidGeometry }` для вызывающего, `ApplyError { Closed, Superseded, Platform }` для хоста, где `Superseded` — возможности сменили ревизию между допуском и применением на следующем owner-ходе, команду надо допустить заново (гонка «допустили на ревизии N, применили на N+1» не прячется в `Platform` и отличима от сбоя ОС); токен привязан к ревизии снимка, а ОС оставляет за собой последнее слово через `Platform` и следующий снимок. Место вызова — `WindowControl::submit` в `flui-app`: допуск при подаче против возможностей снимка, применение на следующем ходе владельца в порядке подачи; `submit` возвращает `Result<(), SubmitError>` (`Admission(AdmissionError)` или `Closed`); `WindowCommand::Close` заменяет метод `close` | LY41 | `crates/flui-platform/src/{window_state.rs,platform_window.rs}`, `crates/flui-app/src/app/window_control.rs`, файлы платформ в `window/` (`apply`) | `workspace`, `reach` | trybuild: `AdmittedCommand` нельзя построить снаружи и голую `WindowCommand` нельзя отдать в `apply`; conformance «`Unsupported` ровно тогда, когда возможность ложна»; смена возможностей между допуском и применением даёт `Superseded` | L + W + T |
| **LY43** | **Покадровые глаголы и материал фона** (D4, D10, R3.6). `set_frame_rate_hint` и `set_input_regions` возвращают `Result<(), Unsupported>` и имеют `Availability` в возможностях окна (регионы — в `InputCapabilities`); одна кроссплатформенная команда материала фона вместо `set_background_appearance` с `Availability` в возможностях (набор материалов называет PR; материалы одной ОС достижимы через `flui::native::<os>`, LY37); `WindowBackgroundAppearance` (в т. ч. `MicaBackdrop`, `MicaAltBackdrop`) удалён (ADR-0254 п. 3, 8). Windows: отказ `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE)` становится `Unsupported(NotOnThisDevice)` вместо записи в журнал; `liquid_glass` остаётся в `window/apple/liquid_glass.rs` | LY42 | `crates/flui-platform/src/{platform_window.rs,window.rs,window_state.rs}`, `crates/flui-native/src/window/{windows,apple}/*`, `crates/flui-app` | `workspace`, `reach`, `cross-typecheck` | conformance; тест на таблице версий (`Since<F>`): ниже порога — `NotOnThisDevice`; Windows — живая смена материала | L + W + T |
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
| **LY78** | **Wayland: основа** (D20, R7.1). Соединение, привязки глобалов, цикл событий на owner-потоке с пробуждением (по образцу ADR-0249), окно `xdg-shell`, снимок и команды окна, дисплеи на живом слое Linux (`display/linux/`), таблица `Wayland` в `hosts/linux.rs` (тип на каждую позицию, чего нет — `Absent<F>`) и `HostWindow`, цель поверхности для wgpu (raw-handles), выбор хоста в `current_platform()` | LY28, LY31, LY45, LY46 | `crates/flui-native/src/{owner,window}/linux/wayland.rs`, `hosts/linux.rs`, `lib.rs`, `Cargo.toml` (фичи `wayland`) | `module-dag`, `globals`, `file-length`, `deps` | conformance окна на безголовом композиторе; правила — на хосте | L; Dev: Linux (Wayland) |
| **LY79** | **Wayland: ввод** (D20). Клавиатура (xkb, раскладка), указатель, касание, перо (`tablet-v2`), жесты pinch и pan (`pointer-gestures`), форма курсора (`cursor-shape`), сборка через `pointer_assembly` | LY29, LY78 | `crates/flui-native/src/input/linux/wayland.rs` | `module-dag`, `file-length` | `assert_backend_conforms` (ввод); записи нативных трасс Wayland | L; Dev: Linux (Wayland, сенсорный ввод или перо при наличии) |
| **LY80** | **Wayland: IME, clipboard, drag and drop** (D20, D8). `text-input-v3` и text-store host; провайдер clipboard и primary selection через `wl_data_device`; DnD как цель и источник | LY30, LY32, LY50, LY78 | `crates/flui-native/src/{text,clipboard,drag}/linux/wayland.rs` | `module-dag`, `file-length` | conformance clipboard и DnD | L; Dev: Linux (Wayland) |
| **LY81** | **Wayland: окно и экран** (D20). Декорации (`xdg-decoration`, запасные клиентские), дробный масштаб, видимость и перекрытие, темп по `frame`-колбэкам, перенос окна между выходами одного масштаба, экспорт `xdg-foreign` для родителя порталов, AT-SPI (живой слой) | LY32, LY78 | `crates/flui-native/src/{window,display}/linux/*`, `accessibility/linux.rs` | `module-dag`, `file-length` | conformance окна и дисплеев | L; Dev: Linux (Wayland, два выхода) |
| **LY82** [P] | **X11: основа** (D20). Соединение, цикл событий на owner-потоке, окно, EWMH-состояние и команды, дисплеи (RandR, живой слой), таблица `X11` в `hosts/linux.rs` и `HostWindow`, цель поверхности, AT-SPI | LY28, LY31, LY45, LY46 | `crates/flui-native/src/{owner,window}/linux/x11.rs`, `hosts/linux.rs`, `Cargo.toml` (фича `x11`) | `module-dag`, `globals`, `file-length`, `deps` | conformance окна на Xvfb с оконным менеджером | L; Dev: Linux (X11) |
| **LY83** | **X11: ввод** (D20). XInput2 (касание, перо, плавная прокрутка), XKB, указатель, курсор | LY29, LY82 | `crates/flui-native/src/input/linux/x11.rs` | `module-dag`, `file-length` | `assert_backend_conforms` (ввод); записи нативных трасс X11 | L; Dev: Linux (X11) |
| **LY84** | **X11: IME, clipboard, drag and drop** (D20, D8). XIM/IBus для IME и text-store host; провайдер clipboard и primary (селекции, INCR); XDND как цель и источник | LY30, LY32, LY50, LY82 | `crates/flui-native/src/{text,clipboard,drag}/linux/x11.rs` | `module-dag`, `file-length` | conformance clipboard и DnD | L; Dev: Linux (X11) |
| **LY85** | **Conformance на обоих и parity** (D20, R7.1). `assert_backend_conforms` и записи нативных трасс на Wayland и X11; `docs/platform-parity.md` и `docs/gaps.md` (запись 5) обновлены по исполненным прогонам; ✅ только после них | LY79, LY80, LY81, LY83, LY84 | `crates/flui-native/tests/*`, `docs/platform-parity.md`, `docs/gaps.md` | `platform-test`, `docs-paths` | вся conformance-таблица на двух хостах | L; Dev: Linux (Wayland и X11) |
| **LY86** | **CI на нативном Linux** (D20, R7.2). Job CI исполняет нативный хост (Wayland и X11) вместо winit | LY85 | `tools/xtask/src/tasks.rs`, `.github/workflows/*` | `ci-verify` | job CI зелёный на нативном хосте | L |
| **LY87** | **Удаление winit** (D20, R7.3). Удалить файлы `winit` в фичах (`owner`, `window`, `input`, `display`, `clipboard`, `drag`), таблицу `hosts/winit.rs`, фичу `winit-backend` и зависимости (`winit`, `arboard`, `ui-events-winit`, `crossbeam-channel`, `zbus` и `futures-util`, если больше никому не нужны), `[[test]]` и `[[example]]` с `required-features = ["winit-backend"]`, `WinitPlatform` в xtask, пропуски в `deny.toml`, разделы документов; общие модули, которыми пользовался только winit, упрощаются; `preferences/linux.rs` и `lifecycle/linux.rs` остаются: ими пользуются нативные хосты | LY86 | `crates/flui-native/{Cargo.toml,src/{owner,window,input,display,clipboard,drag}/winit.rs,src/hosts/winit.rs,examples/*,tests/*}`, `crates/flui-app/Cargo.toml`, `tools/xtask/*`, `deny.toml`, `docs/*` | `deps`, `workspace`, `globals`, `checks`, `docs-links` | существующие наборы; `rg winit` пуст вне архивных корней и ADR | L |

### Планка качества

Задачи D24 и D25: качества, которые должны быть у каждой ОС сразу. Они не вводят новых пунктов
закрытого списка, а доводят его существующие области до планки с гейтом или прогоном (R11).

| ID | Задача | Зависит от | Крейты и файлы | Гейты | Тесты | Где |
|---|---|---|---|---|---|---|
| **LY88** [P] | **Доступность всегда включена** (D24, R11.1). Удалить фичу `a11y` из `flui-native` (`a11y = [dep:accesskit_unix, dep:accesskit_windows, dep:accesskit_macos]`), `flui-app` (`a11y = ["flui-native/a11y"]`) и фасада: адаптеры AccessKit становятся безусловными зависимостями своих ОС; AT-SPI на Linux идёт по D-Bus, который нативному хосту нужен и так (logind, порталы). Мобильные адаптеры (LY65, LY70) и ARIA DOM (LY76) фичи не имеют с самого начала. Убрать `a11y` из `feature-matrix`, `facade-combos`, README и документов | LY32, LY34 | `crates/flui-native/Cargo.toml`, `crates/flui-app/Cargo.toml`, корневой `Cargo.toml`, `crates/flui-native/src/accessibility/*`, `docs/*` | `feature-matrix`, `facade-combos`, `deps`, `workspace`, `cross-typecheck` | `rg` по `feature = "a11y"` пуст; тесты моста доступности идут без `--features`; адаптеры собираются на каждой ОС | L + T |
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
| **LY100** | **Фича `scripted` и один двойник окна** (D29, R11.14). Сценарный примитив контракта — под `scripted`; `flui-testing` и `flui-app[driver]` включают её; 15 реализаций `PlatformWindow` вне ядра сводятся к `flui_testing::MockWindow` | LY44, LY50, LY54 | `crates/flui-platform/src/capability/`, `crates/flui-testing/src/`, `crates/flui-app/Cargo.toml`, тесты `flui-interaction`, `flui-runtime`, `flui-app` | `testing` (ADR-0243), `workspace` | `rg "impl .*PlatformWindow for"` вне ядра и kit — ноль; сценарий диалога через драйвер на headless | L |
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
| Перетаскивание наружу и приём от других приложений | 8 | словарь `drag` и `data_transfer` остаётся в контракте (design §5); таблицы предложений уходят в ядро (LY49) | нативные источники и цели (в т. ч. `NSDraggingDestination`), прогоны с другими приложениями |
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
| R4.10 | LY36 |
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
