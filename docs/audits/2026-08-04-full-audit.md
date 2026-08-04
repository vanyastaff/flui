# Полный аудит FLUI: архитектура, качество кода, техдолг — 2026-08-04

**Модель:** Qwen (Qwen Code, Alibaba Group).

**Метод:** 3 параллельных исследовательских агента (архитектура/layering, качество кода, консистентность/техдолг) + ручная верификация ключевых находок по `file:line` (devtools-заглушка, unsafe-счётчики Windows-бэкенда, wasm-`unwrap`, мёртвые фичи, CI-исключения, счётчики render-объектов). Дерево компилируется чисто (`cargo check -p flui-app -p flui-view -p flui-foundation`); формальных нарушений layering-DAG нет — находки глубже уровня графа зависимостей.

**Объект:** ветка `realm-coexistence` (HEAD `5554abb1`), 28 крейтов + фасад, ~620k строк Rust включая тесты, wgpu 29, Rust 1.97, edition 2024.

**Формат уверенности:** все пункты ниже — **доказанные** (есть `file:line` evidence), кроме явно помеченных иначе.

---

## 1. Executive summary

Проект дисциплинирован на уровне контрактов: layer DAG не нарушен нигде и обеспечен механически (`scripts/check-workspace-inventory.sh`, 827 строк), `unwrap`-burn-down завершён (2 сайта на 620k строк), `todo!/unimplemented!` — только в разрешённых platform-init stubs, каталог render-объектов защищён настоящим guard-тестом, ~9200 тестов.

Системные проблемы сосредоточены в четырёх местах:

1. **`flui-platform` — крейт с 70% всего unsafe кода workspace не имеет ни одного выполняемого теста в CI**, а его Windows-бэкенд почти не документирован по SAFETY и глотает ошибки Win32.
2. **`flui-app/src/app/runner.rs` — god-модуль на 4471 строку**, хардкодящий конкретный `wgpu::Renderer` вопреки существующей абстракции `RasterBackend`; это структурный блокер задокументированного raster-thread переноса (ADR-0027, #559).
3. **Гейты и документация расходятся с реальностью**: port-check whitelist'ы отключают триггеры ровно на известных нарушителях; `.flutter/`-оракул gitignored, поэтому главный DoD-критерий невоспроизводим вне машины мейнтейнера; `docs/testing.md`, `docs/crates.md`, комментарии `ci.yml` и CHANGELOG дрейфуют.
4. **Ложные утверждения в shipped-поверхности**: `flui devtools` печатает "server started" не запуская сервер; `flui-devtools/FEATURES.md` описывает несуществующий инспектор; README приписывает unsafe-границу не тем крейтам.

Вердикт: фундамент здоров; главные риски — честность verification-контуров (CI, SAFETY, DoD-оракул) и концентрация сложности в `flui-app`/`flui-platform`.

---

## 2. Критичные проблемы

### 2.1. `flui-platform`: самый unsafe-крейт без выполняемых тестов в CI

- `.github/workflows/ci.yml:259` — `cargo nextest run --workspace --exclude flui-platform`; `:513` — то же для doc-тестов. Пропускается **256 тестов** (170 unit + 86 integration в 12 файлах `tests/`) на 25k строк src и все doctests.
- Причина исключения — краш, который по `docs/ROADMAP-TRACKER.md:163` (item H9) **только Windows**; матрица CI — ubuntu, но исключение снимает и Linux-выполнимый набор: 23 теста headless-бэкенда (`src/platforms/headless/platform.rs`), весь `tests/` (headless.rs, contract.rs, executor_tests.rs…), unit-тесты task/traits/config. Ни один другой джоб их не подбирает: windows-latest `gpu-test` гоняет только readback-тесты `flui-engine`; `cross-typecheck` — clippy без линковки и тестов.
- **Windows-бэкенд: 104 unsafe-сайта против 6 SAFETY-комментариев**; в `src/platforms/windows/window.rs` — 63 unsafe и **1** SAFETY. По бэкендам: macos 105/94, windows 104/6, android 23/15, web 12/6, winit 2/2.
- Крейт-уровневый `#![allow(unsafe_code)]` (`src/lib.rs:153`) глушит workspace-линт `unsafe_code = "warn"`; соседний комментарий утверждает "каждый блок несёт SAFETY-комментарий" — ложно, причём корневой `Cargo.toml` сам фиксирует ~91 недокументированный сайт как причину не включать `undocumented_unsafe_blocks`.
- ~19 результатов Win32-вызовов глотаются `.ok()` без трейсинга: `SetWindowTextW` (window.rs:947), `SetWindowPos` (971, 996), `GetWindowPlacement` (1198), `DestroyWindow` (1152, 1493) — операции над окнами, видимые вызывающему.
- macOS: `panic!()` как обработка ошибок, достижимая вызывающим кодом: `window_tiling.rs:141,171,199` ("Invalid position for SideBySide/TopBottom/Quarters layout"), `task.rs:143` ("Task was unexpectedly cancelled"), и ловушка имплементоров — default-метод публичного трейта `as_any()` с `panic!("as_any not implemented")` (`traits/window.rs:376`).
- Бонус: `crates/flui-platform/AGENTS.md:16-21` предупреждает, что даже локально `cargo nextest run -p flui-platform` с default-фичами молча пропускает winit-модуль (нужен `--features winit-backend`).

### 2.2. `runner.rs` — god-модуль, хардкодящий GPU-бэкенд

- `crates/flui-app/src/app/runner.rs` — 4471 строка (~3980 production, `#[cfg(test)]` с :3981); `ui_realm.rs` — 4264. Вместе **62%** src-строк `flui-app`.
- Внутри одного файла: четыре platform run-loop (`run_desktop` :2729 — 516 строк, `run_android` :3298 — 344, `run_ios` :3648, `run_web` :3661 — 319), lifecycle-лестница (:524-710), realm install/dispatch/teardown (:1141-1347), frame-gating (:2342-2461), process-TLS `APP_RUNTIME` (:128).
- App-слой именует конкретный бэкенд: `use flui_engine::{Recoverability, wgpu::Renderer}` (`app/direct.rs:48`, `runner.rs:2735,3304,3667`, `hot_reload.rs:208,336`) — хотя `flui-engine` определяет backend-agnostic `RasterBackend`/`RasterOwner` (`flui-engine/src/lib.rs`, `raster.rs`, `raster_owner.rs`), которые раннеры не используют (`RasterOwner` встречается в flui-app один раз — в doc-комментарии `presentation.rs:6`).
- Следствие: deliverable ADR-0027 по raster-потоку (issue #559, adoption `RasterOwner` production-раннерами) открыт; ADR честно помечает это как "mis-bundled", но структурная причина — именно прямая зависимость от `wgpu::Renderer`.
- Декларируемая роль flui-app — "приватный composition root" (`docs/workspace-layers.toml`, disposition `narrow`); 4k-строчный монокль этому прямо противоречит.

### 2.3. `flui devtools` сообщает ложь

- `crates/flui-cli/src/commands/devtools.rs:33-58`: с фичей `devtools` команда проверяет порт, печатает **"DevTools server started on http://localhost:{port}"** и "Press Ctrl+C to stop", пишет в трейс "DevTools server listening" — и блокируется на `ctrl_c()`, ничего не запуская. После проверки порта никто его не занимает.
- Фича `devtools` тянет `dep:flui-devtools`, но CLI не вызывает из крейда ни одного символа (единственная ссылка — TODO :44).
- Находка уже фиксировалась в `docs/audits/2026-07-23-architecture-audit.md:370` и не исправлена.

### 2.4. `.flutter/`-оракул gitignored — DoD-гейт №1 невоспроизводим

- `.gitignore:151` исключает `.flutter/`; каталога нет ни в чекауте, ни в CI. Первый критерий Definition of Done ("Verify against `.flutter/`") и вся parity-методология выполнимы только на машине мейнтейнера; parity-утверждения в ROADMAP/tracker нефальсифицируемы для всех остальных. README честно говорит "Maintainer checkouts may include local `.flutter/` and `.gpui/` reference mirrors" — но DoD сформулирован как обязательный критерий.

### 2.5. Конвенция `expect("BUG: …")` соблюдена на ~35%

- `docs/PANIC-POLICY.md` предписывает префикс `BUG:` на всех production-`expect()`. Замер по production-src (без тестов и doc-примеров): **243 `expect()`, 85 с `BUG:`, 158 без (65%)**.
- Худшие: flui-rendering 3/31, flui-engine 0/22, flui-widgets 38/16, flui-platform 8/14, flui-painting 0/12, flui-hot-reload 0/12, flui-foundation 0/11. Рекордсмены-файлы: `flui-rendering/src/testing/harness.rs` (16), `flui-hot-reload/src/worker.rs` (9), `flui-engine/src/wgpu/backend.rs` (7), `flui-foundation/src/id.rs` (7), `flui-painting/src/text_painter/paint.rs` (7), `flui-widgets/src/scroll/refresh_indicator.rs` (7).
- `expect_used` намеренно не линтится — конвенцию не обеспечивает ничто; новые крейты (flui-app, flui-log, flui-cli, flui-build) соблюдают на 100%.

### 2.6. CHANGELOG отстаёт на 112 коммитов; тегов нет

- `CHANGELOG.md` последний раз тронут 2026-07-25 (`c8069362`); `git log c8069362..HEAD` = **112 коммитов** при темпе ~22/день. Пропущены: вся серия singleton retirement (#586–#592: растворение `AppBinding`, удаление `BindingBase`/`HasInstance`/`impl_binding_singleton!`, де-синглтонизация `Scheduler`, удаление guard'а `UiRealm`), feature-selection каталогов (#574), реархитектура логирования (#571), 4 miri-фикса UB в Android-аллокаторе (#584).
- **Ноль git-тегов**: версия 0.2.0 существует только в манифесте; `justfile version := git describe` всегда падает в commit hash.

---

## 3. Архитектурные недостатки

### 3.1. Утечка типов через границы крейтов

- `pub use cosmic_text::FontSystem` (`crates/flui-painting/src/lib.rs:207`) + `SharedFontSystem(Arc<Mutex<FontSystem>>)` (`text_layout/layout.rs:91`) — сторонний тип в публичном API, кросс-крейтово потребляемый `flui-engine/src/wgpu/text.rs`; версия cosmic-text становится общим контрактом.
- `flui-engine/src/wgpu/renderer.rs:990-1005` — публичные аксессоры `device() -> &wgpu::Device`, `queue()`, `surface()`, `surface_config()`; там же `:274` — **`unsafe impl Send for Renderer {}`** поверх полей с `RawWindowHandle`/`RawDisplayHandle` (`NonNull<c_void>`) — soundness-утверждение на ручной поддержке.
- `flui-rendering/src/lib.rs` — wildcard-реэкспорт `pub mod layer { pub use flui_layer::*; }` и `pub use flui_semantics as semantics`: вся публичная поверхность соседнего крейда семвер-привязана к реэкспортёру.

### 3.2. Двойная ответственность за кэши изображений

- `flui-painting/src/binding.rs:49` — `pub struct ImageCache` (decoded-изображения, лимиты, eviction); `flui-assets/src/cache/mod.rs:38` — `pub struct AssetCache<T: Asset>`; `docs/crates.md` описывает flui-assets как "Asset loading, **caching, image decoding**". Граница (GPU-текстурные кэши engine vs decoded-bytes в painting vs жизненный цикл в assets) не задокументирована ни в crates.md, ни в workspace-layers.toml.
- Символический остаток: сам `ImageCache` живёт в файле `binding.rs`, хотя `bindings/mod.rs` declares "FLUI does not compose a matching [binding] struct".

### 3.3. Лексика "bindings" пережила архитектуру

- `crates/flui-app/src/bindings/mod.rs` (42 строки) — чистая доска реэкспортов (`GestureBinding`, `PaintingBinding`, `PipelineOwner`, `RenderingFlutterBinding`, `Scheduler`, `WidgetsBinding`), повторно реэкспортируемая из корня и prelude крейда (`flui-app/src/lib.rs:54-57,89-92`), при собственном доке о том, что соответствующих структур больше нет.
- ~100 упоминаний удалённого `AppBinding` по workspace; исторические ("retired `AppBinding`") допустимы, но часть production-доков ссылается на него как на существующий: `flui-widgets/src/app/safe_area.rs:37`, `flui-widgets/src/interaction/visibility.rs:40,103`, `flui-widgets/src/scroll/list_view.rs:40`, `flui-platform/src/platforms/winit/platform.rs:38`, `flui-platform/src/platforms/headless/platform.rs:628`, `flui-platform/src/traits/haptics.rs:44-49`, `flui-scheduler/src/scheduler.rs:1207`, `flui-rendering/src/binding/mod.rs:86`, `flui-animation/src/vsync.rs:213`.
- В `runtime-contract.toml` есть 7 `forbidden_pattern`-ратчетов (impl_binding_singleton, HasInstance, REALM_CLAIMED, WidgetsFlutterBinding, три тестовых лока), но `AppBinding` среди них нет — CI эти ссылки не поймает.

### 3.4. Устаревшие ADR и контракты

- **ADR-0016** (`docs/adr/ADR-0016-unified-font-system-registration.md`): статус "Accepted", механизм — `PaintingBinding::instance().font_system()` (:53, :62, :68, :151), приложение ссылается на удалённый `crates/flui-foundation/src/binding.rs`. Реальность: `FONT_SYSTEM` — свободный process-global `OnceLock<Arc<Mutex<FontSystem>>>` (`flui-painting/src/text_layout/layout.rs:48`), `PaintingBinding` — обычный instance-сервис. Коммит `7bcf192e` (retirement) ADR не аннотировал.
- Остальные проверенные ADR в порядке: ADR-0002 корректно Superseded, ADR-0041/0042 соответствуют коду, ADR-0027 активно поддерживается с явными коррекциями.

### 3.5. port-check обеспечивает контракт… кроме известных нарушителей

- Триггер 5 (`Arc::clone` в per-frame путях, `scripts/port-check.sh:284-310`): scope исключает `flui-engine/src/wgpu/backend.rs` и `renderer.rs`; per-frame сайты на месте (`backend.rs:121-122,408-409`, `renderer.rs:656-657`).
- Триггер 7 (`Arc<Mutex<Renderer/Pool>>`-поля): три файла с ровно этим паттерном в glob-whitelist — `texture_pool.rs` (`pool: Arc<Mutex<TexturePoolInner>>` :71, подтверждено), `renderer.rs:147`, `backend.rs` ("Outstanding refactor #1/#2/#3" в `crates/flui-engine/ARCHITECTURE.md`).
- Итог: для нового кода контракт реален; для худших существующих сайтов — приостановлен. Трекинг честный, но статус "задокументировано" не равен "решено".

### 3.6. Мёртвые фичи

- `crates/flui-app/Cargo.toml`: фичи `desktop` (**default**), `android`, `ios`, `web`, `debug-overlay`, `performance-overlay` — **0** ссылок `feature = "…"` во всём крейте. Подключена только `hot-reload` (24 ссылки). Оверлеи управляются полем конфига, не фичей.
- `crates/flui-platform/Cargo.toml`: `web = []` — 0 ссылок; web-бэкенд гейтится `#[cfg(target_arch = "wasm32")]` (`src/platforms/mod.rs:25`).
- Честные контрпримеры (forwarding с документацией): `flui-tree/serde`, `flui-geometry/mint`, `flui-engine/serialization|lyon-debugger`.

### 3.7. `anyhow` в публичном API библиотеки

- Правило: `thiserror` в библиотеках, `anyhow` в бинарях. Нарушение — `flui-platform`: `pub fn current_platform() -> anyhow::Result<Box<dyn Platform>>` (`src/lib.rs:328`), `PlatformReadyCallback = Box<dyn FnOnce(OwnerPlatform) -> anyhow::Result<()>>` (`traits/platform.rs:160`), `fn run(...) -> anyhow::Result<()>` (:242), `app_path()`/dialog-методы (`traits/owner.rs:234,296,306`).
- Смягчение: `traits/owner.rs:370` документирует план типизированной таксономии ошибок; эталон — `flui-layer/src/error.rs:10` ("anyhow::Error is never returned from this crate's public API").

### 3.8. Layer-легальные, но сомнительные рёбра

- `flui-material` (L7, дизайн-система) → напрямую `flui-rendering` (L4), `flui-scheduler` (L2), `flui-interaction` (L2): дизайн-слой компилирует render-машину; использование узкое (~15 файлов, `PostFrameHandle` только в `scaffold_messenger.rs:212` — задокументированный residual ADR-0027 clause 2).
- `flui-widgets` (L6) → прямой ребро в `flui-geometry` (L0) при том, что `flui-types` её реэкспортирует (`flui-types/src/lib.rs:96`): два пути импорта одних типов.

### 3.9. README-утверждение об unsafe ложно

- README: "unsafe ограничен flui-platform, flui-painting, flui-engine". Замер: flui-platform 224, **flui-rendering 36**, **flui-hot-reload 30**, flui-engine 9, **flui-layer 6, flui-foundation 6, flui-types 4, flui-log 3, flui-app 1**; названный в README **flui-painting — 0**.

---

## 4. Качество и стиль кода

### 4.1. Дыра в гейтах: production-`unwrap()` в wasm не видит ни один clippy

- Производственных `.unwrap()` во всём workspace **2** — оба в wasm-бэкенде: `flui-platform/src/platforms/web/platform.rs:113,119` (`f.borrow().as_ref().unwrap()` в rAF-замыкании).
- Разбор сырого счётчика 665: 592 в `#[cfg(test)]`-областях, 49 в doc-примерах, 24 в `src/**/tests.rs` за `#[cfg(test)] mod x;`, 3 ложных срабатывания в комментариях. Burn-down честно завершён — но выжившие сайты структурно проходят все гейты: clippy-джоб — linux, `cross-typecheck` — только msvc/mac, `wasm-check` — `cargo check` без clippy.

### 4.2. Lint-подавления

- 519 `#[allow]/#![allow]` в production src; ~111 избыточны (дублируют workspace-линты корневого `Cargo.toml`): `cast_possible_truncation` 46, `cast_sign_loss` 22, `float_cmp` 13, `cast_precision_loss` 7, `match_same_arms` 6, `cast_possible_wrap` 4 и др. Рекорд — 7 крейт-уровневых allow в `flui-painting/src/lib.rs:159-165`, все дубли.
- Крейт-уровневые подавления, которые стоит трекать: `#![allow(unsafe_code)]` (`flui-platform/src/lib.rs:153`, `flui-rendering/src/pipeline/owner/subtree_arena.rs:41`, `flui-view/src/key/object_key.rs:5` + 30 item-level в flui-hot-reload), `#![allow(unused)]` на весь модуль (`flui-types/src/painting/mod.rs:7` — модуль при этом потребляется, allow скрывает реальную картину), `#![allow(dead_code)]` на модули в shipped-коде (`flui-widgets/src/overlay/mod.rs:70`, `navigator/local_history.rs:38`, `navigator/hero_controller.rs:106`, `flui-platform/src/platforms/windows/{util.rs:2,events.rs:9}`).
- Переходных `#![allow(clippy::unwrap_used)]` в production **нет** (15 оставшихся — tests/benches/examples, где они ещё и избыточны из-за `allow-unwrap-in-tests`).

### 4.3. Размеры

- 17 production-файлов >1500 строк (крупнейшие: runner.rs 4471, ui_realm.rs 4264, wgpu/renderer.rs 4032, interaction/binding.rs 3666, scheduler.rs 3169, arena/mod.rs 2649, element_tree.rs 2643, winit/platform.rs 2480, wgpu/backend.rs 2474, subtree_arena.rs 2462, view/binding.rs 2447, editable_text.rs 2315, animation/controller.rs 2292, focus.rs 2236, geometry/units.rs 2056). `too_many_lines` workspace-разрешён — не отслеживается ничем.
- 7 функций >150 строк: `run_desktop` 516, `run_android` 344, `run_web` 319, `handle_pointer_event_kernel` 197 (`flui-interaction/src/binding.rs:874`), `build_windowed_gpu_stack` 182, `render_scene_content` 163, `render_scene` 151. `ui_realm.rs`/`scheduler.rs` при 4k/3k строках функций >120 строк не имеют — фактура внутри хорошая.
- Отдельно: тестовые файлы внутри `src/` (engine `aa_oracle_tests.rs` 3794, `painter/tests.rs` 2868) раздувают src-метрики крейда.

### 4.4. Unsafe вне flui-platform

- `flui-hot-reload` (dlopen/ABI-граница — второй по риску unsafe после platform FFI): 37 unsafe против 15 SAFETY; корневой `Cargo.toml` сам зафиксировал, что часть комментариев там "описывает инварианты, которые код не устанавливает".
- `flui-rendering/src/pipeline/owner/subtree_arena.rs` — образцовая зона (45/44 по крейту, miri-покрытие), но с файловым `#![allow(unsafe_code)]`.

### 4.5. Недоимплементированная функциональность в shipped-путях (31 TODO, 0 FIXME/HACK)

- `Cargo.toml:230` — webp выключен (image-rs/image-webp#102), при этом `flui-assets/Cargo.toml:57` рекламирует фичу `images` как "PNG, JPEG, GIF, **WebP**, etc." (та же строка в `flui-assets/README.md`).
- `flui-types/src/styling/color.rs:1589,1600` — `to_hsl/from_hsl/to_hsv/from_hsv` не реализованы; 2 `#[ignore = "TODO: Implement…"]`-теста с закомментированными телами.
- `flui-types/src/painting/path.rs:625,696` — arc containment/winding не реализованы (геометрия, релевантная hit-testing).
- `flui-platform/src/platforms/windows/platform.rs:1007` — "TODO: Return actual capabilities" (заглушка возможностей, отдаваемая вызывающим); `windows/window.rs:1376` — taskbar progress (ITaskbarList3) не реализован.
- `flui-platform/src/platforms/macos/mod.rs:20-22` — клавиатура/мышь, NSPasteboard, Core Text помечены как неготовые в shipped macOS-бэкенде.
- `flui-assets/src/lib.rs:192,197` — модули `bundle` и `hot_reload` закомментированы.
- `flui-cli/src/templates/mod.rs:137` — шаблоны Todo/Dashboard/Widget/Plugin/Empty не реализованы (`create_interactive.rs:66` при этом рекламирует "Todo list app (coming soon)").
- `flui-objects`: `sliver/sliver_padding.rs:413` — cross-axis start игнорирует TextDirection; `interaction/meta_data.rs:212`, `interaction/absorb_pointer.rs:109` — не проброшен gesture target id.
- **Нарушение собственного правила** о process-ID маркерах (AGENTS.md, swept 2026-07-12): "TODO T12" в `flui-engine/src/wgpu/aa_oracle_tests.rs:2855,2858,2894,2935,2961` и `offscreen/mod.rs:450` (в cfg(test)-файле, но паттерн реинтродуцирован).

### 4.6. Production-паники вне stubs

- Все 34 `unimplemented!()` — разрешённые trigger-#8 platform-init stubs (ios/linux). Вне их: три macOS `window_tiling.rs`-паники, `task.rs:143`, `traits/window.rs:376` (§2.1) + инвариантные паники без `BUG:`-префикса: `flui-engine/src/wgpu/pipelines.rs:501`, `flui-objects/src/layout/custom_multi_child_layout.rs:101,209,217`, `flui-view/src/tree/element_tree.rs:477,1550,1731`, `flui-rendering/src/context/intrinsics.rs:497-541` (re-entrancy guards), `flui-foundation/src/notifier.rs:263`, `notifier_generic.rs:88` (Flutter-parity, задокументировано).

### 4.7. Обработка ошибок — мелочи

- `.ok();` в production: 34 сайта; actionable-кластер — Windows window backend (~19, §2.1), остальное безобидно (doctor-форматирование, cleanup временных каталогов, oneshot-send).
- `let _ =` — 605 строк, но `unused_must_use = "deny"` означает явность каждого; выборка нашла только отбрасывания `send()` у oneshot-каналов (семантика "receiver dropped" приемлема).
- Известный красный дефект в release-режиме: `flui-interaction eager_dispose_clears_state` — CHANGELOG признаёт "a real defect in the recognizer's dispose guard… Verified red on main"; не исправлен.

### 4.8. Структурные несоответствия

- Размещение тестов: большинство крейтов — `tests/`, но flui-engine/flui-widgets/flui-rendering держат крупные тестовые файлы в `src/` за `#[cfg(test)] mod x;` (например, `flui-widgets/src/navigator/navigator_tests.rs`, 2044 строки) — это меняет применимость `allow-unwrap-in-tests` и усложняет аудит.
- Монолитные `binding.rs` (flui-view 2447, flui-interaction 3666) против декомпозированного `bindings/` в flui-app.
- Snake_case-нарушений в именах файлов нет; tier-консистентность организации (плоские foundation-крейты, `dir/mod.rs` у крупных) соблюдается.

---

## 5. Консистентность и дрейф документации

### 5.1. `docs/testing.md` — серьёзный дрейф

- Формула `just ci` неверна (пропущены `runtime-conformance-check`, `test-ci`, `doc-strict`; показан `cargo test --workspace`, хотя CI исключает flui-platform).
- Каталог harness: указан `crates/flui-rendering/tests/render_object_harness.rs` и "37 типов"; реально `crates/flui-objects/tests/render_object_harness.rs` и **81** тип (AGENTS.md знает правильно).
- Miri: "5 unit-тестов, не входит в layout walk" — прямо противоречит ci.yml и AGENTS.md (два real-`NodePtr` walk'а, один из которых обязан ронять miri при удалении in-flight gate).
- "Visual regression tests (planned)" — существуют: `tests/golden_screenshots.rs` (209 строк) + 6 золотых PNG, `just golden`.
- Битая ссылка `.specify/memory/constitution.md`.

### 5.2. `docs/crates.md`

- Всё ещё перечисляет **удалённый** `BindingBase` среди примитивов flui-foundation (удалён в `5554abb1` 2026-08-04; `rg BindingBase crates/flui-foundation/src` — пусто). Inventory-check валидирует состав и рёбра, но не прозу — дрейф не ловится ничем.
- "20+ crates" против реальных 28 + фасад.
- Битые ссылки `.ai-factory/ARCHITECTURE.md` и `.specify/memory/constitution.md` — из 10 файлов (FOUNDATIONS.md:323, architecture.md, PORT.md, testing.md, contributing.md…).

### 5.3. Три числа render-объектов

- `flui-objects/src/lib.rs:6,22` — "**76** real objects"; `docs/ROADMAP.md:17` — "**79 of ~80**"; реально **81** — и guard-тест `render_object_types_match_exports` (`tests/render_object_harness.rs:11331`) механически подтверждает catalog == `pub use`-экспорты.

### 5.4. Комментарии в `ci.yml`

- Ссылки на **удалённые** `SINGLETON_WINDOW_TEST_LOCK`/`SCHEDULER_PHASE_TEST_LOCK` (их отсутствие теперь обеспечивают `forbidden_pattern`-ратчеты `docs/runtime-contract.toml:2047-2059`).
- "macOS deferred… (see CLAUDE.md: many high-level crates disabled)" — CLAUDE.md есть 4-строчный шим на AGENTS.md, такого утверждения нет; все 28 крейтов `(ACTIVE)`.
- Счётчики интеграционных файлов устарели (37/51/26 против реальных 39/63/27); wasm-check "27 of 34 packages" — пакетов уже 36.

### 5.5. `flui-devtools/FEATURES.md` описывает фикцию

- Заявлен `src/inspector.rs (437 lines)` с API `Inspector::new()`, `attach_to_tree(tree)`, `select_widget(id)`, `get_widget_tree()`, `highlight_widget(id)` и "4 tests"; "Default features: profiling, inspector"; "Total 2,064 LOC / 24 tests".
- Реально: `inspector.rs` — **181 строка**, только `InspectorCounters`/`InspectorSnapshot` (счётчики rebuild'ов через ADR-0040 observation seam, доступа к дереву нет); `Cargo.toml default = []`; крейт 1846 строк. `docs/crates.md` честен ("no tree inspector yet — see the crate docs"), но "crate docs" — это и есть лгущий документ. Там же ссылка на `flui_core` (:213) — крейт, никогда не существовавший (он же: корневой `Cargo.toml:257`, `flui-scheduler/README.md:242`, `flui-assets/README.md:287`).

### 5.6. Мелкий дрейф

- Корневой `Cargo.toml`: комментарий "flui-macros (skeleton — derives land as a follow-up)" устарел — 822 строки, четыре реальных derive (`StatelessView`, `StatefulView`, `Animatable`, `Diagnosticable`) с тестами и `#![deny(missing_docs)]`.
- AGENTS.md: "flui-rendering/src/lib.rs is by far the densest" — устарело: lib.rs 220 строк; плотность переехала в `subtree_arena.rs` (2462), `sliver_protocol.rs` (1922), `storage/tree.rs` (1822), `box_protocol.rs` (1766).
- `examples/README.md` описывает только windows11_demo/hello_world/window_features, ссылается на несуществующие `macos_*`/`linux_*` примеры и не знает про demo/gallery/screenshot/web/hot-reload; `examples/android_scene/Cargo.toml` не тронут с 2026-02-13 (~6 месяцев); `crates/flui-rendering/examples/box_layout.rs.disabled` — мёртвый груз.
- ROADMAP внутренне противоречив: construction-shape diagram называет Catalog.1 "not started" при `(ACTIVE — Catalog.1 slice)` в Cargo.toml и 685 тестах flui-material; "Status at a glance (verified 2026-07-15)" устарел. Честные маркеры тоже есть: "no CI coverage gate" при планках 80/70/85%, widgets "fidelity partial".

---

## 6. Тесты и платформы

- Плотность: здоровые тяжеловесы — flui-widgets 1790 тестов, flui-objects 870, flui-rendering 840, flui-types 650, flui-view 587, flui-engine 510. Тонкие места: **flui-hot-reload 14 тестов / 2619 строк / 30 unsafe** (dlopen!), flui-devtools 24/1846, flui-build 46/4060, flui-macros 7/707.
- Harness-каталог: guard `catalog_covers_every_render_object_name` — substring-проверка (упоминание в комментарии засчитывается; ignored-тесты тоже), покрытие неравномерное: `RenderColoredBox` 228 против **10 типов ровно с 1** тестом (ClipOval/ClipPath/ClipRect, ExcludeSemantics, MergeSemantics, SliverAnimatedOpacity, SliverFillViewport, оба PinnedPersistentHeader, SliverListLazy). Equality-тест exports==catalog при этом настоящий.
- `#[ignore]` ~72: большинство — честные parity-пины против Flutter-оракула; подозрительные: color hsl/hsv (§4.5), `flui-widgets/tests/image_async.rs:484` ("reproducible failure (cause not isolated)"), 4 placeholder-заглушки в `flui-platform/tests/integration_template.rs`.
- **WASM**: compile+link реальны (29 пакетов, rust-lld линкует оба cdylib, import-allowlist чист), но **ничего не выполняет** wasm-вывод — нет ни wasm-bindgen-test, ни headless-smoke. "Работает в вебе" — только компиляция.
- **Android**: заявлен в README, но ни один CI-джоб не компилирует android-triple (cross-typecheck — только msvc + aarch64-apple-darwin); android-примеры вне workspace; 17 тестов `android/memory.rs` (после 4 miri-фиксов UB, #584) в CI не выполняются. iOS-бэкенд — открыто `unimplemented!()`-заглушки (trigger-#8 exempt).
- Локи: `just test` (включая flui-platform) расходится с `just test-ci` — footgun на Windows.
- Lockfile-дубли (deny.toml `multiple-versions = "warn"` живёт, но не чищен): **windows-sys ×5** (0.48/0.52/0.59/0.60.2/0.61.2), **hashbrown ×4** (0.14.5/0.15.5/0.16.1/0.17.1), getrandom ×3, windows-targets ×3, thiserror 1+2, syn 2+3, rustc-hash 1+2, lru 0.16+0.18, downcast-rs 1+2, **indicatif 0.17+0.18 (workspace пинит 0.17 — намеренно старый)**, which 7+8, rustix ×2, objc2 ×2, toml ×2. `wgpu-hal` не дублируется.

---

## 7. Что проверено и хорошо

1. **Layering: ноль нарушений.** Все normal in-workspace рёбра строго вниз; same-layer — только 4 санкционированные пары (types→geometry, interaction→platform, objects→rendering, cli→devtools); material/cupertino не зависят от flui-localizations; зависимые от flui-log — ровно flui-app, flui-cli, фасад, android-demo. `flui-testing` — только в `[dev-dependencies]`. Enforcement реален: ранжирование слоёв, forbidden pairs, `allowed_dependents`, DFS-детекция циклов (`scripts/check-workspace-inventory.sh:550`).
2. **Panic-policy burn-down по `unwrap` завершён** (2 сайта, §4.1); крейт-уровневых `allow(clippy::unwrap_used)` в production нет.
3. **`todo!/unimplemented!` дисциплина** образцовая; все 38 вхождений — разрешённые stubs или doc-комментарии.
4. **Singleton retirement доведён до конца**: нет `AppBinding`/`BindingBase`/`HasInstance`/`impl_binding_singleton!` в коде (только doc-комментарии и ратчеты), `UiRealm`-конструкция свободна от guard'а, задокументированные residuals (`APP_RUNTIME` TLS, `LOCAL_LANES`/`ACTIVE_LANES`, #559) присутствуют и помечены.
5. **Catalog-guard** (exports == `RENDER_OBJECT_TYPES`) настоящий и ловит дрейф.
6. **~9200 `#[test]`-функций**; gpu-test merge-blocking (~440 readback-тестов на WARP); честные `#[ignore]`-пины расхождений с оракулом.
7. **Гигиена фич** в flui-engine/flui-widgets/flui-assets/flui-devtools реальна; facade-combos CI компилирует все 13 комбинаций фич фасада изолированно.

---

## 8. Приоритетный план

| # | Действие | Закрывает |
|---|----------|-----------|
| 1 | Вернуть тесты `flui-platform` в CI (минимум Linux-выполнимый набор: headless/task/traits + `tests/`); Windows-краш изолировать, а не исключать крейт | §2.1 |
| 2 | Декомпозиция `runner.rs` (run-loops по платформам, frame-gating отдельно) + adoption `RasterBackend` раннерами — закрыть #559 | §2.2 |
| 3 | `flui devtools`: запускать реальный сервер или честно печатать "not implemented" | §2.3 |
| 4 | SAFETY-burn-down Windows-бэкенда (98 сайтов), замена `.ok()` на трейсинг, снятие крейт-уровневого `allow(unsafe_code)`; macOS-паники → Result | §2.1, §4.4 |
| 5 | Скрипт/линт на `expect("BUG: …")` + burn-down 158 сайтов | §2.5 |
| 6 | Решить вопрос `.flutter/`-оракула: CI-доступ (хотя бы subset) или честная редакция DoD | §2.4 |
| 7 | Doc-sweep: testing.md, crates.md, комментарии ci.yml, CHANGELOG (+ теги), счётчики 76/79/81, ссылки `AppBinding`/`flui_core`, FEATURES.md, README (unsafe-граница, примеры) | §2.6, §5 |
| 8 | Удалить или подключить мёртвые фичи flui-app/flui-platform | §3.6 |
| 9 | Сжечь port-check whitelist'ы flui-engine (`Arc<Mutex<…>>`, per-frame `Arc::clone`) | §3.5 |
| 10 | Burn-down избыточных allow (~111) и 5 целых модулей под `dead_code`/`unused`; аннотировать ADR-0016 как Superseded/Amended | §4.2, §3.4 |
| 11 | Разобрать lockfile-дубли (windows-sys, hashbrown, syn, thiserror); wasm-runtime-смoke (wasm-bindgen-test); Android-компиляция в CI | §6 |

---

*Аудит выполнен 2026-08-04 моделью Qwen (Qwen Code, Alibaba Group): 3 параллельных исследовательских агента + ручная верификация ключевых находок. Все пункты с `file:line` проверены по дереву на момент HEAD `5554abb1` (ветка `realm-coexistence`).*
