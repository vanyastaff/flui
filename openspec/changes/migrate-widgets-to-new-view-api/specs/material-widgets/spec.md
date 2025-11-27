# Material Design Widgets Specification

## Purpose

This specification references the detailed Material Design widget requirements documented in `crates/flui_widgets/guide/07_material_widgets.md`.

## ADDED Requirements

### Requirement: Material Design Widget Categories

Material Design widgets SHALL be organized into scaffolding, navigation, data display, and feedback categories, as documented in guide/07_material_widgets.md.

#### Scenario: Scaffold provides Material page structure

**GIVEN** a developer needs Material Design page layout
**WHEN** using Scaffold widget
**THEN** widget SHALL support bon builder pattern
**AND** widget SHALL support struct literal pattern
**AND** widget SHALL use RenderScaffold + multiple RenderObjects for parts
**AND** widget SHALL support appBar (PreferredSizeWidget), body, floatingActionButton
**AND** widget SHALL support floatingActionButtonLocation, floatingActionButtonAnimator
**AND** widget SHALL support drawer, endDrawer, drawerScrimColor
**AND** widget SHALL support bottomNavigationBar, bottomSheet, persistentFooterButtons
**AND** widget SHALL support backgroundColor, resizeToAvoidBottomInset, extendBody, extendBodyBehindAppBar
**AND** widget SHALL follow patterns documented in guide/07_material_widgets.md

#### Scenario: AppBar provides Material app bar

**GIVEN** a developer needs Material app bar
**WHEN** using AppBar widget
**THEN** widget SHALL use RenderPhysicalModel + RenderFlex for layout
**AND** widget SHALL support leading, title, actions, flexibleSpace, bottom
**AND** widget SHALL support elevation, scrolledUnderElevation, shadowColor, surfaceTintColor
**AND** widget SHALL support backgroundColor, foregroundColor, iconTheme, actionsIconTheme
**AND** widget SHALL support centerTitle, excludeHeaderSemantics, titleSpacing
**AND** widget SHALL support toolbarHeight, leadingWidth, toolbarTextStyle, titleTextStyle
**AND** widget SHALL follow patterns documented in guide/07_material_widgets.md

#### Scenario: BottomNavigationBar provides bottom tab navigation

**GIVEN** a developer needs bottom navigation tabs
**WHEN** using BottomNavigationBar widget
**THEN** widget SHALL use RenderPhysicalModel + RenderFlex
**AND** widget SHALL support items (List<BottomNavigationBarItem>), currentIndex, onTap
**AND** widget SHALL support type (fixed, shifting), elevation, backgroundColor
**AND** widget SHALL support selectedItemColor, unselectedItemColor, iconSize
**AND** widget SHALL support selectedFontSize, unselectedFontSize
**AND** widget SHALL support showSelectedLabels, showUnselectedLabels
**AND** widget SHALL follow patterns documented in guide/07_material_widgets.md

#### Scenario: Drawer provides side navigation panel

**GIVEN** a developer needs side drawer navigation
**WHEN** using Drawer widget
**THEN** widget SHALL use RenderPhysicalModel + RenderConstrainedBox
**AND** widget SHALL support child, backgroundColor, elevation, shadowColor, surfaceTintColor
**AND** widget SHALL support shape, width, semanticLabel
**AND** widget SHALL integrate with Scaffold.drawer or Scaffold.endDrawer
**AND** widget SHALL follow patterns documented in guide/07_material_widgets.md

---

### Requirement: Material Data Display Widgets

Material Design data display widgets SHALL provide cards, lists, and chips, as documented in guide/07_material_widgets.md.

#### Scenario: Card provides Material card container

**GIVEN** a developer needs Material card
**WHEN** using Card widget
**THEN** widget SHALL use RenderPhysicalModel
**AND** widget SHALL support child, color, shadowColor, surfaceTintColor, elevation
**AND** widget SHALL support shape, borderOnForeground, margin, clipBehavior
**AND** widget SHALL follow patterns documented in guide/07_material_widgets.md

#### Scenario: ListTile provides Material list item

**GIVEN** a developer needs Material list item
**WHEN** using ListTile widget
**THEN** widget SHALL use RenderInkFeatures + RenderFlex
**AND** widget SHALL support leading, title, subtitle, trailing
**AND** widget SHALL support isThreeLine, dense, visualDensity, shape, style
**AND** widget SHALL support selectedColor, iconColor, textColor, contentPadding
**AND** widget SHALL support enabled, onTap, onLongPress, selected
**AND** widget SHALL support tileColor, selectedTileColor, focusColor, hoverColor
**AND** widget SHALL follow patterns documented in guide/07_material_widgets.md

#### Scenario: Chip provides Material chip component

**GIVEN** a developer needs Material chip
**WHEN** using Chip widget
**THEN** widget SHALL use RenderPhysicalModel + RenderInkFeatures + RenderFlex
**AND** widget SHALL support avatar, label, labelStyle, labelPadding
**AND** widget SHALL support deleteIcon, onDeleted, deleteIconColor, deleteButtonTooltipMessage
**AND** widget SHALL support backgroundColor, padding, elevation, shadowColor
**AND** widget SHALL support shape, side, clipBehavior, visualDensity
**AND** widget SHALL support Chip(), InputChip(), ChoiceChip(), FilterChip(), ActionChip() variants
**AND** widget SHALL follow patterns documented in guide/07_material_widgets.md

#### Scenario: Badge provides notification badge

**GIVEN** a developer needs notification badge
**WHEN** using Badge widget
**THEN** widget SHALL use RenderStack + RenderPhysicalModel for badge
**AND** widget SHALL support child, label, isLabelVisible
**AND** widget SHALL support backgroundColor, textColor, smallSize, largeSize
**AND** widget SHALL support textStyle, padding, alignment, offset
**AND** widget SHALL follow patterns documented in guide/07_material_widgets.md

---

### Requirement: Material Feedback Widgets

Material Design feedback widgets SHALL provide dialogs, snackbars, bottom sheets, and progress indicators, as documented in guide/07_material_widgets.md.

#### Scenario: Dialog and AlertDialog provide modal dialogs

**GIVEN** a developer needs modal dialog
**WHEN** using Dialog or AlertDialog widget
**THEN** widget SHALL use RenderPhysicalModel + overlay
**AND** Dialog SHALL support child, backgroundColor, elevation, shadowColor, surfaceTintColor
**AND** AlertDialog SHALL support icon, title, content, actions in RenderFlex
**AND** AlertDialog SHALL support titlePadding, contentPadding, actionsPadding, actionsAlignment
**AND** AlertDialog SHALL support scrollable parameter for long content
**AND** widget SHALL be shown via showDialog<T>() function
**AND** widget SHALL follow patterns documented in guide/07_material_widgets.md

#### Scenario: SnackBar provides temporary message

**GIVEN** a developer needs temporary bottom message
**WHEN** using SnackBar widget
**THEN** widget SHALL use RenderPhysicalModel + RenderFlex
**AND** widget SHALL support content, backgroundColor, elevation, margin, padding
**AND** widget SHALL support width, shape, behavior (fixed, floating)
**AND** widget SHALL support action (SnackBarAction), duration
**AND** widget SHALL support dismissDirection, clipBehavior
**AND** widget SHALL be shown via ScaffoldMessenger.of(context).showSnackBar()
**AND** widget SHALL follow patterns documented in guide/07_material_widgets.md

#### Scenario: BottomSheet provides bottom panel

**GIVEN** a developer needs bottom panel
**WHEN** using BottomSheet widget or showModalBottomSheet()
**THEN** widget SHALL use RenderPhysicalShape + overlay
**AND** widget SHALL support onClosing, builder, backgroundColor, elevation, shape
**AND** widget SHALL support clipBehavior, constraints, enableDrag, showDragHandle
**AND** widget SHALL support dragHandleColor, dragHandleSize
**AND** showModalBottomSheet SHALL support isScrollControlled, isDismissible, barrierColor
**AND** widget SHALL follow patterns documented in guide/07_material_widgets.md

#### Scenario: Progress indicators provide loading feedback

**GIVEN** a developer needs loading indicator
**WHEN** using CircularProgressIndicator or LinearProgressIndicator
**THEN** widget SHALL use RenderCustomPaint for animated drawing
**AND** widget SHALL support value (double? 0.0-1.0, null = indeterminate)
**AND** widget SHALL support backgroundColor, color, valueColor (Animation<Color?>)
**AND** CircularProgressIndicator SHALL support strokeWidth, strokeAlign, strokeCap
**AND** LinearProgressIndicator SHALL support minHeight, borderRadius
**AND** widget SHALL support .adaptive() variant for platform-specific rendering
**AND** widget SHALL follow patterns documented in guide/07_material_widgets.md

#### Scenario: Tooltip provides hover/long-press tooltip

**GIVEN** a developer needs tooltip on hover or long press
**WHEN** using Tooltip widget
**THEN** widget SHALL use RenderPointerListener + overlay with RenderPhysicalModel
**AND** widget SHALL support message (String) or richMessage (InlineSpan)
**AND** widget SHALL support height, padding, margin, verticalOffset, preferBelow
**AND** widget SHALL support decoration, textStyle, textAlign
**AND** widget SHALL support waitDuration, showDuration, exitDuration
**AND** widget SHALL support triggerMode (longPress, tap, manual)
**AND** widget SHALL follow patterns documented in guide/07_material_widgets.md

---

### Requirement: Material Tab and Expansion Widgets

Material Design SHALL provide tabs and expandable list items, as documented in guide/07_material_widgets.md.

#### Scenario: TabBar and TabBarView provide tabbed navigation

**GIVEN** a developer needs tabbed interface
**WHEN** using TabBar and TabBarView widgets
**THEN** TabBar SHALL use RenderPhysicalModel + RenderFlex + RenderDecoratedBox for indicator
**AND** TabBar SHALL support tabs (List<Widget>), controller (TabController)
**AND** TabBar SHALL support isScrollable, padding, indicatorColor, indicator, indicatorSize
**AND** TabBar SHALL support labelColor, labelStyle, unselectedLabelColor, unselectedLabelStyle
**AND** TabBarView SHALL use RenderViewport + RenderSliverFillViewport
**AND** TabBarView SHALL support children (List<Widget>), controller, physics
**AND** widget SHALL follow patterns documented in guide/07_material_widgets.md

#### Scenario: ExpansionTile provides expandable list item

**GIVEN** a developer needs expandable list item
**WHEN** using ExpansionTile widget
**THEN** widget SHALL combine ListTile + AnimatedCrossFade RenderObjects
**AND** widget SHALL support leading, title, subtitle, trailing (expand icon)
**AND** widget SHALL support children (List<Widget>) for expanded content
**AND** widget SHALL support onExpansionChanged, initiallyExpanded, maintainState
**AND** widget SHALL support tilePadding, expandedCrossAxisAlignment, childrenPadding
**AND** widget SHALL support backgroundColor, collapsedBackgroundColor
**AND** widget SHALL follow patterns documented in guide/07_material_widgets.md

---

### Requirement: Material Base Widget

The Material widget SHALL provide base Material Design surface, as documented in guide/07_material_widgets.md.

#### Scenario: Material provides Material Design surface with elevation

**GIVEN** a developer needs Material Design surface
**WHEN** using Material widget
**THEN** widget SHALL use RenderPhysicalModel or RenderPhysicalShape + RenderInkFeatures
**AND** widget SHALL support type (canvas, card, circle, button, transparency)
**AND** widget SHALL support elevation, color, shadowColor, surfaceTintColor
**AND** widget SHALL support textStyle, borderRadius, shape, borderOnForeground
**AND** widget SHALL support clipBehavior, animationDuration
**AND** widget SHALL provide InkFeatures for ripple effects to descendants
**AND** widget SHALL follow patterns documented in guide/07_material_widgets.md

---

## Related Documentation

- **Guide:** `crates/flui_widgets/guide/07_material_widgets.md` - Detailed widget reference
- **Architecture:** `crates/flui_widgets/guide/WIDGET_ARCHITECTURE.md` - Widget organization patterns
- **Implementation:** `crates/flui_widgets/guide/IMPLEMENTATION_GUIDE.md` - Code examples

## Widgets Covered

**Total:** 20 Material Design widgets

**Scaffolding (4):**
- Scaffold, AppBar, BottomNavigationBar, Drawer

**Data Display (4):**
- Card, ListTile, Chip (with InputChip, ChoiceChip, FilterChip, ActionChip variants), Badge

**Feedback (6):**
- Dialog, AlertDialog, SnackBar, BottomSheet
- CircularProgressIndicator, LinearProgressIndicator, Tooltip

**Tabs and Expansion (3):**
- TabBar, TabBarView, ExpansionTile

**Base (1):**
- Material

**Functions (not widgets):**
- showDialog<T>()
- showModalBottomSheet<T>()
- ScaffoldMessenger.of(context).showSnackBar()
