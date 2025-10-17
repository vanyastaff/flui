foundation library
Core Flutter framework primitives.

The features defined in this library are the lowest-level utility classes and functions used by all the other layers of the Flutter framework.

Classes
AbstractNode
Deprecated. Unused by the framework and will be removed in a future version of Flutter. If needed, inline any required functionality of this class directly in the subclass.
AggregatedTimedBlock
Aggregates multiple TimedBlock objects that share a name.
AggregatedTimings
Provides aggregated results for timings collected by FlutterTimeline.
BindingBase
Base class for mixins that provide singleton services.
BitField<T extends dynamic>
A BitField over an enum (or other class whose values implement "index"). Only the first 62 values of the enum can be used as indices.
ByteData
A fixed-length, random-access sequence of bytes that also provides random and unaligned access to the fixed-width integers and floating point numbers represented by those bytes.
CachingIterable<E>
A lazy caching version of Iterable.
Category
A category with which to annotate a class, for documentation purposes.
ChangeNotifier
A class that can be extended or mixed in that provides a change notification API using VoidCallback for notifications.
DiagnosticableNode<T extends Diagnosticable>
DiagnosticsNode that lazily calls the associated Diagnosticable value to implement getChildren and getProperties.
DiagnosticableTree
A base class for providing string and DiagnosticsNode debug representations describing the properties and children of an object.
DiagnosticableTreeNode
DiagnosticsNode for an instance of DiagnosticableTree.
DiagnosticPropertiesBuilder
Builder to accumulate properties and configuration used to assemble a DiagnosticsNode from a Diagnosticable object.
DiagnosticsBlock
DiagnosticsNode that exists mainly to provide a container for other diagnostics that typically lacks a meaningful value of its own.
DiagnosticsNode
Defines diagnostics data for a value.
DiagnosticsProperty<T>
Property with a value of type T.
DiagnosticsSerializationDelegate
A delegate that configures how a hierarchy of DiagnosticsNodes should be serialized.
DiagnosticsStackTrace
Diagnostic with a StackTrace value suitable for displaying stack traces as part of a FlutterError object.
DocumentationIcon
A class annotation to provide a URL to an image that represents the class.
DoubleProperty
Property describing a double value with an optional unit of measurement.
Endian
Endianness of number representation.
EnumProperty<T extends Enum?>
DiagnosticsProperty that has an Enum as value.
ErrorDescription
An explanation of the problem and its cause, any information that may help track down the problem, background information, etc.
ErrorHint
An ErrorHint provides specific, non-obvious advice that may be applicable.
ErrorSpacer
An ErrorSpacer creates an empty DiagnosticsNode, that can be used to tune the spacing between other DiagnosticsNode objects.
ErrorSummary
A short (one line) description of the problem that was detected.
Factory<T>
A factory interface that also reports the type of the created objects.
FlagProperty
Property where the description is either ifTrue or ifFalse depending on whether value is true or false.
FlagsSummary<T>
A summary of multiple properties, indicating whether each of them is present (non-null) or absent (null).
Float32List
A fixed-length list of IEEE 754 single-precision binary floating-point numbers that is viewable as a TypedData.
Float64List
A fixed-length list of IEEE 754 double-precision binary floating-point numbers that is viewable as a TypedData.
FlutterErrorDetails
Class for information provided to FlutterExceptionHandler callbacks.
FlutterMemoryAllocations
An interface for listening to object lifecycle events.
FlutterTimeline
Measures how long blocks of code take to run.
HashedObserverList<T>
A list optimized for the observer pattern, but for larger numbers of observers.
HttpClientResponse
HTTP response for a client connection.
Int32List
A fixed-length list of 32-bit signed integers that is viewable as a TypedData.
Int64List
A fixed-length list of 64-bit signed integers that is viewable as a TypedData.
IntProperty
An int valued property with an optional unit the value is measured in.
IterableProperty<T>
Property with an Iterable<T> value that can be displayed with different DiagnosticsTreeStyle for custom rendering.
Key
A Key is an identifier for Widgets, Elements and SemanticsNodes.
LicenseEntry
A license that covers part of the application's software or assets, to show in an interface such as the LicensePage.
LicenseEntryWithLineBreaks
Variant of LicenseEntry for licenses that separate paragraphs with blank lines and that hard-wrap text within paragraphs. Lines that begin with one or more space characters are also assumed to introduce new paragraphs, unless they start with the same number of spaces as the previous line, in which case it's assumed they are a continuation of an indented paragraph.
LicenseParagraph
A string that represents one paragraph in a LicenseEntry.
LicenseRegistry
A registry for packages to add licenses to, so that they can be displayed together in an interface such as the LicensePage.
Listenable
An object that maintains a list of listeners.
LocalKey
A key that is not a GlobalKey.
MessageProperty
Debugging message displayed like a property.
ObjectCreated
An event that describes creation of an object.
ObjectDisposed
An event that describes disposal of an object.
ObjectEvent
A lifecycle event of an object.
ObjectFlagProperty<T>
A property where the important diagnostic information is primarily whether the value is present (non-null) or absent (null), rather than the actual value of the property itself.
ObserverList<T>
A list optimized for the observer pattern when there are small numbers of observers.
PartialStackFrame
Partial information from a stack frame for stack filtering purposes.
PercentProperty
Property which clamps a double to between 0 and 1 and formats it as a percentage.
PersistentHashMap<K extends Object, V>
A collection of key/value pairs which provides efficient retrieval of value by key.
PlatformDispatcher
Platform event dispatcher singleton.
ReadBuffer
Read-only buffer for reading sequentially from a ByteData instance.
RepetitiveStackFrameFilter
A StackFilter that filters based on repeating lists of PartialStackFrames.
SingletonFlutterWindow
Deprecated. Will be removed in a future version of Flutter.
StackFilter
A class that filters stack frames for additional filtering on FlutterError.defaultStackFilter.
StackFrame
A object representation of a frame from a stack trace.
StringProperty
Property which encloses its string value in quotes.
Summary
An annotation that provides a short description of a class for use in an index.
SynchronousFuture<T>
A Future whose then implementation calls the callback immediately.
TextTreeConfiguration
Configuration specifying how a particular DiagnosticsTreeStyle should be rendered as text art.
TextTreeRenderer
Renderer that creates ASCII art representations of trees of DiagnosticsNode objects.
TimedBlock
Provides start, end, and duration of a named block of code, timed by FlutterTimeline.
Uint8List
A fixed-length list of 8-bit unsigned integers.
Unicode
Constants for useful Unicode characters.
UniqueKey
A key that is only equal to itself.
ValueKey<T>
A key that uses a value of a particular type to identify itself.
ValueListenable<T>
An interface for subclasses of Listenable that expose a value.
ValueNotifier<T>
A ChangeNotifier that holds a single value.
WriteBuffer
Write-only buffer for incrementally building a ByteData instance.
Enums
Brightness
Describes the contrast of a theme or color palette.
DiagnosticLevel
The various priority levels used to filter which diagnostics are shown and omitted.
DiagnosticsTreeStyle
Styles for displaying a node in a DiagnosticsNode tree.
FoundationServiceExtensions
Service extension constants for the foundation library.
TargetPlatform
The platform that user interaction should adapt to target.
Mixins
Diagnosticable
A mixin class for providing string and DiagnosticsNode debug representations describing the properties of an object.
DiagnosticableTreeMixin
A mixin that helps dump string and DiagnosticsNode representations of trees.
Constants
factory → const _Factory
Used to annotate an instance or static method m. Indicates that m must either be abstract or must return a newly allocated object or null. In addition, every method that either implements or overrides m is implicitly annotated with this same annotation.
immutable → const Immutable
Used to annotate a class C. Indicates that C and all subtypes of C must be immutable.
internal → const _Internal
Used to annotate a declaration which should only be used from within the package in which it is declared, and which should not be exposed from said package's public API.
kDebugMode → const bool
A constant that is true if the application was compiled in debug mode.
kFlutterMemoryAllocationsEnabled → const bool
If true, Flutter objects dispatch the memory allocation events.
kIsWasm → const bool
A constant that is true if the application was compiled to WebAssembly.
kIsWeb → const bool
A constant that is true if the application was compiled to run on the web.
kMaxUnsignedSMI → const int
The largest SMI value.
kNoDefaultValue → const Object
Marker object indicating that a DiagnosticsNode has no default value.
kProfileMode → const bool
A constant that is true if the application was compiled in profile mode.
kReleaseMode → const bool
A constant that is true if the application was compiled in release mode.
mustCallSuper → const _MustCallSuper
Used to annotate an instance member (method, getter, setter, operator, or field) m. Indicates that every invocation of a member that overrides m must also invoke m. In addition, every method that overrides m is implicitly annotated with this same annotation.
nonVirtual → const _NonVirtual
Used to annotate an instance member (method, getter, setter, operator, or field) m in a class C or mixin M. Indicates that m should not be overridden in any classes that extend or mixin C or M.
optionalTypeArgs → const _OptionalTypeArgs
Used to annotate a class, mixin, extension, function, method, or typedef declaration C. Indicates that any type arguments declared on C are to be treated as optional.
precisionErrorTolerance → const double
The epsilon of tolerable double precision error.
protected → const _Protected
Used to annotate an instance member in a class or mixin which is meant to be visible only within the declaring library, and to other instance members of the class or mixin, and their subtypes.
required → const Required
Used to annotate a named parameter p in a method or function f. Indicates that every invocation of f must include an argument corresponding to p, despite the fact that p would otherwise be an optional parameter.
visibleForOverriding → const _VisibleForOverriding
Used to annotate an instance member that was made public so that it could be overridden but that is not intended to be referenced from outside the defining library.
visibleForTesting → const _VisibleForTesting
Used to annotate a declaration that was made public, so that it is more visible than otherwise necessary, to make code testable.
Properties
activeDevToolsServerAddress ↔ String?
The address for the active DevTools server used for debugging this application.
getter/setter pair
connectedVmServiceUri ↔ String?
The uri for the connected vm service protocol.
getter/setter pair
dashedTextConfiguration → TextTreeConfiguration
Identical to sparseTextConfiguration except that the lines connecting parent to children are dashed.
final
debugBrightnessOverride ↔ Brightness?
A setting that can be used to override the platform Brightness exposed from BindingBase.platformDispatcher.
getter/setter pair
debugDefaultTargetPlatformOverride ↔ TargetPlatform?
Override the defaultTargetPlatform in debug builds.
getter/setter pair
debugDoublePrecision ↔ int?
Configure debugFormatDouble using num.toStringAsPrecision.
getter/setter pair
debugInstrumentationEnabled ↔ bool
Boolean value indicating whether debugInstrumentAction will instrument actions in debug builds.
getter/setter pair
debugPrint ↔ DebugPrintCallback
Prints a message to the console, which you can access using the "flutter" tool's "logs" command ("flutter logs").
getter/setter pair
debugPrintDone → Future<void>
A Future that resolves when there is no longer any buffered content being printed by debugPrintThrottled (which is the default implementation for debugPrint, which is used to report errors to the console).
no setter
defaultTargetPlatform → TargetPlatform
The TargetPlatform that matches the platform on which the framework is currently executing.
no setter
denseTextConfiguration → TextTreeConfiguration
Dense text tree configuration that minimizes horizontal whitespace.
final
errorPropertyTextConfiguration → TextTreeConfiguration
Render the name on a line followed by the body and properties on the next line omitting the children.
final
errorTextConfiguration → TextTreeConfiguration
Configuration that draws a box around a node ignoring the connection to the parents.
final
flatTextConfiguration → TextTreeConfiguration
Whitespace only configuration where children are not indented.
final
isCanvasKit → bool
Returns true if the application is using CanvasKit.
no setter
isSkiaWeb → bool
Returns true if the application is using CanvasKit or Skwasm.
no setter
isSkwasm → bool
Returns true if the application is using Skwasm.
no setter
shallowTextConfiguration → TextTreeConfiguration
Render a node on multiple lines omitting children.
final
singleLineTextConfiguration → TextTreeConfiguration
Render a node as a single line omitting children.
final
sparseTextConfiguration → TextTreeConfiguration
Default text tree configuration.
final
transitionTextConfiguration → TextTreeConfiguration
Configuration that draws a box around a leaf node.
final
whitespaceTextConfiguration → TextTreeConfiguration
Whitespace only configuration where children are consistently indented two spaces.
final
Functions
binarySearch<T extends Comparable<Object>>(List<T> sortedList, T value) → int
Returns the position of value in the sortedList, if it exists.
clampDouble(double x, double min, double max) → double
Same as num.clamp but optimized for a non-null double.
compute<M, R>(ComputeCallback<M, R> callback, M message, {String? debugLabel}) → Future<R>
Asynchronously runs the given callback - with the provided message - in the background and completes with the result.
consolidateHttpClientResponseBytes(HttpClientResponse response, {bool autoUncompress = true, BytesReceivedCallback? onBytesReceived}) → Future<Uint8List>
Efficiently converts the response body of an HttpClientResponse into a Uint8List.
debugAssertAllFoundationVarsUnset(String reason, {DebugPrintCallback debugPrintOverride = debugPrintThrottled}) → bool
Returns true if none of the foundation library debug variables have been changed.
debugFormatDouble(double? value) → String
Formats a double to have standard formatting.
debugInstrumentAction<T>(String description, Future<T> action()) → Future<T>
Runs the specified action, timing how long the action takes in debug builds when debugInstrumentationEnabled is true.
debugMaybeDispatchCreated(String flutterLibrary, String className, Object object) → bool
If memory allocation tracking is enabled, dispatch Flutter object creation.
debugMaybeDispatchDisposed(Object object) → bool
If memory allocations tracking is enabled, dispatch object disposal.
debugPrintStack({StackTrace? stackTrace, String? label, int? maxFrames}) → void
Dump the stack to the console using debugPrint and FlutterError.defaultStackFilter.
debugPrintSynchronously(String? message, {int? wrapWidth}) → void
Alternative implementation of debugPrint that does not throttle. Used by tests.
debugPrintThrottled(String? message, {int? wrapWidth}) → void
Implementation of debugPrint that throttles messages. This avoids dropping messages on platforms that rate-limit their logging (for example, Android).
debugWordWrap(String message, int width, {String wrapIndent = ''}) → Iterable<String>
Wraps the given string at the given width.
describeEnum(Object enumEntry) → String
Returns a short description of an enum value.
describeIdentity(Object? object) → String
Returns a summary of the runtime type and hash code of object.
lerpDuration(Duration a, Duration b, double t) → Duration
Linearly interpolate between two Durations.
listEquals<T>(List<T>? a, List<T>? b) → bool
Compares two lists for element-by-element equality.
mapEquals<T, U>(Map<T, U>? a, Map<T, U>? b) → bool
Compares two maps for element-by-element equality.
mergeSort<T>(List<T> list, {int start = 0, int? end, int compare(T, T)?}) → void
Sorts a list between start (inclusive) and end (exclusive) using the merge sort algorithm.
objectRuntimeType(Object? object, String optimizedValue) → String
Framework code should use this method in favor of calling toString on Object.runtimeType.
setEquals<T>(Set<T>? a, Set<T>? b) → bool
Compares two sets for element-by-element equality.
shortHash(Object? object) → String
Returns a 5 character long hexadecimal string generated from Object.hashCode's 20 least-significant bits.
Typedefs
AsyncCallback = Future<void> Function()
Signature of callbacks that have no arguments and return no data, but that return a Future to indicate when their work is complete.
AsyncValueGetter<T> = Future<T> Function()
Signature for callbacks that are to asynchronously report a value on demand.
AsyncValueSetter<T> = Future<void> Function(T value)
Signature for callbacks that report that a value has been set and return a Future that completes when the value has been saved.
BytesReceivedCallback = void Function(int cumulative, int? total)
Signature for getting notified when chunks of bytes are received while consolidating the bytes of an HttpClientResponse into a Uint8List.
ComputeCallback<M, R> = FutureOr<R> Function(M message)
Signature for the callback passed to compute.
ComputeImpl = Future<R> Function<M, R>(ComputeCallback<M, R> callback, M message, {String? debugLabel})
The signature of compute, which spawns an isolate, runs callback on that isolate, passes it message, and (eventually) returns the value returned by callback.
ComputePropertyValueCallback<T> = T? Function()
Signature for computing the value of a property.
DebugPrintCallback = void Function(String? message, {int? wrapWidth})
Signature for debugPrint implementations.
DiagnosticPropertiesTransformer = Iterable<DiagnosticsNode> Function(Iterable<DiagnosticsNode> properties)
Signature for DiagnosticPropertiesBuilder transformer.
FlutterExceptionHandler = void Function(FlutterErrorDetails details)
Signature for FlutterError.onError handler.
InformationCollector = Iterable<DiagnosticsNode> Function()
Signature for FlutterErrorDetails.informationCollector callback and other callbacks that collect information describing an error.
IterableFilter<T> = Iterable<T> Function(Iterable<T> input)
Signature for callbacks that filter an iterable.
LicenseEntryCollector = Stream<LicenseEntry> Function()
Signature for callbacks passed to LicenseRegistry.addLicense.
MemoryAllocations = FlutterMemoryAllocations
An interface for listening to object lifecycle events.
ObjectEventListener = void Function(ObjectEvent event)
A listener of ObjectEvent.
ServiceExtensionCallback = Future<Map<String, dynamic>> Function(Map<String, String> parameters)
Signature for service extensions.
StackTraceDemangler = StackTrace Function(StackTrace details)
Signature for a function that demangles StackTrace objects into a format that can be parsed by StackFrame.
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