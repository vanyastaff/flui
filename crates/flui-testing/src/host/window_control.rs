//! The host's control of a headless window, behind the `WindowHandle` its widgets reach,
//! and the close routing a runner registers for each window it opens.

use std::rc::Rc;

use flui_foundation::PresentationId;
use flui_platform::PlatformWindow;

use flui_runtime::owner_turn::{CloseRequestHandler, CloseRequestRouter, PreparedCloseRequest};

use flui_runtime::ui_runtime::UiRuntime;
use flui_view::CloseReason;

use super::CloseAsks;
use super::window::HeadlessWindow;

/// The host's control of a [`HeadlessWindow`], behind the `WindowHandle` its
/// widgets reach through `LifecycleContext::window`, as a runner installs one
/// for each window it opens. The double honours its frame and mode commands
/// at once and answers [`Closed`](flui_platform::SubmitError::Closed) once its
/// window closed; a close it is asked for waits for the host's next turn.
struct HeadlessWindowControl {
    window: Rc<HeadlessWindow>,
    presentation: PresentationId,
    close_asks: CloseAsks,
}

impl flui_platform::WindowControl for HeadlessWindowControl {
    fn state(&self) -> flui_platform::WindowState {
        self.window.state()
    }

    fn submit(
        &self,
        command: flui_platform::WindowCommand,
    ) -> Result<(), flui_platform::SubmitError> {
        if self.window.is_closed() {
            return Err(flui_platform::SubmitError::Closed);
        }
        let admitted = command.admit(&self.window.capabilities())?;
        let command = admitted
            .against(&self.window.capabilities())
            .expect("BUG: the double's capabilities never move");
        self.window
            .take(command)
            .expect("BUG: the double carries out every command its capabilities honour");
        Ok(())
    }

    /// Counted as the window's observation, and queued for the host's next
    /// turn, which asks the window's guard and closes it (a window already
    /// gone has nothing left to close), as a runner's window queue does.
    fn request_close(&self) {
        self.window.chrome.borrow_mut().close_requests += 1;
        if !self.window.is_closed() {
            self.close_asks.borrow_mut().push(self.presentation);
        }
    }
}

/// Attach the host's control of `window` to `presentation`, before its root
/// widget is attached.
pub(super) fn install_window_control(
    ui_runtime: &UiRuntime,
    presentation: PresentationId,
    window: &Rc<HeadlessWindow>,
    close_asks: &CloseAsks,
) {
    let source = flui_view::__runtime::WindowSource::new(HeadlessWindowControl {
        window: Rc::clone(window),
        presentation,
        close_asks: Rc::clone(close_asks),
    });
    assert!(
        ui_runtime.install_window(presentation, source),
        "BUG: the window's presentation was just installed"
    );
}

/// Register `presentation`, drawn in `window`, with `router`: its close guard
/// and, where its platform asks before a window closes (`close_vetoes` holds
/// the user's close), the application's `handler`, as a runner registers
/// each window it opens.
pub(super) fn register_close_requests(
    router: &CloseRequestRouter,
    close_vetoes: &[CloseReason],
    ui_runtime: &UiRuntime,
    presentation: PresentationId,
    window: &Rc<HeadlessWindow>,
    handler: Option<CloseRequestHandler>,
) {
    let address = flui_foundation::PresentationAddress {
        ui_runtime_id: ui_runtime.id(),
        presentation_id: presentation,
    };
    // A phone's window system never asks, so a runner there registers no
    // handler: nothing would ever ask it.
    let handler = handler.filter(|_| close_vetoes.contains(&CloseReason::User));
    let native = Rc::clone(window) as Rc<dyn PlatformWindow>;
    let mut prepared = PreparedCloseRequest::new(address, &native, handler);
    if let Some(guard) = ui_runtime.close_guard_of(presentation) {
        prepared = prepared.guard(guard);
    }
    let publication = router
        .publication(&prepared)
        .expect("BUG: a presentation the host just installed has no registration");
    publication.publish(prepared);
}
