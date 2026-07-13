# Аудит готовности RenderObject-слоя flui (Box/Sliver) + конкурентный анализ

Дата: 2026-06-10. Базис: worktree `priceless-robinson-952b78` (main @ `f86095c6`, после PR #176/#177).
Эталон Flutter: `C:/Users/vanya/RustroverProjects/.flutter/flutter/packages/flutter/lib/src/{rendering,widgets,painting}`.
Методика: 6 доменных аудитов, каждый P0/P1-finding адверсариально верифицирован вторым агентом по исходникам обеих сторон; 6 треков конкурентного исследования. Все ссылки file:line проверены. Опровергнутые/скорректированные пункты — в Приложении A.

---

## 1. Executive summary + scorecard

Пайплайн-хребет (dirty-root layout, compositing-bits, fragment paint, intrinsics 4-cache, stacker-защита) — реальный и сверен с object.dart. Но: **ни один пиксель контента (текст/картинка) фреймворк отрисовать не может** — content-leaves = 0; **sliver-домен — типы без исполнения** (layout-bridge возвращает `Err(ContractViolation)` by design, Core.2); hit-test не доносит локальные координаты до целей; semantics-фаза — warn-заглушка. Сборка после wgpu 29 — зелёная, включая 107/107 GPU-тестов на живом DX12.

| Домен | Готовность | Вердикт одной строкой |
|---|---|---|
| Box-протокол | **55%** | Хребет и кэши честные; нет short-circuit чистых детей, baseline-каналов, hit-transform'ов, debug-валидации |
| Sliver-протокол | **13%** | Типы ~85% верны, исполнение мёртвое: bridge=Err, ctx-заглушки, 4/24 объектов, нет viewport |
| Box↔Sliver взаимодействие | **8%** | Ни одного шва не существует: нет viewport, нет adapter, erasure-слой структурно не выражает кросс-протокол; paint ПАНИКУЕТ на Sliver-узле |
| Каталог объектов | **30%** | 25 объектов + RenderView, все proxy/layout; 0 content-leaves; ~50 объектов Flutter отсутствуют |
| Pipeline / object.dart | **55%** | Layout/paint dirty-водопровод solid + 4 настоящих GC-free-преимущества; semantics=stub, paint=full-tree, mutations-during-layout нет |
| Build health (wgpu 29) | **85%** | 464+23+60+107 тестов зелёные, миграция честная; дыры — resilience (uncaptured-error=panic, device-lost=вечный цикл) |

---

## 2. Box-протокол: верифицированные гэпы

Все пункты **подтверждены** адверсариальной проверкой, если не отмечено `[adjusted]`.

### P1 — расходимости/отсутствия

1. **Нет short-circuit чистого ребёнка** (`!needsLayout && constraints == _constraints`). Flutter: `rendering/object.dart:2852` — ранний return в `layout()`. flui: `pipeline/owner.rs:2090-2308` (`layout_subtree_borrowed_impl`) рекурсирует безусловно (child-callback `owner.rs:2189-2210`, leaf `owner.rs:2138-2140`); хелпер сравнения `storage/state/constraints.rs:71-79` (`has_constraints`) имеет **0 call-sites**, кэш-поле `storage/state/mod.rs:186-194` только пишется (`owner.rs:2262`), никогда не сравнивается. Любой relayout boundary = O(subtree) каждый кадр. Док в `owner.rs:1565-1567` описывает несуществующий fast-path (doc drift).

2. **sizedByParent / performResize не существует как путь.** Flutter: `box.dart:2869-2873`, `object.dart:2884-2902`. flui: `traits/render_object.rs:347-349` — метод определён, 0 call-sites; `perform_resize` нигде нет; `protocol/box_protocol.rs:124-126` жёстко передаёт `sized_by_parent=false` (in-code: отложено до Core.2).

3. **Dry baseline без child-канала.** Flutter: `box.dart:2117-2147` + `layout_helper.dart:67-73` (все контейнерные реализации flex/shifted_box опрашивают детей). flui: `traits/render_object.rs:308-314` — `dry_baseline_raw` без child-callback (в отличие от `intrinsic_raw` :266-274 и `dry_layout_raw` :288-295); драйвер `owner.rs:1132-1162` не делает subtree-acquire. Кэширование (constraints, baseline) включая computed-None — паритет с `box.dart:1072-1104` (`layout_cache.rs:142-156`). Сам RenderParagraph-лист пройдёт без канала, но baseline-alignment в контейнерах (RenderFlex baseline, RenderBaseline) — нет.

4. **Actual baseline: без кэша, без канала, 0 вызовов, без size.height-fallback.** Flutter: `box.dart:2472-2520` (onlyReal + memoize в тот же `_LayoutCacheStorage`), `box.dart:3296-3326` (container defaults). flui: `traits/render_box.rs:265-285` — прямой pass-through мимо BoxLayoutCache; нет аналогов `defaultComputeDistanceTo{First,Highest}ActualBaseline`. Baseline-aligned flex/text сегодня нереализуем.

5. **hitTest-контракт по умолчанию отсутствует** `[adjusted]`. Flutter: `box.dart:2916-2959` — дефолтный `hitTest` = size.contains-гейт → `hitTestChildren || hitTestSelf` → `result.add` + debug-ошибки hasSize/needsLayout. flui: `traits/render_box.rs:162` — `hit_test` обязателен без default-тела; драйвер `owner.rs:661-703` без гейта/проверки laid-out; 18 объектов вручную дублируют `ctx.is_within_size(...)`; общий хелпер `context/hit_test.rs:264-276` — мёртвый (count=0 хардкод). `hit_test_behavior()` (`render_box.rs:135-137`) — мёртвая ручка: walk не читает, единственный override RenderMetaData читает своё поле напрямую (`meta_data.rs:210-211`). Коррекция: HitTestBehavior — ОДИН общий тип (реэкспорт `hit_testing/mod.rs:80`), а не отдельный enum; отдельным является путь `HitTestable` в flui-interaction (`routing/hit_test.rs:487-505`), тоже не питающий walk.

6. **Hit-test не записывает ни transform'ы, ни локальные позиции — результат per-object выбрасывается.** Flutter: `box.dart:942-951` (BoxHitTestEntry.localPosition), `box.dart:799-938` (addWithPaintTransform/Offset/RawTransform). flui: `render_box.rs:461-480` — `hit_test_raw` дропает BoxHitTestCtx (включая стек transform'ов), возвращает bool; `owner.rs:680-701` — драйвер кладёт голые `HitTestEntry::new(id)`; в `flui-interaction/routing/hit_test.rs:279-367` ЕСТЬ Flutter-паритетная машинерия push_transform/push_offset — production-walk её не зовёт, каждый entry.transform = identity; диспетчер `binding.rs:757-767` игнорирует entry.transform — обработчики получают глобальные координаты как «локальные» (`tap.rs:437`). Параллельный R-24-стек в `box_protocol.rs:1086-1208` тоже мёртв. Два стека, ни один не подключён — consolidation debt.

7. **debugAssertDoesMeetConstraints и весь пост-layout валидационный слой отсутствуют.** Flutter: `box.dart:2561-2773` (hasSize/finite/isSatisfiedBy/intrinsics-sanity/dry==wet), `box.dart:3177-3206` + `debug.dart:36-117` (debug paint). flui: геометрия коммитится без проверок (`storage/entry.rs:389-390`, `owner.rs:2261-2262`); `is_satisfied_by` (`box_constraints.rs:317`) — 0 production call-sites; `debug_paint`-флаг (`flui-app/config.rs:51`) write-only. Единственный гейт — root-only `UnboundedConstraint` (`render_view.rs:514-524`). Infinite/нарушающий constraints размер не-root узла коммитится молча.

8. **`BoxConstraints::tighten` не клампит к границам** `[adjusted: latent]`. Flutter: `box.dart:234-241` — `clampDouble(width, minWidth, maxWidth)`. flui: `box_constraints.rs:388-395` — сырое значение (tighten(500) при max=100 → tight 500); параллельный `flui-types/constraints.rs:201-231` тоже без кламп. Коррекция: единственный production-caller — `stack.rs:122,125` на UNCONSTRAINED, где кламп почти no-op ⇒ сегодня латентно, но ловушка для ближайших портов padding/aspect-ratio/sizing. Живой край: отрицательные явные Positioned width/height дают tight-отрицательные constraints (`stack.rs:113,118` без `.max(0)` против `stack.dart:267-270`).

9. **getTransformTo / globalToLocal / localToGlobal / per-node applyPaintTransform** `[adjusted]`. Flutter: `box.dart:3014-3093`, `object.dart:3674-3736`. flui: `render_box.rs:181-188` — identity-заглушки с 0 call-sites; `get_transform_to` нигде нет (`path_to_root` в `storage/tree.rs:692` — только ID); коррекция: `RenderView::apply_paint_transform` СУЩЕСТВУЕТ (`view/render_view.rs:440-444`, только root-transform) — отсутствует именно per-node виртуал + ancestor-fold + подключение push_offset/push_transform в hit-walk.

### P2 (сводно)

- parentUsesSize не передаётся — hardcoded true (`box_protocol.rs:106-126`); boundary = tight-or-root.
- `compute_dry_layout` default = `Size::ZERO` молча (`render_box.rs:252-258`) vs debug-throw `box.dart:2071-2086`; нет `debugCannotComputeDryLayout` (`box.dart:2215-2235`).
- Нет addWithPaintTransform-инверсии с удалением перспективы (`box.dart:799-812`); каждый transform-объект изобретает свою (`transform.rs:202-225`, `hit_testing/transform.rs:45-56`).
- size-дисциплина (_DebugSize/hasSize/debugAdoptSize, `box.dart:31-35, 2238-2434`) отсутствует; `has_size()` лжёт true до layout (`render_box.rs:118-128`); геометрия дублируется (RenderState + поле объекта с публичным `size_mut`).
- Валидность constraints не проверяется на входе layout (`constraints/mod.rs:129-141` — пустая ветка, 0 call-sites; vs `box.dart:543-621`).
- `normalize()` = округление для кэша, не Flutter-починка min>max (`box_constraints.rs:180-187` vs `box.dart:627-639`); причём кэш всё равно keyed bit-exact ⇒ мёртвый вес.
- Отсутствуют: tightForFinite, flipped, width/heightConstraints, constrainDimensions, **constrainSizeAndAttemptToPreserveAspectRatio** (нужен RenderImage/FittedBox), lerp, expand(w,h) (`box.dart:136-524`).

### Паритет/плюсы (не чинить)

- Intrinsics 4-cache (U9) = полный порт `_LayoutCacheStorage` (`layout_cache.rs:78-201`, эскалация `owner.rs:1042-1045` = `box.dart:2856-2861`).
- mark_needs_layout walk — паритет включая cache-escalation (`owner.rs:1014-1073` = `box.dart:2839-2861`).
- Контракт завершения layout: типизированный `ContractViolation` во ВСЕХ профилях + panic→Poisoned+retry (`render_box.rs:437-449`, `entry.rs:353-385`) — осознанно лучше Flutter (у него debug-only).

---

## 3. Sliver-протокол: верифицированные гэпы

### P0

1. **Layout-bridge не реализован.** `traits/render_sliver.rs:333-366` — blanket `perform_layout_raw` возвращает `Err(ContractViolation "Core.2 ... memo D5")`; `SliverLayoutCtx` — без хранения детей вообще (`sliver_protocol.rs:177-181`), child-ops заглушки: `child_count()=0`, `layout_child()=ZERO`, parent_data=None (`sliver_protocol.rs:238-260`); pipeline-walk hard-reject'ит sliver-узлы `ProtocolMismatch` (`owner.rs:2122-2131`, закреплено тестом `tests/layout_dirty_root.rs:497-581`). Эталон: `sliver.dart:1310-1393`, `sliver_padding.dart:104-151` (реальный child.layout + чтение geometry). 4 готовых sliver-объекта — pure-math витрины, проверенные только юнитами.

2. **Viewport render-объектов нет — у протокола нет драйвера.** `view/viewport.rs:117-143` — только абстрактный трейт (`RenderAbstractViewport`, 0 импленторов), RevealedOffset, enums. Нет layoutChildSequence (`viewport.dart:785-883`), correction-петли с `_maxLayoutCyclesPerChild=10` (`viewport.dart:1693-1765`), center/anchor (`_attemptLayout` :1767-1846), shrink-wrap-варианта (:2003). SliverConstraints фреймворк породить не может; `center_offset_adjustment` (`render_sliver.rs:109-111`) — 0 потребителей.

### P1

3. **`calculate_cache_offset` — неверная математика окна.** Flutter `sliver.dart:1597-1611`: a = scrollOffset+cacheOrigin; b = scrollOffset+remainingCacheExtent (+ внешний clamp к remainingCacheExtent — обязателен при фиксе). flui `render_sliver.rs:149-158` (+дубликат `sliver_padding.rs:163-171`): a = cache_origin; b = cache_origin+remaining — пропущен scroll_offset, лишний cache_origin в верхней границе. При scroll=1000/origin=-250/remaining=1100: Flutter [750,2100] vs flui [-250,850]. Все тесты sliver_padding используют scroll_offset=0 (`sliver_padding.rs:455-479,547-560`) — формулы совпадают, тест проходит, скролл-кейс расходится. Бонус-нит: док `render_sliver.rs:116` врёт про calculate_paint_offset (возвращает extent, не offset; сама формула эквивалентна).

4. **Hit-test-стек: заглушки + структурно неверный гейт** `[adjusted]`. (a) blanket `hit_test_raw`→false (`render_sliver.rs:376-390`); (b) pipeline независимо отрезает не-Box (`owner.rs:673-676`) — чинить ДВА слоя; (c) `is_hit` (`sliver_protocol.rs:447-450`) — только main_axis vs `bounds.height()` (игнор cross_axis, hit_test_extent, оси, inclusive vs Flutter exclusive `sliver.dart:1505-1508`); (d) entry без crossAxisPosition (`sliver_protocol.rs:386-402` vs `sliver.dart:1036-1063`); (e) padding/opacity hit_test=false TODO(core.2) vs `sliver_padding.dart:218-235`/`proxy_sliver.dart:75-87`; (f) коррекция: `hit_test_self` default СУЩЕСТВУЕТ (`render_sliver.rs:285-287`), но мёртв — отсутствует именно дефолтный гейт-диспетчер `sliver.dart:1500-1526` и addWithAxisOffset (`sliver.dart:1009-1029`).

5. **RenderSliverPadding игнорирует axisDirection/growthDirection** `[adjusted: latent]`. `sliver_padding.rs:128-143` resolve() — только Axis; Flutter — через applyGrowthDirectionToAxisDirection (`sliver_padding.dart:41-69`, `sliver.dart:160-188`), child paint offset для up/left — другой расчёт (`sliver_padding.dart:191-210`). Коррекция: примитивы flip ЕСТЬ (`viewport_offset.rs:42` ScrollDirection::flip, `flui-types/axis.rs:254-261` AxisDirection::opposite), нет именно композиции apply_growth_direction_* и normalizedGrowthDirection (`sliver.dart:453-461`); латентно до reverse/horizontal-viewport'ов.

6. **Семейство sliver-объектов: 20 из ~24 отсутствуют** `[adjusted: counts]`. Есть 4 прокси (padding/opacity/offstage/ignore_pointer, `objects/mod.rs:83-86`). Нет: MultiBoxAdaptor+BoxChildManager (`sliver_multi_box_adaptor.dart:25,201`), List (`sliver_list.dart:40`), Fixed/VariedExtent (`sliver_fixed_extent_list.dart:42,518,538`), Grid (`sliver_grid.dart:561`), Fill ×4 (`sliver_fill.dart:33,121,186,257`), PersistentHeader ×4 (`sliver_persistent_header.dart:120,352,404,508,797` — единственные ПРОИЗВОДИТЕЛИ maxScrollObstructionExtent; потребители — viewport.dart:1183-1352, sliver_group.dart), DecoratedSliver, Main/CrossAxisGroup (`sliver_group.dart:211,32`), TreeSliver, **Single/ToBoxAdapter** (`sliver.dart:1995,2087`), ConstrainedCrossAxis/AnimatedOpacity/SemanticsAnnotations + RenderProxySliver-база (`proxy_sliver.dart:34,430,449,486`; flui-трейт `render_sliver.rs:412-421` — 3 аксессора, 0 импленторов). Parent-data без потребителей: `sliver_variants.rs:81,150,218`. Box↔Sliver-адаптеров нет (`into_render_object.rs:176-185` — удалены).

7. **SliverConstraints без asBoxConstraints** — box-child-sliver'ы непишимы. Flutter `sliver.dart:483-505`, call-sites в list/grid/fill/fixed_extent/persistent_header/`sliver.dart:2098`. flui: метода нет нигде (только doc `docs/LAYOUT_SYSTEM.md:253`); copy_with покрыт 4/12 полями (`sliver_constraints.rs:245-276`; смягчение: тип Copy с pub-полями).

### P2/P3 (сводно)

- Padding cacheExtent-композиция скопировала форму paintExtent-формулы (`sliver_padding.rs:261-265` vs `sliver_padding.dart:153-162`) — недоучёт кэша.
- Padding hitTestExtent: операнды перепутаны местами (`sliver_padding.rs:266-267` vs `sliver_padding.dart:185-188`); тест не ловит (hit==paint).
- Padding обнуляет overlap вместо уменьшения (`sliver_padding.rs:195` vs `sliver_padding.dart:129-137`) — порт со СТАРОГО Flutter.
- Keep-alive: mixin есть (`keep_alive_mixin.rs:30-97`), движка (_keepAliveBucket/collectGarbage) нет — write-only флаги.
- SliverGridLayout: max-index включает лишний полный ряд (`sliver_grid_delegate.rs:68-74` vs `sliver_grid.dart:227-233`); нет SliverGridGeometry/RegularTileLayout-расслоения и обоих shipping-делегатов.
- Default paint: нет visible-гейта и применения paint_offset (`render_sliver.rs:268-270`; pd.paint_offset пишется в `sliver_padding.rs:357-360`, никто не читает).
- Offstage: пропускает correction вверх и называет это «parity» (`sliver_offstage.rs:8-14,123-139`) — Flutter глотает (`proxy_sliver.dart:349-358`); легитимная дивергенция, требует переразметки доков.
- is_normalized без axis-ортогональности (`sliver_constraints.rs:315-327` vs `sliver.dart:467-473`); is_scrolled_out_of_view — бессмысленная формула.
- get_absolute_size_relative_to_origin без знаковой инверсии up/left (`render_sliver.rs:248-252` vs `sliver.dart:1701-1713`).
- SliverGeometry-валидация: нет layoutExtent<=paintExtent, hitTestExtent>=0, precisionErrorTolerance (`sliver_geometry.rs:304-343` vs `sliver.dart:877-904`).
- childMainAxisPosition default молча 0.0 вместо debug-throw (`render_sliver.rs:169-203` vs `sliver.dart:1639-1663`); нет RenderSliverHelpers (hitTestBoxChild/rightWayUp, `sliver.dart:1898-1980`).
- Кэш-ключ: SliverConstraintsCacheKey пропускает user_scroll_direction/cross_axis_direction (`sliver_protocol.rs:102-113`), Hash включает (`sliver_constraints.rs:90-93`) — латентная несогласованность двух нормализаций.
- ViewportOffset/ScrollDirection — почти паритет (`viewport_offset.rs`), но animate_to = jump_to и notify-guard ДРОПАЕТ вложенные уведомления (`viewport_offset.rs:360-372`); 0 потребителей.

---

## 4. Box↔Sliver взаимодействие

Все P0 подтверждены; два скорректированы по severity/framing.

1. **Erasure-слой структурно не выражает кросс-протокольный layout (P0).** `capabilities.rs:89` — `layout_child` монопротоколен; `SliverLayoutCtxErased` = constraints+complete_layout (`sliver_protocol.rs:282-293`); blanket-импли дают RenderEntry<Box> XOR <Sliver> (`into_render_object.rs:131-173`); production-вход layout только Box-типизирован (`owner.rs:1751-1755`), erasure трактует кросс-протокол как ОШИБКУ (`erased.rs:68-96`, `error.rs:208-214`). `ProtocolCompatible::is_compatible()=true` (`sliver_protocol.rs:76-86`) — маркер без адаптера, потребляется только тестами. Flutter: viewport (RenderBox) кладёт SliverConstraints детям (`viewport.dart:821-840`), sliver кладёт BoxConstraints box-ребёнку (`sliver.dart:2098`). Viewport нельзя написать без расширения context-API — архитектурная работа, не «недостающий файл».

2. **Paint-walk ПАНИКУЕТ на Sliver-узле (P0, единственный walk с паникой).** `owner.rs:2645` → `box_render_object()` (`node.rs:562-564` → `as_box_unchecked` → `.expect("Expected Box protocol node")` :150-152), ВНЕ catch_unwind (:2674); та же паника на boundary-проверке детей `owner.rs:2750-2753`. Layout даёт типизированный ProtocolMismatch, intrinsics/dry — тоже; paint обязан быть симметричен (typed error или skip). Sliver достижим через публичный API (`tree.rs:439-483` insert_sliver*, `owner.rs:823-838` enqueue'ит paint). Сам sliver-paint-bridge существует и корректен (`render_sliver.rs:368-374`) — просто недостижим.

3. **Hit-test через границу отсутствует в обе стороны** `[adjusted: tracked Core.2, не silent-bug]`. Нет Offset→(main,cross) на границе viewport (`viewport.dart:1027-1054`) и нет hitTestBoxChild с rightWayUp-флипом (`sliver.dart:1918-1952`); конвертеры `MainAxisPosition::from_*` (`sliver_protocol.rs:346-353`) — 0 вызовов. Сценарий «тап мимо/в соседний Box» сегодня не проявляется (sliver и так не лэйаутится); Core.2-объём должен включить оба направления + out-of-band-механизм, которого нет в HitTestResult.

4. **SliverLayoutCtx-заглушки** `[adjusted: P0→P1, loud-fail known-deferred]` — в production до заглушек не доходит (Err раньше на двух независимых гейтах); риск — честность поверхности (4 экспортированных sliver-объекта выглядят рабочими) и шумная ошибка на каждый кадр, не тихая порча.

5. **RenderView-корень: два мёртвых пути + третья популяция** `[adjusted]`. Живой путь — только RenderViewAdapter (`render_view.rs:491-534` layout, :543-566 hit, вход `renderer_binding.rs:358-374`); standalone `hit_test` возвращает true не трогая result (`render_view.rs:368-370`), `perform_layout` не лэйаутит ребёнка и читает устаревший ViewConfiguration (:322-338, :193-195 — самим адаптером задокументировано как stale-footgun :496-502). Коррекции: Flutter `view.dart:288-296` передаёт ВОЗМОЖНО-loose constraints с `parentUsesSize: !isTight` и сайзит root ОТ РЕБЁНКА при loose — адаптер же всегда `biggest()`+tight ⇒ «Flutter parity» переоценена (совпадает только при tight-окне). Плюс flui-app держит третью популяцию bare-RenderView per-view (`renderer_binding.rs:83`, обходится в hit-walk :359-363).

6. **Нет out-of-band-учёта**: maxScrollObstructionExtent/scroll-extent/shrink-wrap нигде не агрегируются (поля есть в `sliver_geometry.rs:31`; vs `viewport.dart:1306,1852-1925,2185-2196`).

7. **Paint-offset-математика с growth/axis-флипом отсутствует** (computeAbsolutePaintOffset `viewport.dart:1243-1257`, setChildParentData `sliver.dart:2013-2035`) — латентно до viewport, исторически баг-магнит.

8. **Lazy-машинерия (MultiBoxAdaptor/manager/keep-alive-bucket/GC)** — нет полностью; element-side канал (createChild/removeChild) не имеет аналога в arena-дереве; flui-view KeepAliveNotification — 0 потребителей. Крупнейшая дизайн-задача после erasure-гэпа (GC/keep-alive пересекается со slab-ID и реконсайлером).

9. Хуки `child_main_axis_position` типизированы только на sliver-детей (`render_sliver.rs:169-203`) — для будущего ToBoxAdapter потребуют breaking-перетипизации (Flutter — covariant RenderObject, `sliver.dart:1640`).

10. Storage позволяет кросс-протокольные рёбра без валидации (`tree.rs:475-492` insert_sliver_child под ЛЮБОЙ родитель; Flutter кодирует легальность в типах ContainerRenderObjectMixin) — в сочетании с paint-паникой это runtime-крэш вместо compile-error.

11. SliverPaintOrder — словарный порт без потребителя (`viewport.rs:33-45` vs `viewport.dart:647-672,1391-1407`) — инфлирует кажущийся паритет.

---

## 5. Полная gap-матрица render-объектов

Flutter в скоупе ≈75 публичных box-объектов + ~24 sliver; flui: 25 + RenderView (`objects/mod.rs:90-117`, `render_view.rs:44`). Content-leaves = 0.

| Flutter-объект (file:line) | flui | Severity |
|---|---|---|
| RenderParagraph (`paragraph.dart:326`) | **отсутствует** (текст-движок flui-painting готов, потребителя нет; `text_painter/mod.rs:87`) | **P0** |
| RenderImage (`image.dart:22`) | **отсутствует** `[adjusted→P1]`: картинки УЖЕ рисуются через BoxDecoration.image→RenderDecoratedBox (`decorated_box.rs:120`, `decoration.rs:174-240`, engine DrawImage* `backend.rs:498-552`); нет aspect-preserving sizing (`image.dart:349-358`), centerSlice src→dst, invertColors, matchTextDirection | **P1** |
| RenderViewport/ShrinkWrapping (`viewport.dart:1556,2003`) | отсутствует | **P0** |
| RenderSliverToBox/SingleBoxAdapter (`sliver.dart:1995,2087`) | отсутствует | **P0** |
| RenderSliverList/FixedExtent/VariedExtent/Grid/Fill×4/PersistentHeader×4/Groups/Tree | отсутствуют (см. §3.6) | **P0** (стек целиком) |
| RenderEditable (`editable.dart:267`) + RenderEditablePainter (:2825) | отсутствует `[adjusted]`: caret/selection-ГЕОМЕТРИЯ уже есть (`text_painter/paint.rs:29,83,106`); нет render-объекта и paint-слоя | P1 |
| RenderPositionedBox/Align (`shifted_box.dart:397`) | **partial**: RenderCenter без alignment, кламп факторов 0..1 vs Flutter >1 ok (:405-426), no-child=biggest() vs shrink-wrap (:480-495), нет intrinsic-факторов (`center.rs:25-134`) | P1 |
| RenderIntrinsicWidth/Height (`proxy_box.dart:624,783`) | отсутствуют (U9-кэш без потребителей; stepWidth/Height нигде) | P1 |
| **Системно: intrinsics/dry-layout не форвардятся** (`proxy_box.dart:77-113`, `shifted_box.dart:39-74`) | `[adjusted]` 18 из 21 box-объектов наследуют default 0.0 (`render_box.rs:209-242`), 17/21 — Size::ZERO dry; включая sized_box (фикс-размер репортит intrinsic 0!); переопределяют только aspect_ratio/constrained_box/fractionally_sized_box(+limited_box dry). U9-кэш мемоизирует НЕВЕРНЫЕ ответы для всех обёрток. Латентно до первого потребителя | P1 |
| RenderFlex (`flex.dart:412`) | **partial** `[adjusted]`: нет textDirection/verticalDirection/baseline (локальный CrossAxisAlignment `flex.rs:51-62` без Baseline — при том что канонический flui-types ИМЕЕТ Baseline, `alignment.rs:141-164`, и VerticalDirection есть, `axis.rs:475-538`); нет intrinsics (`flex.dart:771-806`). Ремедиация = подключение существующих примитивов | P1 |
| RenderWrap (`wrap.dart:218`) | отсутствует `[adjusted]`: запланирован (`ROADMAP.md:195`, gap-matrix:185), WrapAlignment/ParentData уже есть | P1 |
| RenderPhysicalModel/Shape (`proxy_box.dart:2132,2280`) | отсутствуют; примитив Canvas::draw_shadow есть (`drawing.rs:252`→shadow.wgsl), нет композита elevation+fill+clip+hitTest и transparentOccluder | P1 |
| RenderPointerListener/MouseRegion (`proxy_box.dart:3210,3318`) | partial: MouseTracker/arena есть (flui-interaction), tree-attach-точек нет | P1 |
| RenderLeaderLayer/FollowerLayer (`proxy_box.dart:4475,4550`) | partial: слои+registry в flui-layer есть, render-пары нет | P1 |
| RenderCustomPaint (`custom_paint.dart:382`) | отсутствует; 6 delegate-модулей (~1800 LOC) feature-gated off (`lib.rs:58-72`) | P1 |
| RenderAnimatedOpacity (`proxy_box.dart:1107`) | отсутствует (FadeTransition-бэкенд) | P1 |
| RenderOpacity (`proxy_box.dart:913-925`) | partial: сеттер не инвалидирует paint (`opacity.rs:89-90` — признанный stub), нет alwaysIncludeSemantics | P2 |
| RenderTransform (`proxy_box.dart:2537-2610`) | partial: нет transformHitTests=false и filterQuality | P2 |
| RenderPadding (`shifted_box.dart:126`) | partial: только resolved EdgeInsets, без RTL | P2 |
| RenderStack/IndexedStack (`stack.dart:371,768`) | partial: без AlignmentGeometry/textDirection; IndexedStack нет | P2 |
| RenderShaderMask/BackdropFilter (`proxy_box.dart:1128,1201`) | partial: слои есть, render-glue нет | P2 |
| OverflowBox-семья (`shifted_box.dart:635,835,1043`) | отсутствует (Constrained/Sized/ConstraintsTransformBox) | P2 |
| RenderBaseline (`shifted_box.dart:1544`) | отсутствует (станет load-bearing с RenderParagraph) | P2 |
| CustomSingle/MultiChildLayout, RenderFlow | отсутствуют (трейты gated) | P2 |
| Semantics-аннотаторы ×6 (`proxy_box.dart:4107-4441`) | отсутствуют (flui-semantics crate есть, объектов нет) | P2 |
| RenderAnnotatedRegion (`proxy_box.dart:4761`) | partial (слой есть) | P2 |
| RenderFittedBox (`proxy_box.dart:2798`) | partial: clip_behavior хранится, не применяется (`fitted_box.rs:27-33`) | P2 |
| RenderRotatedBox / AnimatedSize / ListBody / Table / ListWheelViewport | отсутствуют | P2 |
| Platform views + TextureBox (`platform_view.dart`, `texture.dart:38`) | отсутствуют (нужен flui-engine-дизайн, не порт) | P2 |
| Proxy-sliver-гэпы: SliverAnimatedOpacity/ConstrainedCrossAxis/SemanticsAnnotations/DecoratedSliver | отсутствуют (4/8 прокси) | P2 |
| RenderIgnoreBaseline / ClipRSuperellipse / ErrorBox / PerformanceOverlay | отсутствуют | P3 |
| **Present (26)**: ConstrainedBox, LimitedBox, AspectRatio, Clip{Rect,RRect,Oval,Path}, DecoratedBox, FractionalTranslation, RepaintBoundary, Ignore/AbsorbPointer, Offstage, MetaData, FractionallySizedBox, Opacity*, Transform*, FittedBox*, Padding*, Center*, Stack*, Flex*, 4 sliver-прокси, RenderView (* = partial выше) | present | P3 |
| **Extra (flui-only)**: RenderColoredBox (public vs приватный `basic.dart:8397`), RenderSizedBox (дубликат ConstrainedBox — кандидат на коллапс), RenderViewAdapter, generic RenderClip<S>/ClipGeometry, typed delegates | design wins | P3 |

RTL/TextDirection отсутствует во всём objects/ (рендер-слой; в flui-painting bidi-детекция живая).

---

## 6. Pipeline / object.dart: паритет и преимущества

### Подтверждённый паритет (не re-report)

Dirty-root-цикл (shallow-first sort, snapshot, clean-skip на уровне очереди, markNeedsPaint после layout `owner.rs:1537-1544`=`object.dart:2926-2927`); compositing-bits-walk с bottom-up OR и lost-boundary-веткой (`owner.rs:2360-2495`=`object.dart:3232-3264`); WAS_REPAINT_BOUNDARY/NEEDS_PAINT-дисциплина (`owner.rs:2650-2661`=`object.dart:3566`); intrinsics/dry-мемоизация с boundary-escalation; каталог слоёв flui-layer ≈1:1 с layer.dart (pipeline использует ~6 из 19 типов).

### Гэпы (P1 если не указано)

1. **Semantics — stub**: `run_semantics` = sort+warn+clear (`owner.rs:2932-3001`, in-code признание :2949-2952); `describe_semantics_configuration` — 0 вызовов; SemanticsOwner (`flui-semantics/owner.rs:117`) реальный, но никем не конструируется; layout-walk не помечает semantics (vs `object.dart:2760,2912`); action-dispatch — warn (`binding/mod.rs:360-389`).
2. **Relayout-boundary вырожден** `[adjusted]`: формула верна (`geometry.rs:175-193`), но всегда вызывается как (true,false,_) (`box_protocol.rs:124-126`) ⇒ boundary = tight||root; причём hardcoded `parent_uses_size=true` — ПРОТИВОПОЛОЖНОСТЬ Flutter-default false (`object.dart:2798`); канала parentUsesSize в API нет (`box_protocol.rs:242-243`, `capabilities.rs:89`); markNeedsLayoutForSizedByParentChange (`object.dart:2715-2718`) без эквивалента.
3. **Нет early-out + performResize** — см. §2.1/2.2 (двойная регистрация: это и box-, и pipeline-дефект).
4. **invokeLayoutCallback / mutations-during-layout** — нет (`object.dart:3023-3035,1164-1231`): mid_layout_marks дренится только ПОСЛЕ всего снапшота (`owner.rs:1444-1555`), субдерево пре-захватывается как disjoint &mut (`owner.rs:1619-1727`) — child-list не может расти mid-walk by construction. Структурный блокер LayoutBuilder/OverlayPortal/SliverMultiBoxAdaptor — стена на заявленном роадмапе.
5. **Paint = full-tree rebuild каждый dirty-кадр**: `owner.rs:2545-2576` — fresh LayerTree от root (док сам признаёт «retention out of scope»); dirty-список = триггер+residue (`owner.rs:2587-2605`); boundary только ребейзит OffsetLayer и БЕЗУСЛОВНО рекурсирует (`owner.rs:2750-2766`) vs Flutter repaintCompositedChild/updateLayerProperties + skip чистых boundary (`object.dart:1315-1333,269-292`). Retained-реестр flui-layer (`compositor/retained.rs:20-76`) — 0 вызовов из rendering. Крупнейшая perf-дивергенция.
6. **markNeedsCompositingBitsUpdate без parent-walk и конструкторной инициализации**: `owner.rs:1249-1265` флагует только данный узел (vs `object.dart:3198-3216`); `RenderState::new` не ставит NEEDS_COMPOSITING (`state/mod.rs:231-240` vs `object.dart:2007`); док `node.rs:659-660` описывает несуществующий up-walk (doc drift); insert-пути вообще не ставят compositing-bits (`owner.rs:753-833`). Маскируется отсутствием потребителя needs_compositing.
7. **adoptChild/dropChild-инвалидация неполна; reparent без guard** `[adjusted]`: insert = child layout+paint + parent layout (`owner.rs:805-809`), без compositing/semantics; remove НЕ помечает выжившего родителя (`owner.rs:599-619`; в обычном reconcile маскируется on_update родителя `behavior_commons.rs:225-255`, но на mutation-site не гарантировано); guarded reparent-API нет, зато есть UNguarded: `unified.rs:381-404` raw-splice без depth/cycle/marks, `move_render_object_child` — log-stub (:406-417); redepthChildren нет (depth = u16 при insert, `tree.rs:458-462`); `create_default_parent_data` — мёртвый хук (0 вызовов).
8. **ParentData per-walk transient и в production фактически всегда-None** `[adjusted: хуже заявленного]`: ErasedChildState строится заново на каждый walk (`owner.rs:2146-2149`), коммитится только offset (:2275-2287); lazy-insert есть лишь на mut-пути (`box_protocol.rs:608-623`), read-путь возвращает None (:593-606) ⇒ RenderFlex (`flex.rs:232-242`) и RenderStack (`stack.rs:374-376`) читают None для КАЖДОГО ребёнка — flex-факторы и Positioned-спеки недостижимы; ParentDataElement::apply — признанный stub (`flui-view/parent_data.rs:217-222`). vs Flutter persistent parentData (`object.dart:2100-2111`).
9. **getTransformTo/paintsChild** — см. §2.9.
10. (P2) Frame-level abort vs per-node containment: один Poisoned-узел гасит весь кадр (`owner.rs:325-368` «Err => no layer tree») vs Flutter error-box+остальной кадр (`object.dart:2758-2763`).
11. (P2) Paint не скипает узлы с NEEDS_LAYOUT (`owner.rs:2636-2661` vs `object.dart:3497-3499`) — descendant-error-кейс рисует stale-геометрию; однострочный guard.
12. (P2) markNeedsPaint без boundary-up-walk (`owner.rs:1209-1224` vs `object.dart:3326-3367`) — содержимое очереди семантически неверно для будущего retention; **чинить ДО retention**.
13. (P2) Child-PipelineOwner-дерево удалено без multi-view-flush-замены (`owner.rs:96-100` vs `object.dart:1759-1790`, `binding.dart:691-700`).
14. (P2) semanticBounds нет в трейте (`render_object.rs:392` vs `object.dart:3861`) — будущий breaking change на 30+ объектов; дёшево застолбить сейчас.
15. (P3) reassemble-sweep не подключён к owner; showOnScreen отсутствует (sliver-prerequisite); dedup очередей O(N)-скан на каждый mark (`owner.rs:963,1215,1261`) vs Flutter amortized O(1) (`object.dart:1115-1118,1173`) — 10k boundary-marks/кадр = ~50M сравнений.

### Что у flui ЛУЧШЕ Flutter (status=extra, верифицировано)

1. **Typestate-фазы**: run_layout существует только на `PipelineOwner<Layout>`, переходы consume self, compile_fail-доктест (`owner.rs:101,299-368`, `phase.rs`) — Flutter имеет лишь debug-asserts, исчезающие в release.
2. **Generational ids + эвикция dirty-очередей при dispose**: use-after-free render-объекта структурно невозможен без GC (`owner.rs:599-619`, `dirty.rs:88-93`, `tree.rs:517-522`) — у Flutter только debugDisposed.
3. **Bounded cross-thread dirty-канал с wake** (RepaintHandle/PipelineOwnerHandle, `handle.rs:1-127`, `owner.rs:441-490`): Send+Sync, backpressure вместо роста кучи, generation-валидация, wake из idle. Data-plane ADR-0002 в production; аналога у Flutter нет.
4. **Типизированный LayoutCycle + stacker**: RAII-guard → `RenderError::LayoutCycle` вместо stack overflow (`owner.rs:1969-2044`); ensure_stack на 6 walk'ах — 20k-глубина лэйаутится (PR #177).
5. Типизированный contract-violation во всех профилях + Poisoned-retry (см. §2-паритет) — деградация per-node вместо срыва кадра в части кейсов.
6. Generic RenderClip<S>/ClipGeometry вместо 4+ подклассов; typed delegate-suite; RenderViewAdapter.

---

## 7. wgpu 29 / build health

Всё зелёное, миграция честная; находки — resilience, не поломки.

- **Тесты**: flui-rendering 464/464; flui-painting 23/23; flui-engine 60/60; benches `-- --test` все группы Success (закрыт memory-гэп «benches не гонялись после wgpu 29»; они CPU-only); **GPU-suite 107/107 на живом DX12 / wgpu 29.0.3** (реальные instance/adapter/device, texture-pool на живом девайсе).
- **Миграция genuine**: `renderer.rs:186-190` (Instance::new + new_without_display_handle), :217-246 (request_adapter/device Result→EngineError), :601-622 (CurrentSurfaceTexture + reconfigure-retry-once), :642/651 (depth_slice/multiview_mask); 0 deprecation-warnings на форс-рекомпиле; 0 легаси-имён (ImageCopyTexture/Maintain:: и т.п.); glyphon-ошибки маппятся в EngineError::text_render (`text.rs:384-415`); per-target backend-features на месте (`Cargo.toml:74-92`).
- **P2: нет `Device::on_uncaptured_error`** — wgpu 29 default = panic на любую GPU-валидационную ошибку (wgpu-29.0.3 `device.rs:789`); любой driver-quirk роняет процесс вместо дропа кадра. Крупнейшая resilience-дыра.
- **P2: device-loss не обрабатывается; Lost-recovery не пересоздаёт surface** — только `surface.configure()` на том же handle (`renderer.rs:564-573,610-618`); flui-app считает dropped frame и ретраит вечно (`binding.rs:533-536`). Реальный TDR на Windows = вечный чёрный экран с debug-логом. `set_device_lost_callback` нигде нет.
- **P3**: wildcard-arm склеивает Occluded (минимизированное окно!) и Validation с SurfaceLost (`renderer.rs:621`) — мусорные frames_dropped + проглоченные validation-сигналы; Suboptimal логирует «reconfigure on next resize», но не планирует его.
- **P3**: 7 offscreen blur/morphology-тестов двойно-гейтнуты (feature + hardcoded `#[ignore]`, `offscreen.rs:1416,1429-1488`) — тяжелейший multi-pass-юзер wgpu без runtime-покрытия; `#[ignore]` устарели, снять.
- **P3**: flui-engine без bench-таргетов; `cargo check --benches` вакуумен; 0 map_async/readback ⇒ нет golden-image-пути; GPU-perf-дельта 25→29 не измерена.
- flui-painting wgpu не использует (1 doc-comment) — экспозиция к bump'у нулевая.

---

## 8. Конкурентный анализ: применимые механизмы по трекам

### 8.1 Linebender (Xilem/Masonry, Vello, Parley)

1. **Masonry отказался от BoxConstraints** (PR xilem#1560) в пользу per-axis measure (LenReq: Min/Max/FitContent) + exact-size layout — потому что single-pass всё равно требовал intrinsics/dryLayout. Вывод для flui: протокол Flutter оставить (one-pass hot path, который Masonry теперь не получает никогда), но забрать short-circuit по фиксированным осям и NaN/INF-санитизацию на границах протокола.
2. **Debug-верификация measure-кэша**: в debug пересчитывать и сравнивать с кэшем — ловит «забыли request_layout» класс багов; прямой апгрейд U9-кэша.
3. **Compose-pass** (transform-only re-anchor без relayout/repaint): третья дешёвая dirty-ступень между layout и paint; скролл = O(changed transforms). Пара к damage-фиксу U8.
4. **Vello sparse-strips (vello_hybrid)**: CPU coarse raster (Send, параллелится по repaint-boundary-фрагментам под ADR-0002) + тупой vertex/fragment GPU fine — без compute-шейдеров (WebGL2/старые GPU). Архитектурно совместимее с flui, чем classic compute-Vello; per-fragment strip-кэширование — то, чего нет ни у Vello, ни у Impeller.
5. **Parley** (HarfRust с окт-2025; Slint перешёл, Bevy мигрирует с cosmic-text): precomputed min/max-content widths = ровно две тяжёлые intrinsics RenderParagraph; shape-once/break-many = ровно U2b-сплит flui; inline-box placeholders = WidgetSpan. Вопрос cosmic-text-vs-parley пересмотреть ДО затвердевания RenderParagraph.

### 8.2 GPUI (Zed) / Slint

1. **SumTree высот с двумя измерениями (Count/Height)**: O(log n) pixel↔index, точный scrollExtent, splice-инвалидация (`gpui/elements/list.rs`) — основа child-геометрии SliverList вместо линкед-листа Flutter с estimate-джиттером.
2. **ListOffset{item_ix, offset_in_item}** — логический якорь скролла + Absolute/Proportional-коррекции: убирает correction-ping-pong Flutter для prepend/streaming-кейсов.
3. **View-level reuse incl. hit-state**: GPUI reuse_prepaint/reuse_paint копирует hitboxes/listeners/dispatch-nodes + Scene::replay сплайсит primitive-ranges чистых view. Урок: retention flui обязан сохранять hit-test-дерево и input-handlers чистых boundary, не только пиксели.
4. **Slint damage-инвариант**: изменивший геометрию item даёт в damage И старый, И новый bbox; DirtyRegion = max 3 rect'а с merge по наименьшему приросту площади (O(1), без блоу-апа). Точно класс бага U8.
5. **Slint ListView recycling**: comp.update(row,data) ребиндит живой subtree вместо destroy/rebuild; arena+can_update flui делают пул строк конкретным beat-Flutter (у Flutter recycling невозможен by design).
6. GPUI текст: двухпоколенный LineLayoutCache + 4 x-subpixel-варианта в ключе глифа; SDF-instanced quads + per-primitive content_mask/clip_distance вместо scissor/saveLayer для rect-клипов.

### 8.3 Прочая Rust-экосистема (egui/iced/Floem/Blitz/Taffy)

1. **Никто не имеет sliver-grade-протокола** (iced #160 открыт с 2020; egui show_rows только uniform; Floem — spacer-хаки; Blitz — только paint-culling). Sliver-порт flui — структурный дифференциатор, не nicety.
2. **Relayout boundaries так же редки** (Floem — full-tree от root; GPUI — пересоздание Taffy-дерева каждый кадр) — layout_dirty_root flui это бенчмаркабельный заголовок.
3. **Taffy-кэш как cautionary tale**: 2→4→9 слотов из-за min/max-content-зондов = экспоненциальные блоу-апы; не загрязнять Box-протокол AvailableSpace-режимами; CSS-grid — как leaf-объект поверх Taffy. + stop-at-empty-cache-трюк для mark_needs_layout.
4. **Blitz: параллельный deferred text-shaping** (rayon-батч Parley ДО layout-walk) — высший-ROI параллелизм под ADR-0002: шейпить dirty-параграфы батчем между build и layout, serial-walk бьёт тёплые кэши. Закладывать в RenderParagraph (Send-слот shaped-данных) с первого дня.
5. egui keep-if-used-this-frame / GPUI two-frame-swap — политика эвикции для shaped-text/picture-кэшей без LRU-тюнинга.

### 8.4 Jetpack Compose

1. **Раскол measure/place** (requestRemeasure vs requestRelayout + phase-scoped snapshot-observation): position-only-изменения никогда не перезапускают measure. Для flui: NEEDS_PLACE-бит; скролл viewport = placement-pass. Структурный beat-Flutter (Flutter связан performLayout-API, flui — нет).
2. **UsageByParent 3-state** (InMeasureBlock/InLayoutBlock/NotUsed) вместо булева parent_uses_size — точная эскалация инвалидации.
3. **Depth-sorted dirty-set + measured-once-assert**: shallowest-first дрейнинг гарантирует ≤1 measure/узел/кадр; runtime-assert «measured twice» ловит O(2^depth)-класс багов на месте. flui: заменить Vec+линейный dedup на (depth, GenId)-ordered set; добавить per-pass generation-счётчик-assert.
4. **SubcomposeLayout** — ОДИН механизм build-during-measure (slot-id + deactivate-not-dispose пул, cap 7 per contentType) обслуживает и LayoutBuilder, и lazy-sliver-детей. Ровно недостающий invokeLayoutCallback-аналог (§6.4).
5. **Prefetch state-machine** (compose→apply→premeasure, gate per-contentType EMA vs vsync-deadline) — у Flutter аналога НЕТ; flui владеет scheduler'ом ⇒ idle-фаза префетча sliver-детей. + u64-упаковка constraints как формат cache-key (вместо текущих 4×u32).

### 8.5 UIKit / SwiftUI / Blink / Servo

1. **UIKit: скролл = чистый запрос по кэшу атрибутов** (shouldInvalidateLayout(forBoundsChange:)=false); только opted-in (pinned headers) релэйаутятся; typed invalidation-context (contentSize/OffsetAdjustment, invalidatedItemIndexPaths). flui: флаг scroll-зависимости per-sliver + sliver-geometry-кэш (SliverGeometry уже Hash/Eq bit-exact, кэш — () stub) ⇒ скролл = paint-offset-shift, не relayout всех видимых, как у Flutter.
2. **UIKit self-sizing**: estimate→actual feedback с typed-дельтами и анкорингом offset'а; и pitfall — после коррекции обязан re-run fill в ТОМ ЖЕ pass'е, иначе «дыры» (десятилетие UIKit-багов как чеклист).
3. **LayoutNG**: два кэш-слота (kMeasure/kLayout) — measure-then-layout не вытесняют друг друга, measure-слот хранит ПОЛНЫЙ результат поддерева; **constraint-dependence-биты** (depends_on_block_constraints) — кэш-хиты при РАЗНЫХ constraints; **TryReuseFragmentsFromCache** — line-level reuse фрагментов параграфа = O(damage) инкрементальный текст-layout (Flutter не умеет, SkParagraph целиком) — прямо в дизайн RenderParagraph (Vec<LineFragment>, бинпоиск первой грязной строки).
4. **Чистота layout как контракт**: NG-правило «layout читает только ConstraintSpace, иначе кэш сломан» — задокументировать на perform_layout + debug-проверка.
5. **Servo — отрицательный результат**: parallel-by-default layout проиграл sequential на реальных страницах; выжил только embarrassingly-parallel intrinsics/independent-subtrees. Эмпирически валидирует ADR-0002-деферрал Phase-2; если когда-то — батчи arena-смежных диапазонов, не task-per-node.

### 8.6 Виртуализация (Flutter-баги / RecyclerView / TanStack)

1. **Чеклист корректности viewport-петли**: bounded retry (max 10) на scrollOffsetCorrection; КОНТЕЙНЕРЫ ОБЯЗАНЫ ПРОБРАСЫВАТЬ correction (Flutter шипал этот баг дважды: #59819/2020 incl. correction==0.0-edge, #174368/2025 groups) — сделать результат sliver-layout enum'ом `Correction(f32) | Geometry(...)`, чтобы контейнер НЕ МОГ прочитать stale-поля; epsilon-tolerant-asserts (у Flutter рецидив 1e-13-фейлов на f64, flui на f32 строго хуже).
2. **paintOrigin-кластер**: paintOrigin сдвигает paint и overlap следующего sliver'а, но НЕ layout-позицию и НЕ hit-якорь (источник untappable-контента); flui может сделать hit-координату включающей paint_origin по умолчанию (строго лучше Flutter) + property-тест «hit-покрытие == paint-покрытие» (поймал бы #170999/#149094/#47027). Direction-композиция — исчерпывающая 24-кейс-матрица (4 axis × 2 growth × layout/paint/hit) ДО первого контейнерного sliver'а.
3. **GapWorker (RecyclerView)**: idle-префетч между кадрами с vsync-deadline + per-type running-average create/bind-стоимости, направленный по скорости, отменяемый при реверсе. У Flutter ВСЁ cacheExtent-строительство синхронно в кадре скролла — главный beat-Flutter-рычаг.
4. **Двухуровневый пул** (mCachedViews — реверс без ребинда; RecycledViewPool per-type cap 5) + **keyed Myers-diff (DiffUtil)** на уровне child-manager'а: move = relink в slab O(1), change = can_update — структурно чинит Flutter #21023/#58917 (index-реконсайл рушит State при reorder).
5. **Fenwick-дерево measured-extents per-key** (синтез TanStack+Flutter): O(log n) offset↔index + автоматическая anchor-коррекция при resize выше viewport; точный scrollbar (vs дышащая линейная экстраполяция Flutter #97676); + invariant-assert «ни один ребёнок вне [cacheOrigin, remainingCacheExtent] не материализован» (Flutter #92276 — тихий perf-баг годами); overlap-occlusion хитов по умолчанию (pinned header не tap-through, vs ручной OverlapAbsorber у Flutter).

---

## 9. Рекомендации: волны

Текущий план «RenderImage → RenderParagraph» **подтверждается по порядку**, но обе цели имеют обязательные пререквизиты из аудита, и между ними вклинивается короткая волна гигиены Box-протокола. Sliver (Core.2) — следующий большой блок, и его дизайн должен впитать §8 до первой строчки кода.

**Wave 0 — гигиена перед новыми объектами (маленькие, режут будущие хвосты):**
1. `tighten`-clamp (§2.8) + `.max(0)` в stack.rs:113,118 — до любого порта padding/aspect-ratio.
2. Clean-child short-circuit в layout-walk (§2.1): сравнение cached constraints + needs_layout — главный perf-контракт; убрать doc-drift owner.rs:1565.
3. Paint-guard needs_layout (§6.11, однострочник) + paint typed-error/skip на Sliver-узле вместо паники (§4.2).
4. Hit-test transform-проводка (§2.6): выбрать ОДИН стек (flui-interaction HitTestResult), удалить R-24-дубликат, подключить push_offset/push_transform в walk + дефолтный hitTest-гейт на трейте (§2.5). Без этого любой жест под Transform получает неверные координаты.
5. debug-слой: debugAssertDoesMeetConstraints-минимум (finite + is_satisfied_by) на коммите геометрии (§2.7) — дёшево, ловит всё подряд.
6. wgpu resilience: on_uncaptured_error→EngineError-лог, set_device_lost_callback + пересоздание surface, разлепить Occluded/Validation (§7). Снять `#[ignore]` с offscreen-тестов.

**Wave 1 — RenderImage** (подтверждён первым: меньше RenderParagraph, разблокирует визуальные e2e):
- Пререквизит: `constrainSizeAndAttemptToPreserveAspectRatio` в BoxConstraints (§2 P2-список).
- Скоуп: aspect-preserving sizing (`image.dart:349-404`), fit/alignment/repeat (переиспользовать flui-painting decoration-путь), честный centerSlice src→dst в engine; invertColors/matchTextDirection — отложить с маркером.

**Wave 2 — RenderParagraph + baseline-комплект** (вместе, не порознь — baseline-каналы бессмысленны без текста и наоборот):
- dry_baseline child-канал + actual-baseline кэш/канал/container-defaults (§2.3/2.4).
- Intrinsics/dry-forwarding default для обёрток (§5-системный): авто-деривация через dry-run против stub-детей (Compose-приём) закрывает 18 объектов разом.
- RenderFlex: подключить канонические flui-types-энумы (Baseline/VerticalDirection/TextDirection), intrinsics, baseline-alignment; RenderBaseline.
- Дизайн RenderParagraph: shaped-слот = Send-значение для будущего rayon-батча (Blitz), Vec<LineFragment> для line-reuse (LayoutNG), precompute min/max-content на shaped (Parley), placeholder-слоты под WidgetSpan; решить cosmic-text-vs-parley ДО затвердевания.

**Wave 3 — Core.2 Sliver-исполнение** (крупнейший блок; правильный порядок внутри):
1. Фикс чистой математики до моторики: calculate_cache_offset (+ верхний clamp), padding cache/hitTest/overlap-формулы, direction-композиция (apply_growth_direction_*) + 24-кейс-матрица.
2. Erasure-расширение для кросс-протокольного layout_child (§4.1) — дизайн-сессия уровня U19.
3. `asBoxConstraints` + SliverToBoxAdapter + RenderSliverHelpers (hitTestBoxChild/rightWayUp).
4. RenderViewport: layoutChildSequence + bounded correction-loop (результат-enum Correction|Geometry — compile-enforced пропагация), out-of-band-учёт, applyContentDimensions-петля с ViewportOffset; синхронный «layout this viewport now» для gesture-арбитража (Compose forceRemeasure-урок).
5. Sliver hit-walk: оба направления конверсии, reverse-paint-order, overlap-occlusion default, hit==paint-coverage property-тест.

**Wave 4 — lazy-контент** (beat-Flutter-ядро): MultiBoxAdaptor + child-manager поверх SubcomposeLayout-образного re-entrant-хука (он же даёт LayoutBuilder — закрывает §6.4 одним механизмом); Fenwick-extents + logical scroll anchor; двухуровневый recycling-пул per contentType + keyed-diff; GapWorker-префетч с EMA-бюджетом; pinned headers как post-placement-pass (Compose), не отдельный протокол-тип.

**Параллельно/позже**: paint-retention (сначала §6.12 markNeedsPaint-up-walk!), measure/place-раскол + NEEDS_PLACE (Compose §8.4.1 — после Wave 3, т.к. ломает layout-API), semantics-проводка (SemanticsOwner + semanticBounds-стаб в трейт сейчас, чтобы не ломать 30+ объектов потом), multi-view-flush-история.

---

## Приложение A: опровергнутые и скорректированные пункты (НЕ ре-репортить как было)

Полностью опровергнутых finding'ов в этом цикле нет; скорректированы (репортить только в исправленной форме):

| Пункт | Что опровергнуто внутри |
|---|---|
| Box hitTest-контракт | «flui-interaction имеет ОТДЕЛЬНЫЙ behavior-enum» — ложь: HitTestBehavior один общий тип (реэкспорт `hit_testing/mod.rs:80`); отдельный — лишь путь HitTestable. hit_test_self на sliver-трейте существует |
| Box tighten | «missing isSatisfiedBy» — ложь, метод есть (`box_constraints.rs:317`), не подключён; баг латентный, не живой (один caller на UNCONSTRAINED) |
| Box getTransformTo | «apply_paint_transform отсутствует везде» — ложь: есть на RenderView (`render_view.rs:440-444`); paint_transform-хук есть на supertrait; gesture-плумбинг наполовину существует (unwired) |
| Sliver hit-test (f) | «нет hitTestSelf-сплита вообще» — default hit_test_self существует (`render_sliver.rs:285-287`), мёртв; отсутствует именно гейт-диспетчер |
| Sliver padding directions | «нет flip-хелперов нигде» — ScrollDirection::flip и AxisDirection::opposite есть; нет композиции; латентно до reverse-viewport |
| Sliver family | «headers — единственные потребители overlap/maxScrollObstructionExtent» — они единственные ПРОИЗВОДИТЕЛИ; потребители: viewport/groups/fill/padding/grid |
| Interaction SliverLayoutCtx | P0→P1: loud-fail known-deferred (memo D5), заглушки в production недостижимы — Err раньше |
| Interaction hit-test | Не silent-misrouting: sliver не лэйаутится вообще, гэп tracked Core.2 |
| Interaction RenderView | Flutter performLayout НЕ `tight(_size)` — loose constraints + child-sized root (`view.dart:288-296`); адаптер не full-parity (всегда biggest+tight) |
| Catalog RenderImage | P0→P1: «no app can display an image» — ложь, BoxDecoration.image-путь рендерит end-to-end; flui-assets потребляется flui-engine (feature `assets`) |
| Catalog RenderEditable | «зависит от RenderParagraph-grade layout» — устарело: layout-субстрат + caret/selection-геометрия УЖЕ в flui-painting; не хватает render-объекта |
| Catalog RenderWrap | «no plan entry» — ложь: ROADMAP.md:195 + gap-matrix:185; WrapAlignment/ParentData есть |
| Catalog intrinsics | «22 из 25» → 18 из 21 box-объектов (4 sliver-объекта вне intrinsics-API и у Flutter); baseline-часть — документированный staged-deferral |
| Catalog RenderFlex | Примитивы (Baseline-вариант, VerticalDirection, TextDirection) СУЩЕСТВУЮТ в flui-types — RenderFlex объявил параллельные урезанные энумы; ремедиация = wiring, не строительство |
| Pipeline relayout-boundary | parent_uses_size=true — противоположность Flutter-default false; gap признан in-source (Core.2) |
| Pipeline adopt/drop | Гэп удаления частично маскируется on_update; зато найден НЕохраняемый raw-splice reparent (`unified.rs:381-404`) — хуже заявленного |
| Pipeline ParentData | Хуже заявленного: read-путь возвращает None всегда ⇒ flex-факторы/Positioned недостижимы в production уже сейчас |
| Pipeline getTransformTo | См. Box-строку; inverse-машинерия существует, отключена |

Дополнительно из памяти проекта (прошлые циклы, не повторять): flex spacing-P0 опровергнут эталоном; Waker::noop-finding Copilot опровергнут; naive lock-removal findings опровергнуты (Sync required).
