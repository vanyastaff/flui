# Visual Effects and Interaction Specification

## Purpose

This specification references the detailed visual effects and interaction widget requirements documented in `crates/flui_widgets/guide/04_visual_and_interaction.md`.

## ADDED Requirements

### Requirement: Visual Effects Widget Categories

Visual effects widgets SHALL be organized into transformation, clipping, filtering, and decoration categories, as documented in guide/04_visual_and_interaction.md.

#### Scenario: Transformation widgets apply visual transformations

**GIVEN** a developer needs to transform child widget appearance
**WHEN** using transformation widgets (Opacity, Transform, Transform.rotate, Transform.translate, Transform.scale, RotatedBox)
**THEN** widget SHALL support bon builder pattern
**AND** widget SHALL support struct literal pattern
**AND** Opacity SHALL use RenderOpacity with opacity 0.0-1.0
**AND** Transform SHALL use RenderTransform with Matrix4
**AND** RotatedBox SHALL use RenderRotatedBox with quarterTurns (90° increments)
**AND** widget SHALL follow patterns documented in guide/04_visual_and_interaction.md

#### Scenario: Clipping widgets clip child rendering to shapes

**GIVEN** a developer needs to clip child rendering
**WHEN** using clipping widgets (ClipRect, ClipRRect, ClipOval, ClipPath)
**THEN** widget SHALL support bon builder pattern
**AND** widget SHALL use RenderClipRect, RenderClipRRect, RenderClipOval, or RenderClipPath
**AND** widget SHALL support clipper parameter for custom clip shapes
**AND** widget SHALL support clipBehavior (hardEdge, antiAlias, antiAliasWithSaveLayer)
**AND** ClipPath SHALL require CustomClipper<Path> parameter
**AND** widget SHALL follow patterns documented in guide/04_visual_and_interaction.md

#### Scenario: Filter widgets apply backdrop and color filters

**GIVEN** a developer needs to apply visual filters
**WHEN** using filter widgets (BackdropFilter, ColorFiltered, ShaderMask)
**THEN** BackdropFilter SHALL use RenderBackdropFilter with ImageFilter (blur, matrix)
**AND** ColorFiltered SHALL use RenderColorFiltered with ColorFilter
**AND** ShaderMask SHALL use RenderShaderMask with shaderCallback and blendMode
**AND** widget SHALL follow patterns documented in guide/04_visual_and_interaction.md

#### Scenario: Decoration widgets add backgrounds and borders

**GIVEN** a developer needs decorative backgrounds
**WHEN** using DecoratedBox widget
**THEN** widget SHALL use RenderDecoratedBox
**AND** widget SHALL support Decoration parameter (BoxDecoration, ShapeDecoration)
**AND** widget SHALL support DecorationPosition (background, foreground)
**AND** widget SHALL follow patterns documented in guide/04_visual_and_interaction.md

#### Scenario: RepaintBoundary isolates repaints for performance

**GIVEN** a developer needs to isolate expensive repaints
**WHEN** using RepaintBoundary widget
**THEN** widget SHALL use RenderRepaintBoundary
**AND** widget SHALL create separate layer for child rendering
**AND** child SHALL repaint independently from parent
**AND** widget SHALL follow patterns documented in guide/04_visual_and_interaction.md

---

### Requirement: Interaction Widget Categories

Interaction widgets SHALL handle pointer events, gestures, and user interactions, as documented in guide/04_visual_and_interaction.md.

#### Scenario: GestureDetector recognizes high-level gestures

**GIVEN** a developer needs to handle tap, pan, scale, or drag gestures
**WHEN** using GestureDetector widget
**THEN** widget SHALL use RenderPointerListener or RenderProxyBox
**AND** widget SHALL support tap callbacks (onTap, onTapDown, onTapUp, onTapCancel, onDoubleTap, onLongPress)
**AND** widget SHALL support pan callbacks (onPanStart, onPanUpdate, onPanEnd, onPanCancel)
**AND** widget SHALL support scale callbacks (onScaleStart, onScaleUpdate, onScaleEnd)
**AND** widget SHALL support drag callbacks (onVerticalDrag*, onHorizontalDrag*)
**AND** widget SHALL support force press, secondary tap, tertiary tap callbacks
**AND** widget SHALL support behavior parameter (HitTestBehavior)
**AND** widget SHALL coordinate gestures via gesture arena
**AND** widget SHALL follow patterns documented in guide/04_visual_and_interaction.md

#### Scenario: InkWell provides Material ripple effect

**GIVEN** a developer needs Material ripple effect on tap
**WHEN** using InkWell or InkResponse widget
**THEN** widget SHALL require Material ancestor
**AND** widget SHALL use RenderInkFeatures from Material ancestor
**AND** widget SHALL support tap, double tap, long press callbacks
**AND** widget SHALL support splashColor, highlightColor, borderRadius, customBorder
**AND** widget SHALL support mouseCursor, enableFeedback, excludeFromSemantics
**AND** InkResponse SHALL provide additional control over ripple behavior
**AND** widget SHALL follow patterns documented in guide/04_visual_and_interaction.md

#### Scenario: Listener captures raw pointer events

**GIVEN** a developer needs low-level pointer event handling
**WHEN** using Listener widget
**THEN** widget SHALL use RenderPointerListener
**AND** widget SHALL support onPointerDown, onPointerMove, onPointerUp, onPointerCancel
**AND** widget SHALL support onPointerHover, onPointerEnter, onPointerExit
**AND** widget SHALL support onPointerSignal (scroll wheel)
**AND** widget SHALL support behavior parameter (HitTestBehavior)
**AND** widget SHALL not perform gesture recognition
**AND** widget SHALL follow patterns documented in guide/04_visual_and_interaction.md

#### Scenario: MouseRegion tracks mouse hover state

**GIVEN** a developer needs mouse hover detection
**WHEN** using MouseRegion widget
**THEN** widget SHALL use RenderMouseRegion
**AND** widget SHALL support onEnter, onExit, onHover callbacks
**AND** widget SHALL support cursor parameter (MouseCursor)
**AND** widget SHALL support opaque parameter to block parent events
**AND** widget SHALL follow patterns documented in guide/04_visual_and_interaction.md

#### Scenario: Pointer blocking widgets control event propagation

**GIVEN** a developer needs to block or ignore pointer events
**WHEN** using AbsorbPointer or IgnorePointer widget
**THEN** AbsorbPointer SHALL use RenderAbsorbPointer and block events from propagating
**AND** IgnorePointer SHALL use RenderIgnorePointer and pass events through
**AND** both SHALL support absorbing/ignoring parameter (bool)
**AND** both SHALL support ignoringSemantics parameter
**AND** widget SHALL follow patterns documented in guide/04_visual_and_interaction.md

#### Scenario: Drag and drop widgets enable interactive dragging

**GIVEN** a developer needs drag and drop functionality
**WHEN** using Draggable, LongPressDraggable, or DragTarget widgets
**THEN** Draggable SHALL support child, feedback, childWhenDragging, data parameters
**AND** Draggable SHALL use RenderPointerListener + overlay for feedback
**AND** LongPressDraggable SHALL delay drag start until long press
**AND** DragTarget SHALL use RenderMetaData for hit testing
**AND** DragTarget SHALL support builder, onWillAcceptWithDetails, onAcceptWithDetails callbacks
**AND** widget SHALL follow patterns documented in guide/04_visual_and_interaction.md

#### Scenario: Dismissible enables swipe-to-dismiss

**GIVEN** a developer needs swipe-to-dismiss functionality
**WHEN** using Dismissible widget
**THEN** widget SHALL require key parameter for identity
**AND** widget SHALL use RenderPointerListener + RenderSlideTransition
**AND** widget SHALL support background, secondaryBackground parameters
**AND** widget SHALL support direction, dismissThresholds, movementDuration parameters
**AND** widget SHALL support onResize, onUpdate, onDismissed, confirmDismiss callbacks
**AND** widget SHALL follow patterns documented in guide/04_visual_and_interaction.md

#### Scenario: InteractiveViewer enables pan and zoom

**GIVEN** a developer needs pan and zoom functionality
**WHEN** using InteractiveViewer widget
**THEN** widget SHALL use RenderPointerListener + RenderTransform
**AND** widget SHALL support panEnabled, scaleEnabled parameters
**AND** widget SHALL support minScale, maxScale, boundaryMargin parameters
**AND** widget SHALL support transformationController for programmatic control
**AND** widget SHALL support onInteractionStart, onInteractionUpdate, onInteractionEnd callbacks
**AND** widget SHALL support InteractiveViewer.builder() for large viewports
**AND** widget SHALL follow patterns documented in guide/04_visual_and_interaction.md

---

## Related Documentation

- **Guide:** `crates/flui_widgets/guide/04_visual_and_interaction.md` - Detailed widget reference
- **Architecture:** `crates/flui_widgets/guide/WIDGET_ARCHITECTURE.md` - Widget organization patterns
- **Implementation:** `crates/flui_widgets/guide/IMPLEMENTATION_GUIDE.md` - Code examples

## Widgets Covered

**Total:** 23 visual effects and interaction widgets

**Visual Effects (13):**
- Opacity, Transform, Transform.rotate, Transform.translate, Transform.scale
- RotatedBox, ClipRect, ClipRRect, ClipOval, ClipPath
- BackdropFilter, DecoratedBox, ColorFiltered, ShaderMask, RepaintBoundary

**Interaction (10):**
- GestureDetector, InkWell, InkResponse, Listener, MouseRegion
- AbsorbPointer, IgnorePointer, Draggable, LongPressDraggable, DragTarget
- Dismissible, InteractiveViewer
- Scrollbar (also in scrolling widgets)
