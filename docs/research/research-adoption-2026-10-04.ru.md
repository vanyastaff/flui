# Применение исследования Rust/UI к FLUI

Дата: **4 октября 2026 года**. Исходная подборка:
[материалы за 4 июля — 4 октября](useful-ui-resources-2026-07-04--2026-10-04.ru.md).
База первоначального внедрения (#1418): `origin/main` на `f7d49b596`, после
обновления worktree. Его изменения вошли в `main` на `1da74e162`.
[Предыдущий аудит](2026-10-03-rust-ecosystem-modernization.md) уже находится в
этой базе. Его выводы сверялись с текущим кодом; повторная смена уже обновлённых
API или dependencies не считается новым внедрением.

Текущее продолжение с валидируемым `GlyphImage` основано на слитом `origin/main`
`2992d8c44`: эта база включает dependency API audit (#1419) и уже слитое
исправление panic-payload retirement/Miri coverage (#1420). Bitmap change
перенесён на эту базу отдельно от prerequisite repair. Первоначальные результаты
#1418 ниже сохранены как исторические; они не доказывают прохождение проверок
нового bitmap API.

[Dependency API audit](2026-10-04-dependency-api-audit.md) уже внедрил HTTP
connection pooling внутри asset registry, fallible client construction,
registry-aware network-image identity по
[ADR-0118](../adr/ADR-0118-network-image-registry-identity.md) и обработку
provider response chunks. Это самостоятельное ранее слитое внедрение;
изменения `GlyphImage` не повторяют его и не приписывают себе его результаты.

## Что изменено и зачем

### AccessKit: реальные исправления Linux adapter

Обновлён совместимый набор: `accesskit 0.25.1`, `accesskit_consumer 0.39.1`,
`accesskit_unix 0.24.0`, `accesskit_atspi_common 0.21.0`,
`accesskit_windows 0.35.1`, `accesskit_macos 0.27.1`.
Источник: [Unix release](https://github.com/AccessKit/accesskit/releases/tag/accesskit_unix-v0.24.0)
и [AT-SPI common release](https://github.com/AccessKit/accesskit/releases/tag/accesskit_atspi_common-v0.21.0),
25 сентября. Именно adapter исправляет enabled/sensitive состояния и добавляет
Image interface на Linux; изменение только FLUI role mapping этого не доставило бы.

Manifest задаёт новые минимальные patch versions для общего model/consumer
и desktop adapters. `cargo update -p accesskit_unix --precise 0.24.0`
обновил lockfile: `git diff --text Cargo.lock` показал только шесть version/checksum
пар этого набора. Optional `a11y` и synchronous frame contract сохраняются.

Agent semantics suite намеренно проверяет версию Windows adapter, с которой
сверена wire projection. Для 0.35.1 `node.rs` и `adapter.rs` сравнены с 0.35.0
через `git diff --no-index`: оба сравнения без hunks, exit 0. Обновлена отметка
проверенной версии; role/action policy не менялась.

### Glyph atlas: отсутствующий bitmap должен разрешать повторную попытку

Этот раздел описывает первоначальный repair и его failure cases в #1418.
После перехода на валидируемый `GlyphImage` malformed-buffer/overflow cases
проверяются на construction boundary; текущая engine family использует
валидные изображения с изменёнными dimensions/content, missing replay и
отказ по device limit.

[Glifo fallible rendering](https://github.com/linebender/vello/releases/tag/glifo-v0.4.0)
послужил поводом проверить наш собственный recovery path.
В `Page::grow` новый texture создаётся и live glyphs повторно растеризуются.
Если rasterizer возвращал `None`, запись оставалась в cache, хотя bitmap
в новый texture не загружен. Следующий lookup возвращал эту запись и не пытался
восстановить glyph. Malformed bitmap уже удалялся корректно.

Теперь `None` использует тот же removal/retirement path. Здоровые glyphs остаются
cached; allocation, на который ссылается уже записанный draw текущего frame,
не переиспользуется до `end_frame`. Новая failure family проверяет missing и
malformed bitmap по отдельности и на двух независимых keys, повторный вызов,
здорового соседа и следующий frame. Это проверка recovery/ownership, а не
доказательство сохранения pixels в frame, где replay впервые не удался.
Private test seam нужен потому, что public painter владеет фиксированным
SwashRasterizer; публичный API ради внедрения mock не расширялся.
Соседняя проверка размера bitmap теперь использует checked multiplication:
`u32::MAX × u32::MAX × 4` раньше overflow в debug вместо отказа invalid bitmap.
Четвёртый failure case требует отказа и успешной следующей загрузки.

### GlyphImage: invariant задаётся при создании значения

Продолжение на слитом `origin/main` `2992d8c44`, после первоначального внедрения на `1da74e162`,
переносит проверку byte layout на границу construction.
[ADR-0120](../adr/ADR-0120-validated-glyph-image.md) частично supersedes
ADR-0067/ADR-0092: `GlyphImage` имеет шесть private fields, fallible `try_new`
и read-only accessors. Constructor проверяет представимость ожидаемой длины
и точное соответствие data; ошибки — `SizeOverflow` и
`InvalidDataLength { expected, actual }`. Zero-area bitmap легален с empty data.
Ни struct literal, ни последующая безопасная mutation больше не позволяют
создать inconsistent размеры/content/bytes.

SwashRasterizer валидирует converted output; `GlyphRasterizer::rasterize`
сохраняет `Option`, construction failure отображается в `None`. Engine убирает
дублирующий data-length guard, сохраняя device/packer limits, retry после `None`,
проверку изменившихся dimensions/content и retirement записанных frame regions.
Проверки malformed output перенесены в public constructor cases; engine recovery
использует individually valid bitmap с отличающейся replay формой.

До eviction и page growth engine отвергает bitmap, превышающий device texture
dimensions или представимый диапазон размеров packer. Неудачная admission
не вытесняет здоровые entries и не запускает бесполезный replay при growth;
следующий вызов того же key может повторить rasterization.

Внешние consumers заменяют literal на `try_new(...)?` либо `.ok()` в Option
producer, field reads — на getters, mutation — на construction replacement.
Подробная migration и сохранённые shaping/raster contracts находятся в ADR.
Это breaking API change, устраняющая invalid state в safe Rust; speedup или
новый rendering benchmark не заявляются. Проверки этого продолжения фиксируются
отдельно от результатов предыдущего внедрения ниже.

### Accessibility queries: дорогая диагностика только на failure path

Практика из [rustdoc performance](https://noahlev.org/blog/2026/08/27/making-rustdoc-faster)
и [Rust API design](https://corrode.dev/blog/practical-rust-api-design/) применена
к `A11yTree::find`/`find_by_label`. Успешный unique query больше не собирает
промежуточный Vec совпадений; label query материализуется только при ошибке.
NotFound по-прежнему показывает дерево, Ambiguous — **все** subjects в preorder.
Новый consumer case проверяет 0/1/3 совпадения и отличие payload order от tree order.
Это сохранение контракта с устранением конкретных allocations в коде;
процент ускорения не измерен и не заявляется.

README и architecture уточняют raw root-reachable snapshot: hidden/transparent
nodes могут присутствовать в harness query, а OS/agent projections применяют
дополнительные фильтры. Guide требует initial state → unique subject → action →
observable effect, с конечным frame/time budget и явным virtual clock.
Action acknowledgement, paint recording и native presentation не взаимозаменяемы.

### nextest: рекомендация исправленного runner

Добавлена `recommended = "0.9.145"`, при сохранённой `required = "0.9.133"`.
[Release 0.9.145](https://github.com/nextest-rs/nextest/releases/tag/cargo-nextest-0.9.145)
исправляет capture-pipe inheritance на Unix, особенно Apple, способную дать
ложный leaked-handles report. Native nextest policy показывает предупреждение
пользователям старого runner; новая script/gate реализация не нужна.
`docs/testing.md` объясняет диагностику и advisory exit code команды version.

## Решения по остальным материалам

Таблица покрывает темы исходной подборки; «уже есть» означает наличие названного
решения в прочитанном коде, а не полную сертификацию subsystem. Численные claims
из чужих workloads не перенесены на FLUI.

| Материал / знание | Решение для текущего FLUI и основание |
|---|---|
| AccessKit Android / model schema | Desktop cohort обновлён. Android adapter в FLUI не подключён; Android live regions остаются acceptance reference, не реализованной поддержкой |
| egui mobile IME | Web events сейчас регистрируют pointer/keyboard/focus/wheel/layout, без hidden editable input/composition bridge. Нужен отдельный Web text-input проект с surrounding-text contract и live Samsung/iOS/Android cases; перенос desktop TextStore или одного enum варианта недостаточен |
| Slint 1.18 | Text selection accessibility, динамический z-order и большие тексты — acceptance cases. Сверять paint/hit/semantics outputs; новый Flexbox API другого framework не обосновывает переписывание нашего catalog |
| GPUI/Xilem experience | Headless limits внесены в testing guide. Generic composition/consumer compile time требует измерения отдельного большого приложения; статья не доказывает необходимость type erasure в FLUI |
| Fenestra | Interaction assertions и native/headless границы уточнены. Fixed bundled fonts дают воспроизводимость, но не проверяют system fallback/native IME |
| wgpu 30.0.1 | Уже закреплён. Capability requests выбираются из adapter support. Общий surface/device contract и native smoke остаются обязательными |
| Parley 0.11.1 / harfrust | Уже закреплены. ShapedParagraph создаётся painting layer, engine получает positioned glyphs; сохраняем разделение shaping/rendering по ADR-0092 |
| netrender | DisplayList → Command IR и RasterBackend seam уже выражают recording/rendering separation. Второй protocol/capture stack без consumer не внедряется |
| Fearless SIMD 1.0 / 0.6 | Существующий glam SIMD сохранён прошлой модернизацией. Новая dependency или AVX-512 dispatch требует измеренного kernel и numerical contract |
| Hyperbezier | Исследовательская геометрия. Без продуктового contour requirement не заменяем kurbo Path или существующие curves |
| nextest 0.9.143 | Runner recommendation включает fix. Dylib/archive notes нужны для диагностики; использование layout v2/filtered archives в нашем tooling не установлено |
| Rust 1.97 symbols / bits | Toolchain 1.99 уже включает v0; release сохраняет symbols. Упаковка IDs не меняется без доказанной ошибки или упрощения |
| Rust 1.98 math / strings / ManuallyDrop | Algebraic operations нельзя массово применять к geometry/goldens. Byte-range APIs не заменяют grapheme/UTF-16 contract; generic Drop safety остаётся review обязанностью |
| Rust 1.99 raw ownership | Platform/hot-reload уже используют ownership transfer через into_raw. Предыдущий аудит внедрил owned UTF-8 conversion. Leak reclaim не вводится |
| Polonius Alpha / next solver | Nightly-only compatibility experiments для arity/GAT/derives/compile-fail cases. Stable API/negative assertions не меняются по одному анонсу; в этой работе nightly probe не выполнялся |
| Cargo warnings | Tooling уже явно применяет CARGO_BUILD_WARNINGS=warn в специальных nightly paths. Существующий strict Clippy contract сохраняется; blanket RUSTFLAGS replacement не требуется |
| Cargo profiles / feature inheritance | Уже resolver 3/edition 2024, explicit dev incremental, dbg full debug, release thin LTO, selective feature ownership. Built-in debug не заменяет custom dbg |
| Cooldown / Cargo lints | Ещё не stable в 1.99. Не добавлены в stable config и не заменяют cargo-deny/cargo-shear |
| Separate rmeta / target size | Nightly experiment. Manual artifact consumers должны получать paths из Cargo JSON; отдельный build-dir probe нужен до изменения reload/tooling |
| Clippy additions / moves / fixes | All/pedantic уже охватывают перечисленные правила; unused_async_trait_impl имеет обоснованное allow. Предыдущий аудит добавил выбранные nursery allocation/numerical checks |
| Restriction style rules | Module DAG и tiers уже enforced. Правила размещения definitions не добавлены без ошибки, которую они предотвращают |
| Practical API design | Existing typed realm IDs и lifecycle capabilities сохраняются. Query change использует локальную проверку uniqueness; новая public capability без production consumer не добавляется |
| Rustdoc / compiler performance | Filter-before-materialization применён к queries. Compile-time benchmark suite и PGO/inline требуют отдельного workload; опубликованные средние проценты не являются нашим результатом |
| Taffy 0.14 | Measure/baseline/scroll-overflow contract полезен при будущем flex/grid adapter. FLUI имеет свои RenderBox intrinsics и baselines; второй layout tree или CSS frontend не внедрён |
| Vello CPU/GPU 0.3 | Использован checklist compositing: target clear, transparent operations, singular transforms и capabilities. FLUI не имеет аналогичного gradient cache; не переносим чужой cache fix в отсутствующий код |
| Vello 0.11 / Glifo 0.4 | Glyph failure review дал конкретный atlas fix. Existing swash placement сохраняет bearings; renderer swap не требуется для исправления recovery |
| AnyRender 0.14 | PaintScene/WindowRenderer/ImageRenderer помогают проверять обязанности. Existing engine IR остаётся единственным путем; новая абстракция требует cross-crate ADR и реального consumer |
| Animato 1.7.2 | AnimatedSwitcher уже сохраняет outgoing entries до dismissal и явно задаёт A→B→A semantics. Новая animation library не нужна ради повторения этого lifecycle; FLIP-list/reduced-motion сценарии требуют отдельного catalog feature |
| Blitz | Modular typography/layout/accessibility полезны как reference. Existing crate layers сохраняются; native HTML/CSS frontend не добавлен |
| Subsecond | DevReloadHook уже отделяет reload от app graph. Versioned function calls сами не доказывают state compatibility; hot reload ABI/state contract сохраняется |
| Crownshell | Layer-shell/exclusive zones — отдельные Linux capabilities при конкретном shell consumer. Они не заявлены обычным window API; compositor blur/fractional scaling не обещаются |
| eguidev / ps-qa | Unique subjects и ack/effect distinction уже реализованы; docs усилены и diagnostic completeness pinned. Protocol не зависит от GPU/window backend; native projections отдельно от raw snapshot |
| Strek | Domain command/undo/export architecture — reference для приложений. Generic framework не должен получить чужую document model; action/ordinary input используют существующие dispatch paths |
| wgpu-profiler / Ruffle | Existing ProfileFrame Drop помечает abandoned frame; следующий begin восстанавливает profiler. Existing failure test покрывает submitted/refused/unwind. wasm+profiler запрещён compile-time; чужая wasm leak не приписывается FLUI |
| Compose 1.12 | Testing guide уточняет settle/virtual clock/presentation. Measured size и paint bounds уже различаются; merged semantics, RTL/IME cases требуют своих producer/native tests |
| Chrome / Safari WebGPU | Renderer запрашивает необходимые поддерживаемые capabilities. Subgroup/clip_distances пока без kernel consumer; DOM inspection не заменяет canvas semantics/pixels |

## Следующие изменения с самостоятельным контрактом

1. **Web IME через существующий TextStore.** В browser backend нужен editable
   DOM bridge, который сохраняет surrounding context, переводит selection и
   replacement ranges в явных единицах и применяет изменения через существующий
   frame-transaction gate. До реализации зафиксировать cross-crate контракт в ADR:
   owner/focus lifecycle, composition cancellation, surrounding deletion,
   autocorrect и отключение bridge после detach. Принимать по live cases на
   iOS/Android, включая Samsung Cheonjiin; synthetic key events этого не докажут.
2. **Linux accessibility acceptance.** После compilation прогнать на Linux
   приложение с enabled/disabled controls и labelled image через AT-SPI client.
   Проверить states, Image interface, action refusal и повторную activation.
   Existing core role tests не являются этим native evidence.
3. **Сценарий агента с filtered visibility.** Использовать существующий
   HeadlessDevAgent read path и точный subject; initial assertion, production
   action, конечное ожидание effect, затем assertion значения. В negative control
   отключить product action и убедиться, что сценарий падает. Raw A11yTree
   оставить для assembly diagnostics, где нужны structural containers.
4. **Оптимизация только после workload.** Большой внешний consumer через facade
   должен дать clean/incremental compile time, frame profile, allocation и binary
   size. Затем выбрать один kernel для SIMD или один layout adapter. Изменение
   generic API/renderer оправдано измерением и сохранением behavior, а не числом
   features у другой библиотеки.

Maintainer разрешил breaking changes, улучшающие API и архитектуру. Это применимо
к следующим проектам, когда новая форма устраняет конкретное invalid state или
дублирование ответственности. В этой работе смена raw query semantics нарушила
бы consumer assembly tests и смешала разные projections; явная документация
существующего контракта дала более точный результат. Новые cross-crate decisions
требуют ADR, а изменённое behavior — отличающего regression test.

## Проверки текущего GlyphImage

Текущий source основан на слитом `origin/main` `2992d8c44`. Constructor,
producer wiring, engine admission/replay и consumer migration дополнительно
прочитаны независимым агентом; блокирующих дефектов не найдено.

- `cargo nextest run -p flui-painting --features serde parley_oracle_contract
  --locked --no-capture`: family прошла, включая 19 новых constructor cases.
  Strict painting Clippy с `--all-targets --features serde` также прошёл до
  rebase; проверяемый source при rebase не менялся.
- `cargo test -p flui-painting --doc GlyphImage --locked`: **3 passed**,
  consumer compile-fail cases для struct literal, field mutation и mutable bytes.
- Required GPU family
  `failed_glyph_replay_retries_without_reusing_recorded_regions` с
  `FLUI_REQUIRE_GPU=1`: **1 passed**, семь строк, **5.36 s**.
  Выполнение на Windows, без GPU skip; выбранный adapter helper не печатает.
  Это atlas ownership/retry test, а не заявление о pixel output или speedup.
- Четыре production controls выполнились последовательно: открытые поля дали
  unexpected compile success (**exit 101**); отключённая length validation
  пропустила malformed buffers (**exit 100**); wrapping arithmetic пропустила
  color bitmap с обнулённым byte count и неверно классифицировала maximum size
  (**exit 100**); удаление раннего device-limit guard вызвало лишний healthy
  glyph replay в обеих oversized orientations (**expected 1 / actual 2**, exit
  **100**). Каждый source восстановлен exact-byte в `finally`; restored
  constructor family, три privacy doctests и GPU family прошли снова.
  Логи: `target/glyph-control-*.log`, `target/glyph-constructor-restored.log`,
  `target/glyph-privacy-restored.log`, `target/glyph-device-limit-restored.log`.

`cargo xtask checks` и `cargo xtask check-changed` завершились с **exit 0**.
Changed-crate closure: **494 passed, 0 skipped**, **316.674 s**; strict rustdoc,
**434 passed / 333 ignored** doctests, Windows CLI Clippy, wasm-compatible
closure и facade hot-reload check прошли. Оба cargo-hack each-feature прохода
по **60** конфигураций (библиотеки и tests/benches/examples) прошли.
Журналы: `target/glyph-checks.log`, `target/glyph-check-changed.log`.
Пять локальных ссылок текущего отчёта отдельно проверены через `Test-Path`.
Native macOS/Linux/mobile/browser execution этим прогоном не устанавливается.
Исторические числа из #1418 к этому продолжению не относятся.

## Исторические проверки #1418 и границы выполнения

Работа разделена между агентами по tooling, runtime/testing и graphics;
координатор изучал platform и интегрировал dependency cohort. Изменения
дополнительно прочитаны независимым агентом. Это targeted source review вокруг
тем подборки, не полный повторный audit всех production files и feature combinations.
Сборки выполняются последовательно на общем memory-limited Windows-хосте.

| Область изучения | Source/feature scope |
|---|---|
| Tooling | Root manifest/profiles/lints, .cargo/.config, xtask execution/plans, native nextest version policy; nightly compiler experiments не запускались |
| Graphics | Engine glyph atlas, capability requests, profiler frame guard, shaders/compositing checks и painting shaping boundary; не полный rendering audit |
| Runtime/testing | A11yTree, harness pump/snapshot, runtime agent dispatch, protocol projections и semantics role/action contracts; без native accessibility execution |
| Platform/catalog | Optional a11y manifests/Unix adapter, текущие Web listener registrations, TextStore units, AnimatedSwitcher lifecycle и layout architecture; без mobile/browser device runs |

- `cargo nextest show-config version`: current **0.9.146**, required **0.9.133**,
  recommended **0.9.145**, evaluation **ok**.
- `cargo nextest run -p flui-testing -p flui-semantics`: **44 passed, 0 skipped**.
- `cargo clippy -p flui-testing -p flui-semantics --all-targets -- -D warnings`:
  exit **0**.
- Diagnostic negative control намеренно исключал третий и последующие ambiguity
  descriptions: новая строка consumer table падала с **expected 3 / actual 2**,
  exit **100**. После byte-exact восстановления source таблица прошла.
  Это защита сохранённого контракта; прежняя eager implementation не объявляется
  дефектной. Локальный журнал: `target/research-query-control.json` и `.log`.

- `cargo test --locked -p flui-engine --features testing --lib
  failed_glyph_replay_retries_without_reusing_recorded_regions -- --nocapture`:
  **1 passed, 0 failed, 0 ignored**, четыре cases; restored run также прошёл.
  Windows/DX12, required GPU без skip path; выбранный adapter helper не выводит.
- Возврат missing-replay дефекта дал **exit 101**, ожидаемое число вызовов
  rasterizer **3**, фактическое **2**. Возврат unchecked bitmap product дал
  **exit 101**, `attempt to multiply with overflow`. Оба temporary дефекта
  удалены через saved-byte `finally`; локальные логи
  `target/adoption-glyph-{missing-control,overflow-control,restored}.log`.
- `cargo xtask checks`: **exit 0**, повторный финальный запуск также прошёл;
  журнал `target/research-final-checks.log`. Архивные research links дополнительно
  проверены явным PowerShell обходом: **2** в adoption report, **4** в подборке.
- Внутри `check-changed` строгие
  `cargo clippy --workspace --all-targets --locked -- -D warnings` и
  `cargo clippy -p flui-engine --all-targets --locked --features testing -- -D warnings`
  завершились успешно. Workspace nextest: **650 passed, 10 skipped**
  (12 slow), **608.040 s**. Десять skipped — существующие ignored tests
  desktop-mcp: interactive pointer/keyboard scenarios и helper processes;
  они не являются проверкой accessibility adapter.
- `cargo xtask check-changed`: **exit 0**, локальный журнал
  `target/research-check-changed.log`. Помимо указанных Clippy/nextest прошли:
  strict rustdoc workspace с private items; doctests (по 42 итоговым строкам
  журнала **619 passed, 0 failed, 389 ignored**); a11y all-targets Clippy
  flui-platform для Windows и macOS; Windows CLI и Windows/macOS desktop MCP;
  wasm-compatible workspace Clippy и facade hot-reload check.
  Per-feature cargo-hack не входил в сформированный план этого gate; его
  выполнение не заявляется. Android/iOS targets отсутствуют, iOS runner также
  требует macOS, Linux platform suite требует Xvfb — эти шаги были пропущены.
- Дополнительный Unix adapter check:
  `cargo clippy -p flui-platform --locked --no-default-features --features a11y --all-targets --target x86_64-unknown-linux-gnu -- -D warnings`:
  **exit 0**, **2m 42s**. Компилируются accesskit_unix 0.24.0 и
  accesskit_atspi_common 0.21.0; журнал `target/research-linux-a11y.log`.
  `--no-default-features` изолирует a11y от Linux window-system dependencies;
  native Linux presentation здесь не запускалась.

Linux/macOS native accessibility, mobile IME и browser presentation невозможно
подтвердить Windows compilation/headless suite; их нельзя объявлять проверенными
по одному успешному Cargo gate.
