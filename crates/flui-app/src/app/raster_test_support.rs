//! The crate's shared [`RasterBackend`] test double.
//!
//! Every raster-facing test in this crate scripts the same small surface:
//! what `render_scene` returns (and how that answer evolves across calls),
//! plus a call counter to assert on. [`TestRasterBackend`] captures that as
//! one closure-configurable double so each test module no longer carries its
//! own hand-rolled `impl RasterBackend`.
//!
//! Fixed behavior every current test agrees on (add a knob only when a test
//! actually needs to vary it):
//! - `is_device_lost` is always `false` — doubles that script device loss
//!   also implement the runner's private `DeviceRecovery` seam and stay
//!   hand-written next to those tests,
//! - `has_damage` is always `true`,
//! - `resize` / `mark_dirty` / `mark_full_repaint` are no-ops,
//! - `reconfigure_surface` always succeeds,
//! - `size` reports `(800, 600)` unless overridden with [`with_size`].
//!
//! [`with_size`]: TestRasterBackend::with_size

use flui_engine::{EngineError, RasterBackend};
use flui_layer::Scene;
use flui_types::geometry::{Pixels, Rect};

/// The scripted `render_scene` behavior a [`TestRasterBackend`] carries:
/// zero-based call index and submitted scene in, present outcome out.
type RenderScript = Box<dyn FnMut(u32, &Scene) -> Result<bool, EngineError> + Send>;

/// Closure-configurable [`RasterBackend`] double.
///
/// Construct through [`always_presents`], [`single_shot`],
/// [`fails_once_then_presents`], or — for a behavior none of those name —
/// the general [`new`] with an explicit `render_scene` script.
///
/// [`always_presents`]: TestRasterBackend::always_presents
/// [`single_shot`]: TestRasterBackend::single_shot
/// [`fails_once_then_presents`]: TestRasterBackend::fails_once_then_presents
/// [`new`]: TestRasterBackend::new
pub(crate) struct TestRasterBackend {
    /// Scripted `render_scene` behavior. Invoked with the zero-based index
    /// of the current call (the counter's value before this call), so a
    /// script can vary by call without keeping its own state.
    render: RenderScript,
    /// How many times `render_scene` ran — the "did the scene actually
    /// reach the backend" oracle most tests assert on.
    pub(crate) render_scene_calls: u32,
    /// `Scene::has_content()` of every scene that reached `render_scene`,
    /// in call order. Recorded unconditionally (the query is cheap) so
    /// last-good-retention tests can distinguish "submitted nothing" from
    /// "submitted a blank stand-in scene".
    pub(crate) submitted_scene_had_content: Vec<bool>,
    /// What `size()` reports.
    size: (u32, u32),
    /// The installed pre-present hook, run before every script outcome
    /// that reports a present (`Ok(true)`).
    pre_present_hook: Option<flui_engine::PrePresentHook>,
}

impl TestRasterBackend {
    /// A double whose `render_scene` runs the given script.
    ///
    /// The script receives the zero-based call index and the submitted
    /// scene; call counting and content recording happen before it runs.
    pub(crate) fn new(
        render: impl FnMut(u32, &Scene) -> Result<bool, EngineError> + Send + 'static,
    ) -> Self {
        Self {
            render: Box::new(render),
            render_scene_calls: 0,
            submitted_scene_had_content: Vec::new(),
            size: (800, 600),
            pre_present_hook: None,
        }
    }

    /// Every `render_scene` call reports a successful present.
    pub(crate) fn always_presents() -> Self {
        Self::new(|_, _| Ok(true))
    }

    /// Exactly one `render_scene` call is allowed; it returns `outcome`.
    ///
    /// A second call panics: single-frame tests script one outcome and rely
    /// on the panic to catch a frame that unexpectedly reaches the backend
    /// twice (`EngineError` is not `Clone`, so the outcome cannot simply be
    /// replayed).
    pub(crate) fn single_shot(outcome: Result<bool, EngineError>) -> Self {
        let mut outcome = Some(outcome);
        Self::new(move |_, _| {
            outcome
                .take()
                .expect("render_scene called more than once in a single-frame test")
        })
    }

    /// The first `render_scene` call fails with `error`; every call after
    /// reports a successful present — the shape a transient submit failure
    /// followed by a genuine retry produces.
    pub(crate) fn fails_once_then_presents(error: EngineError) -> Self {
        let mut error = Some(error);
        Self::new(move |_, _| match error.take() {
            Some(error) => Err(error),
            None => Ok(true),
        })
    }

    /// Override the reported surface size (default `(800, 600)`).
    pub(crate) fn with_size(mut self, width: u32, height: u32) -> Self {
        self.size = (width, height);
        self
    }
}

impl RasterBackend for TestRasterBackend {
    fn render_scene(&mut self, scene: &Scene) -> Result<bool, EngineError> {
        let call_index = self.render_scene_calls;
        self.render_scene_calls += 1;
        self.submitted_scene_had_content.push(scene.has_content());
        let outcome = (self.render)(call_index, scene);
        // Mirror the wgpu renderer's contract: the hook runs only for a
        // frame that presents, and before that present.
        if matches!(outcome, Ok(true))
            && let Some(hook) = self.pre_present_hook.as_mut()
        {
            hook();
        }
        outcome
    }
    fn set_pre_present_hook(&mut self, hook: Option<flui_engine::PrePresentHook>) {
        self.pre_present_hook = hook;
    }
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn is_device_lost(&self) -> bool {
        false
    }
    fn mark_dirty(&mut self, _rect: Rect<Pixels>) {}
    fn mark_full_repaint(&mut self) {}
    fn has_damage(&self) -> bool {
        true
    }
    fn size(&self) -> (u32, u32) {
        self.size
    }
    fn reconfigure_surface(&mut self) -> Result<(), EngineError> {
        Ok(())
    }
}
