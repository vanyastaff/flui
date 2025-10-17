services library
Platform services exposed to Flutter apps.

To use, import package:flutter/services.dart.

This library depends only on core Dart libraries and the foundation library.

Classes
AndroidMotionEvent
A Dart version of Android's MotionEvent.
AndroidPointerCoords
Position information for an Android pointer.
AndroidPointerProperties
Properties of an Android pointer.
AndroidViewController
Controls an Android view that is composed using a GL texture.
AppKitViewController
Controller for a macOS platform view.
ApplicationSwitcherDescription
Specifies a description of the application that is pertinent to the embedder's application switcher (also known as "recent tasks") user interface.
AssetBundle
A collection of resources used by the application.
AssetManifest
Contains details about available assets and their variants. See Resolution-aware image assets to learn about asset variants and how to declare them.
AssetMetadata
Contains information about an asset.
AutofillClient
An object that represents an autofillable input field in the autofill workflow.
AutofillConfiguration
A collection of autofill related information that represents an AutofillClient.
AutofillHints
A collection of commonly used autofill hint strings on different platforms.
AutofillScope
An ordered group within which AutofillClients are logically connected.
BackgroundIsolateBinaryMessenger
A BinaryMessenger for use on background (non-root) isolates.
BasicMessageChannel<T>
A named channel for communicating with platform plugins using asynchronous message passing.
BinaryCodec
MessageCodec with unencoded binary messages represented using ByteData.
BinaryMessenger
A messenger which sends binary data across the Flutter platform barrier.
BrowserContextMenu
Controls the browser's context menu on the web platform.
ByteData
A fixed-length, random-access sequence of bytes that also provides random and unaligned access to the fixed-width integers and floating point numbers represented by those bytes.
CachingAssetBundle
An AssetBundle that permanently caches string and structured resources that have been fetched.
ChannelBuffers
The buffering and dispatch mechanism for messages sent by plugins on the engine side to their corresponding plugin code on the framework side.
CharacterBoundary
A TextBoundary subclass for retrieving the range of the grapheme the given position is in.
Clipboard
Utility methods for interacting with the system's clipboard.
ClipboardData
Data stored on the system clipboard.
Color
An immutable color value in ARGB format.
DarwinPlatformViewController
Base class for iOS and macOS view controllers.
DefaultProcessTextService
The service used by default for the text processing feature.
DefaultSpellCheckService
The service used by default to fetch spell check results for text input.
DeferredComponent
Manages the installation and loading of deferred components.
DiagnosticPropertiesBuilder
Builder to accumulate properties and configuration used to assemble a DiagnosticsNode from a Diagnosticable object.
DocumentBoundary
A text boundary that uses the entire document as logical boundary.
EventChannel
A named channel for communicating with platform plugins using event streams.
ExpensiveAndroidViewController
Controls an Android view that is composed using the Android view hierarchy. This controller is created from the PlatformViewsService.initExpensiveAndroidView factory.
FilteringTextInputFormatter
A TextInputFormatter that prevents the insertion of characters matching (or not matching) a particular pattern, by replacing the characters with the given replacementString.
FlutterVersion
Details about the Flutter version this app was compiled with, corresponding to the output of flutter --version.
FontLoader
A class that enables the dynamic loading of fonts at runtime.
FontWeight
The thickness of the glyphs used to draw the text.
GLFWKeyHelper
Helper class that uses GLFW-specific key mappings.
GtkKeyHelper
Helper class that uses GTK-specific key mappings.
HapticFeedback
Allows access to the haptic feedback interface on the device.
HardwareKeyboard
Manages key events from hardware keyboards.
HybridAndroidViewController
Controls an Android view that is composed using the Android view hierarchy. This controller is created from the PlatformViewsService.initHybridAndroidView factory.
ImmutableBuffer
A handle to a read-only byte buffer that is managed by the engine.
IOSSystemContextMenuItemData
Describes a context menu button that will be rendered in the system context menu.
IOSSystemContextMenuItemDataCopy
An IOSSystemContextMenuItemData for the system's built-in copy button.
IOSSystemContextMenuItemDataCut
An IOSSystemContextMenuItemData for the system's built-in cut button.
IOSSystemContextMenuItemDataLiveText
An IOSSystemContextMenuItemData for the system's built-in Live Text (OCR) button.
IOSSystemContextMenuItemDataLookUp
An IOSSystemContextMenuItemData for the system's built-in look up button.
IOSSystemContextMenuItemDataPaste
An IOSSystemContextMenuItemData for the system's built-in paste button.
IOSSystemContextMenuItemDataSearchWeb
An IOSSystemContextMenuItemData for the system's built-in search web button.
IOSSystemContextMenuItemDataSelectAll
An IOSSystemContextMenuItemData for the system's built-in select all button.
IOSSystemContextMenuItemDataShare
An IOSSystemContextMenuItemData for the system's built-in share button.
JSONMessageCodec
MessageCodec with UTF-8 encoded JSON messages.
JSONMethodCodec
MethodCodec with UTF-8 encoded JSON method calls and result envelopes.
KeyboardInsertedContent
A class representing rich content (such as a PNG image) inserted via the system input method.
KeyboardKey
A base class for all keyboard key types.
KeyData
Information about a key event.
KeyDownEvent
An event indicating that the user has pressed a key down on the keyboard.
KeyEvent
Defines the interface for keyboard key events.
KeyEventManager
A singleton class that processes key messages from the platform and dispatches converted messages accordingly.
KeyHelper
Abstract class for window-specific key mappings.
KeyMessage
The assembled information converted from a native key message.
KeyRepeatEvent
An event indicating that the user has been holding a key on the keyboard and causing repeated events.
KeyUpEvent
An event indicating that the user has released a key on the keyboard.
LengthLimitingTextInputFormatter
A TextInputFormatter that prevents the insertion of more characters than allowed.
LineBoundary
A TextBoundary subclass for locating closest line breaks to a given position.
LiveText
Utility methods for interacting with the system's Live Text.
LogicalKeyboardKey
A class with static values that describe the keys that are returned from RawKeyEvent.logicalKey.
Matrix4
4D Matrix. Values are stored in column major order.
MessageCodec<T>
A message encoding/decoding mechanism.
MethodCall
A command object representing the invocation of a named method.
MethodChannel
A named channel for communicating with platform plugins using asynchronous method calls.
MethodCodec
A codec for method calls and enveloped results.
MouseCursor
An interface for mouse cursor definitions.
MouseCursorManager
Maintains the state of mouse cursors and manages how cursors are searched for.
MouseCursorSession
Manages the duration that a pointing device should display a specific mouse cursor.
MouseTrackerAnnotation
The annotation object used to annotate regions that are interested in mouse movements.
NetworkAssetBundle
An AssetBundle that loads resources over the network.
Offset
An immutable 2D floating-point offset.
OptionalMethodChannel
A MethodChannel that ignores missing platform plugins.
ParagraphBoundary
A text boundary that uses paragraphs as logical boundaries.
PhysicalKeyboardKey
A class with static values that describe the keys that are returned from RawKeyEvent.physicalKey.
PlatformAssetBundle
An AssetBundle that loads resources using platform messages.
PlatformViewController
An interface for controlling a single platform view.
PlatformViewsRegistry
A registry responsible for generating unique identifier for platform views.
PlatformViewsService
Provides access to the platform views service.
PointerEnterEvent
The pointer has moved with respect to the device while the pointer is or is not in contact with the device, and it has entered a target object.
PointerEvent
Base class for touch, stylus, or mouse events.
PointerExitEvent
The pointer has moved with respect to the device while the pointer is or is not in contact with the device, and exited a target object.
PointerHoverEvent
The pointer has moved with respect to the device while the pointer is not in contact with the device.
PredictiveBackEvent
Object used to report back gesture progress in Android.
ProcessTextAction
A data structure describing text processing actions.
ProcessTextService
Determines how to interact with the text processing feature.
RawFloatingCursorPoint
The current state and position of the floating cursor.
RawKeyboard
An interface for listening to raw key events.
RawKeyDownEvent
The user has pressed a key on the keyboard.
RawKeyEvent
Defines the interface for raw key events.
RawKeyEventData
Base class for platform-specific key event data.
RawKeyEventDataAndroid
Platform-specific key event data for Android.
RawKeyEventDataFuchsia
Platform-specific key event data for Fuchsia.
RawKeyEventDataIos
Platform-specific key event data for iOS.
RawKeyEventDataLinux
Platform-specific key event data for Linux.
RawKeyEventDataMacOs
Platform-specific key event data for macOS.
RawKeyEventDataWeb
Platform-specific key event data for Web.
RawKeyEventDataWindows
Platform-specific key event data for Windows.
RawKeyUpEvent
The user has released a key on the keyboard.
ReadBuffer
Read-only buffer for reading sequentially from a ByteData instance.
Rect
An immutable, 2D, axis-aligned, floating-point rectangle whose coordinates are relative to a given origin.
RestorationBucket
A RestorationBucket holds pieces of the restoration data that a part of the application needs to restore its state.
RestorationManager
Manages the restoration data in the framework and synchronizes it with the engine.
RootIsolateToken
A token that represents a root isolate.
ScribbleClient
An interface into iOS's stylus handwriting text input.
Scribe
An interface into Android's stylus handwriting text input.
SelectionRect
Represents a selection rect for a character and it's position in the text.
SensitiveContentService
Service for setting the content sensitivity of the native app window (Android View) that contains the app's widget tree.
Size
Holds a 2D floating-point size.
SpellCheckResults
A data structure grouping together the SuggestionSpans and related text of results returned by a spell checker.
SpellCheckService
Determines how spell check results are received for text input.
StandardMessageCodec
MessageCodec using the Flutter standard binary encoding.
StandardMethodCodec
MethodCodec using the Flutter standard binary encoding.
StringCodec
MessageCodec with UTF-8 encoded String messages.
SuggestionSpan
A data structure representing a range of misspelled text and the suggested replacements for this range.
SurfaceAndroidViewController
Controls an Android view that is composed using a GL texture. This controller is created from the PlatformViewsService.initSurfaceAndroidView factory, and is defined for backward compatibility.
SystemChannels
Platform channels used by the Flutter system.
SystemChrome
Controls specific aspects of the operating system's graphical interface and how it interacts with the application.
SystemContextMenuController
Allows access to the system context menu.
SystemMouseCursor
A mouse cursor that is natively supported on the platform that the application is running on.
SystemMouseCursors
A collection of system MouseCursors.
SystemNavigator
Controls specific aspects of the system navigation stack.
SystemSound
Provides access to the library of short system specific sounds for common tasks.
SystemUiOverlayStyle
Specifies a preference for the style of the system overlays.
TextBoundary
An interface for retrieving the logical text boundary (as opposed to the visual boundary) at a given code unit offset in a document.
TextEditingDelta
A structure representing a granular change that has occurred to the editing state as a result of text editing.
TextEditingDeltaDeletion
A structure representing the deletion of a single/or contiguous sequence of characters in an editing state.
TextEditingDeltaInsertion
A structure representing an insertion of a single/or contiguous sequence of characters at some offset of an editing state.
TextEditingDeltaNonTextUpdate
A structure representing changes to the selection and/or composing regions of an editing state and no changes to the text value.
TextEditingDeltaReplacement
A structure representing a replacement of a range of characters with a new sequence of text.
TextEditingValue
The current text, selection, and composing state for editing a run of text.
TextInput
An low-level interface to the system's text input control.
TextInputConfiguration
Controls the visual appearance of the text input control.
TextInputConnection
An interface for interacting with a text input control.
TextInputFormatter
A TextInputFormatter can be optionally injected into an EditableText to provide as-you-type validation and formatting of the text being edited.
TextInputType
The type of information for which to optimize the text input control.
TextLayoutMetrics
A read-only interface for accessing visual information about the implementing text.
TextPosition
A position in a string of text.
TextRange
A range of characters in a string of text.
TextSelection
A range of text that represents a selection.
TextureAndroidViewController
Controls an Android view that is rendered as a texture. This is typically used by AndroidView to display a View in the Android view hierarchy.
UiKitViewController
Controller for an iOS platform view.
Uint8List
A fixed-length list of 8-bit unsigned integers.
UndoManager
A low-level interface to the system's undo manager.
WriteBuffer
Write-only buffer for incrementally building a ByteData instance.
Enums
Brightness
Describes the contrast of a theme or color palette.
ContentSensitivity
The possible values for a widget tree's content sensitivity.
DeviceOrientation
Specifies a particular device orientation.
DiagnosticLevel
The various priority levels used to filter which diagnostics are shown and omitted.
FloatingCursorDragState
The state of a "floating cursor" drag on an iOS soft keyboard.
KeyboardLockMode
Represents a lock mode of a keyboard, such as KeyboardLockMode.capsLock.
KeyboardSide
An enum describing the side of the keyboard that a key is on, to allow discrimination between which key is pressed (e.g. the left or right SHIFT key).
KeyDataTransitMode
The mode in which information of key messages is delivered.
MaxLengthEnforcement
Mechanisms for enforcing maximum length limits.
ModifierKey
An enum describing the type of modifier key that is being pressed.
SelectionChangedCause
Indicates what triggered the change in selected text (including changes to the cursor location).
ServicesServiceExtensions
Service extension constants for the services library.
SmartDashesType
Indicates how to handle the intelligent replacement of dashes in text input.
SmartQuotesType
Indicates how to handle the intelligent replacement of quotes in text input.
SwipeEdge
Enum representing the edge from which a swipe starts in a back gesture.
SystemSoundType
A sound provided by the system.
SystemUiMode
Describes different display configurations for both Android and iOS.
SystemUiOverlay
Specifies a system overlay at a particular location.
TargetPlatform
The platform that user interaction should adapt to target.
TextAffinity
A way to disambiguate a TextPosition when its offset could match two different locations in the rendered string.
TextAlign
Whether and how to align text horizontally.
TextCapitalization
Configures how the platform keyboard will select an uppercase or lowercase keyboard.
TextDirection
A direction in which text flows.
TextInputAction
An action the user has requested the text input control to perform.
UndoDirection
The direction in which an undo action should be performed, whether undo or redo.
Mixins
AutofillScopeMixin
A partial implementation of AutofillScope.
DeltaTextInputClient
An interface to receive granular information from TextInput.
ServicesBinding
Listens for platform messages and directs them to the defaultBinaryMessenger.
SystemContextMenuClient
An interface to receive calls related to the system context menu from the engine.
TextInputClient
An interface to receive information from TextInput.
TextInputControl
An interface for implementing text input controls that receive text editing state changes and visual input control requests.
TextSelectionDelegate
A mixin for manipulating the selection, provided for toolbar or shortcut keys.
UndoManagerClient
An interface to receive events from a native UndoManager.
Constants
appFlavor → const String?
The flavor this app was built with.
kAndroidNumPadMap → const Map<int, LogicalKeyboardKey>
A map of Android key codes which have printable representations, but appear on the number pad. Used to provide different key objects for keys like KEY_EQUALS and NUMPAD_EQUALS.
kAndroidToLogicalKey → const Map<int, LogicalKeyboardKey>
Maps Android-specific key codes to the matching LogicalKeyboardKey.
kAndroidToPhysicalKey → const Map<int, PhysicalKeyboardKey>
Maps Android-specific scan codes to the matching PhysicalKeyboardKey.
kFuchsiaToLogicalKey → const Map<int, LogicalKeyboardKey>
Maps Fuchsia-specific IDs to the matching LogicalKeyboardKey.
kFuchsiaToPhysicalKey → const Map<int, PhysicalKeyboardKey>
Maps Fuchsia-specific USB HID Usage IDs to the matching PhysicalKeyboardKey.
kGlfwNumpadMap → const Map<int, LogicalKeyboardKey>
A map of GLFW key codes which have printable representations, but appear on the number pad. Used to provide different key objects for keys like KEY_EQUALS and NUMPAD_EQUALS.
kGlfwToLogicalKey → const Map<int, LogicalKeyboardKey>
Maps GLFW-specific key codes to the matching LogicalKeyboardKey.
kGtkNumpadMap → const Map<int, LogicalKeyboardKey>
A map of GTK key codes which have printable representations, but appear on the number pad. Used to provide different key objects for keys like KEY_EQUALS and NUMPAD_EQUALS.
kGtkToLogicalKey → const Map<int, LogicalKeyboardKey>
Maps GTK-specific key codes to the matching LogicalKeyboardKey.
kIosNumPadMap → const Map<int, LogicalKeyboardKey>
A map of iOS key codes which have printable representations, but appear on the number pad. Used to provide different key objects for keys like KEY_EQUALS and NUMPAD_EQUALS.
kIosSpecialLogicalMap → const Map<String, LogicalKeyboardKey>
Maps iOS specific string values of nonvisible keys to logical keys
kIosToLogicalKey → const Map<int, LogicalKeyboardKey>
A map of iOS key codes presenting LogicalKeyboardKey.
kIosToPhysicalKey → const Map<int, PhysicalKeyboardKey>
Maps iOS-specific key code values representing PhysicalKeyboardKey.
kLinuxToPhysicalKey → const Map<int, PhysicalKeyboardKey>
Maps XKB specific key code values representing PhysicalKeyboardKey.
kMacOsFunctionKeyMap → const Map<int, LogicalKeyboardKey>
A map of macOS key codes which are numbered function keys, so that they can be excluded when asking "is the Fn modifier down?".
kMacOsNumPadMap → const Map<int, LogicalKeyboardKey>
A map of macOS key codes which have printable representations, but appear on the number pad. Used to provide different key objects for keys like KEY_EQUALS and NUMPAD_EQUALS.
kMacOsToLogicalKey → const Map<int, LogicalKeyboardKey>
A map of macOS key codes presenting LogicalKeyboardKey.
kMacOsToPhysicalKey → const Map<int, PhysicalKeyboardKey>
Maps macOS-specific key code values representing PhysicalKeyboardKey.
kProfilePlatformChannels → const bool
Controls whether platform channel usage can be debugged in release mode.
kWebLocationMap → const Map<String, List<LogicalKeyboardKey?>>
A map of Web KeyboardEvent keys which needs to be decided based on location, typically for numpad keys and modifier keys. Used to provide different key objects for keys like KEY_EQUALS and NUMPAD_EQUALS.
kWebNumPadMap → const Map<String, LogicalKeyboardKey>
A map of Web KeyboardEvent codes which have printable representations, but appear on the number pad. Used to provide different key objects for keys like KEY_EQUALS and NUMPAD_EQUALS.
kWebToLogicalKey → const Map<String, LogicalKeyboardKey>
Maps Web KeyboardEvent codes to the matching LogicalKeyboardKey.
kWebToPhysicalKey → const Map<String, PhysicalKeyboardKey>
Maps Web KeyboardEvent codes to the matching PhysicalKeyboardKey.
kWindowsNumPadMap → const Map<int, LogicalKeyboardKey>
A map of Windows KeyboardEvent codes which have printable representations, but appear on the number pad. Used to provide different key objects for keys like KEY_EQUALS and NUMPAD_EQUALS.
kWindowsToLogicalKey → const Map<int, LogicalKeyboardKey>
Maps Windows KeyboardEvent codes to the matching LogicalKeyboardKey.
kWindowsToPhysicalKey → const Map<int, PhysicalKeyboardKey>
Maps Windows KeyboardEvent codes to the matching PhysicalKeyboardKey.
Properties
debugKeyEventSimulatorTransitModeOverride ↔ KeyDataTransitMode?
Override the transit mode with which key events are simulated.
getter/setter pair
debugPrintKeyboardEvents ↔ bool
Setting to true will cause extensive logging to occur when key events are received.
getter/setter pair
debugProfilePlatformChannels ↔ bool
Controls whether platform channel usage can be debugged in non-release mode.
getter/setter pair
platformViewsRegistry → PlatformViewsRegistry
The PlatformViewsRegistry responsible for generating unique identifiers for platform views.
final
rootBundle → AssetBundle
The AssetBundle from which this application was loaded.
final
shouldProfilePlatformChannels → bool
Profile and print statistics on Platform Channel usage.
no setter
Functions
debugAssertAllServicesVarsUnset(String reason) → bool
Returns true if none of the widget library debug variables have been changed.
debugIsSerializableForRestoration(Object? object) → bool
Returns true when the provided object is serializable for state restoration.
runeToLowerCase(int rune) → int
Convert a UTF32 rune to its lower case.
Typedefs
KeyEventCallback = bool Function(KeyEvent event)
The signature for HardwareKeyboard.addHandler, a callback to decide whether the entire framework handles a key event.
KeyMessageHandler = bool Function(KeyMessage message)
The signature for KeyEventManager.keyMessageHandler.
MessageHandler = Future<ByteData?>? Function(ByteData? message)
A function which takes a platform message and asynchronously returns an encoded response.
PlatformMessageResponseCallback = void Function(ByteData? data)
Signature for responses to platform messages.
PlatformViewCreatedCallback = void Function(int id)
Callback signature for when a platform view was created.
PointerEnterEventListener = void Function(PointerEnterEvent event)
Signature for listening to PointerEnterEvent events.
PointerExitEventListener = void Function(PointerExitEvent event)
Signature for listening to PointerExitEvent events.
PointerHoverEventListener = void Function(PointerHoverEvent event)
Signature for listening to PointerHoverEvent events.
PointTransformer = Offset Function(Offset position)
Converts a given point from the global coordinate system in logical pixels to the local coordinate system for a box.
RawKeyEventHandler = bool Function(RawKeyEvent event)
A callback type used by RawKeyboard.keyEventHandler to send key events to a handler that can determine if the key has been handled or not.
SystemUiChangeCallback = Future<void> Function(bool systemOverlaysAreVisible)
Signature for listening to changes in the SystemUiMode.
TextInputFormatFunction = TextEditingValue Function(TextEditingValue oldValue, TextEditingValue newValue)
Function signature expected for creating custom TextInputFormatter shorthands via TextInputFormatter.withFunction.
UntilPredicate = bool Function(int offset, bool forward)
Signature for a predicate that takes an offset into a UTF-16 string, and a boolean that indicates the search direction.
ValueChanged<T> = void Function(T value)
Signature for callbacks that report that an underlying value has changed.
VoidCallback = void Function()
Signature of callbacks that have no arguments and return no data.
Exceptions / Errors
MissingPluginException
Thrown to indicate that a platform interaction failed to find a handling plugin.
PlatformException
Thrown to indicate that a platform interaction failed in the platform plugin.
