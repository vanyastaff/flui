# flui-rendering — Аудит качества/архитектуры + поэтапный план (2026-06-24)

> Источник: 7-агентный аудит подсистем (pipeline, storage, traits/protocol, box-objects,
> slivers/viewport/virtualization, foundations, testing/docs). Каждая находка сверена с
> `.flutter/` и привязана к `file:line`. Breaking-изменения разрешены.

---

## 0. Executive summary

**Здоровое ядро, протёкшая периферия.** Несущая инфраструктура крейта — в хорошем состоянии и
местами превосходит наивный порт:

- Storage: generational `GenId<Render>` (ABA-safe), **ноль `unsafe`** в `storage/`,
  lock-free `RenderState` (атомики + `Option<T>`, не `OnceCell`), корректный re-layout и
  инвалидация intrinsics-кэша.
- Intrinsics: **настоящая parity** — честный порт Flutter `_LayoutCacheStorage`, реально
  подключён к пайплайну (не MVP — это классическая ловушка, которой здесь избежали).
- Virtualization: `sumtree` (B-дерево) корректен, нет достижимых паник, противоречивые
  proptest'ы; `RenderViewport` — реальный мульти-сливер вьюпорт, не заглушка.
- Тест-сюита studio-grade: ~97.5% содержательных ассертов, двунаправленный catalog-guard,
  criterion-бенчи, proptest в виртуализаторе.

**Где боль:**
1. **`pipeline/owner.rs` — god-file на 5603 LOC** с ≥9 зонами ответственности и весь `unsafe`
   layout-walk размазан по нему. Кейстоун рефакторинга.
2. **Тройной налог на capability-трейты:** каждый рендер-объект обязан писать 3 пустых `impl`
   (`PaintEffectsCapability`/`SemanticsCapability`/`HotReloadCapability`) — **108 пустых impl'ов**
   на 34 файла + макрос-затычка. Flutter держит эти три как default-методы на одном базовом классе.
3. **Каталог "harness ✓" — ложный сигнал комфорта:** все объекты проходят catalog-guard, но
   **10 из 24 box-объектов — MVP-partial**, часть с *молчаливыми* расхождениями (см. §3).
4. **Массивный doc-drift:** `ARCHITECTURE.md` "Outstanding refactors" и `ROADMAP.md` описывают
   уже **поставленный** код как будущую работу — агент по этим докам переписывает существующее.
5. Точечные корректностные баги (paint-retention теряет картинку boundary; SliverOpacity
   alpha-0; ClipPath `contains()==true`; over-clip списков; мёртвый `hit_test_children_reverse`;
   `RenderView::hit_test` возвращает `true`).
6. Нет базовых классов `RenderShiftedBox`/`RenderAlign` и поведенческого `RenderProxySliver` —
   из-за чего ~6 будущих объектов заблокированы, а 4 прокси-сливера дублируют логику.

Стратегия плана: **сначала правда и базы (Phase 0–2), потом — массовое создание объектов
(Phase 3–7) в порядке зависимостей.** Базы дешевле один раз сделать правильно, чем платить за
них в каждом новом объекте.

---

## 1. Находки по severity (сводно)

### P0 — корректность / soundness
- `pipeline/owner.rs:3811-3850, 3917-3990` — **paint-retention заявлен как «incremental retention»,
  но это заглушка**: `retained_subtrees.insert(id, LayerTree::new())` хранит *пустое* дерево;
  чистый retained repaint-boundary **со своими draw-ops молча теряет картинку**, а дети всё равно
  перерисовываются. Либо убрать путь ретенции (рисовать boundary безусловно), либо реализовать
  настоящий кэш субдерева. (Flutter `flushPaint` переиспользует весь слой чистого boundary.)

### P1 — архитектура / parity-gap
- `pipeline/owner.rs:1681-1714` — `add_node_needing_compositing_bits_update` и
  `add_node_needing_semantics` **не будят** `fire_need_visual_update()` (в отличие от layout/paint).
  Одиночная пометка из idle-цикла не планирует кадр → «GIF замёрз». + при отсутствии узла запись
  всё равно пушится, воссоздавая ровно тот silent-loss, от которого защищается инвариант.
- `pipeline/owner.rs:3794-3880` — `run_paint` обходит **всё** дерево от корня, а dirty-list
  использует только как lookup ретенции (вместо обхода по dirty repaint-boundary deepest-first).
  Документированное расхождение, но крупнейший parity-gap подсистемы.
- `pipeline/owner.rs:4303-4372` — `run_semantics` — **no-op + `tracing::warn!` на каждый dirty-узел
  каждый кадр**. Ни `SemanticsConfiguration`, ни регистрации owner'а. (Warn-спам на hot-path — сам
  по себе баг.)
- `pipeline/owner.rs:400` — `drain_pending_dirty()` вызывается **только** в `run_frame`, но
  публичный typestate-API позволяет ручной `into_layout().run_layout()`; кросс-тред-запросы
  (`PipelineOwnerHandle`/async-декоды) тогда теряются. Доки на owner.rs:215 / handle.rs:11-14
  описывают несуществующее поведение.
- `view/render_view.rs:368-370` — **`RenderView::hit_test` — заглушка, возвращает `true`** без
  записей, 0 продакшн-вызовов. `pub fn -> true` — публичная ложь. Удалить или делегировать в
  `PipelineOwner::hit_test`.
- `traits/render_box.rs:381-387` + `ARCHITECTURE.md:94` — **инверсия честности**: рабочий
  `hit_test_raw` задокументирован как «still a placeholder». Код живой (мост на render_box.rs:443,
  драйвер owner.rs:806, тесты). Поправить доки.
- `objects/sliver_opacity.rs:105-208` — alpha==0 возвращает `Some(0)` → пушит OpacityLayer и **всё
  равно рисует ребёнка**; `needs_compositing` инвертирован vs Flutter (`alpha>0`). Модульный доккомм
  обещает поведение, которого нет.
- `objects/sliver_fixed_extent_list.rs:88-132`, `objects/sliver_fill_viewport.rs:109-153` —
  **жадная раскладка всех прикреплённых детей**; весь lazy-протокол fixed-extent адаптера
  (index-from-offset, `estimateMaxScrollOffset`, GC, `scrollOffsetCorrection`) отсутствует.
  `scroll_extent` считает только прикреплённых → неверный тотал при окне. (Lazy-инфра уже есть и
  работает — это проводка, не greenfield.)
- `context/hit_test.rs:292-304` — **`hit_test_children_reverse` мёртв/сломан**: `let count = 0`
  → цикл `0..0` → всегда `None`. `#[must_use]`-хелпер, который может вернуть только `None`. Удалить.

### P2 — качество / долг
- `pipeline/owner.rs` — **5603 LOC god-file**, ноль `#[must_use]` (в т.ч. на `into_*`/`finish`
  переходах, которые потребляют `self` и возвращают единственный хэндл пайплайна). См. §2.
- `traits/render_object.rs:59-141` — 3 обязательных no-op capability-супертрейта → 108 пустых
  impl'ов + `impl_sliver_test_caps!`. Свернуть в default-методы на `RenderObject<P>`. См. §2.
- `traits/render_object.rs:140` — `HotReloadCapability::reassemble` — пустой default, но у Flutter
  тело реальное (`markNeedsLayout/Paint/CompositingBits/Semantics + visitChildren`). Hot-reload
  молча ничего не инвалидирует. MVP-as-parity.
- ~~`objects/sliver_list_lazy.rs:546-547` (+4 сливера) — `has_visual_overflow = … || scroll_offset>0`~~
  **[FALSE POSITIVE — Phase 1, 2026-06-24]** Сверка с `.flutter` (`sliver_fill.dart:157/228/306`,
  `sliver_list.dart:331`, `sliver_fixed_extent_list.dart:486`, `sliver.dart:2115`) показала: `|| scrollOffset > 0.0`
  — **намеренное** поведение Flutter (контент за краем вьюпорта визуально клипается). Не баг, не трогать.
- `objects/viewport.rs:312-318` — реверс-проход (center-sliver) переиспользует cache-окно прямого
  прохода → неверная cache-полоса над центром.
- `storage/mod.rs:17-23`, `state/mod.rs:134-156`, `state/tests.rs:30-31` — **ASCII-диаграмма и
  доки показывают забаненные `RwLock<Box<dyn>>` и `OnceCell<Size>` как текущие** (мигрировано в
  by-value + `Option<T>`). Опасно: показывает refusal-trigger-1 как актуальный.
- `constraints/box_constraints.rs:213` — **`normalize()` коллизия имён с Flutter**: у Flutter это
  семантический clamp (`min≥0, max≥min`), у FLUI — округление до сотых для cache-key. Переименовать
  в `round_for_cache`/`quantized`; при нужде добавить настоящий `normalize()`.
- `context/hit_test.rs:270` — `add_self(target_id: u64)` берёт сырой `u64` на публичной границе
  вместо `RenderId`.
- `objects/*` — молчаливые расхождения: `RenderClipPath/ClipOval::contains()==true` всегда
  (clip.rs:319, Flutter отвергает попадания вне пути); `RenderStack`/`RenderFlex` без
  `textDirection` → RTL молча неверный.
- `SubtreeBorrows` (owner.rs:2574+) держит 3 `Mutex` только чтобы пройти `Send+Sync` на замыкании,
  при том что `check_thread()` уже запрещает кросс-тред. Можно дешевле (`RefCell` + локальный
  `unsafe Sync`, как `NodePtr`).

### P3 — полировка
- Мёртвые поля/биты: `retained_layer_ids` (owner.rs, не читается), `HAS_GEOMETRY` flag
  (flags.rs:901, не выставляется — geometry трекается через `Option::is_some`).
- `HAS_OVERFLOW` бит cfg-gated debug-only → value-set типа меняется между профилями.
- Два файла `flags.rs` (`storage/flags.rs` — тип; `storage/state/flags.rs` — фасад-аксессоры).
  Логика не дублируется, но имя путает — переименовать `state/flags.rs`→`flag_accessors.rs`.
- `parent_data/table_text.rs` — не мёртв (`TableCellParentData`+`TextParentData`), но имя —
  грабли-bag из двух протоколов; `TextRange::len()` может underflow (→ `saturating_sub`).
- `parent_data/mod.rs:115-236` — stringly-typed метадата-таблицы (`type_usage` по `&str`) без
  enforcement — вероятно мёртвый scaffolding, проверить и удалить.
- `protocol/protocol.rs` — `ProtocolCompatible`/`are_protocols_compatible`/`assert_compatible`/
  `BidirectionalProtocol` — спекулятивная generality без имплементоров. Подрезать.
- `think.md` — пустой файл (0 байт), удалить. `tests/layout_pipeline_test.rs.disabled` —
  карантин без причины/issue; уникальное покрытие (hand-written `RenderBox`→pipeline) без замены.

### Doc-drift (отдельный класс — самый высокий риск waste для будущих агентов)
- `ARCHITECTURE.md:142-160, 124` — «Outstanding refactors» описывает пустые stub'ы
  `propagate_constraints_to_child`/`sync_child_size_to_parent` и `get_two_mut`/`get_many_mut`
  как невыполненную работу. **Эти функции не существуют — код поставлен** под именами
  `layout_dirty_root`/`layout_subtree_borrowed`/`get_two_mut`/`get_parent_and_children_mut`.
- `ARCHITECTURE.md:79,113,152` + `owner.rs:5244` — фантомный метод `RenderEntry::layout`
  (реально `layout_leaf_only`).
- `ARCHITECTURE.md:124` — «No unsafe impl Send/Sync in this crate» — ложь (owner.rs:2521-2523).
- `ARCHITECTURE.md:117-118` — описывает `Arc<RwLock<PipelineOwner>>` back-refs, удалённые в Mythos
  Step 9.
- `docs/ROADMAP.md` — весь протокол-стек помечен `[ ]` «design phase»; на деле поставлен.
- `ARCHITECTURE.md:187-195` — «property tests deferred, нужен proptest dep» — proptest уже
  dev-dep и используется (`virtualization/tests.rs`). Реальный gap уже: tree-property + loom + miri.
- `docs/LAYOUT_SYSTEM.md:66-70` — `BoxConstraints { pub min_width: f32 }` (реально `Pixels`).
- `ARCHITECTURE.md:222` — заметка про CLAUDE.md-drift сама устарела (CLAUDE.md теперь шим).

---

## 2. Ключевые архитектурные вердикты

**owner.rs (5603 LOC) → разнести в `pipeline/owner/` (~9 модулей).** Швы чистые — почти всё общается
через `&self/&mut self` + поля `dirty`/`RenderTree`:

| Модуль | Что переносим |
|---|---|
| `owner/mod.rs` | struct `PipelineOwner`, `Debug`/`Default`/ctors, `rebind_phase`, аксессоры полей |
| `owner/dirty_marks.rs` | `mark_needs_layout`, `add_node_needing_*`, `drain_*`, dirty-счётчики |
| `owner/lifecycle.rs` | `run_frame`, `into_*`/`finish`, вставка/удаление, deferred-мутации |
| `owner/layout.rs` | `impl<Layout>` + свободные `layout_subtree_borrowed[_impl]` |
| **`owner/unsafe_borrow.rs`** | `NodePtr`, `SubtreeBorrows`, `LayoutCycleGuard`, `ensure_stack`, `unsafe impl` — **весь unsafe в один аудируемый файл** (наивысшая ценность извлечения) |
| `owner/compositing.rs` | `impl<Compositing>` + `CompositingWalkActions` |
| `owner/paint.rs` | `impl<PaintPhase>` + `FragmentComposer`, `clip_layer` |
| `owner/hit_test.rs` | `hit_test*`/`sliver_hit_*`/`box_hit_*` |
| `owner/queries.rs` | intrinsics/dry-layout/dry-baseline мемоизация + `QuerySlot` |
| `owner/diagnostics.rs` | `node_diagnostics`, `debug_diagnostics_*` |

**Capability-трейты → свернуть в default-методы `RenderObject<P>`.** ISP здесь иллюзорен: ни один
потребитель не выбирает подмножество — все три всегда требуются bound'ами blanket-impl'а. Выигрыша
ноль, налог — 108 пустых impl'ов + тест-макрос. Свёртка заодно даёт `reassemble` реальное
Flutter-тело в одном месте (чинит hot-reload parity).

**Storage by-value `Box<dyn RenderObject<P>>` → KEEP.** Отложенный в ARCHITECTURE.md «inner-mutability
split (Arc<dyn> config + mutation в RenderState)» — закрыть как «won't-do»: решает проблему, которой
у FLUI нет (вся `&self`-мутация уже на lock-free атомиках; disjoint-borrow даёт re-entrant
parent↔child без локов/Arc). Это и есть верная долгосрочная форма.

**Нужны 2 отсутствующие базы (разблокируют ~10 объектов):**
- `RenderShiftedBox`/`RenderAligningShiftedBox` + `RenderAlign`/`RenderPositionedBox` —
  база сдвига; `RenderCenter` становится частным случаем. Блокирует RotatedBox, overflow-boxes,
  IndexedStack-alignment, не-центральный Center.
- Поведенческий `RenderProxySliver` — сейчас 4 прокси-сливера дублируют passthrough
  layout/hit/paint; база разблокирует `RenderSliverConstrainedCrossAxis` и будущие прокси.

**`scrolling` feature → CUT.** 0 потребителей, частично устарел (дублирует
`ScrollableViewportOffset` + жадную fixed-extent математику, которую заменил `Virtualizer`),
не компилируется в CI → молча гниёт. `experimental-delegates` → **KEEP-gated + ADR с sunset-триггером**
(трейты имеют реальные impl'ы и 22 теста, но нет драйвер-объектов).

---

## 3. Матрица паритета box-объектов (10/24 — MVP-partial)

| Объект | Статус | Ключевые пробелы vs Flutter |
|---|---|---|
| ColoredBox, SizedBox, ConstrainedBox, LimitedBox, AspectRatio, Opacity, RepaintBoundary, FractionalTranslation, FractionallySizedBox, ClipRect, ClipRRect, Baseline, Offstage, Transform | **parity** | — |
| **Padding** | MVP | нет `EdgeInsetsDirectional`/TextDirection |
| **DecoratedBox** | MVP | только color bg/fg; нет image/gradient/boxShadow/BlendMode/shape.circle |
| **FittedBox** | MVP | `clipBehavior` хранится, но **не применяется** (overflow не клипается) |
| **ClipOval / ClipPath** | MVP / **молч. дивергенция** | `contains()→true` всегда; Flutter отвергает попадания вне формы |
| **Center** | MVP | хардкод-центр, **нет `Alignment`** → не заменяет `RenderAlign` |
| **AbsorbPointer / IgnorePointer / MetaData** | MVP | self/metadata **не регистрируются** в hit-result (gesture-id не протянут); нет `ignoringSemantics` |
| **Flex** | MVP | **нет `paint`/overflow-clip/индикатора**, нет `textDirection`/`verticalDirection`, нет baseline-методов, нет `clipBehavior` |
| **Stack** | MVP | нет `textDirection` → `AlignmentDirectional`/RTL молча неверны |
| **Image** | MVP | нет `color`+`colorBlendMode`, `repeat`, `centerSlice`, `filterQuality`, `matchTextDirection`, `invertColors`; `ImageFit` без `FitWidth`/`FitHeight` |
| **Paragraph** | MVP | нет `hit_test`, нет clip/fade `TextOverflow`, нет inline-children/selection/semantics |

Сливеры: `SliverToBoxAdapter`, `SliverPadding`, `SliverFillRemaining`(×3), `SliverOffstage`,
`SliverIgnorePointer` — **parity**. `SliverListLazy` — MVP→good (реальная виртуализация).
`SliverOpacity` (alpha-0 баг), `SliverFixedExtentList`/`SliverFillViewport` (жадные) — MVP.
`RenderViewport` — near-parity.

---

## 4. Поэтапный план (в порядке зависимостей)

> Принцип: **truth → bases → existing-gaps → new objects → effects → slivers → semantics.**
> Каждая фаза — самостоятельная и оставляет крейт зелёным. `S/M/L` = малый/средний/крупный объём.

### Phase 0 — Правда и гигиена (разблокирует всех, риск ≈ 0)
0.1 Переписать/удалить `ARCHITECTURE.md` «Outstanding refactors» + thread-safety inventory;
    `ROADMAP.md` пометить протокол done; починить `RenderEntry::layout`→`layout_leaf_only`,
    proptest-claim, CLAUDE.md-drift note. *(самый высокий ROI: доки инструктируют переписывать
    поставленный код)*
0.2 Починить забаненные формы в storage-доках (`mod.rs` ASCII-диаграмма, `state/mod.rs`,
    `state/tests.rs`); `LAYOUT_SYSTEM.md` `f32`→`Pixels`.
0.3 Удалить мёртвое: `hit_test_children_reverse`, `RenderView::hit_test` stub,
    `retained_layer_ids`, `HAS_GEOMETRY` flag, `think.md`; решить судьбу
    `tests/layout_pipeline_test.rs.disabled` (портировать или удалить с issue).
0.4 `CUT` feature `scrolling` + 3 мёртвых метрик-типа.
0.5 ADR с sunset-триггером для `experimental-delegates`.

### Phase 1 — Корректностные баги (молчаливые дивергенции)
1.1 **[P0]** paint-retention: убрать путь, теряющий картинку boundary (рисовать безусловно),
    либо реализовать настоящий subtree-кэш. *(чинит баг + удаляет мёртвое поле + ужимает owner.rs)*
1.2 wake-on-mark: `fire_need_visual_update()` для compositing-bits/semantics-пометок; не пушить
    запись при отсутствующем узле.
1.3 SliverOpacity alpha-0: пропустить paint ребёнка + снять слой; инвертировать `needs_compositing`;
    добавить падающий harness-тест.
1.4 ClipPath/ClipOval `contains()`: реальный тест попадания (winding / engine path-hit).
1.5 ~~`has_visual_overflow`: убрать `|| scroll_offset>0.0`~~ **FALSE POSITIVE** (verified vs `.flutter` — намеренное поведение, не баг; см. §1).
1.6 `normalize()` коллизия: переименовать в `round_for_cache`; при нужде — настоящий `normalize()`.
1.7 `add_self(u64)` → `RenderId`; `TextRange::len()` → `saturating_sub`.
1.8 свернуть per-node warn-спам в `run_semantics` в один агрегированный.

### Phase 2 — Архитектурные рефакторинги (делают создание объектов дешёвым и безопасным)
2.1 Декомпозиция `owner.rs` → `pipeline/owner/` (§2), начиная с `unsafe_borrow.rs`.
2.2 Свернуть 3 capability-трейта в default-методы `RenderObject<P>` (−108 impl'ов, −макрос);
    дать `reassemble` реальное тело.
2.3 Ввести `RenderShiftedBox`/`RenderAligningShiftedBox` + `RenderAlign`/`RenderPositionedBox`;
    `RenderCenter` → делегирует. **(P0-фундамент, S)**
2.4 Ввести поведенческий `RenderProxySliver`; дедуп 4 прокси-сливеров. **(enabler, M)**
2.5 `#[must_use]` на переходах фаз/запросах; rename `state/flags.rs`→`flag_accessors.rs`;
    подрезать `ProtocolCompatible`/`BidirectionalProtocol`; убрать 3 `Mutex` из `SubtreeBorrows`.

### Phase 3 — Закрыть parity-gaps существующих объектов
3.1 **Flex**: `paint`+overflow-clip+индикатор, `textDirection`/`verticalDirection`,
    `compute_distance_to_actual_baseline`+`compute_dry_baseline`, `clipBehavior`. **(L)**
3.2 **Stack**: протянуть `textDirection` (RTL). **(S)**
3.3 **Image**: `FitWidth`/`FitHeight`; затем `color`+`colorBlendMode` → `repeat` → `centerSlice` →
    `filterQuality`. **(M)**
3.4 **Padding** `EdgeInsetsDirectional`; **FittedBox** проводка `clipBehavior`;
    **DecoratedBox** gradient/shadow/image/shape. **(M)**
3.5 **Paragraph**: `hit_test` + clip/fade `TextOverflow`. **(M)**
3.6 Промоут `RenderSliverListLazy`→`RenderSliverList` (variable extent) + fixed-extent lazy-адаптер
    над `Virtualizer`; ретайр жадных `SliverFixedExtentList`/`SliverFillViewport`. **(M, инфра готова)**

### Phase 4 — Новые box-объекты (после баз Phase 2)
Порядок зависимостей: `RenderAlign`(2.3) → **RenderListBody** (S) → **RenderIndexedStack** (S, на Stack)
→ **RenderRotatedBox** (S–M, на Align+Matrix4) → **RenderIntrinsicWidth/Height** (M) →
**RenderConstraintsTransformBox/UnconstrainedBox** (M) → **RenderWrap** (L, `WrapParentData` готов) →
**RenderCustomPaint** (M, un-gate `CustomPainter`) → **RenderCustomSingle/MultiChildLayoutBox**
(M–L, делегаты готовы) → **RenderFlow** (M) → **RenderTable** (L).

### Phase 5 — Эффект/слой-объекты (gated на flui-engine/flui-layer)
**RenderColorFiltered** (S–M) → **RenderImageFiltered** (M) → **RenderShaderMask** (L) →
**RenderBackdropFilter** (M–L, engine `apply_backdrop_blur` уже есть, #305) →
**RenderPhysicalModel/Shape** (L).

### Phase 6 — Расширение сливеров
Поведенческий `RenderProxySliver`(2.4) → **RenderSliverPersistentHeader** family (pinned/floating,
geometry-поля уже есть) → **RenderSliverGrid** (L, `SliverGridDelegate` готов) →
**RenderShrinkWrappingViewport** (L) → `get_offset_to_reveal`/`ensureVisible` на `RenderViewport`
→ viewport `anchor` → реверс-проход cache recompute (1-й fix можно в Phase 1).

### Phase 7 — Семантика + анимация (кросс-крейт enablers)
Построение semantics-дерева (`run_semantics` реальный + `SemanticsConfiguration`) — **разблокирует**
`ignoringSemantics`/`alwaysIncludeSemantics`/a11y во всех объектах выше →
**RenderAnimatedSize** (L, нужна анимация) → **RenderMouseRegion/AnnotatedRegion**
(M, `flui_interaction::MouseTracker` готов) → завершить hit-target регистрацию
AbsorbPointer/MetaData (gesture-id через `BoxHitTestContext`).

---

## 5. Что НЕ трогать (подтверждённо здорово)
- Storage arena (`GenId`, disjoint-borrow в safe Rust, lock-free state) — образцово.
- Intrinsics-кэш — настоящий порт `_LayoutCacheStorage`, оставить.
- `sumtree`/`Virtualizer` — корректен и оправдан (добавить лишь empty-leaf `debug_assert`).
- `RenderViewport` layout-ядро, sliver-протокол (constraints/geometry/helpers) — parity.
- Erasure-машинерия (`LayoutCtxErased` GAT, Direct/Proxy) — несущая для disjoint parent+child
  borrow; не упрощать.
- By-value `Box<dyn RenderObject<P>>` storage-модель — верная долгосрочная форма.

---

## 6. Открытые стратегические развилки (на решение владельца)
1. **Семантика (Phase 7) — порт сейчас или позже?** Сейчас честная заглушка; блокирует a11y-поведение
   ~6 объектов. Большой кросс-крейт объём (`flui-semantics`).
2. **Эффект-объекты (Phase 5)** зависят от роста `flui-engine`/`flui-layer` (ShaderMask/ColorFilter
   слои). Часть инфры есть (backdrop blur), часть — нет.
3. **`experimental-delegates`** — un-gate в Phase 4 (CustomPaint первый потребитель) или сохранить
   за флагом до явного спроса.
