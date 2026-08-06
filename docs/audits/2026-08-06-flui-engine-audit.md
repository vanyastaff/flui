# Аудит flui-engine: рендер-движок, backpressure, ресурсы, текст — 2026-08-06

**Модель:** Qwen (Qwen Code, Alibaba Group).

**Метод:** прямое чтение ключевых модулей (`renderer.rs` — все продакшен-пути, `backend.rs`,
`headless.rs`, `layer_render.rs`, `raster.rs`, `raster_owner.rs` точечно, `error.rs`, `mod.rs`,
`build.rs`, `Cargo.toml`) + 3 параллельных deep-dive агента (raster/backpressure, GPU-ресурсы,
текст/filter-пайплайны) + сверка их утверждений по источнику. Читались также `ci.yml`,
`runtime-contract.toml`, `scripts/port-check.sh`, интеграция в `flui-app` (`runner.rs`,
`ui_realm.rs`, `presentation.rs`).

**Объект:** ветка `main` (HEAD `df5e74fd`), крейт `flui-engine` ~67k строк (из них ~20k —
GPU-readback-тесты), wgpu 29, Rust 1.97. Все пути ниже — `crates/flui-engine/`, если не указано
иначе; строки проверены по текущему состоянию файлов.

**Формат уверенности:** все пункты — доказанные (есть `file:line` evidence), кроме явно
помеченных как «не проверено».

---

## 1. Executive summary

Инженерная культура в крейте высокая: record/replay IR с машинно-проверяемой чистотой
(`DrawSegment: Clone`), A/B-гейт детерминизма, ~440 GPU-readback-тестов с CPU-ораклами
merge-blocking в CI, типизированная модель ошибок с exhaustive-классификацией, честные
SAFETY-комментарии.

Системные проблемы сосредоточены в четырёх местах:

1. **Половина новой архитектуры — невостребованные строительные леса.** `RasterOwner`
   (2 782 строки) + `RasterOptions` + bench + allocation-тест не имеют ни одного
   продакшен-потребителя; прод рендерит синхронно на потоке event loop через
   `Arc<Mutex<Renderer>>`. Damage-tracking (partial scissor + self-heal) мёртв в проде:
   единственный прод-вызов — безусловный `mark_full_repaint()`.
2. **Документация разошлась с кодом.** ARCHITECTURE.md описывает завершённые рефакторинги как
   открытые (и наоборот), README описывает удалённое API эпохи v0.7.0, crate-AGENTS.md утверждает
   «enable-wgpu-tests not run in CI» вопреки merge-blocking gpu-test джобе, port-check-обещание
   вернуть `backend.rs` в trigger 5 не исполнено. Все doctest'ы — `rust,ignore`.
3. **Headless-рендер не совпадает с экранным** — инструмент визуальной самопроверки
   (Definition-of-Done) показывает не то, что увидит пользователь: `ShaderMask` без маски,
   `BackdropFilter` без blur, `FollowerLayer` без разрешения позиции.
4. **Ресурсные бюджеты и recovery-пути неполны.** У offscreen-пулов нет байтовых лимитов
   (до ~500 MB idle VRAM на 4K), учёт атласной памяти в `TextureCache` фиктивный, resize не
   чистит пулы; `DeviceLost` после неудачного `recover()` не ставит wake; `SurfaceValidation`
   в проде не чинится никем.

Вердикт: ядро рендер-пути здоров и хорошо прикрыт тестами; главные риски — честность
документации/DoD-инструментов, старение невстроенных лесов (`RasterOwner`) и дыры в
resource/recovery-контурах long-running приложения.

---

## 2. Что действительно сильно

- **Record/replay IR** (`src/wgpu/command_ir.rs`): чистота машинно-проверена через `Clone`,
  A/B replay клонов в `deterministic_replay_tests.rs` даёт байтовую идентичность (T11 C5-гейт).
- **GPU-сьют ~440 тестов merge-blocking в CI** на windows-latest/WARP (`ci.yml:385-437`,
  `--test-threads 1`, PNG-дампы при падении); CPU-ораклы и дискриминаторы
  (`blur_filter_tests`, `morphology_filter_tests`, `mode_filter_tests` против `Color::blend`,
  `aa_oracle_tests`).
- **Обработка `get_current_texture`** (`wgpu/renderer.rs:1258-1312`): все варианты
  `CurrentSurfaceTexture` разобраны; `Validation` отделён от `SurfaceLost` (защита от вечного
  ретрая), `Occluded` скипает кадр без present, `Outdated|Lost` — один reconfigure+retry.
- **Модель ошибок** (`src/error.rs`): `#[non_exhaustive] EngineError` + `Recoverability` с
  exhaustive-match (`error.rs:220-234`) — новый вариант не скомпилируется без классификации.
- **Анти-wobble меры**: DComp-present на DX12 (`renderer.rs:364-386`),
  `desired_maximum_frame_latency: 1` с задокументированным обоснованием (`renderer.rs:480-500`).
- **unsafe инвентаризирован**: один `unsafe`-блок создания поверхности + 5 блоков disjoint-borrow
  в `buffer_pool.rs:277-308` — проверены, UB нет (но см. §3.4 про хрупкость).

---

## 3. Текущие боли

### 3.1. Архитектура и процесс

#### 3.1.1. `RasterOwner` — 2,8k строк механизма без единого продакшен-потребителя

`src/raster_owner.rs` (2 782 строки) + `src/raster_options.rs` (222) + bench +
`tests/raster_backpressure_allocation.rs`. Прод рендерит по-старому:

- desktop: winit-колбэк → `renderer_frame.lock()` → синхронный `render_scene` на потоке
  event loop (`crates/flui-app/src/app/runner.rs:7244-7246`);
- web: `Arc<Mutex<Option<Renderer>>>` (`crates/flui-app/src/app/runner.rs:8474`).

Код сам это признаёт: «RasterOwner is unwired scaffolding reserved for the planned threaded
raster owner» (`src/raster_owner.rs:1058-1060`); контракт `raster-owner-in-shipping-path` —
`state = "partial"`, owner issue #559 (`docs/runtime-contract.toml:957-967`).
`RasterOptions::max_frames_in_flight` честно помечен «currently INERT against today's
synchronous RasterOwner» (`src/raster_options.rs:46`).

При этом заголовок `crates/flui-app/src/app/presentation.rs:1-6` описывает целевое состояние
как текущее: «raster/surface ownership remains in `flui_engine::RasterOwner`».

Боль: две модели кадра живут параллельно; большой механизм (mailbox ёмкостью 1, lossy ack-канал,
RAII-тикеты, retire→wake) стареет без реальной нагрузки; читатель документации получает ложную
картину. Дефекты самого протокола (§3.5) дешевле чинить до подключения.

#### 3.1.2. Damage tracking мёртв в проде

Единственный прод-вызов — безусловный `renderer.mark_full_repaint()` перед каждым
`render_scene` (`crates/flui-app/src/app/ui_realm.rs:2113`); `RasterOwner::pump` делает то же
(`src/raster_owner.rs:1040-1045`). Все `mark_dirty(rect)` в flui-app — тестовые no-op-моки.
Значит, partial-damage scissor, `force_full_repaint_next_frame` self-heal и
`has_advanced_shape_straddling` (`src/wgpu/renderer.rs:1435-1494`) — инфраструктура без
потребителя; код сам признаёт: «callers use full repaint exclusively». Риск гниения + ложная
уверенность, что оптимизация «есть».

#### 3.1.3. Дрейф документации — системный

- `ARCHITECTURE.md`: Friction log / Outstanding refactors описывают
  `Arc<parking_lot::Mutex<OffscreenRenderer>>` как нерешённую проблему — рефакторинг **уже
  сделан** (`Renderer::offscreen: Option<OffscreenRenderer>`, `Backend<'frame>` с
  `&'frame mut OffscreenRenderer` существует, `renderer.rs:191-210`, `backend.rs:81-86`);
  split `offscreen.rs` на каталог `offscreen/` тоже сделан, но числится отложенным. И наоборот:
  перфрейм-`Arc::clone` в `backend.rs:254-255, 404-405, 831-832` всё ещё живы.
- `README.md` описывает API эпохи v0.7.0 (`ContainerLayer`, `PictureLayer` в flui-engine) —
  эти типы давно в `flui-layer`; примеры README не компилируются.
- `src/wgpu/mod.rs` про `advanced_blend`: «No production caller exists yet (wired in PR-3)» —
  вызовы есть (`opacity_layer.rs:36`, `replay/mod.rs:397`, `ssaa.rs:580`).
- `AGENTS.md` крейта: «enable-wgpu-tests … (not run in CI)» — gpu-test гоняет полный сьют
  merge-blocking (`ci.yml:385-437`).
- `scripts/port-check.sh:284-305`: trigger 5 держит `backend.rs`/`renderer.rs` вне скоупа с
  обещанием «when the refactor lands, backend.rs MUST be added to this trigger's scope in the
  same PR» — рефакторинг `Backend<'a>` лёг, обещание не исполнено; гейт не ловит регрессии,
  для которых создан.

#### 3.1.4. Ни одного компилируемого doctest'а

Все ```rust-блоки в крейте — `rust,ignore` (единственный не-ignore — `compile_fail`-контракт
`Renderer: !Sync` в `renderer.rs`). Примеры в доках гниют молча; `cargo test --doc` не
проверяет ничего.

### 3.2. Рендер-пайплайн

#### 3.2.1. Headless-рендер ≠ экранный рендер

`HeadlessRenderer::walk_layer_tree` (`src/wgpu/headless.rs:218-234`) «зеркалит»
`render_layer_recursive`, но без спец-случаев:

- `ShaderMaskLayer` уходит в generic-impl (save_layer + clip, без GPU-маски,
  `layer_render.rs:334-353`) — на экране работает полный mask-пайплайн
  (`renderer.rs:1813-1965`);
- `BackdropFilterLayer` — no-op без blur (`layer_render.rs:356-366`);
- `FollowerLayer` — `render()` no-op («Transform is calculated by the compositor»,
  `layer_render.rs:423-433`): на экране позиция разрешается через
  `resolve_follower_offset` (`renderer.rs:1636-1650`), в headless — нет.

Headless-девайс создаётся с `DeviceDescriptor::default()` без `required_features/limits`
оконного пути (`headless.rs:43-60`). Это бьёт прямо в DoD-практику «посмотри PNG вместо окна»
(корневой AGENTS.md): инструмент визуальной самопроверки показывает не то, что увидит
пользователь.

#### 3.2.2. Текст всегда поверх всего и выпадает при повторных flush'ах

Текст вне Command IR (T11 не решён, `ARCHITECTURE.md:276`), пишется финальной глобальной фазой
(`replay/mod.rs:251-252, 569-570`). Следствия:

- Z-перемежение текста с геометрией невозможно;
- при нескольких `painter.render` за кадр (backdrop-flush'и) `batch.clear()` (`text.rs:858`)
  уводит текст в первый flush — в последующих его нет.

Статус-кво нигде не описано как продуктовое ограничение.

#### 3.2.3. Переполнение glyph-атласа = потеря кадра без восстановления

`glyphon::PrepareError::AtlasFull` → `EngineError::TextPrepare` →
`Recoverability::Unrecoverable` (`error.rs:230-232`): никакого роста/пересоздания атласа,
никакого частичного рендера; теста нет. Для long-running app с большим разнообразием глифов
(CJK + иконки + эмодзи) — кадровый дроп как steady-state.

#### 3.2.4. Нет инвалидации текстовых кэшей при `register_font`

`PaintingBinding::register_font` (`crates/flui-painting/src/binding.rs:441`) мутирует общий
FontSystem, но уже отшейпленные `Buffer`'ы остаются в `plain_cache`/`rich_cache` под
контент-ключами до LRU-выброса; API очистки снаружи нет вообще (`text.rs:499-899`).

#### 3.2.5. HDR — эвристика без проверки дисплея

`check_hdr_support` возвращает `true` просто для backend==Metal|Dx12
(`renderer.rs:127-135`), `select_surface_format` предпочитает `Rgba16Float`
(`renderer.rs:923-927`). Пайплайн пишет sRGB-байты как есть (гамма-пространство,
Flutter-parity, обоснование `renderer.rs:903-921`). Ни запроса возможностей дисплея, ни
цветовых метаданных. Итог на HDR-дисплее потенциально washed-out/пересвеченный UI, плюс
intermediate-текстуры в fp16 = 2× полоса пропускания. **wgpu 30 добавляет
`Surface::display_hdr_info` и `SurfaceColorSpace` — см.
[docs/research/2026-08-06-wgpu-alternatives-rust-ui-rendering.md](../research/2026-08-06-wgpu-alternatives-rust-ui-rendering.md).**

#### 3.2.6. Фильтры: шесть вручную скопированных семейств

`blur/`, `morphology/`, `color_matrix/`, `gamma/`, `mode/`, `advanced_blend/` — один шаблон
(`apply_*`-драйвер + `pipeline.rs` + `generated.rs`), скопированный почти дословно, включая
UV-rebase (`blur/mod.rs:118-135` ≈ `morphology/mod.rs:96-112`) и `test_device()`-хелпер в
каждом `pipeline.rs`. Общей абстракции «fullscreen ping-pong pass» нет. Новый фильтр ≈ 8 точек
правки и ~600-800 строк копий.

Поверх: **три поколения blur** — Dual Kawase в `offscreen/blur.rs` (backdrop layer-tree путь),
сепарабельный Гаусс в `blur/` (display-list `ImageFilter::Blur`), удалённые compute-шейдеры в
комментариях `shader_compiler.rs:29-35`. `offscreen/` — параллельная вселенная со своим
`TexturePool` и `ShaderCache` (`offscreen/mod.rs:163-164`) и 6 пустыми `#[ignore]`-заглушками
в тестах (`offscreen/mod.rs:475-540`). Backdrop-сигма схлопывает анизотропию:
`f32::midpoint(sigma_x, sigma_y)` (`renderer.rs:1711`) — расхождение с Flutter
`ImageFilter.blur` на анизотропных значениях.

`wgsl_bindgen`-генерация биндингов покрывает 6 из ~17 WGSL-файлов (`build.rs:12-20`);
остальные (blit, downsample/upsample, shadow, masks, gradients, shape) — на ручных layout'ах.

#### 3.2.7. SSAA корректен, но дорог

На каждый SSAA-путь (`ssaa.rs:283+`): ≥3 render pass'а, 2 pooled-текстуры (bucketing по 64 px —
до 63 px waste на сторону, `ssaa.rs:55`), CPU-клон всего сегмента + по-вершинный remap
(`ssaa.rs:445-450`). На сценах с сотнями мелких path'ов — заметный CPU+VRAM налог; метрик
стоимости нет. Защиты на месте: деградация при тайле больше `max_texture_dimension_2d/2`
(`ssaa.rs:354-375`), tile-safe partition прогнан орклами (`pipeline.rs:493+, 658+`).

### 3.3. VRAM и ресурсы

#### 3.3.1. У пулов offscreen-текстур нет байтовых бюджетов

`TexturePool` ограничен только числом idle-текстур: 16 у `OffscreenRenderer`
(`offscreen/mod.rs:165`), 4 у `GpuResources` (`resources.rs:95`); `total_memory_bytes`
(`texture_pool.rs:145`) — статистика, не лимит. На 4K-окне idle-набор пула `OffscreenRenderer`
≈ до ~500 MB VRAM. `take_matching` требует точного совпадения `(w, h, format)`
(`texture_pool.rs:172-178`) — никакого bucketing'а; `clear_texture_pool` существует
(`offscreen/mod.rs:383-384`), но не вызывается никем — при resize текстуры старых размеров
лежат мёртвым грузом до count-based вытеснения.

#### 3.3.2. `TextureCache`: бюджет есть, учёт фиктивный

100 MB захардкожены (`texture_cache.rs:305`); API настройки (`with_memory_budget`,
`set_max_memory_bytes`, `shrink`, `remove`) — ноль прод-вызовов (grep). Eviction не трогает
`use_count > 0` (`texture_cache.rs:690-691`) → working set, используемый каждый кадр,
**невытесняем никогда**. Атлас 2048×2048 (16.8 MB, `atlas.rs:99`) в `memory_bytes()`
(`texture_cache.rs:676-678`) не учитывается — eviction «освобождает» байты на бумаге,
физические пиксели живут до полного `reset()`, который срабатывает только при `atlas_full`
И хотя бы одной неиспользованной записи (`texture_cache.rs:756-760`); если working set целиком
заполняет атлас — новые мелкие изображения **навсегда** уходят в standalone-текстуры.
Плюс: 1×1 placeholder-текстура на каждую atlas-запись (`texture_cache.rs:454-470`), и
`TextureId::Pointer` (идентичность по `Arc::as_ptr`, `texture_cache.rs:76-82`) — ABA после
освобождения `Arc`: новый аллок может получить тот же адрес и унаследовать чужой кэш-ключ.

#### 3.3.3. Нет агрегированного бюджета и реакции на memory pressure

Пять менеджеров в `GpuResources` (`resources.rs:43-103`) живут по разным политикам
(count/bytes/никакой), суммарный VRAM никто не видит; системные сигналы нехватки памяти не
обрабатываются. `UniformPool` не усаживается вовсе (`uniform_pool.rs:80-119`; несущественно
из-за размеров буферов 16-80 B). `ExternalTextureRegistry` — без eviction/лимитов/инвалидации,
с контрактым рассогласованием: `update` всегда берёт `linear_sampler`, игнорируя
`use_linear_filter` из `register` (`external_texture_registry.rs:235-299`); прод-потребителей
нет (заготовка под видео/камеру).

#### 3.3.4. `unsafe` в `buffer_pool.rs` корректен, но хрупок

5 блоков disjoint-borrow (`buffer_pool.rs:277-308`) проверены: алиасинга нет, провенанс
`vertex_ptr` сохранён, lifetimes блокируют dangling в safe-коде. Инвариант держится на том, что
«второй вызов `get_buffer_internal` не трогает `vertex_buffers`» — будущая правка ломает это
без предупреждения компилятора. SAFETY-комментарии здесь load-bearing.

### 3.4. Надёжность рантайма

#### 3.4.1. DeviceLost: после неудачного `recover()` не остаётся wake

Ветка `Err(DeviceLost)` в `render_frame_entered` логирует и дропает кадр без `retry_needed`
(`crates/flui-app/src/app/ui_realm.rs:2130-2136` — в отличие от `SurfaceLost`); единственный
retry-wake живёт в `Ok`-ветке `recover()` в runner'е (`runner.rs:7254-7262`), ветка `Err`
только логирует. Если loop idle (анимаций нет, инпута нет), восстановление может ждать первого
внешнего события неопределённо долго.

#### 3.4.2. `SurfaceValidation` в проде не чинится

Классифицирован как «нужен reconfigure, ретрай зациклится» (`renderer.rs:1301-1312`), но
прод-путь только логирует «external reconfigure required» (`ui_realm.rs:2140-2144`) — внешнего
вызова `reconfigure_surface` нет. Misconfigured surface = вечный дроп кадров.

#### 3.4.3. Весь кадр сериализован одним мьютексом на потоке event loop

`renderer.lock()` вокруг vsync-блокирующего Fifo-present (`runner.rs:7244-7246`) + fallback
`sleep(16ms)` на том же потоке (`runner.rs:7286-7297`) блокирует диспетчеризацию инпута;
device-lost `recover()` — это `pollster::block_on` внутри кадрового колбэка
(`runner.rs:7248-7253`).

#### 3.4.4. Дефекты протокола `RasterOwner` (всплывут при подключении)

- `run_until_shutdown` паркуется навсегда, если `shutdown()` не вызван: дроп всех
  `RasterHandle` не останавливает owner'а — поток + backend утекают
  (`raster_owner.rs:1126-1140`);
- осиротевший при дропе owner'а кадр ретирится **без ack'а** — потребитель не узнает
  (`raster_owner.rs:1215-1232`);
- `Presented`-ack отправляется и для `Ok(false)` (кадр не дошёл до present) — телеметрия
  будет врать при pacing по ack'ам (`raster_owner.rs:1050-1060`);
- wake-хук «не паникуй и не блокируй» — контракт без механической защиты; паник в хуке на
  unwind-пути = abort процесса (`raster_owner.rs:160-175`).

#### 3.4.5. `mark_first_frame_sent` латчится до результата render

Флаг ставится при `Painted`-исходе сегмента (`ui_realm.rs:1930-1936`), а сам `render_scene`
выполняется позже (`ui_realm.rs:2108`): упавший первый кадр всё равно считается отправленным.

### 3.5. Тестирование

- **Только WARP в CI**: gpu-test на windows-latest; Vulkan (Linux/Android) и Metal (macOS/iOS)
  не исполняются в CI вообще — только cross-typecheck. Софтверный растеризатор ≠ железные
  драйверы.
- **CPU-тесты под GPU-гейтом**: структурные C2/C4/C5 (`compose_filter_tests.rs:666-694`) и
  `ShaderCache`-тесты не требуют GPU, но не бегут без `enable-wgpu-tests`.
- **Интеграции `RasterOwner` с реальным `Renderer` нет**; все unit-тесты на `FakeBackend`
  (`raster_owner.rs:1305-1335`); bench ничего не утверждает; allocation-тест одно-поточный по
  явному допущению counting-аллокатора.
- **Wall-clock wake активация** (`ResumeTimeReached` → redraw всем окнам,
  `crates/flui-platform/src/platforms/winit/platform.rs:706-727`) не тестируется — winit не
  даёт строить EventLoop вне main thread; покрытие только slot-уровня.
- **Пустые `#[ignore]`-заглушки**: 6 штук в `offscreen/mod.rs:475-540`.
- **Поведение при `AtlasFull` не покрыто вообще** (§3.2.3).

Сильные стороны, для баланса: гонки ack-порядка и счётчика прогнаны по 200 итераций на реальных
потоках без sleep-костылей (`raster_owner.rs:1480-1540, 2740-2770`), panic-пути render/resize
закрыты (`raster_owner.rs:2330-2470`), self-deregistering wake-хук (`raster_owner.rs:2660-2720`).

---

## 4. Будущие риски

1. **Подключение threaded raster owner (#559)** — главный риск. Мост между
   `InFlightAccounting` (`AtomicU32`, cross-thread, `raster_owner.rs:135`) и
   `FrameClock.in_flight` (`Cell<u8>`, owner-thread,
   `crates/flui-scheduler/src/frame_clock.rs:606-615`) — новый код; забытый retire на
   clock-стороне = вечный `Skip(Backpressure)` (это прямо названо в `frame_clock.rs:431-438`).
   Hot-restart потребует shutdown→пересоздание owner'а под живым окном — не спроектировано;
   `recover()` асинхронный, а `RasterOwner` sync-only.
2. **Multi-window**: `open_secondary_window` открывает окно без контента и без рендера
   (названное ограничение, `runner.rs:7565, 7928`); wake-deadline пинает **все** окна O(N)
   (`platform.rs:719-727`); `next_wake` учитывает только gesture-дедлайны
   (`ui_realm.rs:1678-1682`) — таймеры/анимации wall-clock wake не получают;
   `draw_frame_entered` возвращает outcome только последней презентации
   (`ui_realm.rs:1838-1841`), submit идёт всегда в primary (`ui_realm.rs:2096-2110`) — при
   N>1 это cross-submit.
3. **HiDPI-change не обрабатывается напрямую**: `ScaleFactorChanged` нигде в flui-app не
   матчится; DPR обновляется только через последующий `Resized` (`platform.rs:866-884`,
   `runner.rs:645`) — гарантия winit предполагается, но не зафиксирована и не протестирована
   *(не проверено воспроизведением)*.
4. **VRAM-давление в long-running apps**: стабильный over-budget кэша (§3.3.2), ~500 MB
   idle-пул (§3.3.1), мёртвые текстуры после resize-шторма, отсутствие реакции на драйверные
   сигналы.
5. **Расползание фильтров**: каждое новое семейство удваивает риск расхождения копий
   (отличия уже живут в копиях, не в параметрах: sampler у blur, способ сборки шейдера у
   mode/advanced_blend).
6. **Dependency treadmill**: wgpu 29 / glyphon 0.11 / naga_oil 0.22 / wgpu-profiler 0.27 —
   быстрые upstream'ы с breaking changes. wgpu 30 уже вышел (2026-07-01), glyphon 0.12 требует
   wgpu 30 — апгрейд-цепочка неизбежна; детали в
   [docs/research/2026-08-06-wgpu-alternatives-rust-ui-rendering.md](../research/2026-08-06-wgpu-alternatives-rust-ui-rendering.md).
7. **HDR/WCG**: с приходом реального HDR-контента вся гамма-модель «sRGB-байты как есть»
   потребует пересмотра end-to-end (surface format → blending → tone mapping → метаданные);
   wgpu 30 даёт для этого API.
8. **Текст**: inline-виджеты сейчас эмитятся как `\u{FFFC}` без реальной отрисовки
   (`text.rs:92-99, 946, 957`); с приходом сложной типографики §3.2.3/§3.2.4 станут
   продуктовыми багами.

---

## 5. Приоритеты (что делать в первую очередь)

1. **Починить headless-паритет** (§3.2.1): общий layer-walk для экранного и headless путей —
   фундамент DoD-практики; сейчас golden-инструмент лжёт про ShaderMask/Follower/BackdropFilter.
2. **DeviceLost/SurfaceValidation recovery** (§3.4.1/§3.4.2): retry-wake в ветке
   `Err(DeviceLost)` + авто-reconfigure при `SurfaceValidation`. Дёшево, закрывает реальные
   «вечно мёртвый рендер» сценарии.
3. **Синхронизировать документацию с кодом** (§3.1.3/§3.1.4): один проход по
   ARCHITECTURE.md/README/AGENTS.md/mod.rs + исполнение port-check-обещания (вернуть
   `backend.rs` в trigger 5) + снять `ignore` с ключевых doctest'ов.
4. **Байтовый бюджет и resize-purge для `TexturePool`** (§3.3.1) + честный учёт атласа в
   `TextureCache` (§3.3.2) — до больших изображений и 4K-multi-window.
5. **Решить судьбу `RasterOwner`** (§3.1.1/§3.4.4): либо встроить (#559) с мостом к
   `FrameClock` и фиксом трёх дефектов протокола, либо честно заморозить с пометкой в
   presentation.rs. Дешевле чинить до подключения.
6. **Стратегия atlas-overflow** (§3.2.3): grow-and-retry вместо Unrecoverable-дропа + тест.
7. **Тестовый контур вне WARP**: хотя бы периодический прогон GPU-сьюта на Vulkan — сейчас
   целый класс драйверных багов невидим.
