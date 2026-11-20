//! The macOS lane: real input into an app on the user's desktop, what the accessibility API shows
//! of it, and the window server's capture of its window.
//!
//! - **Input.** Quartz events posted to the app's process (`CGEventPostToPid`), each from a
//!   private event source carrying the lane's tag ([`activity::LANE_TAG`]): the mouse and its
//!   wheel, keys with the character they type, text as Unicode runs ([`keys::text_strokes`]), and
//!   a pen as tablet mouse events with their pressure and proximity ([`mod@pointer`]). Touch has no
//!   Quartz event and is not sent. Positions are global display points; the lane posts at the
//!   centre of the device pixel the driver aimed at, which lies in that pixel on every display
//!   ([`screen`]). An input-method composition goes through the app's own input path
//!   (`input.inject`). A pause is waited out whole, watching the guard as it waits.
//! - **One path for keys.** A key event and typed text alike become keystrokes planned against
//!   what the lane holds ([`held`]): the system-shortcut rule reads those strokes before a
//!   sequence, and the lane posts them, reading the desktop again before each one. Every event
//!   carries the modifiers the lane holds as its flags.
//! - **Safety.** Before every event, and every keystroke, the rules of [`safety`] are read again:
//!   the window is there and in front with its app taking the keys, the point reaches it, nothing
//!   physical is held; before a sequence, `Lane::check` refuses a key the lane has no code for, a
//!   device or button it does not send, a shortcut the system takes, and a run the system has not
//!   permitted (Accessibility). What the lane pressed is released on every exit.
//! - **Permissions.** Posting events needs Accessibility, the guard's tap Input Monitoring, a
//!   capture Screen Recording ([`safety::Permission`]): the lane asks the system before the call
//!   that needs each and answers `not_supported` naming the one missing, never a prompt it waits
//!   on.
//! - **The guard** (`MacosGuard`, on macOS) listens on a session event tap for input the lane did
//!   not send ([`activity`]); a tap the system turned off ends the run, since it would no longer
//!   see the person's input.
//! - **Reading.** [`roles`] is the role table the accessibility API is read against; the lane
//!   projects the window's accessibility tree onto the wire's nodes (`Lane::read_tree`) through
//!   the bounded walk of `lane::windows::walk`, the window frame's own controls left out
//!   ([`frame`]), and presses an element by its identifier (`Lane::invoke`, `AXPress`).
//! - **Capture.** ScreenCaptureKit's capture of the window alone, from macOS 14 (`capture`).
//!
//! The pure parts (the key, role, coordinate and activity tables, the safety rules, the held keys,
//! the pointer plan, the frame projection and the release gates) build on every host, so their
//! tables run on Linux CI; the parts that call macOS are `cfg(target_os = "macos")`.

pub mod activity;
pub mod frame;
pub mod held;
pub mod keys;
pub mod pointer;
pub mod roles;
pub mod safety;
pub mod screen;
pub mod version;

#[cfg(target_os = "macos")]
mod ax;
#[cfg(target_os = "macos")]
mod capture;
#[cfg(target_os = "macos")]
mod guard;
#[cfg(target_os = "macos")]
mod quartz;

#[cfg(target_os = "macos")]
pub use guard::{MacosGuard, STOPPED_WATCHING};
#[cfg(target_os = "macos")]
pub use lane::MacosLane;

/// Whether the system grants the driver's process `permission`, asked without a prompt: what a
/// run on a Mac reads to tell a host that cannot run the lane from a failure. The lane asks the
/// same before each call that needs one.
#[cfg(target_os = "macos")]
#[must_use]
pub fn granted(permission: safety::Permission) -> bool {
    quartz::granted(permission)
}

#[cfg(target_os = "macos")]
mod lane {
    use std::time::{Duration, Instant};

    use flui_protocol::geometry::LogicalSize;
    use flui_protocol::input::PointerDevice;
    use flui_protocol::session::{Isolation, Placement};
    use flui_protocol::tree::Tree;

    use super::guard::MacosGuard;
    use super::held::{HeldKeys, KeyInput};
    use super::keys::{self, KeyName};
    use super::pointer::{self, Point, Pointer, Step};
    use super::safety::{self, Desk, Landing, Permission, Refusal};
    use super::screen::{self, PointRect, WindowInfo};
    use super::{ax, capture, quartz};
    use crate::coordinates::{DevicePoint, WindowGeometry};
    use crate::error::{Error, Result};
    use crate::lane::windows::activity::slices;
    use crate::lane::{ActivityGuard, Lane, LaneCapabilities, OsCapture, PositionFidelity};
    use crate::steps::OsEvent;

    /// How long the lane waits for the app it brought to the front to be there.
    const ACTIVATE_WITHIN: Duration = Duration::from_secs(1);

    /// The window the lane sends to: its number, its frame and the content and scale the app
    /// reports.
    #[derive(Debug, Clone, Copy, PartialEq)]
    struct Target {
        number: u32,
        frame: PointRect,
        size: LogicalSize,
        scale: f64,
    }

    impl Target {
        /// Where the lane posts an event aimed at device pixel `device`: see
        /// [`screen::points_of`].
        fn points_of(&self, device: DevicePoint) -> Result<Point> {
            screen::points_of(
                device,
                self.scale,
                screen::content_rect(self.frame, self.size),
            )
        }
    }

    /// The macOS lane for the app whose process is `pid`: see the [module docs](super).
    #[derive(Debug)]
    pub struct MacosLane {
        pid: libc::pid_t,
        target: Option<Target>,
        guard: MacosGuard,
        keys: HeldKeys,
        pointer: Pointer,
    }

    impl MacosLane {
        /// A lane driving the app whose process is `pid` in `isolation`: see
        /// [`safety::admit_isolation`] for the modes it runs in.
        ///
        /// # Errors
        ///
        /// As [`safety::admit_isolation`]; `invalid_argument` for a process id past what macOS
        /// numbers processes with.
        pub fn try_new(pid: u32, isolation: Isolation) -> Result<Self> {
            safety::admit_isolation(isolation).map_err(Refusal::into_error)?;
            let pid = libc::pid_t::try_from(pid)
                .map_err(|_| Error::invalid_argument(format!("{pid} is no macOS process id")))?;
            Ok(Self {
                pid,
                target: None,
                guard: MacosGuard::new(),
                keys: HeldKeys::new(),
                pointer: Pointer::new(),
            })
        }

        /// The app's window `placement` describes, among the process's ordinary windows.
        fn find(&mut self, placement: &Placement) -> Result<Target> {
            let windows = quartz::windows(false)?;
            let found: Vec<&WindowInfo> = windows
                .iter()
                .filter(|window| {
                    window.pid == self.pid
                        && window.level == screen::NORMAL_LEVEL
                        && screen::is_window(window.bounds, placement.size)
                })
                .collect();
            match found.as_slice() {
                [window] => {
                    let target = Target {
                        number: window.number,
                        frame: window.bounds,
                        size: placement.size,
                        scale: placement.scale,
                    };
                    self.target = Some(target);
                    Ok(target)
                }
                [] => Err(Error::not_found(format!(
                    "process {} shows no window of {} x {}",
                    self.pid, placement.size.width, placement.size.height
                ))),
                several => Err(quartz::lane_error(format!(
                    "process {} shows {} windows of {} x {}; the lane cannot tell which is {}",
                    self.pid,
                    several.len(),
                    placement.size.width,
                    placement.size.height,
                    placement.window
                ))),
            }
        }

        fn target(&self) -> Result<Target> {
            self.target.ok_or_else(|| {
                quartz::lane_error(
                    "the lane has no target window: it sends nothing before it locates one",
                )
            })
        }

        /// The desktop as the next event finds it, the app brought to the front once when
        /// another window is there.
        fn desk(&self, target: Target) -> Result<(Desk, Vec<WindowInfo>)> {
            let mut desk = Desk::quiet();
            desk.window_alive = quartz::windows(false)?
                .iter()
                .any(|window| window.number == target.number && window.pid == self.pid);
            let mut on_screen = quartz::windows(true)?;
            let in_front = |windows: &[WindowInfo]| {
                screen::front_window(windows).is_some_and(|front| front.number == target.number)
                    && ax::focused_pid() == Some(self.pid)
            };
            if desk.window_alive && !in_front(&on_screen) {
                // The window server activates an app asynchronously: the order is read again
                // until the app is in front or the wait is over.
                ax::bring_to_front(self.pid, target.frame);
                let deadline = Instant::now() + ACTIVATE_WITHIN;
                loop {
                    on_screen = quartz::windows(true)?;
                    if in_front(&on_screen) || Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
            desk.in_front = in_front(&on_screen);
            desk.foreground = screen::front_window(&on_screen).map(WindowInfo::describe);
            let (buttons, modifiers, other_key) = quartz::physical();
            desk.buttons = buttons;
            desk.modifiers = modifiers;
            desk.other_key = other_key;
            Ok((desk, on_screen))
        }

        /// The safety rules over the desktop as it is now, for an event landing at `landing` in
        /// global points.
        fn checked(&self, target: Target, landing: Option<(DevicePoint, Point)>) -> Result<()> {
            let (desk, on_screen) = self.desk(target)?;
            let landing = landing.map(|(device, point)| {
                let under = screen::window_at(&on_screen, point);
                Landing::new(
                    device,
                    under.is_some_and(|window| window.number == target.number),
                    under.map(WindowInfo::describe),
                )
            });
            safety::check_event(&desk, landing.as_ref()).map_err(Refusal::into_error)
        }

        /// The key event or text `event` is, its key resolved.
        fn key_input(event: &OsEvent) -> Result<Option<KeyInput>> {
            let key_of = |name: &str| {
                keys::resolve(name).ok_or_else(|| {
                    Refusal::UnknownKey {
                        name: name.to_owned(),
                    }
                    .into_error()
                })
            };
            Ok(match event {
                OsEvent::KeyDown(name) => Some(match key_of(name)? {
                    KeyName::Named(key) => KeyInput::Down {
                        key,
                        shift: false,
                        character: None,
                    },
                    KeyName::Character {
                        key,
                        shift,
                        character,
                    } => KeyInput::Down {
                        key,
                        shift,
                        character: Some(character),
                    },
                }),
                OsEvent::KeyUp(name) => Some(match key_of(name)? {
                    KeyName::Named(key) => KeyInput::Up {
                        key,
                        character: None,
                    },
                    KeyName::Character { key, character, .. } => KeyInput::Up {
                        key,
                        character: Some(character),
                    },
                }),
                OsEvent::Text(text) => Some(KeyInput::Text(text.clone())),
                _ => None,
            })
        }

        /// Post the keystrokes of `input` one by one, the desktop read again before each, each
        /// recorded once it was posted.
        fn type_keys(&mut self, target: Target, input: &KeyInput) -> Result<()> {
            let pid = self.pid;
            for stroke in self.keys.plan(input) {
                self.checked(target, None)?;
                self.keys.deliver(stroke, |stroke, flags| {
                    quartz::keystroke(pid, stroke, flags)
                })?;
            }
            Ok(())
        }

        /// The step a pointer `event` is, its point in global points with the device pixel it
        /// was aimed at.
        fn step_of(
            target: Target,
            event: &OsEvent,
        ) -> Result<Option<(Step, Option<(DevicePoint, Point)>)>> {
            Ok(match event {
                OsEvent::PointerMove { device, to, .. } => {
                    let point = target.points_of(*to)?;
                    Some((
                        Step::Move {
                            device: *device,
                            to: point,
                        },
                        Some((*to, point)),
                    ))
                }
                OsEvent::PointerDown { device, button, .. } => Some((
                    Step::Down {
                        device: *device,
                        button: *button,
                    },
                    None,
                )),
                OsEvent::PointerUp { device, button, .. } => Some((
                    Step::Up {
                        device: *device,
                        button: *button,
                    },
                    None,
                )),
                _ => None,
            })
        }

        /// Where a press or release of `device` lands: where its pointer last went.
        fn landing_of(
            &self,
            device: PointerDevice,
            target: Target,
        ) -> Option<(DevicePoint, Point)> {
            let point = self.pointer.position(device)?;
            Some((screen::device_of(point, target.scale)?, point))
        }

        fn pointer(&mut self, target: Target, event: &OsEvent) -> Result<()> {
            if let OsEvent::Scroll {
                at,
                delta_x,
                delta_y,
            } = event
            {
                let point = target.points_of(*at)?;
                self.checked(target, Some((*at, point)))?;
                // A turn that moves nothing posts nothing.
                if *delta_x == 0 && *delta_y == 0 {
                    return Ok(());
                }
                let (vertical, horizontal) = pointer::scroll_wheels(*delta_x, *delta_y)?;
                return quartz::scroll(self.pid, point, vertical, horizontal, self.keys.flags());
            }
            let Some((step, landing)) = Self::step_of(target, event)? else {
                return Ok(());
            };
            let landing = match step {
                Step::Down { device, .. } | Step::Up { device, .. } => {
                    self.landing_of(device, target)
                }
                Step::Move { .. } => landing,
            };
            let events = self.pointer.plan(step)?;
            self.checked(target, landing)?;
            let (pid, flags) = (self.pid, self.keys.flags());
            self.pointer
                .deliver(&events, |event| quartz::mouse(pid, event, flags))
        }

        /// Wait out `pause` whole, looking at the guard between slices: the person's input ends
        /// the run before the next event, never a pause cut short.
        fn pause(&mut self, pause: Duration) -> Result<()> {
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

        /// Refuse a call that needs `permission` when the system has not granted it.
        fn permitted(permission: Permission) -> Result<()> {
            if quartz::granted(permission) {
                Ok(())
            } else {
                Err(Refusal::MissingPermission { permission }.into_error())
            }
        }
    }

    impl Lane for MacosLane {
        fn name(&self) -> &'static str {
            safety::LANE
        }

        fn capabilities(&self) -> LaneCapabilities {
            LaneCapabilities::new(
                vec![PointerDevice::Mouse, PointerDevice::Pen],
                true,
                Isolation::Shared,
                PositionFidelity::Exact,
            )
        }

        fn guard(&mut self) -> Option<&mut dyn ActivityGuard> {
            Some(&mut self.guard)
        }

        fn locate_window(&mut self, placement: &Placement) -> Result<WindowGeometry> {
            let target = self.find(placement)?;
            Ok(screen::window_geometry(
                screen::content_origin(target.frame, target.size),
                target.size,
                target.scale,
            ))
        }

        fn check(&mut self, events: &[OsEvent]) -> Result<()> {
            let mut inputs = Vec::new();
            let mut pointer = self.pointer;
            for event in events {
                if let Some(input) = Self::key_input(event)? {
                    inputs.push(input);
                    continue;
                }
                // The pointer's plan, as the lane will post it: a device it lacks, a button no
                // event carries.
                let step = match event {
                    OsEvent::PointerMove { device, .. } => Some(Step::Move {
                        device: *device,
                        to: Point::new(0.0, 0.0),
                    }),
                    OsEvent::PointerDown { device, button, .. } => Some(Step::Down {
                        device: *device,
                        button: *button,
                    }),
                    OsEvent::PointerUp { device, button, .. } => Some(Step::Up {
                        device: *device,
                        button: *button,
                    }),
                    _ => None,
                };
                if let Some(step) = step {
                    for planned in pointer.plan(step)? {
                        pointer.apply(&planned);
                    }
                }
            }
            if let Some(handler) = self.keys.system_shortcut(&inputs) {
                return Err(Refusal::SystemShortcut { handler }.into_error());
            }
            Self::permitted(Permission::Accessibility)
        }

        fn send(&mut self, event: &OsEvent) -> Result<()> {
            if let OsEvent::Pause(duration) = event {
                return self.pause(*duration);
            }
            let target = self.target()?;
            if let Some(input) = Self::key_input(event)? {
                return self.type_keys(target, &input);
            }
            self.pointer(target, event)
        }

        fn release_all(&mut self) -> Result<()> {
            let pid = self.pid;
            let keys = self
                .keys
                .release_all(|stroke, flags| quartz::keystroke(pid, stroke, flags));
            let flags = self.keys.flags();
            let pointer = self
                .pointer
                .release_all(|event| quartz::mouse(pid, event, flags));
            keys.and(pointer)
        }

        fn read_tree(&mut self, placement: &Placement) -> Result<Tree> {
            Self::permitted(Permission::Accessibility)?;
            let target = self.find(placement)?;
            let mut tree = ax::read(self.pid, target.frame, target.scale)?;
            if let Some(root) = tree.roots.first_mut() {
                root.window = Some(placement.window);
            }
            Ok(tree)
        }

        fn invoke(&mut self, placement: &Placement, identifier: &str) -> Result<()> {
            Self::permitted(Permission::Accessibility)?;
            let target = self.find(placement)?;
            ax::press(self.pid, target.frame, target.scale, identifier)
        }

        fn capture_window(&mut self, placement: &Placement) -> Result<OsCapture> {
            let target = self.find(placement)?;
            capture::capture(target.number, target.frame, target.size, target.scale)
        }
    }
}
