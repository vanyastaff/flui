# Имена до первой публикации — таблица переименований

- **Статус:** черновик на утверждение владельцу
- **Дата:** 2026-10-06
- **Требования:** [requirements.md](requirements.md); дизайн: [design.md](design.md)
- **База:** `main` @ `4915054c8`

Полная таблица «было → стало» для прохода. Источник для `tools/xtask/renames/naming.toml`
(design.md §4): после утверждения каждая строка становится строкой TOML без пересказа.

**Столбцы.** **Было** — текущее имя и где определено. **Стало** — новое имя; `—` значит
«удалить». **Почему** — правило N1–N10 из requirements.md или прецедент экосистемы.
**alias** — значение `#[doc(alias = "…")]` на новом определении; ставится только для имён,
пришедших из Flutter/Dart/Skia (решение владельца N2), а не для собственных старых имён FLUI.
**Ссылок** — `rg -w --count-matches` по `crates packages src examples docs book tools AGENTS.md`,
включая архивные `docs/plans` и `docs/research` (верхняя граница; проход их не трогает); для
строк из реестра требований число взято оттуда. **Риск:** Н — уникальное имя, < 30 ссылок;
С — 30–200 ссылок, путь модуля, строки в реестрах или снимках; В — > 200 ссылок, обмен имён
(swap), цитаты в ADR/AGENTS, проводные имена. **О1…О5** — строка зависит от открытого вопроса
design.md §7; в таблице стоит рекомендуемый вариант.

## 1. Публичные элементы по крейтам

### flui-foundation

| # | Было | Стало | Почему | alias | Ссылок | Риск |
|---|---|---|---|---|---|---|
| F1 | `geometry::RRect` (`geometry/rrect.rs:135`) | `RoundedRect` | Сокращение Skia, C-CASE; kurbo `RoundedRect` | `RRect` | 301 | В |
| F2 | `geometry::RSuperellipse` | `RoundedSuperellipse` | То же; Flutter сам расшифровывает как «rounded superellipse». Альт.: `ContinuousRoundedRect` (SwiftUI `.continuous`) | `RSuperellipse` | 54 | С |
| F3 | методы `from_rrect`, `to_rrect`, `inflate_rrect`, `deflate_rrect` | `from_rounded_rect`, `to_rounded_rect`, `inflate_rounded_rect`, `deflate_rounded_rect` | Следуют за F1 | — | ~10 | Н |
| F4 | `Transform::ScaleXY` (`geometry/transform.rs:215`) | `ScaleXy` | C-CASE | — | 7 | Н |
| F5 | `VoidCallback`, `ValueChanged`, `ValueGetter`, `ValueSetter`, `ValueTransformer`, `FallibleCallback`, модуль `callbacks` | — | Словарь Dart (N8) и непривязанная поверхность: вне `callbacks.rs`, `lib.rs` и README не используются | — | 15/11/9/7/7/7 | Н |
| F6 | `ArgCallback<Arg>` (`notifier_generic.rs:29`) | `NotifierCallback<Arg>` | `Arg` — шум; колбэк `Notifier<Arg>` (N6: `*Callback`) | — | 17 | Н |
| F7 | `Notifier<Arg>` по пути `notifier_generic::` | `notifier::Notifier<Arg>` | Модуль назван по реализации (N7) | — | 21 | Н |

### flui-platform-api

| # | Было | Стало | Почему | alias | Ссылок | Риск |
|---|---|---|---|---|---|---|
| PA1 | `TargetPlatform::iOS`, `TargetPlatform::MacOS` (`target_platform.rs:43`) | `Ios`, `MacOs` | C-CASE, снимается `#[expect(non_camel_case_types)]`. Проверить serde-имя: если вариант сериализуется, оставить провод через `#[serde(rename)]` | `iOS`, `macOS` | 7 | С |

### flui-platform

| # | Было | Стало | Почему | alias | Ссылок | Риск |
|---|---|---|---|---|---|---|
| PL1 | `IOSPlatform`, `IOSWindow`, `IOSSceneEvent`, `IOSSceneSessionId`, `IOSClipboard`, `IOSSceneAttachmentId`, `IOSExecutor`, `IOSDisplay`, `IOSLifecycle`, `IOSController` | `Ios*` | C-CASE (`Uuid`, `Stdin`) | — | 115 | С (только clippy через `cross-typecheck`) |
| PL2 | `MacOSWindow`, `MacOSPlatform`, `MacOSWindowLevel`, `MacOSTextInput`, `MacOSWindowExt`, `MacOSClipboard`, `MacOSWindowState`, `MacOSDisplay`, `MacOSCollectionBehavior` | `MacOs*` | C-CASE | — | 234 | С (то же) |
| PL3 | `MacOSWindowExtTrait` | — (слить в `MacOsWindowExt`) | Суффикс `Trait` (N5) | — | 5 | Н |
| PL4 | трейт `window::WindowManager` | — | Нет реализаций; два `WindowManager` в крейте (N6) | — | 12 | Н |
| PL5 | macOS `WindowManager`, `WindowInfo`, `SharedWindowManager`; методы `get_window`, `get_window_mut`, `get_group` | `MacOsWindowRegistry`, `WindowRecord`, `SharedMacOsWindowRegistry`; `window`, `window_mut`, `group` | `Manager`, `Info` — шум; C-GETTER | — | 16/9/3 | Н |
| PL6 | `LinuxWindowExt` | — | Нет реализаций (непривязанная поверхность) | — | 4 | Н |
| PL7 | `PlatformHandlers` (`shared/handlers.rs:35`) | `PlatformCallbacks` | Таблица `Fn`-колбэков (N6) | — | 62 | С |
| PL8 | `PlatformConfig::get_fullscreen_hotkey` | `fullscreen_hotkey` | C-GETTER | — | 2 | Н |

### flui-painting

| # | Было | Стало | Почему | alias | Ссылок | Риск |
|---|---|---|---|---|---|---|
| PT1 | `HSLColor`, `HSVColor` | `Hsl`, `Hsv` | C-CASE; palette `Hsl`/`Hsv` рядом с уже существующим `Oklab`. Альт.: `HslColor`/`HsvColor` | `HSLColor`, `HSVColor` | 11/9 | Н |
| PT2 | `BlendMode::SrcATop`, `BlendMode::DstATop` | `SrcAtop`, `DstAtop` | C-CASE; peniko `Compose::SrcAtop` | `srcATop`, `dstATop` | 25/41 | Н |
| PT3 | `Canvas::clip_rrect`, `draw_rrect`, `draw_drrect`, `clip_rsuperellipse` | `clip_rounded_rect`, `draw_rounded_rect`, `draw_rounded_rect_ring`, `clip_rounded_superellipse` | Следуют за F1/F2; `drrect` = «внешний минус внутренний» | `clipRRect`, `drawRRect`, `drawDRRect`, `clipRSuperellipse` | 82/~20 | С |
| PT4 | `Canvas::clip_rect_ext`, `clip_rrect_ext`, `clip_rsuperellipse_ext`, `clip_path_ext` | `clip_rect_with`, `clip_rounded_rect_with`, `clip_rounded_superellipse_with`, `clip_path_with` | `_ext` у метода не говорит, что принимает | — | 20 | Н |
| PT5 | варианты `DrawCommand::DrawRRect`, `DrawDRRect`, `ClipRRect`, `ClipRSuperellipse` | `DrawRoundedRect`, `DrawRoundedRectRing`, `ClipRoundedRect`, `ClipRoundedSuperellipse` | Следуют за F1/F2 | — | 2/4/—/7 | Н |
| PT6 | `TextPainter::get_offset_for_caret`, `get_position_for_offset`, `get_line_metrics`, `get_boxes_for_selection`, `get_word_boundary` | `caret_offset`, `position_at`, `line_metrics`, `selection_boxes`, `word_boundary_at` | C-GETTER, транслитерация | — | 12/6/23/23/15 | С |
| PT7 | `PaintingStyle` (`paint/canvas.rs:86`) | — (`PaintStyle`) | Два enum `Fill`/`Stroke` для одного понятия (N6) | — | 2 | Н |
| PT8 | `ClipBehavior` (`paint/clipping.rs:85`) | — (`Clip`) | Копия `Clip` вариант в вариант, `to_clip` — тождество (N6) | — | 12 | Н |
| PT9 | `MaterialType` (`styling/physical_model.rs:30`) | `SurfaceKind` | За О4 (`Material` → `Surface`) | `MaterialType` | 4 | Н |
| PT10 | `InlineSpanTrait` | `SpanContent` (удалить, если enum `InlineSpan` его поглощает) | Суффикс `Trait` (N5) | — | 8 | Н |
| PT11 | `GlyphInfo` | `GlyphMetrics` | `Info` — шум | — | 6 | Н |

### flui-interaction

| # | Было | Стало | Почему | alias | Ссылок | Риск |
|---|---|---|---|---|---|---|
| I1 | `PointerEventExt` (`traits.rs:84`, = `PointerEventExtTrait`) и `events::PointerEventExt` | один `events::PointerEventExt`; `PointerEventExtTrait` — | Одно имя у двух трейтов (N6), суффикс `Trait` | — | 28/13 | С (фасад) |
| I2 | `Disposable`, `GestureCallback`, `BoxedCallback`, `GestureRecognizerExt` | — | Нет реализаций и использований; `Disposable` — Dart вместо `Drop` | — | 8/22/2/8 | С (фасад `src/interaction.rs`) |
| I3 | `KeyEventHandler` | — (`KeyEventCallback`) | Два словаря (N6) | — | 18 | Н |
| I4 | `RecognizerBase` | `ArenaMembership` | `Base` — композиционные данные арены | — | 79 | С |
| I5 | `get_velocity`, `get_velocity_estimate`, `get_fling_velocity` (×3 трекера) | `velocity`, `estimate`, `fling_velocity` | C-GETTER | `getVelocity`, `getVelocityEstimate` | 25/20/9 | С |
| I6 | `GestureBinding::get_hit_test` | `hit_test` | C-GETTER | — | 1 | Н |
| I7 | `FocusManager`; `LifecycleContext::focus_manager` и `BuildOwner::focus_manager` | `FocusTree`; `focus_tree()` (**О5**) | `Manager` — шум | `FocusManager` | 173/162 | В (таблица AGENTS) |
| I8 | `RawInputHandler` | `RawInputTranslator` | Переводит сырой ввод в события | — | 22 | Н |
| I9 | `PointerRouteHandler`, `GlobalPointerHandler` (`Rc<dyn Fn>`) | `PointerRouteCallback`, `GlobalPointerCallback` | N6 | — | 17/8 | Н |
| I10 | `HandlerId` (`ids.rs:160`) | `PointerRouteId` | Следует за I9 | — | 22 | Н |
| I11 | `MacosFlingVelocityTracker` | `MacOsFlingVelocityTracker` | Одно написание с PL2 | — | ~5 | Н |
| I12 | `OneEuroFilter2D` | `OneEuroFilter2d` | C-CASE; прецедент Bevy `Camera2d` | — | 15 | Н |
| I13 | `NodeContext` (`= Rc<dyn Any>`, `focus_scope.rs:53`) | `FocusNodePayload` | `Context` зарезервирован за типами-контекстами (N1 владельца) | — | 15 | Н |

### flui-scheduler, flui-semantics, flui-animation, flui-layer

| # | Было | Стало | Почему | alias | Ссылок | Риск |
|---|---|---|---|---|---|---|
| S1 | `TickerCanceled`, варианты `Canceled` (`ticker.rs:1343,1395`) | `TickerCancelled`, `Cancelled` | Одно написание (N6), 305 вхождений `Cancelled` | — | 42 | Н |
| SE1 | `SemanticsEventData::get_string`, `get_int`, `get_float`, `get_bool` | `str`, `int`, `float`, `bool` | C-GETTER; ср. `serde_json::Value::as_str` | — | 1 каждый | Н |
| SE2 | `SemanticsActionHandler` (`action.rs:52`) | `SemanticsActionCallback` | N6 | — | 27 | Н |
| A1 | `Curves` с `pub const EaseIn: Cubic` и ещё 19 используемыми константами | модуль `curves`: `EASE_IN`, `LINEAR`, `FAST_OUT_SLOW_IN`, … | «Статический класс» Dart, `#[expect(non_upper_case_globals)]`; прецедент `f64::consts::PI` | на модуле `Curves`, на константах `easeIn`, … | 171 | С |
| A3 | `Curve2D`, `Curve2DSample` | `Curve2d`, `Curve2dSample` | C-CASE | — | 3 | Н |
| A4 | `ext::{AnimatableExt, AnimationExt}`, `tween_types::*`, `CurveExt` в `tween_types.rs` | `animatable::AnimatableExt`, `animation::AnimationExt`, `tween::*`, `curve::CurveExt` | `ext`, `_types` — мешки (N1, N7) | — | 6–7 | Н |
| L1 | `ClipRRectLayer`; `clip_rrect()` | `ClipRoundedRectLayer`; `rounded_rect()` | Следует за F1 | `ClipRRectLayer` | 23 | Н |
| L2 | `ClipSuperellipseLayer` (держит `RSuperellipse`) | `ClipRoundedSuperellipseLayer` | Одно имя геометрии (N6) | — | ~10 | Н |
| L3 | `PictureLayer` | `DisplayListLayer` | Запись называется `DisplayList` (N6), «picture» — словарь Skia | `PictureLayer` | 102 | С |
| L4 | `LayerTree::get_layer`; `testing::inspect::clip_rrects` | `layer`; `clip_rounded_rects` | C-GETTER; F1 | — | 10 | Н |

### flui-rendering

Контексты (N1) — design.md §3; здесь только итоговые строки для подсчёта.

| # | Было | Стало | Почему | alias | Ссылок | Риск |
|---|---|---|---|---|---|---|
| R1 | `BoxLayoutCtx`, `SliverLayoutCtx`, `BoxHitTestCtx`, `SliverHitTestCtx` | `RawBoxLayoutContext`, `RawSliverLayoutContext`, `RawBoxHitTestContext`, `RawSliverHitTestContext` | N1; прецедент `RawWaker`/`Waker` | — | 80/27/31/12 | С |
| R2 | трейты `BoxLayoutCtxErased`, `SliverLayoutCtxErased` | `ErasedBoxLayoutContext`, `ErasedSliverLayoutContext` | N1; словарь `Erased*` для объектно-безопасной формы | — | 51/27 | С |
| R3 | `ErasedBoxLayoutCtx`, `ErasedSliverLayoutCtx` (драйверная реализация) | `DriverBoxLayoutContext`, `DriverSliverLayoutContext` | Освобождает `Erased*` для трейта R2; док: «driver-native implementation» | — | 13/10 | Н |
| R4 | `BoxIntrinsicsCtx`, `BoxDryLayoutCtx`, `BoxDryBaselineCtx` | `BoxIntrinsicsContext`, `BoxDryLayoutContext`, `BoxDryBaselineContext` | N1 | — | 212/92/72 | В |
| R5 | `PaintCx` (`context/paint_cx.rs:351`) | `PaintContext` | N1 | `PaintingContext` | 190 | С |
| R6 | `TextCx` (`pipeline/text_context.rs:255`) | `TextContextMut` | N1; `TextContext` уже занят, это `RefMut` на него | — | 28 | Н |
| R7 | `ContainerParentDataMixin<ChildId>`, файл `container_mixin.rs` | `SiblingLinks<ChildId>`, `sibling_links.rs` | `Mixin` (N8) | `ContainerParentDataMixin` | 31 | С |
| R8 | `SliverMultiBoxAdaptorParentData` | `SliverMultiBoxAdapterParentData` | Одно написание `Adapter` (N6) | `SliverMultiBoxAdaptorParentData` | 80 | С |
| R9 | `ActualBaselineChildCallback` | `BaselineChildCallback` | `Actual` — слово Flutter | — | ~5 | Н |
| R10 | `SliverGridLayout::get_scroll_offset_of_child`, `get_cross_axis_offset_of_child`, `get_min_child_index_for_scroll_offset`, `get_max_child_index_for_scroll_offset` | `child_scroll_offset`, `child_cross_axis_offset`, `first_index_at`, `last_index_at` | C-GETTER | — | 4/3/2/2 | Н |
| R11 | `RenderTree::get_two_mut`, `get_parent_and_children_mut`, `get_subtree_mut` | `get_disjoint_mut`, `parent_and_children_mut`, `subtree_mut` | std `slice::get_disjoint_mut` | — | 12/9/19 | С |
| R12 | `MultiChildLayoutDelegate`, `SingleChildLayoutDelegate`, `FlowDelegate` | `MultiChildLayout`, `SingleChildLayout`, `FlowLayout` (**О3**) | Трейт называется ролью; `Delegate` — паттерн Cocoa/Dart | `MultiChildLayoutDelegate`, … | 39/31/44 | С |
| R13 | `SliverGridDelegate`, `SliverGridDelegateWithFixedCrossAxisCount`, `SliverGridDelegateWithMaxCrossAxisExtent` | `GridTiling`, `FixedCrossAxisCount`, `MaxCrossAxisExtent` (**О3**) | Длинные имена Dart; ср. Compose `GridCells.Fixed/Adaptive` | исходные | 40/18/9 | Н |
| R14 | `CenterLayoutDelegate`, `AspectRatioDelegate` | `CenterLayout`, `AspectRatioLayout` (**О3**) | За R12 | — | 5/6 | Н |
| R15 | `RenderAbstractViewport` | `RevealViewport` | `Abstract` — модификатор класса Dart; трейт отвечает на «куда прокрутить, чтобы показать» | `RenderAbstractViewport` | 5 | Н |
| R16 | `pub mod traits` (`render_object`, `render_box`, `render_sliver`); `pub mod test_support` (`NoopSliver`); `pub mod delegates` | модули на уровень выше; `testing::noop_sliver`; `custom_layout` (по **О3**) | N1, N7 | — | — | С (пути) |

### flui-objects

| # | Было | Стало | Почему | alias | Ссылок | Риск |
|---|---|---|---|---|---|---|
| O1 | `RenderClipRRect` | `RenderClipRoundedRect` | F1; строка реестра `render_object_harness.rs:180` | `RenderClipRRect` | 30 | С |
| O2 | `RenderMetaData`, `MetaDataPayload`, `interaction/meta_data.rs` | `RenderMetadata`, `MetadataPayload`, `metadata.rs` | std `fs::Metadata`; `lib.rs:80` | `RenderMetaData` | 29/11 | С |
| O3 | `RenderPhysicalModelBase`, `RenderSliverFloatingHeaderBase`, `PersistentHeaderCoreView` | `RenderPhysicalClip<S>`, `RenderSliverFloatingHeader<M>`, `PersistentHeaderLayoutView` | `Base`, `Core` | — | 17/16/7 | Н |
| O4 | `RenderListener` | `RenderPointerListener` | За W4; совпадает с именем Flutter, alias не нужен | — | 27 | Н |
| O5 | `RenderTheater` | `RenderOverlayStack` | Внутреннее слово Flutter (`_Theater`); это стек оверлея | — | 37 | С |

### flui-view

| # | Было | Стало | Почему | alias | Ссылок | Риск |
|---|---|---|---|---|---|---|
| V1 | `EventCx<'a>` (`reactive/writer.rs:95`) | `EventContext<'a>` | N1 | — | 509 | В (ADR-0086, 4 ADR, 9 спек) |
| V2 | `ElementBase` | `ErasedElement` | Корень иерархии классов; словарь `Erased*` | — | 206 | В |
| V3 | `StatelessElementBase`, `StatefulElementBase`, `ProxyElementBase`, `InheritedElementBase`, `RenderElementBase<A>`, `RootElementBase`, `NotificationElementBase` | `element::kind::{Stateless, Stateful, Proxy, Inherited, Render<A>, Root, Notification}` | Метки варианта, а не базы (ср. `std::marker`) | — | 46 | С |
| V4 | `ElementCore<V, A>`, модуль `element::generic` | `TypedElement<V, A>`, `element::typed` | `Core`, `generic` (N1) | — | 157 | С |
| V5 | `ElementExt` | — | Пустой трейт без реализаций | — | 4 | Н |
| V6 | `RootElementImpl` | `ElementTreeRoot` | `Impl` | — | 9 | Н |
| V7 | `InheritedData` + derive `InheritedData` (flui-macros) | `InheritedFields` + derive `InheritedFields` | `Data` у трейта значений по полям | — | 45 | С (SDK) |
| V8 | `SlotInfo` | `SlotSnapshot` | `Info` | — | 8 | Н |
| V9 | `SliverMultiBoxAdaptor`, `element/sliver_adaptor.rs` | `SliverMultiBoxAdapter`, `sliver_adapter.rs` | N6 | `SliverMultiBoxAdaptor` | 57 | С |
| V10 | `SliverPersistentHeaderDelegate`, `SharedHeaderDelegate` | `PersistentHeaderContent`, `SharedHeaderContent` (**О3**) | Трейт строит содержимое и сообщает экстенты | `SliverPersistentHeaderDelegate` | 23/8 | Н |
| V11 | `FutureBuilder`, `FutureBuilderState`, `StreamBuilder`, `StreamBuilderState` | `FutureView`, `FutureViewState`, `StreamView`, `StreamViewState` (**О2**) | `*Builder` в Rust — паттерн C-BUILDER | `FutureBuilder`, `StreamBuilder` | 112/58 | С |
| V12 | `LayoutBuilder` | `ConstraintsView` (**О2**) | То же | `LayoutBuilder` | 136 | С |
| V13 | `ErrorViewBuilder`, `SnapshotBuilder<T, E>` (псевдонимы `fn`/`Rc<dyn Fn>`) | `ErrorViewFn`, `SnapshotViewFn<T, E>` (**О2**) | То же | — | ~10 | Н |
| V14 | `tree::test_utils` (`ReconcileEventCollector`) | `tree::reconcile_log` | `utils` | — | 9 | Н |
| V15 | фича `test-utils` | `testing` | N9 | — | 9 | С (Cargo.toml в flui-runtime) |

### flui-widgets

| # | Было | Стало | Почему | alias | Ссылок | Риск |
|---|---|---|---|---|---|---|
| W1 | `MetaData`, `interaction/meta_data.rs` | `Metadata`, `metadata.rs` | std `fs::Metadata` | `MetaData` | 14 | Н |
| W2 | `ClipRRect` | `ClipRoundedRect` | F1 | `ClipRRect` | 46 | С |
| W3 | `ClipRSuperellipse` (виджет, если публичен) | `ClipRoundedSuperellipse` | F2 | `ClipRSuperellipse` | 7 | Н |
| W4 | `Listener` | `PointerListener` | В Rust «listener» читается как наблюдатель; это сырые события указателя | `Listener` | 161 (часть — проза) | С |
| W5 | `MediaQueryData` / `MediaQuery` | `MediaQuery` (значение) / `MediaQueryScope` (виджет) (**О1**, swap) | Правило «значение — существительное, поставщик — `…Scope`» | `MediaQueryData` на значении | 131/272 | В |
| W6 | `IconThemeData` / `IconTheme` | `IconTheme` / `IconThemeScope` (**О1**, swap) | То же | `IconThemeData` | 57/68 | С |
| W7 | `DefaultTextStyle` | `TextStyleScope` | `Default*` — поставщик значения (правило О1) | `DefaultTextStyle` | 74 | С |
| W8 | `Directionality` | `TextDirectionScope` | То же; имя говорит, что поставляется | `Directionality` | 144 | С |
| W9 | `DefaultFocusTraversal`, `DefaultFocusTraversalState` | `FocusTraversalScope`, `FocusTraversalScopeState` | То же | `DefaultFocusTraversal` | 36 | Н |
| W10 | `IconData` | `IconGlyph` | `Data` — шум; это кодовая точка и семейство шрифта | `IconData` | 66 | С |
| W11 | `ImageProvider`, `ImageProviderError`, `DirectImageProvider` | `ImageSource`, `ImageSourceError`, `DirectImageSource` | egui `ImageSource` | `ImageProvider` | 126 | С |
| W12 | `WidgetState`, `WidgetStateProperty<T>`, `WidgetStatesController`, `WidgetStateConstraint`; модуль `widget_state` | `InteractionState`, `InteractionValue<T>`, `InteractionStates`, `InteractionStateConstraint`; `interaction_state`. Альт.: `PerInteraction<T>` | В FLUI вещь — `View`; «widget state» здесь — hover/pressed/focused | `WidgetState`, `WidgetStateProperty`, `MaterialState` | 640 (семейство) | В |
| W13 | `Hero`, `HeroController`, `HeroControllerScope`, `HeroControllerProbe`, `HeroFlight`, `HeroFlightManifest`, `HeroHandle`, `HeroMode`, `HeroRegistry`, `HeroScope`, `HeroState`, `HeroTag`; `FlightManager` | `SharedElement*` (те же суффиксы); `SharedElementFlights` (скрыть из поверхности) (**О4**) | Коинедж Flutter; Android/Compose: shared element transition | `Hero`, … | 554 (семейство) / 35 | В |
| W14 | `AnimatedBuilder`, `AnimatedBuilderState`, `ValueListenableBuilder`, `ValueListenableBuilderState` | `ListenableView`, `ListenableViewState`, `ValueListenableView`, `ValueListenableViewState` (**О2**) | C-BUILDER | исходные | 79/22 | С |
| W15 | псевдонимы `ValueWidgetBuilder<T>`, `DragTargetBuilder<T>`, `AppBuilder`, `ViewportBuilder`, `AnimatedSwitcherTransitionBuilder`, `AnimatedSwitcherLayoutBuilder`, `RouteContentBuilder`, `RoutePageBuilder`, `RouteTransitionsBuilder` | `ValueViewFn<T>`, `DragTargetViewFn<T>`, `AppWrapperFn`, `ViewportFn`, `SwitcherTransitionFn`, `SwitcherLayoutFn`, `RouteContentFn`, `RoutePageFn`, `RouteTransitionsFn` (**О2**) | C-BUILDER; `Widget` вместо `View` | — | ~60 | С |
| W16 | `SliverChildBuilderDelegate` | `LazyChildren` (**О3**). Альт.: `ChildrenByIndex` | Дети по индексу, лениво; ср. Compose `LazyColumn` | `SliverChildBuilderDelegate` | 22 | Н |
| W17 | `LocalizationsDelegate`, `BoxedLocalizationsDelegate`, `DefaultWidgetsLocalizationsDelegate`, `GlobalWidgetsLocalizationsDelegate` | `LocalizationsLoader`, `BoxedLocalizationsLoader`, `DefaultWidgetsLocalizationsLoader`, `GlobalWidgetsLocalizationsLoader` (**О3**) | Объект загружает локализацию для локали — имя по действию | `LocalizationsDelegate` | 75 | С |
| W18 | `SingleActivator` | `Keystroke` | GPUI `Keystroke` (клавиша + модификаторы). Альт.: `KeyChord` | `SingleActivator` | 39 | Н |
| W19 | `TextFormFieldCore`, `form/text_form_field_core.rs` | `TextFormFieldModel`, `text_form_field_model.rs` | `Core`; закреплён в `flui-sdk/tests/surface.rs:68` | — | 17 | С |
| W20 | `FocusChangeHandler` | — (`FocusChangeCallback`) | N6 | — | 8 | Н |
| W21 | `DraggableCanceledDetails` | `DraggableCancelledDetails` | N6 | — | 13 | Н |
| W22 | `ErasedDragData` | `ErasedDragPayload` | `Data`; пара к `MetadataPayload` | — | ~10 | Н |

### flui-runtime, flui-app, flui-testing, flui-assets, flui-devtools, flui-hot-reload

| # | Было | Стало | Почему | alias | Ссылок | Риск |
|---|---|---|---|---|---|---|
| RT1 | `FrameFailureHandler` (обёртка `Arc<dyn Fn>`) | `FrameFailureCallback` | N6 | — | 38 | Н |
| RT2 | фича `test-support` (flui-runtime) | `testing` | N9; dev-зависимости flui-app, flui-testing | — | 11 | С |
| AP1 | `CloseRequestHandler` (обёртка `Fn`) | `CloseRequestCallback` | N6 | — | 23 | Н |
| T1 | `flui_testing::text_store_kit` (`assert_conforms`) | `flui_testing::text_store_conformance` | `kit`; ADR-0090 §4, AGENTS «Extending FLUI» | — | 50 | В (ADR-0090, ADR-0092, AGENTS) |
| AS1 | `AssetCacheCore`, `AssetHandleCore` | `ErasedAssetCache`, `ErasedAssetHandle` | `Core`; `AssetCache` уже занят структурой, словарь `Erased*` | — | 10/16 | Н |
| AS2 | `FontData` (`types/font_data.rs`) | `FontFile` | `Data`; `FontBytes` занят псевдонимом flui-painting | — | 21 | Н |
| AS3 | `pub mod core`, `pub mod types` | приватные `asset`, `key`, `handle`, `state` + реэкспорт | Мешки (N1, N7) | — | — | С (пути) |
| DT1 | `get_events`, `get_events_by_category`, `get_events_in_range`; `PhaseInfo` | `events`, `events_in`, `events_between`; `PhaseTiming` | C-GETTER; `Info` | — | 14/8 | Н |
| HR1 | `get_worker_build_ptr` | `worker_build_ptr` | C-GETTER | — | ~3 | Н |

### flui-material

| # | Было | Стало | Почему | alias | Ссылок | Риск |
|---|---|---|---|---|---|---|
| M1 | `ThemeData` / `Theme`; `ThemeDataOverrides`; модуль `theme_data` | `Theme` (значение) / `ThemeScope` (виджет); `ThemeOverrides`; слить в `theme` (**О1**, swap) | Значение — существительное (iced, GPUI, egui `Style`); поставщик — `…Scope`, как `HeroScope`, `VsyncScope` | `ThemeData` на `Theme`; на `ThemeScope` без alias | 350/374 | В |
| M2 | `AppBarThemeData`, `CardThemeData`, `CheckboxThemeData`, `ChipThemeData`, `DataTableThemeData`, `DialogThemeData`, `DividerThemeData`, `ElevatedButtonThemeData`, `FilledButtonThemeData`, `IconButtonThemeData`, `InputDecorationThemeData`, `NavigationBarThemeData`, `OutlinedButtonThemeData`, `RadioThemeData`, `SwitchThemeData`, `TabBarThemeData`, `TextButtonThemeData` | `*Theme` (`AppBarTheme`, …) (**О1**) | То же | исходные | 281 (семейство) | С |
| M3 | `FabThemeData` | `FloatingActionButtonTheme` | Без сокращения, одно слово на понятие | `FabThemeData` | 11 | Н |
| M4 | `Material`, `MaterialShape`; модуль `material::material` | `Surface`, `SurfaceShape`; `surface` (**О4**) | Material 3 и Compose: surface; модуль `material::material` — `module_inception` | `Material` | 16 вызовов `Material::new`; слово — 782, в основном проза | В |
| M5 | `InkWell`, `InkWellState`; модуль `ink_well` | `Ripple`, `RippleState`; `ripple` (**О4**) | Коинедж Flutter; Compose `ripple()` | `InkWell` | 227 | В |
| M6 | `SnackBar`, `SnackBarAction`, `SnackBarActionState`, `SnackBarClosedReason`, `SnackBarController`, `SnackBarThemeData`; модуль `snack_bar` | `Snackbar*`, `SnackbarTheme`; `snackbar` | Одно слово в Material 3 и Compose (C-CASE) | `SnackBar`, … | 137 | С |
| M7 | `ScaffoldMessenger`, `ScaffoldMessengerHandle`, `ScaffoldMessengerScope`, `ScaffoldMessengerState`; модуль `scaffold_messenger` | `SnackbarHost`, `SnackbarHostHandle`, `SnackbarHostScope`, `SnackbarHostState`; `snackbar_host` (**О4**) | Compose `SnackbarHost`; имя говорит, что держит | `ScaffoldMessenger` | 133 | С |
| M8 | `DefaultTabController`, `DefaultTabControllerState` | `TabControllerScope`, `TabControllerScopeState`; приватный `TabControllerScope` → `InheritedTabController` | Правило О1 | `DefaultTabController` | 67 | С |
| M9 | `ListTile`, `ListTileThemeData`; модуль `list_tile` | `ListItem`, `ListItemTheme`; `list_item`. Альт.: оставить | Material 3 и Compose M3 `ListItem` | `ListTile` | 96 | С |
| M10 | `FlexibleSpaceBarData`, `FlexibleSpaceBarSettings` | `FlexibleSpaceBarExtent`, `FlexibleSpaceBarScope` | `Data`; правило О1 | `FlexibleSpaceBarSettings` | ~15 | Н |

### flui-cupertino

| # | Было | Стало | Почему | alias | Ссылок | Риск |
|---|---|---|---|---|---|---|
| C1 | `CupertinoThemeData` / `CupertinoTheme`; `CupertinoTextThemeData` | `CupertinoTheme` / `CupertinoThemeScope`; `CupertinoTextTheme` (**О1**, swap) | Правило О1 | `CupertinoThemeData`, `CupertinoTextThemeData` | 75 (семейство) | С |
| C2 | `CupertinoColors` (unit struct с константами) | константы уровня модуля `colors::{WHITE, SYSTEM_BLUE, …}` | «Статический класс» Dart, как `Curves` (N8) | `CupertinoColors` на модуле | 45 | Н |

### Фасад `flui`, flui-sdk, flui-macros

| # | Было | Стало | Почему | Риск |
|---|---|---|---|---|
| FA1 | реэкспорты `src/interaction.rs` (`RecognizerBase`, `GestureRecognizerExt`, `PointerEventExt`, `FocusManager`, …), `src/rendering.rs` (`PaintCx`, `BoxIntrinsicsCtx`, `BoxDryLayoutCtx`, `SliverMultiBoxAdaptorParentData`, …), `src/lib.rs` (`ThemeData`, `Scaffold` в доке) | по строкам выше | Следуют за определениями | С |
| SDK1 | пины `flui-sdk/tests/surface.rs` (`WidgetsBinding`, `BorderRadiusExt`, `TextFormFieldCore`, …) | по строкам выше | R12: тест поверхности падает, пока список не обновлён | С |
| MA1 | derive `InheritedData` | `InheritedFields` (V7) | — | С |

**Итого: 130 строк** (F 7, PA 1, PL 8, PT 11, I 13, S/SE/A/L 11, R 16, O 5, V 15, W 22,
RT…HR 9, M 10, C 2; сводные строки фасада, SDK и macros не считаются). В штуках имён — **≈ 330
публичных идентификаторов**: семейства `IOS*`/`MacOS*` (19), `Hero*` (13), `*ThemeData` (17),
`SnackBar*`/`ScaffoldMessenger*` (11), 20 констант `Curves`, 40 методов `get_*`. Из них
удаляются ≈ 20 (непривязанная поверхность и дубли). От открытых вопросов зависят: О1 ≈ 35
имён, О2 ≈ 20, О3 ≈ 16, О4 ≈ 25, О5 — 3; без них проход остаётся ≈ 230 имён.

## 2. Публичные модули и Cargo-фичи

| # | Было | Стало | Почему | Риск |
|---|---|---|---|---|
| PM1 | `flui_foundation::callbacks` | — (F5) | Непривязанная поверхность | Н |
| PM2 | `flui_foundation::notifier_generic` | слить в `notifier` | N7 | Н |
| PM3 | `flui_foundation::geometry::{rrect, rsuperellipse, traits}` | `rounded_rect`, `rounded_superellipse`; `traits` → `units` (`Unit`, `NumericUnit`, `FloatUnit`) и `approx` | N1 | С |
| PM4 | `flui_platform::{shared, traits}` | `input` (events, gestures, keys, scroll), `panic_boundary`, `accessibility`; трейты в `platform`, `host_window` | N1; зависит только flui-app | С (160 `shared::`) |
| PM5 | `flui_interaction::traits` | — (I1, I2; `HitTestTarget` в `hit_test`) | N1 | С |
| PM6 | `flui_rendering::{traits, test_support, delegates}` | R16 | N1 | С |
| PM7 | `flui_animation::{ext, tween_types}` | A4 | N1 | Н |
| PM8 | `flui_assets::{core, types}` | AS3 | N1 | С |
| PM9 | `flui_view::tree::test_utils`, `flui_view::element::generic` | V14, V4 | N1 | Н |
| PM10 | `flui_testing::text_store_kit` | T1 | N1 | В |
| PM11 | `flui_widgets::widget_state` | `interaction_state` (W12) | — | С |
| PM12 | `flui_material::{theme_data, material, ink_well, snack_bar, scaffold_messenger, list_tile}` | M1, M4–M7, M9 | — | С |
| PF1 | `flui-view` фича `test-utils`, `flui-runtime` фича `test-support` | `testing` (V15, RT2) | N9; `--features` в CI, `.config/nextest.toml`, шаблоны `flui-cli`, `docs/testing.md` | С |

## 3. Остаются (решение «оставить», проверено)

Имя из Flutter, которое уже ясно Rust-разработчику без Flutter, остаётся; `#[doc(alias)]` не нужен.

| Имя | Почему остаётся |
|---|---|
| `Padding`, `Column`, `Row`, `Stack`, `Text`, `Center`, `Align`, `Wrap`, `Flex`, `Table`, `Icon`, `Image`, `Opacity`, `Transform`, `Spacer`, `AspectRatio`, `Baseline`, `SafeArea` | Обычные английские слова, те же в iced/SwiftUI/Compose |
| `Container` | iced `widget::Container` — тот же смысл (один ребёнок, отступы, выравнивание, фон). Альт. `Frame` не яснее |
| `SizedBox` | Читается как «коробка заданного размера»; `Sized` занят std. Альт. `FixedSize` хуже как распорка |
| `Expanded`, `Flexible`, `Positioned`, `Offstage` | Обёртки-причастия по прецеденту std `Wrapping<T>`, `Reverse<T>` |
| `ColoredBox`, `DecoratedBox`, `ConstrainedBox`, `UnconstrainedBox`, `LimitedBox`, `OverflowBox`, `SizedOverflowBox`, `FittedBox`, `FractionallySizedBox`, `RotatedBox` | «Box» здесь — прямоугольник протокола `RenderBox`; с `Box<T>` не путается, потому что всегда с определением |
| `Scaffold`, `AppBar`, `FloatingActionButton`, `Card`, `Chip`, `Divider`, `Drawer`, `Switch`, `Radio`, `Checkbox`, `Slider`, `DataTable` | Словарь Material 3 и Compose |
| `Sliver*`, `RenderObject`, `RenderBox`, `RenderSliver`, `ParentData`, `Element`, `BuildContext`, `LifecycleContext` | Ядро модели FLUI (requirements N8, исключения); `ParentData` — одно понятие протокола, альт. `ChildLayoutData` длиннее и не точнее |
| `InheritedView`, `InheritedElement`, `InheritedTheme` | «Наследуется потомками» — обычное английское слово (ср. CSS inheritance); `Context`/`Provider` заняты |
| `ChangeNotifier`, `Listenable`, `ValueNotifier`, `ValueListenable`, `ListenerCallback` | Паттерн наблюдателя, слова не из Dart |
| `EdgeInsets`, `BorderRadius`, `Matrix4`, `BoxDecoration`, `Alignment`, `Offset` | UIKit `UIEdgeInsets`, nalgebra `Matrix4`. Альт. `Insets` (kurbo) — отвергнут: 308 ссылок ради синонима |
| `Diagnosticable`, `DiagnosticsNode` | Обычные слова; derive в `flui-macros` |
| `GestureDetector`, `GestureArena`, `GestureRecognizer`, `*Details`, `*Callback` | Обычные слова |
| `Navigator`, `Route`, `Router`, `ModalRoute`, `PageRoute`, `PopScope` | Обычные слова |
| `AnimationController`, `Tween`, `Curve`, `CurvedAnimation`, `TickerProvider`, `Vsync` | Словарь анимации, не Dart |
| `BuildOwner`, `PipelineOwner`, `WidgetsBinding`, `GestureBinding`, `RendererBinding`, `RenderingBinding`, `HeadlessBinding` | Семейство уйдёт или сменит смысл при переносе рантайма (ADR-0083); переименование сейчас — двойная волна. Запись allowlist `exit = "keep"` с причиной; пересмотр — в спеке переноса |
| `BorderRadiusExt`, `SignalWriteExt`, `BuildContextExt`, `ViewExt`, `AssetCacheExt` | N5: расширение типа другого крейта (`Corners` из flui-foundation) или граница возможности/объектной безопасности (В2 закрывается правилом N5) |
| `CustomPaint`, `CustomPainter`, `CustomClipper` | Обычные слова; трейт назван ролью |
| `MaterialApp`, `CupertinoApp`, `WidgetsApp`, `WidgetsLocalizations`, префикс `Cupertino*` | Различают приложения и пакеты при общем `prelude`; `Widgets` — имя слоя |
| `TextEditingController`, `ScrollController`, `PageController`, `TabController` | «Controller» — обычное слово |
| `LayoutContextApi`, `HitTestContextApi` | Трейты, которые реализуют `Raw*Context`; суффикс `Api` спорный, но не мешок. Пересмотр — вне прохода |

## 4. Внутренние переименования (не ломают API)

| # | Было | Стало | Почему | Ссылок |
|---|---|---|---|---|
| X1 | привязки `ctx` (параметры и локальные) любого типа `…Context` | `cx` | N1 владельца | 3559 в 343 файлах |
| X2 | время жизни `'ctx` | `'cx`; где уже есть `'cx` (`FlowPaintingContext<'ctx, 'cx>`) — вручную | Следует за X1 | 230 |
| X3 | `flui-rendering/src/context/paint_cx.rs` | `context/paint.rs` | R5 | 1 |
| X4 | тестовые модули `event_cx` (×4), `event_cx_tests` в `flui-widgets/tests/` | `event_context` | V1 | 5 |
| X5 | фикстура `flui-view/tests/ui/unit_closure_where_event_cx_expected.{rs,stderr}` | `unit_closure_where_event_context_expected` | V1 | 2 |
| X6 | `element/behavior_commons.rs` | `element/build_cycle.rs` | `commons` | 16 |
| X7 | `element/unified.rs` | `element/behavior_element.rs` | `unified` — история | — |
| X8 | `flui-widgets/src/support.rs` + `support/*` | `callbacks.rs`, `render_view_macro.rs`; раннеры в `testing/` под `cfg(test)` | мешок | 51 |
| X9 | `tools/xtask/src/util.rs` | `repo.rs`, `adr.rs`, `scratch.rs`, `size.rs` | мешок | 72 |
| X10 | `platforms/windows/util.rs` | `win32_convert.rs` | мешок | 6 |
| X11 | `flui-cli/src/build/util/`, `flui-cli/src/types.rs` | `build/host/`, `project_spec.rs` | мешок | 12 |
| X12 | `flui-engine/src/test_support.rs`, `flui-log/src/test_support.rs`, `flui-app/src/app/window_test_support.rs` | `gpu_fixture.rs`, `scoped_subscriber.rs`, `test_window.rs` | мешок | 179/2/5 |
| X13 | `flui-engine/src/shaders/common/`, `blend_helpers.wgsl` | `shaders/include/`, `blend_math.wgsl` (+ `wgsl.rs:436`) | мешок | 89 |
| X14 | `parent_data/{base,box_variants,sliver_variants,table_text}.rs` | `mod.rs`, `box_layouts.rs`, `sliver_layouts.rs`, `table.rs` + `text.rs` | мешок | 16 |
| X15 | `lerp_impls.rs`, `seq/tuple_impls.rs`, `seq/vec_impls.rs` | `lerp.rs`, `seq/tuple.rs`, `seq/vec.rs` | `_impls` | 11 |
| X16 | `tests/support/` (5 крейтов), `tests/public_support/`, `tests/common/` (4 крейта) | по содержимому: `child_process.rs`, `row_runner/`, `recovery_fixtures/`, `mcp_client/`, `harness.rs` | мешки | 36 |
| X17 | `flui-assets/tests/network_support.rs`, `examples/support/ai_http.rs`, `flui-view/benches/shared/`, `flui-rendering/benches/helpers.rs` | `local_http_server.rs`, `examples/ai_streaming/http.rs`, `benches/mocks/`, `benches/tree_builders.rs` | мешки | 6 |
| X18 | пакет `hot-reload-counter-types` | `hot-reload-counter-model` | `types` | — |
| X19 | `run_app_impl`, `run_app_with_config_impl` | `run_app`, `run_app_with_config` в месте определения | `_impl` | 7 |
| X20 | файлы переименованных публичных типов: `rrect.rs`, `clip_rrect.rs` (×3), `meta_data.rs` (×2), `widget_state.rs`, `hero*.rs`, `future_builder.rs`, `stream_builder.rs`, `layout_builder.rs`, `ink_well.rs`, `snack_bar.rs`, `list_tile.rs`, `container_mixin.rs`, `text_form_field_core.rs`, `sliver_adaptor.rs`, `font_data.rs`, `theme_data.rs` | по новому имени типа | Следуют за §1 | — |

## 5. Имена тестов

Перед переименованием — `rg <имя>` по `docs/`, ADR, `ARCHITECTURE.md` (AGENTS «Test names are references»).

| # | Было | Стало | Где цитируется |
|---|---|---|---|
| TN1 | `test_scale_calculation`, `test_double_tap_timing`, `test_two_finger_tap`, `test_team_captain_wins`, `test_reentrancy_remove_self` | `scale_is_span_ratio_of_two_pointers`, `second_tap_after_timeout_starts_a_new_sequence`, `two_finger_tap_fires_once`, `team_captain_wins_the_arena`, `route_removing_itself_during_dispatch_is_skipped` (формулировка — по телу) | — |
| TN2 | `test_jank_detection`, `test_registry_invalidate`, `test_full_frame_lifecycle`, `test_compound_animation_status`, `test_concurrent_access_is_crash_free_and_serialized`, `test_scaffold_platform_plan_matches_scaffold_platform`, `test_access_surface_is_pinned` | без `test_`, исходом: `frame_over_budget_is_reported_as_jank`, `invalidated_key_reloads`, `frame_runs_phases_in_order`, `compound_status_follows_dominant_parent`, остальные — снять префикс | — |
| TN3 | `flui-scheduler/tests/integration_tests.rs` | `frame_lifecycle.rs` | — |
| TN4 | `ui_tests` (`flui-view/tests/trybuild_ui.rs:26`) | `trybuild_ui` | `docs/testing.md` |
| TN5 | `cancelling_renderer_new` | `cancelled_renderer_construction_releases_the_surface` (по телу) | `flui-engine/ARCHITECTURE.md` |
| TN6 | `unbound_generics` | по телу теста | — |
| TN7 | `text_store_kit_conformance`, `text_store_kit_matrix`, файл `flui-testing/tests/text_store_kit.rs` | `text_store_conformance`, `text_store_conformance_matrix`, `text_store_conformance.rs` | `flui-testing/ARCHITECTURE.md` |
| TN8 | `notifier_generic_contract`, `tween_types_contract` | строка в `notifier_contract`, `tween_contract` | — |
| TN9 | `exit_codes`, `completions_bash` | `exit_code_matches_error_kind`, `bash_completions_name_every_subcommand` | — |
| TN10 | фикстура `let_bound_event_closure_without_helper.rs` + `.stderr` | `let_bound_event_closure_without_adapter` | authoring-styles requirements |
| TN11 | `harness_clip_rrect_wraps_child`, `bench_clip_rrect_radius_change`, `clip_rrect_*` тесты | `harness_clip_rounded_rect_wraps_child`, … | `flui-objects` `## Mapping decisions` — проверить `rg` |
| TN12 | `flui_testing_it` | оставить (`docs/testing.md:516`) | — |
