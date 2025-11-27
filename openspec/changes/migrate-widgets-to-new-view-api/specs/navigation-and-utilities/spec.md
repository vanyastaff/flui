# Navigation and Utilities Specification

## Purpose

This specification references the detailed navigation and utility widget requirements documented in `crates/flui_widgets/guide/08_navigation_and_utilities.md`.

## ADDED Requirements

### Requirement: Navigation Widget Categories

Navigation widgets SHALL provide routing, page transitions, and app-level configuration, as documented in guide/08_navigation_and_utilities.md.

#### Scenario: Navigator manages navigation stack

**GIVEN** a developer needs route navigation
**WHEN** using Navigator widget
**THEN** widget SHALL support bon builder pattern
**AND** widget SHALL support struct literal pattern
**AND** widget SHALL use RenderTheater (for Overlay) for route stack
**AND** widget SHALL support pages (List<Page>) for declarative navigation
**AND** widget SHALL support onPopPage callback for page navigation
**AND** widget SHALL support initialRoute, onGenerateRoute, onGenerateInitialRoutes, onUnknownRoute
**AND** widget SHALL support transitionDelegate, observers, clipBehavior
**AND** widget SHALL expose static methods: push(), pop(), pushNamed(), pushReplacement(), etc.
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

#### Scenario: MaterialApp provides Material Design app root

**GIVEN** a developer needs Material Design application
**WHEN** using MaterialApp widget
**THEN** widget SHALL use WidgetsApp + Navigator + Material Design theming
**AND** widget SHALL support home, routes, initialRoute, onGenerateRoute, onUnknownRoute
**AND** widget SHALL support navigatorObservers, builder, title, onGenerateTitle, color
**AND** widget SHALL support theme, darkTheme, themeMode for Material theming
**AND** widget SHALL support locale, localizationsDelegates, supportedLocales
**AND** widget SHALL support debugShowMaterialGrid, showPerformanceOverlay, debugShowCheckedModeBanner
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

#### Scenario: CupertinoApp provides iOS-style app root

**GIVEN** a developer needs iOS-style application
**WHEN** using CupertinoApp widget
**THEN** widget SHALL use WidgetsApp + Navigator + Cupertino theming
**AND** widget SHALL support similar parameters to MaterialApp
**AND** widget SHALL support theme (CupertinoThemeData) instead of Material theme
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

#### Scenario: Route widgets provide page transitions

**GIVEN** a developer needs custom page transition
**WHEN** using PageRouteBuilder, MaterialPageRoute, or CupertinoPageRoute
**THEN** PageRouteBuilder SHALL support pageBuilder, transitionsBuilder, transitionDuration
**AND** PageRouteBuilder SHALL support opaque, barrierDismissible, barrierColor, barrierLabel
**AND** MaterialPageRoute SHALL use platform-specific transition and support builder
**AND** CupertinoPageRoute SHALL use iOS-style slide transition
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

---

### Requirement: Utility Widget Categories

Utility widgets SHALL provide context access, layout adaptation, theming, state management, and custom rendering, as documented in guide/08_navigation_and_utilities.md.

#### Scenario: Builder provides new BuildContext

**GIVEN** a developer needs new BuildContext for InheritedWidget access
**WHEN** using Builder widget
**THEN** widget SHALL call builder with new BuildContext
**AND** widget SHALL use RenderObject created in builder
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

#### Scenario: StatefulBuilder provides local state without StatefulWidget

**GIVEN** a developer needs local state without creating StatefulWidget
**WHEN** using StatefulBuilder widget
**THEN** widget SHALL extend StatefulWidget internally
**AND** widget SHALL call builder(context, setState) for rebuild control
**AND** widget SHALL use RenderObject created in builder
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

#### Scenario: LayoutBuilder provides constraints-based adaptation

**GIVEN** a developer needs responsive layout based on constraints
**WHEN** using LayoutBuilder widget
**THEN** widget SHALL use RenderConstrainedLayoutBuilder
**AND** widget SHALL call builder(context, constraints) on constraint changes
**AND** widget SHALL rebuild when parent constraints change
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

#### Scenario: OrientationBuilder provides orientation-based adaptation

**GIVEN** a developer needs different layouts for portrait/landscape
**WHEN** using OrientationBuilder widget
**THEN** widget SHALL call builder(context, orientation) on orientation changes
**AND** widget SHALL rebuild when device orientation changes
**AND** widget SHALL use RenderObject created in builder
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

---

### Requirement: Data Propagation Widgets

Utility widgets SHALL provide efficient data propagation down the widget tree, as documented in guide/08_navigation_and_utilities.md.

#### Scenario: MediaQuery provides screen information

**GIVEN** a developer needs screen metrics
**WHEN** using MediaQuery widget
**THEN** widget SHALL use InheritedWidget pattern
**AND** widget SHALL support data (MediaQueryData) with size, padding, viewInsets, orientation
**AND** widget SHALL provide static of(context) method for access
**AND** MediaQueryData SHALL expose size, padding, viewInsets, orientation, devicePixelRatio, platformBrightness, textScaler
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

#### Scenario: SafeArea avoids system UI

**GIVEN** a developer needs to avoid system UI (notches, status bar)
**WHEN** using SafeArea widget
**THEN** widget SHALL use RenderPadding based on MediaQuery viewInsets
**AND** widget SHALL support left, top, right, bottom (bool) to control which insets to avoid
**AND** widget SHALL support minimum (EdgeInsets) for minimum padding
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

#### Scenario: Theme provides Material Design theme

**GIVEN** a developer needs Material Design theming
**WHEN** using Theme widget
**THEN** widget SHALL use InheritedTheme pattern
**AND** widget SHALL support data (ThemeData) with colors, typography, component themes
**AND** widget SHALL provide static of(context) method for access
**AND** ThemeData SHALL expose primaryColor, textTheme, appBarTheme, buttonTheme, etc.
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

#### Scenario: InheritedWidget provides custom data propagation

**GIVEN** a developer needs custom data sharing down tree
**WHEN** extending InheritedWidget abstract class
**THEN** widget SHALL not create RenderObject (doesn't participate in rendering)
**AND** widget SHALL implement updateShouldNotify(oldWidget) method
**AND** descendants SHALL access via context.dependOnInheritedWidgetOfExactType<T>()
**AND** widget SHALL efficiently notify dependents on data changes
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

#### Scenario: InheritedTheme provides theme propagation

**GIVEN** a developer needs theme data propagation
**WHEN** extending InheritedTheme base class
**THEN** widget SHALL extend InheritedWidget
**AND** widget SHALL support wrap(context, child) for theme capture
**AND** widget SHALL be used as base for theme widgets (Theme, IconTheme, DefaultTextStyle)
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

---

### Requirement: Reactive Widget Builders

Utility widgets SHALL provide reactive rebuilding based on observable data, as documented in guide/08_navigation_and_utilities.md.

#### Scenario: ValueListenableBuilder listens to ValueNotifier

**GIVEN** a developer needs reactive UI for ValueNotifier
**WHEN** using ValueListenableBuilder<T> widget
**THEN** widget SHALL call builder(context, value, child) on value changes
**AND** widget SHALL support valueListenable (ValueListenable<T>) parameter
**AND** widget SHALL support child parameter for cached non-rebuilding subtree
**AND** widget SHALL use RenderObject created in builder
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

#### Scenario: StreamBuilder listens to Stream

**GIVEN** a developer needs reactive UI for Stream
**WHEN** using StreamBuilder<T> widget
**THEN** widget SHALL call builder(context, snapshot) on stream events
**AND** widget SHALL support stream (Stream<T>) and initialData parameters
**AND** AsyncSnapshot SHALL expose data, error, connectionState
**AND** widget SHALL use RenderObject created in builder
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

#### Scenario: FutureBuilder listens to Future

**GIVEN** a developer needs reactive UI for Future
**WHEN** using FutureBuilder<T> widget
**THEN** widget SHALL call builder(context, snapshot) when future completes
**AND** widget SHALL support future (Future<T>) and initialData parameters
**AND** AsyncSnapshot SHALL expose data, error, connectionState
**AND** widget SHALL provide loading states via ConnectionState
**AND** widget SHALL use RenderObject created in builder
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

---

### Requirement: Form Management Widgets

Utility widgets SHALL provide form validation and state management, as documented in guide/08_navigation_and_utilities.md.

#### Scenario: Form manages form validation and saving

**GIVEN** a developer needs form validation
**WHEN** using Form widget
**THEN** widget SHALL not create RenderObject (manages state only)
**AND** widget SHALL support child, onChanged, autovalidateMode, onWillPop
**AND** widget SHALL provide static of(context) method for FormState access
**AND** FormState SHALL expose validate(), save(), reset() methods
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

#### Scenario: FormField provides base for form fields

**GIVEN** a developer needs custom form field
**WHEN** extending FormField<T> widget
**THEN** widget SHALL extend StatefulWidget with FormFieldState<T>
**AND** widget SHALL support builder(state), onSaved, validator, initialValue
**AND** widget SHALL support autovalidateMode, enabled, restorationId
**AND** FormFieldState SHALL provide validate(), save(), reset() methods
**AND** widget SHALL use RenderObject created in builder
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

---

### Requirement: Focus and Accessibility Widgets

Utility widgets SHALL provide focus management and accessibility, as documented in guide/08_navigation_and_utilities.md.

#### Scenario: Focus manages focus state

**GIVEN** a developer needs focus management
**WHEN** using Focus widget
**THEN** widget SHALL use RenderProxyBox or child's RenderObject
**AND** widget SHALL support focusNode, autofocus, onFocusChange
**AND** widget SHALL support onKey, onKeyEvent for keyboard handling
**AND** widget SHALL support canRequestFocus, skipTraversal, descendantsAreFocusable
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

#### Scenario: FocusScope manages focus subtree

**GIVEN** a developer needs focus scope
**WHEN** using FocusScope widget
**THEN** widget SHALL extend Focus
**AND** widget SHALL support node (FocusScopeNode) parameter
**AND** widget SHALL manage focus tree with FocusScopeNode
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

#### Scenario: Semantics provides accessibility information

**GIVEN** a developer needs accessibility metadata
**WHEN** using Semantics widget
**THEN** widget SHALL use RenderSemantics
**AND** widget SHALL support container, explicitChildNodes, excludeSemantics
**AND** widget SHALL support enabled, checked, toggled, selected, button, slider, textField, etc.
**AND** widget SHALL support label, value, hint, textDirection
**AND** widget SHALL support onTap, onLongPress, onScrollLeft, onIncrease, etc.
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

#### Scenario: ExcludeSemantics hides from accessibility

**GIVEN** a developer needs to hide from screen readers
**WHEN** using ExcludeSemantics widget
**THEN** widget SHALL use RenderSemantics with excludeSemantics: true
**AND** widget SHALL support excluding (bool) parameter
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

#### Scenario: MergeSemantics merges child semantics

**GIVEN** a developer needs to merge child semantics nodes
**WHEN** using MergeSemantics widget
**THEN** widget SHALL use RenderMergeSemantics
**AND** widget SHALL combine child semantics into single node
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

---

### Requirement: Custom Rendering Widgets

Utility widgets SHALL provide custom rendering capabilities, as documented in guide/08_navigation_and_utilities.md.

#### Scenario: CustomPaint provides custom painting

**GIVEN** a developer needs custom drawing
**WHEN** using CustomPaint widget
**THEN** widget SHALL use RenderCustomPaint
**AND** widget SHALL support painter (CustomPainter) for background
**AND** widget SHALL support foregroundPainter (CustomPainter) for foreground
**AND** widget SHALL support size (Size), isComplex, willChange hints
**AND** CustomPainter SHALL implement paint(Canvas, Size) and shouldRepaint(oldDelegate)
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

#### Scenario: Placeholder provides temporary widget

**GIVEN** a developer needs temporary placeholder
**WHEN** using Placeholder widget
**THEN** widget SHALL use RenderLimitedBox + RenderCustomPaint
**AND** widget SHALL draw X pattern in placeholder area
**AND** widget SHALL support color, strokeWidth, fallbackWidth, fallbackHeight
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

---

### Requirement: Platform-Specific Widgets

Utility widgets SHALL provide platform-specific functionality, as documented in guide/08_navigation_and_utilities.md.

#### Scenario: SelectionArea enables text selection

**GIVEN** a developer needs text selection across widgets
**WHEN** using SelectionArea widget
**THEN** widget SHALL use RenderSelectionContainer
**AND** widget SHALL support child, focusNode, selectionControls
**AND** widget SHALL support contextMenuBuilder, magnifierConfiguration
**AND** widget SHALL support onSelectionChanged callback
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

#### Scenario: CupertinoNavigationBar provides iOS navigation bar

**GIVEN** a developer needs iOS-style navigation bar
**WHEN** using CupertinoNavigationBar widget
**THEN** widget SHALL use RenderSliverPersistentHeader
**AND** widget SHALL support leading, middle, trailing
**AND** widget SHALL support backgroundColor, brightness, padding, border
**AND** widget SHALL support transitionBetweenRoutes, heroTag, previousPageTitle
**AND** widget SHALL follow patterns documented in guide/08_navigation_and_utilities.md

---

## Related Documentation

- **Guide:** `crates/flui_widgets/guide/08_navigation_and_utilities.md` - Detailed widget reference
- **Architecture:** `crates/flui_widgets/guide/WIDGET_ARCHITECTURE.md` - Widget organization patterns
- **Implementation:** `crates/flui_widgets/guide/IMPLEMENTATION_GUIDE.md` - Code examples

## Widgets Covered

**Total:** 26+ navigation and utility widgets

**Navigation (6):**
- Navigator, MaterialApp, CupertinoApp
- PageRouteBuilder, MaterialPageRoute, CupertinoPageRoute

**Context and Builders (4):**
- Builder, StatefulBuilder, LayoutBuilder, OrientationBuilder

**Data Propagation (6):**
- MediaQuery, SafeArea, Theme, InheritedWidget, InheritedTheme
- PlatformMenuBar (platform-specific)

**Reactive Builders (3):**
- ValueListenableBuilder<T>, StreamBuilder<T>, FutureBuilder<T>

**Form Management (2):**
- Form, FormField<T>

**Focus and Accessibility (5):**
- Focus, FocusScope, Semantics, ExcludeSemantics, MergeSemantics

**Custom Rendering (2):**
- CustomPaint, Placeholder

**Platform-Specific (2):**
- SelectionArea, CupertinoNavigationBar

**Base Classes (3, not widgets):**
- SingleChildRenderObjectWidget
- MultiChildRenderObjectWidget
- LeafRenderObjectWidget

**Supporting Types:**
- MediaQueryData
- ThemeData
- AsyncSnapshot<T>
- FormState
- CustomPainter
- FocusNode, FocusScopeNode
