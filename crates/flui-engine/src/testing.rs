//! Test support for code that drives the raster executor without a GPU
//! (ADR-0243: the fake sits beside its contract, [`SlotBackend`]).

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use flui_foundation::{geometry::DeviceSize, memory::MemoryPressureLevel};
use flui_layer::{DamageRegion, Scene};

use crate::{
    error::{EngineError, EngineResult},
    raster::PresentDisposition,
    raster_executor::{SlotBackend, SlotId, SlotTarget},
};

/// What a [`ScriptedSlots`] backend does on its next call of one kind.
#[derive(Debug)]
pub enum ScriptedAnswer {
    /// Return this disposition (a render) or succeed (any other call).
    Answer(PresentDisposition),
    /// Return this error.
    Fail(EngineError),
    /// Panic.
    Panic,
}

/// One call the executor made on a [`ScriptedSlots`] backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotCall {
    /// `attach`.
    Attach(SlotId, SlotTarget),
    /// `resize`.
    Resize(SlotId, DeviceSize),
    /// `render`, with the damage it was asked to repaint.
    Render(SlotId, DamageRegion),
    /// `detach`.
    Detach(SlotId),
    /// `reclaim`.
    Reclaim(MemoryPressureLevel),
}

#[derive(Default)]
struct Script {
    calls: Vec<SlotCall>,
    attaches: VecDeque<ScriptedAnswer>,
    resizes: VecDeque<ScriptedAnswer>,
    renders: VecDeque<ScriptedAnswer>,
}

/// A [`SlotBackend`] with no GPU: it logs every call and answers from a
/// script, presenting and succeeding once the script is spent.
///
/// Clones share one script, so a test keeps a clone after handing one to the
/// executor.
#[derive(Clone, Default)]
pub struct ScriptedSlots {
    script: Rc<RefCell<Script>>,
}

impl std::fmt::Debug for ScriptedSlots {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptedSlots")
            .field("calls", &self.script.borrow().calls.len())
            .finish_non_exhaustive()
    }
}

impl ScriptedSlots {
    /// A backend whose every call succeeds and whose every render presents.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer the next unanswered render with `answer`.
    pub fn script_render(&self, answer: ScriptedAnswer) {
        self.script.borrow_mut().renders.push_back(answer);
    }

    /// Answer the next unanswered attach with `answer`.
    pub fn script_attach(&self, answer: ScriptedAnswer) {
        self.script.borrow_mut().attaches.push_back(answer);
    }

    /// Answer the next unanswered resize with `answer`.
    pub fn script_resize(&self, answer: ScriptedAnswer) {
        self.script.borrow_mut().resizes.push_back(answer);
    }

    /// Every call so far, oldest first.
    #[must_use]
    pub fn calls(&self) -> Vec<SlotCall> {
        self.script.borrow().calls.clone()
    }

    /// The renders so far.
    #[must_use]
    pub fn renders(&self) -> Vec<(SlotId, DamageRegion)> {
        self.script
            .borrow()
            .calls
            .iter()
            .filter_map(|call| match *call {
                SlotCall::Render(slot, damage) => Some((slot, damage)),
                _ => None,
            })
            .collect()
    }

    fn answer(&self, call: SlotCall) -> Option<ScriptedAnswer> {
        let mut script = self.script.borrow_mut();
        script.calls.push(call);
        match call {
            SlotCall::Attach(..) => script.attaches.pop_front(),
            SlotCall::Render(..) => script.renders.pop_front(),
            SlotCall::Resize(..) => script.resizes.pop_front(),
            SlotCall::Detach(_) | SlotCall::Reclaim(_) => None,
        }
    }
}

impl SlotBackend for ScriptedSlots {
    fn attach(&mut self, slot: SlotId, target: SlotTarget) -> EngineResult<()> {
        match self.answer(SlotCall::Attach(slot, target)) {
            None | Some(ScriptedAnswer::Answer(_)) => Ok(()),
            Some(ScriptedAnswer::Fail(error)) => Err(error),
            Some(ScriptedAnswer::Panic) => panic!("scripted attach panic"),
        }
    }

    fn resize(&mut self, slot: SlotId, size: DeviceSize) -> EngineResult<()> {
        match self.answer(SlotCall::Resize(slot, size)) {
            None | Some(ScriptedAnswer::Answer(_)) => Ok(()),
            Some(ScriptedAnswer::Fail(error)) => Err(error),
            Some(ScriptedAnswer::Panic) => panic!("scripted resize panic"),
        }
    }

    fn render(
        &mut self,
        slot: SlotId,
        _scene: &Scene,
        damage: DamageRegion,
    ) -> EngineResult<PresentDisposition> {
        match self.answer(SlotCall::Render(slot, damage)) {
            None => Ok(PresentDisposition::Presented),
            Some(ScriptedAnswer::Answer(disposition)) => Ok(disposition),
            Some(ScriptedAnswer::Fail(error)) => Err(error),
            Some(ScriptedAnswer::Panic) => panic!("scripted render panic"),
        }
    }

    fn detach(&mut self, slot: SlotId) {
        let _none = self.answer(SlotCall::Detach(slot));
    }

    fn reclaim(&mut self, level: MemoryPressureLevel) {
        let _none = self.answer(SlotCall::Reclaim(level));
    }
}
