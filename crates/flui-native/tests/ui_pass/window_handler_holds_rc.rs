//! A window's handler lives on its owner thread only, so it may hold
//! owner-local state such as an `Rc` (ADR-0254 §4).

use std::cell::Cell;
use std::rc::Rc;

use flui_native::{OwnerPlatform, WindowOpen};
use flui_platform::{
    CloseDecision, DispatchEventResult, FrameOutcome, FrameRequest, PlatformInput, SurfaceChange,
    WindowHandler, WindowOptions,
};

struct Counting {
    seen: Rc<Cell<u32>>,
}

impl WindowHandler for Counting {
    fn input(&self, _event: PlatformInput) -> DispatchEventResult {
        self.seen.set(self.seen.get() + 1);
        DispatchEventResult::default()
    }

    fn state_changed(&self) {}

    fn frame_requested(&self, _request: FrameRequest) -> FrameOutcome {
        FrameOutcome::Declined
    }

    fn close_requested(&self) -> CloseDecision {
        CloseDecision::Close
    }

    fn closed(&self) {}

    fn surface_changed(&self, _change: SurfaceChange) {}
}

fn open(owner: &OwnerPlatform) -> Option<WindowOpen> {
    let seen = Rc::new(Cell::new(0_u32));
    owner
        .open_window(WindowOptions::default(), Rc::new(Counting { seen }))
        .ok()
}

fn main() {
    let _ = open;
}
