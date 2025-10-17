painting library
The Flutter painting library.

To use, import package:flutter/painting.dart.

This library includes a variety of classes that wrap the Flutter engine's painting API for more specialized purposes, such as painting scaled images, interpolating between shadows, painting borders around boxes, etc.

In particular:

Use the TextPainter class for painting text.
Use Decoration (and more concretely BoxDecoration) for painting boxes.
Classes
Accumulator
Mutable wrapper of an integer that can be passed by reference to track a value across a recursive stack.
Alignment
A point within a rectangle.
AlignmentDirectional
An offset that's expressed as a fraction of a Size, but whose horizontal component is dependent on the writing direction.
AlignmentGeometry
Base class for Alignment that allows for text-direction aware resolution.
AssetBundleImageKey
Key for the image obtained by an AssetImage or ExactAssetImage.
AssetBundleImageProvider
A subclass of ImageProvider that knows about AssetBundles.
AssetImage
Fetches an image from an AssetBundle, having determined the exact image to use based on the context.
AutomaticNotchedShape
A NotchedShape created from ShapeBorders.
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
BoxDecoration
An immutable description of how to paint a box.
BoxPainter
A stateful class that can paint a particular Decoration.
BoxShadow
A shadow cast by a box.
Canvas
An interface for recording graphical operations.
CircleBorder
A border that fits a circle within the available space.
CircularNotchedRectangle
A rectangle with a smooth circular notch.
ClipContext
Clip utilities used by PaintingContext.
Color
An immutable color value in ARGB format.
ColorFilter
A description of a color filter to apply when drawing a shape or compositing a layer with a particular Paint. A color filter is a function that takes two colors, and outputs one color. When applied during compositing, it is independently applied to each pixel of the layer being drawn before the entire layer is merged with the destination.
ColorProperty
DiagnosticsProperty that has an Color as value.
ColorSwatch<T>
A color that has a small table of related colors called a "swatch".
ContinuousRectangleBorder
A rectangular border with smooth continuous transitions between the straight sides and the rounded corners.
Decoration
A description of a box decoration (a decoration applied to a Rect).
DecorationImage
An image for a box decoration.
DecorationImagePainter
The painter for a DecorationImage.
EdgeInsets
An immutable set of offsets in each of the four cardinal directions.
EdgeInsetsDirectional
An immutable set of offsets in each of the four cardinal directions, but whose horizontal components are dependent on the writing direction.
EdgeInsetsGeometry
Base class for EdgeInsets that allows for text-direction aware resolution.
ExactAssetImage
Fetches an image from an AssetBundle, associating it with the given scale.
FileImage
Decodes the given File object as an image, associating it with the given scale.
FittedSizes
The pair of sizes returned by applyBoxFit.
FlutterLogoDecoration
An immutable description of how to paint Flutter's logo.
FontFeature
A feature tag and value that affect the selection of glyphs in a font.
FontVariation
An axis tag and value that can be used to customize variable fonts.
FontWeight
The thickness of the glyphs used to draw the text.
FractionalOffset
An offset that's expressed as a fraction of a Size.
GlyphInfo
The measurements of a character (or a sequence of visually connected characters) within a paragraph.
Gradient
A 2D gradient.
GradientRotation
A GradientTransform that rotates the gradient around the center-point of its bounding box.
GradientTransform
Base class for transforming gradient shaders without applying the same transform to the entire canvas.
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
LinearBorder
An OutlinedBorder like BoxBorder that allows one to define a rectangular (box) border in terms of zero to four LinearBorderEdges, each of which is rendered as a single line.
LinearBorderEdge
Defines the relative size and alignment of one LinearBorder edge.
LinearGradient
A 2D linear gradient.
LineMetrics
LineMetrics stores the measurements and statistics of a single line in the paragraph.
Locale
An identifier used to select a user's language and formatting preferences.
MaskFilter
A mask filter to apply to shapes as they are painted. A mask filter is a function that takes a bitmap of color pixels, and returns another bitmap of color pixels.
MatrixUtils
Utility functions for working with matrices.
MemoryImage
Decodes the given Uint8List buffer as an image, associating it with the given scale.
MultiFrameImageStreamCompleter
Manages the decoding and scheduling of image frames.
NetworkImage
Fetches the given URL from the network, associating it with the given scale.
NotchedShape
A shape with a notch in its outline.
Offset
An immutable 2D floating-point offset.
OneFrameImageStreamCompleter
Manages the loading of dart:ui.Image objects for static ImageStreams (those with only one frame).
OutlinedBorder
A ShapeBorder that draws an outline with the width and color specified by side.
OvalBorder
A border that fits an elliptical shape.
Paint
A description of the style to use when drawing on a Canvas.
Path
A complex, one-dimensional subset of a plane.
PlaceholderDimensions
Holds the Size and baseline required to represent the dimensions of a placeholder in text.
PlaceholderSpan
An immutable placeholder that is embedded inline within text.
RadialGradient
A 2D radial gradient.
Radius
A radius for either circular or elliptical shapes.
Rect
An immutable, 2D, axis-aligned, floating-point rectangle whose coordinates are relative to a given origin.
ResizeImage
Instructs Flutter to decode the image at the specified dimensions instead of at its native size.
ResizeImageKey
Key used internally by ResizeImage.
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
Shader
Base class for objects such as Gradient and ImageShader which correspond to shaders as used by Paint.shader.
ShaderWarmUp
Interface for drawing an image to warm up Skia shader compilations.
Shadow
A single shadow.
ShapeBorder
Base class for shape outlines.
ShapeDecoration
An immutable description of how to paint an arbitrary shape.
Size
Holds a 2D floating-point size.
StadiumBorder
A border that fits a stadium-shaped border (a box with semicircles on the ends) within the rectangle of the widget it is applied to.
StarBorder
A border that fits a star or polygon-shaped border within the rectangle of the widget it is applied to.
StrutStyle
Defines the strut, which sets the minimum height a line can be relative to the baseline.
SweepGradient
A 2D sweep gradient.
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
TextPosition
A position in a string of text.
TextRange
A range of characters in a string of text.
TextScaler
A class that describes how textual contents should be scaled for better readability.
TextSelection
A range of text that represents a selection.
TextSpan
An immutable span of text.
TextStyle
An immutable style describing how to format and paint text.
TransformProperty
Property which handles Matrix4 that represent transforms.
WordBoundary
A TextBoundary subclass for locating word breaks.
Enums
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
Clip
Different ways to clip content.
FilterQuality
Quality levels for image sampling in ImageFilter and Shader objects that sample images and for Canvas operations that render images.
FlutterLogoStyle
Possible ways to draw Flutter's logo.
FontStyle
Whether to use the italic type variation of glyphs in the font.
ImageRepeat
How to paint any portions of a box not covered by an image.
PaintingStyle
Strategies for painting shapes and paths on a canvas.
PathFillType
Determines the winding rule that decides how the interior of a Path is calculated.
PathOperation
Strategies for combining paths.
PlaceholderAlignment
Where to vertically align the placeholder relative to the surrounding text.
RenderComparison
The description of the difference between two objects, in the context of how it will affect the rendering.
ResizeImagePolicy
Configures the behavior for ResizeImage.
StrokeCap
Styles to use for line endings.
StrokeJoin
Styles to use for line segment joins.
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
Mixins
PaintingBinding
Binding for the painting library.
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
debugDisableShadows ↔ bool
Whether to replace all shadows with solid color blocks.
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
imageCache → ImageCache
The singleton that implements the Flutter framework's image cache.
no setter
Functions
applyBoxFit(BoxFit fit, Size inputSize, Size outputSize) → FittedSizes
Apply a BoxFit value.
axisDirectionIsReversed(AxisDirection axisDirection) → bool
Returns whether traveling along the given axis direction visits coordinates along that axis in numerically decreasing order.
axisDirectionToAxis(AxisDirection axisDirection) → Axis
Returns the Axis that contains the given AxisDirection.
combineSemanticsInfo(List<InlineSpanSemanticsInformation> infoList) → List<InlineSpanSemanticsInformation>
Combines _semanticsInfo entries where permissible.
debugAssertAllPaintingVarsUnset(String reason, {bool debugDisableShadowsOverride = false}) → bool
Returns true if none of the painting library debug variables have been changed.
debugCheckCanResolveTextDirection(TextDirection? direction, String target) → bool
Asserts that a given TextDirection is not null.
debugDescribeTransform(Matrix4? transform) → List<String>
Returns a list of strings representing the given transform in a format useful for TransformProperty.
debugFlushLastFrameImageSizeInfo() → void
Flushes inter-frame tracking of image size information from paintImage.
decodeImageFromList(Uint8List bytes) → Future<Image>
Creates an image from a list of bytes.
flipAxis(Axis direction) → Axis
Returns the opposite of the given Axis.
flipAxisDirection(AxisDirection axisDirection) → AxisDirection
Returns the opposite of the given AxisDirection.
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
InlineSpanVisitor = bool Function(InlineSpan span)
Called on each span as InlineSpan.visitChildren walks the InlineSpan tree.
PaintImageCallback = void Function(ImageSizeInfo info)
Called when the framework is about to paint an Image to a Canvas with an ImageSizeInfo that contains the decoded size of the image as well as its output size.
ShaderWarmUpImageCallback = bool Function(Image image)
The signature of debugCaptureShaderWarmUpImage.
ShaderWarmUpPictureCallback = bool Function(Picture picture)
The signature of debugCaptureShaderWarmUpPicture.
VoidCallback = void Function()
Signature of callbacks that have no arguments and return no data.
Exceptions / Errors
NetworkImageLoadException
The exception thrown when the HTTP request to load a network image fails.