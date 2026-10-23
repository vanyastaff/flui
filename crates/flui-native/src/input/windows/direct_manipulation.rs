//! The precision touchpad through a DirectManipulation viewport (ADR-0217 §4).
//!
//! `WM_POINTER` carries a touchpad's contacts but not its meaning: a
//! two-finger pan, its inertia and a pinch are what DirectManipulation
//! recognises. Each window owns one manual-update viewport over its client
//! area. The OS offers it a touchpad contact in `DM_POINTERHITTEST`
//! (touch and pen contacts are never offered: they stay raw `WM_POINTER`
//! contacts), the viewport reports statuses and content transforms from its
//! event handler, and the window's pump (`InputWindow::pump_touchpad`)
//! turns them into input with the shared
//! [`TouchpadAssembler`](manipulation::TouchpadAssembler).
//!
//! # Pumping
//!
//! A manual-update viewport only runs when asked: `IDirectManipulationUpdateManager::Update`
//! processes the contacts seen since the last call and delivers the handler
//! callbacks, synchronously, on the calling thread. The handler only appends
//! a [`ViewportReading`](crate::input::assembly::ViewportReading) to a queue;
//! the pump drains it after `Update` returned, so no user code runs inside a
//! COM callback, and no `RefCell` borrow of the window's touchpad is held
//! across a COM call or user code.
//!
//! Delivery is ordered through the window's outbox: an input callback may run
//! a nested message loop (a dialog, a menu) whose `WM_TIMER` or `WM_PAINT`
//! pumps again. That nested pump appends its input behind what the outer pump
//! has not delivered yet and returns; the outermost delivery drains the outbox
//! in decode order, so a `Changed` never overtakes its `Began` and a
//! cancellation never precedes the steps it ends.
//!
//! The pump runs on the window's own schedule, from three places:
//! the frame (`WM_PAINT`, before the frame callback, so a frame sees the
//! scroll it follows); `WM_TIMER`, which also fires inside the modal loops of
//! a menu or a drag and paces a manipulation no frame is running for; and
//! right after a contact is offered. While a viewport runs or coasts, each
//! pump that delivered input invalidates the window, so the viewport animates
//! at the frame cadence without the application asking for frames. A resting
//! viewport costs nothing: no timer, no invalidation.
//!
//! # The modal size and move loop
//!
//! The system's size and move loop dispatches no application loop. The window
//! pauses its viewport on `WM_ENTERSIZEMOVE` (what is open is cancelled,
//! contacts are declined) and resumes on `WM_EXITSIZEMOVE`: a deterministic
//! pause, never a half-delivered gesture.
//!
//! # Failure
//!
//! A COM failure while pumping, a viewport that cannot be created, or a window
//! destroyed with a manipulation open each end what is open as cancelled and
//! release the viewport; nothing panics and no interface outlives its window.

#[cfg(windows)]
use std::cell::{Cell, RefCell};
#[cfg(windows)]
use std::collections::VecDeque;
#[cfg(windows)]
use std::rc::Rc;

#[cfg(windows)]
use windows::Win32::Foundation::{HWND, POINT, RECT};
#[cfg(windows)]
use windows::Win32::Graphics::DirectManipulation::{
    DIRECTMANIPULATION_CONFIGURATION_INTERACTION, DIRECTMANIPULATION_CONFIGURATION_SCALING,
    DIRECTMANIPULATION_CONFIGURATION_TRANSLATION_INERTIA,
    DIRECTMANIPULATION_CONFIGURATION_TRANSLATION_X, DIRECTMANIPULATION_CONFIGURATION_TRANSLATION_Y,
    DIRECTMANIPULATION_VIEWPORT_OPTIONS_MANUALUPDATE, DirectManipulationManager,
    IDirectManipulationContent, IDirectManipulationFrameInfoProvider, IDirectManipulationManager,
    IDirectManipulationUpdateManager, IDirectManipulationViewport,
};
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::InvalidateRect;
#[cfg(windows)]
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
#[cfg(windows)]
use windows::Win32::System::Performance::QueryPerformanceCounter;
#[cfg(windows)]
use windows::Win32::System::SystemInformation::GetTickCount64;
#[cfg(windows)]
use windows::Win32::UI::Input::Pointer::GetPointerType;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, KillTimer, POINTER_INPUT_TYPE, PT_TOUCHPAD, SetTimer,
};

#[cfg(windows)]
use super::InputWindow;
#[cfg(windows)]
use super::raw::SURFACE;
#[cfg(windows)]
use crate::input::assembly::{
    NativeTime, QpcTicks, ReadingTime, TouchpadFrame, ViewportReading, ViewportStatus,
};
#[cfg(windows)]
use crate::owner::windows::com_apartment::ComApartment;
#[cfg(windows)]
use flui_platform::PlatformInput;
#[cfg(windows)]
use manipulation::TouchpadAssembler;

/// The timer that paces a window's viewport; the value spells "DM".
#[cfg(windows)]
const PUMP_TIMER: usize = 0x444D;

/// How often the timer pumps a running or coasting viewport. The system
/// clamps timers to its own minimum; frames pump faster when they run.
#[cfg(windows)]
const PUMP_INTERVAL_MS: u32 = 8;

/// Half the side of the content rectangle, in content pixels. The content is
/// far larger than any viewport so a manipulation never meets an edge and the
/// system never rubber-bands it.
#[cfg(windows)]
const CONTENT_EXTENT: i32 = 1 << 24;

/// The `IDirectManipulationViewportEventHandler` the viewport reports to,
/// in a module of its own so the lints the `#[implement]` expansion trips
/// are expected here and nowhere else.
#[cfg(windows)]
#[expect(
    clippy::ref_as_ptr,
    clippy::inline_always,
    raw_borrows_via_references,
    reason = "code generated by windows-core's #[implement]"
)]
mod handler {
    use std::cell::RefCell;
    use std::rc::Rc;

    use windows::Win32::Foundation::E_UNEXPECTED;
    use windows::Win32::Graphics::DirectManipulation::{
        DIRECTMANIPULATION_STATUS, IDirectManipulationContent, IDirectManipulationViewport,
        IDirectManipulationViewportEventHandler, IDirectManipulationViewportEventHandler_Impl,
    };
    use windows_core::{Ref, implement};

    use crate::input::assembly::{ContentTransform, ViewportReading, ViewportStatus};
    use crate::owner::panic_boundary::{AfterPanic, os_entry};

    /// Readings the viewport reported during one `Update`.
    pub(in super::super) type ReadingQueue = Rc<RefCell<Vec<ViewportReading>>>;

    /// Appends what the viewport reports to a queue; runs no other code.
    /// Its queue is an `Rc<RefCell<..>>` of the window's thread, so it is not
    /// agile: COM marshals another apartment's calls back to that thread.
    #[implement(IDirectManipulationViewportEventHandler, Agile = false)]
    pub(in super::super) struct Handler {
        queue: ReadingQueue,
    }

    impl Handler {
        pub(in super::super) fn new(queue: ReadingQueue) -> Self {
            Self { queue }
        }

        /// The boundary every callback crosses: `read` makes the reading the
        /// callback reports, if any, and a panic must not unwind into the
        /// system. A queue borrowed elsewhere drops the reading.
        fn com_method(
            &self,
            read: impl FnOnce() -> Option<ViewportReading>,
        ) -> windows_core::Result<()> {
            os_entry(
                "a DirectManipulation viewport handler",
                (),
                AfterPanic::Answer(|()| Err(E_UNEXPECTED.into())),
                || {
                    if let Some(reading) = read()
                        && let Ok(mut queue) = self.queue.try_borrow_mut()
                    {
                        queue.push(reading);
                    }
                    Ok(())
                },
            )
        }
    }

    impl IDirectManipulationViewportEventHandler_Impl for Handler_Impl {
        fn OnViewportStatusChanged(
            &self,
            _viewport: Ref<'_, IDirectManipulationViewport>,
            current: DIRECTMANIPULATION_STATUS,
            _previous: DIRECTMANIPULATION_STATUS,
        ) -> windows_core::Result<()> {
            Handler::com_method(self, || {
                ViewportStatus::from_raw(current.0).map(ViewportReading::Status)
            })
        }

        fn OnViewportUpdated(
            &self,
            _viewport: Ref<'_, IDirectManipulationViewport>,
        ) -> windows_core::Result<()> {
            Handler::com_method(self, || None)
        }

        fn OnContentUpdated(
            &self,
            _viewport: Ref<'_, IDirectManipulationViewport>,
            content: Ref<'_, IDirectManipulationContent>,
        ) -> windows_core::Result<()> {
            Handler::com_method(self, || {
                let mut matrix = [0.0_f32; 6];
                if let Some(content) = content.as_ref() {
                    // SAFETY: `matrix` is a live, writable array of the six
                    // floats `GetContentTransform` fills.
                    if unsafe { content.GetContentTransform(&mut matrix) }.is_err() {
                        return None;
                    }
                }
                ContentTransform::from_matrix(&matrix).map(ViewportReading::Content)
            })
        }
    }
}

#[cfg(windows)]
use handler::{Handler, ReadingQueue};

/// A window's viewport and the COM objects behind it.
///
/// Every interface here was created on the window's thread, inside the
/// single-threaded apartment `_apartment` keeps initialised, and the
/// `windows` interface types are neither `Send` nor `Sync`, so every call
/// below runs on that thread and apartment. The value is shared as
/// `Rc<ComViewport>` so a call runs with no `RefCell` borrow of the window's
/// touchpad held; a release while such a call is in flight defers the
/// teardown in `Drop` to the last owner.
#[cfg(windows)]
struct ComViewport {
    manager: IDirectManipulationManager,
    update: IDirectManipulationUpdateManager,
    viewport: IDirectManipulationViewport,
    cookie: u32,
    hwnd: HWND,
    queue: ReadingQueue,
    /// The client size the viewport covers, in device pixels.
    size: Cell<(i32, i32)>,
    /// Makes the next `update` fail, as a viewport whose `Update` errs.
    #[cfg(test)]
    fail_update: Cell<bool>,
    /// Released after the interfaces above.
    _apartment: Rc<ComApartment>,
}

#[cfg(windows)]
impl ComViewport {
    /// A viewport over `hwnd`'s client area of `size`, enabled and waiting
    /// for contacts.
    fn create(
        hwnd: HWND,
        size: (i32, i32),
        apartment: Rc<ComApartment>,
    ) -> windows_core::Result<Self> {
        let queue = ReadingQueue::default();
        // SAFETY: the calling thread is in the STA `apartment` holds; every
        // out value is a COM interface owned by the returned wrappers, and
        // `hwnd` is the live window this runs for.
        unsafe {
            let manager: IDirectManipulationManager =
                CoCreateInstance(&DirectManipulationManager, None, CLSCTX_INPROC_SERVER)?;
            let update: IDirectManipulationUpdateManager = manager.GetUpdateManager()?;
            let viewport: IDirectManipulationViewport =
                manager.CreateViewport(None::<&IDirectManipulationFrameInfoProvider>, hwnd)?;
            // Pan in both axes with inertia, and pinch; the viewport is
            // updated by `Update` alone (manual), never by a system thread.
            viewport.ActivateConfiguration(
                DIRECTMANIPULATION_CONFIGURATION_INTERACTION
                    | DIRECTMANIPULATION_CONFIGURATION_TRANSLATION_X
                    | DIRECTMANIPULATION_CONFIGURATION_TRANSLATION_Y
                    | DIRECTMANIPULATION_CONFIGURATION_TRANSLATION_INERTIA
                    | DIRECTMANIPULATION_CONFIGURATION_SCALING,
            )?;
            viewport.SetViewportOptions(DIRECTMANIPULATION_VIEWPORT_OPTIONS_MANUALUPDATE)?;
            let this = Self {
                manager,
                update,
                viewport,
                cookie: 0,
                hwnd,
                queue: Rc::clone(&queue),
                size: Cell::new(size),
                #[cfg(test)]
                fail_update: Cell::new(false),
                _apartment: apartment,
            };
            // From here a failure drops `this`, which releases what exists.
            let mut this = this;
            this.set_rect(size)?;
            let content: IDirectManipulationContent = this.viewport.GetPrimaryContent()?;
            content.SetContentRect(&RECT {
                left: -CONTENT_EXTENT,
                top: -CONTENT_EXTENT,
                right: CONTENT_EXTENT,
                bottom: CONTENT_EXTENT,
            })?;
            let handler: windows::Win32::Graphics::DirectManipulation::IDirectManipulationViewportEventHandler =
                Handler::new(queue).into();
            this.cookie = this.viewport.AddEventHandler(Some(hwnd), &handler)?;
            this.manager.Activate(hwnd)?;
            this.viewport.Enable()?;
            this.update
                .Update(None::<&IDirectManipulationFrameInfoProvider>)?;
            Ok(this)
        }
    }

    fn set_rect(&self, (width, height): (i32, i32)) -> windows_core::Result<()> {
        let rect = RECT {
            left: 0,
            top: 0,
            right: width.max(1),
            bottom: height.max(1),
        };
        // SAFETY: `rect` is a live local the call reads.
        unsafe { self.viewport.SetViewportRect(&raw const rect) }?;
        self.size.set((width.max(1), height.max(1)));
        Ok(())
    }

    /// Run the viewport and take what its handler reported.
    fn update(&self) -> windows_core::Result<Vec<ViewportReading>> {
        #[cfg(test)]
        if self.fail_update.replace(false) {
            return Err(windows::Win32::Foundation::E_FAIL.into());
        }
        // SAFETY: see the type for the thread and apartment; a null frame
        // info provider is documented as "no provider". `Update` runs the
        // handler synchronously, and the handler only appends to `queue`,
        // which no caller holds borrowed across this call.
        unsafe {
            self.update
                .Update(None::<&IDirectManipulationFrameInfoProvider>)
        }?;
        Ok(std::mem::take(&mut *self.queue.borrow_mut()))
    }

    /// Return the content to the origin without animation.
    fn reset(&self) -> windows_core::Result<()> {
        let (width, height) = self.size.get();
        // SAFETY: see the type: the interface is called on the thread and
        // apartment that created it; the call takes only plain values.
        unsafe {
            self.viewport
                .ZoomToRect(0.0, 0.0, width as f32, height as f32, false)
        }
    }

    fn set_contact(&self, pointer: u32) -> windows_core::Result<()> {
        // SAFETY: see the type: the interface is called on the thread and
        // apartment that created it; the call takes only plain values.
        unsafe { self.viewport.SetContact(pointer) }
    }

    /// Stop the viewport and release the contacts it holds.
    fn stop(&self) {
        // SAFETY: see the type: the interface is called on the thread and
        // apartment that created it; the calls take no arguments.
        let results = unsafe { [self.viewport.ReleaseAllContacts(), self.viewport.Stop()] };
        for error in results.into_iter().filter_map(Result::err) {
            tracing::debug!(?error, "DirectManipulation stop failed");
        }
    }
}

#[cfg(windows)]
impl Drop for ComViewport {
    fn drop(&mut self) {
        // SAFETY: see the type for the thread and apartment. `hwnd` names
        // the window this viewport was activated for; after the window is
        // destroyed the calls fail and are only logged.
        // Every step is attempted: a failure leaves nothing the interface
        // releases below does not free.
        let results = unsafe {
            [
                self.viewport.Disable(),
                self.viewport.RemoveEventHandler(self.cookie),
                self.viewport.Abandon(),
                self.manager.Deactivate(self.hwnd),
            ]
        };
        for error in results.into_iter().filter_map(Result::err) {
            tracing::debug!(?error, "DirectManipulation teardown step failed");
        }
    }
}

/// A window's precision-touchpad input: its viewport and the assembler.
#[cfg(windows)]
pub(super) struct TouchpadHost {
    viewport: Rc<ComViewport>,
    assembler: TouchpadAssembler,
    status: ViewportStatus,
    /// Paused for the modal size and move loop.
    paused: bool,
    /// The cursor where a frame last read it, in device pixels of the
    /// client area.
    cursor: Option<(f64, f64)>,
    /// Reads the cursor as unreadable, as on another active desktop.
    #[cfg(test)]
    cursor_unreadable: bool,
}

#[cfg(windows)]
impl TouchpadHost {
    /// A host for `hwnd`, or why DirectManipulation is unavailable.
    pub(super) fn create(
        hwnd: HWND,
        size: (i32, i32),
        apartment: Rc<ComApartment>,
    ) -> windows_core::Result<Self> {
        Ok(Self {
            viewport: Rc::new(ComViewport::create(hwnd, size, apartment)?),
            assembler: TouchpadAssembler::new(SURFACE),
            status: ViewportStatus::Ready,
            paused: false,
            cursor: None,
            #[cfg(test)]
            cursor_unreadable: false,
        })
    }

    fn animating(&self) -> bool {
        self.status.is_animating() || self.assembler.is_open()
    }
}

/// The time a reading taken now carries: the coarse uptime tick and the
/// performance counter.
#[cfg(windows)]
fn now() -> ReadingTime {
    let mut counter = 0_i64;
    // SAFETY: `counter` is a live, writable local; `GetTickCount64` takes
    // no arguments.
    let (queried, uptime) =
        unsafe { (QueryPerformanceCounter(&raw mut counter), GetTickCount64()) };
    ReadingTime {
        coarse: NativeTime::TickMillis {
            tick: uptime as u32,
            observed_ms: uptime,
        },
        counter: queried
            .ok()
            .and_then(|()| u64::try_from(counter).ok())
            .map(QpcTicks),
    }
}

#[cfg(windows)]
impl InputWindow {
    /// Whether this window's viewport initialised, for its capability table.
    pub(super) fn has_touchpad(&self) -> bool {
        self.touchpad.host.borrow().is_some()
    }

    /// The frame `host`'s readings are decoded in: the cursor now, or where
    /// it was last read when it cannot be read (another desktop is active);
    /// `None` when it has never been read, since the origin would be a
    /// position the touchpad never reported (ADR-0054).
    fn frame(&self, hwnd: HWND, host: &mut TouchpadHost) -> Option<TouchpadFrame> {
        let mut cursor = POINT::default();
        // SAFETY: `cursor` is a live, writable local; `hwnd` is this
        // window.
        let read = unsafe {
            GetCursorPos(&raw mut cursor).is_ok()
                && windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &raw mut cursor).as_bool()
        };
        #[cfg(test)]
        let read = read && !host.cursor_unreadable;
        let position = if read {
            (f64::from(cursor.x), f64::from(cursor.y))
        } else {
            host.cursor?
        };
        host.cursor = Some(position);
        Some(TouchpadFrame {
            time: now(),
            position,
            scale: self.scale_factor(),
            modifiers: super::raw::message_modifiers(),
        })
    }

    /// The system offered a contact (`DM_POINTERHITTEST`): give it to the
    /// viewport when it is a touchpad's, and start pumping.
    pub(super) fn offer_touchpad_contact(&self, hwnd: HWND, pointer: u32) {
        let mut kind = POINTER_INPUT_TYPE::default();
        // SAFETY: `kind` is a live, writable local.
        if unsafe { GetPointerType(pointer, &raw mut kind) }.is_err() || kind != PT_TOUCHPAD {
            return;
        }
        let Some(viewport) = self.touchpad.accepting() else {
            return;
        };
        if let Err(error) = viewport.set_contact(pointer) {
            tracing::debug!(?error, pointer, "DirectManipulation declined a contact");
            return;
        }
        drop(viewport);
        Self::keep_pumping(hwnd);
        self.pump_touchpad(hwnd);
    }

    /// The frame's pump: only a viewport that runs or coasts has anything
    /// to deliver, so a resting one costs the frame nothing.
    pub(super) fn pump_touchpad_for_frame(&self, hwnd: HWND) {
        if self
            .touchpad
            .host
            .borrow()
            .as_ref()
            .is_some_and(TouchpadHost::animating)
        {
            self.pump_touchpad(hwnd);
        }
    }

    /// Run the viewport, reset it if it stopped, schedule the next pump while
    /// it moves, and deliver what it reported.
    pub(super) fn pump_touchpad(&self, hwnd: HWND) {
        let Some(viewport) = self.touchpad.accepting() else {
            return;
        };
        let readings = match viewport.update() {
            Ok(readings) => readings,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "DirectManipulation failed; the touchpad viewport is released"
                );
                self.touchpad_failed(hwnd);
                return;
            }
        };
        let mut inputs = Vec::new();
        let mut reset = false;
        let frame = {
            let mut host = self.touchpad.host.borrow_mut();
            // Released while `Update` ran (a failure seen by a nested pump).
            let Some(host) = host.as_mut() else { return };
            self.frame(hwnd, host)
        };
        let Some(frame) = frame else {
            // The cursor has never been read (another desktop is active):
            // nothing can place this manipulation, so it is dropped rather
            // than kept and pumped for as long as that lasts. The viewport
            // stops, returns to the origin and drops its contacts; the next
            // contact's hit test starts it again.
            drop(readings);
            drop(viewport);
            self.abandon_touchpad(hwnd, false);
            return;
        };
        let animating = {
            let mut host = self.touchpad.host.borrow_mut();
            let Some(host) = host.as_mut() else { return };
            let mut registry = self.registry.borrow_mut();
            for reading in readings {
                if let ViewportReading::Status(status) = reading {
                    host.status = status;
                }
                let step = host.assembler.assemble(&mut registry, reading, frame);
                inputs.extend(step.inputs);
                reset |= step.reset;
            }
            host.animating()
        };
        if reset && let Err(error) = viewport.reset() {
            tracing::debug!(?error, "DirectManipulation could not reset its viewport");
        }
        drop(viewport);
        if animating {
            Self::keep_pumping(hwnd);
        } else {
            Self::stop_pumping(hwnd);
        }
        let delivering = !inputs.is_empty();
        self.deliver_touchpad(inputs);
        if delivering && animating {
            // The viewport's own frame request: its next step is drawn
            // without the application asking for a frame.
            // SAFETY: `InvalidateRect` takes this window's handle and no
            // rectangle (the whole client area); on a destroyed window it
            // fails harmlessly.
            let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
        }
    }

    /// Deliver `inputs` behind whatever an outer delivery on this window has
    /// not delivered yet, in order (see the module's "Pumping"). Their stamps
    /// raise the window clock's floor first, so input stamped from a message
    /// after them (a key, the cancels a focus loss ends the other contacts
    /// with) is not placed before them.
    fn deliver_touchpad(&self, inputs: Vec<PlatformInput>) {
        self.message_clock.observe(&inputs);
        let slot = &self.touchpad;
        slot.outbox.borrow_mut().extend(inputs);
        if slot.delivering.replace(true) {
            return;
        }
        /// Ends this delivery even when an input callback unwinds, so the
        /// next pump delivers the tail left in the outbox.
        struct Delivering<'a>(&'a Cell<bool>);
        impl Drop for Delivering<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _delivering = Delivering(&slot.delivering);
        loop {
            let next = slot.outbox.borrow_mut().pop_front();
            let Some(input) = next else { break };
            self.callbacks.dispatch_input(input);
        }
    }

    fn keep_pumping(hwnd: HWND) {
        // SAFETY: `SetTimer` takes this window's handle, a constant id and
        // no callback (the timer posts `WM_TIMER`); setting it again
        // replaces it, and on a destroyed window it fails harmlessly.
        let _ = unsafe { SetTimer(Some(hwnd), PUMP_TIMER, PUMP_INTERVAL_MS, None) };
    }

    fn stop_pumping(hwnd: HWND) {
        // SAFETY: `KillTimer` takes this window's handle and the constant id;
        // a timer that is not set fails harmlessly.
        let _ = unsafe { KillTimer(Some(hwnd), PUMP_TIMER) };
    }

    /// The window's client area changed size.
    pub(super) fn resize_touchpad(&self, size: (i32, i32)) {
        let viewport = self.touchpad.viewport();
        if let Some(viewport) = viewport
            && let Err(error) = viewport.set_rect(size)
        {
            tracing::debug!(?error, "DirectManipulation could not resize its viewport");
        }
    }

    /// `WM_ENTERSIZEMOVE`: end what is open as cancelled and decline contacts
    /// until the loop ends.
    pub(super) fn pause_touchpad(&self, hwnd: HWND) {
        self.abandon_touchpad(hwnd, true);
    }

    /// The window lost focus or was minimised: end what is open as cancelled
    /// and return the content to the origin. Contacts are still accepted, so
    /// an inactive window scrolls under the cursor as Windows' do; a paused
    /// viewport stays paused.
    pub(super) fn cancel_touchpad(&self, hwnd: HWND) {
        self.abandon_touchpad(hwnd, false);
    }

    /// `WM_EXITSIZEMOVE`: accept contacts again.
    pub(super) fn resume_touchpad(&self) {
        if let Some(host) = self.touchpad.host.borrow_mut().as_mut() {
            host.paused = false;
        }
    }

    /// The viewport failed: release it, and stop claiming precise scrolling
    /// and trackpad gestures, since two-finger scrolling now reaches the
    /// window as wheel ticks.
    fn touchpad_failed(&self, hwnd: HWND) {
        self.release_touchpad(hwnd);
        self.callbacks
            .set_input_capabilities(crate::input::availability::windows(self.has_touchpad()));
    }

    /// The viewport failed or the window is going away: end what is open as
    /// cancelled and release every interface.
    pub(super) fn release_touchpad(&self, hwnd: HWND) {
        // Taken first, so a pump that user code runs while the cancellation
        // is delivered finds no viewport. Dropping the host releases the
        // viewport once no call holds it; the apartment it holds outlives its
        // interfaces.
        let host = self.touchpad.host.borrow_mut().take();
        if let Some(mut host) = host {
            let inputs = self.abandon_host(hwnd, &mut host);
            drop(host);
            self.deliver_touchpad(inputs);
        }
    }

    /// End what is open; `pause` also declines contacts until
    /// [`Self::resume_touchpad`]. Without it a paused viewport stays paused.
    fn abandon_touchpad(&self, hwnd: HWND, pause: bool) {
        let Some(mut host) = self.touchpad.host.borrow_mut().take() else {
            return;
        };
        host.paused |= pause;
        // The host is out of its slot while COM runs: a reentrant message
        // finds no viewport rather than a borrowed one.
        let inputs = self.abandon_host(hwnd, &mut host);
        let restored = self.touchpad.host.borrow_mut().replace(host);
        debug_assert!(restored.is_none(), "BUG: no viewport is installed twice");
        self.deliver_touchpad(inputs);
    }

    /// Stop `host`'s viewport, return its content to the origin, and end
    /// what was open as cancelled.
    fn abandon_host(&self, hwnd: HWND, host: &mut TouchpadHost) -> Vec<PlatformInput> {
        host.viewport.stop();
        host.status = ViewportStatus::Ready;
        // A host that never read the cursor decoded nothing (a pump that
        // cannot place its readings drops them), so nothing is open to
        // cancel; its content is still returned to the origin.
        let (inputs, reset) = match self.frame(hwnd, host) {
            Some(frame) => {
                let delivered = host
                    .assembler
                    .abandon(&mut self.registry.borrow_mut(), frame);
                (delivered.inputs, delivered.reset)
            }
            None => (Vec::new(), true),
        };
        if reset && let Err(error) = host.viewport.reset() {
            tracing::debug!(?error, "DirectManipulation could not reset its viewport");
        }
        // What the viewport reported, the stop and the reset included, is of
        // a manipulation that no longer exists; the decoder measures the next
        // one from the origin.
        host.viewport.queue.borrow_mut().clear();
        Self::stop_pumping(hwnd);
        inputs
    }

    /// Whether `timer` is the viewport's pacing timer.
    pub(super) fn is_touchpad_timer(timer: usize) -> bool {
        timer == PUMP_TIMER
    }
}

/// Create the viewport for `hwnd`; `None` (logged) when DirectManipulation
/// is unavailable, which leaves the window without precise scrolling and
/// trackpad gestures.
#[cfg(windows)]
pub(super) fn create_host(
    hwnd: HWND,
    size: (i32, i32),
    apartment: Option<Rc<ComApartment>>,
) -> Option<TouchpadHost> {
    let apartment = apartment?;
    match TouchpadHost::create(hwnd, size, apartment) {
        Ok(host) => Some(host),
        Err(error) => {
            tracing::warn!(
                ?hwnd,
                ?error,
                "DirectManipulation did not initialise; the precision touchpad scrolls as wheel ticks"
            );
            None
        }
    }
}

/// What a window's input holds of its precision touchpad.
#[cfg(windows)]
#[derive(Default)]
pub(super) struct TouchpadSlot {
    /// The viewport and its assembler; `None` when DirectManipulation did
    /// not initialise, after it failed, or once the window is destroyed.
    host: RefCell<Option<TouchpadHost>>,
    /// Decoded input not delivered yet, in decode order.
    outbox: RefCell<VecDeque<PlatformInput>>,
    /// Whether a delivery is draining `outbox`.
    delivering: Cell<bool>,
}

#[cfg(windows)]
impl TouchpadSlot {
    /// Install the window's host.
    pub(super) fn install(&self, host: TouchpadHost) {
        let previous = self.host.borrow_mut().replace(host);
        debug_assert!(previous.is_none(), "BUG: a window installs one viewport");
    }

    /// The viewport, to call with no borrow held.
    fn viewport(&self) -> Option<Rc<ComViewport>> {
        self.host
            .borrow()
            .as_ref()
            .map(|host| Rc::clone(&host.viewport))
    }

    /// The viewport while it is not paused for the size and move loop.
    fn accepting(&self) -> Option<Rc<ComViewport>> {
        self.host
            .borrow()
            .as_ref()
            .filter(|host| !host.paused)
            .map(|host| Rc::clone(&host.viewport))
    }
}

#[cfg(all(windows, test))]
mod tests;

#[doc(hidden)]
pub mod manipulation {
    //! A precision touchpad as DirectManipulation reports it (ADR-0217 §4).
    //!
    //! A Win32 viewport reports two things: its status (`Running` while fingers
    //! manipulate it, `Inertia` while it coasts, `Ready` once it stopped) and the
    //! content transform, which accumulates the manipulation since the viewport
    //! last stood at the origin. [`ManipulationDecoder`] turns that stream into
    //! the owned grammar every backend shares: a two-finger pan is a phased
    //! scroll whose coasting is `ScrollPhase::Momentum*`, a pinch is a
    //! [`GestureReading`] on the trackpad contact. [`TouchpadAssembler`] adds
    //! the cursor position, clock and identity and returns finished
    //! [`PlatformInput`]s.
    //!
    //! Everything here is plain data and cfg-free: the Win32 backend copies each
    //! COM callback into a [`ViewportReading`], and Linux CI replays recordings of
    //! them through the same code.
    //!
    //! One manipulation is a pan or a pinch, decided by whichever passes its slop
    //! first; the translation of a pinch is the centroid moving, and is not a
    //! scroll. A viewport that returns to `Ready` asks to be reset to the origin
    //! ([`Delivered::reset`]); the content update that reset causes arrives while
    //! no manipulation is open and is not input.

    #![forbid(unsafe_code)]

    use flui_foundation::geometry::Point;
    use flui_platform::pointer::{
        PointerEvent, PointerPosition, ScrollDelta, ScrollEvent, ScrollPrecision, ScrollUnit,
    };
    use smallvec::SmallVec;

    use crate::input::assembly::{
        ClockChoice, ContactRegistry, ContentTransform, DecodeError, GestureComponent,
        GestureDelta, GesturePhase, GestureReading, NativeContact, NativeReading, NativeSurface,
        ReadingAction, ScrollPhases, ScrollReading, TouchpadFrame, ViewportReading, ViewportStatus,
        decode_derived,
    };
    use crate::input::primary_mouse_info;
    use flui_platform::PlatformInput;

    /// Translation, in device pixels, a pan must travel before it is a pan.
    pub const PAN_SLOP: f64 = 2.0;

    /// Relative scale change a pinch must reach before it is a pinch.
    pub const PINCH_SLOP: f64 = 0.02;

    /// The trackpad contact every pinch of a window reads as: outside the 32-bit
    /// range of `WM_POINTER` ids.
    pub const PINCH_CONTACT: NativeContact = NativeContact(1 << 33);

    /// What the decoder publishes for a reading.
    #[derive(Debug, Clone, Copy, PartialEq)]
    #[non_exhaustive]
    pub enum Manipulation {
        /// A scroll step: `dx` and `dy` are device pixels, positive when the
        /// scroll offset grows (content moves up and left under the fingers).
        Scroll {
            /// The native phases.
            reading: ScrollReading,
            /// Horizontal distance.
            dx: f64,
            /// Vertical distance.
            dy: f64,
        },
        /// A pinch component.
        Pinch(GestureReading),
        /// The viewport has stopped: return its content to the origin.
        ResetViewport,
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum State {
        /// No manipulation is open.
        Idle,
        /// Fingers are down and have not passed a slop yet.
        Undecided,
        Panning,
        Pinching,
        /// The viewport coasts.
        Momentum,
        /// The gesture ended but the viewport still coasts (a pinch's centroid
        /// drift): nothing is published, and its stop still resets it.
        Settling,
    }

    /// Turns a viewport's statuses and content transforms into scrolls, momentum
    /// and pinches.
    #[derive(Debug, Clone, Copy)]
    pub struct ManipulationDecoder {
        state: State,
        /// The last transform seen: where the next delta is measured from.
        anchor: ContentTransform,
    }

    impl Default for ManipulationDecoder {
        fn default() -> Self {
            Self {
                state: State::Idle,
                anchor: ContentTransform::IDENTITY,
            }
        }
    }

    fn scroll(
        fingers: Option<GesturePhase>,
        momentum: Option<GesturePhase>,
        dx: f64,
        dy: f64,
    ) -> Manipulation {
        Manipulation::Scroll {
            reading: ScrollReading {
                fingers,
                momentum,
                precise: true,
            },
            dx,
            dy,
        }
    }

    fn pinch(phase: GesturePhase, delta: GestureDelta) -> Manipulation {
        Manipulation::Pinch(GestureReading {
            component: GestureComponent::Scale,
            phase,
            delta,
        })
    }

    impl ManipulationDecoder {
        /// Whether a manipulation is open.
        #[must_use]
        pub const fn is_open(&self) -> bool {
            !matches!(self.state, State::Idle)
        }

        /// Apply `reading` and return what it publishes, in order.
        pub fn apply(&mut self, reading: ViewportReading) -> SmallVec<[Manipulation; 3]> {
            match reading {
                ViewportReading::Status(status) => self.status(status),
                ViewportReading::Content(transform) => self.content(transform),
            }
        }

        /// End whatever is open as cancelled: the viewport was lost.
        pub fn abandon(&mut self) -> SmallVec<[Manipulation; 3]> {
            self.status(ViewportStatus::Disabled)
        }

        fn status(&mut self, status: ViewportStatus) -> SmallVec<[Manipulation; 3]> {
            use GesturePhase::{Began, Cancelled, Ended};
            let mut out = SmallVec::new();
            let state = self.state;
            match status {
                ViewportStatus::Running => {
                    if state == State::Momentum {
                        out.push(scroll(None, Some(Ended), 0.0, 0.0));
                    }
                    if matches!(state, State::Idle | State::Momentum | State::Settling) {
                        self.state = State::Undecided;
                    }
                }
                ViewportStatus::Inertia => match state {
                    State::Panning => {
                        out.push(scroll(Some(Ended), None, 0.0, 0.0));
                        out.push(scroll(None, Some(Began), 0.0, 0.0));
                        self.state = State::Momentum;
                    }
                    State::Pinching => {
                        out.push(pinch(Ended, GestureDelta::None));
                        self.state = State::Settling;
                    }
                    State::Undecided | State::Idle => {
                        out.push(scroll(None, Some(Began), 0.0, 0.0));
                        self.state = State::Momentum;
                    }
                    State::Momentum | State::Settling => {}
                },
                ViewportStatus::Ready => {
                    self.finish(&mut out, Ended, Ended);
                }
                ViewportStatus::Building
                | ViewportStatus::Enabled
                | ViewportStatus::Disabled
                | ViewportStatus::Suspended => {
                    self.finish(&mut out, Cancelled, Ended);
                }
            }
            out
        }

        /// Close the open manipulation with `fingers` for a pan or pinch and
        /// `momentum` for coasting, and ask for a reset.
        ///
        /// The next manipulation is measured from the origin the reset returns
        /// the content to, so the content updates between this reset and the
        /// next contact need not be seen (a paused window discards them).
        fn finish(
            &mut self,
            out: &mut SmallVec<[Manipulation; 3]>,
            fingers: GesturePhase,
            momentum: GesturePhase,
        ) {
            match self.state {
                State::Idle => return,
                State::Undecided | State::Settling => {}
                State::Panning => out.push(scroll(Some(fingers), None, 0.0, 0.0)),
                State::Pinching => out.push(pinch(fingers, GestureDelta::None)),
                State::Momentum => out.push(scroll(None, Some(momentum), 0.0, 0.0)),
            }
            self.state = State::Idle;
            self.anchor = ContentTransform::IDENTITY;
            out.push(Manipulation::ResetViewport);
        }

        fn content(&mut self, transform: ContentTransform) -> SmallVec<[Manipulation; 3]> {
            use GesturePhase::{Began, Changed};
            let mut out = SmallVec::new();
            let moved = (
                transform.x() - self.anchor.x(),
                transform.y() - self.anchor.y(),
            );
            let zoom = transform.scale() / self.anchor.scale();
            // `0.0 -` also turns a negative zero into zero.
            let (dx, dy) = (0.0 - moved.0, 0.0 - moved.1);
            match self.state {
                State::Idle | State::Settling => self.anchor = transform,
                State::Undecided => {
                    if (zoom - 1.0).abs() >= PINCH_SLOP {
                        out.push(pinch(Began, GestureDelta::Scale(zoom)));
                        self.state = State::Pinching;
                        self.anchor = transform;
                    } else if moved.0.hypot(moved.1) >= PAN_SLOP {
                        out.push(scroll(Some(Began), None, dx, dy));
                        self.state = State::Panning;
                        self.anchor = transform;
                    }
                }
                State::Panning => {
                    if moved != (0.0, 0.0) {
                        out.push(scroll(Some(Changed), None, dx, dy));
                    }
                    self.anchor = transform;
                }
                State::Pinching => {
                    if zoom != 1.0 {
                        out.push(pinch(Changed, GestureDelta::Scale(zoom)));
                    }
                    self.anchor = transform;
                }
                State::Momentum => {
                    if moved != (0.0, 0.0) {
                        out.push(scroll(None, Some(Changed), dx, dy));
                    }
                    self.anchor = transform;
                }
            }
            out
        }
    }

    /// The input a viewport reading delivers.
    #[derive(Debug, Default)]
    pub struct Delivered {
        /// Scrolls and pinch phases, in order.
        pub inputs: Vec<PlatformInput>,
        /// Whether the viewport stopped and should return to the origin.
        pub reset: bool,
    }

    /// A window's precision-touchpad input: the decoder plus the phase grammar.
    #[derive(Debug)]
    pub struct TouchpadAssembler {
        surface: NativeSurface,
        decoder: ManipulationDecoder,
        scrolling: ScrollPhases,
    }

    impl TouchpadAssembler {
        /// An assembler for `surface`.
        #[must_use]
        pub fn new(surface: NativeSurface) -> Self {
            Self {
                surface,
                decoder: ManipulationDecoder::default(),
                scrolling: ScrollPhases::default(),
            }
        }

        /// Whether a manipulation is open.
        #[must_use]
        pub const fn is_open(&self) -> bool {
            self.decoder.is_open()
        }

        /// Decode `reading` at `frame`, assembling pinches against `contacts`.
        pub fn assemble(
            &mut self,
            contacts: &mut ContactRegistry,
            reading: ViewportReading,
            frame: TouchpadFrame,
        ) -> Delivered {
            #[cfg(all(debug_assertions, feature = "native-capture"))]
            contacts.report(crate::input::assembly::capture::Captured::Viewport { reading, frame });
            let steps = self.decoder.apply(reading);
            self.deliver(contacts, &steps, frame)
        }

        /// End whatever is open as cancelled: the window or its viewport is
        /// going away.
        pub fn abandon(
            &mut self,
            contacts: &mut ContactRegistry,
            frame: TouchpadFrame,
        ) -> Delivered {
            #[cfg(all(debug_assertions, feature = "native-capture"))]
            contacts.report(crate::input::assembly::capture::Captured::ViewportAbandoned { frame });
            let steps = self.decoder.abandon();
            let delivered = self.deliver(contacts, &steps, frame);
            self.scrolling.cancel_surface(self.surface);
            delivered
        }

        fn deliver(
            &mut self,
            contacts: &mut ContactRegistry,
            steps: &[Manipulation],
            frame: TouchpadFrame,
        ) -> Delivered {
            let mut delivered = Delivered::default();
            for step in steps {
                match *step {
                    Manipulation::ResetViewport => delivered.reset = true,
                    Manipulation::Scroll { reading, dx, dy } => {
                        delivered
                            .inputs
                            .extend(self.scroll(contacts, reading, (dx, dy), frame));
                    }
                    Manipulation::Pinch(gesture) => {
                        let (x, y) = frame.position;
                        let native = NativeReading::new(
                            PINCH_CONTACT,
                            flui_platform::pointer::PointerKind::Trackpad,
                            ReadingAction::Gesture(gesture),
                            frame.time,
                        )
                        .on(self.surface)
                        .with_primary(true)
                        .with_modifiers(frame.modifiers)
                        .at(x, y, frame.scale);
                        delivered.inputs.extend(
                            decode_derived(contacts, &native)
                                .unwrap_or_else(DecodeError::into_inputs),
                        );
                    }
                }
            }
            delivered
        }

        fn scroll(
            &mut self,
            contacts: &ContactRegistry,
            reading: ScrollReading,
            (dx, dy): (f64, f64),
            frame: TouchpadFrame,
        ) -> Option<PlatformInput> {
            let phase = self.scrolling.phase(self.surface, None, reading)?;
            let time = contacts.clock().stamp(frame.time, ClockChoice::Counter)?;
            let scale = frame.scale;
            let position = PointerPosition::try_new(Point::new(
                frame.position.0 / scale,
                frame.position.1 / scale,
            ))
            .ok()?;
            let delta = ScrollDelta::try_new(ScrollUnit::Pixels, dx / scale, dy / scale)
                .unwrap_or_else(|_| ScrollDelta::zero(ScrollUnit::Pixels));
            Some(PlatformInput::Pointer(PointerEvent::Scroll(
                ScrollEvent::new(primary_mouse_info(), time, position, delta)
                    .with_precision(ScrollPrecision::Precise)
                    .with_phase(phase)
                    .with_modifiers(frame.modifiers),
            )))
        }
    }

    #[cfg(test)]
    mod tests;
}
