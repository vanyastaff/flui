//! Native qualification controls around the real GPU renderer. Injected loss
//! is labelled as injection; recovery still rebuilds the actual GPU stack.
use super::device_recovery::DeviceRecovery;
use super::surface_lifecycle::DisplayColorFollow;
use flui_engine::{EngineError, FrameLatency, PresentDisposition, RasterBackend, Renderer};
use flui_foundation::geometry::Rect;
use flui_platform::WindowId;
use parking_lot::Mutex;
use std::sync::{Arc, Weak};
use std::thread::ThreadId;

/// Controls for the actual renderers of one qualification application.
#[derive(Clone, Default)]
pub struct NativeFrames {
    records: Arc<Mutex<Vec<Weak<Record>>>>,
    semantics: Option<bool>,
}

/// One explicitly identified update observed on the process event clock.
/// Transaction timestamps are wall time, not CPU time or physical scanout.
#[derive(Clone, Debug)]
pub struct NativeFrameSample {
    /// Harness request identity, unique within this window's run.
    pub ticket: std::num::NonZeroU64,
    /// External request stamp supplied by the harness.
    pub requested: flui_platform::EventTime,
    /// Native frame callback receipt before application dispatch.
    pub native_received: flui_platform::EventTime,
    /// Entry into the owning runtime's frame transaction.
    pub transaction_started: flui_platform::EventTime,
    /// Root build consuming this exact request.
    pub build_started: flui_platform::EventTime,
    /// Scene entry into the actual GPU renderer.
    pub scene_submitted: Option<flui_platform::EventTime>,
    /// Return from successful swapchain presentation, not physical display feedback.
    pub present_returned: Option<flui_platform::EventTime>,
    /// Return from the complete synchronous frame transaction.
    pub transaction_finished: Option<flui_platform::EventTime>,
    /// Kernel plus user CPU charged to this frame transaction, where available.
    pub transaction_cpu: Option<std::time::Duration>,
}

/// A renderer's observations, with its permanent native owner identity.
#[derive(Clone, Debug)]
pub struct NativeFrameSnapshot {
    /// Native window identity, never a title lookup.
    pub window: WindowId,
    /// Installed presentation identity; absent only during native construction.
    pub presentation: Option<flui_foundation::PresentationAddress>,
    /// Creating and rendering thread.
    pub owner: ThreadId,
    /// Successful actual swapchain submissions, not physical scanout.
    pub presented: u64,
    /// Attempted GPU stack rebuilds, injected failures included.
    pub recovery_attempts: u32,
    /// Successful GPU stack rebuilds.
    pub recovered: u32,
    /// Last renderer extent.
    pub size: (u32, u32),
    /// Renderer destruction ran on its owner.
    pub released: bool,
    /// Current native display mode period, not an assumed 60 Hz default.
    pub refresh_period: Option<std::time::Duration>,
    /// Latest earlier-frame feedback supplied by the native backend, if any.
    pub physical_feedback: Option<flui_platform::FramePresented>,
    /// Adapter selected by this window's real renderer.
    pub adapter_name: String,
    /// API selected by this window's real renderer.
    pub backend: flui_engine::GpuBackend,
}

struct Record {
    observer: flui_engine::testing::FrameObserver,
    state: Mutex<State>,
}
struct State {
    snapshot: NativeFrameSnapshot,
    lose_after_present: bool,
    injected_loss: bool,
    fail_recoveries: u32,
    native_received: Option<flui_platform::EventTime>,
    transaction_started: Option<flui_platform::EventTime>,
    transaction_cpu_started: Option<std::time::Duration>,
    sample: Option<NativeFrameSample>,
}

/// One registered native renderer. Retaining this never retains UI or GPU resources.
#[derive(Clone)]
pub struct NativeFrame(Arc<Record>);
impl NativeFrame {
    /// Mark consumption in the owning root's build. At most one sample may be
    /// pending or unread; refusal cannot silently overwrite a prior request.
    pub fn begin_sample(
        &self,
        ticket: std::num::NonZeroU64,
        requested: flui_platform::EventTime,
    ) -> bool {
        let mut state = self.0.state.lock();
        assert_eq!(state.snapshot.owner, std::thread::current().id());
        if state.sample.is_some() {
            return false;
        }
        let (Some(native_received), Some(transaction_started)) =
            (state.native_received, state.transaction_started)
        else {
            return false;
        };
        state.sample = Some(NativeFrameSample {
            ticket,
            requested,
            native_received,
            transaction_started,
            build_started: flui_platform::EventTime::now(),
            scene_submitted: None,
            present_returned: None,
            transaction_finished: None,
            transaction_cpu: None,
        });
        true
    }

    /// Consume only a completed frame sample; no wait or allocation on the owner.
    pub fn take_sample(&self) -> Option<NativeFrameSample> {
        let mut state = self.0.state.lock();
        state.sample.as_ref()?.transaction_finished?;
        state.sample.take()
    }

    pub(super) fn received(&self, request: flui_platform::FrameRequest) {
        let mut state = self.0.state.lock();
        state.native_received = Some(flui_platform::EventTime::now());
        if request.presented.is_some() {
            state.snapshot.physical_feedback = request.presented;
        }
    }
    pub(super) fn start_transaction(&self, refresh_period: Option<std::time::Duration>) {
        #[cfg(windows)]
        let cpu = Some(
            flui_native::testing::windows::current_thread_cpu_time()
                .expect("qualification thread CPU query"),
        );
        #[cfg(not(windows))]
        let cpu = None;
        let mut state = self.0.state.lock();
        state.snapshot.refresh_period = refresh_period;
        state.transaction_started = Some(flui_platform::EventTime::now());
        state.transaction_cpu_started = cpu;
    }
    pub(super) fn finish_transaction(&self) {
        #[cfg(windows)]
        let cpu = Some(
            flui_native::testing::windows::current_thread_cpu_time()
                .expect("qualification thread CPU query"),
        );
        #[cfg(not(windows))]
        let cpu = None;
        let mut state = self.0.state.lock();
        state.transaction_started = None;
        let consumed = state
            .transaction_cpu_started
            .take()
            .zip(cpu)
            .map(|(start, end)| end.saturating_sub(start));
        if let Some(sample) = &mut state.sample
            && sample.transaction_finished.is_none()
        {
            sample.transaction_finished = Some(flui_platform::EventTime::now());
            sample.transaction_cpu = consumed;
        }
    }
    /// Current observation, copied without running user code under a lock.
    pub fn snapshot(&self) -> NativeFrameSnapshot {
        self.0.state.lock().snapshot.clone()
    }
    /// Capture the next actual surface image; refuses a pending/unread capture.
    pub fn capture_next(&self) -> bool {
        self.0.observer.request_capture()
    }
    /// Consume the requested GPU readback. This is not display feedback.
    pub fn take_capture(
        &self,
    ) -> Option<
        Result<flui_engine::testing::SurfaceReadback, flui_engine::testing::SurfaceReadbackError>,
    > {
        self.0.observer.take_capture()
    }
    /// Inject loss after the next actual present and fail this many recovery
    /// attempts before allowing a real renderer/device/surface rebuild.
    pub fn lose_after_present(&self, failing_recoveries: u32) {
        let mut state = self.0.state.lock();
        state.lose_after_present = true;
        state.fail_recoveries = failing_recoveries;
    }
    /// Permit recovery after an exhausted injected failure sequence.
    pub fn allow_recovery(&self) {
        self.0.state.lock().fail_recoveries = 0;
    }
}

impl NativeFrames {
    /// Retain a real semantics consumer for every qualification runtime.
    /// False explicitly suppresses collection for the CPU baseline, even if a
    /// native accessibility client attaches. The default preserves native demand.
    pub fn with_semantics(mut self, enabled: bool) -> Self {
        self.semantics = Some(enabled);
        self
    }
    pub(super) fn requests_semantics(&self) -> Option<bool> {
        self.semantics
    }
    pub(super) fn for_window(&self, window: WindowId) -> Option<NativeFrame> {
        self.windows()
            .into_iter()
            .find(|frame| frame.snapshot().window == window)
    }
    /// Find a renderer by the typed identity returned by window installation.
    pub fn for_presentation(
        &self,
        presentation: flui_foundation::PresentationAddress,
    ) -> Option<NativeFrame> {
        self.windows()
            .into_iter()
            .find(|frame| frame.snapshot().presentation == Some(presentation))
    }

    pub(super) fn bind(
        &self,
        window: WindowId,
        presentation: flui_foundation::PresentationAddress,
    ) {
        let frame = self
            .windows()
            .into_iter()
            .find(|frame| frame.snapshot().window == window)
            .expect("BUG: qualification renderer is registered before presentation installation");
        let previous = frame
            .0
            .state
            .lock()
            .snapshot
            .presentation
            .replace(presentation);
        assert!(
            previous.is_none(),
            "BUG: renderer has one permanent presentation"
        );
    }

    /// Currently retained renderer observations. Closed observations survive
    /// only while a test holds their NativeFrame handle.
    pub fn windows(&self) -> Vec<NativeFrame> {
        self.records
            .lock()
            .iter()
            .filter_map(Weak::upgrade)
            .map(NativeFrame)
            .collect::<Vec<_>>()
    }

    /// Run the real Win32 application with qualification-only dedicated admission.
    #[cfg(windows)]
    pub fn run<V, F>(&self, application: crate::Application<V, F>) -> Result<(), crate::AppRunError>
    where
        V: flui_view::View + Clone + 'static,
        F: FnMut(&crate::AppHandle) -> V + 'static,
    {
        crate::testing::run_native_with_renderers(
            application,
            super::window_renderer::WindowRenderers::Qualification(self.clone()),
        )
    }

    pub(super) fn open(&self, window: WindowId, mut renderer: Renderer) -> NativeRenderer {
        let observer = flui_engine::testing::observe_surface(&mut renderer);
        assert!(observer.request_capture());
        let record = Arc::new(Record {
            observer,
            state: Mutex::new(State {
                snapshot: NativeFrameSnapshot {
                    window,
                    presentation: None,
                    owner: std::thread::current().id(),
                    presented: 0,
                    recovery_attempts: 0,
                    recovered: 0,
                    size: renderer.size(),
                    released: false,
                    refresh_period: None,
                    physical_feedback: None,
                    adapter_name: renderer.capabilities().adapter_name.clone(),
                    backend: renderer.capabilities().backend,
                },
                lose_after_present: false,
                injected_loss: false,
                fail_recoveries: 0,
                native_received: None,
                transaction_started: None,
                transaction_cpu_started: None,
                sample: None,
            }),
        });
        let mut records = self.records.lock();
        records.retain(|record| record.strong_count() != 0);
        assert!(
            records.len() < 64,
            "qualification retains at most 64 renderer observations"
        );
        records.push(Arc::downgrade(&record));
        drop(records);
        NativeRenderer { renderer, record }
    }
}

pub(super) struct NativeRenderer {
    renderer: Renderer,
    record: Arc<Record>,
}
impl NativeRenderer {
    fn assert_owner(&self) {
        assert_eq!(
            self.record.state.lock().snapshot.owner,
            std::thread::current().id(),
            "native renderer stayed on its creating owner"
        );
    }
}
impl Drop for NativeRenderer {
    fn drop(&mut self) {
        self.assert_owner();
        self.record.state.lock().snapshot.released = true;
    }
}
impl RasterBackend for NativeRenderer {
    fn render_scene(
        &mut self,
        scene: &flui_layer::Scene,
    ) -> Result<PresentDisposition, EngineError> {
        self.assert_owner();
        if self.record.state.lock().injected_loss {
            return Err(EngineError::DeviceLost);
        }
        {
            let mut state = self.record.state.lock();
            if let Some(sample) = &mut state.sample
                && sample.transaction_finished.is_none()
            {
                sample.scene_submitted = Some(flui_platform::EventTime::now());
            }
        }
        let result = self.renderer.render_scene(scene);
        if matches!(result, Ok(PresentDisposition::Presented)) {
            let mut state = self.record.state.lock();
            if let Some(sample) = &mut state.sample
                && sample.transaction_finished.is_none()
            {
                sample.present_returned = Some(flui_platform::EventTime::now());
            }
            state.snapshot.presented = state
                .snapshot
                .presented
                .checked_add(1)
                .expect("BUG: native frame count exhausted");
            if std::mem::take(&mut state.lose_after_present) {
                state.injected_loss = true;
            }
        }
        result
    }
    fn resize(&mut self, width: u32, height: u32) {
        self.assert_owner();
        self.renderer.resize(width, height);
        self.record.state.lock().snapshot.size = (width, height);
    }
    fn is_device_lost(&self) -> bool {
        self.record.state.lock().injected_loss || self.renderer.is_device_lost()
    }
    fn mark_dirty(&mut self, rect: Rect<f64>) {
        self.renderer.mark_dirty(rect);
    }
    fn mark_full_repaint(&mut self) {
        self.renderer.mark_full_repaint();
    }
    fn has_damage(&self) -> bool {
        self.renderer.has_damage()
    }
    fn size(&self) -> (u32, u32) {
        self.renderer.size()
    }
    fn apply_texture_updates(
        &mut self,
        updates: flui_layer::TextureUpdates,
    ) -> Result<(), EngineError> {
        self.renderer.apply_texture_updates(updates)
    }
    fn reconfigure_surface(&mut self) -> Result<(), EngineError> {
        self.assert_owner();
        self.renderer.reconfigure_surface();
        Ok(())
    }
    fn set_pre_present_hook(&mut self, hook: Option<flui_engine::PrePresentHook>) {
        self.renderer.set_pre_present_hook(hook);
    }
    fn reclaim(&mut self, level: flui_foundation::memory::MemoryPressureLevel) {
        self.renderer.reclaim(level);
    }
}
impl DeviceRecovery for NativeRenderer {
    fn try_recover_device(&mut self) -> Result<(), EngineError> {
        self.assert_owner();
        {
            let mut state = self.record.state.lock();
            state.snapshot.recovery_attempts = state
                .snapshot
                .recovery_attempts
                .checked_add(1)
                .expect("BUG: native recovery count exhausted");
            if state.fail_recoveries != 0 {
                state.fail_recoveries -= 1;
                return Err(EngineError::DeviceLost);
            }
        }
        pollster::block_on(self.renderer.recover())?;
        // A capture requested here can only observe the rebuilt surface. Tests
        // drain older captures before injecting a loss.
        let _ = self.record.observer.request_capture();
        let mut state = self.record.state.lock();
        state.snapshot.recovered = state
            .snapshot
            .recovered
            .checked_add(1)
            .expect("BUG: native recovery count exhausted");
        state.injected_loss = false;
        Ok(())
    }
}

impl std::fmt::Debug for NativeFrames {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeFrames")
            .field("windows", &self.windows().len())
            .finish()
    }
}
impl std::fmt::Debug for NativeFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.snapshot().fmt(f)
    }
}
impl DisplayColorFollow for NativeRenderer {
    fn follow_display_color(
        &mut self,
        display: Option<flui_platform::DisplayColor>,
    ) -> Result<bool, EngineError> {
        self.renderer.follow_display_color(display)
    }
}
impl super::window_renderer::WindowRenderer for NativeRenderer {
    fn set_frame_latency(&mut self, latency: FrameLatency) {
        self.renderer.set_frame_latency(latency);
    }
}
