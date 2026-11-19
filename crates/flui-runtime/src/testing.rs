//! Test doubles for driving a UI runtime with no platform backend and no GPU.
//!
//! Compiled for this crate's own tests and, through the `testing`
//! feature, for `flui-app`'s, which enables it on its dev edge only.
//!
//! - [`ScriptedSink`]: a [`FrameSink`] whose submit verdicts a test scripts.
//! - [`SceneTotals`]: a presentation's retained scene as running totals a test kit
//!   differences across a frame.
//!
//! The sink is not a raster backend: those are minted by a GPU device, and a state-level
//! UI runtime test wants a value with no device ceremony. The window double is
//! `flui_platform::testing::MockWindow`.

use flui_layer::ScenePublication;

use crate::sink::{FrameSink, SubmitVerdict};

/// The scripted submit behavior a [`ScriptedSink`] carries: zero-based call
/// index and submitted scene in, the verdict out.
type SubmitScript = Box<dyn FnMut(u32, &ScenePublication) -> SubmitVerdict + Send>;

/// Closure-configurable [`FrameSink`] double: the headless sink a UI runtime's
/// frame transaction is tested against.
///
/// It scripts [`SubmitVerdict`]s, the UI runtime-side classification every host
/// sink produces, so a test pins the UI runtime's handling of each verdict. How a
/// host maps its own backend outcomes onto verdicts is that host's test.
///
/// Construct through [`always_presents`], [`single_shot`],
/// [`fails_once_then_presents`], or — for a behavior none of those name —
/// the general [`new`] with an explicit script. The surface is `(800, 600)`
/// unless overridden with [`with_size`]. A script answers every frame it is handed: the sink
/// holds any number unsettled unless [`holding`] names its depth, as a test of the in-flight
/// gate does (ADR-0305 §5).
///
/// [`always_presents`]: ScriptedSink::always_presents
/// [`single_shot`]: ScriptedSink::single_shot
/// [`fails_once_then_presents`]: ScriptedSink::fails_once_then_presents
/// [`new`]: ScriptedSink::new
/// [`with_size`]: ScriptedSink::with_size
/// [`holding`]: ScriptedSink::holding
pub struct ScriptedSink {
    /// Invoked with the zero-based index of the current call (the counter's
    /// value before this call), so a script can vary by call without keeping
    /// its own state.
    script: SubmitScript,
    /// How many times `submit` ran — the "did the scene actually leave the
    /// UI runtime" oracle most tests assert on.
    pub submit_calls: u32,
    /// The epoch each submit was handed, in order.
    pub epochs: Vec<flui_foundation::FrameEpoch>,
    /// What `surface_size` reports.
    size: (u32, u32),
    /// What `in_flight_limit` reports.
    limit: std::num::NonZeroUsize,
}

impl std::fmt::Debug for ScriptedSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptedSink")
            .field("submit_calls", &self.submit_calls)
            .field("epochs", &self.epochs)
            .field("size", &self.size)
            .field("limit", &self.limit)
            .finish_non_exhaustive()
    }
}

impl ScriptedSink {
    /// A sink whose `submit` runs the given script, with the call's index and
    /// the publication it was handed.
    pub fn new(
        script: impl FnMut(u32, &ScenePublication) -> SubmitVerdict + Send + 'static,
    ) -> Self {
        Self {
            script: Box::new(script),
            submit_calls: 0,
            epochs: Vec::new(),
            size: (800, 600),
            limit: std::num::NonZeroUsize::MAX,
        }
    }

    /// Every submit presents.
    #[must_use]
    pub fn always_presents() -> Self {
        Self::new(|_, _| SubmitVerdict::Presented)
    }

    /// Exactly one submit is allowed; it returns `verdict`.
    ///
    /// A second submit panics: single-frame tests script one outcome and
    /// rely on the panic to catch a frame that unexpectedly leaves the UI runtime
    /// twice.
    #[must_use]
    pub fn single_shot(verdict: SubmitVerdict) -> Self {
        let mut verdict = Some(verdict);
        Self::new(move |_, _| {
            verdict
                .take()
                .expect("submit called more than once in a single-frame test")
        })
    }

    /// The first submit returns `verdict`; every submit after presents — the
    /// shape a transient submit failure followed by a genuine retry produces.
    #[must_use]
    pub fn fails_once_then_presents(verdict: SubmitVerdict) -> Self {
        let mut verdict = Some(verdict);
        Self::new(move |_, _| verdict.take().unwrap_or(SubmitVerdict::Presented))
    }

    /// Override the reported surface size (default `(800, 600)`).
    #[must_use]
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.size = (width, height);
        self
    }

    /// Hold at most `limit` frames of a presentation unsettled, as a threaded raster side holds
    /// two: the runtime produces no frame for a presentation that has this many pending.
    #[must_use]
    pub fn holding(mut self, limit: std::num::NonZeroUsize) -> Self {
        self.limit = limit;
        self
    }
}

impl FrameSink for ScriptedSink {
    fn surface_size(&mut self) -> (u32, u32) {
        self.size
    }

    fn in_flight_limit(&mut self) -> std::num::NonZeroUsize {
        self.limit
    }

    fn submit(
        &mut self,
        epoch: flui_foundation::FrameEpoch,
        publication: ScenePublication,
    ) -> SubmitVerdict {
        let call_index = self.submit_calls;
        self.submit_calls += 1;
        self.epochs.push(epoch);
        (self.script)(call_index, &publication)
    }
}

/// A presentation's retained scene, as running totals a test kit differences
/// across a frame: what the frame's patch made the owner copy, create and
/// re-depth, and the damage of the version it holds now.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SceneTotals {
    /// The version the scene holds: a frame applied a patch when this moved.
    pub version: flui_layer::SceneVersionId,
    /// Scene nodes the owner's applies have copied or created, in total.
    pub nodes_visited: u64,
    /// Node depths the owner's applies have recomputed, in total.
    pub depth_updates: u64,
    /// The damage of the patch the owner applied last.
    pub patch_damage: flui_layer::DamageRegion,
}
