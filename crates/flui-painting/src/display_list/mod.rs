//! `DisplayList` — the recorded sequence of drawing commands a
//! [`Canvas`](crate::Canvas) produces and the engine replays.
//!
//! - [`command`] — the `DrawCommand` enum, the wire vocabulary shared with
//!   `flui-engine`.
//! - [`command_ops`] — `DrawCommand::bounds` and its per-variant geometry.
//! - [`paragraph`] — [`ShapedParagraph`], the shaped text a
//!   `DrawOp::Paragraph` carries.

use std::num::NonZeroUsize;
use std::sync::Arc;

use smallvec::SmallVec;

use flui_foundation::geometry::{Matrix4, Rect};
use flui_foundation::inspect::{Fields, Inspect, Unit};

mod blur;
pub mod command;
pub mod command_ops;
mod finite;
pub mod paragraph;

pub use blur::BlurSigma;
pub use command::{DrawCommand, DrawOp};
pub use command_ops::DamageExtent;
pub use finite::NonFiniteOps;
pub(crate) use finite::paint_finite;
use finite::{NonFinite, Refused, matrix_finite};
pub use paragraph::{FontBlob, FontFace, ShapedGlyph, ShapedParagraph, ShapedRun};
// The paint vocabulary the commands carry; defined in `crate::paint`.
pub(crate) use crate::paint::{
    BlendMode, Clip, ClipOp, FilterQuality, Paint, PointMode, TextureId,
    image::{ColorFilter, ImageRepeat},
};

/// Which entry of its list's transform table an op was recorded under:
/// `None` is the identity, which takes no entry, and `Some(n)` is
/// `transforms[n - 1]`. Private to this module, so the ids of two lists can
/// be neither compared nor mixed (ADR-0306 §1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TransformId(Option<NonZeroUsize>);

impl TransformId {
    const IDENTITY: Self = Self(None);

    /// This id moved past `offset` entries of a receiving table.
    #[expect(
        clippy::expect_used,
        reason = "each table holds at most isize::MAX / 128 entries, so two never overflow a usize"
    )]
    fn rebased(self, offset: usize) -> Self {
        Self(self.0.map(|entry| {
            entry
                .checked_add(offset)
                .expect("BUG: two transform tables fit in the address space")
        }))
    }
}

/// One stored command: its transform's id and the op.
#[derive(Clone, Debug)]
struct RecordedOp {
    transform: TransformId,
    op: DrawOp,
}

/// The identity a command recorded without a table entry yields: a promoted
/// constant, so every such command yields the same reference.
const IDENTITY: &Matrix4 = &Matrix4::IDENTITY;

// A stored command is its op and one word for its transform. The sizes are
// the measured ones, and they only go down: ADR-0306 §3 sets the op at 112
// bytes and a stored command at 120 once its fat payloads are boxed.
const _: () = assert!(size_of::<TransformId>() == size_of::<usize>());
const _: () = assert!(size_of::<DrawOp>() <= 216);
const _: () = assert!(size_of::<RecordedOp>() <= 224);

/// A recorded sequence of drawing commands.
///
/// The output of [`Canvas::finish`](crate::Canvas::finish) and the input to
/// `PictureLayer`. Commands are read-only; recording and concatenation
/// compute the cached [`bounds`](Self::bounds) from command geometry.
///
/// Transforms are interned: an op holds a list-private id of its entry,
/// consecutive ops under one transform share it, the identity takes none,
/// and [`Self::iter`] yields the transform and the op of each command
/// (ADR-0306 §1, §5).
#[derive(Debug, Clone)]
pub struct DisplayList {
    /// The transforms the commands were recorded under, each finite and none
    /// of them the identity. The first is held inline, so a run under one
    /// transform, the common case of a recorder that translates its canvas,
    /// allocates no table.
    transforms: SmallVec<[Matrix4; 1]>,

    /// Drawing commands in order.
    commands: Vec<RecordedOp>,

    /// Cached union of every command that contributes bounds, or
    /// `None` while no command has contributed one yet.
    ///
    /// `None` and "an empty rect" are different states, and collapsing
    /// them is a bug: a list whose first bounds-carrying command sits at
    /// (100, 100) must not report a rect reaching back to the origin just
    /// because a clip or a `Save` was recorded ahead of it. Seeding is
    /// therefore keyed on this being `None` — never on `commands` being
    /// empty, which asks a different question, and never on the rect
    /// comparing equal to `Rect::ZERO`, which a degenerate command can
    /// legitimately produce.
    pub(crate) bounds: Option<Rect<f64>>,

    /// Cached conservative extent of every pixel the list can change, or
    /// `None` while no command has drawn anything — see
    /// [`Self::damage_extent`].
    ///
    /// Kept apart from `bounds` because the two answer different questions:
    /// `bounds` is the layout-facing box (a paragraph's laid-out size, no
    /// entry for a full-canvas fill), while this one must never fall short of
    /// the ink, since a damage region computed from it decides which pixels a
    /// partial repaint may leave untouched.
    pub(crate) damage: Option<DamageExtent>,

    /// Cached extent of every command whose pixels come from outside the
    /// list, or `None` while no such command was recorded — see
    /// [`Self::volatile_extent`].
    pub(crate) volatile: Option<DamageExtent>,

    /// The ops refused at record because they were not finite — see
    /// [`Self::non_finite`].
    pub(crate) non_finite: NonFiniteOps,
}

/// Folds one command's bounds into an accumulating union: the one place that
/// decides whether a rect *seeds* the union or *joins* it, shared by
/// [`DisplayList::push`] and [`DisplayList::append`].
fn accumulate_bounds(acc: &mut Option<Rect<f64>>, cmd_bounds: Rect<f64>) {
    *acc = Some(match *acc {
        Some(current) => current.union(&cmd_bounds),
        None => cmd_bounds,
    });
}

/// Folds one command's damage extent into an accumulating one; the
/// counterpart of [`accumulate_bounds`] for [`DisplayList::damage_extent`].
fn accumulate_damage(acc: &mut Option<DamageExtent>, extent: DamageExtent) {
    *acc = Some(match *acc {
        Some(current) => current.union(extent),
        None => extent,
    });
}

impl Inspect for DisplayList {
    fn kind(&self) -> &'static str {
        "DisplayList"
    }

    fn inspect(&self, out: &mut Fields<'_>) {
        out.field("commands", &self.commands.len());
        out.field("bounds", &self.bounds).unit(Unit::LogicalPx);
        // Printed only when something was refused, so a clean list reads as before.
        if !self.non_finite.is_empty() {
            out.field("non_finite_transform", &self.non_finite.transform);
            out.field("non_finite_geometry", &self.non_finite.geometry);
        }
    }
}

impl DisplayList {
    /// An empty list.
    pub fn new() -> Self {
        Self {
            transforms: SmallVec::new(),
            commands: Vec::new(),
            bounds: None,
            damage: None,
            volatile: None,
            non_finite: NonFiniteOps::default(),
        }
    }

    /// Empties the list and keeps its room, so a buffer a scene let go of is
    /// recorded into again without allocating. The commands drop last, once
    /// the empty state is committed, so a destructor that panics leaves an
    /// empty list (ADR-0306 §7).
    pub fn clear(&mut self) {
        self.bounds = None;
        self.damage = None;
        self.volatile = None;
        self.non_finite = NonFiniteOps::default();
        self.transforms.clear();
        self.commands.clear();
    }

    /// The commands the list holds room for without allocating.
    #[must_use]
    pub fn room(&self) -> usize {
        self.commands.capacity()
    }

    /// The ops refused at record because they were not finite, by reason
    /// (ADR-0306 §4).
    ///
    /// A refused draw is absent from the list, a refused clip clips
    /// everything, a refused save-layer opens unbounded, and the list's
    /// [`bounds`](Self::bounds) and [`damage_extent`](Self::damage_extent)
    /// hold only finite values. [`Self::append`] sums the counts.
    #[must_use]
    pub fn non_finite(&self) -> NonFiniteOps {
        self.non_finite
    }

    /// The commands, in recording order.
    pub fn iter(&self) -> DrawCommands<'_> {
        DrawCommands {
            transforms: &self.transforms,
            commands: self.commands.iter(),
        }
    }

    /// The command at `index`, or `None` past the end.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<DrawCommand<'_>> {
        self.commands
            .get(index)
            .map(|recorded| view(&self.transforms, recorded))
    }

    /// The number of commands.
    #[must_use]
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether no command was recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// The union of every command that contributes bounds, or `None` when
    /// none has — a list of only clips, saves, or `DrawPaint`s has no extent,
    /// which is a different answer from an empty rect at the origin.
    #[must_use]
    pub fn bounds(&self) -> Option<Rect<f64>> {
        self.bounds
    }

    /// A conservative extent of every pixel replaying this list can change,
    /// in the list's own coordinate space; `None` when no command draws.
    ///
    /// Unlike [`Self::bounds`], this never falls short of the ink: a paragraph
    /// contributes its glyph overflow as well as its laid-out box, a shadow
    /// its blur, a stroke its joins, and a full-canvas fill (`drawColor`,
    /// `drawPaint`, an unbounded save-layer) makes the extent
    /// [`DamageExtent::Unbounded`]. Clips are ignored, which only ever makes
    /// the answer larger. This is what a damage region is built from: a rect
    /// that misses a pixel the list changed leaves that pixel stale.
    #[must_use]
    pub fn damage_extent(&self) -> Option<DamageExtent> {
        self.damage
    }

    /// The extent of the commands whose pixels this list does not
    /// determine, in the list's own coordinate space; `None` when there is
    /// none.
    ///
    /// A texture draw (`DrawOp::Texture`) names an external texture whose
    /// content its producer (a video decoder, a camera, a platform surface)
    /// replaces behind the same id, so replaying an unchanged list can paint
    /// different pixels there. A damage producer that vouches for unchanged
    /// content by the list's identity treats this extent as changed on
    /// every frame.
    #[must_use]
    pub fn volatile_extent(&self) -> Option<DamageExtent> {
        self.volatile
    }

    /// Records `op` under `ctm`: the canvas's one way in.
    ///
    /// An op under a non-finite `ctm`, or holding a non-finite value, is
    /// refused by its kind and counted in [`Self::non_finite`] (ADR-0306 §4);
    /// `paint_finite` answers for the op's paint. Nothing here runs user code.
    pub(crate) fn record(
        &mut self,
        ctm: &Matrix4,
        op: DrawOp,
        paint_finite: impl Fn(&Arc<Paint>) -> bool,
    ) {
        let reason = if !matrix_finite(ctm) {
            NonFinite::Transform
        } else if !op.is_finite(paint_finite) {
            NonFinite::Geometry
        } else {
            self.push(ctm, op);
            return;
        };
        // A replacement keeps a finite CTM; under a non-finite one it is
        // recorded under the identity, which only the replaced op's own
        // geometry would have needed.
        let transform = match reason {
            NonFinite::Transform => IDENTITY,
            NonFinite::Geometry => ctm,
        };
        match finite::refusal(op) {
            Refused::Omitted => self.non_finite.count(reason),
            Refused::Replaced(op) => {
                self.non_finite.count(reason);
                self.push(transform, op);
            }
            Refused::Marker(op) => self.push(transform, op),
        }
    }

    /// Stores `op` under `ctm`, which is finite: the identity takes no
    /// entry, a CTM equal to the last entry reuses it, and any other pushes
    /// one. `Matrix4`'s `==` is exact and reflexive on finite values.
    fn push(&mut self, ctm: &Matrix4, op: DrawOp) {
        let transform = if *ctm == Matrix4::IDENTITY {
            TransformId::IDENTITY
        } else {
            if self.transforms.last() != Some(ctm) {
                self.transforms.push(*ctm);
            }
            TransformId(NonZeroUsize::new(self.transforms.len()))
        };
        account(
            &mut self.bounds,
            &mut self.damage,
            &mut self.volatile,
            DrawCommand {
                transform: ctm,
                op: &op,
            },
        );
        self.commands.push(RecordedOp { transform, op });
    }

    /// Moves every command of `other` onto the end of this list, unioning
    /// the cached bounds and summing the [`non_finite`](Self::non_finite)
    /// counts.
    ///
    /// Concatenates replay state too: a root clip in this list affects `other`,
    /// even when both lists have balanced save/restore pairs. Use
    /// [`Self::append_isolated`] for each independently recorded paint run
    /// when building an accumulator.
    pub fn append(&mut self, mut other: DisplayList) {
        self.non_finite = self.non_finite.saturating_add(other.non_finite);
        if self.commands.is_empty() {
            std::mem::swap(&mut self.transforms, &mut other.transforms);
            std::mem::swap(&mut self.commands, &mut other.commands);
            self.bounds = other.bounds;
            self.damage = other.damage;
            self.volatile = other.volatile;
        } else if !other.commands.is_empty() {
            let offset = self.transforms.len();
            self.transforms.append(&mut other.transforms);
            if offset == 0 {
                self.commands.append(&mut other.commands);
            } else {
                self.commands
                    .extend(other.commands.drain(..).map(|recorded| RecordedOp {
                        transform: recorded.transform.rebased(offset),
                        op: recorded.op,
                    }));
            }
            if let Some(other_bounds) = other.bounds {
                accumulate_bounds(&mut self.bounds, other_bounds);
            }
            if let Some(other_damage) = other.damage {
                accumulate_damage(&mut self.damage, other_damage);
            }
            if let Some(other_volatile) = other.volatile {
                accumulate_damage(&mut self.volatile, other_volatile);
            }
        }
    }

    /// Appends a balanced paint run inside its own save/restore scope.
    ///
    /// The run inherits the caller's clip state, but its clips do not affect
    /// subsequent commands. A run that changes no replay state (no clip, save,
    /// restore or layer scope) is appended as it stands, since a scope around
    /// it would change nothing. An empty run records no command but still adds
    /// its [`non_finite`](Self::non_finite) counts: a run whose every op was
    /// refused is empty. The caller must provide balanced save/restore
    /// commands within the run.
    pub fn append_isolated(&mut self, mut other: DisplayList) {
        if other.is_empty() {
            self.non_finite = self.non_finite.saturating_add(other.non_finite);
            return;
        }
        if !other
            .commands
            .iter()
            .any(|recorded| recorded.op.scopes_replay())
        {
            self.append(other);
            return;
        }
        if self.commands.is_empty() {
            // The first run is wrapped in its own buffer, which becomes this
            // list's: splicing it allocates nothing while the run has room
            // for the two scope commands. The markers hold no geometry, so
            // they move no cached extent.
            other.commands.insert(
                0,
                RecordedOp {
                    transform: TransformId::IDENTITY,
                    op: DrawOp::Save,
                },
            );
            other.push(IDENTITY, DrawOp::Restore);
            other.non_finite = other.non_finite.saturating_add(self.non_finite);
            *self = other;
            return;
        }
        self.push(IDENTITY, DrawOp::Save);
        self.append(other);
        self.push(IDENTITY, DrawOp::Restore);
    }
}

/// Folds `command` into a list's cached bounds and extents, as recording it
/// does.
fn account(
    bounds: &mut Option<Rect<f64>>,
    damage: &mut Option<DamageExtent>,
    volatile: &mut Option<DamageExtent>,
    command: DrawCommand<'_>,
) {
    if let Some(cmd_bounds) = command.bounds() {
        accumulate_bounds(bounds, cmd_bounds);
    }
    if let Some(extent) = command.damage_extent() {
        accumulate_damage(damage, extent);
        if matches!(command.op, DrawOp::Texture { .. }) {
            accumulate_damage(volatile, extent);
        }
    }
}

/// The command `recorded` is, with its transform resolved in `transforms`.
fn view<'a>(transforms: &'a [Matrix4], recorded: &'a RecordedOp) -> DrawCommand<'a> {
    let transform = match recorded.transform.0 {
        None => IDENTITY,
        Some(entry) => &transforms[entry.get() - 1],
    };
    DrawCommand {
        transform,
        op: &recorded.op,
    }
}

impl Default for DisplayList {
    fn default() -> Self {
        Self::new()
    }
}

/// The commands of a [`DisplayList`] in recording order: what
/// [`DisplayList::iter`] returns.
#[derive(Clone, Debug)]
pub struct DrawCommands<'a> {
    transforms: &'a [Matrix4],
    commands: std::slice::Iter<'a, RecordedOp>,
}

impl<'a> Iterator for DrawCommands<'a> {
    type Item = DrawCommand<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let transforms = self.transforms;
        self.commands
            .next()
            .map(|recorded| view(transforms, recorded))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.commands.size_hint()
    }

    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        let transforms = self.transforms;
        self.commands
            .nth(n)
            .map(|recorded| view(transforms, recorded))
    }
}

impl DoubleEndedIterator for DrawCommands<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let transforms = self.transforms;
        self.commands
            .next_back()
            .map(|recorded| view(transforms, recorded))
    }
}

impl ExactSizeIterator for DrawCommands<'_> {}

impl std::iter::FusedIterator for DrawCommands<'_> {}

impl<'a> IntoIterator for &'a DisplayList {
    type Item = DrawCommand<'a>;
    type IntoIter = DrawCommands<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::DisplayList;
    use flui_foundation::geometry::Matrix4;

    use crate::DrawOp;

    #[test]
    fn a_cleared_list_keeps_none_of_its_transforms() {
        let mut list = DisplayList::new();
        for frame in 0..4 {
            list.clear();
            for step in 0..8 {
                list.push(
                    &Matrix4::translation(f64::from(frame * 8 + step + 1), 0.0, 0.0),
                    DrawOp::Save,
                );
            }
            assert_eq!(
                list.transforms.len(),
                8,
                "frame {frame} keeps an earlier frame's transforms"
            );
        }
    }
}
