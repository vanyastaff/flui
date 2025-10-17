rendering library
The Flutter rendering tree.

To use, import package:flutter/rendering.dart.

The RenderObject hierarchy is used by the Flutter Widgets library to implement its layout and painting back-end. Generally, while you may use custom RenderBox classes for specific effects in your applications, most of the time your only interaction with the RenderObject hierarchy will be in debugging layout issues.

If you are developing your own library or application directly on top of the rendering library, then you will want to have a binding (see BindingBase). You can use RenderingFlutterBinding, or you can create your own binding. If you create your own binding, it needs to import at least ServicesBinding, GestureBinding, SchedulerBinding, PaintingBinding, and RendererBinding. The rendering library does not automatically create a binding, but relies on one being initialized with those features.

Classes
AccessibilityFeatures
Additional accessibility features that may be enabled by the platform.
Accumulator
Mutable wrapper of an integer that can be passed by reference to track a value across a recursive stack.
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
AnnotatedRegionLayer<T extends Object>
A composited layer which annotates its children with a value. Pushing this layer to the tree is the common way of adding an annotation.
AnnotationEntry<T>
Information collected for an annotation that is found in the layer tree.
AnnotationResult<T>
Information collected about a list of annotations that are found in the layer tree.
AnnounceSemanticsEvent
An event for a semantic announcement.
AssetBundleImageKey
Key for the image obtained by an AssetImage or ExactAssetImage.
AssetBundleImageProvider
A subclass of ImageProvider that knows about AssetBundles.
AssetImage
Fetches an image from an AssetBundle, having determined the exact image to use based on the context.
AttributedString
A string that carries a list of StringAttributes.
AttributedStringProperty
A DiagnosticsProperty for AttributedStrings, which shows a string when there are no attributes, and more details otherwise.
AutomaticNotchedShape
A NotchedShape created from ShapeBorders.
BackdropFilterLayer
A composited layer that applies a filter to the existing contents of the scene.
BackdropKey
A backdrop key uniquely identifies the backdrop that a BackdropFilterLayer samples from.
BeveledRectangleBorder
A rectangular border with flattened or "beveled" corners.
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
BorderSide
A side of a border of a box.
BoxBorder
Base class for box borders that can paint as rectangles, circles, or rounded rectangles.
BoxConstraints
Immutable layout constraints for RenderBox layout.
BoxDecoration
An immutable description of how to paint a box.
BoxHitTestEntry
A hit test entry used by RenderBox.
BoxHitTestResult
The result of performing a hit test on RenderBoxes.
BoxPainter
A stateful class that can paint a particular Decoration.
BoxParentData
Parent data used by RenderBox and its subclasses.
BoxShadow
A shadow cast by a box.
Canvas
An interface for recording graphical operations.
ChildLayoutHelper
A collection of static functions to layout a RenderBox child with the given set of BoxConstraints.
ChildSemanticsConfigurationsResult
The result that contains the arrangement for the child SemanticsConfigurations.
ChildSemanticsConfigurationsResultBuilder
The builder to build a ChildSemanticsConfigurationsResult based on its annotations.
CircleBorder
A border that fits a circle within the available space.
CircularNotchedRectangle
A rectangle with a smooth circular notch.
ClearSelectionEvent
Clears the selection from the Selectable and removes any existing highlight as if there is no selection at all.
ClipContext
Clip utilities used by PaintingContext.
ClipPathLayer
A composite layer that clips its children using a path.
ClipRectLayer
A composite layer that clips its children using a rectangle.
ClipRRectLayer
A composite layer that clips its children using a rounded rectangle.
ClipRSuperellipseLayer
A composite layer that clips its children using a rounded superellipse.
Color
An immutable color value in ARGB format.
ColorFilter
A description of a color filter to apply when drawing a shape or compositing a layer with a particular Paint. A color filter is a function that takes two colors, and outputs one color. When applied during compositing, it is independently applied to each pixel of the layer being drawn before the entire layer is merged with the destination.
ColorFilterLayer
A composite layer that applies a ColorFilter to its children.
ColorProperty
DiagnosticsProperty that has an Color as value.
ColorSwatch<T>
A color that has a small table of related colors called a "swatch".
Constraints
An abstract set of layout constraints.
ContainerBoxParentData<ChildType extends RenderObject>
Abstract ParentData subclass for RenderBox subclasses that want the ContainerRenderObjectMixin.
ContainerLayer
A composited layer that has a list of children.
ContinuousRectangleBorder
A rectangular border with smooth continuous transitions between the straight sides and the rounded corners.
CustomClipper<T>
An interface for providing custom clips.
CustomPainter
The interface used by CustomPaint (in the widgets library) and RenderCustomPaint (in the rendering library).
CustomPainterSemantics
Contains properties describing information drawn in a rectangle contained by the Canvas used by a CustomPaint.
CustomSemanticsAction
An identifier of a custom semantics action.
Decoration
A description of a box decoration (a decoration applied to a Rect).
DecorationImage
An image for a box decoration.
DecorationImagePainter
The painter for a DecorationImage.
DiagnosticPropertiesBuilder
Builder to accumulate properties and configuration used to assemble a DiagnosticsNode from a Diagnosticable object.
DiagnosticsDebugCreator
A class that creates DiagnosticsNode by wrapping RenderObject.debugCreator.
DiagnosticsNode
Defines diagnostics data for a value.
DiagnosticsProperty<T>
Property with a value of type T.
DirectionallyExtendSelectionEvent
Extends the current selection with respect to a direction.
DoubleProperty
Property describing a double value with an optional unit of measurement.
EdgeInsets
An immutable set of offsets in each of the four cardinal directions.
EdgeInsetsDirectional
An immutable set of offsets in each of the four cardinal directions, but whose horizontal components are dependent on the writing direction.
EdgeInsetsGeometry
Base class for EdgeInsets that allows for text-direction aware resolution.
EnumProperty<T extends Enum?>
DiagnosticsProperty that has an Enum as value.
ErrorDescription
An explanation of the problem and its cause, any information that may help track down the problem, background information, etc.
ErrorHint
An ErrorHint provides specific, non-obvious advice that may be applicable.
ErrorSummary
A short (one line) description of the problem that was detected.
ExactAssetImage
Fetches an image from an AssetBundle, associating it with the given scale.
FileImage
Decodes the given File object as an image, associating it with the given scale.
FittedSizes
The pair of sizes returned by applyBoxFit.
FixedColumnWidth
Sizes the column to a specific number of pixels.
FlagProperty
Property where the description is either ifTrue or ifFalse depending on whether value is true or false.
FlexColumnWidth
Sizes the column by taking a part of the remaining space once all the other columns have been laid out.
FlexParentData
Parent data for use with RenderFlex.
FloatingHeaderSnapConfiguration
Specifies how a floating header is to be "snapped" (animated) into or out of view.
FlowDelegate
A delegate that controls the appearance of a flow layout.
FlowPaintingContext
A context in which a FlowDelegate paints.
FlowParentData
Parent data for use with RenderFlow.
FlutterLogoDecoration
An immutable description of how to paint Flutter's logo.
FocusSemanticEvent
An event to move the accessibility focus.
FollowerLayer
A composited layer that applies a transformation matrix to its children such that they are positioned to match a LeaderLayer.
FontFeature
A feature tag and value that affect the selection of glyphs in a font.
FontVariation
An axis tag and value that can be used to customize variable fonts.
FontWeight
The thickness of the glyphs used to draw the text.
FractionalOffset
An offset that's expressed as a fraction of a Size.
FractionalOffsetTween
An interpolation between two fractional offsets.
FractionColumnWidth
Sizes the column to a fraction of the table's constraints' maxWidth.
GlyphInfo
The measurements of a character (or a sequence of visually connected characters) within a paragraph.
Gradient
A 2D gradient.
GradientRotation
A GradientTransform that rotates the gradient around the center-point of its bounding box.
GradientTransform
Base class for transforming gradient shaders without applying the same transform to the entire canvas.
GranularlyExtendSelectionEvent
Extends the start or end of the selection by a given TextGranularity.
HitTestEntry<T extends HitTestTarget>
Data collected during a hit test about a specific HitTestTarget.
HitTestResult
The result of performing a hit test.
HSLColor
A color represented using alpha, hue, saturation, and lightness.
HSVColor
A color represented using alpha, hue, saturation, and value.
ImageCache
Class for caching images.
ImageCacheStatus
Information about how the ImageCache is tracking an image.
ImageChunkEvent
An immutable notification of image bytes that have been incrementally loaded.
ImageConfiguration
Configuration information passed to the ImageProvider.resolve method to select a specific image.
ImageFilterLayer
A composite layer that applies an ui.ImageFilter to its children.
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
InlineSpan
An immutable span of inline content which forms part of a paragraph.
InlineSpanSemanticsInformation
The textual and semantic label information for an InlineSpan.
IntProperty
An int valued property with an optional unit the value is measured in.
IntrinsicColumnWidth
Sizes the column according to the intrinsic dimensions of all the cells in that column.
Key
A Key is an identifier for Widgets, Elements and SemanticsNodes.
Layer
A composited layer.
LayerHandle<T extends Layer>
A handle to prevent a Layer's platform graphics resources from being disposed.
LayerLink
An object that a LeaderLayer can register with.
LeaderLayer
A composited layer that can be followed by a FollowerLayer.
LinearBorder
An OutlinedBorder like BoxBorder that allows one to define a rectangular (box) border in terms of zero to four LinearBorderEdges, each of which is rendered as a single line.
LinearBorderEdge
Defines the relative size and alignment of one LinearBorder edge.
LinearGradient
A 2D linear gradient.
LineMetrics
LineMetrics stores the measurements and statistics of a single line in the paragraph.
ListBodyParentData
Parent data for use with RenderListBody.
ListWheelChildManager
A delegate used by RenderListWheelViewport to manage its children.
ListWheelParentData
ParentData for use with RenderListWheelViewport.
Locale
An identifier used to select a user's language and formatting preferences.
LocaleStringAttribute
A string attribute that causes the assistive technologies, e.g. VoiceOver, to treat string as a certain language.
LongPressSemanticsEvent
An event which triggers long press semantic feedback.
MaskFilter
A mask filter to apply to shapes as they are painted. A mask filter is a function that takes a bitmap of color pixels, and returns another bitmap of color pixels.
Matrix4
4D Matrix. Values are stored in column major order.
MatrixUtils
Utility functions for working with matrices.
MaxColumnWidth
Sizes the column such that it is the size that is the maximum of two column width specifications.
MemoryImage
Decodes the given Uint8List buffer as an image, associating it with the given scale.
MinColumnWidth
Sizes the column such that it is the size that is the minimum of two column width specifications.
MouseCursor
An interface for mouse cursor definitions.
MouseTracker
Tracks the relationship between mouse devices and annotations, and triggers mouse events and cursor changes accordingly.
MultiChildLayoutDelegate
A delegate that controls the layout of multiple children.
MultiChildLayoutParentData
ParentData used by RenderCustomMultiChildLayoutBox.
MultiFrameImageStreamCompleter
Manages the decoding and scheduling of image frames.
NetworkImage
Fetches the given URL from the network, associating it with the given scale.
NotchedShape
A shape with a notch in its outline.
Offset
An immutable 2D floating-point offset.
OffsetLayer
A layer that is displayed at an offset from its parent layer.
OneFrameImageStreamCompleter
Manages the loading of dart:ui.Image objects for static ImageStreams (those with only one frame).
OpacityLayer
A composited layer that makes its children partially transparent.
OrdinalSortKey
A SemanticsSortKey that sorts based on the double value it is given.
OutlinedBorder
A ShapeBorder that draws an outline with the width and color specified by side.
OvalBorder
A border that fits an elliptical shape.
OverScrollHeaderStretchConfiguration
Specifies how a stretched header is to trigger an AsyncCallback.
Paint
A description of the style to use when drawing on a Canvas.
PaintingContext
A place to paint.
ParentData
Base class for data associated with a RenderObject by its parent.
Path
A complex, one-dimensional subset of a plane.
PerformanceOverlayLayer
A layer that indicates to the compositor that it should display certain performance statistics within it.
PersistentHeaderShowOnScreenConfiguration
Specifies how a pinned header or a floating header should react to RenderObject.showOnScreen calls.
PictureLayer
A composited layer containing a ui.Picture.
PipelineManifold
Manages a tree of PipelineOwners.
PipelineOwner
The pipeline owner manages the rendering pipeline.
PlaceholderDimensions
Holds the Size and baseline required to represent the dimensions of a placeholder in text.
PlaceholderSpan
An immutable placeholder that is embedded inline within text.
PlaceholderSpanIndexSemanticsTag
Used by the RenderParagraph to map its rendering children to their corresponding semantics nodes.
PlatformViewLayer
A layer that shows an embedded UIView on iOS.
PlatformViewRenderBox
A render object for embedding a platform view.
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
RadialGradient
A 2D radial gradient.
Radius
A radius for either circular or elliptical shapes.
Rect
An immutable, 2D, axis-aligned, floating-point rectangle whose coordinates are relative to a given origin.
RelativeRect
An immutable 2D, axis-aligned, floating-point rectangle whose coordinates are given relative to another rectangle's edges, known as the container. Since the dimensions of the rectangle are relative to those of the container, this class has no width and height members. To determine the width or height of the rectangle, convert it to a Rect using toRect() (passing the container's own Rect), and then examine that object.
RenderAbsorbPointer
A render object that absorbs pointers during hit testing.
RenderAbstractViewport
An interface for render objects that are bigger on the inside.
RenderAligningShiftedBox
Abstract class for one-child-layout render boxes that use a AlignmentGeometry to align their children.
RenderAndroidView
A render object for an Android view.
RenderAnimatedOpacity
Makes its child partially transparent, driven from an Animation.
RenderAnimatedSize
A render object that animates its size to its child's size over a given duration and with a given curve. If the child's size itself animates (i.e. if it changes size two frames in a row, as opposed to abruptly changing size in one frame then remaining that size in subsequent frames), this render object sizes itself to fit the child instead of animating itself.
RenderAnnotatedRegion<T extends Object>
Render object which inserts an AnnotatedRegionLayer into the layer tree.
RenderAppKitView
A render object for a macOS platform view.
RenderAspectRatio
Attempts to size the child to a specific aspect ratio.
RenderBackdropFilter
Applies a filter to the existing painted content and then paints child.
RenderBaseline
Shifts the child down such that the child's baseline (or the bottom of the child, if the child has no baseline) is baseline logical pixels below the top of this box, then sizes this box to contain the child.
RenderBlockSemantics
Causes the semantics of all earlier render objects below the same semantic boundary to be dropped.
RenderBox
A render object in a 2D Cartesian coordinate system.
RenderClipOval
Clips its child using an oval.
RenderClipPath
Clips its child using a path.
RenderClipRect
Clips its child using a rectangle.
RenderClipRRect
Clips its child using a rounded rectangle.
RenderClipRSuperellipse
Clips its child using a rounded superellipse.
RenderConstrainedBox
Imposes additional constraints on its child.
RenderConstrainedOverflowBox
A render object that imposes different constraints on its child than it gets from its parent, possibly allowing the child to overflow the parent.
RenderConstraintsTransformBox
A RenderBox that applies an arbitrary transform to its constraints, and sizes its child using the resulting BoxConstraints, optionally clipping, or treating the overflow as an error.
RenderCustomMultiChildLayoutBox
Defers the layout of multiple children to a delegate.
RenderCustomPaint
Provides a canvas on which to draw during the paint phase.
RenderCustomSingleChildLayoutBox
Defers the layout of its single child to a delegate.
RenderDarwinPlatformView<T extends DarwinPlatformViewController>
Common render-layer functionality for iOS and macOS platform views.
RenderDecoratedBox
Paints a Decoration either before or after its child paints.
RenderDecoratedSliver
Paints a Decoration either before or after its child paints.
RenderEditable
Displays some text in a scrollable container with a potentially blinking cursor and with gesture recognizers.
RenderEditablePainter
An interface that paints within a RenderEditable's bounds, above or beneath its text content.
RenderErrorBox
A render object used as a placeholder when an error occurs.
RenderExcludeSemantics
Excludes this subtree from the semantic tree.
RenderFittedBox
Scales and positions its child within itself according to fit.
RenderFlex
Displays its children in a one-dimensional array.
RenderFlow
Implements the flow layout algorithm.
RenderFollowerLayer
Transform the child so that its origin is offset from the origin of the RenderLeaderLayer with the same LayerLink.
RenderFractionallySizedOverflowBox
Sizes its child to a fraction of the total available space.
RenderFractionalTranslation
Applies a translation transformation before painting its child.
RenderIgnoreBaseline
Excludes the child from baseline computations in the parent.
RenderIgnorePointer
A render object that is invisible during hit testing.
RenderImage
An image in the render tree.
RenderIndexedSemantics
A render objects that annotates semantics with an index.
RenderIndexedStack
Implements the same layout algorithm as RenderStack but only paints the child specified by index.
RenderingFlutterBinding
A concrete binding for applications that use the Rendering framework directly. This is the glue that binds the framework to the Flutter engine.
RenderIntrinsicHeight
Sizes its child to the child's intrinsic height.
RenderIntrinsicWidth
Sizes its child to the child's maximum intrinsic width.
RenderLeaderLayer
Provides an anchor for a RenderFollowerLayer.
RenderLimitedBox
Constrains the child's BoxConstraints.maxWidth and BoxConstraints.maxHeight if they're otherwise unconstrained.
RenderListBody
Displays its children sequentially along a given axis, forcing them to the dimensions of the parent in the other axis.
RenderListWheelViewport
Render, onto a wheel, a bigger sequential set of objects inside this viewport.
RenderMergeSemantics
Causes the semantics of all descendants to be merged into this node such that the entire subtree becomes a single leaf in the semantics tree.
RenderMetaData
Holds opaque meta data in the render tree.
RenderMouseRegion
Calls callbacks in response to pointer events that are exclusive to mice.
RenderObject
An object in the render tree.
RenderOffstage
Lays the child out as if it was in the tree, but without painting anything, without making the child available for hit testing, and without taking any room in the parent.
RenderOpacity
Makes its child partially transparent.
RenderPadding
Insets its child by the given padding.
RenderParagraph
A render object that displays a paragraph of text.
RenderPerformanceOverlay
Displays performance statistics.
RenderPhysicalModel
Creates a physical model layer that clips its child to a rounded rectangle.
RenderPhysicalShape
Creates a physical shape layer that clips its child to a Path.
RenderPointerListener
Calls callbacks in response to common pointer events.
RenderPositionedBox
Positions its child using an AlignmentGeometry.
RenderProxyBox
A base class for render boxes that resemble their children.
RenderProxyBoxWithHitTestBehavior
A RenderProxyBox subclass that allows you to customize the hit-testing behavior.
RenderProxySliver
A base class for sliver render objects that resemble their children.
RenderRepaintBoundary
Creates a separate display list for its child.
RenderRotatedBox
Rotates its child by a integral number of quarter turns.
RenderSemanticsAnnotations
Add annotations to the SemanticsNode for this subtree.
RenderSemanticsGestureHandler
Listens for the specified gestures from the semantics server (e.g. an accessibility tool).
RenderShaderMask
Applies a mask generated by a Shader to its child.
RenderShiftedBox
Abstract class for one-child-layout render boxes that provide control over the child's position.
RenderShrinkWrappingViewport
A render object that is bigger on the inside and shrink wraps its children in the main axis.
RenderSizedOverflowBox
A render object that is a specific size but passes its original constraints through to its child, which it allows to overflow.
RenderSliver
Base class for the render objects that implement scroll effects in viewports.
RenderSliverAnimatedOpacity
Makes its sliver child partially transparent, driven from an Animation.
RenderSliverBoxChildManager
A delegate used by RenderSliverMultiBoxAdaptor to manage its children.
RenderSliverConstrainedCrossAxis
Applies a cross-axis constraint to its sliver child.
RenderSliverCrossAxisGroup
A sliver that places multiple sliver children in a linear array along the cross axis.
RenderSliverEdgeInsetsPadding
Insets a RenderSliver by applying resolvedPadding on each side.
RenderSliverFillRemaining
A sliver that contains a single box child that is non-scrollable and fills the remaining space in the viewport.
RenderSliverFillRemainingAndOverscroll
A sliver that contains a single box child that is non-scrollable and fills the remaining space in the viewport including any overscrolled area.
RenderSliverFillRemainingWithScrollable
A sliver that contains a single box child that contains a scrollable and fills the viewport.
RenderSliverFillViewport
A sliver that contains multiple box children that each fill the viewport.
RenderSliverFixedExtentBoxAdaptor
A sliver that contains multiple box children that have the explicit extent in the main axis.
RenderSliverFixedExtentList
A sliver that places multiple box children with the same main axis extent in a linear array.
RenderSliverFloatingPersistentHeader
A sliver with a RenderBox child which shrinks and scrolls like a RenderSliverScrollingPersistentHeader, but immediately comes back when the user scrolls in the reverse direction.
RenderSliverFloatingPinnedPersistentHeader
A sliver with a RenderBox child which shrinks and then remains pinned to the start of the viewport like a RenderSliverPinnedPersistentHeader, but immediately grows when the user scrolls in the reverse direction.
RenderSliverGrid
A sliver that places multiple box children in a two dimensional arrangement.
RenderSliverIgnorePointer
A render object that is invisible during hit testing.
RenderSliverList
A sliver that places multiple box children in a linear array along the main axis.
RenderSliverMainAxisGroup
A sliver that places multiple sliver children in a linear array along the main axis.
RenderSliverMultiBoxAdaptor
A sliver with multiple box children.
RenderSliverOffstage
Lays the sliver child out as if it was in the tree, but without painting anything, without making the sliver child available for hit testing, and without taking any room in the parent.
RenderSliverOpacity
Makes its sliver child partially transparent.
RenderSliverPadding
Insets a RenderSliver, applying padding on each side.
RenderSliverPersistentHeader
A base class for slivers that have a RenderBox child which scrolls normally, except that when it hits the leading edge (typically the top) of the viewport, it shrinks to a minimum size (minExtent).
RenderSliverPinnedPersistentHeader
A sliver with a RenderBox child which never scrolls off the viewport in the positive scroll direction, and which first scrolls on at a full size but then shrinks as the viewport continues to scroll.
RenderSliverScrollingPersistentHeader
A sliver with a RenderBox child which scrolls normally, except that when it hits the leading edge (typically the top) of the viewport, it shrinks to a minimum size before continuing to scroll.
RenderSliverSemanticsAnnotations
Add annotations to the SemanticsNode for this subtree.
RenderSliverSingleBoxAdapter
An abstract class for RenderSlivers that contains a single RenderBox.
RenderSliverToBoxAdapter
A RenderSliver that contains a single RenderBox.
RenderSliverVariedExtentList
A sliver that places multiple box children with the corresponding main axis extent in a linear array.
RenderStack
Implements the stack layout algorithm.
RenderTable
A table where the columns and rows are sized to fit the contents of the cells.
RenderTransform
Applies a transformation before painting its child.
RenderTreeSliver
A sliver that places multiple TreeSliverNodes in a linear array along the main access, while staggering nodes that are animating into and out of view.
RenderUiKitView
A render object for an iOS UIKit UIView.
RenderView
The root of the render tree.
RenderViewport
A render object that is bigger on the inside.
RenderViewportBase<ParentDataClass extends ContainerParentDataMixin<RenderSliver>>
A base class for render objects that are bigger on the inside.
RenderWrap
Displays its children in multiple horizontal or vertical runs.
ResizeImage
Instructs Flutter to decode the image at the specified dimensions instead of at its native size.
ResizeImageKey
Key used internally by ResizeImage.
RevealedOffset
Return value for RenderAbstractViewport.getOffsetToReveal.
RoundedRectangleBorder
A rectangular border with rounded corners.
RoundedSuperellipseBorder
A rectangular border with rounded corners following the shape of an RSuperellipse.
RRect
An immutable rounded rectangle with the custom radii for all four corners.
RSTransform
A transform consisting of a translation, a rotation, and a uniform scale.
RSuperellipse
An immutable rounded superellipse.
SelectAllSelectionEvent
Selects all selectable contents.
SelectedContent
The selected content in a Selectable or SelectionHandler.
SelectedContentRange
This class stores the range information of the selection under a Selectable or SelectionHandler.
SelectionEdgeUpdateEvent
Updates a selection edge.
SelectionEvent
An abstract base class for selection events.
SelectionGeometry
The geometry of the current selection.
SelectionHandler
The abstract interface to handle SelectionEvents.
SelectionPoint
The geometry information of a selection point.
SelectionRegistrar
A registrar that keeps track of Selectables in the subtree.
SelectionUtils
A utility class that provides useful methods for handling selection events.
SelectParagraphSelectionEvent
Selects the entire paragraph at the location.
SelectWordSelectionEvent
Selects the whole word at the location.
SemanticsAction
The possible actions that can be conveyed from the operating system accessibility APIs to a semantics node.
SemanticsActionEvent
An event to request a SemanticsAction of type to be performed on the SemanticsNode identified by nodeId owned by the FlutterView identified by viewId.
SemanticsConfiguration
Describes the semantic information associated with the owning RenderObject.
SemanticsData
Summary information about a SemanticsNode object.
SemanticsEvent
An event sent by the application to notify interested listeners that something happened to the user interface (e.g. a view scrolled).
SemanticsFlag
A Boolean value that can be associated with a semantics node.
SemanticsFlags
Represents a collection of boolean flags that convey semantic information about a widget's accessibility state and properties.
SemanticsHandle
A reference to the semantics information generated by the framework.
SemanticsHintOverrides
Provides hint values which override the default hints on supported platforms.
SemanticsLabelBuilder
Builder for creating semantically correct concatenated labels with proper text direction handling and spacing.
SemanticsNode
A node that represents some semantic data.
SemanticsOwner
Owns SemanticsNode objects and notifies listeners of changes to the render tree semantics.
SemanticsProperties
Contains properties used by assistive technologies to make the application more accessible.
SemanticsService
Allows access to the platform's accessibility services.
SemanticsSortKey
Base class for all sort keys for SemanticsProperties.sortKey accessibility traversal order sorting.
SemanticsTag
A tag for a SemanticsNode.
SemanticsUpdateBuilder
An object that creates SemanticsUpdate objects.
Shader
Base class for objects such as Gradient and ImageShader which correspond to shaders as used by Paint.shader.
ShaderMaskLayer
A composited layer that applies a shader to its children.
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
SingleChildLayoutDelegate
A delegate for computing the layout of a render object with a single child.
Size
Holds a 2D floating-point size.
SliverConstraints
Immutable layout constraints for RenderSliver layout.
SliverGeometry
Describes the amount of space occupied by a RenderSliver.
SliverGridDelegate
Controls the layout of tiles in a grid.
SliverGridDelegateWithFixedCrossAxisCount
Creates grid layouts with a fixed number of tiles in the cross axis.
SliverGridDelegateWithMaxCrossAxisExtent
Creates grid layouts with tiles that each have a maximum cross-axis extent.
SliverGridGeometry
Describes the placement of a child in a RenderSliverGrid.
SliverGridLayout
The size and position of all the tiles in a RenderSliverGrid.
SliverGridParentData
Parent data structure used by RenderSliverGrid.
SliverGridRegularTileLayout
A SliverGridLayout that uses equally sized and spaced tiles.
SliverHitTestEntry
A hit test entry used by RenderSliver.
SliverHitTestResult
The result of performing a hit test on RenderSlivers.
SliverLayoutDimensions
Relates the dimensions of the RenderSliver during layout.
SliverLogicalContainerParentData
Parent data for slivers that have multiple children and that position their children using layout offsets.
SliverLogicalParentData
Parent data structure used by parents of slivers that position their children using layout offsets.
SliverMultiBoxAdaptorParentData
Parent data structure used by RenderSliverMultiBoxAdaptor.
SliverPhysicalContainerParentData
Parent data for slivers that have multiple children and that position their children using absolute coordinates.
SliverPhysicalParentData
Parent data structure used by parents of slivers that position their children using absolute coordinates.
SpellOutStringAttribute
A string attribute that causes the assistive technologies, e.g. VoiceOver, to spell out the string character by character.
StackParentData
Parent data for use with RenderStack.
StadiumBorder
A border that fits a stadium-shaped border (a box with semicircles on the ends) within the rectangle of the widget it is applied to.
StarBorder
A border that fits a star or polygon-shaped border within the rectangle of the widget it is applied to.
StringAttribute
An abstract interface for string attributes that affects how assistive technologies, e.g. VoiceOver or TalkBack, treat the text.
StringProperty
Property which encloses its string value in quotes.
StrutStyle
Defines the strut, which sets the minimum height a line can be relative to the baseline.
SweepGradient
A 2D sweep gradient.
SystemMouseCursors
A collection of system MouseCursors.
TableBorder
Border specification for Table widgets.
TableCellParentData
Parent data used by RenderTable for its children.
TableColumnWidth
Base class to describe how wide a column in a RenderTable should be.
TapSemanticEvent
An event which triggers tap semantic feedback.
TextAlignVertical
The vertical alignment of text within an input box.
TextBox
A rectangle enclosing a run of text.
TextDecoration
A linear decoration to draw near the text.
TextHeightBehavior
Defines how to apply TextStyle.height over and under text.
TextPainter
An object that paints a TextSpan tree into a Canvas.
TextParentData
Parent data used by RenderParagraph and RenderEditable to annotate inline contents (such as WidgetSpans) with.
TextPosition
A position in a string of text.
TextRange
A range of characters in a string of text.
TextScaler
A class that describes how textual contents should be scaled for better readability.
TextSelection
A range of text that represents a selection.
TextSelectionPoint
Represents the coordinates of the point in a selection, and the text direction at that point, relative to top left of the RenderEditable that holds the selection.
TextSpan
An immutable span of text.
TextStyle
An immutable style describing how to format and paint text.
TextTreeConfiguration
Configuration specifying how a particular DiagnosticsTreeStyle should be rendered as text art.
TextureBox
A rectangle upon which a backend texture is mapped.
TextureLayer
A composited layer that maps a backend texture to a rectangle.
TooltipSemanticsEvent
An event for a semantic announcement of a tooltip.
TransformLayer
A composited layer that applies a given transformation matrix to its children.
TransformProperty
Property which handles Matrix4 that represent transforms.
TreeSliverIndentationType
The style of indentation for TreeSliverNodes in a TreeSliver, as handled by RenderTreeSliver.
TreeSliverNodeParentData
Used to pass information down to RenderTreeSliver.
VerticalCaretMovementRun
The consecutive sequence of TextPositions that the caret should move to when the user navigates the paragraph using the upward arrow key or the downward arrow key.
ViewConfiguration
The layout constraints for the root render object.
ViewportOffset
Which part of the content inside the viewport should be visible.
WordBoundary
A TextBoundary subclass for locating word breaks.
WrapParentData
Parent data for use with RenderWrap.
Enums
Assertiveness
Determines the assertiveness level of the accessibility announcement.
Axis
The two cardinal directions in two dimensions.
AxisDirection
A direction along either the horizontal or vertical Axis in which the origin, or zero position, is determined.
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
CacheExtentStyle
The unit of measurement for a Viewport.cacheExtent.
Clip
Different ways to clip content.
CrossAxisAlignment
How the children should be placed along the cross axis in a flex layout.
DebugSemanticsDumpOrder
Used by debugDumpSemanticsTree to specify the order in which child nodes are printed.
DecorationPosition
Where to paint a box decoration.
DiagnosticLevel
The various priority levels used to filter which diagnostics are shown and omitted.
DiagnosticsTreeStyle
Styles for displaying a node in a DiagnosticsNode tree.
FilterQuality
Quality levels for image sampling in ImageFilter and Shader objects that sample images and for Canvas operations that render images.
FlexFit
How the child is inscribed into the available space.
FlutterLogoStyle
Possible ways to draw Flutter's logo.
FontStyle
Whether to use the italic type variation of glyphs in the font.
GrowthDirection
The direction in which a sliver's contents are ordered, relative to the scroll offset axis.
HitTestBehavior
How to behave during hit tests.
ImageRepeat
How to paint any portions of a box not covered by an image.
MainAxisAlignment
How the children should be placed along the main axis in a flex layout.
MainAxisSize
How much space should be occupied in the main axis.
OverflowBoxFit
How much space should be occupied by the OverflowBox if there is no overflow.
PaintingStyle
Strategies for painting shapes and paths on a canvas.
PathFillType
Determines the winding rule that decides how the interior of a Path is calculated.
PathOperation
Strategies for combining paths.
PerformanceOverlayOption
The options that control whether the performance overlay displays certain aspects of the compositor.
PlaceholderAlignment
Where to vertically align the placeholder relative to the surrounding text.
PlatformViewHitTestBehavior
How an embedded platform view behave during hit tests.
RenderAnimatedSizeState
A RenderAnimatedSize can be in exactly one of these states.
RenderComparison
The description of the difference between two objects, in the context of how it will affect the rendering.
RenderingServiceExtensions
Service extension constants for the rendering library.
ResizeImagePolicy
Configures the behavior for ResizeImage.
ScrollDirection
The direction of a scroll, relative to the positive scroll offset axis given by an AxisDirection and a GrowthDirection.
SelectionEventType
The type of a SelectionEvent.
SelectionExtendDirection
The direction to extend a selection.
SelectionResult
The result after handling a SelectionEvent.
SelectionStatus
The status that indicates whether there is a selection and whether the selection is collapsed.
SemanticsRole
An enum to describe the role for a semantics node.
SemanticsValidationResult
The validation result of a form field.
SliverPaintOrder
Specifies an order in which to paint the slivers of a Viewport.
StackFit
How to size the non-positioned children of a Stack.
StrokeCap
Styles to use for line endings.
StrokeJoin
Styles to use for line segment joins.
TableCellVerticalAlignment
Vertical alignment options for cells in RenderTable objects.
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
TextGranularity
The unit of how selection handles move in text.
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
VertexMode
Defines how a list of points is interpreted when drawing a set of triangles.
VerticalDirection
A direction in which boxes flow vertically.
WebHtmlElementStrategy
The strategy for Image.network and NetworkImage to decide whether to display images in HTML elements contained in a platform view instead of fetching bytes.
WrapAlignment
How Wrap should align objects.
WrapCrossAlignment
Who Wrap should align children within a run in the cross axis.
Mixins
ContainerParentDataMixin<ChildType extends RenderObject>
Parent data to support a doubly-linked list of children.
ContainerRenderObjectMixin<ChildType extends RenderObject, ParentDataType extends ContainerParentDataMixin<ChildType>>
Generic mixin for render objects with a list of children.
DebugOverflowIndicatorMixin
An mixin indicator that is drawn when a RenderObject overflows its container.
KeepAliveParentDataMixin
Parent data structure used by RenderSliverWithKeepAliveMixin.
PaintingBinding
Binding for the painting library.
RelayoutWhenSystemFontsChangeMixin
Mixin for RenderObject that will call systemFontsDidChange whenever the system fonts change.
RenderAnimatedOpacityMixin<T extends RenderObject>
Implementation of RenderAnimatedOpacity and RenderSliverAnimatedOpacity.
RenderBoxContainerDefaultsMixin<ChildType extends RenderBox, ParentDataType extends ContainerBoxParentData<ChildType>>
A mixin that provides useful default behaviors for boxes with children managed by the ContainerRenderObjectMixin mixin.
RendererBinding
The glue between the render trees and the Flutter engine.
RenderInlineChildrenContainerDefaults
A mixin that provides useful default behaviors for text RenderBoxes (RenderParagraph and RenderEditable for example) with inline content children managed by the ContainerRenderObjectMixin mixin.
RenderObjectWithChildMixin<ChildType extends RenderObject>
Generic mixin for render objects with one child.
RenderObjectWithLayoutCallbackMixin
A mixin for managing RenderObject with a layoutCallback, which will be invoked during this RenderObject's layout process if scheduled using scheduleLayoutCallback.
RenderProxyBoxMixin<T extends RenderBox>
Implementation of RenderProxyBox.
RenderSliverHelpers
Mixin for RenderSliver subclasses that provides some utility functions.
RenderSliverWithKeepAliveMixin
This class exists to dissociate KeepAlive from RenderSliverMultiBoxAdaptor.
Selectable
A mixin that can be selected by users when under a SelectionArea widget.
SelectionRegistrant
A mixin to auto-register the mixer to the registrar.
SemanticsAnnotationsMixin
A mixin for RenderObjects that want to annotate the SemanticsNode for their subtree.
SemanticsBinding
The glue between the semantics layer and the Flutter engine.
Extension Types
BaselineOffset
A wrapper that represents the baseline location of a RenderBox.
Constants
kDefaultFontSize → const double
The default font size if none is specified.
kTextHeightNone → const double
A TextStyle.height value that indicates the text span should take the height defined by the font, which may not be exactly the height of TextStyle.fontSize.
Properties
debugCaptureShaderWarmUpImage ↔ ShaderWarmUpImageCallback
Called by ShaderWarmUp.execute immediately after it creates an Image.
getter/setter pair
debugCaptureShaderWarmUpPicture ↔ ShaderWarmUpPictureCallback
Called by ShaderWarmUp.execute immediately after it creates a Picture.
getter/setter pair
debugCheckIntrinsicSizes ↔ bool
Check the intrinsic sizes of each RenderBox during layout.
getter/setter pair
debugCurrentRepaintColor ↔ HSVColor
The current color to overlay when repainting a layer.
getter/setter pair
debugDisableClipLayers ↔ bool
Setting to true will cause all clipping effects from the layer tree to be ignored.
getter/setter pair
debugDisableOpacityLayers ↔ bool
Setting to true will cause all opacity effects from the layer tree to be ignored.
getter/setter pair
debugDisablePhysicalShapeLayers ↔ bool
Setting to true will cause all physical modeling effects from the layer tree, such as shadows from elevations, to be ignored.
getter/setter pair
debugDisableShadows ↔ bool
Whether to replace all shadows with solid color blocks.
getter/setter pair
debugEnhanceLayoutTimelineArguments ↔ bool
Adds debugging information to Timeline events related to RenderObject layouts.
getter/setter pair
debugEnhancePaintTimelineArguments ↔ bool
Adds debugging information to Timeline events related to RenderObject paints.
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
debugOnProfilePaint ↔ ProfilePaintCallback?
Callback invoked for every RenderObject painted each frame.
getter/setter pair
debugPaintBaselinesEnabled ↔ bool
Causes each RenderBox to paint a line at each of its baselines.
getter/setter pair
debugPaintLayerBordersEnabled ↔ bool
Causes each Layer to paint a box around its bounds.
getter/setter pair
debugPaintPointersEnabled ↔ bool
Causes objects like RenderPointerListener to flash while they are being tapped. This can be useful to see how large the hit box is, e.g. when debugging buttons that are harder to hit than expected.
getter/setter pair
debugPaintSizeEnabled ↔ bool
Causes each RenderBox to paint a box around its bounds, and some extra boxes, such as RenderPadding, to draw construction lines.
getter/setter pair
debugPrint ↔ DebugPrintCallback
Prints a message to the console, which you can access using the "flutter" tool's "logs" command ("flutter logs").
getter/setter pair
debugPrintLayouts ↔ bool
Log the dirty render objects that are laid out each frame.
getter/setter pair
debugPrintMarkNeedsLayoutStacks ↔ bool
Log the call stacks that mark render objects as needing layout.
getter/setter pair
debugPrintMarkNeedsPaintStacks ↔ bool
Log the call stacks that mark render objects as needing paint.
getter/setter pair
debugProfileLayoutsEnabled ↔ bool
Adds Timeline events for every RenderObject layout.
getter/setter pair
debugProfilePaintsEnabled ↔ bool
Adds Timeline events for every RenderObject painted.
getter/setter pair
debugRepaintRainbowEnabled ↔ bool
Overlay a rotating set of colors when repainting layers in debug mode.
getter/setter pair
debugRepaintTextRainbowEnabled ↔ bool
Overlay a rotating set of colors when repainting text in debug mode.
getter/setter pair
debugSemanticsDisableAnimations ↔ bool?
Overrides the setting of SemanticsBinding.disableAnimations for debugging and testing.
getter/setter pair
imageCache → ImageCache
The singleton that implements the Flutter framework's image cache.
no setter
Functions
applyBoxFit(BoxFit fit, Size inputSize, Size outputSize) → FittedSizes
Apply a BoxFit value.
applyGrowthDirectionToAxisDirection(AxisDirection axisDirection, GrowthDirection growthDirection) → AxisDirection
Flips the AxisDirection if the GrowthDirection is GrowthDirection.reverse.
applyGrowthDirectionToScrollDirection(ScrollDirection scrollDirection, GrowthDirection growthDirection) → ScrollDirection
Flips the ScrollDirection if the GrowthDirection is GrowthDirection.reverse.
axisDirectionIsReversed(AxisDirection axisDirection) → bool
Returns whether traveling along the given axis direction visits coordinates along that axis in numerically decreasing order.
axisDirectionToAxis(AxisDirection axisDirection) → Axis
Returns the Axis that contains the given AxisDirection.
combineSemanticsInfo(List<InlineSpanSemanticsInformation> infoList) → List<InlineSpanSemanticsInformation>
Combines _semanticsInfo entries where permissible.
debugAssertAllPaintingVarsUnset(String reason, {bool debugDisableShadowsOverride = false}) → bool
Returns true if none of the painting library debug variables have been changed.
debugAssertAllRenderVarsUnset(String reason, {bool debugCheckIntrinsicSizesOverride = false}) → bool
Returns true if none of the rendering library debug variables have been changed.
debugCheckCanResolveTextDirection(TextDirection? direction, String target) → bool
Asserts that a given TextDirection is not null.
debugCheckHasBoundedAxis(Axis axis, BoxConstraints constraints) → bool
Returns true if the given Axis is bounded within the given BoxConstraints in both the main and cross axis, throwing an exception otherwise.
debugDescribeTransform(Matrix4? transform) → List<String>
Returns a list of strings representing the given transform in a format useful for TransformProperty.
debugDumpLayerTree() → void
Prints a textual representation of the layer trees.
debugDumpPipelineOwnerTree() → void
Prints a textual representation of the PipelineOwner tree rooted at RendererBinding.rootPipelineOwner.
debugDumpRenderObjectSemanticsTree() → void
Dumps the render object semantics tree.
debugDumpRenderTree() → void
Prints a textual representation of the render trees.
debugDumpSemanticsTree([DebugSemanticsDumpOrder childOrder = DebugSemanticsDumpOrder.traversalOrder]) → void
Prints a textual representation of the semantics trees.
debugFlushLastFrameImageSizeInfo() → void
Flushes inter-frame tracking of image size information from paintImage.
debugPaintPadding(Canvas canvas, Rect outerRect, Rect? innerRect, {double outlineWidth = 2.0}) → void
Paint a diagram showing the given area as padding.
debugResetSemanticsIdCounter() → void
In tests use this function to reset the counter used to generate SemanticsNode.id.
decodeImageFromList(Uint8List bytes) → Future<Image>
Creates an image from a list of bytes.
flipAxis(Axis direction) → Axis
Returns the opposite of the given Axis.
flipAxisDirection(AxisDirection axisDirection) → AxisDirection
Returns the opposite of the given AxisDirection.
flipScrollDirection(ScrollDirection direction) → ScrollDirection
Returns the opposite of the given ScrollDirection.
lerpFontVariations(List<FontVariation>? a, List<FontVariation>? b, double t) → List<FontVariation>?
Interpolate between two lists of FontVariation objects.
paintBorder(Canvas canvas, Rect rect, {BorderSide top = BorderSide.none, BorderSide right = BorderSide.none, BorderSide bottom = BorderSide.none, BorderSide left = BorderSide.none}) → void
Paints a border around the given rectangle on the canvas.
paintImage({required Canvas canvas, required Rect rect, required Image image, String? debugImageLabel, double scale = 1.0, double opacity = 1.0, ColorFilter? colorFilter, BoxFit? fit, Alignment alignment = Alignment.center, Rect? centerSlice, ImageRepeat repeat = ImageRepeat.noRepeat, bool flipHorizontally = false, bool invertColors = false, FilterQuality filterQuality = FilterQuality.medium, bool isAntiAlias = false, BlendMode blendMode = BlendMode.srcOver}) → void
Paints an image into the given rectangle on the canvas.
paintZigZag(Canvas canvas, Paint paint, Offset start, Offset end, int zigs, double width) → void
Draw a line between two points, which cuts diagonally back and forth across the line that connects the two points.
positionDependentBox({required Size size, required Size childSize, required Offset target, required bool preferBelow, double verticalOffset = 0.0, double margin = 10.0}) → Offset
Position a child box within a container box, either above or below a target point.
textDirectionToAxisDirection(TextDirection textDirection) → AxisDirection
Returns the AxisDirection in which reading occurs in the given TextDirection.
Typedefs
BoxConstraintsTransform = BoxConstraints Function(BoxConstraints constraints)
Signature for a function that transforms a BoxConstraints to another BoxConstraints.
BoxHitTest = bool Function(BoxHitTestResult result, Offset position)
Method signature for hit testing a RenderBox.
BoxHitTestWithOutOfBandPosition = bool Function(BoxHitTestResult result)
Method signature for hit testing a RenderBox with a manually managed position (one that is passed out-of-band).
ChildBaselineGetter = double? Function(RenderBox child, BoxConstraints constraints, TextBaseline baseline)
Signature for a function that takes a RenderBox and returns the baseline offset this RenderBox would have if it were laid out with the given BoxConstraints.
ChildLayouter = Size Function(RenderBox child, BoxConstraints constraints)
Signature for a function that takes a RenderBox and returns the Size that the RenderBox would have if it were laid out with the given BoxConstraints.
ChildSemanticsConfigurationsDelegate = ChildSemanticsConfigurationsResult Function(List<SemanticsConfiguration>)
Signature for the SemanticsConfiguration.childConfigurationsDelegate.
CompositionCallback = void Function(Layer layer)
The signature of the callback added in Layer.addCompositionCallback.
DebugPaintCallback = void Function(PaintingContext context, Offset offset, RenderView renderView)
A callback for painting a debug overlay on top of the provided RenderView.
DecoderBufferCallback = Future<Codec> Function(ImmutableBuffer buffer, {bool allowUpscaling, int? cacheHeight, int? cacheWidth})
Performs the decode process for use in ImageProvider.loadBuffer.
HttpClientProvider = HttpClient Function()
Signature for a method that returns an HttpClient.
ImageChunkListener = void Function(ImageChunkEvent event)
Signature for listening to ImageChunkEvent events.
ImageDecoderCallback = Future<Codec> Function(ImmutableBuffer buffer, {TargetImageSizeCallback? getTargetSize})
Performs the decode process for use in ImageProvider.loadImage.
ImageErrorListener = void Function(Object exception, StackTrace? stackTrace)
Signature for reporting errors when resolving images.
ImageListener = void Function(ImageInfo image, bool synchronousCall)
Signature for callbacks reporting that an image is available.
InformationCollector = Iterable<DiagnosticsNode> Function()
Signature for FlutterErrorDetails.informationCollector callback and other callbacks that collect information describing an error.
InlineSpanVisitor = bool Function(InlineSpan span)
Called on each span as InlineSpan.visitChildren walks the InlineSpan tree.
ItemExtentBuilder = double? Function(int index, SliverLayoutDimensions dimensions)
Called to get the item extent by the index of item.
LayoutCallback<T extends Constraints> = void Function(T constraints)
Signature for a function that is called during layout.
MouseTrackerHitTest = HitTestResult Function(Offset offset, int viewId)
Signature for hit testing at the given offset for the specified view.
MoveCursorHandler = void Function(bool extendSelection)
Signature for SemanticsActions that move the cursor.
PaintImageCallback = void Function(ImageSizeInfo info)
Called when the framework is about to paint an Image to a Canvas with an ImageSizeInfo that contains the decoded size of the image as well as its output size.
PaintingContextCallback = void Function(PaintingContext context, Offset offset)
Signature for painting into a PaintingContext.
PipelineOwnerVisitor = void Function(PipelineOwner child)
Signature for the callback to PipelineOwner.visitChildren.
PointerCancelEventListener = void Function(PointerCancelEvent event)
Signature for listening to PointerCancelEvent events.
PointerDownEventListener = void Function(PointerDownEvent event)
Signature for listening to PointerDownEvent events.
PointerMoveEventListener = void Function(PointerMoveEvent event)
Signature for listening to PointerMoveEvent events.
PointerPanZoomEndEventListener = void Function(PointerPanZoomEndEvent event)
Signature for listening to PointerPanZoomEndEvent events.
PointerPanZoomStartEventListener = void Function(PointerPanZoomStartEvent event)
Signature for listening to PointerPanZoomStartEvent events.
PointerPanZoomUpdateEventListener = void Function(PointerPanZoomUpdateEvent event)
Signature for listening to PointerPanZoomUpdateEvent events.
PointerSignalEventListener = void Function(PointerSignalEvent event)
Signature for listening to PointerSignalEvent events.
PointerUpEventListener = void Function(PointerUpEvent event)
Signature for listening to PointerUpEvent events.
ProfilePaintCallback = void Function(RenderObject renderObject)
Signature for debugOnProfilePaint implementations.
RenderObjectVisitor = void Function(RenderObject child)
Signature for a function that is called for each RenderObject.
ScrollToOffsetHandler = void Function(Offset targetOffset)
Signature for the SemanticsAction.scrollToOffset handlers to scroll the scrollable container to the given targetOffset.
SemanticsActionHandler = void Function(Object? args)
Signature for a handler of a SemanticsAction.
SemanticsBuilderCallback = List<CustomPainterSemantics> Function(Size size)
Signature of the function returned by CustomPainter.semanticsBuilder.
SemanticsNodeVisitor = bool Function(SemanticsNode node)
Signature for a function that is called for each SemanticsNode.
SemanticsUpdateCallback = void Function(SemanticsUpdate update)
Signature for a function that receives a semantics update and returns no result.
SetSelectionHandler = void Function(TextSelection selection)
Signature for the SemanticsAction.setSelection handlers to change the text selection (or re-position the cursor) to selection.
SetTextHandler = void Function(String text)
Signature for the SemanticsAction.setText handlers to replace the current text with the input text.
ShaderCallback = Shader Function(Rect bounds)
Signature for a function that creates a Shader for a given Rect.
ShaderWarmUpImageCallback = bool Function(Image image)
The signature of debugCaptureShaderWarmUpImage.
ShaderWarmUpPictureCallback = bool Function(Picture picture)
The signature of debugCaptureShaderWarmUpPicture.
SliverHitTest = bool Function(SliverHitTestResult result, {required double crossAxisPosition, required double mainAxisPosition})
Method signature for hit testing a RenderSliver.
TreeSliverNodesAnimation = ({int fromIndex, int toIndex, double value})
Represents the animation of the children of a parent TreeSliverNode that are animating into or out of view.
ValueChanged<T> = void Function(T value)
Signature for callbacks that report that an underlying value has changed.
ValueGetter<T> = T Function()
Signature for callbacks that are to report a value on demand.
ValueSetter<T> = void Function(T value)
Signature for callbacks that report that a value has been set.
VoidCallback = void Function()
Signature of callbacks that have no arguments and return no data.
Exceptions / Errors
FlutterError
Error class used to report Flutter-specific assertion failures and contract violations.
NetworkImageLoadException
The exception thrown when the HTTP request to load a network image fails.