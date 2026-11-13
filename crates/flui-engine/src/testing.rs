//! Test support for code that drives the raster executor without a GPU
//! (ADR-0243: the fake sits beside its contract, [`SlotBackend`]).

use std::{cell::RefCell, collections::VecDeque, rc::Rc, time::Duration};

use flui_foundation::{ManualClock, geometry::DeviceSize, memory::MemoryPressureLevel};
use flui_layer::{DamageRegion, Scene, TextureUpdates};
use flui_painting::paint::TextureId;
use flui_platform::DisplayColor;

use crate::{
    FrameLatency, UnconfiguredSurface,
    error::{EngineError, EngineResult},
    raster::PresentDisposition,
    raster_executor::{SlotBackend, SlotConfig, SlotId, SlotTarget},
};

/// What a [`ScriptedSlots`] backend does on its next call of one kind.
#[derive(Debug)]
#[non_exhaustive]
pub enum ScriptedAnswer {
    /// Return this disposition (a render) or succeed (any other call).
    Answer(PresentDisposition),
    /// Return this error.
    Fail(EngineError),
    /// Panic.
    Panic,
}

/// The target an attach named, as a [`SlotCall`] records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttachedTarget {
    /// A window's surface.
    Window,
    /// An offscreen texture of this size and format.
    Offscreen(DeviceSize, wgpu::TextureFormat),
}

/// One call the executor made on a [`ScriptedSlots`] backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SlotCall {
    /// `attach`.
    Attach(SlotId, AttachedTarget),
    /// `resize`.
    Resize(SlotId, DeviceSize),
    /// `release`.
    Release(SlotId),
    /// `restore`.
    Restore(SlotId),
    /// `set_display_color`.
    DisplayColor(SlotId, DisplayColor),
    /// `set_latency`.
    Latency(SlotId, FrameLatency),
    /// `apply_textures`; [`ScriptedSlots::textures`] holds what it applied.
    Textures(SlotId),
    /// `render`, with the damage it was asked to repaint.
    Render(SlotId, DamageRegion),
    /// `render_lent`, with whether it dropped the earlier lent scene's
    /// fonts; answered from the same script as `render`.
    Lent(SlotId, bool),
    /// `detach`.
    Detach(SlotId),
    /// `reclaim`.
    Reclaim(MemoryPressureLevel),
    /// `rebuild_device`.
    RebuildDevice,
}

/// The texture ids one `apply_textures` call on a [`ScriptedSlots`] backend
/// was given.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AppliedTextures {
    /// The slot the updates were for.
    pub slot: SlotId,
    /// The textures that received a frame, in the order given.
    pub frames: Vec<TextureId>,
    /// The textures retired, in the order given.
    pub retired: Vec<TextureId>,
}

#[derive(Default)]
struct Script {
    calls: Vec<SlotCall>,
    attaches: VecDeque<ScriptedAnswer>,
    resizes: VecDeque<ScriptedAnswer>,
    restores: VecDeque<ScriptedAnswer>,
    renders: VecDeque<ScriptedAnswer>,
    texture_answers: VecDeque<ScriptedAnswer>,
    textures: Vec<AppliedTextures>,
    rebuilds: VecDeque<ScriptedAnswer>,
    /// The slots the next successful rebuild cannot present to.
    rebuild_refusals: Vec<(SlotId, EngineError)>,
    device_lost: bool,
    lose_in_next_render: bool,
    no_inline_rebuild: bool,
    /// The clock each rebuild advances, and by how much.
    rebuild_takes: Option<(ManualClock, Duration)>,
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

    /// Answer the next unanswered restore with `answer`.
    pub fn script_restore(&self, answer: ScriptedAnswer) {
        self.script.borrow_mut().restores.push_back(answer);
    }

    /// Answer the next unanswered `apply_textures` with `answer`.
    pub fn script_textures(&self, answer: ScriptedAnswer) {
        self.script.borrow_mut().texture_answers.push_back(answer);
    }

    /// The texture updates applied so far, oldest first.
    #[must_use]
    pub fn textures(&self) -> Vec<AppliedTextures> {
        self.script.borrow().textures.clone()
    }

    /// Lose the device: every render fails
    /// [`EngineError::DeviceLost`] until a rebuild succeeds.
    pub fn lose_device(&self) {
        self.script.borrow_mut().device_lost = true;
    }

    /// Lose the device inside the next render, which fails
    /// [`EngineError::DeviceLost`], as wgpu's device-lost callback can fire
    /// while a frame records.
    pub fn lose_device_in_next_render(&self) {
        self.script.borrow_mut().lose_in_next_render = true;
    }

    /// Answer that no device rebuild can succeed where the executor runs, as
    /// the web's context does.
    pub fn cannot_rebuild_inline(&self) {
        self.script.borrow_mut().no_inline_rebuild = true;
    }

    /// Every device rebuild from now on advances `clock` by `took`, as a
    /// driver that blocks the attempt does.
    pub fn rebuilds_take(&self, clock: &ManualClock, took: Duration) {
        self.script.borrow_mut().rebuild_takes = Some((clock.clone(), took));
    }

    /// Answer the next unanswered device rebuild with `answer`; one the
    /// script does not answer succeeds. A [`ScriptedAnswer::Panic`] panics
    /// once the new device is in place, so the backend no longer reports the
    /// device lost.
    pub fn script_rebuild(&self, answer: ScriptedAnswer) {
        self.script.borrow_mut().rebuilds.push_back(answer);
    }

    /// The next successful rebuild answers that the new device cannot
    /// present to `slot`, for `error`.
    pub fn refuse_on_rebuild(&self, slot: SlotId, error: EngineError) {
        self.script
            .borrow_mut()
            .rebuild_refusals
            .push((slot, error));
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
            SlotCall::Render(..) | SlotCall::Lent(..) => script.renders.pop_front(),
            SlotCall::Textures(_) => script.texture_answers.pop_front(),
            SlotCall::Resize(..) => script.resizes.pop_front(),
            SlotCall::Restore(_) => script.restores.pop_front(),
            SlotCall::Release(_)
            | SlotCall::DisplayColor(..)
            | SlotCall::Latency(..)
            | SlotCall::Detach(_)
            | SlotCall::Reclaim(_) => None,
            SlotCall::RebuildDevice => script.rebuilds.pop_front(),
        }
    }

    /// Succeed, fail or panic as the next scripted answer of `call`'s kind
    /// says.
    fn succeed(&self, call: SlotCall, what: &str) -> EngineResult<()> {
        match self.answer(call) {
            None | Some(ScriptedAnswer::Answer(_)) => Ok(()),
            Some(ScriptedAnswer::Fail(error)) => Err(error),
            Some(ScriptedAnswer::Panic) => panic!("scripted {what} panic"),
        }
    }
}

impl SlotBackend for ScriptedSlots {
    fn attach(
        &mut self,
        slot: SlotId,
        target: SlotTarget,
        _config: SlotConfig,
    ) -> EngineResult<()> {
        let attached = match target {
            SlotTarget::Window(_) => AttachedTarget::Window,
            SlotTarget::Offscreen { size, format } => AttachedTarget::Offscreen(size, format),
        };
        // The surface, if any, drops here, after the script is released.
        self.succeed(SlotCall::Attach(slot, attached), "attach")
    }

    fn resize(&mut self, slot: SlotId, size: DeviceSize) -> EngineResult<()> {
        self.succeed(SlotCall::Resize(slot, size), "resize")
    }

    fn release(&mut self, slot: SlotId) {
        let _none = self.answer(SlotCall::Release(slot));
    }

    fn restore(&mut self, slot: SlotId, surface: UnconfiguredSurface) -> EngineResult<()> {
        let answer = self.succeed(SlotCall::Restore(slot), "restore");
        drop(surface);
        answer
    }

    fn set_display_color(&mut self, slot: SlotId, color: DisplayColor) -> EngineResult<()> {
        let _none = self.answer(SlotCall::DisplayColor(slot, color));
        Ok(())
    }

    fn set_latency(&mut self, slot: SlotId, latency: FrameLatency) {
        let _none = self.answer(SlotCall::Latency(slot, latency));
    }

    fn apply_textures(&mut self, slot: SlotId, updates: TextureUpdates) -> EngineResult<()> {
        let applied = AppliedTextures {
            slot,
            frames: updates.frames.iter().map(|(id, _)| *id).collect(),
            retired: updates.retired.clone(),
        };
        self.script.borrow_mut().textures.push(applied);
        // The images drop here, after the script is released.
        self.succeed(SlotCall::Textures(slot), "textures")
    }

    fn render(
        &mut self,
        slot: SlotId,
        _scene: &Scene,
        damage: DamageRegion,
    ) -> EngineResult<PresentDisposition> {
        {
            let mut script = self.script.borrow_mut();
            if std::mem::take(&mut script.lose_in_next_render) {
                script.device_lost = true;
            }
            if script.device_lost {
                script.calls.push(SlotCall::Render(slot, damage));
                return Err(EngineError::DeviceLost);
            }
        }
        match self.answer(SlotCall::Render(slot, damage)) {
            None => Ok(PresentDisposition::Presented),
            Some(ScriptedAnswer::Answer(disposition)) => Ok(disposition),
            Some(ScriptedAnswer::Fail(error)) => Err(error),
            Some(ScriptedAnswer::Panic) => panic!("scripted render panic"),
        }
    }

    fn render_lent(
        &mut self,
        slot: SlotId,
        _scene: &Scene,
        reset_fonts: bool,
    ) -> EngineResult<PresentDisposition> {
        if self.script.borrow().device_lost {
            self.script
                .borrow_mut()
                .calls
                .push(SlotCall::Lent(slot, reset_fonts));
            return Err(EngineError::DeviceLost);
        }
        match self.answer(SlotCall::Lent(slot, reset_fonts)) {
            None => Ok(PresentDisposition::Presented),
            Some(ScriptedAnswer::Answer(disposition)) => Ok(disposition),
            Some(ScriptedAnswer::Fail(error)) => Err(error),
            Some(ScriptedAnswer::Panic) => panic!("scripted lent render panic"),
        }
    }

    fn detach(&mut self, slot: SlotId) {
        let _none = self.answer(SlotCall::Detach(slot));
    }

    fn reclaim(&mut self, level: MemoryPressureLevel) {
        let _none = self.answer(SlotCall::Reclaim(level));
    }

    fn device_lost(&self) -> bool {
        self.script.borrow().device_lost
    }

    fn rebuild_device(&mut self) -> EngineResult<Vec<(SlotId, EngineError)>> {
        let takes = self.script.borrow().rebuild_takes.clone();
        if let Some((clock, took)) = takes {
            clock.advance(took);
        }
        match self.answer(SlotCall::RebuildDevice) {
            None | Some(ScriptedAnswer::Answer(_)) => {
                let mut script = self.script.borrow_mut();
                script.device_lost = false;
                Ok(std::mem::take(&mut script.rebuild_refusals))
            }
            Some(ScriptedAnswer::Fail(error)) => Err(error),
            Some(ScriptedAnswer::Panic) => {
                // The new device is in place when the rebuild unwinds.
                self.script.borrow_mut().device_lost = false;
                panic!("scripted device rebuild panic")
            }
        }
    }

    fn rebuilds_inline(&self) -> bool {
        !self.script.borrow().no_inline_rebuild
    }
}

/// A surface factory over wgpu's no-op backend, on `display`: its surfaces
/// need neither a GPU nor a window, so a test drives a window slot's commands
/// on [`ScriptedSlots`]. No real adapter presents to them.
///
/// `display` answers the display the factory's targets must be on, as the
/// host's connection does for [`GpuContext::bootstrap`](crate::GpuContext::bootstrap).
#[must_use]
pub fn noop_surface_factory(
    display: std::sync::Arc<dyn flui_platform::DisplayConnection>,
) -> crate::SurfaceFactory {
    crate::SurfaceFactory::new(noop_instance(&display), display)
}

/// An instance of wgpu's no-op backend alone, on `display`.
pub(crate) fn noop_instance(
    display: &std::sync::Arc<dyn flui_platform::DisplayConnection>,
) -> wgpu::Instance {
    wgpu::Instance::new(crate::surface_factory::instance_descriptor(
        display,
        wgpu::Backends::NOOP,
        wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions {
                enable: true,
                ..Default::default()
            },
            ..Default::default()
        },
    ))
}

/// The display a test's [`FakeWindow`]s are on: an Xlib display handle
/// without a connection pointer, for [`noop_surface_factory`]. Nothing reads
/// through it; the no-op backend never opens a display.
#[derive(Debug, Default, Clone, Copy)]
pub struct FakeDisplay;

/// An Xlib display handle without a connection pointer.
fn fake_display_handle() -> raw_window_handle::DisplayHandle<'static> {
    let raw = raw_window_handle::RawDisplayHandle::Xlib(raw_window_handle::XlibDisplayHandle::new(
        None, 0,
    ));
    #[expect(
        unsafe_code,
        reason = "test double: a display handle without a pointer"
    )]
    // SAFETY: the handle carries no pointer, so `borrow_raw` has no pointee
    // whose validity it must be promised.
    unsafe {
        raw_window_handle::DisplayHandle::borrow_raw(raw)
    }
}

impl raw_window_handle::HasDisplayHandle for FakeDisplay {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Ok(fake_display_handle())
    }
}

impl flui_platform::DisplayConnection for FakeDisplay {}

/// A window on [`FakeDisplay`], named by an Xlib window id: a target a
/// [`noop_surface_factory`] makes a surface for, which no real adapter
/// presents to.
#[derive(Debug, Clone, Copy)]
pub struct FakeWindow {
    id: std::ffi::c_ulong,
}

impl FakeWindow {
    /// A window with Xlib id `id`.
    #[must_use]
    pub fn new(id: std::ffi::c_ulong) -> Self {
        Self { id }
    }
}

impl raw_window_handle::HasWindowHandle for FakeWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let raw = raw_window_handle::RawWindowHandle::Xlib(
            raw_window_handle::XlibWindowHandle::new(self.id),
        );
        // SAFETY: an Xlib window handle is an integer id, not a pointer, so
        // `borrow_raw` has no pointee whose validity it must be promised.
        #[expect(unsafe_code, reason = "test double: a window handle without a pointer")]
        let handle = unsafe { raw_window_handle::WindowHandle::borrow_raw(raw) };
        Ok(handle)
    }
}

impl raw_window_handle::HasDisplayHandle for FakeWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Ok(fake_display_handle())
    }
}

impl flui_platform::SurfaceTarget for FakeWindow {
    fn pre_present_notify(&self, _statistics: flui_platform::SwapChainStatistics) {}
}

/// What a test's window slot acquires instead of a surface texture.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) enum ScriptedAcquire {
    /// The surface hands out no texture in time.
    Timeout,
    /// The surface is lost, and stays lost after a reconfigure.
    Lost,
}
