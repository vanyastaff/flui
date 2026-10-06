# Имена до первой публикации — требования (уровень 1)

- **Статус:** черновик
- **Дата:** 2026-10-06
- **База:** `main` @ `4915054c8`
- **Уровень 0:** [../release/requirements.md](../release/requirements.md)

## Зачем

После публикации 0.2.0 на crates.io каждое переименование публичного пути, типа, трейта,
метода или Cargo-фичи станет breaking change для потребителей. Сейчас переименование стоит
одного PR и `rg`. Планка владельца: имя говорит, что вещь **есть** или **делает**, а не что
это мешок (`util`, `helpers`, `common`, `support`, `kit`, `*_commons`). Имя не повторяет
модуль, не тащит словарь Dart и пишется одинаково во всём workspace.

Документ содержит инвентарь (счётчики и file:line), правила со ссылками на первоисточники,
спецификацию гейта `cargo xtask names` и требования R1..R16. Сами переименования делаются
в отдельных PR по этому документу.

## Метод и границы инвентаря

Только чтение: `git ls-files`, `rg`, `cargo clippy --explain`. Ничего не собиралось. Объём:
`crates/`, `packages/`, `src/`, `examples/`, `tools/` (вместе с xtask), тесты и бенчи.
«Ссылок» означает `rg -w -c` по `crates packages src examples docs book tools` вместе с
`ARCHITECTURE.md`, ADR и `AGENTS.md`. Отдельно указаны ссылки из неархивных документов, когда они есть.
Эвристики помечены как эвристики. Точный счёт даст первый `--seed` гейта (R14).

## Текущее состояние (счётчики)

| Категория | Найдено | Из них публичных (breaking) |
|---|---|---|
| a. Файлы и каталоги-мешки (`util`, `support`, `common`, `kit`, `*_commons`, `core`, `types`, `traits`, `shared`, `generic`, `base`, `*_impls`, `helpers`) | 50 (23 каталога, 27 файлов) | путь модуля публичен у 15 (см. b) |
| a′. `*_ext.rs` с одним трейтом `FooExt` | 3 (`platforms/{linux,macos,windows}/window_ext.rs`) | идиоматично, остаются |
| b. `pub mod` с пустым именем | 15, плюс 6 скрытых швов `__*` (вопрос В3) | 15 |
| c1. Публичные типы и трейты со словами-шумом (`Base`, `Core`, `Impl`, `Mixin`, `Manager`, `Info`, `Handler(s)`) | 30 | 30 |
| c2. Одно имя у двух разных публичных сущностей, или две схемы имён для одного понятия | 7 пар; `Ctx`/`Cx`/`Context`: 27 типов в трёх написаниях | все |
| c3. Публичные трейты и типы без единой реализации или использования (непривязанная поверхность) | 7 | 7 |
| c4. Транслитерация Dart/Flutter, где имя в духе Rust яснее | 9 (без семейства `*ThemeData`, вопрос В4) | 9 |
| c5. Префикс `get_` в нарушение C-GETTER (без `get`/`get_mut`, `pub(crate)` и зеркал макросов Win32) | 40 методов | 40 |
| c6. Аббревиатуры не по C-CASE (`HSLColor`, `IOS*`, `MacOS*`, `iOS`, `SrcATop`) | 22 типа и 8 вариантов | все |
| c7. Разное написание одного слова (`Canceled`/`Cancelled`, `Adaptor`/`Adapter`) | 2 слова: 106/305 и 242/555 вхождений | 5 типов |
| c8. Cargo-фичи с тестовым кодом под тремя именами (`testing` ×10, `test-utils`, `test-support`) | 2 | 2 |
| c9. Повтор имени модуля в элементе публичного модуля (эвристика по аналогии с `module_name_repetitions`) | ~400 в публичных модулях, ~836 во всех | обзор, не построчно |
| d. Имена тестов, которые не читаются как поведение | 19 (12 с префиксом `test_`) | — |
| — `pub fn set_*` в стиле изменяемых полей Dart | 421 | вне объёма (форма API) |

Названия гонщиков таблиц (`*_contract`, `*_matrix`; около 150 штук) соответствуют соглашению AGENTS «семейство — одна
таблица», а поведение несут строки таблицы. Здесь они не считаются находками.

## Реестр

Столбцы: **Публ.** = публичное имя, переименование ломает совместимость. **Ссылок** = `rg`, включая документы.
Строки упорядочены по важности. Ниже 82 строки, остаток дан итогами в конце раздела.

### Публичные имена (c1–c8), первые 15 помечены ★

| # | Сейчас | Где | Почему слабо | Предлагается | Публ. | Ссылок |
|---|---|---|---|---|---|---|
| 1★ | `flui_interaction::PointerEventExt` (= `traits::PointerEventExtTrait`) и `events::PointerEventExt` | `flui-interaction/src/traits.rs:84`, `events.rs:458`, реэкспорт `lib.rs:307` | Два разных трейта под одним публичным именем. Фасад `src/interaction.rs:15` отдаёт второй, корень крейта отдаёт первый. Суффикс `Trait` — шум | Один `PointerEventExt` в `events`. Второй удалить, его методы `position`/`pointer_id` дублируют первый | да | 11 + 24 |
| 2★ | `BoxLayoutCtx` и `BoxLayoutContext`; так же `SliverLayoutCtx`/`SliverLayoutContext`, `BoxHitTestCtx`/`BoxHitTestContext`, `SliverHitTestCtx`/`SliverHitTestContext` | `flui-rendering/src/protocol/box_protocol.rs:426,1616`, `sliver_protocol.rs:260,1142`, `context/mod.rs:107-126` | Два разных типа отличаются только сокращением. `*Context` — псевдоним обёртки `LayoutContext<P,…>` для авторов, `*Ctx` — нижний контекст возможности, который эта обёртка содержит | `RawBoxLayoutContext`, `RawSliverLayoutContext`, `RawBoxHitTestContext`, `RawSliverHitTestContext` (прецедент std: `RawWaker`/`Waker`, `RawFd`). Окончательное написание решает В5 | да | 77/228, 25/67, 30/162, 11/45 |
| 3★ | `ElementBase` | `flui-view/src/view/view.rs:201`, реэкспорт `lib.rs:229` | Корень иерархии классов в духе Dart. Это объектно-безопасная стёртая форма элемента | `ErasedElement`: словарь `Erased*` в репозитории уже есть (`ErasedBoxLayoutCtx`, `ErasedDragData`) | да | 198 (неархивные документы: 8) |
| 4★ | `StatelessElementBase`, `StatefulElementBase`, `ProxyElementBase`, `InheritedElementBase`, `RenderElementBase<A>`, `RootElementBase`, `NotificationElementBase` | `flui-view/src/element/kind.rs:67-121` | Маркер-трейты, названные как базовые классы. Суффикс `Base` неверен: это метки варианта `ElementKind` | `element::kind::{Stateless, Stateful, Proxy, Inherited, Render<A>, Root, Notification}` (прецедент: `std::marker::{Send, Sync}`) | да | 4–14 каждый, 46 всего |
| 5★ | `ElementCore<V, A>`, модуль `element::generic` | `flui-view/src/element/generic.rs:293`, `element/mod.rs:37` | `Core` и `generic` не говорят, что это. Это типизированное состояние элемента с детьми по арности, рядом с `Element<V, A, B>` в `unified.rs` | `TypedElement<V, A>` в `element::typed` (пара к `ErasedElement`). Удалить, если `Element<V,A,B>` его поглотил | да | 159 (неархивные документы: 6) |
| 6★ | `Curves` с `pub const EaseIn: Cubic` под `#[expect(non_upper_case_globals)]` | `flui-animation/src/curve.rs:1004-1007` | «Статический класс» Dart. Нарушает C-CASE и глушит lint, который это ловит | Модуль `curves` с `EASE_IN`, `LINEAR`, … (прецедент: `f64::consts::PI`) | да | 166 |
| 7★ | `TargetPlatform::iOS`, `TargetPlatform::MacOS`; `IOSPlatform`, `IOSWindow` и ещё 6 `IOS*`; `MacOSPlatform`, `MacOSWindow`, `MacOSWindowExt` и ещё 7 `MacOS*` | `flui-platform-api/src/target_platform.rs:43`; `flui-platform/src/platforms/{ios,macos}/` | C-CASE: аббревиатура пишется одним словом (`Uuid`, `Stdin`). `iOS` под `#[expect(non_camel_case_types)]` | `Ios`, `MacOs`; `IosPlatform`, `MacOsWindow`, … | да | 7 (варианты), 115 (`IOS*`), 234 (`MacOS*`) |
| 8★ | `HSLColor`, `HSVColor` | `flui-painting/src/styling/hsl_hsv.rs:11,134` | C-CASE | `HslColor`, `HsvColor` | да | 8 + 8 |
| 9★ | `MetaData` (виджет), `RenderMetaData`, `MetaDataPayload`, файлы `meta_data.rs` | `flui-widgets/src/interaction/meta_data.rs:40`, `flui-objects/src/interaction/meta_data.rs:37,44` | Написание Flutter. В Rust это одно слово (`std::fs::Metadata`) | `Metadata`, `RenderMetadata`, `MetadataPayload`, `metadata.rs` | да | 12 |
| 10★ | `Disposable`, `GestureCallback`, `BoxedCallback`, `GestureRecognizerExt` | `flui-interaction/src/traits.rs:188,60,74,146` | Ни одной реализации и ни одного использования. `Disposable` — идиома Dart (`dispose`) вместо `Drop`. `GestureRecognizerExt` отдаёт фасад (`src/interaction.rs:29`) | Удалить (непривязанная поверхность). Модуль `traits` расформировать (см. b) | да | 4, 21, 1, 5 |
| 11★ | `KeyEventHandler` и `KeyEventCallback`; `FocusChangeHandler` и `FocusChangeCallback` | `flui-interaction/src/routing/focus_scope.rs:40`, `focus.rs:21,24`; `flui-widgets/src/interaction/focus.rs:55` | Два словаря для одного понятия. В репозитории 67 публичных `*Callback` и 8 `*Handler` | Только `*Callback`, совпадающие псевдонимы объединить | да | 17/8, 7/8 |
| 12★ | `RecognizerBase` | `flui-interaction/src/recognizers/recognizer.rs:88`, фасад `src/interaction.rs:25` | `Base` — композиционные данные: членство в арене и основной указатель | `ArenaMembership` | да | 78 |
| 13★ | `ContainerParentDataMixin<ChildId>`, файл `container_mixin.rs` | `flui-rendering/src/parent_data/container_mixin.rs:33` | `Mixin` — понятие Dart. Внутри только `previous_sibling`/`next_sibling` | `SiblingLinks<ChildId>`, `sibling_links.rs` | да | 30 |
| 14★ | `flui_testing::text_store_kit` (`assert_conforms`) | `flui-testing/src/lib.rs:102`, `text_store_kit.rs`, `text_store_kit/cases.rs` | `kit` — мешок. Модуль — набор проверок соответствия `TextStore` (ADR-0090 §4) | `flui_testing::text_store_conformance` | да | 43 (ADR-0090, ADR-0092, AGENTS.md, 3 ARCHITECTURE/README; неархивные документы: 12) |
| 15★ | Геттеры в стиле Flutter: `get_velocity`, `get_velocity_estimate`, `get_fling_velocity` (×3 трекера) | `flui-interaction/src/processing/velocity.rs:271-930` | C-GETTER. Транслитерация `getVelocity` | `velocity()`, `estimate()`, `fling_velocity(allow_slow)` | да | 25, 20, 9 |
| 16 | `TextPainter::get_offset_for_caret`, `get_position_for_offset`, `get_line_metrics`, `get_boxes_for_selection`, `get_word_boundary` | `flui-painting/src/text_painter/paint.rs:46-96` | C-GETTER, транслитерация Flutter | `caret_offset(pos)`, `position_at(offset)`, `line_metrics()`, `selection_boxes(range)`, `word_boundary_at(pos)` | да | 12, 6, 23, 23, 15 |
| 17 | `SliverGridLayout::get_scroll_offset_of_child` и ещё 3 `get_*_of_child`/`get_*_for_scroll_offset` | `flui-rendering/src/delegates/sliver_grid_delegate.rs:39-86` | C-GETTER, транслитерация Flutter | `child_scroll_offset(i)`, `child_cross_axis_offset(i)`, `first_index_at(offset)`, `last_index_at(offset)` | да | 4, 3, 2, 2 |
| 18 | `RenderTree::get_two_mut`, `get_parent_and_children_mut`, `get_subtree_mut` | `flui-rendering/src/storage/tree.rs:274-355` | Расходится с прецедентом std для непересекающихся заимствований | `get_disjoint_mut` (std `slice::get_disjoint_mut`, 1.86), `parent_and_children_mut`, `subtree_mut` | да | 12, 9, 19 |
| 19 | `LayerTree::get_layer`, `GestureBinding::get_hit_test`, `DiagnosticsNode::get_property`/`get_property_f64`, `PlatformConfig::get_fullscreen_hotkey` | `flui-layer/src/tree/layer_tree.rs:240`, `flui-interaction/src/binding.rs:763`, `flui-foundation/src/debug.rs:679,729`, `flui-platform/src/config.rs:220` | C-GETTER | `layer(id)`, `hit_test(id)`, `property(name)`, `property_f64(name)`, `fullscreen_hotkey()` | да | 10, 1, 12, 4, 2 |
| 20 | `SemanticsEventData::get_string`/`get_int`/`get_float`/`get_bool`; devtools `get_events*` ×3 | `flui-semantics/src/event.rs:119-143`; `flui-devtools/src/timeline.rs:469-489` | C-GETTER. Для доступа к значению по типу прецедент `serde_json::Value::as_str` | `str(key)`, `int(key)`, …; `events()`, `events_in(category)`, `events_between(a, b)` | да | 1 каждый; 14 |
| 21 | `FocusManager`, `BuildOwner::focus_manager()` | `flui-interaction/src/routing/focus.rs:66` | `Manager` — шум, хотя понятие реальное. См. В1 | `FocusTree`/`Focus`, по решению В1 | да | 168 + 157 (`focus_manager`, в том числе таблица AGENTS) |
| 22 | `FlightManager` | `flui-widgets/src/navigator/hero_flight.rs:725` | `Manager`. Публичен только ради `__test_access` | `HeroFlights`; убрать из публичной поверхности | да (скрыт) | 34 |
| 23 | Трейт `WindowManager` и структура macOS `WindowManager`, `WindowInfo`, `SharedWindowManager` | `flui-platform/src/window.rs:436`; `platforms/macos/window_manager.rs:52,280,444` | У трейта нет реализаций. Два разных `WindowManager`. `Info` — шум | Трейт удалить. Структуру переименовать в `MacOsWindowRegistry`, `WindowInfo` — в `WindowRecord` | да | 12 |
| 24 | `ElementExt` (пустой трейт без реализаций) | `flui-view/src/view/into_view.rs:172` | Непривязанная поверхность | Удалить | да | 5 |
| 25 | `LinuxWindowExt` (нет реализаций) | `flui-platform/src/platforms/linux/window_ext.rs:42` | Непривязанная поверхность | Удалить или реализовать (решение платформенной спеки) | да | 2 |
| 26 | `RootElementImpl` | `flui-view/src/element/root.rs:84`, `lib.rs:195` | `Impl` — шум. Рядом трейт `RootElement` | `ElementTreeRoot` | да | 8 |
| 27 | `TextFormFieldCore` (в закреплённом списке поверхности SDK через `__private`) | `flui-widgets/src/form/text_form_field_core.rs:134`, `flui-sdk/tests/surface.rs:68` | `Core` | `TextFormFieldModel`. Список SDK обновить в том же PR | да (скрыт) | 15 |
| 28 | `AssetCacheCore`/`AssetCacheExt`, `AssetHandleCore`/`AssetHandleExt` | `flui-assets/src/cache/mod.rs:345,435`, `types/handle.rs:311,374` | `Core` и `Ext` на собственных трейтах одного крейта: пара «минимальный трейт + общая реализация» | `AssetCache` (объектно-безопасный) с `AssetCacheExt`, или один трейт, если `dyn` не нужен (В2) | да | 9, 15 |
| 29 | `RenderPhysicalModelBase`, `RenderSliverFloatingHeaderBase`, `PersistentHeaderCoreView` | `flui-objects/src/proxy/physical_model.rs:292`; `sliver/sliver_persistent_header.rs:812,950` | `Base` и `Core` у обобщённых движков за двумя псевдонимами | `RenderPhysicalClip<S>`, `RenderSliverFloatingHeader<M>`, `PersistentHeaderLayoutView` | да | 16, 15, 6 |
| 30 | `PlatformHandlers` | `flui-platform/src/shared/handlers.rs:35` | `Handlers` — таблица необязательных колбэков | `PlatformCallbacks` | да | 61 |
| 31 | `RawInputHandler` | `flui-interaction/src/processing/raw_input.rs:245` | `Handler` ничего не говорит. Объект переводит сырой ввод в события | `RawInputTranslator` | да | 21 |
| 32 | `SlotInfo`, `PhaseInfo`, `GlyphInfo` | `flui-view/src/reactive/mod.rs:174`; `flui-devtools/src/profiler.rs:101`; `flui-painting/src/typography/text_metrics.rs:408` | `Info` — шум | `SlotSnapshot` (так и описан), `PhaseTiming`, `GlyphMetrics` | да | 7, 7, — |
| 33 | `VoidCallback`, `ArgCallback<Arg>`, `ListenerCallback` | `flui-foundation/src/callbacks.rs:72`, `notifier_generic.rs:29`, `notifier.rs:45` | `Void` — тип Dart. Три имени для `Fn` без аргументов и с аргументом | `Callback`, `Listener<Arg>`, `Listener` (= `Listener<()>`) | да | 12, 16, 64 |
| 34 | `Notifier<Arg>`, модуль `notifier_generic` | `flui-foundation/src/lib.rs:213` | Модуль назван по устройству реализации, а не по смыслу | В `notifier` рядом с `ChangeNotifier` | да (путь) | 21 |
| 35 | `TickerCanceled`, `DraggableCanceledDetails`, варианты `Canceled` (`ticker.rs:1343,1395`) при 305 вхождениях `Cancelled` | `flui-scheduler/src/ticker.rs`, `flui-widgets/…/draggable` | Два написания одного слова | `Cancelled` везде (как `tokio_util::sync::CancellationToken::is_cancelled`) | да | 106 |
| 36 | `SliverMultiBoxAdaptor`, `SliverMultiBoxAdaptorParentData`, `element/sliver_adaptor.rs` и `SliverToBoxAdapter`, `RenderViewAdapter` | `flui-rendering/src/parent_data/sliver_variants.rs:159`, `flui-view/src/element/sliver_adaptor.rs` | Два написания (непоследовательность Flutter перенесена как есть) | `Adapter` везде | да | 242 |
| 37 | `PaintCx`, `EventCx`, `TextCx` и `BoxIntrinsicsCtx`, `BoxDryLayoutCtx`, `BoxDryBaselineCtx`, `ErasedBoxLayoutCtx`, `ErasedSliverLayoutCtx` и 15 `*Context` | `flui-rendering/src/context/{paint_cx,intrinsics}.rs`, `flui-view/src/reactive/writer.rs:95`, `flui-rendering/src/pipeline/text_context.rs:255` | Три написания одного понятия, у модулей тоже (`paint_cx.rs`, `pub mod event_cx` ×4) | Одно написание для типов (В5). Привязки везде `cx`, как в std (`cx: &mut Context<'_>`) | да | 190, 495, 26, 211, 95, 70, 11, 9 |
| 38 | `BoxDryBaselineCtx`, `DryBaselineChildRequest/Response`, `ActualBaselineChildCallback` | `flui-rendering/src/context/intrinsics.rs:28-101`, `protocol/box_protocol.rs:344` | Входит в строку 37. `Actual` — слово Flutter (`getDistanceToActualBaseline`) | `BaselineChildCallback` | да | 70 |
| 39 | `RRect`, `ClipRRect`, `RenderClipRRect`, `ClipRRectLayer`, `DRRect`; `RSuperellipse` | `flui-foundation/src/geometry/…`, `flui-painting`, `flui-objects`, `flui-layer` | Аббревиатура Skia. По C-CASE `RRect` — два слова «R»+«Rect» и непрозрачно. kurbo: `RoundedRect` | `RoundedRect`, `ClipRoundedRect`, …, `RoundedSuperellipse`, по решению В4 | да | 283, 42, 51 |
| 40 | `BlendMode::SrcATop`, `DstATop`, `ScaleXY` | `flui-painting` | C-CASE (`atop` — одно слово) | `SrcAtop`, `DstAtop`, `ScaleXy` | да | — |
| 41 | `Canvas::clip_rect_ext`, `clip_rrect_ext`, `clip_rsuperellipse_ext`, `clip_path_ext` | `flui-painting/src/canvas/clipping.rs:39-72` | `_ext` у метода значит «расширенная перегрузка». Не говорит, что принимает | `clip_rect_with(rect, op, behavior)` или один `clip(shape, ClipSpec)` | да | 20 |
| 42 | `CurveExt` в `tween_types.rs` | `flui-animation/src/tween_types.rs:602` | Трейт-расширение `Curve` живёт в файле твинов | В `curve.rs` рядом с `Curve` | да (путь) | — |
| 43 | `BorderRadiusExt` на `BorderRadius = Corners<Radius<f64>>` (свой тип) | `flui-painting/src/styling/border_radius.rs:40`; закреплён в `flui-sdk/tests/surface.rs:41,178` | Расширение собственного псевдонима: нужно только потому, что псевдоним не позволяет inherent `impl` | Inherent-методы на `Corners<Radius<f64>>` или newtype `BorderRadius` (В2) | да | 3 в SDK |
| 44 | `SignalWriteExt` на собственном `Signal<T>` | `flui-view/src/reactive/mod.rs:1037` | `Ext` на своём типе. Оправдано только если это граница возможности (запись запрещена в `build`, ADR-0074) | Оставить при В2 «Ext = граница возможности», иначе inherent | да | 30 |
| 45 | `run_app_impl`, `run_app_with_config_impl` | `flui-app/src/app/runner/mod.rs:230,240` | `_impl` в `pub fn`. При реэкспорте переименовываются в `run_app` | Назвать `run_app`/`run_app_with_config` в месте определения | нет | 4, 3 |
| 46 | Фичи `test-utils` (flui-view), `test-support` (flui-runtime) при `testing` у 10 крейтов | `crates/flui-view/Cargo.toml:112`, `crates/flui-runtime/Cargo.toml:27` | Три имени одного понятия. `utils` — мешок | `testing` | да (фича) | 9 + 11 (в том числе Cargo.toml в flui-app, flui-testing) |
| 47 | `FontData` | `flui-assets/src/types/font_data.rs:10` | `Data` — шум. Внутри байты шрифта | `FontBytes` | да | — |
| 48 | `InheritedData` + derive `derive_inherited_data` | `flui-view/src/view/inherited.rs:247`, `flui-macros/src/lib.rs:140` | `Data` у трейта значений по полям | `InheritedFields` | да | 2 в SDK |

### Модули и файлы (a, b)

| # | Сейчас | Где | Почему слабо | Предлагается | Публ. | Ссылок |
|---|---|---|---|---|---|---|
| 49 | `pub mod core` (`Asset`, `AssetMetadata`), `pub mod types` (`AssetKey`, handle, state, `FontData`) | `flui-assets/src/lib.rs:164,167` | Два мешка | Модули `asset`, `key`, `handle`, `state` (приватные + реэкспорт) | да | — |
| 50 | `pub mod shared` (events, gestures, keys, scroll, visibility, panic_boundary, …), `pub mod traits` | `flui-platform/src/lib.rs:182,184` | `shared` — «общее для бэкендов», не что это | Разнести по смыслу: `input` (events, gestures, keys, scroll), `panic_boundary`, `accessibility`; трейты в `platform`, `host_window` | да (зависит только flui-app) | 160 `shared::` |
| 51 | `pub mod traits` | `flui-interaction/src/lib.rs:154`, `flui-rendering/src/lib.rs:104`, `flui-foundation/src/geometry/mod.rs:57` | Модуль по виду элемента (трейты), а не по предмету | interaction: удалить (строки 1, 10), `HitTestTarget` в `hit_test`. rendering: `render_object`, `render_box`, `render_sliver` на уровень выше. geometry: `units` (`Unit`, `NumericUnit`, `FloatUnit`) и `approx` | да | — |
| 52 | `pub mod ext` (`AnimatableExt`, `AnimationExt`), `pub mod tween_types` | `flui-animation/src/lib.rs:105,118` | `ext` — мешок. `_types` — шум | Расширения рядом со своими трейтами (`animatable`, `animation`), `tween` | да (пути) | 6–7 |
| 53 | `pub mod test_utils` (`ReconcileEventCollector`) | `flui-view/src/tree/mod.rs:22` | `utils` | `tree::reconcile_log`, под фичей `testing` | да | 9 |
| 54 | `pub mod test_support` (`NoopSliver`), вложенный `pub mod test_support` | `flui-rendering/src/lib.rs:91`; `context/intrinsics.rs:522` | Два модуля рядом с `testing` | `testing::noop_sliver`; вложенный — по содержимому | да | 4 |
| 55 | `element/behavior_commons.rs` | `flui-view/src/element/mod.rs:27` | `commons` — мешок «извлечённых свободных функций» | `element/build_cycle.rs` (`build_or_recover`, `should_build_with_trace`, `stamp_sliver_slot`) | нет | 16 |
| 56 | `element/unified.rs` (`Element<V, A, B>`) | `flui-view/src/element/unified.rs:50` | `unified` — история («объединённый»), а не суть | `element/behavior_element.rs` или прямо в `element/mod.rs` | нет | — |
| 57 | `flui-widgets/src/support.rs` + `support/{child_process,retirement,test_cases}.rs` | `lib.rs:78` | Мешок: макрос `generic_render_view_element`, адаптеры колбэков, тестовые раннеры | `callbacks.rs` (адаптеры), `render_view_macro.rs`; тестовые раннеры в `testing/` под `cfg(test)` | нет | 51 |
| 58 | `tools/xtask/src/util.rs` | `main.rs:7` | Мешок: корень репо, метаданные, ADR, размеры, `ScratchDir` | `repo.rs` (root, read, metadata), `adr.rs`, `scratch.rs`, `size.rs` | нет | 72 |
| 59 | `platforms/windows/util.rs` | `flui-platform` | «Windows utility functions and helpers» | `win32_convert.rs` (lparam, wide, dpi) | нет | 6 |
| 60 | `flui-cli/src/build/util/{cargo,environment,process}.rs` | `build/mod.rs:33` | `util` | `build/host/` (обращения к хост-инструментам) | нет | 12 |
| 61 | `flui-cli/src/types.rs` | — | `types` | `project_spec.rs` (валидированные `ProjectName`, org id, путь) | нет | — |
| 62 | `flui-engine/src/test_support.rs`, `flui-log/src/test_support.rs`, `flui-app/src/app/window_test_support.rs` | — | `support` | `gpu_fixture.rs`, `scoped_subscriber.rs`, `test_window.rs` | нет | 179, 2, 5 |
| 63 | `flui-engine/src/shaders/common/*.wgsl`, `blend_helpers.wgsl` | xtask `wgsl.rs:436` | `common`, `helpers` | `shaders/include/`, `blend_math.wgsl` | нет | 89 |
| 64 | `parent_data/base.rs` (трейт `ParentData`), `box_variants.rs`, `sliver_variants.rs`, `table_text.rs` | `flui-rendering/src/parent_data/` | `base`, `variants`; `table_text` — склейка двух тем | `parent_data/mod.rs`, `box_layouts.rs`, `sliver_layouts.rs`, `table.rs` + `text.rs` | нет | 16 |
| 65 | `lerp_impls.rs`, `seq/tuple_impls.rs`, `seq/vec_impls.rs` | `flui-painting`, `flui-view` | `_impls` по виду кода | `lerp.rs`, `seq/tuple.rs`, `seq/vec.rs` | нет | 3, 5, 3 |
| 66 | `form/text_form_field_core.rs` | `flui-widgets` | Строка 27 | `form/text_form_field_model.rs` | нет | — |
| 67 | `tests/support/` (flui-animation, flui-log, flui-painting, flui-view, desktop-mcp), `tests/public_support/` | 5 + 1 каталог | `support` | По содержимому: `child_process.rs` в `tests/child_process.rs`; painting → `tests/row_runner/`; view → `tests/recovery_fixtures/`; desktop-mcp → `tests/mcp_client/` | нет | 11 |
| 68 | `tests/common/` (flui-rendering, flui-widgets, flui-material, flui-cupertino) | 4 каталога | `common`. Три из них — «тонкий реэкспорт headless-харнесса» | `tests/harness.rs` (реэкспорт), `tests/row_runner.rs` | нет | 25 |
| 69 | `flui-assets/tests/network_support.rs`, `examples/support/ai_http.rs`, `flui-view/benches/shared/`, `flui-rendering/benches/helpers.rs` | — | `support`, `shared`, `helpers` | `local_http_server.rs`, `examples/ai_streaming/http.rs`, `benches/mocks/`, `benches/tree_builders.rs` | нет | 3, 1, 2, — |
| 70 | Пример-пакет `hot-reload-counter-types` в `examples/hot_reload_counter/types/` | — | `types` | `hot-reload-counter-model` | нет | — |
| 71 | Три копии `child_process.rs` (flui-animation tests, flui-widgets src и tests) | — | Дубль за мешками `support`/`common` | Одна копия в `flui-testing` (вне объёма — дедупликация, отметить) | нет | — |

### Имена тестов (d)

| # | Сейчас | Где | Почему слабо | Предлагается | Ссылок в документах |
|---|---|---|---|---|---|
| 72 | `test_scale_calculation`, `test_double_tap_timing`, `test_two_finger_tap`, `test_team_captain_wins`, `test_reentrancy_remove_self` | `flui-interaction/src/recognizers/{scale,double_tap,multi_tap}.rs`, `arena/team.rs:506`, `routing/pointer_router.rs:456` | Префикс `test_` повторяет `#[test]`. Первые три называют тему, а не исход | `scale_is_span_ratio_of_two_pointers`, `second_tap_after_timeout_starts_a_new_sequence`, `two_finger_tap_fires_once`, `team_captain_wins_the_arena`, `route_removing_itself_during_dispatch_is_skipped` (точные формулировки — по телу теста) | 0 |
| 73 | `test_jank_detection`, `test_registry_invalidate`, `test_full_frame_lifecycle`, `test_compound_animation_status`, `test_concurrent_access_is_crash_free_and_serialized`, `test_scaffold_platform_plan_matches_scaffold_platform`, `test_access_surface_is_pinned` | devtools `profiler.rs:479`; assets `registry/mod.rs:696`; `flui-scheduler/tests/integration_tests.rs:21`; animation `compound.rs:268`; platform `macos/clipboard.rs:200`; cli `scaffold.rs:274`; widgets `__test_access.rs:787` | Префикс `test_`; первые четыре — темы | Убрать `test_`, сформулировать исход: `frame_over_budget_is_reported_as_jank`, `invalidated_key_reloads`, `frame_runs_phases_in_order`, `compound_status_follows_dominant_parent` | 0 |
| 74 | `flui-scheduler/tests/integration_tests.rs` | — | Имя файла ничего не говорит | `frame_lifecycle.rs` | 0 |
| 75 | `ui_tests` | `flui-view/tests/trybuild_ui.rs:26` | Против `trybuild_ui` в painting, rendering, engine | `trybuild_ui` | `docs/testing.md` |
| 76 | `cancelling_renderer_new` | `flui-engine/src/renderer.rs:2218` | Имя метода, а не исход | `cancelled_renderer_construction_releases_the_surface` (по телу) | `flui-engine/ARCHITECTURE.md` |
| 77 | `unbound_generics` | `flui-painting/src/text_layout/context.rs:798` | Тема без исхода | По телу теста | 0 |
| 78 | `text_store_kit_conformance`, `text_store_kit_matrix` | `flui-widgets/tests/contracts.rs:127`, `flui-testing/tests/text_store_kit.rs:511` | Тянут `kit` (строка 14) | `text_store_conformance`, `text_store_conformance_matrix` | `flui-testing/ARCHITECTURE.md` |
| 79 | `notifier_generic_contract`, `tween_types_contract` | `flui-foundation/src/notifier_generic.rs:400`, `flui-animation/src/tween_types.rs:654` | Следуют за переименованием модуля | `notifier_contract` (как строка таблицы), `tween_contract` | 0 |
| 80 | `exit_codes`, `completions_bash` | `flui-cli/src/error.rs:378`, `flui-cli/tests/cli_completions.rs:14` | Существительные без исхода | `exit_code_matches_error_kind`, `bash_completions_name_every_subcommand` | 0 |
| 81 | `flui_testing_it` против `<crate>_it` | `crates/flui-testing/Cargo.toml` | Задокументированное исключение (`docs/testing.md:516`) | Оставить | — |
| 82 | Fixture `let_bound_event_closure_without_helper.rs` | `flui-view/tests/ui/` | `helper` в имени fixture | `let_bound_event_closure_without_adapter.rs` + `.stderr` | 0 |

**Итоги по остальному.** a: ещё 9 путей с `support`/`common` в тестах и бенчах, по смыслу те же, что в строках 67–69.
c5: ещё 13 `get_*` (`get_events_by_category`, `get_events_in_range`, `get_window`, `get_window_mut`, `get_group`,
`get_worker_build_ptr` и другие) переименовываются по строкам 15–20. `get_x_lparam`/`get_y_lparam` зеркалят макросы Win32 и
остаются. c6: ещё 15 типов `IOS*`/`MacOS*` по строке 7. c9: ~400 повторов модуля в публичных модулях.
Почти все исчезают, если модуль, который держит один тип, станет приватным и реэкспортируется (правило N7).
Остаток проверит clippy (см. «Принуждение»). Сеттеры `set_*` (421) в этом документе не рассматриваются.

## Правила (для AGENTS.md, «Имена»)

Каждое правило проверяемо, гейт указан в скобках.

- **N1. Имя говорит, что вещь есть или делает.** Файлы, каталоги, модули и публичные элементы не
  называются словами-мешками: `util(s)`, `helper(s)`, `common(s)`, `support`, `misc`, `stuff`, `kit`, `shared`, `base`,
  `core`, `impl(s)`, `generic`, `types`, `traits`, `mixin`, `tmp`, `wip`, `old`, `new`, `v2`. Прецедент: std называет модули
  по предмету (`io`, `fmt`, `collections`), а не по виду содержимого. [Rust API Guidelines, C-CASE, предисловие о
  naming; RFC 430]. (`names`: path-word, mod-word, item-word)
- **N2. Регистр по C-CASE.** Аббревиатура пишется одним словом: `Uuid`, `Stdin`, `HslColor`, `Ios`, `MacOs`. Варианты
  enum — `UpperCamelCase`, константы — `SCREAMING_SNAKE_CASE`. `#[expect(non_*_case*)]` разрешён только в
  `generated.rs`. [C-CASE]. (`names`: acronym, case-expect; rustc `non_camel_case_types`, `non_upper_case_globals`)
- **N3. Геттер без `get_`.** `fn velocity(&self)`. `get`, `get_mut`, `get_unchecked(_mut)`, `get_disjoint_mut`,
  `get_or_insert*` разрешены по прецеденту std (`Cell::get`, `Vec::get_mut`, `slice::get_disjoint_mut`). [C-GETTER].
  (`names`: getter)
- **N4. Преобразования — `as_`/`to_`/`into_`/`from_`** по стоимости. [C-CONV]. Порядок слов в типах ошибок —
  глагол-объект-`Error` (`ParseIntError`). [C-WORD-ORDER]. (review)
- **N5. `FooExt` — только настоящее расширение:** трейт добавляет методы чужому типу или трейту, или стоит на границе
  возможности (объектная безопасность, запрет в `build`). Живёт рядом с базой. Свой тип получает inherent-методы.
  Суффикс `Trait` запрещён. [RFC 445; std `std::os::unix::fs::PermissionsExt`, `OsStrExt`; `futures::StreamExt`].
  (`names`: item-word для `ExtTrait`; остальное — review по В2)
- **N6. Одно понятие — одно слово.** Колбэк — `*Callback`, контекст — одно написание (В5), `Cancelled`, `Adapter`, привязка
  контекста — `cx`. Одно имя у двух публичных сущностей одного крейта запрещено. [C-CASE «consistent»; std
  `task::Context` + `cx`]. (`names`: dup-name; `typos.toml` `extend-words` для написаний)
- **N7. Один канонический путь.** Публичный модуль существует, если по нему ищут (`flui_widgets::scroll`). Модуль ради одного
  типа приватный и реэкспортируется. Элемент публичного модуля не повторяет его имя (`io::Error`, не `io::IoError`).
  [std; `clippy::module_name_repetitions`]. (clippy на выбранных крейтах, см. ниже)
- **N8. Без словаря Dart, если у Rust есть своё слово:** `Drop`, а не `Disposable`; `Callback`, а не `VoidCallback`;
  `mod curves` + `const`, а не «статический класс»; `Metadata`, а не `MetaData`; маркер-трейт, а не `*Base`; композиция, а не
  `Mixin`. Исключения — общеупотребительные понятия UI (`Element`, `RenderObject`, `Sliver`) и решения В4.
  [AGENTS «Design stance»]. (`names`: item-word `Base`/`Mixin`/`Impl`; review)
- **N9. Cargo-фичи — без пустых слов.** Тестовая поверхность всегда называется `testing`. Никаких `use-`/`with-`/`utils`.
  [C-FEATURE]. (`names`: feature)
- **N10. Тест называется исходом.** Без префикса `test_` и без существительного-темы. Гонщик таблицы — `<семейство>_contract`
  или `_matrix`, строка — исход. [AGENTS «Writing tests»]. (`names`: test-name)

## Принуждение

### Что даёт clippy (проверено `cargo clippy --explain` на 1.99 и по книге clippy)

| Lint | Что проверяет | Помогает? |
|---|---|---|
| `module_name_repetitions` | Элемент **публичного** модуля, имя которого начинается или кончается именем модуля. С 1.84 в группе `restriction` (раньше `pedantic`), в workspace сейчас `allow`. Настройки: `allowed-prefixes` (по умолчанию `to`, `as`, `into`, `from`, `try_into`, `try_from`) и `allow-exact-repetitions` | Частично. Включить `warn` на крейтах facade и SDK и в официальных пакетах после N7. На ядре сначала прогон `--seed`-оценки: эвристика даёт ~400 |
| `disallowed_names` | Только имена **привязок** (`let foo`). По умолчанию `foo`, `baz`, `quux` | Нет: не видит элементы, модули, файлы |
| `disallowed_types` / `disallowed_methods` | Места **использования** перечисленных путей | Только на время миграции: запретить старые пути, пока живут реэкспорты |
| `wildcard_imports` (pedantic, уже включён) | `use x::*` | Косвенно: при переименовании компилятор найдёт каждое имя |
| `enum_variant_names` (style), `struct_field_names` (pedantic) | Варианты и поля повторяют имя enum или структуры | Да, уже работают |
| `module_inception` (style) | `mod foo` внутри `foo` | Да (ограничивает варианты для строк 56, 64) |
| `self_named_module_files` / `mod_module_files` | Стиль раскладки модулей | Не про словарь |
| rustc `non_camel_case_types`, `non_upper_case_globals` | C-CASE | Да, если не глушить (`case-expect`) |

**Вывод:** ни один lint clippy не проверяет словарь имён файлов, каталогов и модулей и не ловит
`Base`/`Core`/`Impl`/`Kit` в именах элементов. Нужен свой гейт.

### Гейт `cargo xtask names`

- **Где:** `tools/xtask/src/names.rs` + `names/{classes,scan,tests}.rs`. Лексер Rust берётся из `markers` (`markers/tokens.rs`,
  `lex.rs`), разбор элементов — через `syn` (уже зависимость xtask). Раннер таблиц — `table_test.rs`.
- **Вход:** `git ls-files` по `crates packages src examples tools tests benches`. Вне проверки: архивные корни (`docs/plans`,
  `docs/research`), `**/generated.rs`, `target/`, `.worktrees/`.
- **Классы находок** (у каждого свой ключ в allowlist):
  1. `path-word` — сегмент пути (каталог или основа файла) после разбиения по `_`/`-` содержит слово из списка N1.
     `tmp`, `wip`, `old`, `new`, `v2` считаются только как целый сегмент или последний кусок. Fixture trybuild
     (`tests/ui/`, `compile_fail/`, `routable_ui/`) проверяются только на слова-мешки.
  2. `mod-word` — `mod x` (в том числе инлайновый и `#[path]`) с тем же списком. `tests`, `sealed`, `prelude` и швы `__*` из
     списка В3 разрешены.
  3. `item-word` — `pub` struct, enum, trait, type, fn, const, static, macro: CamelCase/snake-слова из списка `Kit`, `Helper(s)`,
     `Util(s)`, `Impl`, `Base`, `Mixin`, `Common`, `Misc`, `Stuff`, `Thing`, `Wrapper`, `ExtTrait`, `Manager`, `Info`, `Data`,
     `Core`, `Handler(s)`. Последние пять — «спорные»: допустимы только с записью allowlist и причиной (`ParentData`,
     `ThemeData` по В4).
  4. `getter` — `pub fn get_*` вне списка N3.
  5. `acronym` — идентификатор типа или варианта с двумя заглавными подряд, за которыми идёт строчная, либо начало с
     строчной перед заглавной (`iOS`). Однобуквенный префикс (`RRect`) — тоже находка до решения В4.
  6. `case-expect` — `#[expect|allow(non_camel_case_types|non_upper_case_globals|non_snake_case)]` вне `generated.rs`.
  7. `dup-name` — одно имя у двух `pub`-определений одного крейта, кроме взаимоисключающих `platforms/<os>/`.
  8. `ctx-spelling` — имена типов и модулей на `Ctx`/`Cx`/`Context` не в выбранном по В5 написании.
  9. `feature` — фича в `[features]` с `util`, `helper`, `support`, `use-`, `with-`; тестовая фича не названа `testing`.
  10. `test-name` — функция под `#[test]` начинается с `test_` или состоит из одного слова.
- **Allowlist:** `tools/xtask/allowlists/names.toml`. Его пишет `cargo xtask names --seed`. Ключ — (`path`, `class`, `word`),
  `count` точный и только убывает. Рост, убывание без правки записи и запись, которой ничего не соответствует, —
  находки, как в `markers` и `file-length`. У записи есть `reason`, `exit` проверяет `ratchet.rs` (ADR или шаг плана).
  Для «спорных» слов `exit = "keep"` допустим только вместе с `reason`, ссылающимся на правило или решение В1–В5.
- **Самопроверка `cargo xtask names --self-test`:** таблица строк, по одной на случай:
  `src/util.rs` → находка; `src/baseline.rs` → нет; `src/text_store_kit.rs` → находка;
  `platforms/windows/window_ext.rs` с `pub trait WindowsWindowExt` → нет; `src/ext.rs` → находка;
  `#[cfg(test)] mod tests` → нет; `pub struct FooHelper` → находка; `pub trait PointerEventExtTrait` → находка;
  `pub fn get_mut` / `get_disjoint_mut` → нет; `pub fn get_layer` → находка; `HSLColor` → находка; `HslColor` → нет;
  вариант `iOS` → находка; `#[expect(non_upper_case_globals)]` в `curve.rs` → находка, в `generated.rs` → нет;
  два `pub struct WindowManager` в одном крейте → находка, в `platforms/macos` и `platforms/windows` → нет;
  фича `test-utils` → находка; `#[test] fn test_x` → находка; `#[test] fn drag_cancels_on_second_pointer` → нет;
  файл в `docs/research/` → не читается; запись allowlist с `count` больше факта, меньше факта, без совпадений → три находки.
- **Подключение:** пары `("names --self-test", …)` и `("names", …)` в `tools/xtask/src/tasks/checks.rs`. Так гейт попадает в
  job `checks`, который проверяет агрегатор `ci` (AGENTS: «A new gate is a `cargo xtask` command *and* a step»). Строка в
  таблице «What the compiler and gates enforce» в AGENTS.md. `typos.toml`: `[default.extend-words]` для N6 (`canceled`,
  `adaptor`).

## Требования

- **R1.** КОГДА в рабочем дереве есть файл, каталог или `mod`, имя которого содержит слово-мешок N1 и не покрыто записью
  allowlist, СИСТЕМА ДОЛЖНА завершать `cargo xtask names` с ошибкой, называющей путь, класс и слово.
- **R2.** КОГДА публичный элемент содержит запрещённое слово (`item-word`), СИСТЕМА ДОЛЖНА сообщать о находке. Для «спорных»
  слов СИСТЕМА ДОЛЖНА требовать запись allowlist с `reason`.
- **R3.** КОГДА `count` записи allowlist не равен факту или запись ничему не соответствует, СИСТЕМА ДОЛЖНА завершать гейт
  с ошибкой (allowlist только сжимается).
- **R4.** КОГДА запускается `cargo xtask checks`, СИСТЕМА ДОЛЖНА выполнять `names --self-test` и `names`. Проверка:
  самопроверка ловит строку, удалённую из таблицы классов.
- **R5.** КОГДА определён `pub fn get_*` вне списка N3, СИСТЕМА ДОЛЖНА сообщать класс `getter`.
- **R6.** КОГДА идентификатор нарушает C-CASE для аббревиатур или `#[expect(non_*_case*)]` стоит вне `generated.rs`,
  СИСТЕМА ДОЛЖНА сообщать класс `acronym`/`case-expect`.
- **R7.** КОГДА в одном крейте два `pub`-определения носят одно имя вне взаимоисключающих платформенных модулей,
  СИСТЕМА ДОЛЖНА сообщать `dup-name`. Проверка: `PointerEventExt` в flui-interaction до исправления.
- **R8.** КОГДА крейт объявляет фичу с тестовой поверхностью, СИСТЕМА ДОЛЖНА требовать имя `testing`.
- **R9.** КОГДА функция под `#[test]` начинается с `test_`, СИСТЕМА ДОЛЖНА сообщать `test-name`.
- **R10.** КОГДА перед публикацией 0.2.0 сравнивается allowlist `names.toml`, СИСТЕМА ДОЛЖНА не содержать записей с `exit`,
  отличным от `keep`, для крейтов, публикуемых в 0.2.0 (строки ★1–15 выполнены). Проверка — сам гейт с флагом
  `--release-surface`.
- **R11.** КОГДА PR переименовывает тест, СИСТЕМА ДОЛЖНА находить каждую цитату старого имени в неархивных документах
  (ARCHITECTURE.md, ADR, `docs/`, AGENTS.md) и отказывать, если цитата осталась. Реализация: класс `test-citation` гейта —
  имя в code span неархивного Markdown вида `snake_case` с `_contract`, `_matrix` или известным `#[test]` должно
  существовать.
- **R12.** КОГДА переименован публичный путь из закреплённого списка `flui-sdk/tests/surface.rs`, СИСТЕМА ДОЛЖНА падать на
  тесте поверхности, пока список не обновлён в том же PR (поведение есть, требование — не обходить его).
- **R13.** КОГДА переименован файл или якорь, на который ссылается неархивный Markdown, СИСТЕМА ДОЛЖНА падать в
  `docs-links`/`docs-paths` (есть). PR переименования прогоняет `cargo xtask checks`.
- **R14.** КОГДА гейт впервые запускается с `--seed`, СИСТЕМА ДОЛЖНА записывать allowlist, счёт которого совпадает с
  инвентарём этого документа ±5 % по каждому классу. Расхождение больше — повод исправить либо гейт, либо документ.
- **R15.** КОГДА публичное имя переименовано, СИСТЕМА ДОЛЖНА иметь фрагмент `changelog.d/` в разделе `### Changed` со старым
  и новым именем (есть `changelog --check`; требование — к содержанию PR).
- **R16.** КОГДА тип с ручным `Debug` (`f.debug_struct("SliverLayoutCtx")`) переименован, СИСТЕМА ДОЛЖНА выводить новое имя.
  Проверка: класс `dup-name` сверяет строковый литерал первого аргумента `debug_struct`/`debug_tuple` с именем типа в
  `impl Debug for`.

### Сценарии отказа при переименовании

- **Ссылка в документе:** `crates/flui-testing/ARCHITECTURE.md` ссылается на `text_store_kit.rs` → `docs-links` падает (R13).
- **Имя теста, процитированное в ADR или ARCHITECTURE:** `cancelling_renderer_new` в `flui-engine/ARCHITECTURE.md`,
  `text_store_kit_matrix` в `flui-testing/ARCHITECTURE.md`. Сегодня этого не ловит ни один гейт (R11).
- **Фича, которой пользуются потребители:** `test-utils` в dev-зависимости `flui-runtime`, `test-support` в `flui-app` и
  `flui-testing`. Также `--features` в CI, `.config/nextest.toml`, шаблонах `flui-cli`, `docs/testing.md`. Переименование
  фичи без правки хотя бы одного места ломает сборку только на той платформе или в той дорожке, где фича включена.
- **Закреплённая поверхность SDK:** `BorderRadiusExt` (`surface.rs:41,178`), `TextFormFieldCore` (`:68`), реэкспорты фасада
  `src/interaction.rs` (`RecognizerBase`, `GestureRecognizerExt`, `PointerEventExt`). R12.
- **Текст сообщений и снимки:** `flui-widgets/tests/parent_data_ancestry.rs:45` проверяет текст `"BoxLayoutCtx::from_erased"`.
  `.stderr` в trybuild содержат имена типов, insta-снимки — `Debug`-вывод. После переименования снимки
  перегенерируются с объяснением в PR (AGENTS: regenerated snapshot is a claim).
- **Проводные имена:** написания `flui-protocol` (ADR-0080) зафиксированы. Переименование Rust-типа не должно менять
  сериализованное имя. Гипотеза, не проверена: если `abi_token` hot reload хэширует имена типов, переименование
  меняет токен и требует пересборки плагинов.
- **Реестры:** `render_object_harness.rs` хранит имена render-объектов строками (`"RenderClipRRect"`, строка 180), а
  `flui-objects/src/lib.rs:80` перечисляет `RenderMetaData`. Их нужно править в том же PR, вместе с именами `harness_*`.

## Вне объёма

- Форма API: сеттеры `set_*` (421), builders вместо изменяемых полей. Отдельная спека.
- Имена крейтов (`flui-objects` и др.) — решение уровня 0, сейчас не предлагаются.
- Дедупликация трёх `child_process.rs` и нескольких раннеров таблиц (строка 71) — только отметка.
- Маркеры процесса в doc-комментариях (`FR-019`, `Phase 1` в `element/kind.rs`) — гейт `markers`.
- Имена приватных локальных привязок и полей (кроме `cx`).
- Документы `docs/plans`, `docs/research` (архив).

## Открытые вопросы (решает владелец)

- **В1. `FocusManager` и семейство `*Manager`.** Понятие реальное: дерево фокуса со слушателями. Варианты: `FocusTree`
  (по устройству), `Focus` (как `egui::Memory`/GPUI `FocusHandle`), оставить с записью allowlist. Касается 168 + 157 ссылок и
  таблицы возможностей в AGENTS.md (`focus_manager`).
- **В2. Трейты `Ext` на собственных типах** (`BorderRadiusExt`, `SignalWriteExt`, `BuildContextExt`, `ViewExt`,
  `AssetCacheExt`). Предложение: `Ext` допустим только для чужого типа или на границе возможности/объектной безопасности,
  иначе inherent. Нужно подтверждение для `BuildContextExt` (56) и `ViewExt` (124).
- **В3. Скрытые швы `__runtime` (view, interaction), `__private`, `__private_text`, `__test_access`, `__derive`.** Предложение:
  оставить подчёркнутые модули (прецедент `serde::__private`), разрешить ровно три написания: `__private` (макросы и
  крейты workspace), `__runtime` (корни композиции, ADR-0081 §4), `__test_access`. `__private_text` и `__derive` — переименовать
  в `__private::text` и `__private::derive`.
- **В4. Словарь Flutter/Skia, который оставляем:** `*ThemeData`/`MediaQueryData`/`IconData` (пара «значение — виджет»),
  `RRect`/`RSuperellipse`, `Diagnosticable`, `Listenable`/`ChangeNotifier`, `*Delegate` (14). Предложение: оставить `*Data`
  как устоявшуюся пару, `RRect` → `RoundedRect` (kurbo) решить до 0.2.0, `VoidCallback`/`Disposable`/`Curves` убрать.
- **В5. Одно написание контекста:** `Context` (std `task::Context`, привязка `cx`) или `Cx` (GPUI/Xilem, короче,
  `EventCx` = 495 ссылок). Предложение: типы `*Context`, привязки `cx`. `PaintCx`/`EventCx` переименовываются вместе с
  `*Ctx`-типами одним PR на крейт.
