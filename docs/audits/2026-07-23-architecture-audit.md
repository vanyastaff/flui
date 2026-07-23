# Аудит архитектуры FLUI — 2026-07-23

**Метод:** 10 параллельных исследовательских агентов по направлениям (дерево/реконсиляция, ownership/unsafe, layout, рендер-пайплайн, планировщик/конкурентность, состояние/BuildContext, ввод/жесты, API/traits, диагностика/тесты/бенчмарки, анимация/темы/семантика/текст). Все ключевые утверждения сверены с кодом; три самых тяжёлых (неинкрементальный paint, безусловный `mark_full_repaint`, приватная gesture arena) дополнительно проверены вручную по `file:line`. Формат уверенности: **доказанный дефект** (есть evidence), **риск** (правдоподобно, не подтверждено), **гипотеза** (требует замера).

**Объект:** 28 crates, ~573k строк Rust (включая тесты), wgpu 29, Rust 1.96, edition 2024.

---

## 8.1. Executive summary

**Что построено.** FLUI — это добросовестный, хорошо протестированный *перенос* Flutter на Rust: трёхдеревная модель (View → Element → Render), протокол constraints-down/size-up, slivers, gesture arena, inherited-зависимости, dirty-флаги. Качество исполнения местами высокое: генерационные ID с ABA-защитой (`crates/flui-foundation/src/id.rs:983-1023`), slab-хранилища вместо указательного графа, дисциплинированный `unsafe` в `subtree_arena` с Miri-покрытием, headless-детерминированный `HeadlessBinding::pump_frame` (`crates/flui-binding/src/lib.rs:510-550`), точные множества зависимых для inherited (не broadcast), настоящая ленивая виртуализация sliver-списков, структурная отмена async-задач через `TaskToken` + генерационные гейты. Это — реальные преимущества перед Dart-оригиналом, и их мало.

**Главная системная проблема.** Проект скопировал не только контракты Flutter, но и его *экономику кадра*, не реализовав при этом те оптимизации, которые делают эту экономику приемлемой, и не использовав те оптимизации, которые позволяет Rust. Три фундаментальных дефекта:

1. **Render-пайплайн immediate-mode выше уровня GPU-кэшей.** Любой dirty-узел вызывает полный обход дерева paint, пересоздание всего `LayerTree` и полный repaint окна (`crates/flui-rendering/src/pipeline/owner/paint.rs:50-84`, `crates/flui-app/src/app/binding.rs:1392`). Damage tracking реализован, но обойдён. Repaint boundaries формируют структуру слоёв, но не режут стоимость paint. Это хуже Flutter, который удерживает pictures и boundary-subtrees.
2. **Rebuild-модель перестраивает и глубоко клонирует поддеревья без identity short-circuit.** Каждый `update` ребёнка безусловно клонирует view (2–3 раза) и заново помечает элемент dirty (`crates/flui-view/src/element/dispatch.rs:149-153`, `id_reconcile.rs:166,258,284`). Единственное противоядие (`Memo`) не используется нигде. Изменение состояния у корня = O(поддерево) клонов и rebuild'ов на кадр.
3. **Текст — split-brain архитектура с двойным shaping.** Измерение идёт через `cosmic-text TextLayout`, отрисовка — через отдельный glyphon-стек; результаты не разделяются; каждый `layout()` делает 3 shaping'а; весь shaping сериализован через один глобальный `Mutex<FontSystem>`; глифы игнорируют CTM — текст ломается при DPR≠1 и трансформациях (`crates/flui-engine/src/wgpu/text.rs:867,880`, `crates/flui-painting/src/text_layout/measure.rs:49-55`).

**Архитектурный потолок.** При текущем дизайне потолок упрётся в paint/composite-путь: O(N) CPU-работы на кадр при любом изменении плюс полный present, независимо от размера изменения. Для IDE-класса интерфейсов (10⁴–10⁵ узлов, постоянные мелкие обновления: курсор, подсветка, статус-бар) это фатально — мигающий курсор перерисовывает весь UI. Потолок build-пути — Flutter-parity с более тяжёлой константой (клоны, аллокации, vtable-диспетч).

**Вердикт.** Основу можно развивать: модель деревьев, генерационные ID, хранилища, invalidation-ядро, тестовая инфраструктура — здоровые. Перепроектирования требуют: (а) удержание paint/display-list между кадрами и wiring damage; (б) текстовый стек; (в) планировщик (половина его публичного API — мёртвый код); (г) ownership-модель вокруг `Arc<RwLock<…>>`. Полного redesign не требуется — требуется честная последовательность структурных фаз (см. 8.10). Ответ на главный вопрос аудита: **нет** — спроектированный сегодня под Rust фреймворк выглядел бы иначе (см. 8.9); но текущий проект находится на расстоянии миграции, а не переписывания от этой архитектуры.

---

## 8.2. Architecture map

### Crates и потоки данных

```
platform (winit/Win32/AppKit) ──events──► flui-app (AppBinding, UiRealm, runner)
     │                                        │
     │     ┌──────────────────────────────────┼───────────────────────┐
     │     ▼                                  ▼                       ▼
     │  flui-interaction                flui-scheduler          flui-view
     │  (gesture arena, focus,          (drive_frame:           (ElementTree: slab,
     │   hit-route lanes)                transient/micro/        BuildOwner: dirty-heap,
     │     │                             persistent/pipeline)    inherited, GlobalKey)
     │     │                                  │                       │ Box<dyn View>
     │     │ hit-test                         ▼                       ▼ create/update
     │     └────────────────────────►  flui-rendering ◄────────────────┘
     │                                 (RenderTree: slab Box<dyn RenderObject>,
     │                                  PipelineOwner: layout→paint→composite→semantics)
     │                                        │ LayerTree (свежий каждый кадр)
     │                                        ▼
     │                                 flui-layer (19 вариантов Layer)
     │                                        ▼
     └──────present◄── wgpu ◄── flui-engine (Renderer, DrawBatcher, GpuReplay,
                                   glyphon, lyon, атласы, пулы)
```

### Ключевые типы и владение

- `ElementTree` — `Slab<ElementNode>` + `generations: Vec<NonZeroU32>`; `ElementId = (gen<<32)|index` в `NonZeroU64` (`crates/flui-view/src/tree/element_tree.rs:357-370`). `ElementNode` ≈ 120–136 B + гарантированные 3 heap-аллокации на элемент (Box элемента, `Arc<AtomicBool>` dirty-флаг, `Arc<HashMap<TypeId, ElementId>>` inherited-карта) + `Vec` детей + Box ключа.
- `ElementKind` — `#[non_exhaustive]` enum из 11 вариантов, каждый — `Box<dyn …ElementBase>` (31 метод в vtable); 3 варианта не имеют конструкторов, все 66 каталожных render-виджетов идут через `Variable`-arity.
- `RenderTree` — slab `RenderNode` с `Box<dyn RenderObject<P>>` (~27 методов в vtable); layout ребёнка — второй dyn-hop через `&dyn Fn(RenderId, BoxConstraints) -> Size`.
- `PipelineOwner` — `Arc<RwLock<PipelineOwner>>`, встречается 71 раз в 20 файлах; выдаётся каждому элементу и через `BuildContext::pipeline_owner()` как `'static`-capability.
- `Scheduler` — thread-local синглтон с ~20 независимыми `parking_lot::Mutex` полями (`crates/flui-scheduler/src/scheduler.rs:331-409`).
- `UiRealm` — `pub(crate) !Send` владелец с bounded-256 inbox, генерационным `RealmId`, TLS-входом; максимум один на процесс (`REALM_CLAIMED`).

### Жизненный цикл UI-узла

`View::create_element()` → `ElementTree::insert` (аллокации, depth/slot, inherited-скоуп O(k)-клон если провайдер, регистрация GlobalKey, ancestor-walk для ParentData + `pipeline_owner.write()`) → `mount` → build под `BuildOwner::build_scope` (элемент вынимается из slab по значению, build под `catch_unwind` с read-only `&tree`, обратная вставка, `reconcile_children_by_id`) → `update` при пересборке родителя (клон view, безусловный re-mark dirty) → unmount: keyed → очередь `inactive_elements` (drain deepest-first в `finalize_tree`), unkeyed → немедленное удаление из slab с bump генерации.

### Жизненный цикл кадра (production, desktop)

platform wake → realm FIFO (reentrancy-gate, stale-realm rejection) → drain owner inbox (bounded 256) → dirty-gate → `Scheduler::drive_frame`: transient callbacks (пусто в проде) + microtasks + один poll `AsyncDriver` → persistent (пусто) → pipeline closure: gesture flush → `Vsync::tick_all` → build → layout (`run_layout`: dirty-очередь shallow-first, на каждый dirty-root — `SubtreeArena`: DFS O(subtree) + скан slab O(len) + HashMap + 4 Mutex) → paint (полный обход, свежий `LayerTree`) → `Scene` → `mark_full_repaint()` → `Renderer::render_scene`: clear-pass = encoder+submit #1 → рекурсивный обход слоёв → Command IR → `GpuReplay` (до 5 render-pass'ов на сегмент) → submit #2 → blocking Fifo present на UI-потоке → post-frame callbacks.

### Invalidation flow

`RebuildHandle::schedule()` → HashSet inbox → dirty-heap (переупорядочивание `rekey_dirty_depths` каждый drain, т.к. `ElementCore::depth` — это sibling-slot, а не глубина) → `build_scope` → `mark_needs_layout` (подъём до relayout boundary; boundary вычисляется без `parentUsesSize` — жёстко `true`, `crates/flui-rendering/src/protocol/box_protocol.rs:124-130`) → `mark_needs_paint` → полный paint → полный present. Semantics: любой dirty-узел → полная пересборка semantics-дерева (`crates/flui-rendering/src/pipeline/owner/semantics.rs:214-229`).

### Event flow

winit/Win32 событие → `AppBinding::handle_input` → `GestureBinding`: hit-test только на Down и некэшированном Scroll (полный обход дерева, без пространственного индекса), результат кэшируется per-pointer в DashMap; Move коалесцируются по одному на кадр; hover-Move без Down-записи **молча дропается** (`crates/flui-interaction/src/binding.rs:669`) → interaction lane: snapshot маршрутов, `catch_unwind` на цель, клон события на каждую запись. Gesture arena в проде приватная на каждый детектор (см. K4).

### Async flow

Два мира: (а) кооперативный `AsyncDriver` (poll раз в кадр, wake-коалесцирование, `TaskToken` cancel-on-drop, генерационная защита от stale-записей) — чистый, структурный, лучше Dart; (б) хардкодный tokio вне builders: `ForegroundExecutor::spawn` паникует без ambient runtime, очередь unbounded и дренируется только на Win32, `simple_block_on` — busy-spin 100% CPU; `BackgroundExecutor` (num_cpus потоков) без production-потребителя; декод изображений — один поток `BridgeRuntime`, inline, без `spawn_blocking`.

---

## 8.3. Critical findings

Формат уверенности: High = доказано кодом; Medium = сильное свидетельство, часть путей не трассирована; Low = гипотеза, требует замера.

### [K1] Paint никогда не инкрементален: полный обход, полный LayerTree, полный present на любой dirty-узел

**Severity:** Critical · **Confidence:** High (проверено вручную) · **Category:** Rendering / Performance

**Где:** `crates/flui-rendering/src/pipeline/owner/paint.rs:50-84` (полный обход; «A fresh full `LayerTree` is produced every paint pass»), `paint.rs:187-367` (параметр `dirty_set` протянут через всю рекурсию и **нигде не читается**), `crates/flui-app/src/app/binding.rs:1392` (безусловный `renderer.mark_full_repaint()`), `crates/flui-engine/src/wgpu/renderer.rs:1458` («Partial damage is currently unused»), `crates/flui-layer/src/compositor/retained.rs:20-26` (retained-композитор — пустая статистическая оболочка).

**Текущее поведение:** один `needs_paint` → полный обход дерева от корня, пересоздание `Canvas`/`DisplayList` на узел, полная пересборка slab-дерева слоёв, clear + полный рендер + present всего окна. Repaint boundaries превращаются в `OffsetLayer`, что влияет на структуру слоёв, но не на стоимость. Damage tracker (3-rect merge), scissor-путь частичного рендера и self-heal логика (~500 LOC) работают только в тестах.

**Почему это проблема:** стоимость кадра перестаёт зависеть от размера изменения. Flutter удерживает `Picture` на repaint boundary и перезаписывает только dirty-поддеревья; здесь — O(N) записи фрагментов + O(N) слоёв + полный GPU-проход на мигающий курсор. Это делает недостижимым заявленный класс приложений (IDE, редакторы, dashboard'ы с постоянными мелкими обновлениями). Мёртвый `dirty_set` — доказательство, что pruning был спроектирован, но не реализован.

**Сценарий:** редактор на 10k узлов, курсор мигает 2 Гц → 120 полных обходов/пересборок дерева слоёв/полных present'ов в минуту. Изменение цвета одного `ColoredBox` в 10k-дереве — 10k узлов работы.

**Root cause:** архитектурное решение «retention out of scope» (задокументировано в `paint.rs:50-53`) принято до того, как появились потребители; без удержания boundary-subtree и display-list retention весь нижележащий damage-конвейер бесполезен, и он был отключён одной строкой `mark_full_repaint()`.

**Решение:** (1) сделать `dirty_set` рабочим: prune обхода по dirty-поддеревьям repaint boundaries; (2) удерживать `LayerTree`/фрагменты boundary-поддеревьев между кадрами (structural sharing или per-boundary кэш с инвалидацией по dirty); (3) включить damage-rect'ы от dirty boundary до `Renderer::mark_dirty` вместо full repaint; (4) мерить: bytes записанных фрагментов/кадр.

**Альтернативы:** retained display-list per RenderObject (как Flutter `Picture`); или tile-based damage (Slint) — дешевле, но грубее для трансформов.

**Trade-offs:** сложность инвалидации retained-структур (главный источник багов у Flutter); +память на удержание слоёв; миграция внутренняя, публичный API не трогает.

**Проверка:** бенч «один dirty leaf в 10k-дереве»: ожидание — суб-линейная стоимость paint; тест: `dirty_set` фильтрует обход (счётчик посещённых узлов); GPU-readback: частичный damage оставляет неповреждённые пиксели нетронутыми.

---

### [K2] Rebuild перестраивает поддерево без identity short-circuit; 2–3 глубоких клона view на обновление

**Severity:** Critical · **Confidence:** High · **Category:** Architecture / Performance

**Где:** `crates/flui-view/src/element/id_reconcile.rs:166,258,284` (каждый совпавший ребёнок — `update`), `crates/flui-view/src/element/dispatch.rs:149-153` (`update` → `clone_box` новой view + **безусловный re-mark dirty**), `crates/flui-view/src/element/unified.rs:306` (клон старой view до успеха update), `crates/flui-view/src/element/behavior.rs:746,381,564` (клоны в reconcile), `crates/flui-view/src/view/view.rs:140-146` (`should_skip_rebuild` = `false` по умолчанию; переопределяет только `Memo`, 0 использований в `flui-widgets`/examples).

**Текущее поведение:** `set_state`-эквивалент (rebuild родителя) заново строит `Vec<Box<dyn View>>` детей, клонирует каждую view 2–3 раза и помечает каждый существующий элемент dirty — «переиспользован» и «требует rebuild» здесь одно и то же. Flutter-эквивалент `identical(old, new)` (const-виджеты как бесплатный cut-off) отсутствует структурно: views — всегда свежие значения.

**Почему это проблема:** стоимость rebuild пропорциональна поддереву, а не изменению; нагрузка — heap-аллокации и deep-клоны содержимого view (включая `String`/`Vec`/callback-поля). `Row` с 50 детьми ≈ 150 аллокаций+клонов на rebuild сверх конструирования views. Это главный CPU- и аллокационный хот-спот фреймворка.

**Сценарий:** `set_state` у корня приложения на 10k элементов (смена темы, locale) → O(10k) клонов ×2–3 + полный rebuild каждый кадр, пока идёт анимация темы.

**Root cause:** поведенческая лояльность к Flutter без переноса его оптимизационного контракта (`identical`/`const`), плюс `DynClone` как скрытая цена cloneable-config модели: clone стал невидимым и потому бесплатным в умах авторов call-site'ов.

**Решение:** (1) equality-guard: `V: PartialEq` fast-path — если `new == old`, не re-mark dirty (opt-in через derive, как `Memo`, но по значению, а не обёрткой); (2) убрать двойные клоны: reconcile должен потреблять `Box<dyn View>` по значению, а не клонировать «на всякий случай» (клон до успеха update — признак недоверия к собственному протоколу ошибок); (3) записывать rebuild reason (см. H9) — побочно сделает ложные rebuild'ы видимыми; (4) задокументировать divergence от Flutter: у нас нет const-виджетов, есть PartialEq-cutoff.

**Альтернативы:** `Arc<dyn View>` с pointer-equality cutoff (дешёвый, но связывает модель владения); fine-grained reactivity (сигналы) — отклонено контрактом C1, и этот аудит согласен: hidden-dependency graph без tooling хуже, чем element-granular rebuild с видимыми причинами.

**Trade-offs:** `PartialEq` на view с callback-полями (`Rc<dyn Fn>`) не сравним по значению → partial-derive или поле-исключения; риск «застывшего» UI при ошибочном eq — смягчается debug-счётчиком пропущенных rebuild'ов.

**Проверка:** бенч rebuild 1k/10k элементов до/после; тест: родитель возвращает eq-эквивалентного ребёнка → 0 rebuild'ов; allocation-counter на rebuild-пути (сейчас счётчиков нет вообще).

---

### [K3] Текстовый стек: двойной shaping, тройное измерение, глобальный мьютекс, игнор CTM

**Severity:** Critical · **Confidence:** High (код протрассирован; визуальный дефект DPR — Medium, нет DPR-2 теста) · **Category:** Rendering / Layout / Correctness

**Где:** измерение — `crates/flui-painting/src/text_layout/layout.rs:48` (глобальный `OnceLock<Arc<Mutex<FontSystem>>>`), `text_painter/measure.rs:49-55` (`layout()` = реальный + min + max intrinsic = **3 shaping'а**), `measure.rs:284-325` (intrinsic/dry-probes без кэша), ключ кэша только `(min_width, max_width)` с `f32::EPSILON` (`measure.rs:34-39`); отрисовка — `crates/flui-engine/src/wgpu/text.rs:584-599` (отдельный glyphon `Buffer`-стек), `text.rs:478-507,516` (кэш-ключ `(text, font_size)` без DPR), `text.rs:249-311,562` (String-фингерпринт всех run'ов **перед** lookup, каждый кадр), `text.rs:867,880` (`TextArea.scale = 1.0` жёстко), `crates/flui-engine/src/wgpu/painter/draw.rs:254,282` (трансформ применяется только к позиции).

**Текущее поведение:** каждый уникальный текст shape'ится дважды (measure + paint) в двух стеках, не делящих результатов; layout всегда считает ещё и оба intrinsic; всё это сериализовано через один процесс-wide мьютекс; ключ кэша отрисовки аллоцирует `String` на текст на кадр. Глифы растеризуются в логическом размере: при DPR=2 текст вдвое мельче, чем задумано; под поворотом/сдвигом рендерится axis-aligned.

**Почему это проблема:** текст — самый частый контент в UI профессионального класса. Здесь: 2× shaping константа на всём, 3× на измерении, невозможность параллельного/фонового shaping (Rust-преимущество не использовано), ложные кэш-промахи от float-ключа, и корректностный дефект на любом HiDPI-дисплее (основной класс desktop-устройств).

**Сценарий:** `Flex` с N текстовыми детьми при resize → N×3 shaping'а на проход под глобальным мьютексом; 500 текстов @120 Гц → 60k String-аллокаций/сек только на ключи кэша; ноутбук с DPR 2 → весь текст в 2× меньше.

**Root cause:** measure-стек и paint-стек выросли независимо и не были слиты; `FontSystem` сделан глобальным синглтоном, потому что «так проще», а не потому что это единственная модель; CTM-игнор — следствие того, что текст обрабатывается как 2D-примитив с позицией, а не как контент в device space (Flutter/Skia растеризует глифы в device space).

**Решение:** (1) единый shaping-результат (shaped runs + метрики), разделяемый measure и paint; glyphon-буфер — производный view, а не повторный shaping; (2) `layout()` без intrinsic по умолчанию; intrinsic — лениво и мемоизировано по `(width-class, dpi)`; (3) per-thread `FontSystem` (cosmic-text это позволяет) или sharded pool — shaping перестаёт быть глобальной секцией; (4) ключи кэша — precomputed u64-хэши (xxh) содержимого runs + quantized scale, без String; (5) текст в device space: scale из CTM входит в shaping key и `TextArea.scale`.

**Альтернативы:** parley (per-thread design изначально); cosmyc/parley + собственный glyph-cache; оставить cosmic-text, но шардировать — минимальная миграция.

**Trade-offs:** слияние стеков трогает flui-painting и flui-engine одновременно; per-thread FontSystem +память на поток; device-space shaping меняет семантику кэша (инвалидация при зуме — чаще).

**Проверка:** shaping-counter (ожидание: 1 shaping на уникальный (текст, стиль, ширина, scale)); DPR-2 readback-тест с эталонной растровой картинкой; бенч параллельного shaping на 2+ потоках; текст под `TransformLayer` — golden.

---

### [K4] Gesture arena не смонтирована в production: кросс-виджетная дизамбигуация отсутствует

**Severity:** Critical · **Confidence:** High (проверено вручную) · **Category:** Correctness / Architecture

**Где:** `crates/flui-widgets/src/interaction/gesture_detector.rs:456-457` (fallback `unwrap_or_else(GestureArena::new)`), то же в `draggable.rs:804-805`, `navigator/back_gesture.rs:545-546`; единственный production-монтаж scope — workaround в `crates/flui-material/src/chip.rs:995-1015`, где прямо написано: «no app shell installs a `GestureArenaScope` anywhere today». Арена биндинга (`crates/flui-interaction/src/binding.rs:406,441`) закрывается и свипится пустой.

**Текущее поведение:** каждый `GestureDetector`/`Draggable` получает собственную приватную арену. Все механизмы open/close/sweep/eager-winner/teams портированы точно, но соревноваться в них нечему: конфликты между *разными* виджетами (tap внутри scroll, drag внутри swipe-back) не разрешаются — побеждают оба recognizer'а в своих аренах.

**Почему это проблема:** арена — единственный механизм Flutter, делающий жесты предсказуемыми в композиции. Без общей арены базовые паттерны (кнопка в скролле, pull-to-refresh внутри horizontal pager) ведут себя некорректно: тап+скролл одновременно. Это поведенческая дивергенция от Flutter фундаментального уровня, замаскированная тестами (тесты монтируют scope явно).

**Сценарий:** `Button` в `ListView`: пользователь начинает скролл с пальца на кнопке → tap recognizer выигрывает свою арену, pan recognizer — свою; срабатывают оба.

**Root cause:** app-shell (flui-app/`run_app`) не устанавливает `GestureArenaScope` у корня; widget-уровень защитился fallback'ом, и fallback стал нормой. Разрыв «биндинг владеет ареной ↔ виджеты ищут её в inherited» никто не замкнул.

**Решение:** монтировать `GestureArenaScope(binding.arena().clone(), …)` в `run_app`/app-shell автоматически; убрать silent-fallback (debug-assert при отсутствии scope, как Flutter требует `GestureBinding`); chip-workaround удалить.

**Альтернативы:** implicit arena через `GestureBinding` без inherited (как Flutter) — но текущая scope-модель explicit и лучше тестируется; оставить scope, починить монтаж.

**Trade-offs:** поведение существующих демо изменится (жесты начнут конкурировать — это и есть исправление); миграция — одна строка в shell + удаление workaround'ов.

**Проверка:** интеграционный тест tap-inside-scroll: ровно один recognizer побеждает; grep-гейт: `unwrap_or_else(GestureArena::new)` = 0 вне тестов.

---

### [H1] Планировщик: lock-soup из ~20 мьютексов и ~6 мёртвых публичных API; бюджет кадра — театр

**Severity:** High · **Confidence:** High · **Category:** Architecture / Concurrency

**Где:** `crates/flui-scheduler/src/scheduler.rs:331-409` (~20 независимых `parking_lot::Mutex` полей в одном `Scheduler`); мёртвые API (0 внешних вызовов, проверено grep по `crates/`): `add_task` (:1248), `execute_idle_callbacks` (:1909), `schedule_microtask` (:1228), `add_persistent_frame_callback` (:1196), `VsyncScheduler` (конструируется только в тесте, :2041), `FrameSkipPolicy::frames_to_skip`; приоритеты: `spawn_with_priority` игнорирует приоритет (`crates/flui-platform/src/executor.rs:93-100`); бюджет: `FrameBudget` гейтует только пустую task queue, пайплайн ничем не ограничен.

**Текущее поведение:** thread-local синглтон, используемый одним потоком, но обёрнутый в Arc/DashMap/~20 мьютексов «на всякий случай». Кросс-полевые инварианты (флаг `frame_scheduled` vs очередь vs vsync-состояние) атомарно удержать невозможно — фазовые переходы рвутся на части любым внешним waker'ом. В production реальная работа `drive_frame` — это переданный pipeline-closure; приоритетные очереди, idle, skip-policy ничего не исполняют.

**Почему это проблема:** (а) ложное ощущение механизмов: инженер, настраивающий `FrameSkipPolicy`/`FrameBudget`, меняет ничто; (б) каждое пересечение полей — гонка по построению; (в) ~40% crate — поддерживаемый мёртвый код; (г) модель «всё под мьютексом» препятствует настоящей многопоточности сильнее, чем single-owner, потому что скрывает реальные точки синхронизации.

**Сценарий:** waker executor-потока вызывает `set_on_frame_scheduled` посреди фазового перехода → наблюдает половину состояния. Тонкая настройка jank через skip-policy: нулевой эффект.

**Root cause:** портировался интерфейс `SchedulerBinding`, а не его инвариант владения: во Flutter биндинг уникально принадлежит UI-потоку, фазы — последовательность `&mut`-переходов. В Rust этот инвариант выражается типом (один владелец, состояния-типсы), а не 20 мьютексами.

**Решение:** (1) удалить мёртвые API (или подключить — но честно, с потребителем); (2) переписать `Scheduler` как single-owner state machine: `enum FramePhase` + один `&mut self` на переход, без мьютексов; внешний мир общается через уже существующие bounded-каналы; (3) typestate для фаз (Idle/Transient/Frame/PostFrame) — компилятор запрещает половинчатые переходы; (4) бюджет кадра либо реально гейтует пайплайн (defer non-urgent rebuild), либо удалён.

**Альтернативы:** actor-модель (mailbox + состояние внутри) — ближе к ADR-0027, больше работы; typestate — дешевле и даёт тот же инвариант.

**Trade-offs:** ломка публичного API flui-scheduler (он молод, потребителей почти нет — сейчас дёшево, позже дорого); typestate усложняет типы ошибок компилятора умеренно.

**Проверка:** grep-гейт мёртвых API = 0; loom/тест фазовых переходов; инспекция: один мьютекс или ни одного на горячем пути кадра.

---

### [H2] Параллелизм: scaffolding есть, исполнения нет — UI, raster и present на одном потоке

**Severity:** High · **Confidence:** High · **Category:** Concurrency / Performance

**Где:** `crates/flui-app/src/app/binding.rs:1393` (`render_scene` синхронно на UI-потоке), `crates/flui-engine/src/wgpu/renderer.rs:937-958` (blocking Fifo present), `crates/flui-app/src/app/raster_owner.rs:630,804` (mailbox-поток — только `#[cfg(test)]`), `crates/flui-assets/src/bridge.rs:64-75` (`worker_threads(1)`, inline decode), `crates/flui-platform/src/executor.rs:61-66` (num_cpus pool без production-потребителя), `crates/flui-app/src/app/ui_realm.rs:467` (один realm на процесс). Production `thread::spawn`: ~8 сайтов (winit executor, Win32-диалоги, devtools). Rayon/par_iter: 0 в проде. `RasterOwner` протестирован, но не подключён.

**Текущее поведение:** один event-loop поток делает всё: build, layout, paint, raster, present (блокирующий), shaping текста (под глобальным мьютексом), poll async-задач. Фоновые потоки: 1 поток декода изображений + простаивающий num_cpus пул. Многопоточный UI-runtime, описанный ADR-0027, реализован на ~15%.

**Почему это проблема:** кадр 40 мс блокирует ввод (общий FIFO), блокирует present, и заблокированный present сдвигает начало следующего кадра — jank-спираль на ровном месте. Декод десяти 4K-изображений — строго последовательный. Пропускная способность железа не используется вообще, при том что вся freshness-машинерия (`ResultStamp`, `GenerationGate`, `RebuildHandle`) уже написана и лежит как `dead_code`.

**Сценарий:** экран с галереей изображений + скролл: декоды выстраиваются в очередь на одном потоке; длинный кадр layout задерживает обработку pointer-событий на ту же длительность.

**Root cause:** архитектура ADR-0027 (realm-акторы, lanes) спроектирована «на вырост», но скрещивающие швы (кому принадлежит кадр между UI и raster) не были проведены до конца; однопоточность стала де-факто моделью, а scaffolding — декорацией. Это опаснее честной однопоточности: типы обещают `Send`, семантика — thread-local.

**Решение (минимальный unlock-set):** (а) декод изображений → `spawn_blocking` на существующий `BackgroundExecutor` (freshness-гейты уже есть); (б) raster/present → за `RasterOwner` mailbox (протестирован): UI-поток отдаёт кадр и продолжает; (в) per-thread `FontSystem` (см. K3) — снимает последнюю глобальную секцию; (г) текст-shaping и image-decode как приоритетные задачи с версией кадра. Parallel layout независимых поддеревьев — фаза 4, после замеров, не раньше.

**Альтернативы:** render-thread модель как у браузеров (main + compositor) — это и есть (б); game-engine style (fixed simulation + render) — не подходит UI.

**Trade-offs:** raster-thread требует `Scene: Send` (сейчас `Arc` с задокументированным `!Sync`, `binding.rs:1280-1284` — надо чинить владение); latency +0..1 кадр на present; сложность отладки гонок — смягчается тем, что lanes уже типизированы.

**Проверка:** бенч: UI-поток свободен >50% кадра при raster-нагрузке; декод 10 изображений — время ≈ max, а не sum; лоом-тесты mailbox-протокола.

---

### [H3] `Arc<RwLock>` как лаундеринг владения; `'static` pipeline-capability; `Arc<dyn Any>` service locator

**Severity:** High · **Confidence:** High · **Category:** Safety / Architecture

**Где:** `crates/flui-view/src/context/build_context.rs:291-293` (`pipeline_owner() -> Option<Arc<RwLock<PipelineOwner>>>` — сохраняемый навсегда), `crates/flui-view/src/view/view.rs:471,491` (`set_pipeline_owner_any(Arc<dyn Any + Send + Sync>)` + downcast в `element_build_context.rs:494-499`), `crates/flui-view/src/context/element_build_context.rs:47-50` (tree+owner за `Arc<RwLock>`), `crates/flui-view/src/binding.rs:2241-2270` (регрессионные тесты «deadlock on reentrant observer» — уже случался), 96 токенов `RwLock` в flui-view, 71 вхождение `Arc<RwLock<PipelineOwner>>` в 20 файлах.

**Текущее поведение:** фазовое владение (build владеет tree, layout владеет render tree) выражено не типами, а разделяемыми блокировками, которые любой держатель handle'а может взять из любой фазы/потока. `parking_lot::RwLock` не реентрантен: read-внутри-write на одном потоке — deadlock. Вставка элемента берёт `pipeline_owner.write()` на каждого render-ребёнка с ParentDataView-предком (`element_tree.rs:787`).

**Почему это проблема:** инвариант «одна фаза владеет одним деревом» — центральный в этой архитектуре — не проверяем компилятором и нарушаем любым callback'ом. Уже подтверждённый класс багов (регрессионные тесты). `Arc<dyn Any>`-инъекция owner'а прячет граф владения от тип-системы — прямая противоположность контрактному стилю C1–C9.

**Сценарий:** виджет сохраняет `pipeline_owner()` в `init_state`, дёргает `.read()` из post-frame callback'а во время layout write → deadlock или (при удаче) чтение половины кадра. Re-entrant observer во время notify → deadlock (уже был).

**Root cause:** горячие объекты (`ElementTree`, `PipelineOwner`) имеют несколько логических «хозяев» в разные моменты кадра; вместо передачи владения по фазам (move/`&mut`-цепочка) выбрано совместное владение через `Arc<RwLock>` — классический признак модели владения, проигравшей borrow checker'у.

**Решение:** (1) убрать `pipeline_owner()` из `BuildContext`; кадровые потребности — через узкие capability (`LayoutQueryHandle` с версией кадра); (2) фазовое владение: `BuildOwner` берёт `&mut ElementTree` на build-фазу, `PipelineOwner` — `&mut RenderTree` на layout/paint; контексты несут только `&`-ссылки с lifetimes; (3) `set_pipeline_owner_any` удалить — типизированный проводник при конструкции binding'а; (4) notify — всегда snapshot-then-fire (уже есть как pattern, сделать единственным).

**Альтернативы:** аренная модель с reentrant-aware lock (как `recursive` мьютексы) — лечит симптом; оставить `Arc<RwLock>`, но фазовые токены на взятие — полумера, лучше чем сейчас.

**Trade-offs:** это самая инвазивная из предложенных перестроек: трогает сигнатуры контекстов (публичный API); зато переводит главный инвариант из «дисциплины» в типы. Миграция поэтапная: сначала удаление capability, потом фазы.

**Проверка:** loom/тест на re-entrant notify; статический гейт: `Arc<RwLock<PipelineOwner>>` не экспортируется из flui-view; все deadlock-регрессионные тесты продолжают проходить после упрощения.

---

### [H4] GlobalKey-reparent оставляет потомкам устаревшую глубину → нарушение порядка dirty-heap

**Severity:** High · **Confidence:** High · **Category:** Correctness

**Где:** `crates/flui-view/src/tree/element_tree.rs:1462,1573` (обновляется `depth` только корня переносимого поддерева; у Flutter `_updateDepth` рекурсивен); потребители глубины: `rekey_dirty_depths` (`build_owner.rs:447-472`), порядок `finalize_tree`, dependent-depth records.

**Текущее поведение:** перенос keyed-поддерева с глубины 2 на глубину 8 оставляет всем его потомкам `depth=3`. Dirty-heap упорядочен по глубине (shallowest-first — инвариант, который код в других местах старательно поддерживает): дети могут собраться раньше родителей.

**Почему это проблема:** сборка ребёнка до родителя нарушает контракт inherited-видимости и порядка `did_change_dependencies`; ошибка зависит от таймингов и конфигурации dirty-множества — трудновоспроизводима в проде, невидима в юнит-тестах на неглубоких деревьях.

**Сценарий:** GlobalKey-перенос панели из сайдбара в модалку (типичный «detach/attach») в кадре, где dirty и перенесённый предок, и его потомок.

**Root cause:** при портировании reparent-пути потеряна рекурсивная часть `_updateDepth`; компенсационный `rekey_dirty_depths` лечит heap, но не `ElementNode.depth` и не порядок финализации.

**Решение:** рекурсивное обновление depth по поддереву при reparent (O(subtree), редкая операция — дёшево); инвариант-тест: после любого reparent `depth(child) == depth(parent)+1` по всему поддереву.

**Альтернативы:** вычислять глубину на лету при push в heap (O(depth) на push) — убирает хранимую глубину вообще, но медленнее на горячем пути.

**Trade-offs:** ничтожные: редкая операция, локальное изменение.

**Проверка:** proptest: случайные reparent'ы + dirty-множества → порядок сборки всегда shallowest-first; инвариант глубины после каждого reparent.

---

### [H5] Утечка dependents у inherited-провайдеров

**Severity:** High (для долгоживущих сессий) · **Confidence:** High · **Category:** Correctness / Memory

**Где:** `crates/flui-view/src/element/behavior.rs:933` (`remove_dependent` — 0 production-вызовов; только тесты), `:1030-1037` (unmount провайдера чистит только его собственную карту; unmount зависимого не чистит ничего).

**Текущее поведение:** каждый элемент, вызвавший `depend_on` и затем размонтированный, остаётся в `dependents: HashMap<ElementId, usize>` провайдера навсегда. Генерационные ID предотвращают неверные rebuild'ы (stale id → `None` при drain), но не утечку: корневой `Theme` копит записи и на каждый notify итерирует мусор.

**Сценарий:** длинная сессия: ленивый список из тысяч элементов под `Theme`, скролл туда-сюда час → карта провайдера растёт монотонно; каждый переключатель темы оплачивает весь накопленный мусор.

**Root cause:** при deactivate/unmount зависимого нет обратного вызова провайдеру (Flutter это делает в `Element.deactivate` → `removeDependent`); нет и reverse-map dependent→providers, поэтому «некому» знать, откуда удалять.

**Решение:** reverse-map (dependent → SmallVec провайдеров) либо регистрация в `dep_sink` вместе с unmount-hook: при unmount элемента удалить его из всех провайдеров, записанных в его зависимостях. Дёшево: данные уже собираются при регистрации.

**Альтернативы:** ленивая чистка при notify (evict stale по generation-miss) — не чинит рост между notify; periodic compaction — костыль.

**Trade-offs:** +SmallVec на элемент с зависимостями (память); код локален в behavior.rs.

**Проверка:** тест: mount/unmount N зависимых → `dependents.len()` возвращается к исходному; бенч notify до/после при 10k исторических зависимых.

---

### [H6] Семантика: полная пересборка на любое изменение, нестабильные ID, нет OS-моста, текст невидим

**Severity:** High · **Confidence:** High · **Category:** Architecture / Correctness (accessibility)

**Где:** `crates/flui-rendering/src/pipeline/owner/semantics.rs:68-74,214-229` (один dirty → полный обход + `owner.clear()` + reinsert всего), `crates/flui-semantics/src/owner.rs:335-374` (flush пушит каждый узел в callback), `crates/flui-app/src/app/renderer_binding.rs:632-651` (`perform_semantics_action` — TODO-стаб, действия assistive-tech дропаются), ADR-0014:115 (callback не имеет потребителя; accesskit — только в документах), `crates/flui-objects/src/text/paragraph.rs` (нет `describe_semantics_configuration` — обычный `Text` отсутствует в a11y-дереве), семантика выключена по умолчанию (`renderer_binding.rs:187`).

**Текущее поведение:** подсистема — пятая проекция дерева с dirty-очередью, но сборка — full-rebuild с нестабильными ID и flush всего дерева. Платформенного моста нет; действия от screen reader'а принимаются и отбрасываются; 99% текстового контента невидим.

**Почему это проблема:** accessibility здесь — декорация, а не возможность. Для «профессионального ПО» это юридический блокер (WCAG/§508). Нестабильные ID + flush-всё сломали бы фокус и announcements любого реального моста, даже если бы он появился.

**Решение:** (1) стабильные `SemanticsId` (привязка к `RenderId`, а не к slab-вставке); (2) инкрементальный апдейт: dirty-поддерево → diff → update/insert/remove по месту; (3) accesskit-мост (стандарт де-факто в Rust: egui, Xilem, Slint); (4) `RenderParagraph` self-describe (label = текст) — Flutter-паритет; (5) включение семантики → не rebuild мира, а подписка.

**Альтернативы:** свой платформенный backend (AccessKit уже решил Win32/UIA, AppKit/AX, AT-SPI — изобретать незачем).

**Trade-offs:** инкрементальная семантика — вторая по сложности инвалидационная система в проекте; но её модель проще paint-retention (дерево плоское, без геометрии).

**Проверка:** golden-diff semantics updates (одна метка → один update); e2e с accesskit- consumer'ом: focus survives label change; действие «tap» доходит до кнопки.

---

### [H7] Тема: `ThemeData` по значению на каждое чтение; нет paint-only пути для стиля

**Severity:** High · **Confidence:** High (размер ~4–6 KB — оценка по полям, не `size_of`; помечено UNVERIFIED агентом) · **Category:** Performance / API

**Где:** `crates/flui-material/src/theme.rs:85` (`Theme::of` возвращает `ThemeData` **by value** — полный deep clone; 30 call-site'ов в 23 файлах), `:108-112` (`update_should_notify` — глубокое сравнение), `TextStyle` — 13 полей со `String` + 4 `Vec` (`crates/flui-types/src/typography/text_style.rs:222-253`). `Arc<ThemeData>` не существует. Разделение layout/paint есть только у `TextStyle::layout_affecting_eq`.

**Текущее поведение:** каждый из N виджетов-читателей клонирует мульти-килобайтовый агрегат (137 pub-полей) со всеми heap-полями на каждый rebuild; смена одной темы → rebuild всех dependents (точное множество — это хорошо) с полной стоимостью layout+paint, даже если поменялся только цвет.

**Сценарий:** toggle темы на экране с 200 читателями ≈ 1 MB скопированной памяти + тысячи heap-клонов за кадр; цветовая анимация темы — полный rebuild+layout экрана на кадр.

**Root cause:** механический перенос Flutter-идиомы «ThemeData — value object» в язык, где клон дорог и виден; paint/layout-сплит стилей (дизайн-возможность, отсутствующая во Flutter — leapfrog-зона по AGENTS.md) не реализован.

**Решение:** `Theme::of() -> Arc<ThemeData>` (или `ThemeRef` с копируемыми полями-копиями по требованию); `update_should_notify` → pointer-eq fast-path; классификация полей темы layout-affecting vs paint-only (как минимум цвета) с маршрутизацией через `Invalidation::{Layout,Paint}` — механизм уже существует в `text_painter`.

**Альтернативы:** design-tokens с индивидуальными подписками (fine-grained по полям) — точнее, но это шаг к сигналам; `Arc<ThemeData>` закрывает 80% стоимости за 5% работы.

**Trade-offs:** `Arc` в публичном сигнатуре — API-изменение; paint-only классификация требует дисциплины у авторов component-themes (неверная классификация = визуальный баг).

**Проверка:** бенч theme-toggle: аллокации и время кадра до/после; тест: смена `primaryColor` не трогает layout-фазу (счётчик perform_layout).

---

### [H8] Layout: цена soundness `SubtreeArena`, отсутствие `parentUsesSize`, однопоточность по конструкции

**Severity:** High · **Confidence:** High · **Category:** Performance / Architecture

**Где:** `crates/flui-rendering/src/pipeline/owner/subtree_arena.rs:154-195,426-444` (на каждый dirty-root: DFS + полный скан slab `get_subtree_mut` O(slab len), `storage/tree.rs:403-417` + HashMap + 4 `Mutex<Vec>`), `:726,779,820,872` (≥4 heap-аллокации на не-лист: `child_ids.to_vec()`, `child_states` Vec, `Arc<AtomicBool>`, `clone_box` parent-data), `crates/flui-rendering/src/protocol/box_protocol.rs:106-130` (`parent_uses_size` жёстко `true` — `layout_child` не принимает параметр), `subtree_arena.rs:265-280` (`check_thread` panic; layout однопоточен по построению).

**Текущее поведение:** relayout любого dirty-root'а платит O(slab len + subtree) конструкцией арены до начала собственно layout'а; на высокооборотных списках slab раздувается, и эта константа растёт. Из-за отсутствия `parentUsesSize` relayout-boundary строго меньше, чем во Flutter → листовое изменение поднимается выше → layout шире, чем нужно. Параллельный layout независимых поддеревьев невозможен: фаза держит `&mut RenderTree`, child-callbacks мутируют состояние inline.

**Почему это проблема:** самый горячий путь фреймворка несёт постоянную дань безопасности, которую можно структурировать иначе; и теряет самый дешёвый механизм Flutter по сужению инвалидации.

**Root cause:** `SubtreeArena` — честное исправление UB (Miri поймал старую схему алиасинга), но исправление выбрано в точке «снять все `&mut` заранее и раздать через raw-pointer HashMap», что и навязывает скан+аллокации. `parentUsesSize` не проложен через GAT-контексты — пробел протокола, а не концепции.

**Решение:** (1) пробросить `parent_uses_size` через `layout_child` (Flutter-паритет, сужает инвалидацию — быстрый выигрыш); (2) арена без полного скана: двухпроходная схема (собрать dirty-поддерево индексами → split-access через сортированные индексы/slab-итератор вместо HashMap+scan) или epoch-based арена, переиспользуемая между dirty-root'ами кадра; (3) убрать per-node аллокации: scratch-буферы в PipelineOwner (reuse Vec'ов между узлами/кадрами), parent-data без `clone_box` (COW или регистрация через протокол).

**Альтернативы:** per-subtree арены постоянного времени жизни (persistent arena per relayout-boundary) — ближе к ECS-мышлению, большая перестройка; petgraph-style индексные подграфы.

**Trade-offs:** split-access сложнее и требует тех же Miri-гонок; `parent_uses_size` меняет поведение существующих render-объектов (нужен аудит каталога 74 объектов — какие реально игнорируют размер ребёнка).

**Проверка:** бенч relayout leaf в 10k-дереве (ожидание: исчезновение O(slab len)); allocation counters на layout-пути; harness-тест: ребёнок OverflowBox становится boundary.

---

### [H9] Наблюдаемость близка к нулю; devtools — плацебо; документация devtools сфабрикована

**Severity:** High · **Confidence:** High · **Category:** Diagnostics

**Где:** 1,794 `tracing::`-ссылки, но 13 `span!` + ~44 `#[instrument]` на весь workspace; на rebuild/layout/paint путях — **4 span'а** (`layout.rs:52`, `paint.rs:59`, `compositing.rs:68`, `binding.rs:1310`); на пути rebuild — 0. `RebuildReason`/rebuild-reason tracking: 0 совпадений. Allocation counters: 0. `crates/flui-devtools/FEATURES.md:34-56` описывает несуществующий `src/inspector.rs` («437 lines, ✅ Complete»), несуществующие зависимости (`flui_core`) и фичи; `crates/flui-cli/src/commands/devtools.rs:33-58` печатает «DevTools server started» и блокируется, не запуская ничего; `Profiler`/`Timeline` не подключены ни к одному crate.

**Текущее поведение:** на вопросы «почему этот виджет пересобрался?», «почему кадр 28 мс?», «кто изменил состояние?» система ответить не может — нет ни причин rebuild, ни фазового тайминга, ни счётчиков аллокаций, ни подключённого потребителя телеметрии. Единственный гистограммный таймер кадра живёт в примере.

**Почему это проблема:** аудит раздела 3.21 промпта проваливается по всем 12 вопросам. Для фреймворка, чьё главное обещание — предсказуемость, отсутствие объяснимости — стратегический дефект: ни пользователи, ни сами разработчики FLUI не могут верифицировать производительность (бенчмарки, кстати, в CI компилируются, но не исполняются — регрессии невидимы).

**Root cause:** tracing добавлялся как логирование («что случилось»), а не как инструментация («сколько стоило и почему»); devtools развивался как отдельный каркас без интеграционного контракта, а FEATURES.md, по-видимому, сгенерирован вперёд факта.

**Решение:** (1) `RebuildReason` enum (SetState/ParentUpdate/DependencyChanged/AnimationTick/…) сквозь `schedule_build_for` → dirty-записи → per-element счётчики; (2) фазовые span'ы с duration на build/layout/paint/composite/present, агрегация в существующий `Timeline`; (3) allocation counters за feature-флагом (dhat/счётчики в точках: element alloc, view clone, display list); (4) удалить или реализовать CLI `flui devtools`; переписать FEATURES.md по факту; (5) CI: исполнять бенчи и хранить baseline.

**Альтернативы:** внешний профилировщик (tracy/puffin — уже есть закомментированные feature'ы) как основа timeline вместо своего.

**Trade-offs:** span'ы на горячем пути — ненулевая цена без subscriber'а (tracing near-free, но не free); counters за cfg — честно.

**Проверка:** демо: запуск примера с флагом диагностики выдаёт ответ «почему rebuild» для конкретного элемента; CI-бенч с regression gate.

---

### Сводная таблица Medium/Low-находок

| ID | Находка | Severity / Confidence | Где |
|----|---------|----------------------|-----|
| M1 | Opacity/blend-слои берут full-viewport offscreen независимо от bounds (40×40 кнопка = 3 полноэкранных прохода); тени — до 8× избыточной тесселяции одной path + 8× overdraw, в обход `path_cache`; минимум 2 `queue.submit`/кадр +1 на backdrop-filter | Medium / High | `crates/flui-engine/src/wgpu/layer_render/opacity_layer.rs:86-88`, `batches/paths.rs:120-157`, `renderer.rs:1369`, `backend.rs:457-487` |
| M2 | Постоянная layout-ошибка → бесконечный цикл полных кадров: `Err` ребёнка схлопывается в `Size::ZERO`, узел остаётся `NEEDS_LAYOUT`, `run_layout` всегда ре-маркает paint; нет счётчика retry/poison | Medium / High | `subtree_arena.rs:893-907`, `pipeline/owner/layout.rs:150,528-537` |
| M3 | flui-tree — мёртвая абстракция: `TreeRead/Nav/Write` не реализует ни одно дерево, кроме layer; `depth.rs` (1,159 LOC) без потребителей; AGENTS.md утверждает обратное; ручные generations дублируют slotmap | Medium / High | `crates/flui-tree/src/depth.rs`, отсутствие `impl Tree*` в flui-view |
| M4 | `ElementKind`: закрытый enum, где каждый вариант — `Box<dyn>`: девиртуализации нет, а тег+padding 24–32 B/узел есть; 3 варианта неконструируемы; все 66 render-виджетов — `Variable` arity; `ElementArity` — пустой маркер | Medium / High | `crates/flui-view/src/element/kind.rs:335-383`, `crates/flui-widgets/src/support.rs:11-22` |
| M5 | BuildContext: 20 обязательных методов, 0 дефолтов — толстый dyn-trait; `mounted()` всегда `true` в проде; `did_change_dependencies` не вызывается после `init_state` (дивергенция от Flutter); `ElementBuildContext` — мёртвая вторая реализация, но pub | Medium / High | `crates/flui-view/src/context/build_context.rs:50-346`, `element_build_context.rs:719-721`, `view/stateful.rs:103-115` |
| M6 | `ForegroundExecutor`: panic без ambient tokio; unbounded flume-очередь (нарушение ADR-0027); drain только на Win32; `simple_block_on` — busy-spin 100% CPU | Medium / High | `crates/flui-platform/src/executor.rs:23-36,181,215-217` |
| M7 | Hover/enter/exit/cursor мертвы в проде (Move без Down дропается; `MouseTracker::update_with_event` — 0 production-вызовов); Tab-traversal не привязан к клавише; a11y-действия дропаются в стабе | Medium / High | `crates/flui-interaction/src/binding.rs:669`, `focus.rs`, `renderer_binding.rs:632-651` |
| M8 | Нет touch/stylus/DnD/gamepad ни на одном backend'е — мобильная/планшетная история структурно отсутствует при наличии touch-recognizer'ов | Medium / High | `crates/flui-platform/src/platforms/winit/platform.rs:541-629` (нет `WindowEvent::Touch`) |
| M9 | Нет user-facing `set_state`: канон — `Rc<Cell<T>>` + `RebuildHandle` + `Option::expect("BUG")` boilerplate; мутация состояния в любой фазе не детектируется (guard только на scheduling) | Low-Medium / High | `crates/flui-view/src/view/stateful.rs`, `examples/vertical_slice_demo/tree.rs:307-320` |
| M10 | flui-platform исключён из CI по возможно мёртвому багу (документы противоречат); fuzzing отсутствует; бенчмарки в CI не исполняются; insta-снапшотов 6 | Medium / High | `ci.yml:189,375`, `docs/ROADMAP-TRACKER.md:163` vs `.rust-studio/specs/flui-completion/backlog.md:52` |
| M11 | Per-frame CPU-аллокации в рендер-хотлупе: `DrawSegment` отрастает с нуля каждый сегмент каждый кадр; `Canvas::new()` на узел; `Vec<TextArea>` на кадр | Medium / High | `crates/flui-engine/src/wgpu/batches/mod.rs:116-130`, `paint_cx.rs:283`, `text.rs:736` |
| M12 | Атлас текстур append-only (полный сброс при переполнении); glyphon-кэш вытесняет статичный текст через ~60 кадров → re-shaping; `String` клонируется на текст на кадр для ключа | Medium / Medium | `crates/flui-engine/src/wgpu/atlas.rs:195-241`, `text.rs:194-198,627-667` |
| M13 | `AnimationController` fan-out: цепочка controller→curved→proxy гоняет несколько полных `notify_listeners` (lock+snapshot+sort+re-lock) на тик; каждый implicitly-animated виджет аллоцирует целый `Scheduler` (~20 мьютексов, DashMap) | Medium / High | `crates/flui-foundation/src/notifier.rs:311-369`, `crates/flui-widgets/src/animation/implicitly_animated.rs:78` |
| M14 | `NodeLinks::depth: u16` — паника в debug/переполнение в release на глубине >65 535; редко, но без защиты | Low / Medium | `crates/flui-rendering/src/storage/links.rs:31` |
| M15 | Мёртвый `unsafe`: 3 `pub const unsafe fn …_unchecked` в `id.rs` без единого вызова; SIMD color (SSE2/NEON) без sanitizer-покрытия | Low / High | `crates/flui-foundation/src/id.rs:148,314,365`, `crates/flui-types/src/styling/color.rs:307,347,568` |
| M16 | flui-reactivity (8.5k LOC сигналов) мёртв вне workspace; `SIGNAL_RUNTIME` — процесс-глобальный DashMap (cross-realm bleed, если подключат) | Low / High | root `Cargo.toml:63-68`, `crates/flui-reactivity/src/runtime.rs:541` |
| M17 | bon — обычная зависимость flui-view, используется одним тестом; `syn = "full"` в flui-macros избыточен — время компиляции downstream | Low / High | `crates/flui-view/Cargo.toml:72`, `crates/flui-macros/Cargo.toml:24` |
| M18 | `IntoView`: документированные impl'ы для литералов не существуют; нет `Option`/`Either`-мостов → 265 `.boxed()` в виджетах; derive не поддерживает keyed-виджеты | Low / High | `crates/flui-view/src/view/into_view.rs:14-21`, `crates/flui-macros/src/lib.rs:79-96` |
| M19 | Views `!Send` по построению (84 `Rc<dyn Fn>` поля в flui-widgets): будущая многопоточная история столкнётся с публичной формой виджетов | Medium (стратегический) / High | flui-widgets, 84 поля `Rc<dyn Fn…>` |
| M20 | `Scene` — `Arc` с задокументированным `!Sync` («redesign pending») — UB-сосед при ошибочном кросс-поточном чтении; `unsafe impl Send for Renderer` независимо не обоснован | Medium / Medium | `crates/flui-app/src/app/binding.rs:1280-1284`, `crates/flui-engine/src/wgpu/renderer.rs:274` |

---

## 8.4. Performance model

N = узлов в дереве, S = поддерево под dirty-boundary, V = видимая полоса списка, k = inherited-провайдеров в скоупе, D = глубина. «Ожидаемая» — разумная цель для Rust-native дизайна, не гарантия.

| Операция | Текущая сложность | Ожидаемая | Аллокации (текущие) | Главный bottleneck | Рекомендация |
|---|---|---|---|---|---|
| Initial mount (N узлов) | O(N·(1+D·ε)) | O(N) | ≥3–5 на элемент + view | 3 гарант. heap-аллокации/элемент (Box, Arc<AtomicBool>, Arc<HashMap>) | пулы элементов по view-типу; inherited-карта без Arc на узел (side-table только у провайдеров) |
| Rebuild одного листа | O(1) + build листа | O(1) | 2–3 клона view | безусловный `clone_box` + re-mark dirty | equality-guard, потребление Box по значению |
| Rebuild родителя с C детьми | O(C) updates + O(subtree) rebuilds | O(C) сравнений + O(изменённые) | 2–3C клона + ≥4 temp-коллекции на родителя | отсутствие identity/eq short-circuit; `updateChildren`-temp'ы | eq-cutoff; переиспользуемые scratch-буферы reconcile |
| Вставка в начало списка (unkeyed) | O(C) updates (все слоты сдвигаются) + O(C) rebuilds | O(C) сравнений, O(1) структурно | O(C) клонов | позиционное сопоставление + re-mark dirty | keyed-by-default для списков; eq-cutoff спасает rebuild |
| Reorder keyed-списка | O(C) hash-операций; reparent O(subtree)+stale depth | O(C) | O(C) hashmap + клоны | hash-index построение на reconcile | scratch HashMap переиспользуемый; починить depth (H4) |
| Скролл длинного списка | layout O(V) + арена O(attached) + **paint O(N)** + full present | O(V) + O(damage) | O(attached) арена + O(N) фрагменты | полный paint и present (K1) | retained boundaries + damage |
| Resize окна | O(N) layout + O(N) paint + full present | O(N) layout, O(N) paint (честно) | O(N) | как у Flutter, но + арена-константа (H8) | scratch-буферы; damage не поможет (всё dirty) |
| Смена глобальной темы | O(dependents) rebuilds + O(N) paint; ThemeData-клон на читателя | O(dependents) + paint-only где возможно | Arc<ThemeData> вместо клонов | by-value тема (H7) + K1 | Arc темы + layout/paint сплит |
| Анимация transform/opacity | paint-only возможен (`RenderAnimatedOpacity`), но transitions rebuild'ят поддерево на кадр; paint всё равно O(N) | O(1) CPU + GPU-дривен | 0 в идеале | rebuild-паттерн transitions + K1 | proxy-анимации как единственный паттерн; layer-transform канал |
| Анимация layout | O(S) layout + O(N) paint на кадр | O(S) + O(damage) | арена + фрагменты | K1 + H8 | damage; арена-скретч |
| Ввод текста (редактор) | 3 shaping на layout + 2× стек + глобальный мьютекс + String-ключи | 1 shaping на изменённый абзац | String-фингерпринт/кадр | K3 | единый стек, per-thread FontSystem, хэш-ключи |
| Открытие overlay | O(N) paint + full present | O(overlay-subtree) + composite | слои заново | K1 | retained layers |
| Удаление большого поддерева | O(n) eager или inactive-drain; dependents не чистятся | O(n) | snapshot Vec | утечка dependents (H5) | reverse-map; drain честный |
| Pointer move (после Down) | ~O(1) (кэшированный маршрут) + клон события на запись | O(1) | 1–2 клона события | `PointerEvent` 152 B клон на entry | разделяемый Arc-событие или COW |
| Scroll-события (некэшир.) | hit-test O(N) + resolve/release маршрута **на каждый тик** | O(1) амортизир. | HashMap+Vec churn @60–120 Гц | ephemeral route per tick (риск, не замерено) | кэшировать маршрут скролла за кадр |
| Hit-test (Down) | O(N) рекурсия + `ensure_stack` на уровень | O(N) худший; spatial index — опция | HitTestResult + transforms | полный обход; нет пропуска clean-subtree | skip статичных поддеревьев по флагу; spatial index позже |

**Чего не хватает для точной модели (измерить до оптимизаций):** bytes/element (size_of инвентаризация), allocs/rebuild, allocs/frame, длительность фаз кадра, hit-rate кэшей (текст/тесселяция/текстуры), память после удаления поддерева, p50/p95/p99 кадра. Существующие 52 бенча покрывают микро-пути (layout фигур ≤1k узлов, виртуализатор, жесты), но ни один не меряет rebuild-at-scale, полный кадр, скролл, hit-test, текст; CI бенчи не исполняет.

---

## 8.5. Ownership map

| Ресурс | Владелец (фактический) | Тип ссылки | Время жизни | Потенциальная проблема |
|---|---|---|---|---|
| `ElementNode` | slab в `ElementTree` | `ElementId` (генерац.) | до unmount + finalize | дерево за `Arc<RwLock>` в контекстах (H3) |
| `View` (конфиг) | элемент (поле `ElementCore`) | `Box<dyn View>` + dyn-clone | до замены update'ом | 2–3 клона на update (K2) |
| `ViewState` | `StatefulBehavior` внутри элемента | inline в Box | до `dispose` при unmount | выживает reparent корректно; мутации без фазовой защиты (M9) |
| `RenderNode` | slab в `RenderTree` | `RenderId` (генерац.) | до remove | доступ через `*mut` в `SubtreeArena` (обоснованный unsafe) |
| `PipelineOwner` | binding; разделяется со всеми | `Arc<RwLock>` ×71 | процесс | любой может залочить из любой фазы (H3) |
| `LayerTree` | никто (пересоздаётся) | значение | 1 кадр | K1: нет retention |
| `Scene` | binding → renderer | `Arc` (признан `!Sync`) | до render | M20: UB-сосед |
| `Renderer` | 3 сайта | `Arc<Mutex<Renderer>>` + `unsafe impl Send` | процесс | скрытая thread affinity; блокирует UI (H2) |
| `FontSystem` | глобальный синглтон | `OnceLock<Arc<Mutex>>` | процесс | сериализация shaping (K3) |
| GPU-буферы/текстуры | пулы (pow2-bucket 64 MB, 16 idle textures) | RAII + frame-counter eviction | кадры | exact-descriptor match → churn при анимации фильтров (риск) |
| Listeners/callbacks | `ChangeNotifier` | `Arc<Mutex<HashMap<Id, Arc<dyn Fn>>>>` | до unsubscribe | 114 `catch_unwind` — защита вместо ясности владения |
| Focus/Mouse/TextInput | TLS `Box::leak` синглтоны | thread-local ref | процесс | hook-stealing при втором binding на потоке (документировано) |
| `Scheduler` | TLS синглтон | thread-local + ~20 внутр. мьютексов | процесс | lock-soup (H1); per-widget экземпляры (M13) |
| Gesture arena | никто в проде (приватная на детектор) | `Rc` | жест | K4 |
| Ассеты/изображения | `BridgeRuntime` + кэши (moka TTL, lru) | Arc | TTL/TTI | 1 поток декода (H2) |
| Роуты указателей | interaction lane | `Rc<HandlerCell>` + `Weak` | последовательность указателя | !Send по дизайну (ADR-0027) — осознанно |
| Signal runtime (dormant) | процесс-глобальный | `Lazy<DashMap>` | процесс | cross-realm bleed при подключении (M16) |

Вывод карты: модель «сlab владеет, ID ссылается» — здоровая в трёх деревьях; вся патология сосредоточена на швах: `Arc<RwLock>` вокруг owner'ов, TLS-синглтоны, глобальный FontSystem. Это поддаётся точечной хирургии, а не переписыванию.

---

## 8.6. Invalidation matrix

«Нужно» — что требует семантика изменения; «Фактически» — что выполняет текущий пайплайн. ✗ = лишняя работа.

| Изменение | Build | Layout | Paint | Composite | Semantics |
|---|---|---|---|---|---|
| set_state на листе | нужно: лист · факт: лист | нужно: до boundary · факт: до boundary (шире из-за `parentUsesSize` ✗) | нужно: узел · факт: **всё дерево** ✗ | нужно: damage · факт: **всё окно** ✗ | нужно: узел · факт: **всё дерево** ✗ (если включена) |
| rebuild родителя (новые props) | нужно: родитель+изменённые · факт: **всё поддерево** ✗ (K2) | как выше ✗ | всё дерево ✗ | всё окно ✗ | всё дерево ✗ |
| paint-only свойство (цвет) | не нужно · факт: не запускается ✓ | не нужно ✓ | нужно: узел · факт: всё дерево ✗ | всё окно ✗ | не нужно ✓ |
| opacity-анимация (proxy) | не нужно ✓ (proxy-путь) | не нужно ✓ | нужно: узел · факт: всё дерево ✗ | всё окно ✗ | не нужно ✓ |
| transition-анимация (FadeTransition) | не нужно · факт: **rebuild на кадр** ✗ | иногда | всё дерево ✗ | всё окно ✗ | не нужно ✓ |
| смена темы (только цвета) | нужно: dependents · факт: dependents ✓ | нужно: не всем · факт: всем dependents ✗ (нет layout/paint сплита) | всё дерево ✗ | всё окно ✗ | контраст/labels — частично |
| сдвиг скролла | нужно: viewport-контент | нужно: viewport-контент ✓ (ленивый) | нужно: видимая полоса · факт: всё дерево ✗ | всё окно ✗ | видимая полоса · факт: всё |
| правка текста | нужно: узел | нужно: узел (3× shaping ✗) | всё дерево ✗ | всё окно ✗ | узел · факт: всё |
| hover | ничего · факт: ничего (hover мёртв, M7) | — | — | — | — |
| resize окна | корневые dependents | всё (честно) | всё (честно) | всё (честно) | всё |

Итог: Build-инвалидация избыточна из-за K2; Layout — слегка избыточен (parentUsesSize); Paint/Composite/Semantics — **всегда глобальны** независимо от изменения. Главный недостающий механизм — гранулярность paint/composite; он же самый влияющий на UX.

---

## 8.7. Unsafe audit

Всего: 36 `unsafe fn`, 257 `unsafe {}` блоков, 46 `unsafe impl`. ~60% токенов — platform FFI (Win32/AppKit).

| Блок | Invariant | Кто поддерживает | Документирован? | Нарушим из safe API? | Альтернатива | Локализован? | Miri/sanitizer |
|---|---|---|---|---|---|---|---|
| `pipeline::owner::subtree_arena` (~15 блоков, `NodePtr(*mut RenderNode)` + `unsafe impl Send/Sync`) | N непересекающихся `&mut RenderNode` reborrow'ятся по слотам, по одному на уровень рекурсии | `collect_disjoint_mut` (дедуп), `LayoutCycleGuard`, `check_thread()`, `PhantomData<&'tree mut ()>` | да (модульные доки) | нарушение → паника, не silent UB (guard'ы) | split-access по сортированным индексам; persistent per-boundary арены | да, один модуль | **Miri: да** (`just miri`, CI advisory) |
| flui-platform FFI (247 токенов: Win32 window 61, AppKit 60) | валидность HWND/NSView, поток UI-вызовов | platform shim'ы | частично | потенциально (кросс-поточный вызов) | winit как единственный слой (уже fallback) | по backend-файлам | нет (Windows-only heap-corruption открыт, M10) |
| `flui-foundation/src/id.rs:148,314,365` — 3 `pub const unsafe fn …_unchecked` | caller гарантирует валидность packed id | никто — **0 вызовов** | комментарий признаёт dead | да, но вызывать некому | **удалить** | да | нет (не нужен — удалить) |
| `flui-types/src/styling/color.rs:307,347,568` (SSE2/NEON, feature `simd`) | корректность intrinsics по спецификации | автор intrinsics | да | нет (чистая математика) | `std::simd` когда стабилизируется; auto-vectorization | да | **нет** — добавить тесты под `target_feature` + Miri-interpreter |
| `unsafe impl Send for Renderer` (`engine/src/wgpu/renderer.rs:274`) | все захваченные хендлы Send (wgpu-хендлы Send) | владелец Renderer | комментарий | при смене полей — неявно | убрать причины (wgpu 29 хендлы Send/Sync — проверить, вероятно impl избыточен) | точечно | нет |
| `flui-hot-reload`: dlopen/dlsym + `unsafe impl Send for DynLib`, `BuildPtr` | символы валидны пока lib загружена; отсутствие гонок reload | host-протокол | частично | да (unload при живых указателях) | протокол с pin-версиями; `safer-ffi` | модуль dynlib/worker | нет |
| `flui-layer/src/tree/layer_tree.rs:981+` | — | — | — | — | — | `#[cfg(test)]` only | тесты |

Оценка: unsafe-дисциплина выше средней по экосистеме (Miri на главном хот-споте, guards, thread-checks). Просроченные зоны: platform FFI без sanitizer-прогона (Windows heap-corruption не закрыт), SIMD без покрытия, мёртвый unsafe API. Никакого unsafe, «прячущего неудачную модель владения», за пределами subtree_arena не обнаружено; сама арена — осознанная цена (см. H8).

---

## 8.8. API assessment

| Критерий | Оценка /10 | Обоснование |
|---|---:|---|
| Clarity | 6 | Facade и prelude — чистые; но `IntoView`-доки описывают несуществующие impl'ы, ложные причины имён макросов, AGENTS.md/FEATURES.md местами расходятся с кодом |
| Ergonomics | 5 | Минимальный виджет ~10 строк (derive) — хорошо; stateful — 3 impl-блока; нет `set_state` (Rc<Cell>+handle boilerplate); keyed ломает derive; `Option`/conditional — ручной `.boxed()`; 265 `.boxed()` в собственных виджетах — симптом |
| Composability | 6 | Композиция view'ов естественна; но conditional/dynamic arms неоднородны, arity-система не доведена (всё Variable), gestures требуют ручного scope (который никто не монтирует) |
| Type safety | 7 | Генерационные ID, NonZero, типизированные constraints, unit-типы (`px`) — сильно; минусы: `Arc<dyn Any>`-инъекция owner'а, `TypeId`-ключи inherited, `mounted()` врёт |
| Diagnostics | 2 | См. H9: причин rebuild нет, фазового тайминга нет, devtools — плацебо, часть crate-доков сфабрикована |
| Stability | 5 | Необратимо экспонировано: RPITIT в `build` (capture-set — breaking), `parking_lot::RwLock`+`Arc` в сигнатуре `BuildContext`, `DynClone`-supertrait, 1-based ID. Хорошо: `#[non_exhaustive] ElementKind` |
| Discoverability | 6 | Prelude курирован, коллизии задокументированы; но 3 пустых variants ElementKind, мёртвые API scheduler'а, `EventRouter` (публичный, но тест-only) дезориентируют |
| Compile time | 4 | Структурно: per-shape `Element<V,…>` мономорфизация, RPITIT повсюду, tuple-генерики до 16, `bon` в deps ради одного теста, `syn="full"`; величина не замерена (нет `--timings` бюджета в CI) — направление определённо плохое |
| Extensibility | 6 | Сторонний виджет реализуем (derive + 2 trait'а), trait'ы не sealed; но custom render object требует понимания GAT-контекстов и протоколов; каталожный harness-гейт — хороший прецедент |
| Testability | 8 | Лучшая сторона: `HeadlessBinding::pump_frame` с ManualClock, render-harness на реальном PipelineOwner без GPU/окна, каталожный guard, WARP readback-suite, детерминированные анимации. Минусы: 6 insta-снапшотов, нет fuzz |

**Итог: 5.5/10.** API-ядро (derive, prelude, типы) — крепкое; провалы — диагностика, compile-time риски и нечестные/мёртвые поверхности.

---

## 8.9. Recommended target architecture

Ответ на главный вопрос: фреймворк, спроектированный сегодня под Rust без оглядки на Flutter, сохранил бы у FLUI протокол constraints→size, slivers, генерационные ID и headless-тестирование — и отличался бы в десяти точках. Ниже целевая архитектура с границами ответственности. Что взять у других систем — с обоснованием по пяти пунктам (проблема / пригодность для Rust / что не копировать / что адаптировать / вердикт).

### Слои и направление зависимостей

```
app ──► widgets ──► view (element tree) ──► rendering (render tree)
                         │                        │
                         ▼                        ▼
                    foundation ◄────────── pipeline (owner, scheduler)
                         │                        │
                         └────► layer (display list, retained) ──► engine (wgpu backend)
                                      ▲
                                semantics (инкрементальная проекция)
```

Зависимости строго вниз; pipeline знает о деревьях, деревья не знают о pipeline (сейчас элементы тащат `Arc<RwLock<PipelineOwner>>` — перевернуть).

### 1. Storage model — сохранить slab, добавить проекции

Три дерева остаются (разделение конфигурации/жизненного цикла/геометрии — здравая граница, подтверждённая и Flutter, и браузерными движками), но: элемент хранится как compact header (parent, first-child/next-sibling или slot-range, flags-bitset, depth u16→u32) + side-tables для редких полей (key, global-key hash, dependents, inherited-провайдерская карта). `Arc<HashMap>` inherited на каждый узел уходит: карта живёт только у провайдеров, lookup — подъём с кэшем последнего результата (как Flutter `InheritedElement._inheritedWidgets`, но ленивый). Цель: 1 аллокация на элемент (его Box) вместо 3–5.

*У Bevy ECS:* проблема — cache-local обход больших структур. Не копировать: archetype-миграции и query-DSL — у UI-дерева форма важнее набора компонентов. Адаптировать: dense-хранение и SoA-мышление для горячих полей (dirty-битсет вместо `Arc<AtomicBool>` на узел). Вердикт: component-tables для горячих данных, дерево — для топологии.

### 2. Node identity — оставить как есть

Генерационные ID — лучшее решение проекта; расширить на `SemanticsId` (стабильность через привязку к `RenderId`, H6).

### 3. State model — element-granular + eq-cutoff + видимые причины

Без сигналов (согласие с C1, но по аргументам, а не по догме): fine-grained graph без tooling создаёт hidden-dependency debugging, хуже чем явный rebuild. Модель: `set_state(fn(&mut S))` на `ViewState` (метод существует внутри — сделать публичным через handle), eq-guard на view update (PartialEq fast-path), `RebuildReason` сквозь весь путь. Мутации состояния — через один метод, чтобы фазовый guard покрывал и мутацию, а не только scheduling.

*У SolidJS/Leptos:* проблема — O(изменение) обновления. Не копировать: runtime-граф зависимостей с автотрекингом (скрытые зависимости, утечки подписок). Адаптировать: ничего в ядро; точечно — proxy-анимации уже делают ту же работу руками. Вердикт: element-granularity + eq-cutoff даёт 80% выигрыша signals за 10% сложности.

### 4. Dependency model — типизированная среда вместо TypeId-locator

`BuildContext` сократить до struct-of-capabilities: identity, `depend_on<T>()` (typed, O(1)), schedule, query API. Убрать `pipeline_owner()`, `Arc<dyn Any>`-инъекцию. Зависимости узла — enumerable (reverse-map, H5) — это одновременно чинит утечку и даёт dependency inspector.

*У SwiftUI Environment:* проблема — типизированное окружение вниз по дереву. Не копировать: магию property wrappers. Адаптировать: typed key-path доступ с compile-time известным типом значения (уже почти есть через `BuildContextExt`). Вердикт: текущий `depend_on` + reverse-map + удаление небезопасных capability достаточно.

### 5. Reconciliation — оставить алгоритм, убрать налог

`updateChildren`-порт корректен (O(n+m) avg); добавить: eq-cutoff на update, scratch-буферы (zero-alloc reconcile), удаление лишних клонов (K2), починка depth при reparent (H4).

### 6. Layout — довести протокол до Flutter, потом превзойти

`parent_uses_size` через `layout_child` (сужение инвалидации); арена без полного скана slab и без per-node аллокаций (scratch, split-access); poison-путь вместо бесконечного retry (M2); intrinsic-кэш — сохранить (он корректен); текст-измерение — единый стек (K3). Параллельный layout независимых поддеревьев — только после фазовой чистки владения и замеров (форк-джойн по relayout-boundary; ожидаемый выигрыш — resize/initial-mount больших деревьев, не стоит sync-cost для мелких).

### 7. Rendering — retained, damage-driven, backend-тонкий

Ключевое отличие целевой архитектуры от текущей: **display list и layer tree — retained структуры с инкрементальной записью**. Repaint boundary владеет своим фрагментом; dirty узел перезаписывает только свой boundary; layer tree — persistent (structural sharing) или патчится; damage = union перезаписанных boundary-rect'ов → `mark_dirty` вместо `mark_full_repaint`; opacity-слои — bounds-sized; тени — один tessellated mesh с blur в шейдере. Текст: shaped runs разделяются measure/paint, per-thread FontSystem, device-space растеризация.

*У браузерных движков (Blink/WebRender):* проблема — минимальная работа на кадр при сложных сценах. Не копировать: полную retained-mode сложность (display item invalidation в Blink — годы багов). Адаптировать: boundary-granular retention (грубее Blink, мелче Flutter — Flutter тот же принцип, у них Picture на boundary). Вердикт: это и есть Flutter-модель, которую проект объявил, но не построил.
*У Slint:* damage tracker 3-rect — уже взят (flui-layer/damage.rs), осталось подключить.

### 8. Event routing — общая арена + живой hover

`GestureArenaScope` в app-shell (K4); hover как первоклассный путь (hit-test на enter, кэш hover-маршрута); Tab-traversal привязать; a11y-действия до handler'ов. Spatial index — отложить (O(N) hit-test приемлем до 10⁵ узлов; профилировать сначала).

### 9. Async model — один мир

`AsyncDriver` (scoped, cancel-on-unmount, generation-gated) — единственный способ интеграции async в UI; tokio — только за platform-швом (assets, сеть) через `spawn_blocking` + версионированные результаты; `ForegroundExecutor` удалить или переписать (bounded, все платформы drain'ят, без busy-spin). Structured concurrency: scope виджета владеет задачами; unmount отменяет.

*У GPUI:* проблема — async в UI без хаоса. Адаптировать: их модель `AppContext`+foreground executor близка к AsyncDriver; GPUI подтверждает, что single-executor-per-window с версионированием работает на IDE-классе (Zed). Не копировать: их глобальный `App` синглтон.

### 10. Multithreading — три честных потока, потом опционально больше

UI-поток (build/layout/paint-record) → raster-поток (replay+submit+present через `RasterOwner`) → worker-пул (декод, shaping, assets) с версиями кадра и отменой. Это не «многопоточность ради галочки»: измеримая цель — UI-поток свободен >50% кадра, input latency < 8 мс при нагрузке. Parallel layout/reconciliation — фаза 4, по результатам замеров.

### 11. Platform abstraction — узкие capability-traits вместо God-trait

Текущее разделение (windowing/input/clipboard/text-input по отдельным трейтам) — в основном здоровое; держать правило «один trait = одна возможность»; touch/stylus/DnD/gamepad — расширить input-шов (M8), web/embedded — через тот же `Renderer`-шов (CommandRenderer уже wgpu-free по сигнатуре; второй backend — реалистичен после выноса 19 LayerRender в backend-индифферентный слой).

### 12. Diagnostics — first-class, не опция

`RebuildReason`, фазовые span'ы с duration, allocation counters (feature-gated), frame timeline → devtools-сервер реальный или команда удалена; p50/p95/p99 кадра в демо; «почему rebuild» — отвечаемый вопрос из инспектора. CI исполняет бенчи с baseline-гейтом.

---

## 8.10. Migration plan

### Phase 0 — Instrumentation (1–2 недели, без изменения поведения)

Измерить до любых изменений: size_of-инвентаризация узлов; allocation counters на rebuild/layout/paint; фазовые таймеры кадра в runner; бенчи: rebuild 1k/10k, полный кадр через HeadlessBinding, скролл списка, hit-test, текст; CI исполняет бенчи (baseline без гейта). RebuildReason-scaffolding (enum, протянуть, пока без UI).
**Зависимости:** нет. **Риски:** низкие. **Критерий выхода:** дашборд «что стоит кадр» воспроизводимо; все дальнейшие фазы меряются от этой базы. **Тесты:** бенчи + smoke.

### Phase 1 — Safe local improvements (2–4 недели, публичный API не ломается)

- Починки корректности: depth при GlobalKey-reparent (H4), dependents-leak (H5), layout retry poison (M2), gesture arena в app-shell (K4), hover/Tab/a11y-actions (M7), `did_change_dependencies` parity (M5).
- Мёртвый код: удалить `id.rs` unchecked-функции, `EventRouter` или пометить экспериментальным, devtools FEATURES.md по факту, CLI-команда честная, flui-platform CI — решить (re-enable по backlog-данным или закрыть расследованием).
- Дешёвая производительность: scratch-буферы reconcile/layout, удаление двойных клонов update (K2 частично), String-ключи текстового кэша → хэши (K3 частично), opacity bounds-sizing (M1), один submit на кадр без backdrop (M1), `Scheduler` per-widget → shared ticker (M13), bon → dev-dep (M17).
- Диагностика: span'ы фаз, RebuildReason в действии.
**Критерий:** все correctness-находки закрыты тестами, которые без фикса падают; бенчи показывают −30%+ аллокаций на rebuild/scroll путях. **Риски:** поведение жестов изменится (это и есть фикс) — прогон демо.

### Phase 2 — Structural refactoring (1–2 месяца, внутренние модели)

- Retained paint: boundary-fragment retention, рабочий `dirty_set`-pruning, layer-tree persistence/patching, damage до рендерера (K1) — самый большой и самый ценный этап.
- Текст: слияние measure/paint стеков, per-thread FontSystem, device-space shaping, DPR-fix (K3).
- Арена без скана и per-node аллокаций; `parent_uses_size` (H8).
- Семантика: стабильные ID + инкрементальные апдейты (H6, часть 1).
- Scheduler как single-owner state machine; удаление мёртвых API (H1).
**Зависимости:** Phase 0 (база замеров), Phase 1. **Риски:** retained paint — классический рассадник invalidation-багов; митигация — harness + golden на каждый класс dirty-сценария, Miri на новой арене. **Критерий:** paint стоимость ∝ dirty-области (бенч dirty-leaf в 10k); текст — 1 shaping на уникальный ключ; DPR-2 readback green; scheduler без мьютексов на горячем пути.

### Phase 3 — API redesign (2–4 недели, ломка публичного API один раз)

- `BuildContext` → struct-of-capabilities; удаление `pipeline_owner()`, `Arc<dyn Any>`-инъекции (H3); `mounted()` честный или удалён.
- `set_state` публичный с фазовым guard (M9); eq-guard для view updates (K2) с derive.
- `Theme::of -> Arc<ThemeData>` + paint/layout классификация полей темы (H7).
- Удаление `Arc<RwLock>` из публичных сигнатур; `ElementBuildContext` — удалить.
- Финализация arity: довести leaf/single/optional или удалить варианты (M4); `IntoView` для Option/Either/литералов (M18).
**Зависимости:** Phase 1 (поведение стабилизировано). **Риски:** единственный момент, когда ломка дешевле, чем позже — версия 0.x; собрать все ломки в один релиз. **Критерий:** semver-совместимый далее план; миграционный гайд; демо портированы.

### Phase 4 — Parallel and incremental runtime (1–2 месяца)

- `RasterOwner` в проде: raster/present вне UI-потока; `Scene: Send` по-настоящему (M20); bounded lanes честно везде.
- Декод/shaping/assets на worker-пуле с версиями кадра (H2).
- Views `Send` там, где возможно: аудит 84 `Rc<dyn Fn>` полей → `Arc` за обоснованием или документированный owner-plane (M19).
- Только после замеров: parallel layout независимых поддеревьев; spatial index для hit-test — если профиль скажет.
- accesskit-мост (H6, часть 2); touch/stylus/DnD platform-швы (M8).
**Зависимости:** Phase 2 (ownership чистый — иначе потоки унаследуют хаос), Phase 3. **Риски:** гонки — митигация loom/каналы уже типизированы; прирост latency present — измерить. **Критерий:** UI-поток свободен >50% кадра под нагрузкой; input latency <8 мс; a11y e2e через accesskit.

### Phase 5 — Stabilization (непрерывно → релиз)

Бенч-гейт регрессий в CI; fuzz (hit-test, reconcile, текст-парсеры); proptest на инварианты (depth, shallowest-first, generations); Miri на всём flui-rendering (не только арене); sanitizer-прогон platform FFI (закрыть Windows-вопрос); документация по факту (удалить/чинить AGENTS.md-расхождения); kriterии релиза: p99 кадра < 16 мс на reference-сцене 10k узлов при 1 dirty/кадр, память/узел ≤ целевого бюджета, ноль deadlock-регрессий, API frozen.

---

## 11. Обязательные итоговые выводы

**1. Улучшение Flutter или перенос?**
Перенос с точечными улучшениями. Перенесены: модель деревьев, протокол layout, gesture arena, slivers, inherited-зависимости — местами с более чистой реализацией (генерационные ID, отменяемые async-задачи, структурный unsafe). Не перенесены оптимизации, делающие модель жизнеспособной (retained paint, identity short-circuit, `parentUsesSize`), и не использованы преимущества Rust (параллелизм, per-thread текст, владение через типы). По экономике кадра сегодня это **хуже Flutter** при лучшей безопасности типов.

**2. Какие ограничения Flutter скопированы случайно?**
Полный repaint окна (у Flutter-то он ограничен boundary-retention — здесь нет); rebuild поддерева без identity-cutoff (в Flutter спасают `const`-виджеты — здесь нечем); отсутствие `parentUsesSize` в boundary-логике; `ThemeData` как value-object (в Dart копия дёшева из-за GC-семантики ссылок — в Rust это deep clone мульти-KB структуры); однопоточный UI как де-факто модель (в Dart — вынужденная, здесь — нет); публичный доступ к pipeline owner'у через context (аналогично `context.findRenderObject()`, но с lock'ами); ThemeData deep-compare notify.

**3. Какие возможности Rust не используются?**
Безопасный параллелизм (0 rayon в проде, 1 поток декода, num_cpus-пул простаивает); владение через типы вместо `Arc<RwLock>` (H3); typestate для фаз кадра; per-thread ресурсы (FontSystem); `Send`/`Sync` как инструмент дизайна (частично — «костюм» на thread-local структурах); scratch/arena-аллокаторы на горячих путях (наоборот — zero-capacity regrow каждый кадр); компактные битфлаги вместо `Arc<AtomicBool>`; `PartialEq`-cutoff; сигнатурная честность (`Scene: Send`).

**4. Компонент с наивысшим архитектурным риском?**
Paint/composite-конвейер (K1): он определяет стоимость каждого кадра, его retained-переделка — самая сложная работа в проекте, и она не начата, при том что мёртвый `dirty_set` и damage-машинерия создают иллюзию готовности. Второй — текстовый стек (K3): корректностный дефект на HiDPI + 2–3× константа на самом частом контенте.

**5. Какой компонент первым упрётся в performance ceiling?**
Полный обход paint + пересоздание `LayerTree` + full present. При 10⁴+ узлах и частых мелких обновлениях (IDE-класс) это потолок на уровне «непригодно», не зависящий от оптимизации микропутей. Следом — rebuild-константа из клонов view (K2).

**6. Какие три решения нельзя стабилизировать в текущем виде?**
(a) `BuildContext` с `pipeline_owner()` и `Arc<RwLock>` в сигнатурах — после стабилизации H3 будет неснимаемым; (b) публичный планировщик с мёртвыми API и lock-soup (H1) — застабилизирует театр; (c) текстовый split-brain measure/paint (K3) — после стабилизации `TextPainter` слияние стеков станет breaking.

**7. Какие пять изменений дадут максимальный эффект?**
1. Retained paint + damage (K1) — переводит стоимость кадра с O(N) на O(dirty). 2. Eq-cutoff + удаление лишних клонов view (K2) — главный CPU/аллокационный выигрыш build-фазы. 3. Единый текстовый стек + per-thread FontSystem + device-space (K3) — корректность на HiDPI + снятие 2–3× константы. 4. Gesture arena в app-shell (K4) — одна строка, чинит фундаментальную дивергенцию. 5. RebuildReason + фазовые таймеры + исполняемые бенчи (H9/Phase 0) — делает все остальные улучшения проверяемыми и необратимыми.

**8. Что сохранить без изменений?**
Генерационные ID (id.rs); slab-модель трёх деревьев; constraints-протокол и мемоизация intrinsic (корректна); sliver-виртуализацию (sumtree Virtualizer); gesture-арену как алгоритм; `HeadlessBinding`/render-harness/parity-corpus; точные dependent-множества inherited; `TaskToken`+generation-gates async; дисциплину unsafe в subtree_arena (сама модель — менять, дисциплину — нет); курированный facade/prelude; политику зависимостей (deny.toml, pinned wgpu).

**9. Что удалить?**
Мёртвые API планировщика (6 шт.); `ElementBuildContext` (мёртвая pub-реализация); `id.rs` unchecked-trio; `EventRouter` (или явно эксперимент); flui-tree `depth.rs` и TreeRead/Nav/Write, если нет потребителей к 1.0; retained.rs-пустышку в flui-layer (слить с реальным retention); фейковую CLI `flui devtools`; `ForegroundExecutor` (или переписать — но не оставлять); per-widget `Scheduler` в implicit-анимациях; `bon` из deps flui-view; FEATURES.md как жанр (доки по факту).

**10. Что перепроектировать до стабильной версии?**
Paint-retention и damage (K1); текстовый стек (K3); `BuildContext` и владение owner'ами (H3); планировщик (H1); семантику до инкрементальной + accesskit (H6); `ThemeData` sharing (H7). Всё остальное — локальные фиксы.

**11. Поддержит ли архитектура IDE-уровень сложности?**
Сегодня — нет: K1 (полный repaint на курсор), K3 (текст — основной контент IDE), отсутствие диагностики и a11y делают IDE-класс недостижимым. После Phase 2 (retained paint + текст) + Phase 4 (raster-поток) — да: протокол layout, slivers и виртуализация уже на этом уровне.

**12. Эффективно ли на mobile, desktop и web?**
Desktop: после K1/K3 — да. Mobile: структурно нет — нет touch/stylus на platform-шве (M8), нет lifecycle-интеграции, блокирующий present без raster-потока хуже переносится на tile-GPU (до 5 render-pass'ов на сегмент — риск), DPR-баг текстa критичен именно там. Web: wgpu-путь рабочий (есть web_demo), но full-repaint модель дорога в браузере; embedded не оценивался (не заявлен в коде).

**13. Может ли безопасно использовать несколько потоков?**
Может — и это реальный потенциальный козырь против Flutter: lanes типизированы, каналы bounded, freshness-гейты написаны, `RasterOwner` протестирован, unsafe локализован. Но сейчас многопоточность — scaffolding без исполнения, а `Arc<RwLock>`-модель и `!Send` views (84 `Rc<dyn Fn>` поля) — препятствия, которые надо снять в Phase 3–4. Осторожный ответ: да, после фаз 2–4; нет, если стабилизировать как есть.

**14. Возможен ли стабильный публичный API?**
Возможен, если Phase 3 (единая ломка) случится до 1.0: убрать `Arc<RwLock>`/`Arc<dyn Any>` из сигнатур, RPITIT-соглашение зафиксировать, `#[non_exhaustive]`-дисциплину распространить. Текущие необратимо-экспонированные элементы (RwLock в `BuildContext`, DynClone-supertrait, 1-based ID) после 1.0 станут постоянным налогом.

**15. Как должна выглядеть архитектура 1.0?**
Три slab-дерева с генерационными ID (как сейчас) + retained layer/display-list с boundary-гранулярной инвалидацией и damage до present; build-фаза с eq-cutoff и видимыми причинами rebuild; единый текстовый стек на per-thread FontSystem; планировщик — single-owner state machine; `BuildContext` — узкий capability-struct; UI/raster/workers — три честных потока с версионированными результатами; инкрементальная семантика через accesskit; диагностика, отвечающая «почему» на каждый кадр; бенч-гейт в CI. Это тот же проект — не другой: 80% целевой архитектуры уже заложено в его контрактах, не хватает исполнения в пяти подсистемах.

---

## 12. Финальный критерий

> Если бы современный UI-фреймворк проектировался сегодня специально для Rust — выглядел бы он так же?

**Нет.** Он бы: удерживал display lists и слои между кадрами вместо полной перезаписи; выражал владение фазами типами, а не `Arc<RwLock>`; делал shaping и декод на worker-потоках изначально; имел бы единый текстовый стек; резал rebuild по равенству значений; объяснял каждую инвалидацию. Но: он бы почти наверняка сохранил трёхдеревную модель, constraints-протокол, slivers, генерационные ID и headless-тестируемость — потому что это правильные решения, подтверждённые независимо (браузеры, GPUI, Xilem). Расстояние от «текущий FLUI» до «Rust-native FLUI» — не redesign, а пять подсистем (paint-retention, текст, scheduler, ownership швов, диагностика) плюс дисциплина измерения. Проект ближе к цели, чем кажется по этому аудиту: аудит беспощаден к разрыву между заявленным («leapfrog») и исполненным («перенос»), а не к абсолютному качеству кода, которое местами высокое.
