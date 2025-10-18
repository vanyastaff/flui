# Glossary Types Mapping - Куда идут типы из glossary

> Подробная карта распределения 3500+ типов из glossary по крейтам Flui

## 🎯 Обзор

После анализа всех файлов glossary (animation.md, foundation.md, gestures.md, material.md, painting.md, physics.md, rendering.md, scheduler.md, semantics.md, service.md, widgets.md) обнаружено **более 3500 типов**.

Этот документ определяет, какие типы должны попасть в `flui_types` (базовый крейт), а какие - в другие специализированные крейты.

## 📦 Распределение по крейтам

### flui_types - Базовые типы (приоритет: CRITICAL)

**Критерий:** Примитивные типы данных без зависимостей, используемые повсеместно

#### ✅ Geometry (РЕАЛИЗОВАНО - 1910 строк, 68 тестов)
- ✅ Point, Rect, Size, Offset
- ✅ RRect (rounded rect) - РЕАЛИЗОВАНО

#### ✅ Layout (РЕАЛИЗОВАНО - 2136 строк, 49 тестов)
- ✅ Axis, AxisDirection, Orientation, VerticalDirection
- ✅ EdgeInsets, EdgeInsetsDirectional, EdgeInsetsGeometry
- ✅ Alignment, AlignmentDirectional, AlignmentGeometry
- ✅ MainAxisAlignment, CrossAxisAlignment, MainAxisSize
- ✅ BoxFit, BoxShape
- ✅ FlexFit, WrapAlignment, WrapCrossAlignment

#### ✅ Styling (РЕАЛИЗОВАНО - 3287 строк, 116 тестов)
**Источники:** painting.md, material.md

**Color system:**
- ✅ Color (RGBA)
- ❌ ColorSwatch<T> (планируется позже)
- ✅ HSLColor, HSVColor
- ✅ Colors константы (Material Design palette - MaterialColors)

**Borders:**
- ✅ BorderSide, BorderStyle
- ✅ Radius
- ✅ Border, BorderDirectional
- ✅ BorderRadius, BorderRadiusDirectional
- ✅ BoxBorder (trait)

**Border shapes:**
- ✅ ShapeBorder (trait)
- ✅ RoundedRectangleBorder
- ✅ BeveledRectangleBorder
- ✅ CircleBorder
- ✅ OvalBorder
- ✅ StadiumBorder
- ✅ StarBorder
- ✅ ContinuousRectangleBorder
- ❌ RoundedSuperellipseBorder (планируется позже)
- ✅ LinearBorder, LinearBorderEdges

**Shadows:**
- ✅ Shadow
- ✅ BoxShadow

**Gradients:**
- ✅ Gradient (enum)
- ✅ LinearGradient
- ✅ RadialGradient
- ✅ SweepGradient
- ✅ GradientTransform (trait), GradientRotation
- ✅ TileMode

**Decorations:**
- ✅ Decoration (trait)
- ✅ BoxDecoration
- ❌ ShapeDecoration (планируется позже)
- ✅ DecorationImage
- ✅ BoxFit, ImageRepeat
- ✅ ColorFilter, BlendMode (30+ вариантов)

#### ✅ Typography (РЕАЛИЗОВАНО - 983 строки, 50 тестов)
**Источники:** painting.md, material.md, widgets.md

**Text styling:**
- ✅ TextStyle
- ✅ StrutStyle
- ✅ FontWeight, FontStyle
- ✅ FontFeature, FontVariation
- ✅ TextShadow

**Text alignment:**
- ✅ TextAlign
- ✅ TextAlignVertical
- ✅ TextBaseline
- ✅ TextDirection
- ✅ TextAffinity

**Text decoration:**
- ✅ TextDecoration
- ✅ TextDecorationStyle
- ✅ TextOverflow
- ✅ TextWidthBasis
- ✅ TextHeightBehavior
- ✅ TextLeadingDistribution
- ✅ TextDecorationConfig

**Text metrics:**
- ✅ TextPosition
- ✅ TextRange
- ✅ TextSelection
- ✅ TextBox
- ✅ GlyphInfo
- ✅ LineMetrics

**Text spans:**
- ✅ InlineSpan, InlineSpanTrait
- ✅ TextSpan
- ✅ PlaceholderSpan
- ✅ PlaceholderDimensions
- ✅ PlaceholderAlignment
- ✅ MouseCursor

#### ✅ Painting (РЕАЛИЗОВАНО - 1048 строк, 62 теста)
**Источники:** painting.md, rendering.md

**Image handling:**
- ✅ BoxFit (Fill, Contain, Cover, FitWidth, FitHeight, None, ScaleDown)
- ✅ ImageRepeat (Repeat, RepeatX, RepeatY, NoRepeat)
- ✅ ImageConfiguration
- ✅ FittedSizes
- ✅ ColorFilter (Mode, Matrix, LinearToSrgbGamma, SrgbToLinearGamma)

**Clipping:**
- ✅ Clip (None, HardEdge, AntiAlias, AntiAliasWithSaveLayer)
- ✅ ClipBehavior
- ✅ NotchedShape (trait)
- ✅ AutomaticNotchedShape
- ✅ CircularNotchedRectangle

**Canvas primitives:**
- ✅ BlendMode (30+ режимів: Porter-Duff + розширені)
- ✅ TileMode (Clamp, Repeat, Mirror, Decal)
- ✅ BlurStyle (Normal, Solid, Outer, Inner)
- ✅ FilterQuality (None, Low, Medium, High)
- ✅ PaintingStyle (Fill, Stroke)
- ✅ PathFillType (NonZero, EvenOdd)
- ✅ PathOperation (Difference, Union, Intersect, Xor, ReverseDifference)
- ✅ StrokeCap (Butt, Round, Square)
- ✅ StrokeJoin (Miter, Round, Bevel)
- ✅ VertexMode (Triangles, TriangleStrip, TriangleFan)

**Shaders:**
- ✅ Shader (LinearGradient, RadialGradient, SweepGradient, Image)
- ✅ ImageShader
- ✅ MaskFilter

**Matrix utilities:**
- ❌ Matrix4 (можливо з glam або dart:ui - планується пізніше)
- ❌ MatrixUtils (утиліти, може бути в flui_rendering)
- ❌ RSTransform (планується пізніше)

#### ✅ Animation (РЕАЛИЗОВАНО - 1089 строк, 37 тестов)
**Источники:** animation.md

**Curve system:**
- ✅ Curve trait
- ✅ ParametricCurve<T>
- ✅ Curve2D, Curve2DSample

**Standard curves:**
- ✅ Linear (LinearCurve через Curve trait)
- ✅ SawTooth
- ✅ Interval
- ✅ Threshold

**Cubic curves:**
- ✅ Cubic (Bézier curves с бінарним пошуком)

**Elastic curves:**
- ✅ ElasticInCurve, ElasticOutCurve, ElasticInOutCurve

**Catmull-Rom:**
- ✅ CatmullRomCurve
- ✅ CatmullRomSpline

**Curve modifiers:**
- ✅ FlippedCurve
- ✅ ReverseCurve

**Predefined curves:**
- ✅ Curves (collection: 20+ variants - EaseIn, EaseOut, EaseInOut, FastOutSlowIn, BounceIn, etc.)

**Tween system:**
- ✅ Tween<T> trait
- ✅ Animatable<T> trait

**Concrete tweens:**
- ✅ FloatTween (Tween<f32>)
- ✅ IntTween (integer tween з округленням)
- ✅ StepTween (integer tween з floor)
- ✅ ConstantTween<T>
- ✅ ReverseTween<T>

**Geometric tweens:**
- ✅ ColorTween
- ✅ SizeTween
- ✅ RectTween
- ✅ OffsetTween
- ✅ AlignmentTween
- ✅ EdgeInsetsTween
- ✅ BorderRadiusTween

**Complex tweens:**
- ✅ TweenSequence (з FloatTween)
- ✅ TweenSequenceItem

**Animation status:**
- ✅ AnimationStatus (Dismissed, Forward, Reverse, Completed)
- ✅ AnimationBehavior (Normal, Preserve)

#### ✅ Physics (РЕАЛИЗОВАНО - 902 строки, 47 тестов)
**Источники:** physics.md

**Simulations:**
- ✅ Simulation (base trait)
- ✅ Tolerance (max допустимі величини)

**Friction:**
- ✅ FrictionSimulation
- ✅ BoundedFrictionSimulation

**Spring:**
- ✅ SpringDescription (constants: mass, stiffness, damping)
- ✅ SpringSimulation
- ✅ SpringType (Critical, Underdamped, Overdamped)
- ❌ ScrollSpringSimulation (не включено, буде в flui_gestures)

**Gravity:**
- ✅ GravitySimulation

**Utilities:**
- ✅ ClampedSimulation (обмежує іншу simulation)

#### ✅ Gestures Details (РЕАЛИЗОВАНО - 758 строк, 23 теста)
**Источники:** gestures.md

**Tap details:**
- ✅ TapDownDetails (global, local position, device kind)
- ✅ TapUpDetails (global, local position, device kind)
- ❌ TapMoveDetails (не включено, рідко використовується)

**Drag details:**
- ✅ DragStartDetails (time, global/local position, device kind)
- ✅ DragUpdateDetails (time, delta, primary_delta, global/local position)
- ✅ DragEndDetails (velocity, primary_velocity)
- ✅ DragDownDetails (global, local position)

**Scale details:**
- ✅ ScaleStartDetails (focal_point, pointer_count)
- ✅ ScaleUpdateDetails (focal_point, scale, rotation, horizontal/vertical scale)
- ✅ ScaleEndDetails (velocity, pointer_count)

**Long press details:**
- ✅ LongPressDownDetails (global/local position, device kind)
- ✅ LongPressStartDetails (global/local position)
- ✅ LongPressMoveUpdateDetails (global/local position, offset from origin)
- ✅ LongPressEndDetails (global/local position, velocity)

**Force press:**
- ✅ ForcePressDetails (global/local position, pressure, max_pressure)

**Velocity:**
- ✅ Velocity (pixels_per_second, magnitude, direction, clamp_magnitude)
- ✅ VelocityEstimate (offset, pixels_per_second, duration, confidence)
- ✅ OffsetPair (local + global offset pair)

**Pointer data:**
- ✅ PointerData (повна інформація про pointer state)
- ✅ PointerDeviceKind (Touch, Mouse, Stylus, InvertedStylus, Trackpad)

**Не включено:**
- ❌ SerialTapDownDetails (рідко використовується)
- ❌ SerialTapUpDetails (рідко використовується)
- ❌ SerialTapCancelDetails (рідко використовується)
- ❌ PositionedGestureDetails (abstract interface, буде в flui_gestures)

#### ✅ Constraints (РЕАЛИЗОВАНО - 1008 строк, 41 тест)
**Источники:** rendering.md

**Box constraints:**
- ✅ BoxConstraints (перенесено з flui_core в flui_types)
  - Методи: tight, loose, tight_for_width, tight_for_height, expand
  - Операції: constrain, loosen, tighten, enforce_width, enforce_height
  - Перевірки: is_tight, is_normalized, is_satisfied_by
  - Додані методи: deflate_width, deflate_height

**Sliver constraints:**
- ✅ SliverConstraints (для списків і scroll effects)
- ✅ SliverGeometry

**Scroll metrics:**
- ✅ FixedExtentMetrics (з підрахунком індексів елементів)
- ✅ FixedScrollMetrics (з fraction і page tracking)

**Growth direction:**
- ✅ GrowthDirection (Forward, Reverse)
- ✅ ScrollDirection (Idle, Forward, Reverse)
- ✅ AxisDirection (re-exported з layout, додано метод flip())

**Не включено (будуть в інших крейтах):**
- ❌ SliverLogicalContainerParentData (flui_rendering)
- ❌ SliverPhysicalContainerParentData (flui_rendering)
- ❌ FlexParentData (flui_rendering)
- ❌ BoxConstraintsTween (це тип для анімації, можливо в flui_animation)

#### ✅ Semantics Data (РЕАЛИЗОВАНО - 599 строк, 35 тестов)
**Источники:** semantics.md

**Semantic primitives:**
- ✅ SemanticsTag
- ✅ SemanticsFlags
- ✅ SemanticsAction
- ✅ SemanticsRole (enum: Button, Link, Image, etc.)

**Semantic data:**
- ✅ SemanticsData (summary про node)
- ✅ SemanticsProperties (властивості для a11y)
- ✅ SemanticsHintOverrides

**Semantic events:**
- ✅ SemanticsEvent (base trait)
- ✅ AnnounceSemanticsEvent
- ✅ TapSemanticEvent
- ✅ LongPressSemanticsEvent
- ✅ FocusSemanticEvent
- ✅ TooltipSemanticsEvent

**Sort keys:**
- ✅ SemanticsSortKey (base trait)
- ✅ OrdinalSortKey

**String attributes:**
- ✅ StringAttribute (base trait)
- ✅ AttributedString
- ✅ LocaleStringAttribute
- ✅ SpellOutStringAttribute

#### ✅ Platform Types (РЕАЛИЗОВАНО - 557 строк, 24 теста)
**Источники:** foundation.md, service.md

**Platform info:**
- ✅ TargetPlatform (enum: Android, iOS, macOS, Linux, Windows, Fuchsia, Web)
- ✅ Brightness (Light, Dark)

**Device orientation:**
- ✅ DeviceOrientation (PortraitUp, PortraitDown, LandscapeLeft, LandscapeRight)

**Locale:**
- ✅ Locale (language, country, script)

---

### flui_foundation - Foundation layer

**З foundation.md (~100 типів):**

**Keys (✅ РЕАЛИЗОВАНО):**
- Key
- LocalKey
- ValueKey<T>
- ObjectKey
- UniqueKey
- GlobalKey<T>
- GlobalObjectKey<T>

**Observables (✅ РЕАЛИЗОВАНО):**
- Listenable
- ChangeNotifier
- ValueListenable<T>
- ValueNotifier<T>
- ObserverList<T>
- HashedObserverList<T>

**Diagnostics (✅ РЕАЛИЗОВАНО):**
- Diagnosticable, DiagnosticableTree, DiagnosticableTreeMixin
- DiagnosticsNode
- DiagnosticsProperty<T>
- DiagnosticPropertiesBuilder
- DiagnosticsTreeStyle (enum)
- DiagnosticLevel (enum)
- + багато інших diagnostic типів

**Utilities:**
- BitField<T>
- CachingIterable<E>
- Unicode (константи)
- Factory<T>
- Category, DocumentationIcon, Summary (annotations)

**Error handling:**
- FlutterError
- FlutterErrorDetails
- ErrorDescription, ErrorHint, ErrorSummary, ErrorSpacer

**Bindings:**
- BindingBase

**Futures:**
- SynchronousFuture<T>

**Collections:**
- PersistentHashMap<K, V>

**Memory:**
- FlutterMemoryAllocations
- ObjectCreated, ObjectDisposed, ObjectEvent

**License:**
- LicenseEntry, LicenseEntryWithLineBreaks
- LicenseParagraph
- LicenseRegistry

**Platform dispatcher:**
- PlatformDispatcher (може бути в flui_platform)
- SingletonFlutterWindow (deprecated)

---

### flui_core - Core traits (49 тестів)

**Widget система (✅ РЕАЛИЗОВАНО ПОЛНОСТЬЮ):**
- ✅ Widget (з DynClone + Downcast)
- ✅ StatelessWidget
- ✅ StatefulWidget
- ✅ State<T> (з DowncastSync)
- ✅ InheritedWidget (з impl_inherited_widget! макросом)
- ✅ RenderObjectWidget
- ✅ LeafRenderObjectWidget
- ✅ SingleChildRenderObjectWidget
- ✅ MultiChildRenderObjectWidget

**Element система (✅ РЕАЛИЗОВАНО ПОЛНОСТЬЮ):**
- ✅ Element (з DowncastSync)
- ✅ ComponentElement
- ✅ StatefulElement
- ✅ InheritedElement
- ✅ RenderObjectElement (новий!)
- ⏳ LeafRenderObjectElement (можна додати пізніше)
- ⏳ SingleChildRenderObjectElement (можна додати пізніше)
- ⏳ MultiChildRenderObjectElement (можна додати пізніше)

**RenderObject система (✅ РЕАЛИЗОВАНО):**
- ✅ RenderObject trait (з DowncastSync, перенесено з flui_rendering)

**BuildContext (✅ РЕАЛИЗОВАНО):**
- ✅ BuildContext

**Parent data (✅ РЕАЛИЗОВАНО):**
- ✅ ParentData (з DowncastSync)
- ✅ ContainerParentData<ChildId>
- ✅ BoxParentData
- ✅ ContainerBoxParentData<ChildId>

**Utilities (✅ РЕАЛИЗОВАНО):**
- ✅ BoxConstraints (перенесено з flui_types)

---

### flui_rendering - Rendering system (66 тестів, +51 нових)

**З rendering.md (~550 типів):**

**RenderObject hierarchy (✅ РЕАЛИЗОВАНО з інтеграцією в flui_core):**
- ✅ RenderObject (trait перенесено в flui_core з DowncastSync)
- ✅ RenderBox (основна реалізація box protocol)
- ⏳ RenderSliver (для scrollable списків - планується)
- ✅ RenderProxyBox (passes layout to child)
- ⏳ RenderShiftedBox (планується)

**Specialized render objects (🚧 В ПРОЦЕСІ - 10/42 реалізовано):**

**Layout render objects:**
- ✅ **RenderFlex** (550 строк, 15 тестів) - Row/Column layout з flexible children, MainAxisAlignment, CrossAxisAlignment
- ✅ **RenderPadding** (280 строк, 8 тестів) - Padding layout з EdgeInsets
- ✅ **RenderStack** (330 строк, 13 тестів) - Positioned layout з StackFit, non-positioned і positioned children
- ✅ **RenderConstrainedBox** (180 строк, 10 тестів) - ConstrainedBox/SizedBox з additional constraints
- ✅ **RenderDecoratedBox** (320 строк, 10 тестів) - Паинт BoxDecoration до/після child, DecorationPosition (2025-01-18)
- ✅ **RenderAspectRatio** (390 строк, 17 тестів) - Підтримка aspect ratio (width/height), tight constraints handling (2025-01-18)
- ✅ **RenderLimitedBox** (380 строк, 13 тестів) - Обмежує розмір при unbounded constraints (2025-01-18)
- ✅ **RenderIndexedStack** (430 строк, 13 тестів) - Stack з visible index, показує тільки один child (2025-01-18)
- ✅ **RenderPositionedBox** (410 строк, 16 тестів) - Align/Center widget з width_factor/height_factor (2025-01-18)
- ✅ **RenderFractionallySizedBox** (400 строк, 15 тестів) - Процентний розмір child від parent (widthFactor/heightFactor) (2025-01-18)
- ⏳ RenderWrap - wrap layout
- ⏳ RenderIntrinsicWidth, RenderIntrinsicHeight
- ⏳ RenderFlow
- ⏳ RenderTable
- ⏳ RenderListBody

**Visual effects render objects:**
- ⏳ RenderOpacity, RenderAnimatedOpacity
- ⏳ RenderTransform, RenderRotatedBox
- ⏳ RenderClipRect, RenderClipRRect, RenderClipOval, RenderClipPath
- ⏳ RenderPhysicalModel, RenderPhysicalShape
- ⏳ RenderCustomPaint
- ⏳ RenderRepaintBoundary
- ⏳ RenderBackdropFilter
- ⏳ RenderShaderMask

**Interaction render objects:**
- ⏳ RenderIgnorePointer, RenderAbsorbPointer
- ⏳ RenderMouseRegion
- ⏳ RenderPointerListener

**Accessibility render objects:**
- ⏳ RenderSemanticsAnnotations
- ⏳ RenderMergeSemantics, RenderBlockSemantics, RenderExcludeSemantics, RenderIndexedSemantics

**Advanced render objects:**
- ⏳ RenderLeaderLayer, RenderFollowerLayer
- ⏳ RenderOffstage
- ⏳ RenderMetaData
- ⏳ RenderListWheelViewport

**Text render objects:**
- ⏳ RenderEditableLine (для text)
- ⏳ RenderParagraph (для text)

**Image render objects:**
- ⏳ RenderImage

**Sliver render objects (⏳ PLANNED):**
- RenderSliver (base)
- RenderSliverToBoxAdapter
- RenderSliverPadding
- RenderSliverList
- RenderSliverFixedExtentList
- RenderSliverVariedExtentList
- RenderSliverGrid
- RenderSliverFillViewport
- RenderSliverFillRemaining
- RenderSliverPersistentHeader
- RenderSliverFloatingPersistentHeader
- RenderSliverPinnedPersistentHeader
- RenderSliverAnimatedOpacity
- RenderSliverIgnorePointer, RenderSliverOffstage
- RenderSliverOpacity
- RenderSliverCrossAxisGroup, RenderSliverMainAxisGroup

**Viewport:**
- RenderViewport, RenderShrinkWrappingViewport
- RenderAbstractViewport
- ViewportOffset
- RevealedOffset

**Layers:**
- Layer, ContainerLayer
- PictureLayer, TextureLayer
- OffsetLayer
- ClipPathLayer, ClipRectLayer, ClipRRectLayer
- OpacityLayer
- ColorFilterLayer, ImageFilterLayer
- ShaderMaskLayer, BackdropFilterLayer
- TransformLayer
- FollowerLayer, LeaderLayer
- LayerLink
- AnnotatedRegionLayer<T>
- PlatformViewLayer
- PerformanceOverlayLayer

**Pipeline:**
- PipelineOwner
- PipelineManifold
- RenderingFlutterBinding

**Painting context:**
- PaintingContext
- ClipContext
- Paint, Path, Canvas (maybe from dart:ui/egui)

**Hit testing:**
- HitTestEntry<T>, HitTestResult, HitTestTarget, HitTestable
- BoxHitTestEntry, BoxHitTestResult
- SliverHitTestEntry, SliverHitTestResult
- HitTestDispatcher

**Mouse:**
- MouseCursor, MouseTracker
- SystemMouseCursors

**Layout delegates:**
- MultiChildLayoutDelegate
- SingleChildLayoutDelegate
- FlowDelegate
- SliverGridDelegate
- SliverGridDelegateWithFixedCrossAxisCount
- SliverGridDelegateWithMaxCrossAxisExtent

**Table:**
- TableColumnWidth (trait)
- FixedColumnWidth, FlexColumnWidth, FractionColumnWidth
- IntrinsicColumnWidth
- MaxColumnWidth, MinColumnWidth
- TableBorder
- TableCellParentData

**Platform views:**
- PlatformViewRenderBox
- RenderAndroidView
- RenderUiKitView
- RenderAppKitView

**Utilities:**
- RelativeRect
- ChildLayoutHelper

---

### flui_animation - Animation system

**З animation.md (~60 типів):**

**Animation core:**
- Animation<T>
- AnimationController
- Ticker, TickerProvider (maybe in flui_scheduler)
- TickerFuture
- TickerCanceled (exception)

**Animation combinators:**
- CompoundAnimation<T>
- AnimationMin<T>, AnimationMax<T>
- AnimationMean
- ProxyAnimation
- ReverseAnimation
- TrainHoppingAnimation
- CurvedAnimation

**Specialized animations:**
- AlwaysStoppedAnimation<T>

**Mixins:**
- AnimationEagerListenerMixin
- AnimationLazyListenerMixin
- AnimationLocalListenersMixin
- AnimationLocalStatusListenersMixin
- AnimationWithParentMixin<T>

---

### flui_gestures - Gesture system

**З gestures.md (~125 типів):**

**Gesture recognizers:**
- GestureRecognizer (base)
- OneSequenceGestureRecognizer
- PrimaryPointerGestureRecognizer
- BaseTapGestureRecognizer
- BaseTapAndDragGestureRecognizer
- EagerGestureRecognizer

**Tap recognizers:**
- TapGestureRecognizer
- DoubleTapGestureRecognizer
- SerialTapGestureRecognizer

**Drag recognizers:**
- DragGestureRecognizer
- HorizontalDragGestureRecognizer
- VerticalDragGestureRecognizer
- PanGestureRecognizer

**Multi-drag recognizers:**
- MultiDragGestureRecognizer
- ImmediateMultiDragGestureRecognizer
- HorizontalMultiDragGestureRecognizer
- VerticalMultiDragGestureRecognizer
- DelayedMultiDragGestureRecognizer

**Combined recognizers:**
- TapAndDragGestureRecognizer
- TapAndHorizontalDragGestureRecognizer
- TapAndPanGestureRecognizer

**Other recognizers:**
- ScaleGestureRecognizer
- LongPressGestureRecognizer
- ForcePressGestureRecognizer
- MultiTapGestureRecognizer

**Gesture arena:**
- GestureArenaManager
- GestureArenaMember
- GestureArenaEntry
- GestureArenaTeam
- GestureDisposition (enum)

**Pointer events (багато може бути в flui_types або platform layer):**
- PointerEvent (base)
- PointerDownEvent, PointerUpEvent, PointerMoveEvent, PointerCancelEvent
- PointerAddedEvent, PointerRemovedEvent
- PointerHoverEvent
- PointerEnterEvent, PointerExitEvent
- PointerScrollEvent
- PointerPanZoomStartEvent, PointerPanZoomUpdateEvent, PointerPanZoomEndEvent
- PointerSignalEvent
- PointerEventConverter
- PointerEventResampler
- PointerRouter

**Velocity tracking:**
- VelocityTracker
- IOSScrollViewFlingVelocityTracker
- MacOSScrollViewFlingVelocityTracker
- PolynomialFit
- LeastSquaresSolver

**Utilities:**
- DeviceGestureSettings
- Drag (interface)
- MultiDragPointerState
- SamplingClock

**Hit testing:**
- HitTestTarget, HitTestable, HitTestDispatcher
- HitTestEntry<T>, HitTestResult
- FlutterErrorDetailsForPointerEventDispatcher

**Bindings:**
- GestureBinding

---

### flui_scheduler - Scheduling

**З scheduler.md (~12 типів):**

**Scheduler:**
- SchedulerBinding (mixin)
- SchedulerPhase (enum)
- Priority

**Frame timing:**
- FrameTiming
- PerformanceModeRequestHandle

**Ticker (може перемістити з animation):**
- Ticker
- TickerProvider
- TickerFuture
- TickerCanceled (exception)

**Lifecycle:**
- AppLifecycleState (enum)

**Service extensions:**
- SchedulerServiceExtensions (enum)

---

### flui_painting - Painting utilities

**З painting.md (~160 типів):**

**Image providers:**
- ImageProvider<T>
- AssetBundleImageProvider
- AssetImage, ExactAssetImage
- NetworkImage
- FileImage
- MemoryImage
- ResizeImage
- AssetBundleImageKey
- ResizeImageKey

**Image caching:**
- ImageCache
- ImageCacheStatus
- ImageInfo
- ImageStream
- ImageStreamCompleter
- OneFrameImageStreamCompleter
- MultiFrameImageStreamCompleter
- ImageStreamCompleterHandle
- ImageStreamListener
- ImageChunkEvent
- ImageSizeInfo

**Text painting:**
- TextPainter
- TextLayoutMetrics
- InlineSpanSemanticsInformation
- WordBoundary (можливо в service)

**Canvas/Paint (можливо з dart:ui або egui):**
- Canvas
- Paint
- Path

**Bindings:**
- PaintingBinding

**Shape warm-up:**
- ShaderWarmUp

**Network loading:**
- NetworkImageLoadException

---

### flui_semantics - Semantic system

**З semantics.md (~43 типи):**

**Semantics tree:**
- SemanticsNode
- SemanticsOwner
- SemanticsConfiguration
- ChildSemanticsConfigurationsResult
- ChildSemanticsConfigurationsResultBuilder
- SemanticsHandle

**Semantics utilities:**
- AccessibilityFeatures
- SemanticsService
- SemanticsLabelBuilder

**Bindings:**
- SemanticsBinding

**Builder:**
- SemanticsUpdateBuilder (maybe from dart:ui)

**Validation:**
- SemanticsValidationResult (enum)

**Debug:**
- DebugSemanticsDumpOrder (enum)

---

### flui_service - Platform services

**З service.md (~530 типів):**

**Platform views:**
- PlatformViewController
- PlatformViewsService
- PlatformViewsRegistry
- AndroidViewController (+ варіанти)
- AppKitViewController, UiKitViewController
- DarwinPlatformViewController

**Asset management:**
- AssetBundle (base trait)
- CachingAssetBundle
- NetworkAssetBundle
- PlatformAssetBundle
- AssetManifest, AssetMetadata

**Autofill:**
- AutofillClient, AutofillConfiguration
- AutofillScope, AutofillScopeMixin
- AutofillHints

**Binary messaging:**
- BinaryMessenger
- BackgroundIsolateBinaryMessenger

**Message channels:**
- BasicMessageChannel<T>
- MethodChannel
- OptionalMethodChannel
- EventChannel
- MethodCall

**Codecs:**
- MessageCodec<T>
- MethodCodec
- BinaryCodec, StringCodec
- JSONMessageCodec, JSONMethodCodec
- StandardMessageCodec, StandardMethodCodec

**Buffering:**
- ChannelBuffers
- ReadBuffer, WriteBuffer
- ImmutableBuffer

**Clipboard:**
- Clipboard
- ClipboardData

**Context menus:**
- ContextMenuController
- SystemContextMenuController
- BrowserContextMenu

**Deferred components:**
- DeferredComponent

**Device:**
- FlutterVersion

**Font:**
- FontLoader

**Haptic:**
- HapticFeedback

**Keyboard:**
- HardwareKeyboard
- KeyboardKey, LogicalKeyboardKey, PhysicalKeyboardKey
- KeyEvent, KeyDownEvent, KeyUpEvent, KeyRepeatEvent
- KeyData, KeyMessage
- KeyEventManager
- KeyboardInsertedContent
- CharacterBoundary, DocumentBoundary, LineBoundary, ParagraphBoundary, WordBoundary, TextBoundary
- RawKeyboard (deprecated), RawKeyEvent (deprecated)

**Live Text:**
- LiveText

**Mouse:**
- MouseCursorManager, MouseCursorSession
- MouseTrackerAnnotation

**Predictive back:**
- PredictiveBackEvent

**Process text:**
- ProcessTextService, ProcessTextAction

**Restoration:**
- RestorationManager, RestorationBucket
- RootIsolateToken

**Scribble:**
- ScribbleClient, Scribe

**Selection:**
- SelectionRect

**Sensitive content:**
- SensitiveContentService

**Spell check:**
- SpellCheckService, DefaultSpellCheckService
- SpellCheckResults
- SuggestionSpan

**System channels:**
- SystemChannels

**System integration:**
- SystemChrome
- SystemNavigator
- SystemSound
- SystemUiOverlayStyle

**Text editing:**
- TextInput
- TextInputClient, DeltaTextInputClient
- TextInputControl
- TextInputConnection
- TextInputConfiguration
- TextInputType
- TextInputFormatter
- FilteringTextInputFormatter
- LengthLimitingTextInputFormatter
- TextEditingValue
- TextEditingDelta (+ варіанти)
- TextLayoutMetrics
- TextSelectionDelegate

**Undo:**
- UndoManager, UndoManagerClient

**Bindings:**
- ServicesBinding

**Exceptions:**
- MissingPluginException
- PlatformException

---

### flui_widgets - Widget library

**З widgets.md (~1000+ типів):**

Це ВЕЛИЧЕЗНА бібліотека. Основні категорії:

**Basic widgets:**
- Container, Padding, Center, Align
- SizedBox, ConstrainedBox, AspectRatio
- Baseline, FittedBox, FractionallySizedBox
- LimitedBox, Offstage, OverflowBox
- Transform, RotatedBox
- Visibility

**Layout:**
- Row, Column, Flex
- Stack, Positioned, IndexedStack
- Wrap, Flow
- ListBody, ListView, GridView
- Table, TableRow, TableCell
- CustomMultiChildLayout, CustomSingleChildLayout

**Scrolling:**
- SingleChildScrollView
- CustomScrollView
- ScrollView, BoxScrollView
- ListView (+ варіанти)
- GridView (+ варіанти)
- PageView
- ListWheelScrollView

**Text:**
- Text, RichText
- DefaultTextStyle, DefaultTextHeightBehavior
- SelectableText
- EditableText

**Images:**
- Image (+ варіанти)
- RawImage
- Icon
- Texture

**Input:**
- TextField, TextFormField
- Checkbox, CheckboxListTile
- Radio, RadioListTile
- Switch, SwitchListTile
- Slider, RangeSlider
- DropdownButton, DropdownMenu

**Buttons:**
- TextButton
- ElevatedButton
- OutlinedButton
- IconButton
- FloatingActionButton

**Interaction:**
- GestureDetector
- Listener, MouseRegion
- Draggable, DragTarget, LongPressDraggable
- Dismissible
- InteractiveViewer
- IgnorePointer, AbsorbPointer

**Animation:**
- AnimatedContainer, AnimatedPadding
- AnimatedAlign, AnimatedPositioned
- AnimatedOpacity, AnimatedRotation, AnimatedScale, AnimatedSlide
- AnimatedDefaultTextStyle
- AnimatedSwitcher, AnimatedCrossFade
- AnimatedSize
- Hero
- + багато інших

**Navigation:**
- Navigator, NavigatorState
- Route<T>, ModalRoute<T>, PageRoute<T>
- MaterialPageRoute, CupertinoPageRoute
- PageRouteBuilder

**Forms:**
- Form, FormField<T>, FormState
- AutofillGroup

**Media:**
- Image, Icon
- RawImage
- Placeholder, CircularProgressIndicator, LinearProgressIndicator

**Accessibility:**
- Semantics, MergeSemantics, ExcludeSemantics, BlockSemantics
- IndexedSemantics

**Platform views:**
- AndroidView, UiKitView, AppKitView, HtmlElementView

**Inherited widgets:**
- InheritedWidget
- InheritedModel<T>
- InheritedNotifier<T>
- InheritedTheme

**Themes:**
- Theme, ThemeData
- IconTheme, IconThemeData
- DefaultTextStyle
- MediaQuery, MediaQueryData

**Misc:**
- Builder
- StatefulBuilder
- LayoutBuilder
- FutureBuilder<T>, StreamBuilder<T>
- ValueListenableBuilder<T>
- Directionality
- Localizations<T>
- WillPopScope, PopScope
- SafeArea
- Spacer
- Divider
- Placeholder
- Banner

І ще СОТНІ інших...

---

### flui_material - Material Design

**З material.md (~1000+ типів):**

Ще одна ВЕЛИЧЕЗНА бібліотека Material Design компонентів. Деякі основні:

**Material widgets:**
- Scaffold, AppBar, BottomNavigationBar
- Drawer, EndDrawer
- FloatingActionButton
- SnackBar, MaterialBanner
- BottomSheet, ModalBottomSheet
- Dialog, AlertDialog, SimpleDialog
- Card
- Chip (+ варіанти: InputChip, ChoiceChip, FilterChip, ActionChip)
- ListTile, ExpansionTile
- Stepper, Step
- DataTable, DataRow, DataColumn
- TabBar, TabBarView, Tab
- NavigationBar, NavigationRail
- Menu, MenuBar, MenuButton
- Badge
- Tooltip
- ProgressIndicator (Circular, Linear)
- RefreshIndicator
- Autocomplete
- DatePicker, TimePicker
- SearchBar, SearchAnchor
- CarouselView

**Material theming:**
- MaterialApp
- ThemeData
- ColorScheme
- TextTheme
- ButtonThemeData
- AppBarTheme
- BottomNavigationBarTheme
- CardTheme
- ChipTheme
- DialogTheme
- + десятки інших theme data класів

**Material utilities:**
- Material, MaterialType
- InkWell, InkResponse
- Ink
- MaterialButton (base)

І багато інших...

---

## 📊 Підсумкова статистика по flui_types

### Що ПОВИННО бути в flui_types:

| Категорія | Типів | Строк | Тестів | Приоритет | Статус |
|-----------|-------|-------|--------|-----------|--------|
| **Geometry** | 5 | 1910 | 68 | CRITICAL | ✅ Done |
| **Layout** | 24 | 2136 | 49 | CRITICAL | ✅ Done |
| **Styling** | 45+ | 3287 | 116 | CRITICAL | ✅ Done |
| **Typography** | 30+ | 983 | 50 | HIGH | ✅ Done |
| **Painting** | 25+ | 1048 | 62 | MEDIUM | ✅ Done |
| **Animation** | 35+ | 1089 | 37 | HIGH | ✅ Done |
| **Constraints** | 11 | 1008 | 41 | HIGH | ✅ Done |
| **Gestures Details** | 17 | 758 | 23 | MEDIUM | ✅ Done |
| **Physics** | 10 | 902 | 47 | MEDIUM | ✅ Done |
| **Semantics Data** | 15+ | 599 | 35 | LOW | ✅ Done |
| **Platform Types** | 5+ | 557 | 24 | MEDIUM | ✅ Done |
| **TOTAL** | **~237 типів** | **~13677 строк** | **~524 тести** | | |

### Що НЕ повинно бути в flui_types:

- **Widgets** (~1000+ типів) → flui_widgets
- **Material виджети** (~1000+ типів) → flui_material
- **RenderObjects** (~550+ типів) → flui_rendering
- **Gesture recognizers** (~80+ типів) → flui_gestures
- **Animation controllers** (~20+ типів) → flui_animation
- **Service APIs** (~500+ типів) → flui_service
- **Painting APIs** (~80+ типів) → flui_painting
- **Platform views** (~30+ типів) → flui_platform
- **Scheduler** (~12 типів) → flui_scheduler

---

## 🎯 Висновок

З **3500+ типів** у glossary, **лише ~237 типів** (~7%) належать до базового крейту `flui_types`. Решта - це high-level компоненти (віджети, render objects, controllers, services), які йдуть в спеціалізовані крейти.

**🎉 ЗАВЕРШЕНО!** В `flui_types` реалізовано **всі 11 основних модулів**:
- ✅ Geometry (1910 строк, 68 тестів)
- ✅ Layout (2136 строк, 49 тестів)
- ✅ Styling (3287 строк, 116 тестів)
- ✅ Typography (983 строки, 50 тестів)
- ✅ Painting (1048 строк, 62 теста)
- ✅ Animation (1089 строк, 37 тестів)
- ✅ Constraints (1008 строк, 41 тест)
- ✅ Gestures (758 строк, 23 теста)
- ✅ Physics (902 строки, 47 тестів)
- ✅ Semantics (599 строк, 35 тестів)
- ✅ Platform (557 строк, 24 теста)

**Разом: ~13677 строк коду і ~524 тести**, що створює **comprehensive базу типів** для всього фреймворку!

**🎊 НОВИНКА!** В `flui_core` реалізовано **три-древесну архітектуру Flutter**:
- ✅ Widget система (9 traits з DynClone + Downcast)
- ✅ Element система (4 implementations з DowncastSync)
- ✅ RenderObject система (trait з DowncastSync)
- ✅ ParentData система (4 types з DowncastSync)

**Разом: 49 тестів**, що реалізує **повну архітектуру Widget → Element → RenderObject**!

---

**Наступний крок:** Оновити [FLUI_TYPES_ROADMAP.md](FLUI_TYPES_ROADMAP.md) з урахуванням цього розширеного списку типів.
