//! The closed set of commands the owner feeds the raster executor, and the
//! answers it gets back (ADR-0305 §3, §4).
//!
//! Every enum here is exhaustive (ADR-0068 §2): a new command, change, refusal
//! or verdict breaks every match that has to classify it.

use std::fmt;

use flui_foundation::{
    FrameEpoch, PresentationAddress, SurfaceGeneration, geometry::DeviceSize,
    memory::MemoryPressureLevel,
};
use flui_layer::SceneSnapshot;

use super::SlotId;

/// How many lifecycle commands wait for the executor before a send answers
/// [`RasterSendError::Full`].
pub const LIFECYCLE_CAPACITY: usize = 64;

/// One command for the raster executor.
///
/// `Attach` and `Detach` travel in order on the bounded lifecycle queue;
/// `Frame` and `Configure` coalesce in the slot's inbox; `Trim` coalesces to
/// the strongest level. A turn drains the lifecycle queue before any inbox,
/// so a frame never reaches a slot that is not attached yet.
///
/// The window commands of ADR-0305 §3 (`Release`, `Restore`) arrive with
/// window slots, and `RetryDevice` with the device rebuild (§8).
#[derive(Debug)]
pub enum RasterCommand {
    /// Give `slot`, a freshly minted id, a target and bind it to `address`.
    Attach {
        /// The slot's id, from [`mint_slot`](super::RasterSender::mint_slot).
        slot: SlotId,
        /// The presentation whose frames the slot renders.
        address: PresentationAddress,
        /// What the slot renders into.
        target: SlotTarget,
        /// The slot's starting configuration.
        config: SlotConfig,
    },
    /// Take `slot` down; its pending frame leaves with it, unsettled.
    Detach {
        /// The slot to take down.
        slot: SlotId,
    },
    /// Render `snapshot` into `slot`, superseding a frame still pending there.
    Frame {
        /// The slot to render into.
        slot: SlotId,
        /// The frame, stamped with the slot's address and surface generation.
        snapshot: SceneSnapshot,
    },
    /// Change one thing about `slot`; each kind of change keeps its latest
    /// value until the next turn applies it.
    Configure {
        /// The slot to change.
        slot: SlotId,
        /// What changes.
        change: SlotChange,
    },
    /// Give back GPU memory the next frames can rebuild, before the next frame.
    Trim(MemoryPressureLevel),
}

impl RasterCommand {
    /// The slot the command names; `None` for one addressed to the executor.
    #[must_use]
    pub fn slot(&self) -> Option<SlotId> {
        match self {
            Self::Attach { slot, .. }
            | Self::Detach { slot }
            | Self::Frame { slot, .. }
            | Self::Configure { slot, .. } => Some(*slot),
            Self::Trim(_) => None,
        }
    }
}

/// What a slot renders into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotTarget {
    /// A texture the executor reads back: headless rendering and tests.
    Offscreen {
        /// The target's size in device pixels.
        size: DeviceSize,
        /// An encoded RGBA8 or BGRA8 colour format.
        format: wgpu::TextureFormat,
    },
}

impl SlotTarget {
    /// The size the target is attached at.
    #[must_use]
    pub fn size(&self) -> DeviceSize {
        match *self {
            Self::Offscreen { size, .. } => size,
        }
    }
}

/// A slot's starting configuration, given at [`RasterCommand::Attach`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotConfig {
    /// Whether the slot's presentation reaches the screen.
    pub visibility: SlotVisibility,
}

impl SlotConfig {
    /// A slot that starts shown.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The configuration with `visibility`.
    #[must_use]
    pub fn with_visibility(mut self, visibility: SlotVisibility) -> Self {
        self.visibility = visibility;
        self
    }
}

/// One change to a slot, sent in [`RasterCommand::Configure`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotChange {
    /// A new target size in device pixels. A side of zero or less leaves the
    /// slot's configuration as it is and mints no generation.
    Size(DeviceSize),
    /// Whether the presentation reaches the screen.
    Shown(SlotVisibility),
}

/// Whether a slot's presentation reaches the screen: the window's visible,
/// minimised and occluded state.
///
/// A frame for a slot that is not shown is not rendered and settles
/// [`RasterVerdict::NotShown`]; the slot redraws in full once it is shown
/// again (ADR-0305 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SlotVisibility {
    /// Shown and not covered.
    #[default]
    Shown,
    /// Hidden by its application.
    Hidden,
    /// Minimised.
    Minimized,
    /// Shown and unminimised, and none of it reaches the screen.
    Occluded,
}

impl SlotVisibility {
    /// Whether frames of the slot are rendered.
    #[must_use]
    pub fn is_shown(self) -> bool {
        match self {
            Self::Shown => true,
            Self::Hidden | Self::Minimized | Self::Occluded => false,
        }
    }
}

/// What an accepted [`RasterCommand`] answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendReceipt {
    /// Queued or coalesced; nothing for the owner to stamp.
    Accepted,
    /// The slot's configuration changed: its next frame is stamped with this
    /// generation.
    Configured(SurfaceGeneration),
    /// Accepted with a size of zero or less, which mints no generation; the
    /// slot keeps its configuration (an attach with one is refused by the
    /// executor and reported in the slot's record).
    Unconfigured,
}

/// A command the executor did not accept, handed back.
///
/// No send drops a command: every answer accepts it or returns it, boxed, so
/// a refusal costs one allocation and an accepted send moves no more than it
/// sent.
pub enum RasterSendError {
    /// The lifecycle queue holds [`LIFECYCLE_CAPACITY`] commands; send it
    /// again after the executor's next turn.
    Full(Box<RasterCommand>),
    /// The command names a slot that is not attached under that id, one
    /// detaching, or (a frame) one bound to another presentation.
    StaleSlot(Box<RasterCommand>),
    /// The executor has stopped.
    Gone(Box<RasterCommand>),
}

impl RasterSendError {
    /// The command, back.
    #[must_use]
    pub fn into_command(self) -> RasterCommand {
        match self {
            Self::Full(command) | Self::StaleSlot(command) | Self::Gone(command) => *command,
        }
    }

    pub(crate) fn full(command: RasterCommand) -> Self {
        Self::Full(Box::new(command))
    }

    pub(crate) fn stale_slot(command: RasterCommand) -> Self {
        Self::StaleSlot(Box::new(command))
    }

    pub(crate) fn gone(command: RasterCommand) -> Self {
        Self::Gone(Box::new(command))
    }

    fn reason(&self) -> &'static str {
        match self {
            Self::Full(_) => "the lifecycle queue is full",
            Self::StaleSlot(_) => "the command names a stale slot",
            Self::Gone(_) => "the raster executor has stopped",
        }
    }
}

impl fmt::Debug for RasterSendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let command = match self {
            Self::Full(command) | Self::StaleSlot(command) | Self::Gone(command) => command,
        };
        f.debug_struct("RasterSendError")
            .field("reason", &self.reason())
            .field("slot", &command.slot())
            .finish_non_exhaustive()
    }
}

impl fmt::Display for RasterSendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reason())
    }
}

impl std::error::Error for RasterSendError {}

/// What became of a frame, as the executor settles it.
///
/// The UI runtime's submit verdict without its `Queued` answer, which only a
/// send gives: the host maps one to the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterVerdict {
    /// The frame rendered and reached its target.
    Presented,
    /// The frame rendered and had nothing to present.
    NoPresent,
    /// The frame owed content and was not shown: its slot is hidden, or its
    /// surface was unavailable.
    NotShown,
    /// Rendering failed for now (a transient error, or a contained panic);
    /// a paced retry can succeed.
    Retry,
    /// The frame was stamped against a surface generation the slot no longer
    /// has, or the surface went away while it rendered.
    SurfaceStale,
    /// The device was lost.
    DeviceLost,
    /// The frame failed in a way no retry of it fixes, or the executor
    /// stopped before rendering it.
    Failed,
}

/// The settlement of one frame, written into its slot's record.
///
/// A frame superseded in the inbox gets none of its own: the settlement of
/// the frame that superseded it settles it too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotSettlement {
    /// The presentation the frame was stamped for.
    pub address: PresentationAddress,
    /// The frame's epoch.
    pub epoch: FrameEpoch,
    /// What became of it.
    pub verdict: RasterVerdict,
}

/// Whether the executor still takes commands (ADR-0305 §9).
///
/// An inline executor stops only by shutting down; the state of a raster
/// thread that stops without one arrives with the threaded placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutorHealth {
    /// It takes commands.
    Running,
    /// It was shut down or dropped; every frame still pending settled
    /// [`RasterVerdict::Failed`], and every send answers
    /// [`RasterSendError::Gone`].
    ShutDown,
}
