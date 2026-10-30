//! The Android host's input: `MotionEvent`s copied into plain facts for the
//! translation in `motion`, and `KeyEvent`s into the owned keyboard
//! vocabulary.
//!
//! - [`keys`]: the decidable half of the key translation; built on every
//!   target and public, hidden from the documentation, for the backend
//!   conformance table in `tests/`;
//! - `motion`: `MotionEvent`s as plain facts, decoded into owned pointer
//!   input; `scroll`: the scroll factors' failure policy. Both are pure, built
//!   for Android and for tests on every host;
//! - the rest needs the NDK or the JVM: reading the event's fields,
//!   discovering its device's motion ranges and reading the scroll factors
//!   through Java.

#[doc(hidden)]
pub mod keys {
    //! The decidable half of the Android backend's key translation: the named
    //! keys the key-code table leaves unnamed, which meta state a key's
    //! character is looked up with, and which key belongs to the input method.
    //!
    //! `KEYCODE_MENU` is the Menu key of a phone's keyboard and of every PC
    //! keyboard attached to it, the key that opens the focused item's context
    //! menu on Android (`ShowContextMenuIntent`'s Android binding); the table
    //! gives it a physical code only, so it is named here.
    //!
    //! Android's `KeyCharacterMap.get` answers no character while Control or
    //! Meta is held, so Ctrl+X would reach a shortcut as an unidentified key. A
    //! key's logical value is the character it types without those modifiers,
    //! as on every other backend: Shift, Alt (the alternate characters the
    //! device's map assigns it), Caps Lock and Num Lock still choose it, and the
    //! event carries Control and Meta as modifiers beside it.
    //!
    //! Cfg-free plain values, so Linux CI pins them; the FFI half below reads
    //! the map through JNI.

    #![forbid(unsafe_code)]

    use flui_platform::keyboard::{ImeComposition, NamedKey};

    /// A key the native queue delivers while the input method composes belongs
    /// to the input method. Android hands every key to the input method before
    /// native code (`NativePreImeInputStage` pre-dispatches it), so a key that
    /// reaches FLUI during a composition is one the input method passed on; it
    /// arrives marked [`ImeComposition::Active`] so no shortcut, focus traversal
    /// or drag cancellation takes it from the composition.
    #[must_use]
    pub const fn key_composition(composing: bool) -> ImeComposition {
        if composing {
            ImeComposition::Active
        } else {
            ImeComposition::Inactive
        }
    }

    /// `KeyEvent.KEYCODE_MENU`.
    pub const KEYCODE_MENU: i32 = 82;

    /// The named key `keycode` is where the key-code table names none, or
    /// `None` when it is a character key or unknown.
    #[must_use]
    pub const fn unnamed_key(keycode: i32) -> Option<NamedKey> {
        match keycode {
            KEYCODE_MENU => Some(NamedKey::ContextMenu),
            _ => None,
        }
    }

    /// `KeyEvent.META_CTRL_ON`, `META_CTRL_LEFT_ON` and `META_CTRL_RIGHT_ON`.
    const META_CTRL_MASK: u32 = 0x0000_7000;
    /// `KeyEvent.META_META_ON`, `META_META_LEFT_ON` and `META_META_RIGHT_ON`.
    const META_META_MASK: u32 = 0x0007_0000;

    /// The meta state to look a key's character up with, for an event whose
    /// meta state is `meta_state`: Control and Meta left out.
    #[must_use]
    pub const fn character_meta_state(meta_state: u32) -> u32 {
        meta_state & !(META_CTRL_MASK | META_META_MASK)
    }

    #[cfg(test)]
    mod tests;
}

#[cfg(any(target_os = "android", test))]
pub(crate) mod motion {
    //! Android `MotionEvent`s as plain facts, decoded into owned pointer input.
    //!
    //! The Android backend copies each event out of the NDK into a [`Motion`]
    //! and hands it to [`AndroidMotion`], which makes the readings the shared
    //! [`ContactRegistry`] decodes. Nothing here names an Android type, so Linux
    //! CI runs the translation the device runs.
    //!
    //! Coordinates and contact axes are physical pixels; assembly divides them
    //! by the window scale once. Sensor presence comes from the device's motion
    //! ranges ([`DeviceCapabilities`]), not from whether an axis reads zero.
    //!
    //! What the translation decides, beyond copying fields:
    //!
    //! - **A hover exit before a press is not a leave.** Android ends a hover
    //!   with `ACTION_HOVER_EXIT` at the instant the pen or cursor goes down, and
    //!   starts one again with `ACTION_HOVER_ENTER` when it lifts. The exit is
    //!   held until the next event: a press from the same device at the same
    //!   time continues the hovering contact, so a pen keeps one pointer identity
    //!   from hover through contact and back; any other event delivers it as a
    //!   leave first, and so does an owner turn [`EXIT_GRACE`] after it with no
    //!   event in between ([`AndroidMotion::finish_drain`]): the press may reach
    //!   the queue a turn after its exit, and a pen leaving range sends nothing
    //!   more. An enter for a contact the registry still holds is a move.
    //! - **A touchpad is recognized by its source.** A touchpad in pointer mode
    //!   reports finger tools from [`SOURCE_MOUSE`] (Android 14's gesture
    //!   converter). Its hovering finger is a cursor, delivered as the mouse. Its
    //!   gestures arrive as fake fingers placed around the cursor, which are
    //!   not touches: a press whose first pointer carries a pinch factor opens a
    //!   pinch, delivered as `PanZoom` scale about the cursor; a press without
    //!   one opens a two-finger scroll, delivered as phased `Scroll` from the
    //!   cursor by the gesture's scroll distance; a press with a swipe finger
    //!   count is a three- or four-finger swipe, which the system navigates with
    //!   and FLUI does not translate. The classification reads the gesture axes
    //!   (`AXIS_GESTURE_*`, API 34) rather than `getClassification`, which the
    //!   NDK event wrapper does not expose.

    #![forbid(unsafe_code)]

    use std::collections::HashMap;
    use std::time::Duration;

    use flui_platform::EventTime;
    use flui_platform::keyboard::Modifiers;
    use flui_platform::pointer::{
        PenTool, PointerButton, PointerButtons, PointerEvent, PointerKind, ScrollDelta,
        ScrollPhase, ScrollPrecision, ScrollUnit,
    };
    use web_time::Instant;

    use crate::input::assembly::{
        ContactRegistry, ContactScroll, DecodeError, GestureComponent, GestureDelta, GesturePhase,
        GestureReading, MoveBatch, NativeCancel, NativeClock, NativeContact, NativeDevice,
        NativeReading, NativeSurface, PenAngles, RawSensors, ReadingAction, ReadingTime,
        ScrollPhases, ScrollReading, decode,
    };
    use flui_platform::PlatformInput;

    /// `InputDevice.SOURCE_MOUSE`. A touchpad in pointer mode reports from it.
    pub const SOURCE_MOUSE: u32 = 0x0000_2002;

    /// How long a hover exit waits for the press it may continue into before it
    /// is delivered as a leave: a few owner turns at the backend's 16 ms poll,
    /// against the microseconds between the two in the dispatcher.
    pub const EXIT_GRACE: Duration = Duration::from_millis(40);

    /// The Android activity's one input surface.
    const SURFACE: NativeSurface = NativeSurface(0);

    /// The contact label of a touchpad pinch, apart from every pointer id.
    const PINCH: NativeContact = NativeContact(u64::MAX);

    /// `MotionEvent` action codes, as the backend reads them.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[non_exhaustive]
    pub enum MotionAction {
        /// `ACTION_DOWN`.
        Down,
        /// `ACTION_UP`.
        Up,
        /// `ACTION_MOVE`.
        Move,
        /// `ACTION_CANCEL`.
        Cancel,
        /// `ACTION_POINTER_DOWN`.
        PointerDown,
        /// `ACTION_POINTER_UP`.
        PointerUp,
        /// `ACTION_HOVER_ENTER`.
        HoverEnter,
        /// `ACTION_HOVER_MOVE`.
        HoverMove,
        /// `ACTION_HOVER_EXIT`.
        HoverExit,
        /// `ACTION_SCROLL`.
        Scroll,
        /// `ACTION_BUTTON_PRESS`.
        ButtonPress,
        /// `ACTION_BUTTON_RELEASE`.
        #[cfg_attr(
            all(test, not(target_os = "android")),
            expect(dead_code, reason = "only the NDK decoding reads this action")
        )]
        ButtonRelease,
        /// Any other action, which delivers nothing.
        #[cfg_attr(
            all(test, not(target_os = "android")),
            expect(dead_code, reason = "only the NDK decoding reads this action")
        )]
        Other,
    }

    /// `MotionEvent` tool types.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[non_exhaustive]
    pub enum Tool {
        /// `TOOL_TYPE_FINGER`.
        Finger,
        /// `TOOL_TYPE_STYLUS`.
        Stylus,
        /// `TOOL_TYPE_ERASER`.
        Eraser,
        /// `TOOL_TYPE_MOUSE`.
        Mouse,
        /// `TOOL_TYPE_UNKNOWN` and any other.
        Unknown,
    }

    /// `MotionEvent` button bits.
    pub mod button {
        /// `BUTTON_PRIMARY`.
        pub const PRIMARY: u32 = 0x01;
        /// `BUTTON_SECONDARY`.
        pub const SECONDARY: u32 = 0x02;
        /// `BUTTON_TERTIARY`.
        pub const TERTIARY: u32 = 0x04;
        /// `BUTTON_BACK`.
        pub const BACK: u32 = 0x08;
        /// `BUTTON_FORWARD`.
        pub const FORWARD: u32 = 0x10;
        /// `BUTTON_STYLUS_PRIMARY`.
        pub const STYLUS_PRIMARY: u32 = 0x20;
        /// `BUTTON_STYLUS_SECONDARY`.
        pub const STYLUS_SECONDARY: u32 = 0x40;
    }

    /// The motion axes whose ranges decide which sensors a device reports.
    pub mod axis {
        /// `AXIS_PRESSURE`.
        pub const PRESSURE: u32 = 2;
        /// `AXIS_TOUCH_MAJOR`.
        pub const TOUCH_MAJOR: u32 = 4;
        /// `AXIS_TOUCH_MINOR`.
        pub const TOUCH_MINOR: u32 = 5;
        /// `AXIS_ORIENTATION`.
        pub const ORIENTATION: u32 = 8;
        /// `AXIS_TILT`.
        pub const TILT: u32 = 25;
        /// Every axis the decoder asks a device's motion ranges about.
        pub const SENSORS: [u32; 5] = [PRESSURE, TOUCH_MAJOR, TOUCH_MINOR, TILT, ORIENTATION];
    }

    /// One pointer's axis values at one instant, as the event reports them.
    #[derive(Debug, Clone, Copy, Default, PartialEq)]
    pub struct Axes {
        /// `AXIS_X`, physical pixels.
        pub x: f32,
        /// `AXIS_Y`, physical pixels.
        pub y: f32,
        /// `AXIS_PRESSURE`.
        pub pressure: f32,
        /// `AXIS_TOUCH_MAJOR`, physical pixels.
        pub touch_major: f32,
        /// `AXIS_TOUCH_MINOR`, physical pixels.
        pub touch_minor: f32,
        /// `AXIS_TILT`, radians from the screen normal.
        pub tilt: f32,
        /// `AXIS_ORIENTATION`, radians clockwise from up.
        pub orientation: f32,
        /// `AXIS_HSCROLL`.
        pub hscroll: f32,
        /// `AXIS_VSCROLL`.
        pub vscroll: f32,
        /// `AXIS_GESTURE_SCROLL_X_DISTANCE`, physical pixels.
        pub gesture_scroll_x: f32,
        /// `AXIS_GESTURE_SCROLL_Y_DISTANCE`, physical pixels.
        pub gesture_scroll_y: f32,
        /// `AXIS_GESTURE_PINCH_SCALE_FACTOR`, the scale since the last event.
        pub pinch_scale: f32,
        /// `AXIS_GESTURE_SWIPE_FINGER_COUNT`.
        pub swipe_fingers: f32,
    }

    /// One pointer of an event: its id, tool, current axes and the historical
    /// samples batched before them, oldest first.
    #[derive(Debug, Clone, PartialEq)]
    pub struct PointerFacts {
        /// `getPointerId`.
        pub id: i32,
        /// `getToolType`.
        pub tool: Tool,
        /// The current values.
        pub axes: Axes,
        /// `getHistorical*`, each with its time.
        pub history: Vec<(EventTime, Axes)>,
    }

    /// One `MotionEvent`, copied out of the NDK.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Motion<'a> {
        /// `getDeviceId`.
        pub device: i32,
        /// `getSource`.
        pub source: u32,
        /// The action, without its pointer index.
        pub action: MotionAction,
        /// The index of the pointer the action is about.
        pub action_index: usize,
        /// `getActionButton`.
        pub action_button: u32,
        /// `getButtonState`.
        pub buttons: u32,
        /// `FLAG_CANCELED` (API 33): the pointer an `ACTION_POINTER_UP` lifts
        /// was cancelled (a rejected palm), not released.
        pub canceled: bool,
        /// `getMetaState`, as owned modifiers.
        pub modifiers: Modifiers,
        /// `getEventTime`; `None` when it is not a time.
        pub time: Option<EventTime>,
        /// Every pointer, in index order.
        pub pointers: &'a [PointerFacts],
    }

    /// The axes a device's motion ranges say it reports, for one source.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct DeviceCapabilities {
        /// The source the ranges were read for.
        pub source: u32,
        /// One bit per axis id.
        pub axes: u64,
    }

    impl DeviceCapabilities {
        /// Ranges for `axes` of `source`.
        #[must_use]
        pub fn new(source: u32, axes: impl IntoIterator<Item = u32>) -> Self {
            Self {
                source,
                axes: axes
                    .into_iter()
                    .filter(|axis| *axis < 64)
                    .fold(0, |bits, axis| bits | (1_u64 << axis)),
            }
        }

        /// Whether the device reports `axis`.
        #[must_use]
        pub const fn has(self, axis: u32) -> bool {
            axis < 64 && self.axes & (1_u64 << axis) != 0
        }
    }

    /// Whether an event of `source` doing `action` needs its device's Java query,
    /// given the capabilities `cached` for its device id. Only moves (in contact
    /// or hovering, the events at the sample rate) use the cache; every other
    /// event queries again, so a device the OS removed or reconfigured is seen at
    /// the press that starts a sequence and at the lift, exit or cancel that ends
    /// one (a device reset cancels its open sequences). A source the cache was
    /// not read for queries.
    #[must_use]
    pub fn needs_device_query(
        cached: Option<DeviceCapabilities>,
        source: u32,
        action: MotionAction,
    ) -> bool {
        match cached {
            Some(cached) if cached.source == source => {
                !matches!(action, MotionAction::Move | MotionAction::HoverMove)
            }
            _ => true,
        }
    }

    /// What the backend's Java query found about an event's device.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum DeviceDiscovery {
        /// The device exists; `replaced` when the OS replaced its object since
        /// the last event (a reconfiguration), which ends its contacts.
        Present {
            /// Its motion ranges.
            capabilities: DeviceCapabilities,
            /// Whether it replaced the device the decoder knew by that id.
            replaced: bool,
        },
        /// No device has the id any longer.
        #[cfg_attr(
            all(test, not(target_os = "android")),
            expect(dead_code, reason = "only the Java device query finds a device gone")
        )]
        Removed,
        /// The query failed: the event is delivered without device or sensors.
        Unavailable,
    }

    /// An exit held until the next event shows whether it continues into a press.
    #[derive(Debug, Clone, Copy)]
    struct PendingExit {
        device: i32,
        time: EventTime,
        reading: NativeReading,
        held_at: Instant,
    }

    /// A touchpad gesture open on one device.
    #[derive(Debug, Clone, Copy)]
    enum TouchpadGesture {
        /// A pinch about `focal`, in physical pixels.
        Pinch { focal: (f64, f64) },
        /// A two-finger scroll from the cursor `cursor`.
        Scroll { cursor: NativeContact },
        /// A three- or four-finger swipe, not translated.
        Swipe,
    }

    /// Where a device's touchpad cursor last hovered.
    #[derive(Debug, Clone, Copy)]
    struct Cursor {
        contact: NativeContact,
        position: (f64, f64),
    }

    /// Input state for the Android window owner: contact and device identity
    /// from the shared pointer assembly, and what the translation holds between
    /// events.
    #[derive(Debug)]
    pub struct AndroidMotion {
        contacts: ContactRegistry,
        devices: HashMap<i32, DeviceCapabilities>,
        batch: MoveBatch,
        last_time: Option<EventTime>,
        pending_exit: Option<PendingExit>,
        gestures: HashMap<i32, TouchpadGesture>,
        cursors: HashMap<i32, Cursor>,
        scrolling: ScrollPhases,
    }

    impl Default for AndroidMotion {
        fn default() -> Self {
            Self::with_registry(ContactRegistry::new(NativeClock::epoch_only()))
        }
    }

    /// The label of a native device id, preserving its sign bits injectively.
    #[must_use]
    pub fn native_device(id: i32) -> NativeDevice {
        NativeDevice(u64::from(u32::from_ne_bytes(id.to_ne_bytes())))
    }

    /// Whether events from `source` with `tool` are a touchpad in pointer mode.
    #[must_use]
    pub const fn is_touchpad(source: u32, tool: Tool) -> bool {
        source & SOURCE_MOUSE == SOURCE_MOUSE && matches!(tool, Tool::Finger)
    }

    /// `InputDevice.SOURCE_TOUCHSCREEN`.
    pub const SOURCE_TOUCHSCREEN: u32 = 0x0000_1002;

    /// The `MotionEvent` action code a touch-exploration hover hands the
    /// accessibility adapter (`ACTION_HOVER_ENTER`, `_MOVE` or `_EXIT`). A
    /// finger hovers over the touchscreen only while TalkBack's explore by
    /// touch turns touches into hovers (ADR-0255 decision 3); any other event,
    /// a pen's or a mouse's hover included, is `None` and reaches the window
    /// as input.
    #[must_use]
    pub const fn touch_exploration(action: MotionAction, tool: Tool, source: u32) -> Option<i32> {
        if source & SOURCE_TOUCHSCREEN != SOURCE_TOUCHSCREEN || !matches!(tool, Tool::Finger) {
            return None;
        }
        match action {
            MotionAction::HoverEnter => Some(9),
            MotionAction::HoverMove => Some(7),
            MotionAction::HoverExit => Some(10),
            _ => None,
        }
    }

    impl AndroidMotion {
        /// A decoder over `contacts`.
        #[must_use]
        pub fn with_registry(contacts: ContactRegistry) -> Self {
            Self {
                contacts,
                devices: HashMap::new(),
                batch: MoveBatch::new(),
                last_time: None,
                pending_exit: None,
                gestures: HashMap::new(),
                cursors: HashMap::new(),
                scrolling: ScrollPhases::default(),
            }
        }

        /// End every contact: the activity paused, lost focus or its window went
        /// away. A held exit is delivered first.
        pub fn cancel_contacts(&mut self, reason: NativeCancel) -> Vec<PlatformInput> {
            let mut output = self.release_exit();
            self.gestures.clear();
            self.scrolling.cancel_surface(SURFACE);
            if let Some(time) = self.last_time {
                output.extend(self.contacts.cancel_surface(SURFACE, time, reason));
            }
            output
        }

        /// At the end of an owner turn's input drain, `now`: deliver a held exit
        /// as a leave once no press has continued it for [`EXIT_GRACE`].
        pub fn finish_drain(&mut self, now: Instant) -> Vec<PlatformInput> {
            match self.pending_exit {
                Some(pending) if now.saturating_duration_since(pending.held_at) >= EXIT_GRACE => {
                    self.release_exit()
                }
                _ => Vec::new(),
            }
        }

        /// Deliver the held exit, if any, as a leave.
        fn release_exit(&mut self) -> Vec<PlatformInput> {
            let Some(pending) = self.pending_exit.take() else {
                return Vec::new();
            };
            decode(&mut self.contacts, &pending.reading)
                .unwrap_or_else(DecodeError::into_inputs)
                .into_vec()
        }

        /// Track the device behind an event from its discovery. `None` when the
        /// event cannot be delivered (a removed device, or no device identity
        /// left); otherwise the device label its readings carry and its
        /// capabilities, both absent for a virtual device without an
        /// `InputDevice`.
        fn discover(
            &mut self,
            id: i32,
            discovery: DeviceDiscovery,
            time: EventTime,
            output: &mut Vec<PlatformInput>,
        ) -> Option<(Option<NativeDevice>, Option<DeviceCapabilities>)> {
            let native = native_device(id);
            let had_device = self.devices.contains_key(&id);
            let replacement = match discovery {
                DeviceDiscovery::Present { replaced, .. } => replaced,
                DeviceDiscovery::Removed => true,
                DeviceDiscovery::Unavailable => false,
            };
            if replacement && self.devices.remove(&id).is_some() {
                self.gestures.remove(&id);
                self.cursors.remove(&id);
                self.scrolling.remove_device(native);
                let (ended, removed) = self.contacts.remove_device(native, time);
                output.extend(ended.into_iter().map(|(_, input)| input));
                output.extend(
                    removed
                        .map(|change| PlatformInput::Pointer(PointerEvent::DeviceRemoved(change))),
                );
            }
            match discovery {
                DeviceDiscovery::Present { capabilities, .. } => {
                    if self.devices.insert(id, capabilities).is_none() {
                        // The registry refuses permanently after its final identity.
                        let added = self.contacts.add_device(native, PointerKind::Unknown, time);
                        if added.is_none() && self.contacts.devices().id(native).is_none() {
                            self.devices.remove(&id);
                            return None;
                        }
                        output.extend(added.map(|change| {
                            PlatformInput::Pointer(PointerEvent::DeviceAdded(change))
                        }));
                    }
                    Some((Some(native), Some(capabilities)))
                }
                DeviceDiscovery::Removed if had_device => None,
                // Android-generated and virtual events can have no InputDevice.
                // Preserve their contact and position, without device or sensors.
                DeviceDiscovery::Removed | DeviceDiscovery::Unavailable => Some((None, None)),
            }
        }

        /// The input `motion` delivers, at `scale` physical pixels per logical
        /// pixel, with wheel axes projected by `wheel`, received at `now`.
        pub fn decode(
            &mut self,
            motion: &Motion<'_>,
            scale: f64,
            wheel: &dyn Fn(f32, f32) -> Option<ScrollDelta>,
            discovery: DeviceDiscovery,
            now: Instant,
        ) -> Vec<PlatformInput> {
            if let Some(time) = motion.time {
                self.last_time = Some(time);
            }
            let Some(delivery_time) = motion.time.or(self.last_time) else {
                return Vec::new();
            };
            let mut output = Vec::new();
            if let Some(pending) = self.pending_exit.take() {
                let continues = pending.device == motion.device
                    && motion.action == MotionAction::Down
                    && motion.time == Some(pending.time);
                if !continues {
                    output.extend(
                        decode(&mut self.contacts, &pending.reading)
                            .unwrap_or_else(DecodeError::into_inputs),
                    );
                }
            }
            let Some((device, capabilities)) =
                self.discover(motion.device, discovery, delivery_time, &mut output)
            else {
                return output;
            };
            let event = Event {
                motion,
                device,
                capabilities,
                scale,
                time: delivery_time,
                received: now,
            };
            let touchpad = motion
                .pointers
                .first()
                .is_some_and(|pointer| is_touchpad(motion.source, pointer.tool));
            if touchpad && self.touchpad_gesture(&event, &mut output) {
                return output;
            }
            let contacts_from = output.len();
            for (index, pointer) in motion.pointers.iter().enumerate() {
                if matches!(
                    motion.action,
                    MotionAction::Down
                        | MotionAction::PointerDown
                        | MotionAction::Up
                        | MotionAction::PointerUp
                ) && index != motion.action_index
                {
                    continue;
                }
                self.pointer(&event, pointer, touchpad, wheel, &mut output);
            }
            // The contacts one event reports were sampled together: one frame.
            if motion.pointers.len() > 1 {
                let contacts = output.split_off(contacts_from);
                output.extend(crate::input::assembly::framed([contacts
                    .into_iter()
                    .collect()]));
            }
            output
        }

        /// Decode `pointer` of a pointer event; a touchpad's is its cursor.
        fn pointer(
            &mut self,
            event: &Event<'_, '_>,
            pointer: &PointerFacts,
            touchpad: bool,
            wheel: &dyn Fn(f32, f32) -> Option<ScrollDelta>,
            output: &mut Vec<PlatformInput>,
        ) {
            let motion = event.motion;
            let Ok(label) = u64::try_from(pointer.id) else {
                return;
            };
            let contact = NativeContact(label);
            let kind = if touchpad {
                PointerKind::Mouse
            } else {
                pointer_kind(pointer.tool)
            };
            let device = event.device;
            if touchpad
                && matches!(
                    motion.action,
                    MotionAction::HoverEnter | MotionAction::HoverMove | MotionAction::HoverExit
                )
            {
                self.cursors.insert(
                    motion.device,
                    Cursor {
                        contact,
                        position: (f64::from(pointer.axes.x), f64::from(pointer.axes.y)),
                    },
                );
            }
            let in_contact = match motion.action {
                MotionAction::Down | MotionAction::PointerDown | MotionAction::Move => true,
                MotionAction::ButtonPress | MotionAction::ButtonRelease => self
                    .contacts
                    .buttons(SURFACE, device, contact)
                    .is_some_and(|buttons| buttons.contains(PointerButton::PRIMARY)),
                _ => false,
            } && matches!(kind, PointerKind::Touch | PointerKind::Pen { .. });
            let buttons = native_buttons(motion.buttons, in_contact);
            let reading = |action, time: EventTime, axes: &Axes| {
                let mut reading =
                    NativeReading::new(contact, kind, action, ReadingTime::epoch(time))
                        .on(SURFACE)
                        .at(f64::from(axes.x), f64::from(axes.y), event.scale)
                        .with_buttons(buttons)
                        .with_modifiers(motion.modifiers)
                        .with_sensors(native_sensors(kind, event.capabilities, axes));
                if let Some(device) = device {
                    reading = reading.from_device(device);
                }
                reading
            };
            let current = |action| reading(action, event.time, &pointer.axes);
            let open = self.contacts.pointer(SURFACE, device, contact).is_some();
            let assembled = match motion.action {
                MotionAction::Cancel => decode(
                    &mut self.contacts,
                    &current(ReadingAction::Cancel(NativeCancel::Platform)),
                ),
                // The system took the action pointer back (palm rejection): its
                // sequence ends without a lift a recogniser could count.
                MotionAction::Up | MotionAction::PointerUp if motion.canceled => decode(
                    &mut self.contacts,
                    &current(ReadingAction::Cancel(NativeCancel::Platform)),
                ),
                MotionAction::Down | MotionAction::PointerDown => {
                    let button = if kind == PointerKind::Mouse {
                        buttons.iter().next().unwrap_or(PointerButton::PRIMARY)
                    } else {
                        PointerButton::PRIMARY
                    };
                    decode(&mut self.contacts, &current(ReadingAction::Press(button)))
                }
                MotionAction::Up | MotionAction::PointerUp => {
                    let button = if kind == PointerKind::Mouse {
                        self.contacts
                            .buttons(SURFACE, device, contact)
                            .and_then(|held| held.iter().find(|button| !buttons.contains(*button)))
                            .unwrap_or(PointerButton::PRIMARY)
                    } else {
                        PointerButton::PRIMARY
                    };
                    decode(&mut self.contacts, &current(ReadingAction::Release(button)))
                }
                MotionAction::Move | MotionAction::HoverMove => {
                    for (time, past) in &pointer.history {
                        self.batch
                            .push_coalesced(reading(ReadingAction::Move, *time, past));
                    }
                    self.batch
                        .decode(&mut self.contacts, &current(ReadingAction::Move))
                }
                MotionAction::ButtonPress | MotionAction::ButtonRelease => {
                    let Some(button) = native_button(motion.action_button) else {
                        return;
                    };
                    decode(
                        &mut self.contacts,
                        &current(if motion.action == MotionAction::ButtonPress {
                            ReadingAction::Press(button)
                        } else {
                            ReadingAction::Release(button)
                        }),
                    )
                }
                // The contact never left: Android re-enters after a press lifts.
                MotionAction::HoverEnter if open => {
                    decode(&mut self.contacts, &current(ReadingAction::Move))
                }
                MotionAction::HoverEnter => {
                    decode(&mut self.contacts, &current(ReadingAction::Enter))
                }
                MotionAction::HoverExit => {
                    self.pending_exit = Some(PendingExit {
                        device: motion.device,
                        time: event.time,
                        reading: current(ReadingAction::Leave),
                        held_at: event.received,
                    });
                    return;
                }
                MotionAction::Scroll => {
                    // A wheel reports from where the cursor hovers: place it first.
                    let mut placed = decode(&mut self.contacts, &current(ReadingAction::Move))
                        .unwrap_or_else(DecodeError::into_inputs);
                    if let Some(delta) = wheel(pointer.axes.hscroll, pointer.axes.vscroll) {
                        placed.extend(self.contacts.scroll(&ContactScroll {
                            surface: SURFACE,
                            device,
                            contact,
                            phase: None,
                            delta,
                            precision: ScrollPrecision::Unknown,
                            time: event.time,
                            modifiers: motion.modifiers,
                        }));
                    }
                    Ok(placed)
                }
                MotionAction::Other => return,
            };
            output.extend(assembled.unwrap_or_else(DecodeError::into_inputs));
        }

        /// Decode a touchpad gesture's fake fingers; `false` when the event is
        /// the touchpad's cursor instead.
        fn touchpad_gesture(
            &mut self,
            event: &Event<'_, '_>,
            output: &mut Vec<PlatformInput>,
        ) -> bool {
            let motion = event.motion;
            let Some(first) = motion.pointers.first() else {
                return false;
            };
            let open = self.gestures.get(&motion.device).copied();
            let gesture = match (motion.action, open) {
                // A click presses a button; a gesture presses none.
                (MotionAction::Down, None) if motion.buttons == 0 => {
                    let gesture = if first.axes.swipe_fingers > 0.0 {
                        TouchpadGesture::Swipe
                    } else if first.axes.pinch_scale > 0.0 {
                        let focal = self.cursors.get(&motion.device).map_or(
                            (f64::from(first.axes.x), f64::from(first.axes.y)),
                            |cursor| cursor.position,
                        );
                        TouchpadGesture::Pinch { focal }
                    } else {
                        let cursor = self.cursor(event, first);
                        TouchpadGesture::Scroll { cursor }
                    };
                    self.gestures.insert(motion.device, gesture);
                    gesture
                }
                (
                    MotionAction::Move
                    | MotionAction::PointerDown
                    | MotionAction::PointerUp
                    | MotionAction::Up
                    | MotionAction::Cancel,
                    Some(gesture),
                ) => gesture,
                _ => return false,
            };
            let phase = match motion.action {
                MotionAction::Down => GesturePhase::Began,
                MotionAction::Move => GesturePhase::Changed,
                MotionAction::Up if motion.canceled => GesturePhase::Cancelled,
                MotionAction::Up => GesturePhase::Ended,
                MotionAction::Cancel => GesturePhase::Cancelled,
                // The second finger of a pinch comes and goes inside it.
                _ => return true,
            };
            if matches!(phase, GesturePhase::Ended | GesturePhase::Cancelled) {
                self.gestures.remove(&motion.device);
            }
            match gesture {
                TouchpadGesture::Pinch { focal } => {
                    let factor = f64::from(first.axes.pinch_scale);
                    let delta = match phase {
                        GesturePhase::Began | GesturePhase::Changed
                            if factor.is_finite() && factor > 0.0 =>
                        {
                            GestureDelta::Scale(factor)
                        }
                        _ => GestureDelta::None,
                    };
                    let mut reading = NativeReading::new(
                        PINCH,
                        PointerKind::Trackpad,
                        ReadingAction::Gesture(GestureReading {
                            component: GestureComponent::Scale,
                            phase,
                            delta,
                        }),
                        ReadingTime::epoch(event.time),
                    )
                    .on(SURFACE)
                    .at(focal.0, focal.1, event.scale)
                    .with_primary(true)
                    .with_modifiers(motion.modifiers);
                    if let Some(device) = event.device {
                        reading = reading.from_device(device);
                    }
                    output.extend(
                        decode(&mut self.contacts, &reading)
                            .unwrap_or_else(DecodeError::into_inputs),
                    );
                }
                TouchpadGesture::Scroll { cursor } => {
                    let Some(owned) = self.scrolling.phase(
                        SURFACE,
                        event.device,
                        ScrollReading::fingers(phase, true),
                    ) else {
                        return true;
                    };
                    let (x, y) = if owned == ScrollPhase::Changed {
                        (
                            f64::from(first.axes.gesture_scroll_x) / event.scale,
                            f64::from(first.axes.gesture_scroll_y) / event.scale,
                        )
                    } else {
                        (0.0, 0.0)
                    };
                    let Ok(delta) = ScrollDelta::try_new(ScrollUnit::Pixels, x, y) else {
                        return true;
                    };
                    output.extend(self.contacts.scroll(&ContactScroll {
                        surface: SURFACE,
                        device: event.device,
                        contact: cursor,
                        phase: Some(owned),
                        delta,
                        precision: ScrollPrecision::Precise,
                        time: event.time,
                        modifiers: motion.modifiers,
                    }));
                }
                TouchpadGesture::Swipe => {}
            }
            true
        }

        /// The touchpad cursor a scroll starts from, placed where it last hovered
        /// when its contact is no longer open.
        fn cursor(&mut self, event: &Event<'_, '_>, first: &PointerFacts) -> NativeContact {
            let motion = event.motion;
            let cursor = self.cursors.get(&motion.device).copied().unwrap_or(Cursor {
                contact: NativeContact(0),
                position: (f64::from(first.axes.x), f64::from(first.axes.y)),
            });
            if self
                .contacts
                .pointer(SURFACE, event.device, cursor.contact)
                .is_none()
            {
                let mut reading = NativeReading::new(
                    cursor.contact,
                    PointerKind::Mouse,
                    ReadingAction::Move,
                    ReadingTime::epoch(event.time),
                )
                .on(SURFACE)
                .at(cursor.position.0, cursor.position.1, event.scale)
                .with_buttons(PointerButtons::NONE)
                .with_modifiers(motion.modifiers);
                if let Some(device) = event.device {
                    reading = reading.from_device(device);
                }
                // Without its cursor the scroll has no position and delivers nothing.
                let _ = decode(&mut self.contacts, &reading);
            }
            cursor.contact
        }
    }

    /// What every reading of one event shares.
    struct Event<'motion, 'pointers> {
        motion: &'motion Motion<'pointers>,
        device: Option<NativeDevice>,
        capabilities: Option<DeviceCapabilities>,
        scale: f64,
        time: EventTime,
        received: Instant,
    }

    /// The owned kind of a tool.
    #[must_use]
    pub const fn pointer_kind(tool: Tool) -> PointerKind {
        match tool {
            Tool::Finger => PointerKind::Touch,
            Tool::Stylus => PointerKind::Pen { tool: PenTool::Tip },
            Tool::Eraser => PointerKind::Pen {
                tool: PenTool::Eraser,
            },
            Tool::Mouse => PointerKind::Mouse,
            Tool::Unknown => PointerKind::Unknown,
        }
    }

    /// The owned button of a native button bit.
    #[must_use]
    pub const fn native_button(bit: u32) -> Option<PointerButton> {
        match bit {
            button::PRIMARY => Some(PointerButton::PRIMARY),
            button::SECONDARY | button::STYLUS_PRIMARY => Some(PointerButton::SECONDARY),
            button::TERTIARY | button::STYLUS_SECONDARY => Some(PointerButton::AUXILIARY),
            button::BACK => Some(PointerButton::BACK),
            button::FORWARD => Some(PointerButton::FORWARD),
            _ => None,
        }
    }

    /// The owned buttons of a native button state; a touching contact also holds
    /// the primary button.
    #[must_use]
    pub fn native_buttons(state: u32, contact: bool) -> PointerButtons {
        let mut buttons: PointerButtons = [
            button::PRIMARY,
            button::SECONDARY,
            button::TERTIARY,
            button::BACK,
            button::FORWARD,
            button::STYLUS_PRIMARY,
            button::STYLUS_SECONDARY,
        ]
        .into_iter()
        .filter(|bit| state & bit != 0)
        .filter_map(native_button)
        .collect();
        if contact {
            buttons = buttons.with(PointerButton::PRIMARY);
        }
        buttons
    }

    /// Android measures tilt from the screen normal and orientation clockwise from
    /// upwards; owned altitude is above the surface and azimuth from rightwards.
    /// An angle outside its native range is not reported.
    #[must_use]
    pub fn pen_angles(tilt: Option<f64>, orientation: Option<f64>) -> Option<PenAngles> {
        let altitude = tilt
            .filter(|tilt| (0.0..=f64::from(core::f32::consts::FRAC_PI_2)).contains(tilt))
            .map(|tilt| core::f64::consts::FRAC_PI_2 - tilt.min(core::f64::consts::FRAC_PI_2));
        let azimuth = orientation
            .filter(|orientation| orientation.is_finite())
            .map(|orientation| orientation - core::f64::consts::FRAC_PI_2);
        (altitude.is_some() || azimuth.is_some())
            .then_some(PenAngles::Spherical { altitude, azimuth })
    }

    /// The sensors a device's motion ranges say it reports, read from `axes`.
    /// Contact axes stay in physical pixels; assembly divides them by the scale.
    #[must_use]
    pub fn native_sensors(
        kind: PointerKind,
        capabilities: Option<DeviceCapabilities>,
        axes: &Axes,
    ) -> RawSensors {
        let mut sensors = RawSensors::default();
        let Some(capabilities) = capabilities else {
            return sensors;
        };
        if matches!(kind, PointerKind::Touch | PointerKind::Pen { .. }) {
            if capabilities.has(axis::PRESSURE) {
                // Android pressure may overshoot one by calibration: clamp it.
                let pressure = f64::from(axes.pressure);
                sensors.pressure = pressure.is_finite().then(|| pressure.clamp(0.0, 1.0));
            }
            if capabilities.has(axis::TOUCH_MAJOR) && capabilities.has(axis::TOUCH_MINOR) {
                let (major, minor) = (f64::from(axes.touch_major), f64::from(axes.touch_minor));
                if major.is_finite() && minor.is_finite() && major >= 0.0 && minor >= 0.0 {
                    sensors.contact_size = Some((major, minor));
                }
            }
        }
        if matches!(kind, PointerKind::Pen { .. }) {
            sensors.angles = pen_angles(
                capabilities.has(axis::TILT).then(|| f64::from(axes.tilt)),
                capabilities
                    .has(axis::ORIENTATION)
                    .then(|| f64::from(axes.orientation)),
            );
        }
        sensors
    }

    #[cfg(test)]
    mod tests;
}

#[cfg(any(target_os = "android", test))]
pub(crate) mod scroll {
    //! Portable failure-policy seam for Android's owner-local scroll-factor cache.

    #![forbid(unsafe_code)]

    use flui_foundation::geometry::DevicePixelRatio;
    use flui_platform::pointer::{ScrollDelta, ScrollUnit};

    use crate::input::scroll::{WheelAmount, wheel_step};
    use crate::version::gate::android::{AndroidApi, ScaledScrollFactor};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum FactorApi {
        ThemeAttribute,
        ViewConfiguration,
    }

    impl FactorApi {
        pub(crate) fn for_api(api: AndroidApi) -> Self {
            match api.since::<ScaledScrollFactor>() {
                Some(_) => Self::ViewConfiguration,
                None => Self::ThemeAttribute,
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub(crate) struct PixelFactors {
        horizontal: f64,
        vertical: f64,
    }

    impl PixelFactors {
        pub(crate) fn from_physical(
            horizontal: f64,
            vertical: f64,
            ratio: DevicePixelRatio,
        ) -> Option<Self> {
            let (horizontal, vertical) = (ratio.to_logical(horizontal), ratio.to_logical(vertical));
            if horizontal.is_finite()
                && vertical.is_finite()
                && horizontal >= 0.0
                && vertical >= 0.0
            {
                Some(Self {
                    horizontal,
                    vertical,
                })
            } else {
                None
            }
        }
    }

    #[derive(Default)]
    pub(crate) struct FactorCache {
        accepted: Option<PixelFactors>,
    }

    impl FactorCache {
        /// A failed refresh retains the accepted logical factors, even across DPI
        /// changes. With no accepted observation the authored compatibility policy
        /// treats an axis unit as one line, without claiming an OS measurement.
        pub(crate) fn observe(&mut self, reading: Option<PixelFactors>) {
            if let Some(reading) = reading {
                self.accepted = Some(reading);
            }
        }

        pub(crate) fn policy(&self) -> AxisPolicy {
            self.accepted
                .map_or(AxisPolicy::AuthoredLines, AxisPolicy::Pixels)
        }
    }

    #[derive(Clone, Copy)]
    pub(crate) enum AxisPolicy {
        Pixels(PixelFactors),
        AuthoredLines,
    }

    impl AxisPolicy {
        /// The step `AXIS_HSCROLL` and `AXIS_VSCROLL` make, by the one wheel
        /// rule: Android's scroll axes move the content. `None` for an axis
        /// value that is not finite; the event is dropped.
        pub(crate) fn delta(self, horizontal: f32, vertical: f32) -> Option<ScrollDelta> {
            let (unit, horizontal, vertical) = match self {
                Self::Pixels(factors) => (
                    ScrollUnit::Pixels,
                    f64::from(horizontal) * factors.horizontal,
                    f64::from(vertical) * factors.vertical,
                ),
                Self::AuthoredLines => (
                    ScrollUnit::Lines,
                    f64::from(horizontal),
                    f64::from(vertical),
                ),
            };
            wheel_step(
                unit,
                WheelAmount::content(horizontal),
                WheelAmount::content(vertical),
            )
            .ok()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn android_axis_factor_policy() {
            for api in [23, 24, 25]
                .map(AndroidApi::from_level)
                .into_iter()
                .chain([AndroidApi::UNKNOWN])
            {
                assert_eq!(FactorApi::for_api(api), FactorApi::ThemeAttribute, "{api}");
            }
            for api in [26, 35, 36].map(AndroidApi::from_level) {
                assert_eq!(
                    FactorApi::for_api(api),
                    FactorApi::ViewConfiguration,
                    "{api}"
                );
            }
            let mut cache = FactorCache::default();
            cache.observe(None);
            let fallback = cache.policy().delta(0.25, -0.5).expect("authored fallback");
            assert_eq!(
                (fallback.unit(), fallback.x(), fallback.y()),
                (ScrollUnit::Lines, -0.25, 0.5)
            );
            cache.observe(PixelFactors::from_physical(
                48.0,
                64.0,
                DevicePixelRatio::new(2.0).expect("density"),
            ));
            for _ in 0..2 {
                cache.observe(None);
                let accepted = cache
                    .policy()
                    .delta(0.25, -0.5)
                    .expect("accepted native factors");
                assert_eq!(
                    (accepted.unit(), accepted.x(), accepted.y()),
                    (ScrollUnit::Pixels, -6.0, 16.0)
                );
            }
            cache.observe(PixelFactors::from_physical(
                12.0,
                24.0,
                DevicePixelRatio::new(3.0).expect("new density"),
            ));
            let recovered = cache.policy().delta(0.25, -0.5).expect("new context");
            assert_eq!((recovered.x(), recovered.y()), (-1.0, 4.0));
            for value in [-1.0, f64::NAN, f64::INFINITY] {
                assert!(PixelFactors::from_physical(value, 1.0, DevicePixelRatio::ONE).is_none());
            }
            let tiny = DevicePixelRatio::new(f64::from_bits(1)).expect("finite density");
            assert!(PixelFactors::from_physical(1.0, 1.0, tiny).is_none());
            assert!(cache.policy().delta(f32::NAN, 0.0).is_none());
        }
    }
}

#[cfg(target_os = "android")]
use std::{
    collections::HashMap,
    sync::Arc,
    sync::atomic::{AtomicBool, Ordering},
};

#[cfg(target_os = "android")]
use android_activity::input::{Axis, KeyAction, MotionAction as NdkAction, ToolType};
#[cfg(target_os = "android")]
use flui_platform::{EventTime, keyboard::Modifiers as OwnedModifiers};
#[cfg(target_os = "android")]
use keyboard_types::{Key, KeyState, Modifiers, NamedKey};
#[cfg(target_os = "android")]
use parking_lot::Mutex;
#[cfg(target_os = "android")]
use ui_events::keyboard::KeyboardEvent;

#[cfg(target_os = "android")]
use crate::{
    input::assembly::NativeCancel,
    input::keys::adapter::keyboard_input,
    owner::android::{ActivityInput, AndroidLoop},
    version::gate::android::AndroidApi,
};
#[cfg(target_os = "android")]
use flui_platform::PlatformInput;
#[cfg(target_os = "android")]
use motion::{
    AndroidMotion, Axes, DeviceCapabilities, DeviceDiscovery, Motion, MotionAction, PointerFacts,
    Tool, axis, needs_device_query,
};

/// `AXIS_GESTURE_SCROLL_X_DISTANCE` (API 34).
#[cfg(target_os = "android")]
const AXIS_GESTURE_SCROLL_X_DISTANCE: u32 = 50;
/// `AXIS_GESTURE_SCROLL_Y_DISTANCE` (API 34).
#[cfg(target_os = "android")]
const AXIS_GESTURE_SCROLL_Y_DISTANCE: u32 = 51;
/// `AXIS_GESTURE_PINCH_SCALE_FACTOR` (API 34).
#[cfg(target_os = "android")]
const AXIS_GESTURE_PINCH_SCALE_FACTOR: u32 = 52;
/// `AXIS_GESTURE_SWIPE_FINGER_COUNT` (API 34).
#[cfg(target_os = "android")]
const AXIS_GESTURE_SWIPE_FINGER_COUNT: u32 = 53;
/// `MotionEvent.FLAG_CANCELED` (API 33).
#[cfg(target_os = "android")]
const FLAG_CANCELED: u32 = 0x20;

#[derive(Clone)]
#[cfg(target_os = "android")]
struct CachedDevice {
    capabilities: DeviceCapabilities,
    // The cache and a query snapshot share this global reference. Platform is
    // Send + Sync, so Rc cannot represent that ownership here.
    object: Arc<jni::refs::Global<jni::objects::JObject<'static>>>,
}

#[cfg(target_os = "android")]
enum DeviceReading {
    Present {
        cached: CachedDevice,
        replaced: bool,
    },
    Removed,
    Unavailable,
}

/// Query Java's capability metadata before entering the owner-local state guard.
/// InputManager replaces its cached Java object when a device is reconfigured.
/// JNI object identity avoids descriptors (which can be duplicated) and hidden APIs.
/// For a move the cached reading stands ([`needs_device_query`]): the
/// per-event attach and lookup would run at the touch sample rate.
#[cfg(target_os = "android")]
fn motion_device(
    app: &android_activity::AndroidApp,
    event: &android_activity::input::MotionEvent<'_>,
    cached: Option<CachedDevice>,
) -> DeviceReading {
    use jni::{JValue, JavaVM, jni_sig, jni_str};

    if let Some(cached) = cached.as_ref()
        && !needs_device_query(
            Some(cached.capabilities),
            u32::from(event.source()),
            action(event.action()),
        )
    {
        return DeviceReading::Present {
            cached: cached.clone(),
            replaced: false,
        };
    }
    // SAFETY: AndroidApp owns the running Android VM for its entire lifetime.
    // JavaVM is a borrowed VM handle; constructing it does not destroy the VM.
    #[expect(
        unsafe_code,
        reason = "AndroidApp supplies the live VM handle required by JNI"
    )]
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let result = vm.attach_current_thread(|env| -> jni::errors::Result<_> {
        let result = (|| {
            let device = env
                .call_static_method(
                    jni_str!("android/view/InputDevice"),
                    jni_str!("getDevice"),
                    jni_sig!("(I)Landroid/view/InputDevice;"),
                    &[JValue::Int(event.device_id())],
                )?
                .l()?;
            if device.is_null() {
                return Ok(DeviceReading::Removed);
            }
            let source = u32::from(event.source());
            let replaced = match cached.as_ref() {
                Some(cached) => !env.is_same_object(&device, cached.object.as_ref())?,
                None => false,
            };
            if let Some(cached) = cached.as_ref()
                && !replaced
                && cached.capabilities.source == source
            {
                return Ok(DeviceReading::Present {
                    cached: cached.clone(),
                    replaced,
                });
            }
            let mut reported = Vec::with_capacity(axis::SENSORS.len());
            for axis in axis::SENSORS {
                let range = env
                    .call_method(
                        &device,
                        jni_str!("getMotionRange"),
                        jni_sig!("(II)Landroid/view/InputDevice$MotionRange;"),
                        &[
                            JValue::Int(
                                i32::try_from(axis).expect("BUG: Android sensor axes fit i32"),
                            ),
                            JValue::Int(i32::from_ne_bytes(source.to_ne_bytes())),
                        ],
                    )?
                    .l()?;
                if !range.is_null() {
                    reported.push(axis);
                }
            }
            Ok(DeviceReading::Present {
                cached: CachedDevice {
                    capabilities: DeviceCapabilities::new(source, reported),
                    object: Arc::new(env.new_global_ref(&device)?),
                },
                replaced,
            })
        })();
        if result.is_err() {
            // Capability discovery is optional. Do not leave an exception pending
            // on the event-loop thread or manufacture readings after a failure.
            env.exception_clear();
        }
        result
    });
    result.unwrap_or(DeviceReading::Unavailable)
}

/// The Android host's input, as the loop holds it ([`ActivityInput`]): the
/// translation of the Activity's queue with the Java device objects its
/// capability cache compares by identity, and the scroll factors. No user
/// callback lives here.
#[derive(Default)]
#[cfg(target_os = "android")]
pub(crate) struct AndroidInput {
    state: Mutex<AndroidInputState>,
    scroll_factors: Mutex<scroll::FactorCache>,
}

#[cfg(target_os = "android")]
impl ActivityInput for AndroidInput {
    /// The first `MotionEvent` of the process is logged at `info`
    /// (`FIRST_MOTION_SEEN`), every later one at `debug`.
    fn motion(
        &self,
        app: &android_activity::AndroidApp,
        event: &android_activity::input::MotionEvent<'_>,
        scale: f64,
    ) -> Vec<PlatformInput> {
        /// Whether any `MotionEvent` has reached the input in this process.
        static FIRST_MOTION_SEEN: AtomicBool = AtomicBool::new(false);

        let scroll_policy = self.scroll_factors.lock().policy();
        let cached = self.state.lock().capabilities(event.device_id());
        let device = motion_device(app, event, cached);
        let events = self
            .state
            .lock()
            .convert_motion_event(event, scale, scroll_policy, device);
        // The one place a touch is visible between the OS and the framework:
        // a tap that changes nothing on screen is diagnosed from these lines
        // (present → the framework's hit test or arena; absent → the queue
        // never delivered it). The FIRST motion event of the process is an
        // `info` line — a lifecycle fact like "first frame rendered", visible
        // under the default `info` filter — and every one after it is
        // `debug`, so a drag does not flood logcat.
        let first = !FIRST_MOTION_SEEN.swap(true, Ordering::Relaxed);
        let pointer = event.pointer_at_index(0);
        // One field list, two levels: `tracing`'s callsite level must be a
        // constant, so the level is pasted by a local macro rather than
        // chosen at runtime.
        macro_rules! motion_event {
            ($level:expr) => {
                tracing::event!(
                    target: "flui_native::android::input",
                    $level,
                    action = ?event.action(),
                    pointers = event.pointer_count(),
                    converted = events.len(),
                    x = pointer.x(),
                    y = pointer.y(),
                    scale_factor = scale,
                    first,
                    "motion event reached the input queue"
                )
            };
        }
        if first {
            motion_event!(tracing::Level::INFO);
        } else {
            motion_event!(tracing::Level::DEBUG);
        }
        events
    }

    fn touch_exploration(
        &self,
        event: &android_activity::input::MotionEvent<'_>,
    ) -> Option<(i32, f32, f32)> {
        let pointer = event.pointer_at_index(0);
        let code = motion::touch_exploration(
            action(event.action()),
            tool(pointer.tool_type()),
            u32::from(event.source()),
        )?;
        Some((code, pointer.x(), pointer.y()))
    }

    fn key(
        &self,
        app: &android_activity::AndroidApp,
        event: &android_activity::input::KeyEvent<'_>,
        composing: bool,
    ) -> Option<PlatformInput> {
        convert_key_event(app, event, composing)
    }

    fn finish_drain(&self) -> Vec<PlatformInput> {
        self.state.lock().finish_drain()
    }

    fn focus_lost(&self) -> Vec<PlatformInput> {
        self.state.lock().cancel_contacts(NativeCancel::FocusLost)
    }

    fn capture_lost(&self) -> Vec<PlatformInput> {
        self.state.lock().cancel_contacts(NativeCancel::CaptureLost)
    }

    fn refresh_scroll_factors(&self, app: &android_activity::AndroidApp, api: AndroidApi) {
        // Query outside the cache's guard. A failed refresh keeps accepted
        // logical factors; only the never-observed case uses authored line
        // normalization for compatibility with supported API21 devices.
        let reading = scroll_factors(app, api).ok();
        self.scroll_factors.lock().observe(reading);
    }
}

/// The input position of the Android host's table: the loop answers it.
#[cfg(target_os = "android")]
impl crate::host::Input<AndroidLoop> for AndroidLoop {
    fn keyboard_layout(_: &AndroidLoop) -> String {
        // Android reports no keyboard layout identifier: empty means unknown.
        String::new()
    }

    fn on_keyboard_layout_change(_: &AndroidLoop, callback: Box<dyn FnMut() + Send>) {
        // Nothing signals a layout change, so the callback is never called.
        drop(callback);
    }
}

/// The translation of the Activity's queue, and the Java device objects its
/// capability cache compares by identity.
#[derive(Default)]
#[cfg(target_os = "android")]
struct AndroidInputState {
    motion: AndroidMotion,
    devices: HashMap<i32, CachedDevice>,
}

#[cfg(target_os = "android")]
impl AndroidInputState {
    fn cancel_contacts(&mut self, reason: NativeCancel) -> Vec<PlatformInput> {
        self.motion.cancel_contacts(reason)
    }

    /// What the translation held that is due by the end of this drain.
    fn finish_drain(&mut self) -> Vec<PlatformInput> {
        self.motion.finish_drain(web_time::Instant::now())
    }

    fn capabilities(&self, id: i32) -> Option<CachedDevice> {
        self.devices.get(&id).cloned()
    }

    fn convert_motion_event(
        &mut self,
        event: &android_activity::input::MotionEvent<'_>,
        scale_factor: f64,
        scroll_policy: scroll::AxisPolicy,
        reading: DeviceReading,
    ) -> Vec<PlatformInput> {
        let id = event.device_id();
        let discovery = match reading {
            DeviceReading::Present { cached, replaced } => {
                let capabilities = cached.capabilities;
                self.devices.insert(id, cached);
                DeviceDiscovery::Present {
                    capabilities,
                    replaced,
                }
            }
            DeviceReading::Removed => {
                self.devices.remove(&id);
                DeviceDiscovery::Removed
            }
            DeviceReading::Unavailable => DeviceDiscovery::Unavailable,
        };
        let pointers: Vec<PointerFacts> = event
            .pointers()
            .map(|pointer| PointerFacts {
                id: pointer.pointer_id(),
                tool: tool(pointer.tool_type()),
                axes: axes(&|axis| pointer.axis_value(axis)),
                history: pointer
                    .history()
                    .filter_map(|past| {
                        let time = u64::try_from(past.event_time())
                            .ok()
                            .map(EventTime::from_nanos)?;
                        Some((time, axes(&|axis| past.axis_value(axis))))
                    })
                    .collect(),
            })
            .collect();
        let motion = Motion {
            device: id,
            source: u32::from(event.source()),
            action: action(event.action()),
            action_index: event.pointer_index(),
            action_button: u32::from(event.action_button()),
            buttons: event.button_state().0,
            canceled: event.flags().0 & FLAG_CANCELED != 0,
            modifiers: owned_modifiers(event.meta_state()),
            time: u64::try_from(event.event_time())
                .ok()
                .map(EventTime::from_nanos),
            pointers: &pointers,
        };
        // The accepted activity-context policy already projects native
        // factors into logical pixels exactly once.
        self.motion.decode(
            &motion,
            scale_factor,
            &|horizontal, vertical| scroll_policy.delta(horizontal, vertical),
            discovery,
            web_time::Instant::now(),
        )
    }
}

#[cfg(target_os = "android")]
fn action(action: NdkAction) -> MotionAction {
    match action {
        NdkAction::Down => MotionAction::Down,
        NdkAction::Up => MotionAction::Up,
        NdkAction::Move => MotionAction::Move,
        NdkAction::Cancel => MotionAction::Cancel,
        NdkAction::PointerDown => MotionAction::PointerDown,
        NdkAction::PointerUp => MotionAction::PointerUp,
        NdkAction::HoverEnter => MotionAction::HoverEnter,
        NdkAction::HoverMove => MotionAction::HoverMove,
        NdkAction::HoverExit => MotionAction::HoverExit,
        NdkAction::Scroll => MotionAction::Scroll,
        NdkAction::ButtonPress => MotionAction::ButtonPress,
        NdkAction::ButtonRelease => MotionAction::ButtonRelease,
        _ => MotionAction::Other,
    }
}

#[cfg(target_os = "android")]
fn tool(tool: ToolType) -> Tool {
    match tool {
        ToolType::Finger => Tool::Finger,
        ToolType::Stylus => Tool::Stylus,
        ToolType::Eraser => Tool::Eraser,
        ToolType::Mouse => Tool::Mouse,
        _ => Tool::Unknown,
    }
}

/// The axes the translation reads, from `value`.
#[cfg(target_os = "android")]
fn axes(value: &dyn Fn(Axis) -> f32) -> Axes {
    Axes {
        x: value(Axis::X),
        y: value(Axis::Y),
        pressure: value(Axis::Pressure),
        touch_major: value(Axis::TouchMajor),
        touch_minor: value(Axis::TouchMinor),
        tilt: value(Axis::Tilt),
        orientation: value(Axis::Orientation),
        hscroll: value(Axis::Hscroll),
        vscroll: value(Axis::Vscroll),
        gesture_scroll_x: value(Axis::from(AXIS_GESTURE_SCROLL_X_DISTANCE)),
        gesture_scroll_y: value(Axis::from(AXIS_GESTURE_SCROLL_Y_DISTANCE)),
        pinch_scale: value(Axis::from(AXIS_GESTURE_PINCH_SCALE_FACTOR)),
        swipe_fingers: value(Axis::from(AXIS_GESTURE_SWIPE_FINGER_COUNT)),
    }
}

#[cfg(target_os = "android")]
fn owned_modifiers(meta: android_activity::input::MetaState) -> OwnedModifiers {
    let native = convert_meta_state(meta);
    let mut owned = OwnedModifiers::NONE;
    for (from, to) in [
        (Modifiers::SHIFT, OwnedModifiers::SHIFT),
        (Modifiers::ALT, OwnedModifiers::ALT),
        (Modifiers::CONTROL, OwnedModifiers::CONTROL),
        (Modifiers::META, OwnedModifiers::META),
        (Modifiers::CAPS_LOCK, OwnedModifiers::CAPS_LOCK),
        (Modifiers::NUM_LOCK, OwnedModifiers::NUM_LOCK),
    ] {
        if native.contains(from) {
            owned |= to;
        }
    }
    owned
}

/// Convert an Android `KeyEvent` to a `PlatformInput`. `composing` is whether
/// the input method composes now: a key it passed on during a composition is
/// still its own ([`keys::key_composition`]).
#[cfg(target_os = "android")]
fn convert_key_event(
    app: &android_activity::AndroidApp,
    event: &android_activity::input::KeyEvent<'_>,
    composing: bool,
) -> Option<PlatformInput> {
    let action = event.action();
    let state = match action {
        KeyAction::Up => KeyState::Up,
        // Multiple = auto-repeat; treat as Down
        KeyAction::Down | KeyAction::Multiple => KeyState::Down,
        _ => return None,
    };

    // Keycode implements Into<u32> via num_enum::IntoPrimitive
    let keycode_i32 = i32::try_from(u32::from(event.key_code())).ok()?;
    let code = ui_events::keyboard::android::keycode_to_code(keycode_i32);
    let named_key = match ui_events::keyboard::android::keycode_to_named_key(keycode_i32) {
        NamedKey::Unidentified => keys::unnamed_key(keycode_i32)
            .and_then(|named| named.as_str().parse().ok())
            .unwrap_or(NamedKey::Unidentified),
        named => named,
    };
    let location = ui_events::keyboard::android::keycode_to_location(keycode_i32);

    let mapped = if named_key == NamedKey::Unidentified {
        let lookup =
            android_activity::input::MetaState(keys::character_meta_state(event.meta_state().0));
        app.device_key_character_map(event.device_id())
            .and_then(|map| map.get(event.key_code(), lookup))
            .ok()
    } else {
        None
    };
    let modifiers = convert_meta_state(event.meta_state());
    let key = mapped_key(named_key, mapped, keycode_i32, modifiers);
    let repeat = event.repeat_count() > 0;

    Some(keyboard_input(
        KeyboardEvent {
            state,
            key,
            code,
            location,
            modifiers,
            repeat,
            is_composing: keys::key_composition(composing)
                == flui_platform::keyboard::ImeComposition::Active,
        },
        u64::try_from(event.event_time()).ok()?,
    ))
}

#[cfg(target_os = "android")]
fn mapped_key(
    named: NamedKey,
    mapped: Option<android_activity::input::KeyMapChar>,
    keycode: i32,
    modifiers: Modifiers,
) -> Key {
    use android_activity::input::KeyMapChar;
    if named != NamedKey::Unidentified {
        return Key::Named(named);
    }
    match mapped {
        Some(KeyMapChar::Unicode(character)) => Key::Character(character.to_string()),
        Some(KeyMapChar::CombiningAccent(_)) => Key::Named(NamedKey::Dead),
        Some(KeyMapChar::None) => Key::Named(NamedKey::Unidentified),
        None => fallback_character(keycode, modifiers)
            .map_or(Key::Named(NamedKey::Unidentified), |character| {
                Key::Character(character.to_string())
            }),
    }
}

/// Legacy US keycode fallback only when the native map lookup fails.
#[cfg(target_os = "android")]
fn fallback_character(keycode: i32, modifiers: Modifiers) -> Option<char> {
    use ui_events::keyboard::android::{
        KEYCODE_0, KEYCODE_9, KEYCODE_A, KEYCODE_APOSTROPHE, KEYCODE_AT, KEYCODE_BACKSLASH,
        KEYCODE_COMMA, KEYCODE_EQUALS, KEYCODE_GRAVE, KEYCODE_LEFT_BRACKET, KEYCODE_MINUS,
        KEYCODE_PERIOD, KEYCODE_PLUS, KEYCODE_POUND, KEYCODE_RIGHT_BRACKET, KEYCODE_SEMICOLON,
        KEYCODE_SLASH, KEYCODE_SPACE, KEYCODE_STAR, KEYCODE_Z,
    };
    let shift = modifiers.contains(Modifiers::SHIFT);
    if (KEYCODE_A..=KEYCODE_Z).contains(&keycode) {
        let base = if shift ^ modifiers.contains(Modifiers::CAPS_LOCK) {
            b'A'
        } else {
            b'a'
        };
        return u8::try_from(keycode - KEYCODE_A)
            .ok()
            .map(|offset| char::from(base + offset));
    }
    if (KEYCODE_0..=KEYCODE_9).contains(&keycode) {
        let offset = usize::try_from(keycode - KEYCODE_0).ok()?;
        return Some(char::from(if shift {
            b")!@#$%^&*("[offset]
        } else {
            b"0123456789"[offset]
        }));
    }
    let (plain, shifted) = match keycode {
        KEYCODE_SPACE => (' ', ' '),
        KEYCODE_COMMA => (',', '<'),
        KEYCODE_PERIOD => ('.', '>'),
        KEYCODE_MINUS => ('-', '_'),
        KEYCODE_EQUALS => ('=', '+'),
        KEYCODE_LEFT_BRACKET => ('[', '{'),
        KEYCODE_RIGHT_BRACKET => (']', '}'),
        KEYCODE_BACKSLASH => ('\\', '|'),
        KEYCODE_SEMICOLON => (';', ':'),
        KEYCODE_APOSTROPHE => ('\'', '"'),
        KEYCODE_SLASH => ('/', '?'),
        KEYCODE_GRAVE => ('`', '~'),
        KEYCODE_AT => ('@', '@'),
        KEYCODE_PLUS => ('+', '+'),
        KEYCODE_POUND => ('#', '#'),
        KEYCODE_STAR => ('*', '*'),
        _ => return None,
    };
    Some(if shift { shifted } else { plain })
}

/// Convert Android `MetaState` to `keyboard_types::Modifiers`.
#[cfg(target_os = "android")]
fn convert_meta_state(meta: android_activity::input::MetaState) -> Modifiers {
    // MetaState is a newtype wrapping u32: MetaState(pub u32)
    let bits: u32 = meta.0;
    let mut mods = Modifiers::empty();

    // Android meta state flags
    const META_SHIFT: u32 = 0x01;
    const META_ALT: u32 = 0x02;
    const META_CTRL: u32 = 0x1000;
    const META_META: u32 = 0x10000;
    const META_CAPS_LOCK: u32 = 0x0010_0000;
    const META_NUM_LOCK: u32 = 0x0020_0000;

    if bits & META_SHIFT != 0 {
        mods |= Modifiers::SHIFT;
    }
    if bits & META_ALT != 0 {
        mods |= Modifiers::ALT;
    }
    if bits & META_CTRL != 0 {
        mods |= Modifiers::CONTROL;
    }
    if bits & META_META != 0 {
        mods |= Modifiers::META;
    }
    if bits & META_CAPS_LOCK != 0 {
        mods |= Modifiers::CAPS_LOCK;
    }
    if bits & META_NUM_LOCK != 0 {
        mods |= Modifiers::NUM_LOCK;
    }

    mods
}

/// The distance one unit of a wheel or a scrolling axis moves: the public
/// API26 factors, or the public theme attribute on API21–25, projected through
/// the same Activity resource metrics that produced the measurement. The loop
/// reads it with the preferences, on the same events.
#[cfg(target_os = "android")]
fn scroll_factors(
    app: &android_activity::AndroidApp,
    api: crate::version::gate::android::AndroidApi,
) -> Result<scroll::PixelFactors, flui_platform::PreferenceQueryError> {
    use crate::owner::android::activity::with_activity;
    use flui_foundation::geometry::DevicePixelRatio;
    use flui_platform::PreferenceQueryError;
    use jni::{JValue, jni_sig, jni_str};
    use scroll::{FactorApi, PixelFactors};

    let factors = with_activity(app, |env, activity| {
        let resources = env
            .call_method(
                activity,
                jni_str!("getResources"),
                jni_sig!("()Landroid/content/res/Resources;"),
                &[],
            )?
            .l()?;
        let metrics = env
            .call_method(
                &resources,
                jni_str!("getDisplayMetrics"),
                jni_sig!("()Landroid/util/DisplayMetrics;"),
                &[],
            )?
            .l()?;
        let density = env
            .get_field(&metrics, jni_str!("density"), jni_sig!("F"))?
            .f()?;
        if FactorApi::for_api(api) == FactorApi::ThemeAttribute {
            let value =
                env.new_object(jni_str!("android/util/TypedValue"), jni_sig!("()V"), &[])?;
            let theme = env
                .call_method(
                    activity,
                    jni_str!("getTheme"),
                    jni_sig!("()Landroid/content/res/Resources$Theme;"),
                    &[],
                )?
                .l()?;
            let attribute = env
                .get_static_field(
                    jni_str!("android/R$attr"),
                    jni_str!("listPreferredItemHeight"),
                    jni_sig!("I"),
                )?
                .i()?;
            let resolved = env
                .call_method(
                    &theme,
                    jni_str!("resolveAttribute"),
                    jni_sig!("(ILandroid/util/TypedValue;Z)Z"),
                    &[
                        JValue::Int(attribute),
                        JValue::Object(&value),
                        JValue::Bool(true),
                    ],
                )?
                .z()?;
            if !resolved {
                return Ok(None);
            }
            let factor = env
                .call_method(
                    &value,
                    jni_str!("getDimension"),
                    jni_sig!("(Landroid/util/DisplayMetrics;)F"),
                    &[JValue::Object(&metrics)],
                )?
                .f()?;
            return Ok(Some((
                f64::from(factor),
                f64::from(factor),
                f64::from(density),
            )));
        }
        let config = env
            .call_static_method(
                jni_str!("android/view/ViewConfiguration"),
                jni_str!("get"),
                jni_sig!("(Landroid/content/Context;)Landroid/view/ViewConfiguration;"),
                &[JValue::Object(activity)],
            )?
            .l()?;
        let x = env
            .call_method(
                &config,
                jni_str!("getScaledHorizontalScrollFactor"),
                jni_sig!("()F"),
                &[],
            )?
            .f()?;
        let y = env
            .call_method(
                &config,
                jni_str!("getScaledVerticalScrollFactor"),
                jni_sig!("()F"),
                &[],
            )?
            .f()?;
        Ok(Some((f64::from(x), f64::from(y), f64::from(density))))
    })?;
    let (horizontal, vertical, density) = factors.ok_or_else(|| PreferenceQueryError::Native {
        message: "Activity theme has no scroll factor observation".into(),
    })?;
    let ratio =
        DevicePixelRatio::new(density).ok_or(flui_platform::InvalidPreference::PixelRatio)?;
    PixelFactors::from_physical(horizontal, vertical, ratio).ok_or_else(|| {
        PreferenceQueryError::Native {
            message: "Android returned invalid scroll factors".into(),
        }
    })
}

#[cfg(all(target_os = "android", test))]
mod tests {
    use super::*;

    #[test]
    fn android_native_key_map_preserves_logical_keys() {
        use android_activity::input::KeyMapChar;
        for (named, mapped, expected) in [
            (
                NamedKey::Unidentified,
                Some(KeyMapChar::Unicode('A')),
                Key::Character("A".into()),
            ),
            (
                NamedKey::Unidentified,
                Some(KeyMapChar::Unicode('Я')),
                Key::Character("Я".into()),
            ),
            (
                NamedKey::Unidentified,
                Some(KeyMapChar::Unicode('!')),
                Key::Character("!".into()),
            ),
            (
                NamedKey::Unidentified,
                Some(KeyMapChar::CombiningAccent('´')),
                Key::Named(NamedKey::Dead),
            ),
            (
                NamedKey::Unidentified,
                Some(KeyMapChar::None),
                Key::Named(NamedKey::Unidentified),
            ),
            (NamedKey::Unidentified, None, Key::Character("a".into())),
            (NamedKey::Enter, None, Key::Named(NamedKey::Enter)),
        ] {
            assert_eq!(
                mapped_key(
                    named,
                    mapped,
                    ui_events::keyboard::android::KEYCODE_A,
                    Modifiers::empty()
                ),
                expected
            );
        }
        for (code, modifiers, expected) in [
            (29, Modifiers::SHIFT, "A"),
            (54, Modifiers::CAPS_LOCK, "Z"),
            (29, Modifiers::SHIFT | Modifiers::CAPS_LOCK, "a"),
            (8, Modifiers::SHIFT, "!"),
            (11, Modifiers::empty(), "4"),
            (76, Modifiers::SHIFT, "?"),
        ] {
            assert_eq!(
                mapped_key(NamedKey::Unidentified, None, code, modifiers),
                Key::Character(expected.into())
            );
        }
    }
}
