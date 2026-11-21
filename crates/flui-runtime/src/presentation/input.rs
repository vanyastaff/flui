//! Input a presentation owns: its gesture binding, the held pointer queue, the
//! input regions handed to the window and the haptics host.

use std::cell::RefCell;
use std::rc::Rc;

use flui_interaction::binding::GestureBinding;

use super::PresentationState;
use crate::held_input::HeldPointerQueue;

impl PresentationState {
    #[must_use]
    pub(crate) fn gestures(&self) -> &GestureBinding {
        &self.gestures
    }

    /// The bounded queue used while this presentation has no committed tree.
    #[must_use]
    pub(crate) fn held_pointer_input(&self) -> &RefCell<HeldPointerQueue> {
        &self.held_pointer_input
    }

    /// Collects the input regions of the frame whose scene is being built, from the tree
    /// that scene was painted from.
    pub(crate) fn stage_input_regions(&self) {
        let regions = self
            .pipeline
            .with(flui_rendering::pipeline::PipelineOwner::input_regions);
        let mut staged = self.staged_input_regions.borrow_mut();
        // A frame whose regions did not change keeps the staged slice, so a
        // steady frame allocates no new one.
        if **staged != *regions {
            *staged = Rc::<[flui_platform::input::regions::InputRegion]>::from(regions);
        }
    }

    /// The input regions the frame about to be submitted carries: the slice the window gets
    /// once that frame is shown ([`Self::publish_input_regions`]).
    pub(crate) fn staged_input_regions(&self) -> Rc<[flui_platform::input::regions::InputRegion]> {
        Rc::clone(&self.staged_input_regions.borrow())
    }

    /// Hands the window `regions`, the input regions of a frame the sink accepted. Called on
    /// every composited frame, changed or not: the backend compares its projection with
    /// what the OS holds (ADR-0217 §5).
    ///
    /// The regions are committed before the backend runs, and no borrow is held while it
    /// does: a backend that panics or reenters leaves them committed for the next move.
    pub(crate) fn publish_input_regions(
        &self,
        regions: Rc<[flui_platform::input::regions::InputRegion]>,
    ) {
        *self.published_input_regions.borrow_mut() = Rc::clone(&regions);
        self.hand_input_regions(&regions);
    }

    /// Hands the window the last committed input regions again, after it moved: on another
    /// display the backend's projection key differs, so it re-sends them without waiting
    /// for a frame.
    pub(crate) fn republish_input_regions(&self) {
        let regions = Rc::clone(&self.published_input_regions.borrow());
        self.hand_input_regions(&regions);
    }

    /// Hands `regions` to the window. A window whose input capabilities leave input regions
    /// `Unavailable` refuses them on every frame; the regions then have no effect, which
    /// those capabilities already say, so the refusal is only traced.
    fn hand_input_regions(&self, regions: &[flui_platform::input::regions::InputRegion]) {
        if let Some(Err(refused)) = self.with_window(|window| window.set_input_regions(regions)) {
            tracing::trace!(%refused, "the window takes no input regions");
        }
    }

    /// This presentation's haptics host: its frame queue and outcome
    /// mailbox, played on the window's actuator (ADR-0210).
    pub(crate) fn haptics(&self) -> &flui_interaction::haptics::HapticsHost {
        &self.haptics
    }

    /// Tell the haptics host whether the window can show anything: hidden,
    /// occluded or not running drains its frame queue (ADR-0210 §4).
    pub(crate) fn sync_haptics_visibility(&self) {
        self.haptics.set_visible(
            !self.clock.is_hidden()
                && self.window_execution.get() == flui_platform::WindowExecutionState::Running,
        );
    }

    // ========================================================================
    // Frame accounting and the performance overlay (moved from the retired
    // `AppBinding`)
    // ========================================================================
}
