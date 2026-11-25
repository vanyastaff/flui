//! The Windows lane: real input into an app on the user's desktop, what UI Automation shows of it,
//! the compositor's capture of its window, and the job that ends it with the driver.
//!
//! - **Input.** `SendInput` for the mouse, the wheel and keys, each key with its scan code; text
//!   as Unicode packets (`VK_PACKET`, [`keys::text_strokes`]); touch and pen through a synthetic
//!   pointer device (`InjectSyntheticPointerInput`, [`contacts`]). Every event carries the lane's
//!   tag ([`activity::LANE_TAG`]). Positions are exact device pixels: the lane runs per-monitor DPI
//!   aware and maps with [`screen`]. A scroll of `n` device pixels turns the wheel by `n` wheel
//!   units (120 a notch), which the app turns into a distance of its own, so a scroll's distance
//!   is not compared, only that it arrived. An input-method composition goes through the app's own
//!   input path (`input.inject`): a text service's composition is out of the lane's reach. A pause
//!   is waited out whole, watching the guard as it waits.
//! - **One path for keys.** A key event and typed text alike become keystrokes planned against
//!   what the lane holds ([`held`]): the shell-hotkey rule reads those strokes before a sequence,
//!   and the lane sends them, reading the desktop again before each one.
//! - **Safety.** Before every event, and every keystroke, the rules of [`safety`] are read again:
//!   the desktop takes input, the window is the app's and in front, the point reaches it, nothing
//!   physical is held; before a sequence, `Lane::check` refuses a key the lane has no code for, a
//!   device or button it does not send, and a chord the OS shell takes. What the lane pressed is
//!   released on every exit, and what Windows refused to release stays held to be released again.
//! - **The guard** (`WindowsGuard`, on Windows) watches the low-level hooks and the digitizers' raw
//!   input for input the lane did not send ([`activity`]); a guard that stopped seeing the lane's
//!   own input ends the run, since it would no longer see the person's.
//! - **Reading.** [`roles`] is the role table UI Automation is read against; the lane projects the
//!   window's UI Automation tree onto the wire's nodes (`Lane::read_tree`) through the bounded
//!   [`walk`](crate::lane::walk).
//! - **Process.** A launched app runs in a kill-on-close job (`job`, on Windows), which it joins
//!   before its first instruction.
//!
//! The pure parts (the key, role, coordinate and activity tables, the safety rules, the held keys
//! and buttons, the contact frames and what a `SendInput` call answers) build on every host, so
//! their tables run on Linux CI; the parts that call Windows are `cfg(windows)`.

pub mod activity;
pub mod contacts;
pub mod held;
pub mod keys;
pub mod roles;
pub mod safety;
pub mod screen;
pub mod send_input;

#[cfg(windows)]
mod capture;
#[cfg(windows)]
mod guard;
#[cfg(windows)]
pub mod job;
#[cfg(windows)]
mod uia;
#[cfg(windows)]
mod win32;

#[cfg(windows)]
pub use guard::{STOPPED_WATCHING, WindowsGuard};
#[cfg(windows)]
pub use lane::WindowsLane;

/// Whether the calling thread's desktop is the one taking the user's input: no when the
/// workstation is locked, the session is disconnected, or another desktop is switched in (a test
/// desktop among them). The lane sends nothing off that desktop, and says so as a lane the host
/// cannot run.
#[cfg(windows)]
#[must_use]
pub fn desktop_takes_input() -> bool {
    win32::on_input_desktop()
}

/// Release everything the lane holds: the keys through `send_stroke`, then the mouse buttons
/// through `release_button`, then the touch and pen contacts through `inject`, every part tried
/// whatever the one before it came to, a panic included.
///
/// # Errors
///
/// What [`release_outcome`](crate::lane::release_outcome) makes of the three releases:
/// [`Error::ReleaseFailed`](crate::Error::ReleaseFailed) naming everything still held.
///
/// # Panics
///
/// The first part's panic, resumed once every part was tried and what the others leave held is
/// logged at error level; discarded instead when the thread is already unwinding.
pub fn release_held(
    keys: &mut held::HeldKeys,
    buttons: &mut held::HeldButtons,
    contacts: &mut contacts::Contacts,
    send_stroke: impl FnMut(held::Keystroke) -> crate::Result<()>,
    release_button: impl FnMut(u8) -> crate::Result<()>,
    inject: impl FnMut(&contacts::Frame) -> crate::Result<()>,
) -> crate::Result<()> {
    let mut panicked = flui_foundation::panic::FirstPanic::new();
    let keys = panicked.contain(|| keys.release_all(send_stroke));
    let buttons = panicked.contain(|| buttons.release_all(release_button));
    let lifted = panicked.contain(|| contacts.release_all(inject));
    crate::lane::release_settled(panicked, [keys, buttons, lifted])
}

#[cfg(windows)]
mod lane {
    use std::time::Instant;

    use flui_protocol::input::{PointerButton, PointerDevice};
    use flui_protocol::session::{Isolation, Placement};
    use flui_protocol::tree::Tree;

    use super::contacts::{Contacts, PointerKind};
    use super::guard::WindowsGuard;
    use super::held::{HeldButtons, HeldKeys, KeyInput, Keystroke};
    use super::keys::{self, KeyName, VirtualKey};
    use super::safety::{self, Desk, Held, Landing, Refusal};
    use super::win32::{self, DeviceUnits, Hwnd, PointerDevice as SyntheticDevice};
    use super::{capture, screen, send_input, uia};
    use crate::coordinates::{DevicePoint, WindowGeometry};
    use crate::error::{Error, Result};
    use crate::lane::pacing::{ECHO_WITHIN, slices};
    use crate::lane::{
        AccessibilityApi, ActingInput, ActivityGuard, Delivered, Lane, LaneCapabilities,
        PositionFidelity, SendFailure,
    };
    use crate::steps::OsEvent;

    /// The Windows lane for the app whose process is `pid`: see the
    /// [module docs](super).
    #[derive(Debug)]
    pub struct WindowsLane {
        pid: u32,
        window: Option<Hwnd>,
        guard: WindowsGuard,
        keys: HeldKeys,
        buttons: HeldButtons,
        contacts: Contacts,
        touch: Option<SyntheticDevice>,
        pen: Option<SyntheticDevice>,
    }

    /// Wait for `guard` to have seen more of the lane's own events than `before`: a hook Windows
    /// removed sees no more input, the person's included. A guard that is not watching waits for
    /// nothing.
    fn echo_since(guard: &WindowsGuard, before: Option<u64>) -> Result<()> {
        let Some(before) = before else {
            return Ok(());
        };
        let deadline = Instant::now() + ECHO_WITHIN;
        while guard.echoes().is_some_and(|seen| seen <= before) {
            if Instant::now() >= deadline {
                return Err(Error::Lane {
                    lane: safety::LANE.to_owned(),
                    unavailable: true,
                    message: format!(
                        "the user-activity guard's hook did not see the lane's own input within \
                         {} ms: Windows may have removed it, and it would not see yours",
                        ECHO_WITHIN.as_millis()
                    ),
                });
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Ok(())
    }

    /// The synthetic device of `kind` in `slot`, made on first use.
    fn device(slot: &mut Option<SyntheticDevice>, kind: PointerKind) -> Result<&SyntheticDevice> {
        if slot.as_ref().is_none_or(|device| device.kind() != kind) {
            *slot = Some(SyntheticDevice::create(kind)?);
        }
        Ok(slot.as_ref().expect("BUG: the device was made just above"))
    }

    impl WindowsLane {
        /// A lane driving the app whose process is `pid` in `isolation`: see
        /// [`safety::admit_isolation`] for the modes it runs in.
        ///
        /// # Errors
        ///
        /// As [`safety::admit_isolation`].
        pub fn try_new(pid: u32, isolation: Isolation) -> Result<Self> {
            safety::admit_isolation(isolation).map_err(Refusal::into_error)?;
            Ok(Self::for_process(pid))
        }

        /// A lane driving the app whose process is `pid` (`Driver::app_pid`) on the shared
        /// desktop this process runs on, announced and guarded.
        #[must_use]
        pub fn for_process(pid: u32) -> Self {
            Self {
                pid,
                window: None,
                guard: WindowsGuard::new(),
                keys: HeldKeys::new(),
                buttons: HeldButtons::default(),
                contacts: Contacts::new(),
                touch: None,
                pen: None,
            }
        }

        /// The app's window `placement` describes, among the process's visible windows.
        fn find(&mut self, placement: &Placement) -> Result<(Hwnd, DevicePoint)> {
            let mut found = Vec::new();
            for window in win32::process_windows(self.pid)? {
                let Ok((origin, width, height, dpi)) = win32::client_area(window) else {
                    continue;
                };
                if screen::is_window(width, height, dpi, placement.size, placement.scale) {
                    found.push((window, origin));
                }
            }
            match found.as_slice() {
                [(window, origin)] => {
                    self.window = Some(*window);
                    Ok((*window, *origin))
                }
                [] => Err(Error::not_found(format!(
                    "process {} shows no window of {} x {} at scale {}",
                    self.pid, placement.size.width, placement.size.height, placement.scale
                ))),
                several => Err(win32::lane_error(format!(
                    "process {} shows {} windows of {} x {} at scale {}; the lane cannot tell \
                     which is {}",
                    self.pid,
                    several.len(),
                    placement.size.width,
                    placement.size.height,
                    placement.scale,
                    placement.window
                ))),
            }
        }

        fn target(&self) -> Result<Hwnd> {
            self.window.ok_or_else(|| {
                win32::lane_error(
                    "the lane has no target window: it sends nothing before it locates one",
                )
            })
        }

        /// The desktop as the next event finds it, the app's window brought to the front once
        /// when another is there.
        fn desk(&self, window: Hwnd) -> Desk {
            let mut desk = Desk::quiet();
            desk.input_desktop = super::desktop_takes_input();
            if !desk.input_desktop {
                return desk;
            }
            desk.window_alive = win32::window_pid(window) == Some(self.pid);
            if win32::foreground() != Some(window) && desk.window_alive {
                win32::bring_to_front(window);
            }
            let front = win32::foreground();
            desk.in_front = front == Some(window);
            desk.foreground = front.map(win32::describe);
            desk.buttons = win32::buttons_down();
            desk.modifiers = win32::modifiers_down();
            desk.other_key = win32::other_key_down(self.keys.held());
            desk
        }

        /// What the lane holds, as the safety rules read it.
        fn held(&self) -> Held {
            Held {
                buttons: self.buttons.bits(),
                keys: self.keys.held().to_vec(),
            }
        }

        /// The safety rules over the desktop as it is now, for an event landing at `landing`.
        fn checked(&self, window: Hwnd, landing: Option<DevicePoint>) -> Result<()> {
            let desk = self.desk(window);
            let landing = if desk.input_desktop {
                landing.map(|point| {
                    let under = win32::root_at(point);
                    Landing::new(point, under == Some(window), under.map(win32::describe))
                })
            } else {
                None
            };
            safety::check_event(&desk, landing.as_ref(), &self.held()).map_err(Refusal::into_error)
        }

        /// Where a pointer `event` lands.
        fn landing_of(&self, event: &OsEvent) -> Option<DevicePoint> {
            match event {
                OsEvent::PointerMove { to, .. } => Some(*to),
                OsEvent::Scroll { at, .. } => Some(*at),
                OsEvent::PointerDown {
                    pointer, device, ..
                }
                | OsEvent::PointerUp {
                    pointer, device, ..
                } => match kind_of(*device) {
                    Some(kind) => self.contacts.position(pointer, kind),
                    None => win32::cursor(),
                },
                _ => None,
            }
        }

        /// Send one `SendInput` event that holds nothing (a pointer move, a wheel turn) through
        /// `send`, then wait for the guard to see it come back.
        fn echoed(&self, send: impl FnOnce() -> Result<()>) -> Result<()> {
            let before = self.guard.echoes();
            send()?;
            echo_since(&self.guard, before)
        }

        /// The key event or text `event` is, its keys resolved on the app's layout.
        fn key_input(&self, event: &OsEvent) -> Result<Option<KeyInput>> {
            Ok(match event {
                OsEvent::KeyDown(name) => {
                    let (key, shift) = self.key_of(name)?;
                    Some(KeyInput::Down { key, shift })
                }
                OsEvent::KeyUp(name) => Some(KeyInput::Up {
                    key: self.key_of(name)?.0,
                }),
                OsEvent::Text(text) => Some(KeyInput::Text(text.clone())),
                _ => None,
            })
        }

        /// The key `name` names and whether it needs Shift, the layout of the app's window
        /// deciding a character's key.
        fn key_of(&self, name: &str) -> Result<(VirtualKey, bool)> {
            let unknown = || {
                Refusal::UnknownKey {
                    name: name.to_owned(),
                }
                .into_error()
            };
            match keys::resolve(name).ok_or_else(unknown)? {
                KeyName::Virtual { key, shift } => Ok((key, shift)),
                KeyName::Character(c) => {
                    win32::character_key(c, self.target()?).ok_or_else(unknown)
                }
            }
        }

        /// Send the keystrokes of `input` one by one, the desktop read again before each, each
        /// recorded and said in `delivered` as soon as Windows took it, and then confirmed through
        /// the guard's echo.
        fn type_keys(
            &mut self,
            window: Hwnd,
            input: &KeyInput,
            delivered: &mut Delivered,
        ) -> Result<()> {
            for stroke in self.keys.plan(input) {
                self.checked(window, None)?;
                let before = self.guard.echoes();
                let guard = &self.guard;
                self.keys.deliver(
                    stroke,
                    delivered,
                    || send_stroke(stroke),
                    || echo_since(guard, before),
                )?;
            }
            Ok(())
        }

        /// Press or release `button`, said in `delivered` as soon as Windows took it.
        fn mouse_button(
            &mut self,
            button: PointerButton,
            pressed: bool,
            delivered: &mut Delivered,
        ) -> Result<()> {
            let swapped = win32::buttons_swapped();
            let physical = match button {
                PointerButton::Primary => u8::from(swapped),
                PointerButton::Secondary => u8::from(!swapped),
                PointerButton::Middle => 2,
                PointerButton::Back => 3,
                PointerButton::Forward => 4,
                _ => {
                    return Err(Refusal::UnsentInput {
                        what: format!("the mouse button {button:?}"),
                    }
                    .into_error());
                }
            };
            let before = self.guard.echoes();
            let guard = &self.guard;
            self.buttons.deliver(
                physical,
                pressed,
                delivered,
                || win32::mouse_button(physical, pressed),
                || echo_since(guard, before),
            )
        }

        fn pointer(&mut self, event: &OsEvent, delivered: &mut Delivered) -> Result<()> {
            match event {
                OsEvent::PointerMove {
                    pointer,
                    device: kind,
                    to,
                } => match kind_of(*kind) {
                    Some(kind) => self.inject_move(pointer, kind, *to),
                    None => self.echoed(|| win32::move_pointer(*to)),
                },
                OsEvent::PointerDown {
                    pointer,
                    device: kind,
                    button,
                } => match kind_of(*kind) {
                    Some(kind) => {
                        let (contacts, slot) = self.contacts_and(kind);
                        let device = device(slot, kind)?;
                        contacts.down(pointer, kind, |frame| {
                            delivered
                                .send(&ActingInput, || device.inject(frame))
                                .map_err(Error::from)
                        })
                    }
                    None => self.mouse_button(*button, true, delivered),
                },
                OsEvent::PointerUp {
                    pointer,
                    device: kind,
                    button,
                } => match kind_of(*kind) {
                    Some(kind) => {
                        let (contacts, slot) = self.contacts_and(kind);
                        let device = device(slot, kind)?;
                        contacts.up(pointer, kind, |frame| {
                            delivered
                                .send(&ActingInput, || device.inject(frame))
                                .map_err(Error::from)
                        })
                    }
                    None => self.mouse_button(*button, false, delivered),
                },
                OsEvent::Scroll {
                    at,
                    delta_x,
                    delta_y,
                } => {
                    self.echoed(|| win32::move_pointer(*at))?;
                    // A turn that sends nothing waits for no echo.
                    if *delta_x == 0 && *delta_y == 0 {
                        return Ok(());
                    }
                    let before = self.guard.echoes();
                    delivered.send(&ActingInput, || win32::wheel(*delta_x, *delta_y))?;
                    echo_since(&self.guard, before)
                }
                _ => Ok(()),
            }
        }

        fn inject_move(&mut self, pointer: &str, kind: PointerKind, to: DevicePoint) -> Result<()> {
            let (contacts, slot) = self.contacts_and(kind);
            let device = device(slot, kind)?;
            contacts.move_to(pointer, kind, to, |frame| device.inject(frame))
        }

        /// The contacts and the device slot of `kind`, borrowed apart.
        fn contacts_and(
            &mut self,
            kind: PointerKind,
        ) -> (&mut Contacts, &mut Option<SyntheticDevice>) {
            let slot = match kind {
                PointerKind::Touch => &mut self.touch,
                _ => &mut self.pen,
            };
            (&mut self.contacts, slot)
        }

        /// Wait out `pause` whole, looking at the guard between slices: the person's input ends
        /// the run before the next event, never a pause cut short.
        fn pause(&mut self, pause: std::time::Duration) -> Result<()> {
            for slice in slices(pause) {
                if let Some(foreign) = self.guard.foreign_input() {
                    return Err(Error::InterruptedByUser {
                        lane: safety::LANE.to_owned(),
                        event: foreign.description,
                    });
                }
                std::thread::sleep(slice);
            }
            Ok(())
        }
    }

    /// Send one keystroke through `SendInput`: a character Windows took in part (its press without
    /// its release) fails as partly sent.
    fn send_stroke(stroke: Keystroke) -> std::result::Result<(), SendFailure> {
        match stroke {
            Keystroke::Press(key) => win32::key(key, true),
            Keystroke::Release(key) => win32::key(key, false),
            Keystroke::ShiftFor(_) => win32::key(keys::SHIFT, true),
            Keystroke::UnshiftFor(_) => win32::key(keys::SHIFT, false),
            Keystroke::Unit(_) | Keystroke::UnitUp(_) => {
                win32::keyboard(&send_input::packet_events(stroke), "a character")
            }
        }
    }

    /// The synthetic device a pointer of `device` goes through; `None` for the mouse.
    fn kind_of(device: PointerDevice) -> Option<PointerKind> {
        match device {
            PointerDevice::Touch => Some(PointerKind::Touch),
            PointerDevice::Pen => Some(PointerKind::Pen),
            _ => None,
        }
    }

    impl Lane for WindowsLane {
        fn name(&self) -> &'static str {
            safety::LANE
        }

        fn capabilities(&self) -> LaneCapabilities {
            LaneCapabilities::new(
                vec![
                    PointerDevice::Mouse,
                    PointerDevice::Touch,
                    PointerDevice::Pen,
                ],
                true,
                Isolation::Shared,
                PositionFidelity::Exact,
            )
            .accessibility(AccessibilityApi::UiAutomation)
        }

        fn guard(&mut self) -> Option<&mut dyn ActivityGuard> {
            Some(&mut self.guard)
        }

        fn locate_window(&mut self, placement: &Placement) -> Result<WindowGeometry> {
            let _units = DeviceUnits::enter();
            let (_, origin) = self.find(placement)?;
            Ok(crate::lane::screen::window_geometry(
                origin,
                placement.size,
                placement.scale,
            ))
        }

        fn check(&mut self, events: &[OsEvent]) -> Result<()> {
            let mut inputs = Vec::new();
            for event in events {
                match event {
                    OsEvent::PointerDown { device, button, .. }
                    | OsEvent::PointerUp { device, button, .. }
                        if kind_of(*device).is_some() && *button != PointerButton::Primary =>
                    {
                        return Err(Refusal::UnsentInput {
                            what: format!("the {button:?} button of a {device:?}"),
                        }
                        .into_error());
                    }
                    _ => inputs.extend(self.key_input(event)?),
                }
            }
            // The strokes the lane will send, typed text's Tab and Enter included, read as the OS
            // reads them after what it has already seen of the lane's keys.
            match self.keys.shell_hotkey(&inputs) {
                Some(handler) => Err(Refusal::ShellHotkey { handler }.into_error()),
                None => Ok(()),
            }
        }

        fn send(&mut self, event: &OsEvent) -> Result<()> {
            self.send_counted(event, &mut Delivered::default())
        }

        /// A key event or text is a keystroke or several, a button, a contact's press or release
        /// and a turn of the wheel one input each, each said once Windows took it, so text cut
        /// part way is partly sent.
        fn send_counted(&mut self, event: &OsEvent, delivered: &mut Delivered) -> Result<()> {
            // `send_guarded` refuses it first; a caller that sends directly hears the same.
            if matches!(event, OsEvent::Composition(_)) {
                return Err(crate::lane::no_composition(self.name()));
            }
            if let OsEvent::Pause(duration) = event {
                return self.pause(*duration);
            }
            let _units = DeviceUnits::enter();
            let window = self.target()?;
            if let Some(input) = self.key_input(event)? {
                return self.type_keys(window, &input, delivered);
            }
            self.checked(window, self.landing_of(event))?;
            self.pointer(event, delivered)
        }

        fn release_all(&mut self) -> Result<()> {
            let _units = DeviceUnits::enter();
            let (touch, pen) = (&mut self.touch, &mut self.pen);
            super::release_held(
                &mut self.keys,
                &mut self.buttons,
                &mut self.contacts,
                |stroke| send_stroke(stroke).map_err(Error::from),
                |button| win32::mouse_button(button, false).map_err(Error::from),
                |frame| {
                    let slot = match frame.kind {
                        PointerKind::Touch => &mut *touch,
                        _ => &mut *pen,
                    };
                    device(slot, frame.kind)?.inject(frame)
                },
            )
        }

        fn read_tree(&mut self, placement: &Placement, _app: &Tree) -> Result<Tree> {
            let _units = DeviceUnits::enter();
            let (window, _) = self.find(placement)?;
            let mut tree = uia::read(window)?;
            if let Some(root) = tree.roots.first_mut() {
                root.window = Some(placement.window);
            }
            Ok(tree)
        }

        fn invoke(&mut self, placement: &Placement, identifier: &str, _app: &Tree) -> Result<()> {
            let _units = DeviceUnits::enter();
            let (window, _) = self.find(placement)?;
            uia::invoke(window, identifier)
        }

        fn capture_window(&mut self, placement: &Placement) -> Result<crate::pixels::Image> {
            let _units = DeviceUnits::enter();
            let (window, _) = self.find(placement)?;
            capture::capture(window)
        }
    }
}
