//! The frame sink of a [`HeadlessHost`](super::HeadlessHost) and the reasons it leaves a frame
//! unshown.

use std::cell::OnceCell;
use std::num::NonZeroU32;

use flui_rendering::layer::{
    LayerTree, ScenePublication, SceneVersion, TextureRegistry, TextureUpdates,
};
use flui_runtime::sink::{FrameSink, SubmitVerdict};

/// The frame sink of a [`HeadlessHost`](super::HeadlessHost): a surface of fixed size that
/// presents every scene and keeps the last version, unless told to leave
/// some unshown ([`HeadlessHost::leave_unshown`](super::HeadlessHost::leave_unshown)).
/// Like a runner's raster host, a frame it presents takes the primary
/// presentation's texture frames and retirements with it
/// ([`Self::textures`]); one it leaves unshown leaves them in the registry.
///
/// The version is flattened into a layer tree only when a test reads it, so a
/// frame costs the sink nothing that grows with the tree.
#[derive(Debug)]
pub struct HeadlessSink {
    pub(super) size: (u32, u32),
    last: Option<(SceneVersion, OnceCell<Option<LayerTree>>)>,
    submits: u64,
    /// The next frames left unshown, and how.
    pub(super) unshown: Option<(NonZeroU32, UnshownFrame)>,
    /// The epochs of the frames presented most recently, oldest first, at
    /// most [`PRESENTED_KEPT`].
    presented: std::collections::VecDeque<flui_foundation::FrameEpoch>,
    /// The primary presentation's external textures.
    registry: Option<TextureRegistry>,
    /// What the frame presented last took from the registry.
    textures: TextureUpdates,
}

/// Why a [`HeadlessSink`] leaves a frame unshown, as a real surface does:
/// each is the verdict a runner's sink answers then, and the UI runtime
/// recovers from it as it does on screen (ADR-0334, ADR-0335).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnshownFrame {
    /// The surface is not ready to show the window yet
    /// ([`SubmitVerdict::Retry`]): the frame is retried at the next paced
    /// wake, and pointer input waits for the first frame shown (ADR-0048).
    NotReady,
    /// The surface is covered or released and shows nothing
    /// ([`SubmitVerdict::NotShown`]): the frame is retried.
    Withdrawn,
    /// The surface was lost or went stale ([`SubmitVerdict::SurfaceStale`]):
    /// the frame is retried against the surface configured again.
    SurfaceLost,
    /// The GPU device was lost ([`SubmitVerdict::DeviceLost`]): the frame is
    /// retried after the dropped-frame backoff. The UI runtime reports it as
    /// a dropped frame, which [`HeadlessHost::pump`](super::HeadlessHost::pump) raises (see its
    /// [module docs](super#failures)).
    DeviceLost,
    /// The frame failed in a way its retry cannot fix
    /// ([`SubmitVerdict::Failed`]): it is painted again after the
    /// dropped-frame backoff.
    Failed,
    /// The window cannot show a frame until it changes
    /// ([`SubmitVerdict::Refused`]): nothing is retried until its next
    /// change asks for a frame.
    Refused,
}

impl UnshownFrame {
    const fn verdict(self) -> SubmitVerdict {
        match self {
            Self::NotReady => SubmitVerdict::Retry,
            Self::Withdrawn => SubmitVerdict::NotShown,
            Self::SurfaceLost => SubmitVerdict::SurfaceStale,
            Self::DeviceLost => SubmitVerdict::DeviceLost,
            Self::Failed => SubmitVerdict::Failed,
            Self::Refused => SubmitVerdict::Refused,
        }
    }
}

/// How many presented frames a [`HeadlessSink`] remembers for a presentation
/// report to name.
const PRESENTED_KEPT: usize = 16;

impl HeadlessSink {
    /// A `width` × `height` surface that has presented nothing yet.
    #[must_use]
    pub(crate) fn new(width: u32, height: u32, registry: Option<TextureRegistry>) -> Self {
        Self {
            size: (width, height),
            last: None,
            submits: 0,
            unshown: None,
            // Its room is taken here, so a frame that presents allocates nothing.
            presented: std::collections::VecDeque::with_capacity(PRESENTED_KEPT),
            registry,
            textures: TextureUpdates::default(),
        }
    }

    /// The epoch of the frame presented last, which a presentation report
    /// may name ([`HeadlessHost::report_presented`](super::HeadlessHost::report_presented)).
    #[must_use]
    pub fn last_presented(&self) -> Option<flui_foundation::FrameEpoch> {
        self.presented.back().copied()
    }

    /// Whether the frame of `epoch` is one of the frames presented recently.
    pub(super) fn presented(&self, epoch: flui_foundation::FrameEpoch) -> bool {
        self.presented.contains(&epoch)
    }

    /// The layer tree the pipeline's frame node paints in the last version
    /// submitted, rooted at its device-pixel-ratio layer and retained across
    /// frames that painted nothing.
    #[must_use]
    pub fn layer_tree(&self) -> Option<&LayerTree> {
        let (version, tree) = self.last.as_ref()?;
        tree.get_or_init(|| {
            let root = version.node(version.root())?;
            let frame = root.content().slots().next()?;
            version.flatten_node(frame)
        })
        .as_ref()
    }

    /// The last version submitted.
    #[must_use]
    pub fn version(&self) -> Option<&SceneVersion> {
        self.last.as_ref().map(|(version, _)| version)
    }

    /// How many scenes have been presented: a frame left unshown is not
    /// counted.
    #[must_use]
    pub fn submits(&self) -> u64 {
        self.submits
    }

    /// The texture frames and retirements the frame presented last took
    /// from the registry, as a raster host applies them before it draws:
    /// the newest frame of each texture fed since the frame before.
    #[must_use]
    pub fn textures(&self) -> &TextureUpdates {
        &self.textures
    }
}

impl FrameSink for HeadlessSink {
    fn surface_size(&mut self) -> (u32, u32) {
        self.size
    }

    fn submit(
        &mut self,
        epoch: flui_foundation::FrameEpoch,
        publication: ScenePublication,
    ) -> SubmitVerdict {
        if let Some((frames, how)) = self.unshown {
            self.unshown = NonZeroU32::new(frames.get() - 1).map(|left| (left, how));
            return how.verdict();
        }
        // Taken with the frame it rides, as the raster host takes them.
        self.textures = self
            .registry
            .as_ref()
            .map_or_else(TextureUpdates::default, TextureRegistry::take_updates);
        self.last = Some((publication.scene, OnceCell::new()));
        self.submits = self.submits.saturating_add(1);
        if self.presented.len() == PRESENTED_KEPT {
            self.presented.pop_front();
        }
        self.presented.push_back(epoch);
        SubmitVerdict::Presented
    }
}
