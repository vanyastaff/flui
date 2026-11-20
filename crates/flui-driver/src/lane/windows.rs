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
//!   input path (`input.inject`): a text service's composition is out of the lane's reach.
//! - **Safety.** Before every event the rules of [`safety`] are read again: the desktop takes
//!   input, the window is the app's and in front, the point reaches it, nothing physical is held;
//!   before a sequence, [`WindowsLane::check`](crate::Lane::check) refuses a key the lane has no
//!   code for, a device or button it does not send, and a chord the OS shell takes. What the lane
//!   pressed is released on every exit.
//! - **The guard** ([`WindowsGuard`]) watches the low-level hooks and the digitizers'
//!   raw input for input the lane did not send ([`activity`]).
//! - **Reading.** [`roles`] is the role table UI Automation is read against; the lane projects the
//!   window's UI Automation tree onto the wire's nodes (`Lane::read_tree`).
//! - **Process.** A launched app runs in a kill-on-close job ([`job`]).
//!
//! The pure parts (the key, role, coordinate and activity tables, the safety rules and the
//! contact frames) build on every host, so their tables run on Linux CI; the parts that call
//! Windows are `cfg(windows)`.

pub mod activity;
pub mod contacts;
pub mod keys;
pub mod roles;
pub mod safety;
pub mod screen;

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
pub use guard::WindowsGuard;
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

#[cfg(windows)]
mod lane {
    use flui_protocol::input::{PointerButton, PointerDevice};
    use flui_protocol::session::{Isolation, Placement};
    use flui_protocol::tree::Tree;

    use super::contacts::{Contacts, PointerKind};
    use super::guard::WindowsGuard;
    use super::keys::{self, KeyName, TextStroke, VirtualKey};
    use super::safety::{self, Desk, Held, KeyEdge, Landing, Refusal};
    use super::win32::{self, DeviceUnits, Hwnd, PointerDevice as SyntheticDevice};
    use super::{capture, screen, uia};
    use crate::coordinates::{DevicePoint, WindowGeometry};
    use crate::error::{Error, Result};
    use crate::lane::{ActivityGuard, Lane, LaneCapabilities, OsCapture, PositionFidelity};
    use crate::steps::OsEvent;

    /// The Windows lane for the app whose process is `pid`: see the
    /// [module docs](super).
    #[derive(Debug)]
    pub struct WindowsLane {
        pid: u32,
        window: Option<Hwnd>,
        guard: WindowsGuard,
        held: Held,
        /// The keys pressed with a Shift the lane added, by virtual-key code.
        shifted: Vec<u16>,
        contacts: Contacts,
        touch: Option<SyntheticDevice>,
        pen: Option<SyntheticDevice>,
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
                held: Held::default(),
                shifted: Vec::new(),
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
            desk.other_key = win32::other_key_down(&self.held.keys);
            desk
        }

        fn landing(window: Hwnd, point: DevicePoint) -> Landing {
            let under = win32::root_at(point);
            Landing::new(point, under == Some(window), under.map(win32::describe))
        }

        /// Where `event` lands, for a pointer event.
        fn landing_of(&self, window: Hwnd, event: &OsEvent) -> Option<Landing> {
            let point = match event {
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
            }?;
            Some(Self::landing(window, point))
        }

        fn device(&mut self, kind: PointerKind) -> Result<&SyntheticDevice> {
            let slot = match kind {
                PointerKind::Touch => &mut self.touch,
                _ => &mut self.pen,
            };
            if slot.as_ref().is_none_or(|device| device.kind() != kind) {
                *slot = Some(SyntheticDevice::create(kind)?);
            }
            Ok(slot
                .as_ref()
                .expect("BUG: the device was created just above"))
        }

        fn inject(&mut self, frame: Option<super::contacts::Frame>) -> Result<()> {
            match frame {
                Some(frame) => {
                    let kind = frame.kind;
                    self.device(kind)?.inject(&frame)
                }
                None => Ok(()),
            }
        }

        fn press(&mut self, key: VirtualKey) -> Result<()> {
            win32::key(key, true)?;
            if !self.held.keys.contains(&key.code) {
                self.held.keys.push(key.code);
            }
            Ok(())
        }

        /// Release `key`; one whose release Windows refused stays held, for `release_all` to try
        /// again.
        fn release(&mut self, key: VirtualKey) -> Result<()> {
            win32::key(key, false)?;
            self.held.keys.retain(|code| *code != key.code);
            Ok(())
        }

        /// The key `name` names and whether it needs Shift, the layout of the app's window
        /// deciding a character's key.
        fn key_of(&self, name: &str) -> Result<(VirtualKey, bool)> {
            let unknown = || Refusal::UnknownKey {
                name: name.to_owned(),
            };
            match keys::resolve(name).ok_or_else(unknown) {
                Ok(KeyName::Virtual { key, shift }) => Ok((key, shift)),
                Ok(KeyName::Character(c)) => {
                    let window = self.target()?;
                    win32::character_key(c, window).ok_or_else(|| unknown().into_error())
                }
                Err(refusal) => Err(refusal.into_error()),
            }
        }

        fn key_down(&mut self, name: &str) -> Result<()> {
            let (key, shift) = self.key_of(name)?;
            if shift && !self.held.keys.contains(&keys::SHIFT.code) {
                self.press(keys::SHIFT)?;
                self.shifted.push(key.code);
            }
            self.press(key)
        }

        fn key_up(&mut self, name: &str) -> Result<()> {
            let (key, _) = self.key_of(name)?;
            let released = self.release(key);
            if let Some(index) = self.shifted.iter().position(|code| *code == key.code) {
                self.shifted.remove(index);
                self.release(keys::SHIFT)?;
            }
            released
        }

        fn mouse_button(&mut self, button: PointerButton, pressed: bool) -> Result<()> {
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
            win32::mouse_button(physical, pressed)?;
            if pressed {
                self.held.buttons |= 1 << physical;
            } else {
                self.held.buttons &= !(1 << physical);
            }
            Ok(())
        }

        fn perform(&mut self, event: &OsEvent) -> Result<()> {
            match event {
                OsEvent::PointerMove {
                    pointer,
                    device,
                    to,
                } => match kind_of(*device) {
                    Some(kind) => {
                        let frame = self.contacts.move_to(pointer, kind, *to);
                        self.inject(frame)
                    }
                    None => win32::move_pointer(*to),
                },
                OsEvent::PointerDown {
                    pointer,
                    device,
                    button,
                } => match kind_of(*device) {
                    Some(kind) => {
                        let frame = self
                            .contacts
                            .down(pointer, kind)
                            .map_err(Refusal::into_error)?;
                        self.inject(Some(frame))
                    }
                    None => self.mouse_button(*button, true),
                },
                OsEvent::PointerUp {
                    pointer,
                    device,
                    button,
                } => match kind_of(*device) {
                    Some(kind) => {
                        let frame = self
                            .contacts
                            .up(pointer, kind)
                            .map_err(Refusal::into_error)?;
                        self.inject(Some(frame))
                    }
                    None => self.mouse_button(*button, false),
                },
                OsEvent::Scroll {
                    at,
                    delta_x,
                    delta_y,
                } => {
                    win32::move_pointer(*at)?;
                    win32::wheel(*delta_x, *delta_y)
                }
                OsEvent::KeyDown(name) => self.key_down(name),
                OsEvent::KeyUp(name) => self.key_up(name),
                OsEvent::Text(text) => {
                    for stroke in keys::text_strokes(text) {
                        match stroke {
                            TextStroke::Key(key) => {
                                self.press(key)?;
                                self.release(key)?;
                            }
                            TextStroke::Unit(unit) => win32::unicode(unit)?,
                        }
                    }
                    Ok(())
                }
                _ => Ok(()),
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
        }

        fn guard(&mut self) -> Option<&mut dyn ActivityGuard> {
            Some(&mut self.guard)
        }

        fn locate_window(&mut self, placement: &Placement) -> Result<WindowGeometry> {
            let _units = DeviceUnits::enter();
            let (_, origin) = self.find(placement)?;
            Ok(screen::window_geometry(
                origin,
                placement.size,
                placement.scale,
            ))
        }

        fn check(&mut self, events: &[OsEvent]) -> Result<()> {
            let mut edges = Vec::new();
            for event in events {
                match event {
                    OsEvent::KeyDown(name) | OsEvent::KeyUp(name) => {
                        let (key, shift) = self.key_of(name)?;
                        let down = matches!(event, OsEvent::KeyDown(_));
                        match (down, shift) {
                            (true, true) => {
                                edges.push(KeyEdge::Down(keys::SHIFT));
                                edges.push(KeyEdge::Down(key));
                            }
                            (true, false) => edges.push(KeyEdge::Down(key)),
                            (false, true) => {
                                edges.push(KeyEdge::Up(key));
                                edges.push(KeyEdge::Up(keys::SHIFT));
                            }
                            (false, false) => edges.push(KeyEdge::Up(key)),
                        }
                    }
                    OsEvent::PointerDown { device, button, .. }
                    | OsEvent::PointerUp { device, button, .. }
                        if kind_of(*device).is_some() && *button != PointerButton::Primary =>
                    {
                        return Err(Refusal::UnsentInput {
                            what: format!("the {button:?} button of a {device:?}"),
                        }
                        .into_error());
                    }
                    _ => {}
                }
            }
            match safety::shell_hotkey(&edges) {
                Some(handler) => Err(Refusal::ShellHotkey { handler }.into_error()),
                None => Ok(()),
            }
        }

        fn send(&mut self, event: &OsEvent) -> Result<()> {
            if let OsEvent::Pause(duration) = event {
                std::thread::sleep((*duration).min(win32::LONGEST_PAUSE));
                return Ok(());
            }
            let _units = DeviceUnits::enter();
            let window = self.target()?;
            let desk = self.desk(window);
            let landing = if desk.input_desktop {
                self.landing_of(window, event)
            } else {
                None
            };
            safety::check_event(&desk, landing.as_ref(), &self.held)
                .map_err(Refusal::into_error)?;
            self.perform(event)
        }

        fn release_all(&mut self) -> Result<()> {
            let _units = DeviceUnits::enter();
            let mut first: Option<Error> = None;
            let mut keep = |result: Result<()>| {
                if let Err(error) = result {
                    first.get_or_insert(error);
                }
            };
            for code in std::mem::take(&mut self.held.keys).into_iter().rev() {
                keep(win32::key(
                    VirtualKey {
                        code,
                        extended: keys::is_extended(code),
                    },
                    false,
                ));
            }
            self.shifted.clear();
            for physical in 0..5_u8 {
                if self.held.buttons & (1 << physical) != 0 {
                    keep(win32::mouse_button(physical, false));
                }
            }
            self.held.buttons = 0;
            for frame in self.contacts.release_all() {
                let injected = self.inject(Some(frame));
                keep(injected);
            }
            first.map_or(Ok(()), Err)
        }

        fn read_tree(&mut self, placement: &Placement) -> Result<Tree> {
            let _units = DeviceUnits::enter();
            let (window, _) = self.find(placement)?;
            let mut tree = uia::read(window)?;
            if let Some(root) = tree.roots.first_mut() {
                root.window = Some(placement.window);
            }
            Ok(tree)
        }

        fn invoke(&mut self, placement: &Placement, identifier: &str) -> Result<()> {
            let _units = DeviceUnits::enter();
            let (window, _) = self.find(placement)?;
            uia::invoke(window, identifier)
        }

        fn capture_window(&mut self, placement: &Placement) -> Result<OsCapture> {
            let _units = DeviceUnits::enter();
            let (window, _) = self.find(placement)?;
            capture::capture(window)
        }
    }
}
