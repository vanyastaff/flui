widgets library
The Flutter widgets framework.

To use, import package:flutter/widgets.dart.

See also:

flutter.dev/widgets for a catalog of commonly-used Flutter widgets.
Classes
AbsorbPointer
A widget that absorbs pointers during hit testing.
AbstractLayoutBuilder<LayoutInfoType>
An abstract superclass for widgets that defer their building until layout.
Accumulator
Mutable wrapper of an integer that can be passed by reference to track a value across a recursive stack.
Action<T extends Intent>
Base class for an action or command to be performed.
ActionDispatcher
An action dispatcher that invokes the actions given to it.
ActionListener
A helper widget for making sure that listeners on an action are removed properly.
Actions
A widget that maps Intents to Actions to be used by its descendants when invoking an Action.
ActivateAction
An Action that activates the currently focused control.
ActivateIntent
An Intent that activates the currently focused control.
Align
A widget that aligns its child within itself and optionally sizes itself based on the child's size.
Alignment
A point within a rectangle.
AlignmentDirectional
An offset that's expressed as a fraction of a Size, but whose horizontal component is dependent on the writing direction.
AlignmentGeometry
Base class for Alignment that allows for text-direction aware resolution.
AlignmentGeometryTween
An interpolation between two AlignmentGeometry.
AlignmentTween
An interpolation between two alignments.
AlignTransition
Animated version of an Align that animates its Align.alignment property.
AlwaysScrollableScrollPhysics
Scroll physics that always lets the user scroll.
AlwaysStoppedAnimation<T>
An animation that is always stopped at a given value.
AndroidView
Embeds an Android view in the Widget hierarchy.
AndroidViewSurface
Integrates an Android view with Flutter's compositor, touch, and semantics subsystems.
Animatable<T>
An object that can produce a value of type T given an Animation<double> as input.
AnimatedAlign
Animated version of Align which automatically transitions the child's position over a given duration whenever the given alignment changes.
AnimatedBuilder
A general-purpose widget for building animations.
AnimatedContainer
Animated version of Container that gradually changes its values over a period of time.
AnimatedCrossFade
A widget that cross-fades between two given children and animates itself between their sizes.
AnimatedDefaultTextStyle
Animated version of DefaultTextStyle which automatically transitions the default text style (the text style to apply to descendant Text widgets without explicit style) over a given duration whenever the given style changes.
AnimatedFractionallySizedBox
Animated version of FractionallySizedBox which automatically transitions the child's size over a given duration whenever the given widthFactor or heightFactor changes, as well as the position whenever the given alignment changes.
AnimatedGrid
A scrolling container that animates items when they are inserted into or removed from a grid. in a grid.
AnimatedGridState
The State for an AnimatedGrid that animates items when they are inserted or removed.
AnimatedList
A scrolling container that animates items when they are inserted or removed.
AnimatedListState
The AnimatedListState for AnimatedList, a scrolling list container that animates items when they are inserted or removed.
AnimatedModalBarrier
A widget that prevents the user from interacting with widgets behind itself, and can be configured with an animated color value.
AnimatedOpacity
Animated version of Opacity which automatically transitions the child's opacity over a given duration whenever the given opacity changes.
AnimatedPadding
Animated version of Padding which automatically transitions the indentation over a given duration whenever the given inset changes.
AnimatedPhysicalModel
Animated version of PhysicalModel.
AnimatedPositioned
Animated version of Positioned which automatically transitions the child's position over a given duration whenever the given position changes.
AnimatedPositionedDirectional
Animated version of PositionedDirectional which automatically transitions the child's position over a given duration whenever the given position changes.
AnimatedRotation
Animated version of Transform.rotate which automatically transitions the child's rotation over a given duration whenever the given rotation changes.
AnimatedScale
Animated version of Transform.scale which automatically transitions the child's scale over a given duration whenever the given scale changes.
AnimatedSize
Animated widget that automatically transitions its size over a given duration whenever the given child's size changes.
AnimatedSlide
Widget which automatically transitions the child's offset relative to its normal position whenever the given offset changes.
AnimatedSwitcher
A widget that by default does a cross-fade between a new widget and the widget previously set on the AnimatedSwitcher as a child.
AnimatedWidget
A widget that rebuilds when the given Listenable changes value.
AnimatedWidgetBaseState<T extends ImplicitlyAnimatedWidget>
A base class for widgets with implicit animations that need to rebuild their widget tree as the animation runs.
Animation<T>
A value which might change over time, moving forward or backward.
AnimationController
A controller for an animation.
AnimationMax<T extends num>
An animation that tracks the maximum of two other animations.
AnimationMean
An animation of doubles that tracks the mean of two other animations.
AnimationMin<T extends num>
An animation that tracks the minimum of two other animations.
AnimationStyle
Used to override the default parameters of an animation.
AnnotatedRegion<T extends Object>
Annotates a region of the layer tree with a value.
AppKitView
Widget that contains a macOS AppKit view.
AppLifecycleListener
A listener that can be used to listen to changes in the application lifecycle.
AspectRatio
A widget that attempts to size the child to a specific aspect ratio.
AssetBundle
A collection of resources used by the application.
AssetBundleImageKey
Key for the image obtained by an AssetImage or ExactAssetImage.
AssetBundleImageProvider
A subclass of ImageProvider that knows about AssetBundles.
AssetImage
Fetches an image from an AssetBundle, having determined the exact image to use based on the context.
AsyncSnapshot<T>
Immutable representation of the most recent interaction with an asynchronous computation.
AutocompleteFirstOptionIntent
An Intent to highlight the first option in the autocomplete list.
AutocompleteHighlightedOption
An inherited widget used to indicate which autocomplete option should be highlighted for keyboard navigation.
AutocompleteLastOptionIntent
An Intent to highlight the last option in the autocomplete list.
AutocompleteNextOptionIntent
An Intent to highlight the next option in the autocomplete list.
AutocompleteNextPageOptionIntent
An Intent to highlight the option one page after the currently highlighted option in the autocomplete list.
AutocompletePreviousOptionIntent
An Intent to highlight the previous option in the autocomplete list.
AutocompletePreviousPageOptionIntent
An Intent to highlight the option one page before the currently highlighted option in the autocomplete list.
AutofillGroup
An AutofillScope widget that groups AutofillClients together.
AutofillGroupState
State associated with an AutofillGroup widget.
AutofillHints
A collection of commonly used autofill hint strings on different platforms.
AutomaticKeepAlive
Allows subtrees to request to be kept alive in lazy lists.
AutomaticNotchedShape
A NotchedShape created from ShapeBorders.
BackButtonDispatcher
Report to a Router when the user taps the back button on platforms that support back buttons (such as Android).
BackButtonListener
A convenience widget that registers a callback for when the back button is pressed.
BackdropFilter
A widget that applies a filter to the existing painted content and then paints child.
BackdropGroup
A widget that establishes a shared backdrop layer for all child BackdropFilter widgets that opt into using it.
BackdropKey
A backdrop key uniquely identifies the backdrop that a BackdropFilterLayer samples from.
BallisticScrollActivity
The activity a scroll view performs after being set into motion.
Banner
Displays a diagonal message above the corner of another widget.
BannerPainter
Paints a Banner.
Baseline
A widget that positions its child according to the child's baseline.
BeveledRectangleBorder
A rectangular border with flattened or "beveled" corners.
BlockSemantics
A widget that drops the semantics of all widget that were painted before it in the same semantic container.
Border
A border of a box, comprised of four sides: top, right, bottom, left.
BorderDirectional
A border of a box, comprised of four sides, the lateral sides of which flip over based on the reading direction.
BorderRadius
An immutable set of radii for each corner of a rectangle.
BorderRadiusDirectional
An immutable set of radii for each corner of a rectangle, but with the corners specified in a manner dependent on the writing direction.
BorderRadiusGeometry
Base class for BorderRadius that allows for text-direction aware resolution.
BorderRadiusTween
An interpolation between two BorderRadiuss.
BorderSide
A side of a border of a box.
BorderTween
An interpolation between two Borders.
BottomNavigationBarItem
An interactive button within either material's BottomNavigationBar or the iOS themed CupertinoTabBar with an icon and title.
BouncingScrollPhysics
Scroll physics for environments that allow the scroll offset to go beyond the bounds of the content, but then bounce the content back to the edge of those bounds.
BouncingScrollSimulation
An implementation of scroll physics that matches iOS.
BoxBorder
Base class for box borders that can paint as rectangles, circles, or rounded rectangles.
BoxConstraints
Immutable layout constraints for RenderBox layout.
BoxConstraintsTween
An interpolation between two BoxConstraints.
BoxDecoration
An immutable description of how to paint a box.
BoxPainter
A stateful class that can paint a particular Decoration.
BoxScrollView
A ScrollView that uses a single child layout model.
BoxShadow
A shadow cast by a box.
BuildContext
A handle to the location of a widget in the widget tree.
Builder
A stateless utility widget whose build method uses its builder callback to create the widget's child.
BuildOwner
Manager class for the widgets framework.
BuildScope
A class that determines the scope of a BuildOwner.buildScope operation.
ButtonActivateIntent
An Intent that activates the currently focused button.
CallbackAction<T extends Intent>
An Action that takes a callback in order to configure it without having to create an explicit Action subclass just to call a callback.
CallbackShortcuts
A widget that binds key combinations to specific callbacks.
Canvas
An interface for recording graphical operations.
CapturedThemes
Stores a list of captured InheritedThemes that can be wrapped around a child Widget.
CatmullRomCurve
An animation easing curve that passes smoothly through the given control points using a centripetal Catmull-Rom spline.
CatmullRomSpline
A 2D spline that passes smoothly through the given control points using a centripetal Catmull-Rom spline.
Center
A widget that centers its child within itself.
ChangeNotifier
A class that can be extended or mixed in that provides a change notification API using VoidCallback for notifications.
CharacterActivator
A shortcut combination that is triggered by a key event that produces a specific character.
CharacterRange
A range of characters of a Characters.
Characters
The characters of a string.
CheckedModeBanner
Displays a Banner saying "DEBUG" when running in debug mode. MaterialApp builds one of these by default.
ChildBackButtonDispatcher
A variant of BackButtonDispatcher which listens to notifications from a parent back button dispatcher, and can take priority from its parent for the handling of such notifications.
ChildVicinity
The relative position of a child in a TwoDimensionalViewport in relation to other children of the viewport.
CircleBorder
A border that fits a circle within the available space.
CircularNotchedRectangle
A rectangle with a smooth circular notch.
ClampingScrollPhysics
Scroll physics for environments that prevent the scroll offset from reaching beyond the bounds of the content.
ClampingScrollSimulation
An implementation of scroll physics that aligns with Android.
ClipboardStatusNotifier
A ValueNotifier whose value indicates whether the current contents of the clipboard can be pasted.
ClipContext
Clip utilities used by PaintingContext.
ClipOval
A widget that clips its child using an oval.
ClipPath
A widget that clips its child using a path.
ClipRect
A widget that clips its child using a rectangle.
ClipRRect
A widget that clips its child using a rounded rectangle.
ClipRSuperellipse
A widget that clips its child using a rounded superellipse.
Color
An immutable color value in ARGB format.
ColoredBox
A widget that paints its area with a specified Color and then draws its child on top of that color.
ColorFilter
A description of a color filter to apply when drawing a shape or compositing a layer with a particular Paint. A color filter is a function that takes two colors, and outputs one color. When applied during compositing, it is independently applied to each pixel of the layer being drawn before the entire layer is merged with the destination.
ColorFiltered
Applies a ColorFilter to its child.
ColorProperty
DiagnosticsProperty that has an Color as value.
ColorSwatch<T>
A color that has a small table of related colors called a "swatch".
ColorTween
An interpolation between two colors.
Column
A widget that displays its children in a vertical array.
ComponentElement
An Element that composes other Elements.
CompositedTransformFollower
A widget that follows a CompositedTransformTarget.
CompositedTransformTarget
A widget that can be targeted by a CompositedTransformFollower.
CompoundAnimation<T>
An interface for combining multiple Animations. Subclasses need only implement the value getter to control how the child animations are combined. Can be chained to combine more than 2 animations.
ConstantTween<T>
A tween with a constant value.
ConstrainedBox
A widget that imposes additional constraints on its child.
ConstrainedLayoutBuilder<ConstraintType extends Constraints>
A specialized AbstractLayoutBuilder whose widget subtree depends on the incoming ConstraintType that will be imposed on the widget.
ConstraintsTransformBox
A container widget that applies an arbitrary transform to its constraints, and sizes its child using the resulting BoxConstraints, optionally clipping, or treating the overflow as an error.
Container
A convenience widget that combines common painting, positioning, and sizing widgets.
ContentInsertionConfiguration
Configures the ability to insert media content through the soft keyboard.
ContextAction<T extends Intent>
An abstract Action subclass that adds an optional BuildContext to the isEnabled and invoke methods to be able to provide context to actions.
ContextMenuButtonItem
The type and callback for a context menu button.
ContextMenuController
Builds and manages a context menu at a given location.
ContinuousRectangleBorder
A rectangular border with smooth continuous transitions between the straight sides and the rounded corners.
CopySelectionTextIntent
An Intent that represents a user interaction that attempts to copy or cut the current selection in the field.
Cubic
A cubic polynomial mapping of the unit interval.
Curve
An parametric animation easing curve, i.e. a mapping of the unit interval to the unit interval.
Curve2D
Abstract class that defines an API for evaluating 2D parametric curves.
Curve2DSample
A class that holds a sample of a 2D parametric curve, containing the value (the X, Y coordinates) of the curve at the parametric value t.
CurvedAnimation
An animation that applies a curve to another animation.
Curves
A collection of common animation curves.
CurveTween
Transforms the value of the given animation by the given curve.
CustomClipper<T>
An interface for providing custom clips.
CustomMultiChildLayout
A widget that uses a delegate to size and position multiple children.
CustomPaint
A widget that provides a canvas on which to draw during the paint phase.
CustomPainter
The interface used by CustomPaint (in the widgets library) and RenderCustomPaint (in the rendering library).
CustomPainterSemantics
Contains properties describing information drawn in a rectangle contained by the Canvas used by a CustomPaint.
CustomScrollView
A ScrollView that creates custom scroll effects using slivers.
CustomSingleChildLayout
A widget that defers the layout of its single child to a delegate.
DebugCreator
A wrapper class for the Element that is the creator of a RenderObject.
DecoratedBox
A widget that paints a Decoration either before or after its child paints.
DecoratedBoxTransition
Animated version of a DecoratedBox that animates the different properties of its Decoration.
DecoratedSliver
A sliver widget that paints a Decoration either before or after its child paints.
Decoration
A description of a box decoration (a decoration applied to a Rect).
DecorationImage
An image for a box decoration.
DecorationImagePainter
The painter for a DecorationImage.
DecorationTween
An interpolation between two Decorations.
DefaultAssetBundle
A widget that determines the default asset bundle for its descendants.
DefaultPlatformMenuDelegate
The platform menu delegate that handles the built-in macOS platform menu generation using the 'flutter/menu' channel.
DefaultSelectionStyle
The selection style to apply to descendant EditableText widgets which don't have an explicit style.
DefaultTextEditingShortcuts
A widget with the shortcuts used for the default text editing behavior.
DefaultTextHeightBehavior
The TextHeightBehavior that will apply to descendant Text and EditableText widgets which have not explicitly set Text.textHeightBehavior.
DefaultTextStyle
The text style to apply to descendant Text widgets which don't have an explicit style.
DefaultTextStyleTransition
Animated version of a DefaultTextStyle that animates the different properties of its TextStyle.
DefaultTransitionDelegate<T>
The default implementation of TransitionDelegate that the Navigator will use if its Navigator.transitionDelegate is not specified.
DefaultWidgetsLocalizations
US English localizations for the widgets library.
DeleteCharacterIntent
Deletes the character before or after the caret location, based on whether forward is true.
DeleteToLineBreakIntent
Deletes from the current caret location to the previous or next soft or hard line break, based on whether forward is true.
DeleteToNextWordBoundaryIntent
Deletes from the current caret location to the previous or next word boundary, based on whether forward is true.
DesktopTextSelectionToolbarLayoutDelegate
Positions the toolbar at anchor if it fits, otherwise moves it so that it just fits fully on-screen.
DevToolsDeepLinkProperty
Debugging message for DevTools deep links.
DiagnosticsNode
Defines diagnostics data for a value.
DirectionalCaretMovementIntent
A DirectionalTextEditingIntent that moves the caret or the selection to a new location.
DirectionalFocusAction
An Action that moves the focus to the focusable node in the direction configured by the associated DirectionalFocusIntent.direction.
DirectionalFocusIntent
An Intent that represents moving to the next focusable node in the given direction.
Directionality
A widget that determines the ambient directionality of text and text-direction-sensitive render objects.
DirectionalTextEditingIntent
A text editing related Intent that performs an operation towards a given direction of the current caret location.
DisableWidgetInspectorScope
Disables the Flutter DevTools Widget Inspector for a Widget subtree.
DismissAction
An Action that dismisses the focused widget.
Dismissible
A widget that can be dismissed by dragging in the indicated direction.
DismissIntent
An Intent that dismisses the currently focused widget.
DismissMenuAction
An action that closes all the menus associated with the given MenuController.
DismissUpdateDetails
Details for DismissUpdateCallback.
DisplayFeatureSubScreen
Positions child such that it avoids overlapping any DisplayFeature that splits the screen into sub-screens.
DisposableBuildContext<T extends State<StatefulWidget>>
Provides non-leaking access to a BuildContext.
DoNothingAction
An Action that doesn't perform any action when invoked.
DoNothingAndStopPropagationIntent
An Intent that is bound to a DoNothingAction, but, in addition to not performing an action, also stops the propagation of the key event bound to this intent to other key event handlers in the focus chain.
DoNothingAndStopPropagationTextIntent
An Intent to send the event straight to the engine.
DoNothingIntent
An Intent that is bound to a DoNothingAction.
DragBoundary
Provides a DragBoundaryDelegate for its descendants whose bounds are those defined by this widget.
DragBoundaryDelegate<T>
The interface for defining the algorithm for a boundary that a specified shape is dragged within.
DragDownDetails
Details object for callbacks that use GestureDragDownCallback.
DragEndDetails
Details object for callbacks that use GestureDragEndCallback.
Draggable<T extends Object>
A widget that can be dragged from to a DragTarget.
DraggableDetails
Represents the details when a specific pointer event occurred on the Draggable.
DraggableScrollableActuator
A widget that can notify a descendent DraggableScrollableSheet that it should reset its position to the initial state.
DraggableScrollableController
Controls a DraggableScrollableSheet.
DraggableScrollableNotification
A Notification related to the extent, which is the size, and scroll offset, which is the position of the child list, of the DraggableScrollableSheet.
DraggableScrollableSheet
A container for a Scrollable that responds to drag gestures by resizing the scrollable until a limit is reached, and then scrolling.
DragScrollActivity
The activity a scroll view performs when the user drags their finger across the screen.
DragStartDetails
Details object for callbacks that use GestureDragStartCallback.
DragTarget<T extends Object>
A widget that receives data when a Draggable widget is dropped.
DragTargetDetails<T>
Represents the details when a pointer event occurred on the DragTarget.
DragUpdateDetails
Details object for callbacks that use GestureDragUpdateCallback.
DrivenScrollActivity
An activity that drives a scroll view through a given animation.
DualTransitionBuilder
A transition builder that animates its child based on the AnimationStatus of the provided animation.
EdgeDraggingAutoScroller
An auto scroller that scrolls the scrollable if a drag gesture drags close to its edge.
EdgeInsets
An immutable set of offsets in each of the four cardinal directions.
EdgeInsetsDirectional
An immutable set of offsets in each of the four cardinal directions, but whose horizontal components are dependent on the writing direction.
EdgeInsetsGeometry
Base class for EdgeInsets that allows for text-direction aware resolution.
EdgeInsetsGeometryTween
An interpolation between two EdgeInsetsGeometrys.
EdgeInsetsTween
An interpolation between two EdgeInsetss.
EditableText
A basic text input field.
EditableTextState
State for an EditableText.
EditableTextTapOutsideIntent
An Intent that represents a tap outside the field.
EditableTextTapUpOutsideIntent
An Intent that represents a tap outside the field.
ElasticInCurve
An oscillating curve that grows in magnitude while overshooting its bounds.
ElasticInOutCurve
An oscillating curve that grows and then shrinks in magnitude while overshooting its bounds.
ElasticOutCurve
An oscillating curve that shrinks in magnitude while overshooting its bounds.
Element
An instantiation of a Widget at a particular location in the tree.
EmptyTextSelectionControls
Text selection controls that do not show any toolbars or handles.
EnableWidgetInspectorScope
Enables the Flutter DevTools Widget Inspector for a Widget subtree.
ErrorDescription
An explanation of the problem and its cause, any information that may help track down the problem, background information, etc.
ErrorHint
An ErrorHint provides specific, non-obvious advice that may be applicable.
ErrorSummary
A short (one line) description of the problem that was detected.
ErrorWidget
A widget that renders an exception's message.
ExactAssetImage
Fetches an image from an AssetBundle, associating it with the given scale.
ExcludeFocus
A widget that controls whether or not the descendants of this widget are focusable.
ExcludeFocusTraversal
A widget that controls whether or not the descendants of this widget are traversable.
ExcludeSemantics
A widget that drops all the semantics of its descendants.
Expanded
A widget that expands a child of a Row, Column, or Flex so that the child fills the available space.
ExpandSelectionToDocumentBoundaryIntent
Expands the current selection to the document boundary in the direction given by forward.
ExpandSelectionToLineBreakIntent
Expands the current selection to the closest line break in the direction given by forward.
Expansible
A StatefulWidget that expands and collapses.
ExpansibleController
A controller for managing the expansion state of an Expansible.
ExtendSelectionByCharacterIntent
Extends, or moves the current selection from the current TextSelection.extent position to the previous or the next character boundary.
ExtendSelectionByPageIntent
Scrolls up or down by page depending on the forward parameter. Extends the selection up or down by page based on the forward parameter.
ExtendSelectionToDocumentBoundaryIntent
Extends, or moves the current selection from the current TextSelection.extent position to the start or the end of the document.
ExtendSelectionToLineBreakIntent
Extends, or moves the current selection from the current TextSelection.extent position to the closest line break in the direction given by forward.
ExtendSelectionToNextParagraphBoundaryIntent
Extends, or moves the current selection from the current TextSelection.extent position to the previous or the next paragraph boundary.
ExtendSelectionToNextParagraphBoundaryOrCaretLocationIntent
Extends, or moves the current selection from the current TextSelection.extent position to the previous or the next paragraph boundary depending on the forward parameter.
ExtendSelectionToNextWordBoundaryIntent
Extends, or moves the current selection from the current TextSelection.extent position to the previous or the next word boundary.
ExtendSelectionToNextWordBoundaryOrCaretLocationIntent
Extends, or moves the current selection from the current TextSelection.extent position to the previous or the next word boundary, or the TextSelection.base position if it's closer in the move direction.
ExtendSelectionVerticallyToAdjacentLineIntent
Extends, or moves the current selection from the current TextSelection.extent position to the closest position on the adjacent line.
ExtendSelectionVerticallyToAdjacentPageIntent
Expands, or moves the current selection from the current TextSelection.extent position to the closest position on the adjacent page.
FadeInImage
An image that shows a placeholder image while the target image is loading, then fades in the new image when it loads.
FadeTransition
Animates the opacity of a widget.
Feedback
Provides platform-specific acoustic and/or haptic feedback for certain actions.
FileImage
Decodes the given File object as an image, associating it with the given scale.
FittedBox
Scales and positions its child within itself according to fit.
FittedSizes
The pair of sizes returned by applyBoxFit.
FixedColumnWidth
Sizes the column to a specific number of pixels.
FixedExtentMetrics
Metrics for a ScrollPosition to a scroll view with fixed item sizes.
FixedExtentScrollController
A controller for scroll views whose items have the same size.
FixedExtentScrollPhysics
A snapping physics that always lands directly on items instead of anywhere within the scroll extent.
FixedScrollMetrics
An immutable snapshot of values associated with a Scrollable viewport.
Flex
A widget that displays its children in a one-dimensional array.
FlexColumnWidth
Sizes the column by taking a part of the remaining space once all the other columns have been laid out.
Flexible
A widget that controls how a child of a Row, Column, or Flex flexes.
FlippedCurve
A curve that is the reversed inversion of its given curve.
FlippedTweenSequence
Enables creating a flipped Animation whose value is defined by a sequence of Tweens.
Flow
A widget that sizes and positions children efficiently, according to the logic in a FlowDelegate.
FlowDelegate
A delegate that controls the appearance of a flow layout.
FlowPaintingContext
A context in which a FlowDelegate paints.
FlutterErrorDetails
Class for information provided to FlutterExceptionHandler callbacks.
FlutterLogo
The Flutter logo, in widget form. This widget respects the IconTheme. For guidelines on using the Flutter logo, visit https://flutter.dev/brand.
FlutterLogoDecoration
An immutable description of how to paint Flutter's logo.
Focus
A widget that manages a FocusNode to allow keyboard focus to be given to this widget and its descendants.
FocusableActionDetector
A widget that combines the functionality of Actions, Shortcuts, MouseRegion and a Focus widget to create a detector that defines actions and key bindings, and provides callbacks for handling focus and hover highlights.
FocusAttachment
An attachment point for a FocusNode.
FocusManager
Manages the focus tree.
FocusNode
An object that can be used by a stateful widget to obtain the keyboard focus and to handle keyboard events.
FocusOrder
Base class for all sort orders for OrderedTraversalPolicy traversal.
FocusScope
A FocusScope is similar to a Focus, but also serves as a scope for its descendants, restricting focus traversal to the scoped controls.
FocusScopeNode
A subclass of FocusNode that acts as a scope for its descendants, maintaining information about which descendant is currently or was last focused.
FocusTraversalGroup
A widget that describes the inherited focus policy for focus traversal for its descendants, grouping them into a separate traversal group.
FocusTraversalOrder
An inherited widget that describes the order in which its child subtree should be traversed.
FocusTraversalPolicy
Determines how focusable widgets are traversed within a FocusTraversalGroup.
FontFeature
A feature tag and value that affect the selection of glyphs in a font.
FontVariation
An axis tag and value that can be used to customize variable fonts.
FontWeight
The thickness of the glyphs used to draw the text.
ForcePressDetails
Details object for callbacks that use GestureForcePressStartCallback, GestureForcePressPeakCallback, GestureForcePressEndCallback or GestureForcePressUpdateCallback.
Form
An optional container for grouping together multiple form field widgets (e.g. TextField widgets).
FormField<T>
A single form field.
FormFieldState<T>
The current state of a FormField. Passed to the FormFieldBuilder method for use in constructing the form field's widget.
FormState
State associated with a Form widget.
FractionallySizedBox
A widget that sizes its child to a fraction of the total available space. For more details about the layout algorithm, see RenderFractionallySizedOverflowBox.
FractionalOffset
An offset that's expressed as a fraction of a Size.
FractionalOffsetTween
An interpolation between two fractional offsets.
FractionalTranslation
Applies a translation transformation before painting its child.
FractionColumnWidth
Sizes the column to a fraction of the table's constraints' maxWidth.
FutureBuilder<T>
A widget that builds itself based on the latest snapshot of interaction with a Future.
GestureDetector
A widget that detects gestures.
GestureRecognizerFactory<T extends GestureRecognizer>
Factory for creating gesture recognizers.
GestureRecognizerFactoryWithHandlers<T extends GestureRecognizer>
Factory for creating gesture recognizers that delegates to callbacks.
GlobalKey<T extends State<StatefulWidget>>
A key that is unique across the entire app.
GlobalObjectKey<T extends State<StatefulWidget>>
A global key that takes its identity from the object used as its value.
GlowingOverscrollIndicator
A visual indication that a scroll view has overscrolled.
GlyphInfo
The measurements of a character (or a sequence of visually connected characters) within a paragraph.
Gradient
A 2D gradient.
GradientRotation
A GradientTransform that rotates the gradient around the center-point of its bounding box.
GradientTransform
Base class for transforming gradient shaders without applying the same transform to the entire canvas.
GridPaper
A widget that draws a rectilinear grid of lines one pixel wide.
GridView
A scrollable, 2D array of widgets.
Hero
A widget that marks its child as being a candidate for hero animations.
HeroController
A Navigator observer that manages Hero transitions.
HeroControllerScope
An inherited widget to host a hero controller.
HeroMode
Enables or disables Heroes in the widget subtree.
HoldScrollActivity
A scroll activity that does nothing but can be released to resume normal idle behavior.
HSLColor
A color represented using alpha, hue, saturation, and lightness.
HSVColor
A color represented using alpha, hue, saturation, and value.
HtmlElementView
Embeds an HTML element in the Widget hierarchy in Flutter web.
Icon
A graphical icon widget drawn with a glyph from a font described in an IconData such as material's predefined IconDatas in Icons.
IconData
A description of an icon fulfilled by a font glyph.
IconDataProperty
DiagnosticsProperty that has an IconData as value.
IconTheme
Controls the default properties of icons in a widget subtree.
IconThemeData
Defines the size, font variations, color, opacity, and shadows of icons.
IdleScrollActivity
A scroll activity that does nothing.
IgnoreBaseline
A widget that causes the parent to ignore the child for the purposes of baseline alignment.
IgnorePointer
A widget that is invisible during hit testing.
Image
A widget that displays an image.
ImageCache
Class for caching images.
ImageCacheStatus
Information about how the ImageCache is tracking an image.
ImageChunkEvent
An immutable notification of image bytes that have been incrementally loaded.
ImageConfiguration
Configuration information passed to the ImageProvider.resolve method to select a specific image.
ImageFiltered
Applies an ImageFilter to its child.
ImageIcon
An icon that comes from an ImageProvider, e.g. an AssetImage.
ImageInfo
A dart:ui.Image object with its corresponding scale.
ImageProvider<T extends Object>
Identifies an image without committing to the precise final asset. This allows a set of images to be identified and for the precise image to later be resolved based on the environment, e.g. the device pixel ratio.
ImageShader
A shader (as used by Paint.shader) that tiles an image.
ImageSizeInfo
Tracks the bytes used by a dart:ui.Image compared to the bytes needed to paint that image without scaling it.
ImageStream
A handle to an image resource.
ImageStreamCompleter
Base class for those that manage the loading of dart:ui.Image objects for ImageStreams.
ImageStreamCompleterHandle
An opaque handle that keeps an ImageStreamCompleter alive even if it has lost its last listener.
ImageStreamListener
Interface for receiving notifications about the loading of an image.
ImplicitlyAnimatedWidget
An abstract class for building widgets that animate changes to their properties.
ImplicitlyAnimatedWidgetState<T extends ImplicitlyAnimatedWidget>
A base class for the State of widgets with implicit animations.
IndexedSemantics
A widget that annotates the child semantics with an index.
IndexedSlot<T extends Element?>
A value for Element.slot used for children of MultiChildRenderObjectElements.
IndexedStack
A Stack that shows a single child from a list of children.
InheritedElement
An Element that uses an InheritedWidget as its configuration.
InheritedModel<T>
An InheritedWidget that's intended to be used as the base class for models whose dependents may only depend on one part or "aspect" of the overall model.
InheritedModelElement<T>
An Element that uses a InheritedModel as its configuration.
InheritedNotifier<T extends Listenable>
An inherited widget for a Listenable notifier, which updates its dependencies when the notifier is triggered.
InheritedTheme
An InheritedWidget that defines visual properties like colors and text styles, which the child's subtree depends on.
InheritedWidget
Base class for widgets that efficiently propagate information down the tree.
InlineSpan
An immutable span of inline content which forms part of a paragraph.
InlineSpanSemanticsInformation
The textual and semantic label information for an InlineSpan.
InspectorButton
An abstract base class for creating Material or Cupertino-styled inspector buttons.
InspectorReferenceData
Structure to help reference count Dart objects referenced by a GUI tool using WidgetInspectorService.
InspectorSelection
Mutable selection state of the inspector.
InspectorSerializationDelegate
A delegate that configures how a hierarchy of DiagnosticsNodes are serialized by the Flutter Inspector.
Intent
An abstract class representing a particular configuration of an Action.
InteractiveViewer
A widget that enables pan and zoom interactions with its child.
Interval
A curve that is 0.0 until begin, then curved (according to curve) from 0.0 at begin to 1.0 at end, then remains 1.0 past end.
IntrinsicColumnWidth
Sizes the column according to the intrinsic dimensions of all the cells in that column.
IntrinsicHeight
A widget that sizes its child to the child's intrinsic height.
IntrinsicWidth
A widget that sizes its child to the child's maximum intrinsic width.
IntTween
An interpolation between two integers that rounds.
IOSSystemContextMenuItem
Describes a context menu button that will be rendered in the iOS system context menu and not by Flutter itself.
IOSSystemContextMenuItemCopy
Creates an instance of IOSSystemContextMenuItem for the system's built-in copy button.
IOSSystemContextMenuItemCut
Creates an instance of IOSSystemContextMenuItem for the system's built-in cut button.
IOSSystemContextMenuItemLiveText
Creates an instance of IOSSystemContextMenuItem for the system's built-in Live Text button.
IOSSystemContextMenuItemLookUp
Creates an instance of IOSSystemContextMenuItem for the system's built-in look up button.
IOSSystemContextMenuItemPaste
Creates an instance of IOSSystemContextMenuItem for the system's built-in paste button.
IOSSystemContextMenuItemSearchWeb
Creates an instance of IOSSystemContextMenuItem for the system's built-in search web button.
IOSSystemContextMenuItemSelectAll
Creates an instance of IOSSystemContextMenuItem for the system's built-in select all button.
IOSSystemContextMenuItemShare
Creates an instance of IOSSystemContextMenuItem for the system's built-in share button.
KeepAlive
Mark a child as needing to stay alive even when it's in a lazy list that would otherwise remove it.
KeepAliveHandle
A Listenable which can be manually triggered.
KeepAliveNotification
Indicates that the subtree through which this notification bubbles must be kept alive even if it would normally be discarded as an optimization.
Key
A Key is an identifier for Widgets, Elements and SemanticsNodes.
KeyboardInsertedContent
A class representing rich content (such as a PNG image) inserted via the system input method.
KeyboardListener
A widget that calls a callback whenever the user presses or releases a key on a keyboard.
KeyedSubtree
A widget that builds its child.
KeyEvent
Defines the interface for keyboard key events.
KeySet<T extends KeyboardKey>
A set of KeyboardKeys that can be used as the keys in a Map.
LabeledGlobalKey<T extends State<StatefulWidget>>
A global key with a debugging label.
LayerLink
An object that a LeaderLayer can register with.
LayoutBuilder
Builds a widget tree that can depend on the parent widget's size.
LayoutChangedNotification
Indicates that the layout of one of the descendants of the object receiving this notification has changed in some way, and that therefore any assumptions about that layout are no longer valid.
LayoutId
Metadata for identifying children in a CustomMultiChildLayout.
LeafRenderObjectElement
An Element that uses a LeafRenderObjectWidget as its configuration.
LeafRenderObjectWidget
A superclass for RenderObjectWidgets that configure RenderObject subclasses that have no children.
LexicalFocusOrder
Can be given to a FocusTraversalOrder widget to use a String to assign a lexical order to a widget subtree that is using a OrderedTraversalPolicy to define the order in which widgets should be traversed with the keyboard.
LimitedBox
A box that limits its size only when it's unconstrained.
LinearBorder
An OutlinedBorder like BoxBorder that allows one to define a rectangular (box) border in terms of zero to four LinearBorderEdges, each of which is rendered as a single line.
LinearBorderEdge
Defines the relative size and alignment of one LinearBorder edge.
LinearGradient
A 2D linear gradient.
LineMetrics
LineMetrics stores the measurements and statistics of a single line in the paragraph.
ListBody
A widget that arranges its children sequentially along a given axis, forcing them to the dimension of the parent in the other axis.
Listenable
An object that maintains a list of listeners.
ListenableBuilder
A general-purpose widget for building a widget subtree when a Listenable changes.
Listener
A widget that calls callbacks in response to common pointer events.
ListView
A scrollable list of widgets arranged linearly.
ListWheelChildBuilderDelegate
A delegate that supplies children for ListWheelScrollView using a builder callback.
ListWheelChildDelegate
A delegate that supplies children for ListWheelScrollView.
ListWheelChildListDelegate
A delegate that supplies children for ListWheelScrollView using an explicit list.
ListWheelChildLoopingListDelegate
A delegate that supplies infinite children for ListWheelScrollView by looping an explicit list.
ListWheelElement
Element that supports building children lazily for ListWheelViewport.
ListWheelScrollView
A box in which children on a wheel can be scrolled.
ListWheelViewport
A viewport showing a subset of children on a wheel.
LiveTextInputStatusNotifier
A ValueNotifier whose value indicates whether the current device supports the Live Text (OCR) function.
Locale
An identifier used to select a user's language and formatting preferences.
LocalHistoryEntry
An entry in the history of a LocalHistoryRoute.
Localizations
Defines the Locale for its child and the localized resources that the child depends on.
LocalizationsDelegate<T>
A factory for a set of localized resources of type T, to be loaded by a Localizations widget.
LocalizationsResolver
A helper class used to manage localization resolution.
LocalKey
A key that is not a GlobalKey.
LogicalKeySet
A set of LogicalKeyboardKeys that can be used as the keys in a map.
LongPressDraggable<T extends Object>
Makes its child draggable starting from long press.
LongPressEndDetails
Details for callbacks that use GestureLongPressEndCallback.
LongPressMoveUpdateDetails
Details for callbacks that use GestureLongPressMoveUpdateCallback.
LongPressStartDetails
Details for callbacks that use GestureLongPressStartCallback.
LookupBoundary
A lookup boundary controls what entities are visible to descendants of the boundary via the static lookup methods provided by the boundary.
MagnifierController
A controller for a magnifier.
MagnifierDecoration
The decorations to put around the loupe in a RawMagnifier.
MagnifierInfo
A data class that contains the geometry information of text layouts and selection gestures, used to position magnifiers.
MaskFilter
A mask filter to apply to shapes as they are painted. A mask filter is a function that takes a bitmap of color pixels, and returns another bitmap of color pixels.
Matrix4
4D Matrix. Values are stored in column major order.
Matrix4Tween
An interpolation between two Matrix4s.
MatrixTransition
Animates the Matrix4 of a transformed widget.
MatrixUtils
Utility functions for working with matrices.
MaxColumnWidth
Sizes the column such that it is the size that is the maximum of two column width specifications.
MediaQuery
Establishes a subtree in which media queries resolve to the given data.
MediaQueryData
Information about a piece of media (e.g., a window).
MemoryImage
Decodes the given Uint8List buffer as an image, associating it with the given scale.
MenuController
A controller used to manage a menu created by a subclass of RawMenuAnchor, such as MenuAnchor, MenuBar, SubmenuButton.
MergeSemantics
A widget that merges the semantics of its descendants.
MetaData
Holds opaque meta data in the render tree.
MinColumnWidth
Sizes the column such that it is the size that is the minimum of two column width specifications.
ModalBarrier
A widget that prevents the user from interacting with widgets behind itself.
ModalRoute<T>
A route that blocks interaction with previous routes.
MouseCursor
An interface for mouse cursor definitions.
MouseRegion
A widget that tracks the movement of mice.
MultiChildLayoutDelegate
A delegate that controls the layout of multiple children.
MultiChildRenderObjectElement
An Element that uses a MultiChildRenderObjectWidget as its configuration.
MultiChildRenderObjectWidget
A superclass for RenderObjectWidgets that configure RenderObject subclasses that have a single list of children. (This superclass only provides the storage for that child list, it doesn't actually provide the updating logic.)
MultiFrameImageStreamCompleter
Manages the decoding and scheduling of image frames.
MultiSelectableSelectionContainerDelegate
A delegate that handles events and updates for multiple Selectable children.
NavigationNotification
A notification that a change in navigation has taken place.
NavigationToolbar
NavigationToolbar is a layout helper to position 3 widgets or groups of widgets along a horizontal axis that's sensible for an application's navigation bar such as in Material Design and in iOS.
Navigator
A widget that manages a set of child widgets with a stack discipline.
NavigatorObserver
An interface for observing the behavior of a Navigator.
NavigatorPopHandler<T>
Enables the handling of system back gestures.
NavigatorState
The state for a Navigator widget.
NestedScrollView
A scrolling view inside of which can be nested other scrolling views, with their scroll positions being intrinsically linked.
NestedScrollViewState
The State for a NestedScrollView.
NestedScrollViewViewport
The Viewport variant used by NestedScrollView.
NetworkImage
Fetches the given URL from the network, associating it with the given scale.
NeverScrollableScrollPhysics
Scroll physics that does not allow the user to scroll.
NextFocusAction
An Action that moves the focus to the next focusable node in the focus order.
NextFocusIntent
An Intent bound to NextFocusAction, which moves the focus to the next focusable node in the focus traversal order.
NotchedShape
A shape with a notch in its outline.
Notification
A notification that can bubble up the widget tree.
NotificationListener<T extends Notification>
A widget that listens for Notifications bubbling up the tree.
NumericFocusOrder
Can be given to a FocusTraversalOrder widget to assign a numerical order to a widget subtree that is using a OrderedTraversalPolicy to define the order in which widgets should be traversed with the keyboard.
ObjectKey
A key that takes its identity from the object used as its value.
Offset
An immutable 2D floating-point offset.
Offstage
A widget that lays the child out as if it was in the tree, but without painting anything, without making the child available for hit testing, and without taking any room in the parent.
OneFrameImageStreamCompleter
Manages the loading of dart:ui.Image objects for static ImageStreams (those with only one frame).
Opacity
A widget that makes its child partially transparent.
OrderedTraversalPolicy
A FocusTraversalPolicy that orders nodes by an explicit order that resides in the nearest FocusTraversalOrder widget ancestor.
OrientationBuilder
Builds a widget tree that can depend on the parent widget's orientation (distinct from the device orientation).
OutlinedBorder
A ShapeBorder that draws an outline with the width and color specified by side.
OvalBorder
A border that fits an elliptical shape.
OverflowBar
A widget that lays out its children in a row unless they "overflow" the available horizontal space, in which case it lays them out in a column instead.
OverflowBox
A widget that imposes different constraints on its child than it gets from its parent, possibly allowing the child to overflow the parent.
Overlay
A stack of entries that can be managed independently.
OverlayEntry
A place in an Overlay that can contain a widget.
OverlayPortal
A widget that renders its overlay child on an Overlay.
OverlayPortalController
A class to show, hide and bring to top an OverlayPortal's overlay child in the target Overlay.
OverlayRoute<T>
A route that displays widgets in the Navigator's Overlay.
OverlayState
The current state of an Overlay.
OverscrollIndicatorNotification
A notification that either a GlowingOverscrollIndicator or a StretchingOverscrollIndicator will start showing an overscroll indication.
OverscrollNotification
A notification that a Scrollable widget has not changed its scroll position because the change would have caused its scroll position to go outside of its scroll bounds.
Padding
A widget that insets its child by the given padding.
Page<T>
Describes the configuration of a Route.
PageController
A controller for PageView.
PageMetrics
Metrics for a PageView.
PageRoute<T>
A modal route that replaces the entire screen.
PageRouteBuilder<T>
A utility class for defining one-off page routes in terms of callbacks.
PageScrollPhysics
Scroll physics used by a PageView.
PageStorage
Establish a subtree in which widgets can opt into persisting states after being destroyed.
PageStorageBucket
A storage bucket associated with a page in an app.
PageStorageKey<T>
A Key that can be used to persist the widget state in storage after the destruction and will be restored when recreated.
PageView
A scrollable list that works page by page.
Paint
A description of the style to use when drawing on a Canvas.
PaintingContext
A place to paint.
ParametricCurve<T>
An abstract class providing an interface for evaluating a parametric curve.
ParentDataElement<T extends ParentData>
An Element that uses a ParentDataWidget as its configuration.
ParentDataWidget<T extends ParentData>
Base class for widgets that hook ParentData information to children of RenderObjectWidgets.
PasteTextIntent
An Intent to paste text from Clipboard to the field.
Path
A complex, one-dimensional subset of a plane.
PerformanceOverlay
Displays performance statistics.
PhysicalModel
A widget representing a physical layer that clips its children to a shape.
PhysicalShape
A widget representing a physical layer that clips its children to a path.
PinnedHeaderSliver
A sliver that keeps its Widget child at the top of the a CustomScrollView.
Placeholder
A widget that draws a box that represents where other widgets will one day be added.
PlaceholderDimensions
Holds the Size and baseline required to represent the dimensions of a placeholder in text.
PlaceholderSpan
An immutable placeholder that is embedded inline within text.
PlatformMenu
A class for representing menu items that have child submenus.
PlatformMenuBar
A menu bar that uses the platform's native APIs to construct and render a menu described by a PlatformMenu/PlatformMenuItem hierarchy.
PlatformMenuDelegate
An abstract delegate class that can be used to set WidgetsBinding.platformMenuDelegate to provide for managing platform menus.
PlatformMenuItem
A class for PlatformMenuItems that do not have submenus (as a PlatformMenu would), but can be selected.
PlatformMenuItemGroup
A class that groups other menu items into sections delineated by dividers.
PlatformProvidedMenuItem
A class that represents a menu item that is provided by the platform.
PlatformRouteInformationProvider
The route information provider that propagates the platform route information changes.
PlatformSelectableRegionContextMenu
A widget that provides native selection context menu for its child subtree.
PlatformViewCreationParams
The parameters used to create a PlatformViewController.
PlatformViewLink
Links a platform view with the Flutter framework.
PlatformViewSurface
Integrates a platform view with Flutter's compositor, touch, and semantics subsystems.
PointerCancelEvent
The input from the pointer is no longer directed towards this receiver.
PointerDownEvent
The pointer has made contact with the device.
PointerEvent
Base class for touch, stylus, or mouse events.
PointerMoveEvent
The pointer has moved with respect to the device while the pointer is in contact with the device.
PointerUpEvent
The pointer has stopped making contact with the device.
PopEntry<T>
Allows listening to and preventing pops.
PopScope<T>
Manages back navigation gestures.
PopupRoute<T>
A modal route that overlays a widget over the current route.
Positioned
A widget that controls where a child of a Stack is positioned.
PositionedDirectional
A widget that controls where a child of a Stack is positioned without committing to a specific TextDirection.
PositionedTransition
Animated version of Positioned which takes a specific Animation<RelativeRect> to transition the child's position from a start position to an end position over the lifetime of the animation.
PredictiveBackRoute
An interface for a route that supports predictive back gestures.
PreferredSize
A widget with a preferred size.
PreferredSizeWidget
An interface for widgets that can return the size this widget would prefer if it were otherwise unconstrained.
PreviousFocusAction
An Action that moves the focus to the previous focusable node in the focus order.
PreviousFocusIntent
An Intent bound to PreviousFocusAction, which moves the focus to the previous focusable node in the focus traversal order.
PrimaryScrollController
Associates a ScrollController with a subtree.
PrioritizedAction
An Action that iterates through a list of Intents, invoking the first that is enabled.
PrioritizedIntents
An Intent that evaluates a series of specified orderedIntents for execution.
ProxyAnimation
An animation that is a proxy for another animation.
ProxyElement
An Element that uses a ProxyWidget as its configuration.
ProxyWidget
A widget that has a child widget provided to it, instead of building a new widget.
RadialGradient
A 2D radial gradient.
RadioGroup<T>
A group for radios.
RadioGroupRegistry<T>
An abstract interface for registering a group of radios.
Radius
A radius for either circular or elliptical shapes.
RangeMaintainingScrollPhysics
Scroll physics that attempt to keep the scroll position in range when the contents change dimensions suddenly.
RawAutocomplete<T extends Object>
A widget for helping the user make a selection by entering some text and choosing from among a list of options.
RawDialogRoute<T>
A general dialog route which allows for customization of the dialog popup.
RawGestureDetector
A widget that detects gestures described by the given gesture factories.
RawGestureDetectorState
State for a RawGestureDetector.
RawImage
A widget that displays a dart:ui.Image directly.
RawKeyboardListener
A widget that calls a callback whenever the user presses or releases a key on a keyboard.
RawKeyEvent
Defines the interface for raw key events.
RawMagnifier
A common base class for magnifiers.
RawMenuAnchor
A widget that wraps a child and anchors a floating menu.
RawMenuAnchorGroup
Creates a menu anchor that is always visible and is not displayed in an OverlayPortal.
RawMenuOverlayInfo
Anchor and menu information passed to RawMenuAnchor.
RawRadio<T>
A Radio button that provides basic radio functionalities.
RawScrollbar
An extendable base class for building scrollbars that fade in and out.
RawScrollbarState<T extends RawScrollbar>
The state for a RawScrollbar widget, also shared by the Scrollbar and CupertinoScrollbar widgets.
RawView
The lower level workhorse widget for View that bootstraps a render tree for a view.
ReadingOrderTraversalPolicy
Traverses the focus order in "reading order".
Rect
An immutable, 2D, axis-aligned, floating-point rectangle whose coordinates are relative to a given origin.
RectTween
An interpolation between two rectangles.
RedoTextIntent
An Intent that represents a user interaction that attempts to go back to the previous editing state.
RelativePositionedTransition
Animated version of Positioned which transitions the child's position based on the value of rect relative to a bounding box with the specified size.
RelativeRect
An immutable 2D, axis-aligned, floating-point rectangle whose coordinates are given relative to another rectangle's edges, known as the container. Since the dimensions of the rectangle are relative to those of the container, this class has no width and height members. To determine the width or height of the rectangle, convert it to a Rect using toRect() (passing the container's own Rect), and then examine that object.
RelativeRectTween
An interpolation between two relative rects.
RenderBox
A render object in a 2D Cartesian coordinate system.
RenderNestedScrollViewViewport
The RenderViewport variant used by NestedScrollView.
RenderObject
An object in the render tree.
RenderObjectElement
An Element that uses a RenderObjectWidget as its configuration.
RenderObjectToWidgetAdapter<T extends RenderObject>
A bridge from a RenderObject to an Element tree.
RenderObjectToWidgetElement<T extends RenderObject>
The root of an element tree that is hosted by a RenderObject.
RenderObjectWidget
RenderObjectWidgets provide the configuration for RenderObjectElements, which wrap RenderObjects, which provide the actual rendering of the application.
RenderSemanticsGestureHandler
Listens for the specified gestures from the semantics server (e.g. an accessibility tool).
RenderSliverOverlapAbsorber
A sliver that wraps another, forcing its layout extent to be treated as overlap.
RenderSliverOverlapInjector
A sliver that has a sliver geometry based on the values stored in a SliverOverlapAbsorberHandle.
RenderTapRegion
A render object that defines a region that can detect taps inside or outside of itself and any group of regions it belongs to, without participating in the gesture disambiguation system.
RenderTapRegionSurface
A render object that provides notification of a tap inside or outside of a set of registered regions, without participating in the gesture disambiguation system (other than to consume tap down events if TapRegion.consumeOutsideTaps is true).
RenderTreeRootElement
A RenderObjectElement used to manage the root of a render tree.
RenderTwoDimensionalViewport
A base class for viewing render objects that scroll in two dimensions.
ReorderableDelayedDragStartListener
A wrapper widget that will recognize the start of a drag operation by looking for a long press event. Once it is recognized, it will start a drag operation on the wrapped item in the reorderable list.
ReorderableDragStartListener
A wrapper widget that will recognize the start of a drag on the wrapped widget by a PointerDownEvent, and immediately initiate dragging the wrapped item to a new location in a reorderable list.
ReorderableList
A scrolling container that allows the user to interactively reorder the list items.
ReorderableListState
The state for a list that allows the user to interactively reorder the list items.
RepaintBoundary
A widget that creates a separate display list for its child.
ReplaceTextIntent
An Intent that represents a user interaction that attempts to modify the current TextEditingValue in an input field.
RequestFocusAction
An Action that requests the focus on the node it is given in its RequestFocusIntent.
RequestFocusIntent
An intent for use with the RequestFocusAction, which supplies the FocusNode that should be focused.
ResizeImage
Instructs Flutter to decode the image at the specified dimensions instead of at its native size.
ResizeImageKey
Key used internally by ResizeImage.
RestorableBool
A RestorableProperty that knows how to store and restore a bool.
RestorableBoolN
A RestorableProperty that knows how to store and restore a bool that is nullable.
RestorableChangeNotifier<T extends ChangeNotifier>
A base class for creating a RestorableProperty that stores and restores a ChangeNotifier.
RestorableDateTime
A RestorableValue that knows how to save and restore DateTime.
RestorableDateTimeN
A RestorableValue that knows how to save and restore DateTime that is nullable.
RestorableDouble
A RestorableProperty that knows how to store and restore a double.
RestorableDoubleN
A RestorableProperty that knows how to store and restore a double that is nullable.
RestorableEnum<T extends Enum>
A RestorableProperty that knows how to store and restore an Enum type.
RestorableEnumN<T extends Enum>
A RestorableProperty that knows how to store and restore a nullable Enum type.
RestorableInt
A RestorableProperty that knows how to store and restore an int.
RestorableIntN
A RestorableProperty that knows how to store and restore an int that is nullable.
RestorableListenable<T extends Listenable>
A base class for creating a RestorableProperty that stores and restores a Listenable.
RestorableNum<T extends num>
A RestorableProperty that knows how to store and restore a num.
RestorableNumN<T extends num?>
A RestorableProperty that knows how to store and restore a num that is nullable.
RestorableProperty<T>
Manages an object of type T, whose value a State object wants to have restored during state restoration.
RestorableRouteFuture<T>
Gives access to a Route object and its return value that was added to a navigator via one of its "restorable" API methods.
RestorableString
A RestorableProperty that knows how to store and restore a String.
RestorableStringN
A RestorableProperty that knows how to store and restore a String that is nullable.
RestorableTextEditingController
A RestorableProperty that knows how to store and restore a TextEditingController.
RestorableValue<T>
A RestorableProperty that makes the wrapped value accessible to the owning State object via the value getter and setter.
RestorationBucket
A RestorationBucket holds pieces of the restoration data that a part of the application needs to restore its state.
RestorationScope
Creates a new scope for restoration IDs used by descendant widgets to claim RestorationBuckets.
ReverseAnimation
An animation that is the reverse of another animation.
ReverseTween<T extends Object?>
A Tween that evaluates its parent in reverse.
RichText
A paragraph of rich text.
RootBackButtonDispatcher
The default implementation of back button dispatcher for the root router.
RootElement
The root of the element tree.
RootRenderObjectElement
Deprecated. Unused in the framework and will be removed in a future version of Flutter.
RootRestorationScope
Inserts a child bucket of RestorationManager.rootBucket into the widget tree and makes it available to descendants via RestorationScope.of.
RootWidget
A widget for the root of the widget tree.
RotatedBox
A widget that rotates its child by a integral number of quarter turns.
RotationTransition
Animates the rotation of a widget.
RoundedRectangleBorder
A rectangular border with rounded corners.
RoundedSuperellipseBorder
A rectangular border with rounded corners following the shape of an RSuperellipse.
Route<T>
An abstraction for an entry managed by a Navigator.
RouteAware
An interface for objects that are aware of their current Route.
RouteInformation
A piece of routing information.
RouteInformationParser<T>
A delegate that is used by the Router widget to parse a route information into a configuration of type T.
RouteInformationProvider
A route information provider that provides route information for the Router widget
RouteObserver<R extends Route>
A Navigator observer that notifies RouteAwares of changes to the state of their Route.
Router<T>
The dispatcher for opening and closing pages of an application.
RouterConfig<T>
A convenient bundle to configure a Router widget.
RouterDelegate<T>
A delegate that is used by the Router widget to build and configure a navigating widget.
RouteSettings
Data that might be useful in constructing a Route.
RouteTransitionRecord
A Route wrapper interface that can be staged for TransitionDelegate to decide how its underlying Route should transition on or off screen.
Row
A widget that displays its children in a horizontal array.
RRect
An immutable rounded rectangle with the custom radii for all four corners.
RSTransform
A transform consisting of a translation, a rotation, and a uniform scale.
RSuperellipse
An immutable rounded superellipse.
SafeArea
A widget that insets its child with sufficient padding to avoid intrusions by the operating system.
SawTooth
A sawtooth curve that repeats a given number of times over the unit interval.
ScaleEndDetails
Details for GestureScaleEndCallback.
ScaleStartDetails
Details for GestureScaleStartCallback.
ScaleTransition
Animates the scale of a transformed widget.
ScaleUpdateDetails
Details for GestureScaleUpdateCallback.
Scrollable
A widget that manages scrolling in one dimension and informs the Viewport through which the content is viewed.
ScrollableDetails
Describes the aspects of a Scrollable widget to inform inherited widgets like ScrollBehavior for decorating or enumerate the properties of combined Scrollables, such as TwoDimensionalScrollable.
ScrollableState
State object for a Scrollable widget.
ScrollAction
An Action that scrolls the relevant Scrollable by the amount configured in the ScrollIntent given to it.
ScrollActivity
Base class for scrolling activities like dragging and flinging.
ScrollActivityDelegate
A backend for a ScrollActivity.
ScrollAwareImageProvider<T extends Object>
An ImageProvider that makes use of Scrollable.recommendDeferredLoadingForContext to avoid loading images when rapidly scrolling.
ScrollbarPainter
Paints a scrollbar's track and thumb.
ScrollBehavior
Describes how Scrollable widgets should behave.
ScrollConfiguration
Controls how Scrollable widgets behave in a subtree.
ScrollContext
An interface that Scrollable widgets implement in order to use ScrollPosition.
ScrollController
Controls a scrollable widget.
ScrollDragController
Scrolls a scroll view as the user drags their finger across the screen.
ScrollEndNotification
A notification that a Scrollable widget has stopped scrolling.
ScrollHoldController
Interface for holding a Scrollable stationary.
ScrollIncrementDetails
A details object that describes the type of scroll increment being requested of a ScrollIncrementCalculator function, as well as the current metrics for the scrollable.
ScrollIntent
An Intent that represents scrolling the nearest scrollable by an amount appropriate for the type specified.
ScrollMetricsNotification
A notification that a scrollable widget's ScrollMetrics have changed.
ScrollNotification
A Notification related to scrolling.
ScrollNotificationObserver
Notifies its listeners when a descendant scrolls.
ScrollNotificationObserverState
The listener list state for a ScrollNotificationObserver returned by ScrollNotificationObserver.of.
ScrollPhysics
Determines the physics of a Scrollable widget.
ScrollPosition
Determines which portion of the content is visible in a scroll view.
ScrollPositionWithSingleContext
A scroll position that manages scroll activities for a single ScrollContext.
ScrollSpringSimulation
A SpringSimulation where the value of x is guaranteed to have exactly the end value when the simulation isDone.
ScrollStartNotification
A notification that a Scrollable widget has started scrolling.
ScrollToDocumentBoundaryIntent
Scrolls to the beginning or end of the document depending on the forward parameter.
ScrollUpdateNotification
A notification that a Scrollable widget has changed its scroll position.
ScrollView
A widget that combines a Scrollable and a Viewport to create an interactive scrolling pane of content in one dimension.
SelectableRegion
A widget that introduces an area for user selections.
SelectableRegionSelectionStatusScope
Notifies its listeners when the selection under a SelectableRegion or SelectionArea is being changed or finalized.
SelectableRegionState
State for a SelectableRegion.
SelectAction
An action that selects the currently focused control.
SelectAllTextIntent
An Intent to select everything in the field.
SelectIntent
An Intent that selects the currently focused control.
SelectionContainer
A container that handles SelectionEvents for the Selectables in the subtree.
SelectionContainerDelegate
A delegate to handle SelectionEvents for a SelectionContainer.
SelectionDetails
A read-only interface for accessing the details of a selection under a SelectionListener.
SelectionListener
A SelectionContainer that allows the user to access the SelectionDetails and listen to selection changes for the child subtree it wraps under a SelectionArea or SelectableRegion.
SelectionListenerNotifier
Notifies listeners when the selection under a SelectionListener has been changed.
SelectionOverlay
An object that manages a pair of selection handles and a toolbar.
SelectionRegistrarScope
An inherited widget to host a SelectionRegistrar for the subtree.
Semantics
A widget that annotates the widget tree with a description of the meaning of the widgets.
SemanticsDebugger
A widget that visualizes the semantics for the child.
SemanticsGestureDelegate
A base class that describes what semantics notations a RawGestureDetector should add to the render object RenderSemanticsGestureHandler.
SensitiveContent
Widget to set the ContentSensitivity of content in the widget tree.
SensitiveContentHost
Host of the current content sensitivity for the widget tree that contains some number SensitiveContent widgets.
Shader
Base class for objects such as Gradient and ImageShader which correspond to shaders as used by Paint.shader.
ShaderMask
A widget that applies a mask generated by a Shader to its child.
ShaderWarmUp
Interface for drawing an image to warm up Skia shader compilations.
Shadow
A single shadow.
ShapeBorder
Base class for shape outlines.
ShapeBorderClipper
A CustomClipper that clips to the outer path of a ShapeBorder.
ShapeDecoration
An immutable description of how to paint an arbitrary shape.
SharedAppData
Enables sharing key/value data with its child and all of the child's descendants.
ShortcutActivator
An interface to define the keyboard key combination to trigger a shortcut.
ShortcutManager
A manager of keyboard shortcut bindings used by Shortcuts to handle key events.
ShortcutMapProperty
A DiagnosticsProperty which handles formatting a Map<LogicalKeySet, Intent> (the same type as the Shortcuts.shortcuts property) so that its diagnostic output is human-readable.
ShortcutRegistrar
A widget that holds a ShortcutRegistry which allows descendants to add, remove, or replace shortcuts.
ShortcutRegistry
A class used by ShortcutRegistrar that allows adding or removing shortcut bindings by descendants of the ShortcutRegistrar.
ShortcutRegistryEntry
A entry returned by ShortcutRegistry.addAll that allows the caller to identify the shortcuts they registered with the ShortcutRegistry through the ShortcutRegistrar.
Shortcuts
A widget that creates key bindings to specific actions for its descendants.
ShortcutSerialization
A class used by MenuSerializableShortcut to describe the shortcut for serialization to send to the platform for rendering a PlatformMenuBar.
ShrinkWrappingViewport
A widget that is bigger on the inside and shrink wraps its children in the main axis.
Simulation
The base class for all simulations.
SingleActivator
A shortcut key combination of a single key and modifiers.
SingleChildLayoutDelegate
A delegate for computing the layout of a render object with a single child.
SingleChildRenderObjectElement
An Element that uses a SingleChildRenderObjectWidget as its configuration.
SingleChildRenderObjectWidget
A superclass for RenderObjectWidgets that configure RenderObject subclasses that have a single child slot.
SingleChildScrollView
A box in which a single widget can be scrolled.
Size
Holds a 2D floating-point size.
SizeChangedLayoutNotification
Indicates that the size of one of the descendants of the object receiving this notification has changed, and that therefore any assumptions about that layout are no longer valid.
SizeChangedLayoutNotifier
A widget that automatically dispatches a SizeChangedLayoutNotification when the layout dimensions of its child change.
SizedBox
A box with a specified size.
SizedOverflowBox
A widget that is a specific size but passes its original constraints through to its child, which may then overflow.
SizeTransition
Animates its own size and clips and aligns its child.
SizeTween
An interpolation between two sizes.
SlideTransition
Animates the position of a widget relative to its normal position.
SliverAnimatedGrid
A SliverGrid that animates items when they are inserted or removed.
SliverAnimatedGridState
The state for a SliverAnimatedGrid that animates items when they are inserted or removed.
SliverAnimatedList
A SliverList that animates items when they are inserted or removed.
SliverAnimatedListState
The state for a SliverAnimatedList that animates items when they are inserted or removed.
SliverAnimatedOpacity
Animated version of SliverOpacity which automatically transitions the sliver child's opacity over a given duration whenever the given opacity changes.
SliverChildBuilderDelegate
A delegate that supplies children for slivers using a builder callback.
SliverChildDelegate
A delegate that supplies children for slivers.
SliverChildListDelegate
A delegate that supplies children for slivers using an explicit list.
SliverConstrainedCrossAxis
A sliver that constrains the cross axis extent of its sliver child.
SliverCrossAxisExpanded
Set a flex factor for allocating space in the cross axis direction.
SliverCrossAxisGroup
A sliver that places multiple sliver children in a linear array along the cross axis.
SliverEnsureSemantics
A sliver that ensures its sliver child is included in the semantics tree.
SliverFadeTransition
Animates the opacity of a sliver widget.
SliverFillRemaining
A sliver that contains a single box child that fills the remaining space in the viewport.
SliverFillViewport
A sliver that contains multiple box children that each fills the viewport.
SliverFixedExtentList
A sliver that places multiple box children with the same main axis extent in a linear array.
SliverFloatingHeader
A sliver that shows its child when the user scrolls forward and hides it when the user scrolls backwards.
SliverGrid
A sliver that places multiple box children in a two dimensional arrangement.
SliverGridDelegate
Controls the layout of tiles in a grid.
SliverGridDelegateWithFixedCrossAxisCount
Creates grid layouts with a fixed number of tiles in the cross axis.
SliverGridDelegateWithMaxCrossAxisExtent
Creates grid layouts with tiles that each have a maximum cross-axis extent.
SliverIgnorePointer
A sliver widget that is invisible during hit testing.
SliverLayoutBuilder
Builds a sliver widget tree that can depend on its own SliverConstraints.
SliverList
A sliver that places multiple box children in a linear array along the main axis.
SliverMainAxisGroup
A sliver that places multiple sliver children in a linear array along the main axis, one after another.
SliverMultiBoxAdaptorElement
An element that lazily builds children for a SliverMultiBoxAdaptorWidget.
SliverMultiBoxAdaptorWidget
A base class for slivers that have multiple box children.
SliverOffstage
A sliver that lays its sliver child out as if it was in the tree, but without painting anything, without making the sliver child available for hit testing, and without taking any room in the parent.
SliverOpacity
A sliver widget that makes its sliver child partially transparent.
SliverOverlapAbsorber
A sliver that wraps another, forcing its layout extent to be treated as overlap.
SliverOverlapAbsorberHandle
Handle to provide to a SliverOverlapAbsorber, a SliverOverlapInjector, and an NestedScrollViewViewport, to shift overlap in a NestedScrollView.
SliverOverlapInjector
A sliver that has a sliver geometry based on the values stored in a SliverOverlapAbsorberHandle.
SliverPadding
A sliver that applies padding on each side of another sliver.
SliverPersistentHeader
A sliver whose size varies when the sliver is scrolled to the edge of the viewport opposite the sliver's GrowthDirection.
SliverPersistentHeaderDelegate
Delegate for configuring a SliverPersistentHeader.
SliverPrototypeExtentList
A sliver that places its box children in a linear array and constrains them to have the same extent as a prototype item along the main axis.
SliverReorderableList
A sliver list that allows the user to interactively reorder the list items.
SliverReorderableListState
The state for a sliver list that allows the user to interactively reorder the list items.
SliverResizingHeader
A sliver that is pinned to the start of its CustomScrollView and reacts to scrolling by resizing between the intrinsic sizes of its min and max extent prototypes.
SliverSafeArea
A sliver that insets another sliver by sufficient padding to avoid intrusions by the operating system.
SliverSemantics
A sliver that annotates its subtree with a description of the meaning of the slivers.
SliverToBoxAdapter
A sliver that contains a single box widget.
SliverVariedExtentList
A sliver that places its box children in a linear array and constrains them to have the corresponding extent returned by itemExtentBuilder.
SliverVisibility
Whether to show or hide a sliver child.
SliverWithKeepAliveWidget
A base class for slivers that have KeepAlive children.
SlottedMultiChildRenderObjectWidget<SlotType, ChildType extends RenderObject>
A superclass for RenderObjectWidgets that configure RenderObject subclasses that organize their children in different slots.
SlottedRenderObjectElement<SlotType, ChildType extends RenderObject>
Element used by the SlottedMultiChildRenderObjectWidget.
SnapshotController
A controller for the SnapshotWidget that controls when the child image is displayed and when to regenerated the child image.
SnapshotPainter
A painter used to paint either a snapshot or the child widgets that would be a snapshot.
SnapshotWidget
A widget that can replace its child with a snapshotted version of the child.
Spacer
Spacer creates an adjustable, empty spacer that can be used to tune the spacing between widgets in a Flex container, like Row or Column.
SpellCheckConfiguration
Controls how spell check is performed for text input.
Split
A curve that progresses according to beginCurve until split, then according to endCurve.
SpringDescription
Structure that describes a spring's constants.
Stack
A widget that positions its children relative to the edges of its box.
StadiumBorder
A border that fits a stadium-shaped border (a box with semicircles on the ends) within the rectangle of the widget it is applied to.
StarBorder
A border that fits a star or polygon-shaped border within the rectangle of the widget it is applied to.
State<T extends StatefulWidget>
The logic and internal state for a StatefulWidget.
StatefulBuilder
A platonic widget that both has state and calls a closure to obtain its child widget.
StatefulElement
An Element that uses a StatefulWidget as its configuration.
StatefulWidget
A widget that has mutable state.
StatelessElement
An Element that uses a StatelessWidget as its configuration.
StatelessWidget
A widget that does not require mutable state.
StaticSelectionContainerDelegate
A delegate that manages updating multiple Selectable children where the Selectables do not change or move around frequently.
StatusTransitionWidget
A widget that rebuilds when the given animation changes status.
StepTween
An interpolation between two integers that floors.
StreamBuilder<T>
Widget that builds itself based on the latest snapshot of interaction with a Stream.
StreamBuilderBase<T, S>
Base class for widgets that build themselves based on interaction with a specified Stream.
StretchingOverscrollIndicator
A Material Design visual indication that a scroll view has overscrolled.
StrutStyle
Defines the strut, which sets the minimum height a line can be relative to the baseline.
SweepGradient
A 2D sweep gradient.
SystemContextMenu
Displays the system context menu on top of the Flutter view.
SystemMouseCursors
A collection of system MouseCursors.
SystemTextScaler
A TextScaler that reflects the user's font scale preferences from the platform's accessibility settings.
Table
A widget that uses the table layout algorithm for its children.
TableBorder
Border specification for Table widgets.
TableCell
A widget that controls how a child of a Table is aligned.
TableColumnWidth
Base class to describe how wide a column in a RenderTable should be.
TableRow
A horizontal group of cells in a Table.
TapDownDetails
Details for GestureTapDownCallback, such as position.
TapRegion
A widget that defines a region that can detect taps inside or outside of itself and any group of regions it belongs to, without participating in the gesture disambiguation system (other than to consume tap down events if consumeOutsideTaps is true).
TapRegionRegistry
An interface for registering and unregistering a RenderTapRegion (typically created with a TapRegion widget) with a RenderTapRegionSurface (typically created with a TapRegionSurface widget).
TapRegionSurface
A widget that provides notification of a tap inside or outside of a set of registered regions, without participating in the gesture disambiguation system.
TapUpDetails
Details for GestureTapUpCallback, such as position.
Text
A run of text with a single style.
TextAlignVertical
The vertical alignment of text within an input box.
TextBox
A rectangle enclosing a run of text.
TextDecoration
A linear decoration to draw near the text.
TextEditingController
A controller for an editable text field.
TextEditingValue
The current text, selection, and composing state for editing a run of text.
TextFieldTapRegion
A TapRegion that adds its children to the tap region group for widgets based on the EditableText text editing widget, such as TextField and CupertinoTextField.
TextHeightBehavior
Defines how to apply TextStyle.height over and under text.
TextInputType
The type of information for which to optimize the text input control.
TextMagnifierConfiguration
A configuration object for a magnifier (e.g. in a text field).
TextPainter
An object that paints a TextSpan tree into a Canvas.
TextPosition
A position in a string of text.
TextRange
A range of characters in a string of text.
TextScaler
A class that describes how textual contents should be scaled for better readability.
TextSelection
A range of text that represents a selection.
TextSelectionControls
An interface for building the selection UI, to be provided by the implementer of the toolbar widget.
TextSelectionGestureDetector
A gesture detector to respond to non-exclusive event chains for a text field.
TextSelectionGestureDetectorBuilder
Builds a TextSelectionGestureDetector to wrap an EditableText.
TextSelectionGestureDetectorBuilderDelegate
Delegate interface for the TextSelectionGestureDetectorBuilder.
TextSelectionOverlay
An object that manages a pair of text selection handles for a RenderEditable.
TextSelectionPoint
Represents the coordinates of the point in a selection, and the text direction at that point, relative to top left of the RenderEditable that holds the selection.
TextSelectionToolbarAnchors
The position information for a text selection toolbar.
TextSelectionToolbarLayoutDelegate
A SingleChildLayoutDelegate for use with CustomSingleChildLayout that positions its child above anchorAbove if it fits, or otherwise below anchorBelow.
TextSpan
An immutable span of text.
TextStyle
An immutable style describing how to format and paint text.
TextStyleTween
An interpolation between two TextStyles.
Texture
A rectangle upon which a backend texture is mapped.
ThreePointCubic
A cubic polynomial composed of two curves that share a common center point.
Threshold
A curve that is 0.0 until it hits the threshold, then it jumps to 1.0.
TickerFuture
An object representing an ongoing Ticker sequence.
TickerMode
Enables or disables tickers (and thus animation controllers) in the widget subtree.
TickerProvider
An interface implemented by classes that can vend Ticker objects.
Title
A widget that describes this app in the operating system.
ToggleablePainter
A base class for a CustomPainter that may be passed to ToggleableStateMixin.buildToggleable to draw the visual representation of a Toggleable.
Tolerance
Structure that specifies maximum allowable magnitudes for distances, durations, and velocity differences to be considered equal.
ToolbarItemsParentData
ParentData that determines whether or not to paint the corresponding child.
ToolbarOptions
Toolbar configuration for EditableText.
TrackingScrollController
A ScrollController whose initialScrollOffset tracks its most recently updated ScrollPosition.
TrainHoppingAnimation
This animation starts by proxying one animation, but when the value of that animation crosses the value of the second (either because the second is going in the opposite direction, or because the one overtakes the other), the animation hops over to proxying the second animation.
Transform
A widget that applies a transformation before painting its child.
TransformationController
A thin wrapper on ValueNotifier whose value is a Matrix4 representing a transformation.
TransformProperty
Property which handles Matrix4 that represent transforms.
TransitionDelegate<T>
The delegate that decides how pages added and removed from Navigator.pages transition in or out of the screen.
TransitionRoute<T>
A route with entrance and exit transitions.
TransposeCharactersIntent
An Intent that represents a user interaction that attempts to swap the characters immediately around the cursor.
TreeSliver<T>
A widget that displays TreeSliverNodes that expand and collapse in a vertically and horizontally scrolling Viewport.
TreeSliverController
Enables control over the TreeSliverNodes of a TreeSliver.
TreeSliverNode<T>
A data structure for configuring children of a TreeSliver.
Tween<T extends Object?>
A linear interpolation between a beginning and ending value.
TweenAnimationBuilder<T extends Object?>
Widget builder that animates a property of a Widget to a target value whenever the target value changes.
TweenSequence<T>
Enables creating an Animation whose value is defined by a sequence of Tweens.
TweenSequenceItem<T>
A simple holder for one element of a TweenSequence.
TwoDimensionalChildBuilderDelegate
A delegate that supplies children for a TwoDimensionalScrollView using a builder callback.
TwoDimensionalChildDelegate
A delegate that supplies children for scrolling in two dimensions.
TwoDimensionalChildListDelegate
A delegate that supplies children for a TwoDimensionalViewport using an explicit two dimensional array.
TwoDimensionalChildManager
A delegate used by RenderTwoDimensionalViewport to manage its children.
TwoDimensionalScrollable
A widget that manages scrolling in both the vertical and horizontal dimensions and informs the TwoDimensionalViewport through which the content is viewed.
TwoDimensionalScrollableState
State object for a TwoDimensionalScrollable widget.
TwoDimensionalScrollView
A widget that combines a TwoDimensionalScrollable and a TwoDimensionalViewport to create an interactive scrolling pane of content in both vertical and horizontal dimensions.
TwoDimensionalViewport
A widget through which a portion of larger content can be viewed, typically in combination with a TwoDimensionalScrollable.
TwoDimensionalViewportParentData
Parent data structure used by RenderTwoDimensionalViewport.
UiKitView
Embeds an iOS view in the Widget hierarchy.
UnconstrainedBox
A widget that imposes no constraints on its child, allowing it to render at its "natural" size.
UndoHistory<T>
Provides undo/redo capabilities for a ValueNotifier.
UndoHistoryController
A controller for the undo history, for example for an editable text field.
UndoHistoryState<T>
State for a UndoHistory.
UndoHistoryValue
Represents whether the current undo stack can undo or redo.
UndoTextIntent
An Intent that represents a user interaction that attempts to go back to the previous editing state.
UniqueKey
A key that is only equal to itself.
UniqueWidget<T extends State<StatefulWidget>>
Base class for stateful widgets that have exactly one inflated instance in the tree.
UnmanagedRestorationScope
Inserts a provided RestorationBucket into the widget tree and makes it available to descendants via RestorationScope.of.
UpdateSelectionIntent
An Intent that represents a user interaction that attempts to change the selection in an input field.
UserScrollNotification
A notification that the user has changed the ScrollDirection in which they are scrolling, or have stopped scrolling.
ValueKey<T>
A key that uses a value of a particular type to identify itself.
ValueListenableBuilder<T>
A widget whose content stays synced with a ValueListenable.
ValueNotifier<T>
A ChangeNotifier that holds a single value.
Velocity
A velocity in two dimensions.
View
Bootstraps a render tree that is rendered into the provided FlutterView.
ViewAnchor
Decorates a child widget with a side View.
ViewCollection
A collection of sibling Views.
Viewport
A widget through which a portion of larger content can be viewed, typically in combination with a Scrollable.
Visibility
Whether to show or hide a child.
VoidCallbackAction
An Action that invokes the VoidCallback given to it in the VoidCallbackIntent passed to it when invoked.
VoidCallbackIntent
An Intent that keeps a VoidCallback to be invoked by a VoidCallbackAction when it receives this intent.
WeakMap<K, V>
Does not hold keys from garbage collection.
Widget
Describes the configuration for an Element.
WidgetInspector
A widget that enables inspecting the child widget's structure.
WidgetOrderTraversalPolicy
A FocusTraversalPolicy that traverses the focus order in widget hierarchy order.
WidgetsApp
A convenience widget that wraps a number of widgets that are commonly required for an application.
WidgetsBindingObserver
Interface for classes that register with the Widgets layer binding.
WidgetsFlutterBinding
A concrete binding for applications based on the Widgets framework.
WidgetsLocalizations
Interface for localized resource values for the lowest levels of the Flutter framework.
WidgetSpan
An immutable widget that is embedded inline within text.
WidgetStateBorderSide
Defines a BorderSide whose value depends on a set of WidgetStates which represent the interactive state of a component.
WidgetStateColor
Defines a Color that is also a WidgetStateProperty.
WidgetStateMapper<T>
Uses a WidgetStateMap to resolve to a single value of type T based on the current set of Widget states.
WidgetStateMouseCursor
Defines a MouseCursor whose value depends on a set of WidgetStates which represent the interactive state of a component.
WidgetStateOutlinedBorder
Defines an OutlinedBorder whose value depends on a set of WidgetStates which represent the interactive state of a component.
WidgetStateProperty<T>
Interface for classes that resolve to a value of type T based on a widget's interactive "state", which is defined as a set of WidgetStates.
WidgetStatePropertyAll<T>
Convenience class for creating a WidgetStateProperty that resolves to the given value for all states.
WidgetStatesConstraint
This class allows WidgetState enum values to be combined using WidgetStateOperators.
WidgetStatesController
Manages a set of WidgetStates and notifies listeners of changes.
WidgetStateTextStyle
Defines a TextStyle that is also a WidgetStateProperty.
WidgetToRenderBoxAdapter
An adapter for placing a specific RenderBox in the widget tree.
WillPopScope
Registers a callback to veto attempts by the user to dismiss the enclosing ModalRoute.
WordBoundary
A TextBoundary subclass for locating word breaks.
Wrap
A widget that displays its children in multiple horizontal or vertical runs.
Enums
AndroidOverscrollIndicator
Types of overscroll indicators supported by TargetPlatform.android.
AnimationBehavior
Configures how an AnimationController behaves when animations are disabled.
AnimationStatus
The status of an animation.
AppLifecycleState
States that an application can be in once it is running.
AutofillContextAction
Predefined autofill context clean up actions.
AutovalidateMode
Used to configure the auto validation of FormField and Form widgets.
Axis
The two cardinal directions in two dimensions.
AxisDirection
A direction along either the horizontal or vertical Axis in which the origin, or zero position, is determined.
BannerLocation
Where to show a Banner.
BlendMode
Algorithms to use when painting on the canvas.
BlurStyle
Styles to use for blurs in MaskFilter objects.
BorderStyle
The style of line to draw for a BorderSide in a Border.
BoxFit
How a box should be inscribed into another box.
BoxShape
The shape to use when rendering a Border or BoxDecoration.
Brightness
Describes the contrast of a theme or color palette.
ChangeReportingBehavior
The behavior of reporting the selected item index in a ListWheelScrollView.
Clip
Different ways to clip content.
ClipboardStatus
An enumeration of the status of the content on the user's clipboard.
ConnectionState
The state of connection to an asynchronous computation.
ContextMenuButtonType
The buttons that can appear in a context menu by default.
CrossAxisAlignment
How the children should be placed along the cross axis in a flex layout.
CrossFadeState
Specifies which of two children to show. See AnimatedCrossFade.
DecorationPosition
Where to paint a box decoration.
DiagnosticLevel
The various priority levels used to filter which diagnostics are shown and omitted.
DiagonalDragBehavior
Specifies how to configure the DragGestureRecognizers of a TwoDimensionalScrollable.
DismissDirection
The direction in which a Dismissible can be dismissed.
FilterQuality
Quality levels for image sampling in ImageFilter and Shader objects that sample images and for Canvas operations that render images.
FlexFit
How the child is inscribed into the available space.
FloatingHeaderSnapMode
Specifies how a partially visible SliverFloatingHeader animates into a view when a user scroll gesture ends.
FlutterLogoStyle
Possible ways to draw Flutter's logo.
FocusHighlightMode
An enum to describe which kind of focus highlight behavior to use when displaying focus information.
FocusHighlightStrategy
An enum to describe how the current value of FocusManager.highlightMode is determined. The strategy is set on FocusManager.highlightStrategy.
FontStyle
Whether to use the italic type variation of glyphs in the font.
GrowthDirection
The direction in which a sliver's contents are ordered, relative to the scroll offset axis.
HeroFlightDirection
Direction of the hero's flight based on the navigation operation.
HitTestBehavior
How to behave during hit tests.
ImageRepeat
How to paint any portions of a box not covered by an image.
InspectorButtonVariant
Defines the visual and behavioral variants for an InspectorButton.
KeyEventResult
An enum that describes how to handle a key event handled by a FocusOnKeyCallback or FocusOnKeyEventCallback.
LiveTextInputStatus
An enumeration that indicates whether the current device is available for Live Text input.
LockState
Determines how the state of a lock key is used to accept a shortcut.
MainAxisAlignment
How the children should be placed along the main axis in a flex layout.
MainAxisSize
How much space should be occupied in the main axis.
NavigationMode
Describes the navigation mode to be set by a MediaQuery widget.
OptionsViewOpenDirection
A direction in which to open the options-view overlay.
Orientation
Whether in portrait or landscape.
OverflowBarAlignment
Defines the horizontal alignment of OverflowBar children when they're laid out in an overflow column.
PaintingStyle
Strategies for painting shapes and paths on a canvas.
PanAxis
This enum is used to specify the behavior of the InteractiveViewer when the user drags the viewport.
PathFillType
Determines the winding rule that decides how the interior of a Path is calculated.
PathOperation
Strategies for combining paths.
PlaceholderAlignment
Where to vertically align the placeholder relative to the surrounding text.
PlatformProvidedMenuItemType
The list of possible platform provided, prebuilt menus for use in a PlatformMenuBar.
RenderComparison
The description of the difference between two objects, in the context of how it will affect the rendering.
ResizeImagePolicy
Configures the behavior for ResizeImage.
RouteInformationReportingType
The Router's intention when it reports a new RouteInformation to the RouteInformationProvider.
RoutePopDisposition
Indicates whether the current route should be popped.
ScrollbarOrientation
An orientation along either the horizontal or vertical Axis.
ScrollDecelerationRate
The rate at which scroll momentum will be decelerated.
ScrollIncrementType
Describes the type of scroll increment that will be performed by a ScrollAction on a Scrollable.
ScrollPositionAlignmentPolicy
The policy to use when applying the alignment parameter of ScrollPosition.ensureVisible.
ScrollViewKeyboardDismissBehavior
A representation of how a ScrollView should dismiss the on-screen keyboard.
SelectableRegionSelectionStatus
The status of the selection under a SelectableRegion.
SelectionChangedCause
Indicates what triggered the change in selected text (including changes to the cursor location).
SliverPaintOrder
Specifies an order in which to paint the slivers of a Viewport.
SmartDashesType
Indicates how to handle the intelligent replacement of dashes in text input.
SmartQuotesType
Indicates how to handle the intelligent replacement of quotes in text input.
SnapshotMode
Controls how the SnapshotWidget paints its child.
StackFit
How to size the non-positioned children of a Stack.
StandardComponentType
An enum identifying standard UI components.
StrokeCap
Styles to use for line endings.
StrokeJoin
Styles to use for line segment joins.
TableCellVerticalAlignment
Vertical alignment options for cells in RenderTable objects.
TargetPlatform
The platform that user interaction should adapt to target.
TextAffinity
A way to disambiguate a TextPosition when its offset could match two different locations in the rendered string.
TextAlign
Whether and how to align text horizontally.
TextBaseline
A horizontal line used for aligning text.
TextDecorationStyle
The style in which to draw a text decoration
TextDirection
A direction in which text flows.
TextLeadingDistribution
How the "leading" is distributed over and under the text.
TextOverflow
How overflowing text should be handled.
TextSelectionHandleType
The type of selection handle to be displayed.
TextWidthBasis
The different ways of measuring the width of one or more lines of text.
TileMode
Defines what happens at the edge of a gradient or the sampling of a source image in an ImageFilter.
TraversalDirection
A direction along either the horizontal or vertical axes.
TraversalEdgeBehavior
Controls the focus transfer at the edges of a FocusScopeNode. For movement transfers (previous or next), the edge represents the first or last items. For directional transfers, the edge represents the outermost items of the FocusScopeNode, For example: for moving downwards, the edge node is the one with the largest bottom coordinate; for moving leftwards, the edge node is the one with the smallest x coordinate.
UnfocusDisposition
Describe what should happen after FocusNode.unfocus is called.
VertexMode
Defines how a list of points is interpreted when drawing a set of triangles.
VerticalDirection
A direction in which boxes flow vertically.
WebHtmlElementStrategy
The strategy for Image.network and NetworkImage to decide whether to display images in HTML elements contained in a platform view instead of fetching bytes.
WidgetInspectorServiceExtensions
Service extension constants for the Widget Inspector.
WidgetsServiceExtensions
Service extension constants for the widgets library.
WidgetState
Interactive states that some of the widgets can take on when receiving input from the user.
WrapAlignment
How Wrap should align objects.
WrapCrossAlignment
Who Wrap should align children within a run in the cross axis.
Mixins
AnimationEagerListenerMixin
A mixin that replaces the didRegisterListener/didUnregisterListener contract with a dispose contract.
AnimationLazyListenerMixin
A mixin that helps listen to another object only when this object has registered listeners.
AnimationLocalListenersMixin
A mixin that implements the addListener/removeListener protocol and notifies all the registered listeners when notifyListeners is called.
AnimationLocalStatusListenersMixin
A mixin that implements the addStatusListener/removeStatusListener protocol and notifies all the registered listeners when notifyStatusListeners is called.
AnimationWithParentMixin<T>
Implements most of the Animation interface by deferring its behavior to a given parent Animation.
AutomaticKeepAliveClientMixin<T extends StatefulWidget>
A mixin with convenience methods for clients of AutomaticKeepAlive. It is used with State subclasses to manage keep-alive behavior in lazily built lists.
DirectionalFocusTraversalPolicyMixin
A mixin class that provides an implementation for finding a node in a particular direction.
LocalHistoryRoute<T>
A mixin used by routes to handle back navigations internally by popping a list.
MenuSerializableShortcut
A mixin allowing a ShortcutActivator to provide data for serialization of the shortcut when sending to the platform.
NotifiableElementMixin
Mixin this class to allow receiving Notification objects dispatched by child elements.
PaintingBinding
Binding for the painting library.
PopNavigatorRouterDelegateMixin<T>
A mixin that wires RouterDelegate.popRoute to the Navigator it builds.
RadioClient<T>
A client for a RadioGroupRegistry.
RenderAbstractLayoutBuilderMixin<LayoutInfoType, ChildType extends RenderObject>
Generic mixin for RenderObjects created by an AbstractLayoutBuilder with the the same LayoutInfoType.
RestorationMixin<S extends StatefulWidget>
Manages the restoration data for a State object of a StatefulWidget.
RootElementMixin
Mixin for the element at the root of the tree.
ScrollMetrics
A description of a Scrollable's contents, useful for modeling the state of its viewport.
SingleTickerProviderStateMixin<T extends StatefulWidget>
Provides a single Ticker that is configured to only tick while the current tree is enabled, as defined by TickerMode.
SlottedContainerRenderObjectMixin<SlotType, ChildType extends RenderObject>
Mixin for a RenderObject configured by a SlottedMultiChildRenderObjectWidget.
SlottedMultiChildRenderObjectWidgetMixin<SlotType, ChildType extends RenderObject>
A mixin version of SlottedMultiChildRenderObjectWidget.
TextSelectionDelegate
A mixin for manipulating the selection, provided for toolbar or shortcut keys.
TextSelectionHandleControls
TextSelectionControls that specifically do not manage the toolbar in order to leave that to EditableText.contextMenuBuilder.
TickerProviderStateMixin<T extends StatefulWidget>
Provides Ticker objects that are configured to only tick while the current tree is enabled, as defined by TickerMode.
ToggleableStateMixin<S extends StatefulWidget>
A mixin for StatefulWidgets that implement toggleable controls with toggle animations (e.g. Switches, CupertinoSwitches, Checkboxes, CupertinoCheckboxes, Radios, and CupertinoRadios).
TreeSliverStateMixin<T>
A mixin for classes implementing a tree structure as expected by a TreeSliverController.
ViewportElementMixin
A mixin that allows Elements containing Viewport like widgets to correctly modify the notification depth of a ViewportNotificationMixin.
ViewportNotificationMixin
Mixin for Notifications that track how many RenderAbstractViewport they have bubbled through.
WidgetInspectorService
Service used by GUI tools to interact with the WidgetInspector.
WidgetsBinding
The glue between the widgets layer and the Flutter engine.
Extension Types
OverlayChildLayoutInfo
The additional layout information available to the OverlayPortal.overlayChildLayoutBuilder callback.
Extensions
StringCharacters on String
WidgetStateOperators on WidgetStatesConstraint
These operators can be used inside a WidgetStateMap to combine states and find a match.
Constants
factory → const _Factory
Used to annotate an instance or static method m. Indicates that m must either be abstract or must return a newly allocated object or null. In addition, every method that either implements or overrides m is implicitly annotated with this same annotation.
immutable → const Immutable
Used to annotate a class C. Indicates that C and all subtypes of C must be immutable.
kAlwaysCompleteAnimation → const Animation<double>
An animation that is always complete.
kAlwaysDismissedAnimation → const Animation<double>
An animation that is always dismissed.
kDefaultContentInsertionMimeTypes → const List<String>
The default mime types to be used when allowedMimeTypes is not provided.
kDefaultFontSize → const double
The default font size if none is specified.
kDefaultRouteDirectionalTraversalEdgeBehavior → const TraversalEdgeBehavior
The default value of Navigator.routeDirectionalTraversalEdgeBehavior.
kDefaultRouteTraversalEdgeBehavior → const TraversalEdgeBehavior
The default value of Navigator.routeTraversalEdgeBehavior.
kTextHeightNone → const double
A TextStyle.height value that indicates the text span should take the height defined by the font, which may not be exactly the height of TextStyle.fontSize.
mustCallSuper → const _MustCallSuper
Used to annotate an instance member (method, getter, setter, operator, or field) m. Indicates that every invocation of a member that overrides m must also invoke m. In addition, every method that overrides m is implicitly annotated with this same annotation.
optionalTypeArgs → const _OptionalTypeArgs
Used to annotate a class, mixin, extension, function, method, or typedef declaration C. Indicates that any type arguments declared on C are to be treated as optional.
protected → const _Protected
Used to annotate an instance member in a class or mixin which is meant to be visible only within the declaring library, and to other instance members of the class or mixin, and their subtypes.
required → const Required
Used to annotate a named parameter p in a method or function f. Indicates that every invocation of f must include an argument corresponding to p, despite the fact that p would otherwise be an optional parameter.
staticIconProvider → const Object
Annotation for classes that only provide static const IconData instances.
visibleForTesting → const _VisibleForTesting
Used to annotate a declaration that was made public, so that it is more visible than otherwise necessary, to make code testable.
widgetFactory → const _WidgetFactory
Annotation which marks a function as a widget factory for the purpose of widget creation tracking.
Properties
debugCaptureShaderWarmUpImage ↔ ShaderWarmUpImageCallback
Called by ShaderWarmUp.execute immediately after it creates an Image.
getter/setter pair
debugCaptureShaderWarmUpPicture ↔ ShaderWarmUpPictureCallback
Called by ShaderWarmUp.execute immediately after it creates a Picture.
getter/setter pair
debugDisableShadows ↔ bool
Whether to replace all shadows with solid color blocks.
getter/setter pair
debugEnhanceBuildTimelineArguments ↔ bool
Adds debugging information to Timeline events related to Widget builds.
getter/setter pair
debugFocusChanges ↔ bool
Setting to true will cause extensive logging to occur when focus changes occur.
getter/setter pair
debugHighlightDeprecatedWidgets ↔ bool
Show banners for deprecated widgets.
getter/setter pair
debugImageOverheadAllowance ↔ int
The number of bytes an image must use before it triggers inversion when debugInvertOversizedImages is true.
getter/setter pair
debugInvertOversizedImages ↔ bool
If true, the framework will color invert and horizontally flip images that have been decoded to a size taking at least debugImageOverheadAllowance bytes more than necessary.
getter/setter pair
debugNetworkImageHttpClientProvider ↔ HttpClientProvider?
Provider from which NetworkImage will get its HttpClient in debug builds.
getter/setter pair
debugOnPaintImage ↔ PaintImageCallback?
If not null, called when the framework is about to paint an Image to a Canvas with an ImageSizeInfo that contains the decoded size of the image as well as its output size.
getter/setter pair
debugOnRebuildDirtyWidget ↔ RebuildDirtyWidgetCallback?
Callback invoked for every dirty widget built each frame.
getter/setter pair
debugPrint ↔ DebugPrintCallback
Prints a message to the console, which you can access using the "flutter" tool's "logs" command ("flutter logs").
getter/setter pair
debugPrintBuildScope ↔ bool
Log all calls to BuildOwner.buildScope.
getter/setter pair
debugPrintGlobalKeyedWidgetLifecycle ↔ bool
Log when widgets with global keys are deactivated and log when they are reactivated (retaken).
getter/setter pair
debugPrintRebuildDirtyWidgets ↔ bool
Log the dirty widgets that are built each frame.
getter/setter pair
debugPrintScheduleBuildForStacks ↔ bool
Log the call stacks that mark widgets as needing to be rebuilt.
getter/setter pair
debugProfileBuildsEnabled ↔ bool
Adds Timeline events for every Widget built.
getter/setter pair
debugProfileBuildsEnabledUserWidgets ↔ bool
Adds Timeline events for every user-created Widget built.
getter/setter pair
emptyTextSelectionControls → TextSelectionControls
Text selection controls that do not show any toolbars or handles.
final
imageCache → ImageCache
The singleton that implements the Flutter framework's image cache.
no setter
primaryFocus → FocusNode?
Provides convenient access to the current FocusManager.primaryFocus from the WidgetsBinding instance.
no setter
Functions
applyBoxFit(BoxFit fit, Size inputSize, Size outputSize) → FittedSizes
Apply a BoxFit value.
axisDirectionIsReversed(AxisDirection axisDirection) → bool
Returns whether traveling along the given axis direction visits coordinates along that axis in numerically decreasing order.
axisDirectionToAxis(AxisDirection axisDirection) → Axis
Returns the Axis that contains the given AxisDirection.
basicLocaleListResolution(List<Locale>? preferredLocales, Iterable<Locale> supportedLocales) → Locale
The default locale resolution algorithm.
buildTextSpanWithSpellCheckSuggestions(TextEditingValue value, bool composingWithinCurrentTextRange, TextStyle? style, TextStyle misspelledTextStyle, SpellCheckResults spellCheckResults) → TextSpan
Builds the TextSpan tree given the current state of the text input and spell check results.
childDragAnchorStrategy(Draggable<Object> draggable, BuildContext context, Offset position) → Offset
Display the feedback anchored at the position of the original child.
combineKeyEventResults(Iterable<KeyEventResult> results) → KeyEventResult
Combine the results returned by multiple FocusOnKeyCallbacks or FocusOnKeyEventCallbacks.
combineSemanticsInfo(List<InlineSpanSemanticsInformation> infoList) → List<InlineSpanSemanticsInformation>
Combines _semanticsInfo entries where permissible.
createLocalImageConfiguration(BuildContext context, {Size? size}) → ImageConfiguration
Creates an ImageConfiguration based on the given BuildContext (and optionally size).
debugAssertAllPaintingVarsUnset(String reason, {bool debugDisableShadowsOverride = false}) → bool
Returns true if none of the painting library debug variables have been changed.
debugAssertAllWidgetVarsUnset(String reason) → bool
Returns true if none of the widget library debug variables have been changed.
debugCheckCanResolveTextDirection(TextDirection? direction, String target) → bool
Asserts that a given TextDirection is not null.
debugCheckHasDirectionality(BuildContext context, {String? why, String? hint, String? alternative}) → bool
Asserts that the given context has a Directionality ancestor.
debugCheckHasMediaQuery(BuildContext context) → bool
Asserts that the given context has a MediaQuery ancestor.
debugCheckHasOverlay(BuildContext context) → bool
Asserts that the given context has an Overlay ancestor.
debugCheckHasTable(BuildContext context) → bool
Asserts that the given context has a Table ancestor.
debugCheckHasWidgetsLocalizations(BuildContext context) → bool
Asserts that the given context has a Localizations ancestor that contains a WidgetsLocalizations delegate.
debugChildrenHaveDuplicateKeys(Widget parent, Iterable<Widget> children, {String? message}) → bool
Asserts if the given child list contains any duplicate non-null keys.
debugDescribeFocusTree() → String
Returns a text representation of the current focus tree, along with the current attributes on each node.
debugDescribeTransform(Matrix4? transform) → List<String>
Returns a list of strings representing the given transform in a format useful for TransformProperty.
debugDumpApp() → void
Print a string representation of the currently running app.
debugDumpFocusTree() → void
Prints a text representation of the current focus tree, along with the current attributes on each node.
debugDumpLayerTree() → void
Prints a textual representation of the layer trees.
debugDumpRenderTree() → void
Prints a textual representation of the render trees.
debugFlushLastFrameImageSizeInfo() → void
Flushes inter-frame tracking of image size information from paintImage.
debugIsLocalCreationLocation(Object object) → bool
Returns if an object is user created.
debugIsWidgetLocalCreation(Widget widget) → bool
Returns true if a Widget is user created.
debugItemsHaveDuplicateKeys(Iterable<Widget> items) → bool
Asserts if the given list of items contains any duplicate non-null keys.
debugPrintStack({StackTrace? stackTrace, String? label, int? maxFrames}) → void
Dump the stack to the console using debugPrint and FlutterError.defaultStackFilter.
debugTransformDebugCreator(Iterable<DiagnosticsNode> properties) → Iterable<DiagnosticsNode>
Transformer to parse and gather information about DiagnosticsDebugCreator.
debugWidgetBuilderValue(Widget widget, Widget? built) → void
Asserts that the built widget is not null.
decodeImageFromList(Uint8List bytes) → Future<Image>
Creates an image from a list of bytes.
defaultScrollNotificationPredicate(ScrollNotification notification) → bool
A ScrollNotificationPredicate that checks whether notification.depth == 0, which means that the notification did not bubble through any intervening scrolling widgets.
flipAxis(Axis direction) → Axis
Returns the opposite of the given Axis.
flipAxisDirection(AxisDirection axisDirection) → AxisDirection
Returns the opposite of the given AxisDirection.
getAxisDirectionFromAxisReverseAndDirectionality(BuildContext context, Axis axis, bool reverse) → AxisDirection
Returns the AxisDirection in the given Axis in the current Directionality (or the reverse if reverse is true).
intentForMacOSSelector(String selectorName) → Intent?
Maps the selector from NSStandardKeyBindingResponding to the Intent if the selector is recognized.
lerpFontVariations(List<FontVariation>? a, List<FontVariation>? b, double t) → List<FontVariation>?
Interpolate between two lists of FontVariation objects.
paintBorder(Canvas canvas, Rect rect, {BorderSide top = BorderSide.none, BorderSide right = BorderSide.none, BorderSide bottom = BorderSide.none, BorderSide left = BorderSide.none}) → void
Paints a border around the given rectangle on the canvas.
paintImage({required Canvas canvas, required Rect rect, required Image image, String? debugImageLabel, double scale = 1.0, double opacity = 1.0, ColorFilter? colorFilter, BoxFit? fit, Alignment alignment = Alignment.center, Rect? centerSlice, ImageRepeat repeat = ImageRepeat.noRepeat, bool flipHorizontally = false, bool invertColors = false, FilterQuality filterQuality = FilterQuality.medium, bool isAntiAlias = false, BlendMode blendMode = BlendMode.srcOver}) → void
Paints an image into the given rectangle on the canvas.
paintZigZag(Canvas canvas, Paint paint, Offset start, Offset end, int zigs, double width) → void
Draw a line between two points, which cuts diagonally back and forth across the line that connects the two points.
pointerDragAnchorStrategy(Draggable<Object> draggable, BuildContext context, Offset position) → Offset
Display the feedback anchored at the position of the touch that started the drag.
positionDependentBox({required Size size, required Size childSize, required Offset target, required bool preferBelow, double verticalOffset = 0.0, double margin = 10.0}) → Offset
Position a child box within a container box, either above or below a target point.
precacheImage(ImageProvider<Object> provider, BuildContext context, {Size? size, ImageErrorListener? onError}) → Future<void>
Prefetches an image into the image cache.
runApp(Widget app) → void
Inflate the given widget and attach it to the view.
runWidget(Widget app) → void
Inflate the given widget and bootstrap the widget tree.
showGeneralDialog<T extends Object?>({required BuildContext context, required RoutePageBuilder pageBuilder, bool barrierDismissible = false, String? barrierLabel, Color barrierColor = const Color(0x80000000), Duration transitionDuration = const Duration(milliseconds: 200), RouteTransitionsBuilder? transitionBuilder, bool useRootNavigator = true, bool fullscreenDialog = false, RouteSettings? routeSettings, Offset? anchorPoint, bool? requestFocus}) → Future<T?>
Displays a dialog above the current contents of the app.
textDirectionToAxisDirection(TextDirection textDirection) → AxisDirection
Returns the AxisDirection in which reading occurs in the given TextDirection.
Typedefs
ActionListenerCallback = void Function(Action<Intent> action)
The kind of callback that an Action uses to notify of changes to the action's state.
AnimatableCallback<T> = T Function(double value)
A typedef used by Animatable.fromCallback to create an Animatable from a callback.
AnimatedCrossFadeBuilder = Widget Function(Widget topChild, Key topChildKey, Widget bottomChild, Key bottomChildKey)
Signature for the AnimatedCrossFade.layoutBuilder callback.
AnimatedItemBuilder = Widget Function(BuildContext context, int index, Animation<double> animation)
Signature for the builder callback used by AnimatedList, AnimatedList.separated & AnimatedGrid to build their animated children.
AnimatedRemovedItemBuilder = Widget Function(BuildContext context, Animation<double> animation)
Signature for the builder callback used in AnimatedListState.removeItem and AnimatedGridState.removeItem to animate their children after they have been removed.
AnimatedSwitcherLayoutBuilder = Widget Function(Widget? currentChild, List<Widget> previousChildren)
Signature for builders used to generate custom layouts for AnimatedSwitcher.
AnimatedSwitcherTransitionBuilder = Widget Function(Widget child, Animation<double> animation)
Signature for builders used to generate custom transitions for AnimatedSwitcher.
AnimatedTransitionBuilder = Widget Function(BuildContext context, Animation<double> animation, Widget? child)
Builder callback used by DualTransitionBuilder.
AnimationStatusListener = void Function(AnimationStatus status)
Signature for listeners attached using Animation.addStatusListener.
AppExitRequestCallback = Future<AppExitResponse> Function()
A callback type that is used by AppLifecycleListener.onExitRequested to ask the application if it wants to cancel application termination or not.
AppPrivateCommandCallback = void Function(String action, Map<String, dynamic> data)
Signature for the callback that reports the app private command results.
AsyncWidgetBuilder<T> = Widget Function(BuildContext context, AsyncSnapshot<T> snapshot)
Signature for strategies that build widgets based on asynchronous interaction.
AutocompleteFieldViewBuilder = Widget Function(BuildContext context, TextEditingController textEditingController, FocusNode focusNode, VoidCallback onFieldSubmitted)
The type of the Autocomplete callback which returns the widget that contains the input TextField or TextFormField.
AutocompleteOnSelected<T extends Object> = void Function(T option)
The type of the callback used by the RawAutocomplete widget to indicate that the user has selected an option.
AutocompleteOptionsBuilder<T extends Object> = FutureOr<Iterable<T>> Function(TextEditingValue textEditingValue)
The type of the RawAutocomplete callback which computes the list of optional completions for the widget's field, based on the text the user has entered so far.
AutocompleteOptionsViewBuilder<T extends Object> = Widget Function(BuildContext context, AutocompleteOnSelected<T> onSelected, Iterable<T> options)
The type of the RawAutocomplete callback which returns a Widget that displays the specified options and calls onSelected if the user selects an option.
AutocompleteOptionToString<T extends Object> = String Function(T option)
The type of the RawAutocomplete callback that converts an option value to a string which can be displayed in the widget's options menu.
BoxConstraintsTransform = BoxConstraints Function(BoxConstraints constraints)
Signature for a function that transforms a BoxConstraints to another BoxConstraints.
ChildIndexGetter = int? Function(Key key)
Called to find the new index of a child based on its key in case of reordering.
ConditionalElementVisitor = bool Function(Element element)
Signature for the callback to BuildContext.visitAncestorElements.
ConfirmDismissCallback = Future<bool?> Function(DismissDirection direction)
Signature used by Dismissible to give the application an opportunity to confirm or veto a dismiss gesture.
CreatePlatformViewCallback = PlatformViewController Function(PlatformViewCreationParams params)
Constructs a PlatformViewController.
CreateRectTween = Tween<Rect?> Function(Rect? begin, Rect? end)
Signature for a function that takes two Rect instances and returns a RectTween that transitions between them.
DecoderBufferCallback = Future<Codec> Function(ImmutableBuffer buffer, {bool allowUpscaling, int? cacheHeight, int? cacheWidth})
Performs the decode process for use in ImageProvider.loadBuffer.
DelegatedTransitionBuilder = Widget? Function(BuildContext context, Animation<double> animation, Animation<double> secondaryAnimation, bool allowSnapshotting, Widget? child)
Signature for a builder used to control a page's exit transition.
DidRemovePageCallback = void Function(Page<Object?> page)
Signature for the Navigator.onDidRemovePage callback.
DismissDirectionCallback = void Function(DismissDirection direction)
Signature used by Dismissible to indicate that it has been dismissed in the given direction.
DismissUpdateCallback = void Function(DismissUpdateDetails details)
Signature used by Dismissible to indicate that the dismissible has been dragged.
DragAnchorStrategy = Offset Function(Draggable<Object> draggable, BuildContext context, Offset position)
Signature for the strategy that determines the drag start point of a Draggable.
DragEndCallback = void Function(DraggableDetails details)
Signature for when the draggable is dropped.
DraggableCanceledCallback = void Function(Velocity velocity, Offset offset)
Signature for when a Draggable is dropped without being accepted by a DragTarget.
DragTargetAccept<T> = void Function(T data)
Signature for causing a DragTarget to accept the given data.
DragTargetAcceptWithDetails<T> = void Function(DragTargetDetails<T> details)
Signature for determining information about the acceptance by a DragTarget.
DragTargetBuilder<T> = Widget Function(BuildContext context, List<T?> candidateData, List rejectedData)
Signature for building children of a DragTarget.
DragTargetLeave<T> = void Function(T? data)
Signature for when a Draggable leaves a DragTarget.
DragTargetMove<T> = void Function(DragTargetDetails<T> details)
Signature for when a Draggable moves within a DragTarget.
DragTargetWillAccept<T> = bool Function(T? data)
Signature for determining whether the given data will be accepted by a DragTarget.
DragTargetWillAcceptWithDetails<T> = bool Function(DragTargetDetails<T> details)
Signature for determining whether the given data will be accepted by a DragTarget, based on provided information.
DragUpdateCallback = void Function(DragUpdateDetails details)
Signature for when a Draggable is dragged across the screen.
EditableTextContextMenuBuilder = Widget Function(BuildContext context, EditableTextState editableTextState)
Signature for a widget builder that builds a context menu for the given EditableTextState.
ElementCreatedCallback = void Function(Object element)
The signature of the function that gets called when the HtmlElementView DOM element is created.
ElementVisitor = void Function(Element element)
Signature for the callback to BuildContext.visitChildElements.
ErrorWidgetBuilder = Widget Function(FlutterErrorDetails details)
Signature for the constructor that is called when an error occurs while building a widget.
ExitWidgetSelectionButtonBuilder = Widget Function(BuildContext context, {required GlobalKey<State<StatefulWidget>> key, required VoidCallback onPressed, required String semanticsLabel})
Signature for the builder callback used by WidgetInspector.exitWidgetSelectionButtonBuilder.
ExpansibleBuilder = Widget Function(BuildContext context, Widget header, Widget body, Animation<double> animation)
The type of the callback that uses the header and body of an Expansible widget to build the widget.
ExpansibleComponentBuilder = Widget Function(BuildContext context, Animation<double> animation)
The type of the callback that returns the header or body of an Expansible.
FocusOnKeyCallback = KeyEventResult Function(FocusNode node, RawKeyEvent event)
Signature of a callback used by Focus.onKey and FocusScope.onKey to receive key events.
FocusOnKeyEventCallback = KeyEventResult Function(FocusNode node, KeyEvent event)
Signature of a callback used by Focus.onKeyEvent and FocusScope.onKeyEvent to receive key events.
FormFieldBuilder<T> = Widget Function(FormFieldState<T> field)
Signature for building the widget representing the form field.
FormFieldErrorBuilder = Widget Function(BuildContext context, String errorText)
Signature for a callback that builds an error widget.
FormFieldSetter<T> = void Function(T? newValue)
Signature for being notified when a form field changes value.
FormFieldValidator<T> = String? Function(T? value)
Signature for validating a form field.
GenerateAppTitle = String Function(BuildContext context)
The signature of WidgetsApp.onGenerateTitle.
GestureDragCancelCallback = void Function()
Signature for when the pointer that previously triggered a GestureDragDownCallback did not complete.
GestureDragDownCallback = void Function(DragDownDetails details)
Signature for when a pointer has contacted the screen and might begin to move.
GestureDragEndCallback = void Function(DragEndDetails details)
Signature for when a pointer that was previously in contact with the screen and moving is no longer in contact with the screen.
GestureDragStartCallback = void Function(DragStartDetails details)
Signature for when a pointer has contacted the screen and has begun to move.
GestureDragUpdateCallback = void Function(DragUpdateDetails details)
Signature for when a pointer that is in contact with the screen and moving has moved again.
GestureForcePressEndCallback = void Function(ForcePressDetails details)
Signature for when the pointer that previously triggered a ForcePressGestureRecognizer.onStart callback is no longer in contact with the screen.
GestureForcePressPeakCallback = void Function(ForcePressDetails details)
Signature used by ForcePressGestureRecognizer for when a pointer that has pressed with at least ForcePressGestureRecognizer.peakPressure.
GestureForcePressStartCallback = void Function(ForcePressDetails details)
Signature used by a ForcePressGestureRecognizer for when a pointer has pressed with at least ForcePressGestureRecognizer.startPressure.
GestureForcePressUpdateCallback = void Function(ForcePressDetails details)
Signature used by ForcePressGestureRecognizer during the frames after the triggering of a ForcePressGestureRecognizer.onStart callback.
GestureLongPressCallback = void Function()
Callback signature for LongPressGestureRecognizer.onLongPress.
GestureLongPressEndCallback = void Function(LongPressEndDetails details)
Callback signature for LongPressGestureRecognizer.onLongPressEnd.
GestureLongPressMoveUpdateCallback = void Function(LongPressMoveUpdateDetails details)
Callback signature for LongPressGestureRecognizer.onLongPressMoveUpdate.
GestureLongPressStartCallback = void Function(LongPressStartDetails details)
Callback signature for LongPressGestureRecognizer.onLongPressStart.
GestureLongPressUpCallback = void Function()
Callback signature for LongPressGestureRecognizer.onLongPressUp.
GestureRecognizerFactoryConstructor<T extends GestureRecognizer> = T Function()
Signature for closures that implement GestureRecognizerFactory.constructor.
GestureRecognizerFactoryInitializer<T extends GestureRecognizer> = void Function(T instance)
Signature for closures that implement GestureRecognizerFactory.initializer.
GestureScaleEndCallback = void Function(ScaleEndDetails details)
Signature for when the pointers are no longer in contact with the screen.
GestureScaleStartCallback = void Function(ScaleStartDetails details)
Signature for when the pointers in contact with the screen have established a focal point and initial scale of 1.0.
GestureScaleUpdateCallback = void Function(ScaleUpdateDetails details)
Signature for when the pointers in contact with the screen have indicated a new focal point and/or scale.
GestureTapCallback = void Function()
Signature for when a tap has occurred.
GestureTapCancelCallback = void Function()
Signature for when the pointer that previously triggered a GestureTapDownCallback will not end up causing a tap.
GestureTapDownCallback = void Function(TapDownDetails details)
Signature for when a pointer that might cause a tap has contacted the screen.
GestureTapUpCallback = void Function(TapUpDetails details)
Signature for when a pointer that will trigger a tap has stopped contacting the screen.
HeroFlightShuttleBuilder = Widget Function(BuildContext flightContext, Animation<double> animation, HeroFlightDirection flightDirection, BuildContext fromHeroContext, BuildContext toHeroContext)
A function that lets Heroes self supply a Widget that is shown during the hero's flight from one route to another instead of default (which is to show the destination route's instance of the Hero).
HeroPlaceholderBuilder = Widget Function(BuildContext context, Size heroSize, Widget child)
Signature for a function that builds a Hero placeholder widget given a child and a Size.
HttpClientProvider = HttpClient Function()
Signature for a method that returns an HttpClient.
ImageChunkListener = void Function(ImageChunkEvent event)
Signature for listening to ImageChunkEvent events.
ImageDecoderCallback = Future<Codec> Function(ImmutableBuffer buffer, {TargetImageSizeCallback? getTargetSize})
Performs the decode process for use in ImageProvider.loadImage.
ImageErrorListener = void Function(Object exception, StackTrace? stackTrace)
Signature for reporting errors when resolving images.
ImageErrorWidgetBuilder = Widget Function(BuildContext context, Object error, StackTrace? stackTrace)
Signature used by Image.errorBuilder to create a replacement widget to render instead of the image.
ImageFrameBuilder = Widget Function(BuildContext context, Widget child, int? frame, bool wasSynchronouslyLoaded)
Signature used by Image.frameBuilder to control the widget that will be used when an Image is built.
ImageListener = void Function(ImageInfo image, bool synchronousCall)
Signature for callbacks reporting that an image is available.
ImageLoadingBuilder = Widget Function(BuildContext context, Widget child, ImageChunkEvent? loadingProgress)
Signature used by Image.loadingBuilder to build a representation of the image's loading progress.
IndexedWidgetBuilder = Widget Function(BuildContext context, int index)
Signature for a function that creates a widget for a given index, e.g., in a list.
InitialRouteListFactory = List<Route> Function(String initialRoute)
The signature of WidgetsApp.onGenerateInitialRoutes.
InlineSpanVisitor = bool Function(InlineSpan span)
Called on each span as InlineSpan.visitChildren walks the InlineSpan tree.
InspectorSelectionChangedCallback = void Function()
Signature for the selection change callback used by WidgetInspectorService.selectionChangedCallback.
InteractiveViewerWidgetBuilder = Widget Function(BuildContext context, Quad viewport)
A signature for widget builders that take a Quad of the current viewport.
LayoutWidgetBuilder = Widget Function(BuildContext context, BoxConstraints constraints)
The signature of the LayoutBuilder builder function.
LocaleListResolutionCallback = Locale? Function(List<Locale>? locales, Iterable<Locale> supportedLocales)
The signature of WidgetsApp.localeListResolutionCallback.
LocaleResolutionCallback = Locale? Function(Locale? locale, Iterable<Locale> supportedLocales)
The signature of WidgetsApp.localeResolutionCallback.
MagnifierBuilder = Widget? Function(BuildContext context, MagnifierController controller, ValueNotifier<MagnifierInfo> magnifierInfo)
Signature for a builder that builds a Widget with a MagnifierController.
MenuItemSerializableIdGenerator = int Function(PlatformMenuItem item)
The signature for a function that generates unique menu item IDs for serialization of a PlatformMenuItem.
MoveExitWidgetSelectionButtonBuilder = Widget Function(BuildContext context, {required VoidCallback onPressed, required String semanticsLabel, bool usesDefaultAlignment})
Signature for the builder callback used by WidgetInspector.moveExitWidgetSelectionButtonBuilder.
NavigatorFinderCallback = NavigatorState Function(BuildContext context)
A callback that given a BuildContext finds a NavigatorState.
NestedScrollViewHeaderSliversBuilder = List<Widget> Function(BuildContext context, bool innerBoxIsScrolled)
Signature used by NestedScrollView for building its header.
NotificationListenerCallback<T extends Notification> = bool Function(T notification)
Signature for Notification listeners.
NullableIndexedWidgetBuilder = Widget? Function(BuildContext context, int index)
Signature for a function that creates a widget for a given index, e.g., in a list, but may return null.
OnInvokeCallback<T extends Intent> = Object? Function(T intent)
The signature of a callback accepted by CallbackAction.onInvoke.
OnKeyEventCallback = KeyEventResult Function(KeyEvent event)
Signature of a callback used by FocusManager.addEarlyKeyEventHandler and FocusManager.addLateKeyEventHandler.
OrientationWidgetBuilder = Widget Function(BuildContext context, Orientation orientation)
Signature for a function that builds a widget given an Orientation.
OverlayChildLayoutBuilder = Widget Function(BuildContext context, OverlayChildLayoutInfo info)
The signature of the widget builder callback used in OverlayPortal.overlayChildLayoutBuilder.
PageRouteFactory = PageRoute<T> Function<T>(RouteSettings settings, WidgetBuilder builder)
The signature of WidgetsApp.pageRouteBuilder.
PaintImageCallback = void Function(ImageSizeInfo info)
Called when the framework is about to paint an Image to a Canvas with an ImageSizeInfo that contains the decoded size of the image as well as its output size.
PlatformViewSurfaceFactory = Widget Function(BuildContext context, PlatformViewController controller)
A factory for a surface presenting a platform view as part of the widget hierarchy.
PointerCancelEventListener = void Function(PointerCancelEvent event)
Signature for listening to PointerCancelEvent events.
PointerDownEventListener = void Function(PointerDownEvent event)
Signature for listening to PointerDownEvent events.
PointerMoveEventListener = void Function(PointerMoveEvent event)
Signature for listening to PointerMoveEvent events.
PointerUpEventListener = void Function(PointerUpEvent event)
Signature for listening to PointerUpEvent events.
PopInvokedCallback = void Function(bool didPop)
A callback type for informing that a navigation pop has been invoked, whether or not it was handled successfully.
PopInvokedWithResultCallback<T> = void Function(bool didPop, T? result)
A callback type for informing that a navigation pop has been invoked, whether or not it was handled successfully.
PopPageCallback = bool Function(Route route, dynamic result)
Signature for the Navigator.onPopPage callback.
PopResultCallback<T> = void Function(T? result)
A signature for a function that is passed the result of a Route.
RadioBuilder = Widget Function(BuildContext context, ToggleableStateMixin<StatefulWidget> state)
Signature for RawRadio.builder.
RawMenuAnchorChildBuilder = Widget Function(BuildContext context, MenuController controller, Widget? child)
Signature for the builder function used by RawMenuAnchor.builder to build the widget that the RawMenuAnchor surrounds.
RawMenuAnchorCloseRequestedCallback = void Function(VoidCallback hideOverlay)
Signature for the callback used by RawMenuAnchor.onCloseRequested to intercept requests to close a menu.
RawMenuAnchorOpenRequestedCallback = void Function(Offset? position, VoidCallback showOverlay)
Signature for the callback used by RawMenuAnchor.onOpenRequested to intercept requests to open a menu.
RawMenuAnchorOverlayBuilder = Widget Function(BuildContext context, RawMenuOverlayInfo info)
Signature for the builder function used by RawMenuAnchor.overlayBuilder to build a menu's overlay.
RebuildDirtyWidgetCallback = void Function(Element e, bool builtOnce)
Signature for debugOnRebuildDirtyWidget implementations.
RegisterServiceExtensionCallback = void Function({required ServiceExtensionCallback callback, required String name})
Signature for a method that registers the service extension callback with the given name.
RegisterViewFactory = void Function(String, Object (int viewId), {bool isVisible})
Function signature for ui_web.platformViewRegistry.registerViewFactory.
RenderConstrainedLayoutBuilder<LayoutInfoType, ChildType extends RenderObject> = RenderAbstractLayoutBuilderMixin<LayoutInfoType, ChildType>
Generic mixin for RenderObjects created by an AbstractLayoutBuilder with the the same LayoutInfoType.
ReorderCallback = void Function(int oldIndex, int newIndex)
A callback used by ReorderableList to report that a list item has moved to a new position in the list.
ReorderDragBoundaryProvider = DragBoundaryDelegate<Rect>? Function(BuildContext context)
Used to provide drag boundaries during drag-and-drop reordering.
ReorderItemProxyDecorator = Widget Function(Widget child, int index, Animation<double> animation)
Signature for the builder callback used to decorate the dragging item in ReorderableList and SliverReorderableList.
RestorableRouteBuilder<T> = Route<T> Function(BuildContext context, Object? arguments)
Creates a Route that is to be added to a Navigator.
RouteCompletionCallback<T> = void Function(T result)
A callback to handle the result of a completed Route.
RouteFactory = Route? Function(RouteSettings settings)
Creates a route for the given route settings.
RouteListFactory = List<Route> Function(NavigatorState navigator, String initialRoute)
Creates a series of one or more routes.
RoutePageBuilder = Widget Function(BuildContext context, Animation<double> animation, Animation<double> secondaryAnimation)
Signature for the function that builds a route's primary contents. Used in PageRouteBuilder and showGeneralDialog.
RoutePredicate = bool Function(Route route)
Signature for the Navigator.popUntil predicate argument.
RoutePresentationCallback = String Function(NavigatorState navigator, Object? arguments)
A callback that given some arguments and a navigator adds a new restorable route to that navigator and returns the opaque ID of that new route.
RouteTransitionsBuilder = Widget Function(BuildContext context, Animation<double> animation, Animation<double> secondaryAnimation, Widget child)
Signature for the function that builds a route's transitions. Used in PageRouteBuilder and showGeneralDialog.
ScrollableWidgetBuilder = Widget Function(BuildContext context, ScrollController scrollController)
The signature of a method that provides a BuildContext and ScrollController for building a widget that may overflow the draggable Axis of the containing DraggableScrollableSheet.
ScrollControllerCallback = void Function(ScrollPosition position)
Signature for when a ScrollController has added or removed a ScrollPosition.
ScrollIncrementCalculator = double Function(ScrollIncrementDetails details)
A typedef for a function that can calculate the offset for a type of scroll increment given a ScrollIncrementDetails.
ScrollNotificationCallback = void Function(ScrollNotification notification)
A ScrollNotification listener for ScrollNotificationObserver.
ScrollNotificationPredicate = bool Function(ScrollNotification notification)
A predicate for ScrollNotification, used to customize widgets that listen to notifications from their children.
SelectableRegionContextMenuBuilder = Widget Function(BuildContext context, SelectableRegionState selectableRegionState)
Signature for a widget builder that builds a context menu for the given SelectableRegionState.
SelectionChangedCallback = void Function(TextSelection selection, SelectionChangedCause? cause)
Signature for the callback that reports when the user changes the selection (including the cursor location).
SemanticIndexCallback = int? Function(Widget widget, int localIndex)
A callback which produces a semantic index given a widget and the local index.
SemanticsBuilderCallback = List<CustomPainterSemantics> Function(Size size)
Signature of the function returned by CustomPainter.semanticsBuilder.
ShaderCallback = Shader Function(Rect bounds)
Signature for a function that creates a Shader for a given Rect.
ShaderWarmUpImageCallback = bool Function(Image image)
The signature of debugCaptureShaderWarmUpImage.
ShaderWarmUpPictureCallback = bool Function(Picture picture)
The signature of debugCaptureShaderWarmUpPicture.
SharedAppDataInitCallback<T> = T Function()
The type of the SharedAppData.getValue init parameter.
SliverLayoutWidgetBuilder = Widget Function(BuildContext context, SliverConstraints constraints)
The signature of the SliverLayoutBuilder builder function.
StatefulWidgetBuilder = Widget Function(BuildContext context, StateSetter setState)
Signature for the builder callback used by StatefulBuilder.
StateSetter = void Function(VoidCallback fn)
The signature of State.setState functions.
TapBehaviorButtonBuilder = Widget Function(BuildContext context, {required VoidCallback onPressed, required bool selectionOnTapEnabled, required String semanticsLabel})
Signature for the builder callback used by WidgetInspector.tapBehaviorButtonBuilder.
TapRegionCallback = void Function(PointerDownEvent event)
Signature for a callback called for a PointerDownEvent relative to a TapRegion.
TapRegionUpCallback = void Function(PointerUpEvent event)
Signature for a callback called for a PointerUpEvent relative to a TapRegion.
ToolbarBuilder = Widget Function(BuildContext context, Widget child)
The type for a Function that builds a toolbar's container with the given child.
TransformCallback = Matrix4 Function(double animationValue)
Signature for the callback to MatrixTransition.onTransform.
TransitionBuilder = Widget Function(BuildContext context, Widget? child)
A builder that builds a widget given a child.
TraversalRequestFocusCallback = void Function(FocusNode node, {double? alignment, ScrollPositionAlignmentPolicy? alignmentPolicy, Curve? curve, Duration? duration})
Signature for the callback that's called when a traversal policy requests focus.
TreeSliverNodeBuilder = Widget Function(BuildContext context, TreeSliverNode<Object?> node, AnimationStyle animationStyle)
Signature for a function that creates a Widget to represent the given TreeSliverNode in the TreeSliver.
TreeSliverNodeCallback = void Function(TreeSliverNode<Object?> node)
Signature for a function that is called when a TreeSliverNode is toggled, changing its expanded state.
TreeSliverRowExtentBuilder = double Function(TreeSliverNode<Object?> node, SliverLayoutDimensions dimensions)
Signature for a function that returns an extent for the given TreeSliverNode in the TreeSliver.
TweenConstructor<T extends Object> = Tween<T> Function(T targetValue)
Signature for a Tween factory.
TweenVisitor<T extends Object> = Tween<T>? Function(Tween<T>? tween, T targetValue, TweenConstructor<T> constructor)
Signature for callbacks passed to ImplicitlyAnimatedWidgetState.forEachTween.
TwoDimensionalIndexedWidgetBuilder = Widget? Function(BuildContext context, ChildVicinity vicinity)
Signature for a function that creates a widget for a given ChildVicinity, e.g., in a TwoDimensionalScrollView, but may return null.
TwoDimensionalViewportBuilder = Widget Function(BuildContext context, ViewportOffset verticalPosition, ViewportOffset horizontalPosition)
Signature used by TwoDimensionalScrollable to build the viewport through which the scrollable content is displayed.
ValueChanged<T> = void Function(T value)
Signature for callbacks that report that an underlying value has changed.
ValueGetter<T> = T Function()
Signature for callbacks that are to report a value on demand.
ValueListenableTransformer<T> = T Function(T)
Signature for method used to transform values in Animation.fromValueListenable.
ValueSetter<T> = void Function(T value)
Signature for callbacks that report that a value has been set.
ValueWidgetBuilder<T> = Widget Function(BuildContext context, T value, Widget? child)
Builds a Widget when given a concrete value of a ValueListenable<T>.
ViewportBuilder = Widget Function(BuildContext context, ViewportOffset position)
Signature used by Scrollable to build the viewport through which the scrollable content is displayed.
VoidCallback = void Function()
Signature of callbacks that have no arguments and return no data.
WidgetBuilder = Widget Function(BuildContext context)
Signature for a function that creates a widget, e.g. StatelessWidget.build or State.build.
WidgetPropertyResolver<T> = T Function(Set<WidgetState> states)
Signature for the function that returns a value of type T based on a given set of states.
WidgetStateMap<T> = Map<WidgetStatesConstraint, T>
A Map used to resolve to a single value of type T based on the current set of Widget states.
WillPopCallback = Future<bool> Function()
Signature for a callback that verifies that it's OK to call Navigator.pop.
Exceptions / Errors
FlutterError
Error class used to report Flutter-specific assertion failures and contract violations.
NetworkImageLoadException
The exception thrown when the HTTP request to load a network image fails.
TickerCanceled
Exception thrown by Ticker objects on the TickerFuture.orCancel future when the ticker is canceled.