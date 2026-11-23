//! A presentation's retained scene: the [`SceneOwner`] its pipeline's paint
//! patches and its sink draws from (ADR-0304 §7, §8).
//!
//! Each frame that paints buffers the pipeline's patch, composes the scene's
//! root around the pipeline's frame node and the debug overlays' nodes (the
//! performance overlay's readout, then the build banner), and applies it. A
//! frame whose pipeline paints nothing applies a patch only when the build
//! banner changed: its node and the root. The version is reported back to the pipeline, whose probes,
//! input regions, leader watches and follower hit-testing read it, and is
//! published to the sink with the damage since the version the raster side
//! last acknowledged.
//!
//! A patch the owner refuses is a producer defect or an exhausted sequence:
//! the lineage is renewed, and the pipeline, finding its staged patch never
//! applied, repaints from the root into the new lineage on the next frame
//! (ADR-0304 §10).
//!
//! A version the surface withheld (`SubmitVerdict::NotShown`) is owed to the
//! screen as it is: the retry publishes it again, with the damage still
//! counted from the acknowledged base, and paints nothing (ADR-0304 §7). The
//! scene holds that debt beside the target the version was applied for, so a
//! retry that finds another target or a newer version repaints instead. Only a
//! verdict that shows the version, or a new version applied, pays the debt.
//!
//! A frame that paints nothing while a texture frame waits in the
//! presentation's registry publishes the current version again too, when it
//! was applied for the frame's target: the raster host takes the texture
//! frame with it and damages only the texture's users (ADR-0304 §8, §9).

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use crate::build_overlay::Banner;
use crate::performance_overlay::Readout;
use flui_foundation::RenderId;
use flui_foundation::SurfaceGeneration;
use flui_foundation::geometry::{DevicePixelRatio, Rect};
use flui_layer::{
    BoundaryContent, ClipRectLayer, ReleasedContents, ResyncReason, SceneNodeId, SceneOp,
    SceneOwner, ScenePublication, SceneVersion, SceneVersionId, SceneWriter, SurfaceKey,
    TextureRegistry, TransformLayer,
};
use flui_painting::{DisplayList, TextContext, TextLayoutError};

/// The retained scene of one presentation, and the nodes its debug overlays
/// are published at.
#[derive(Debug, Default)]
pub(crate) struct PresentationScene {
    owner: SceneOwner,
    /// The performance overlay the current version shows.
    overlay: Option<ShownOverlay<Readout>>,
    /// The build banner the current version shows.
    banner: Option<ShownOverlay<Banner>>,
    /// Patches refused in a row since the last one applied: a second
    /// refusal is a producer defect a rebuild did not cure, so it asks for
    /// no retry.
    refusals: u8,
    /// The target the current version was applied for; `None` before the
    /// first version and after a renewal.
    target: Option<SceneTarget>,
    /// The version a withheld frame left owed to the screen, until a frame
    /// applies a version or the raster side shows it or a later version of
    /// its lineage ([`Self::held_retry`]). A republish leaves it: the
    /// republished frame may be discarded before any verdict.
    owed: Option<SceneVersionId>,
    /// The version the window refused: a texture frame does not publish it
    /// again, since a refused window costs none (ADR-0334 §4); the frame
    /// waits in the registry. It holds only while it is the current version,
    /// so a frame that applies a version or renews the lineage ends it by
    /// making another current, and the raster side showing it, or a later
    /// version of its lineage, clears it ([`Self::completed`]).
    refused: Option<SceneVersionId>,
}

/// What a version is drawn for: the surface's size in device pixels and the
/// device pixel ratio the pipeline painted it under.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SceneTarget {
    pub(crate) size: (u32, u32),
    pub(crate) scale: DevicePixelRatio,
}

impl SceneTarget {
    /// The target a frame laid out under `constraints` at `scale` draws for:
    /// each logical extent in whole device pixels, snapped by the conversion
    /// the hosts share (`flui_foundation::geometry::device_extent`), so the
    /// patch's surface rounds as the window's does; an unbounded or
    /// non-finite extent is no surface. The largest surface is a host's
    /// policy, applied where it sizes the window, and the renderer refuses a
    /// surface past its device's limit; the runtime holds neither, so it does
    /// not clamp.
    pub(crate) fn of(
        constraints: flui_rendering::constraints::BoxConstraints,
        scale: DevicePixelRatio,
    ) -> Self {
        let extent =
            |logical: f64| flui_foundation::geometry::device_extent(logical, scale).unwrap_or(0);
        Self {
            size: (
                extent(constraints.max_width),
                extent(constraints.max_height),
            ),
            scale,
        }
    }
}

/// What a frame owes a version its surface withheld (ADR-0304 §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeldRetry {
    /// No withheld version is owed.
    None,
    /// The current version is owed as it is: a frame whose paint changes
    /// nothing publishes it again.
    Republish,
    /// The owed version is no longer what the screen should show (a newer
    /// version was applied since, or the target changed): the frame repaints
    /// the tree in full.
    Repaint,
}

/// What [`PresentationScene::apply`] did with a frame's patch.
pub(crate) enum Produced {
    /// The patch is the new version: report it to the pipeline and publish.
    Applied {
        version: SceneVersion,
        released: ReleasedContents,
    },
    /// The owner refused the patch and renewed its lineage; the pipeline
    /// repaints into it on the next frame, which `retry` asks for unless
    /// the patch before was refused too.
    Refused {
        released: ReleasedContents,
        retry: bool,
    },
}

/// What a debug overlay draws over the presentation's content: one recording
/// inside its bounds, its labels shaped through the UI runtime's text context
/// (the performance overlay's [`Readout`], the build overlay's [`Banner`]).
///
/// Two values that compare equal, shaped over the same fonts, record the
/// same commands: the scene keeps the chunk it published for the first.
pub(crate) trait OverlayContent: PartialEq + std::fmt::Debug {
    /// What a trace calls it.
    const NAME: &'static str;

    /// Where it draws, in logical pixels from the top-left corner of the
    /// surface: the clip over its recording.
    fn bounds(&self) -> Rect<f64>;

    /// Its recording, its labels shaped through `text`.
    ///
    /// # Errors
    ///
    /// The [`TextLayoutError`] of the first label that does not shape.
    fn record(&self, text: &mut TextContext) -> Result<DisplayList, TextLayoutError>;
}

impl OverlayContent for Readout {
    const NAME: &'static str = "performance overlay";

    fn bounds(&self) -> Rect<f64> {
        crate::performance_overlay::BOUNDS
    }

    fn record(&self, text: &mut TextContext) -> Result<DisplayList, TextLayoutError> {
        Readout::record(self, text)
    }
}

impl OverlayContent for Banner {
    const NAME: &'static str = "build overlay";

    fn bounds(&self) -> Rect<f64> {
        Banner::bounds(self)
    }

    fn record(&self, text: &mut TextContext) -> Result<DisplayList, TextLayoutError> {
        Banner::record(self, text)
    }
}

/// A debug overlay a frame composes beside the pipeline's frame node: what it
/// draws, the context its labels are shaped through and the device pixel
/// ratio it is drawn under.
pub(crate) struct OverlayFrame<'t, C> {
    pub(crate) content: C,
    pub(crate) text: &'t flui_rendering::TextContextHandle,
    pub(crate) device_pixel_ratio: f64,
    /// The render object the overlay's effects and chunk are filed under.
    pub(crate) owner: RenderId,
}

/// An overlay the current version shows: its node and what its chunk was
/// recorded from.
#[derive(Debug)]
struct ShownOverlay<C> {
    node: SceneNodeId,
    content: C,
    /// The fonts generation the labels were shaped at: a registration may
    /// shape the same text differently.
    fonts: u64,
    device_pixel_ratio: f64,
}

impl<C: OverlayContent> ShownOverlay<C> {
    /// Whether this shows `wanted` as it is, fonts aside: the same content at
    /// the same scale.
    fn shows(&self, wanted: &OverlayFrame<'_, C>) -> bool {
        self.content == wanted.content
            && self.device_pixel_ratio.to_bits() == wanted.device_pixel_ratio.to_bits()
    }
}

impl PresentationScene {
    /// Runs `paint` with a patch open, then composes the scene's root around
    /// the frame node, the performance overlay `compose` names and `banner`,
    /// in the same patch: when `paint` painted, or when the banner the
    /// version shows is not `banner`. Returns whether there is a patch to
    /// [`Self::apply`].
    ///
    /// The banner is published on a frame the pipeline paints nothing in: it
    /// is the development tools', so a report that changes it repaints no
    /// boundary of the pipeline, and its patch holds the banner's node and
    /// the root alone. The performance overlay changes only with the frames
    /// it measures, so it waits for one that paints.
    pub(crate) fn produce<'t, E>(
        &mut self,
        paint: impl FnOnce(&mut SceneWriter<'_>) -> Result<bool, E>,
        banner: Option<OverlayFrame<'t, Banner>>,
        compose: impl FnOnce() -> (Option<SceneNodeId>, Option<OverlayFrame<'t, Readout>>),
    ) -> Result<bool, E> {
        let mut writer = self.owner.writer();
        let painted = paint(&mut writer)?;
        let lineage = writer.current().id().lineage();
        let banner_shown = match (
            self.banner
                .as_ref()
                .filter(|shown| shown.node.lineage() == lineage),
            banner.as_ref(),
        ) {
            (None, None) => true,
            (Some(shown), Some(wanted)) => shown.shows(wanted),
            _ => false,
        };
        if !painted && banner_shown {
            return Ok(false);
        }
        let (frame, overlay) = compose();
        Self::compose(
            &mut self.overlay,
            &mut self.banner,
            &mut writer,
            frame,
            overlay,
            banner,
        );
        Ok(true)
    }

    /// Republishes the scene's root around the pipeline's `frame` node and
    /// the overlays, in the patch `writer` buffers, when it does not slot
    /// them already; publishes an overlay's node when what it draws changed.
    fn compose(
        overlay_shown: &mut Option<ShownOverlay<Readout>>,
        banner_shown: &mut Option<ShownOverlay<Banner>>,
        writer: &mut SceneWriter<'_>,
        frame: Option<SceneNodeId>,
        overlay: Option<OverlayFrame<'_, Readout>>,
        banner: Option<OverlayFrame<'_, Banner>>,
    ) {
        let overlay = Self::compose_node(overlay_shown, writer, overlay);
        let banner = Self::compose_node(banner_shown, writer, banner);

        let current = writer.current();
        let root = current.root();
        let wanted = frame.into_iter().chain(overlay).chain(banner);
        let slotted = current
            .node(root)
            .is_some_and(|node| node.content().slots().eq(wanted.clone()));
        if slotted {
            return;
        }
        let mut content = BoundaryContent::builder();
        for node in wanted {
            content.slot(node);
        }
        writer.push(SceneOp::Publish {
            node: root,
            content: Arc::new(content.finish()),
        });
    }

    /// The node `wanted` is shown at this frame, from what `shown` showed
    /// ([`Self::compose_overlay`]); the node shown before is retired when
    /// none is shown now.
    fn compose_node<C: OverlayContent>(
        shown: &mut Option<ShownOverlay<C>>,
        writer: &mut SceneWriter<'_>,
        wanted: Option<OverlayFrame<'_, C>>,
    ) -> Option<SceneNodeId> {
        let lineage = writer.current().id().lineage();
        let previous = shown.take().filter(|shown| shown.node.lineage() == lineage);
        let retired = previous.as_ref().map(|previous| previous.node);
        *shown = match wanted {
            Some(wanted) => Self::compose_overlay(writer, previous, wanted),
            None => None,
        };
        let node = shown.as_ref().map(|shown| shown.node);
        if let Some(retired) = retired.filter(|_| node.is_none()) {
            writer.push(SceneOp::Retire { node: retired });
        }
        node
    }

    /// The overlay this frame shows: `previous` as it is when the content,
    /// the fonts and the scale are those it was recorded from, or while the
    /// text context is lent; otherwise the content recorded as a new content
    /// version and published at `previous`'s node or a new one. Content that
    /// cannot be recorded shows none.
    ///
    /// Only the overlay's own chunk takes a new version, so no recording the
    /// raster side keeps for another chunk changes with it, and content that
    /// holds still publishes nothing and damages nothing.
    fn compose_overlay<C: OverlayContent>(
        writer: &mut SceneWriter<'_>,
        previous: Option<ShownOverlay<C>>,
        overlay: OverlayFrame<'_, C>,
    ) -> Option<ShownOverlay<C>> {
        let unchanged = |fonts: u64| {
            previous
                .as_ref()
                .is_some_and(|previous| previous.shows(&overlay) && previous.fonts == fonts)
        };
        // The context is free at scene assembly; were it lent, the overlay
        // keeps what it shows, or skips the frame, rather than panic on it.
        let recorded = overlay.text.try_with(|text| {
            let fonts = text.fonts().generation();
            (!unchanged(fonts)).then(|| (fonts, overlay.content.record(text)))
        });
        let (fonts, mut recording) = match recorded {
            None => {
                tracing::warn!(
                    overlay = C::NAME,
                    "debug overlay not recorded: the ui_runtime's text context is lent"
                );
                return previous;
            }
            Some(None) => return previous,
            Some(Some((fonts, Ok(recording)))) => (fonts, recording),
            Some(Some((_, Err(error)))) => {
                tracing::warn!(
                    overlay = C::NAME,
                    %error,
                    "debug overlay skipped: label layout failed"
                );
                return None;
            }
        };
        // A lineage out of versions records no overlay: the pipeline's own
        // patch is refused with it and the scene is rebuilt.
        let version = writer.next_content_version().ok()?;
        let node = previous.map_or_else(|| writer.mint_node(), |previous| previous.node);
        let mut content = BoundaryContent::builder();
        let mut ordinal = 0;
        if (overlay.device_pixel_ratio - 1.0).abs() >= f64::EPSILON {
            content.push_effect(
                TransformLayer::new(flui_foundation::geometry::Matrix4::scaling(
                    overlay.device_pixel_ratio,
                    overlay.device_pixel_ratio,
                    1.0,
                )),
                overlay.owner,
                ordinal,
            );
            ordinal += 1;
        }
        content.push_effect(
            ClipRectLayer::hard_edge(overlay.content.bounds()),
            overlay.owner,
            ordinal,
        );
        content.chunk(writer.chunk(overlay.owner, 0, version, &mut recording));
        writer.push(SceneOp::Publish {
            node,
            content: Arc::new(content.finish()),
        });
        Some(ShownOverlay {
            node,
            content: overlay.content,
            fonts,
            device_pixel_ratio: overlay.device_pixel_ratio,
        })
    }

    /// Where the current version draws the build banner, in logical pixels;
    /// `None` while it draws none.
    pub(crate) fn banner_bounds(&self) -> Option<Rect<f64>> {
        self.banner.as_ref().map(|shown| shown.content.bounds())
    }

    /// Applies the buffered patch for `target`.
    pub(crate) fn apply(&mut self, target: SceneTarget) -> Produced {
        let surface = SurfaceKey {
            size: target.size,
            generation: SurfaceGeneration::ZERO,
        };
        // A new version, or a renewed lineage, replaces whatever was owed.
        self.owed = None;
        match self.owner.apply(surface) {
            Ok(applied) => {
                self.refusals = 0;
                self.target = Some(target);
                Produced::Applied {
                    version: self.owner.current().clone(),
                    released: applied.released,
                }
            }
            Err(error) => {
                self.refusals = self.refusals.saturating_add(1);
                let retry = self.refusals < 2;
                let released = self.renew_lineage(ResyncReason::Rebuilt);
                tracing::error!(
                    %error,
                    retry,
                    "BUG: the pipeline's scene patch was refused; the scene is rebuilt"
                );
                Produced::Refused { released, retry }
            }
        }
    }

    /// The current version with the damage since the acknowledged one.
    pub(crate) fn publication(&self) -> ScenePublication {
        self.owner.publication()
    }

    /// The raster side shows `rastered`: the log before it is let go, and a
    /// debt for that version or an older one of its lineage is paid, since the
    /// screen shows what it owed or a later tree.
    pub(crate) fn completed(&mut self, rastered: SceneVersionId) -> ReleasedContents {
        if self.owed.is_some_and(|owed| {
            owed.lineage() == rastered.lineage() && owed.seq() <= rastered.seq()
        }) {
            self.owed = None;
        }
        // A window that shows the refused version, or a later one, takes
        // frames again: a refusal that settled before a republish of the
        // same version was shown no longer holds its texture frames.
        if self.refused.is_some_and(|refused| {
            refused.lineage() == rastered.lineage() && refused.seq() <= rastered.seq()
        }) {
            self.refused = None;
        }
        self.owner.completed(Some(rastered))
    }

    /// The surface withheld `version`: the next frame owes it to the screen.
    /// A debt for a later version of the same lineage stays: a withheld
    /// settlement that arrives late never replaces a newer debt with an older
    /// version.
    fn owe(&mut self, version: SceneVersionId) {
        let newer_owed = self
            .owed
            .is_some_and(|owed| owed.lineage() == version.lineage() && owed.seq() > version.seq());
        if !newer_owed {
            self.owed = Some(version);
        }
    }

    /// What a withheld version owes the frame about to draw for `target`:
    /// [`HeldRetry::Republish`] when it is the current version and was applied
    /// for `target`, [`HeldRetry::Repaint`] otherwise. The debt stays until a
    /// verdict or a new version pays it: a frame that applies a version
    /// ([`Self::apply`]), or the raster side showing it ([`Self::completed`]).
    /// A republish ([`Self::republish`]) pays nothing, so a republished frame
    /// that is discarded before its submit, or fails, leaves it to the next.
    pub(crate) fn held_retry(&self, target: SceneTarget) -> HeldRetry {
        match self.owed {
            None => HeldRetry::None,
            Some(owed) if owed == self.owner.current().id() && self.target == Some(target) => {
                HeldRetry::Republish
            }
            Some(_) => HeldRetry::Repaint,
        }
    }
    /// Whether `textures` holds updates the held version owes a publication
    /// for (ADR-0304 §8): a retirement, or a pending frame of a texture an
    /// attached entry of the version draws. `false` unless the version was
    /// applied for `target`, the target the window's pipeline now lays out
    /// for, and while the window refuses the version ([`Self::refuse`]): a
    /// target change is carried by the repaint it causes, and a texture
    /// nothing shows, or a window that takes nothing, costs no frame; their
    /// updates wait for the next publication. The one answer the runner's
    /// gate, sampling admission and the frame's republish read.
    pub(crate) fn owes_texture_frame(
        &self,
        textures: &TextureRegistry,
        target: Option<SceneTarget>,
    ) -> bool {
        if target.is_none() || self.target != target {
            return false;
        }
        let current = self.owner.current();
        if self.refused == Some(current.id()) {
            return false;
        }
        textures.has_updates_for(|texture| current.draws_texture(texture))
    }

    /// The window refused `version`: a texture frame does not publish it
    /// again until a frame applies a version ([`Self::apply`]) or the raster
    /// side shows it ([`Self::completed`]).
    fn refuse(&mut self, version: SceneVersionId) {
        self.refused = Some(version);
    }

    /// The current version again, with the damage since the acknowledged
    /// base, for a frame that painted nothing: a withheld version owed to the
    /// screen, or the held version a texture frame rides to the raster host
    /// (ADR-0304 §7, §8). A withheld version's debt stays until the frame's
    /// verdict: shown, it is paid ([`Self::completed`]); withheld again, it
    /// is owed again.
    pub(crate) fn republish(&self) -> ScenePublication {
        self.owner.publication()
    }

    /// Starts a new lineage, which no debt or target carries over to.
    fn renew_lineage(&mut self, reason: ResyncReason) -> ReleasedContents {
        self.overlay = None;
        self.banner = None;
        self.target = None;
        self.owed = None;
        self.owner.renew(reason)
    }

    #[cfg(feature = "hot-reload")]
    /// Starts a new lineage: every node id the pipeline holds is refused, so
    /// its next frame repaints from the root (a hot reload, ADR-0304 §10).
    pub(crate) fn renew(&mut self, reason: ResyncReason) -> ReleasedContents {
        self.renew_lineage(reason)
    }
}

/// A presentation's scene, lent out whole for the span of a frame that
/// paints and applies it, so the code that frame runs (paint bodies, visual
/// update callbacks, a `tracing` subscriber) never meets it borrowed: a
/// re-entry finds it lent and gets [`SceneLent`] (ADR-0266).
#[derive(Debug)]
pub(crate) struct SceneSlot {
    scene: RefCell<Option<PresentationScene>>,
    /// A renewal asked for while the scene was lent, run when it returns.
    renew_owed: Cell<Option<ResyncReason>>,
    /// The newest version the raster side showed while the scene was lent,
    /// acknowledged when it returns.
    completed_owed: Cell<Option<SceneVersionId>>,
    /// The version the window refused while the scene was lent, latched when
    /// it returns.
    refused_owed: Cell<Option<SceneVersionId>>,
}

impl Default for SceneSlot {
    fn default() -> Self {
        Self {
            scene: RefCell::new(Some(PresentationScene::default())),
            renew_owed: Cell::new(None),
            completed_owed: Cell::new(None),
            refused_owed: Cell::new(None),
        }
    }
}

/// The presentation's scene is lent to the frame producing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the presentation's scene is lent to the frame producing it")]
pub(crate) struct SceneLent;

impl SceneSlot {
    /// Takes the scene out for the frame; it returns when the guard drops,
    /// panic or not. `Err` when a frame already holds it.
    pub(crate) fn lend(&self) -> Result<LentScene<'_>, SceneLent> {
        let scene = self.scene.borrow_mut().take().ok_or(SceneLent)?;
        Ok(LentScene {
            slot: self,
            scene: Some(scene),
        })
    }

    /// The scene's running totals; see [`crate::testing::SceneTotals`].
    /// `Err` while a frame holds it.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn totals(&self) -> Result<crate::testing::SceneTotals, SceneLent> {
        let scene = self.scene.borrow();
        let owner = &scene.as_ref().ok_or(SceneLent)?.owner;
        Ok(crate::testing::SceneTotals {
            version: owner.current().id(),
            nodes_visited: owner.table_copies() + owner.table_nodes_created(),
            depth_updates: owner.depth_updates(),
            patch_damage: owner.patch_damage(),
        })
    }
    /// The current version with the damage since the acknowledged one.
    #[cfg(test)]
    pub(crate) fn publication(&self) -> Result<ScenePublication, SceneLent> {
        self.scene
            .borrow()
            .as_ref()
            .map(PresentationScene::publication)
            .ok_or(SceneLent)
    }

    /// The raster side shows `rastered`; see [`PresentationScene::completed`].
    /// While a frame holds the scene the acknowledgement waits for its return
    /// (the newest of a lineage kept), so the debt of a withheld version it
    /// pays is not left standing, and nothing is released now.
    pub(crate) fn completed(&self, rastered: SceneVersionId) -> ReleasedContents {
        let mut scene = self.scene.borrow_mut();
        if let Some(scene) = scene.as_mut() {
            return scene.completed(rastered);
        }
        let newer_owed = self.completed_owed.get().is_some_and(|owed| {
            owed.lineage() == rastered.lineage() && owed.seq() > rastered.seq()
        });
        if !newer_owed {
            self.completed_owed.set(Some(rastered));
        }
        ReleasedContents::default()
    }

    /// The surface withheld `version`; see [`PresentationScene::held_retry`].
    /// `Err` while a frame holds the scene.
    pub(crate) fn owe(&self, version: SceneVersionId) -> Result<(), SceneLent> {
        let mut scene = self.scene.borrow_mut();
        scene.as_mut().ok_or(SceneLent)?.owe(version);
        Ok(())
    }

    /// See [`PresentationScene::owes_texture_frame`]. `false` while a frame
    /// holds the scene: that frame answers for itself.
    pub(crate) fn owes_texture_frame(
        &self,
        textures: &TextureRegistry,
        target: Option<SceneTarget>,
    ) -> bool {
        self.scene
            .borrow()
            .as_ref()
            .is_some_and(|scene| scene.owes_texture_frame(textures, target))
    }

    /// The window refused `version`; see [`PresentationScene::refuse`].
    /// While a frame holds the scene the refusal waits for its return, where
    /// a version that frame applied overrides it.
    pub(crate) fn refuse(&self, version: SceneVersionId) {
        let mut scene = self.scene.borrow_mut();
        if let Some(scene) = scene.as_mut() {
            scene.refuse(version);
        } else {
            self.refused_owed.set(Some(version));
        }
    }

    /// Runs the destructors of what the scene's owner released, here with no
    /// borrow held (an annotation's `Drop` is user code), and hands the owner
    /// back what nobody else holds, for its next frames to write into
    /// (ADR-0304 §7). While a frame holds the scene, what was kept is freed
    /// instead.
    pub(crate) fn restock(&self, released: ReleasedContents) {
        let reclaimed = released.reclaim();
        let mut scene = self.scene.borrow_mut();
        if let Some(scene) = scene.as_mut() {
            scene.owner.restock(reclaimed);
        }
    }

    /// Starts a new lineage now, or, while a frame holds the scene, when it
    /// returns: the renewal is owed, never dropped.
    #[cfg(feature = "hot-reload")]
    pub(crate) fn renew(&self, reason: ResyncReason) -> ReleasedContents {
        let mut scene = self.scene.borrow_mut();
        if let Some(scene) = scene.as_mut() {
            scene.renew(reason)
        } else {
            self.renew_owed.set(Some(reason));
            ReleasedContents::default()
        }
    }
}

/// A frame's hold on its presentation's scene; see [`SceneSlot::lend`].
pub(crate) struct LentScene<'s> {
    slot: &'s SceneSlot,
    scene: Option<PresentationScene>,
}

impl std::ops::Deref for LentScene<'_> {
    type Target = PresentationScene;

    fn deref(&self) -> &PresentationScene {
        self.scene
            .as_ref()
            .expect("BUG: a lent scene is held until the guard drops")
    }
}

impl std::ops::DerefMut for LentScene<'_> {
    fn deref_mut(&mut self) -> &mut PresentationScene {
        self.scene
            .as_mut()
            .expect("BUG: a lent scene is held until the guard drops")
    }
}

impl Drop for LentScene<'_> {
    fn drop(&mut self) {
        let Some(mut scene) = self.scene.take() else {
            return;
        };
        // An acknowledgement, a refusal, then a renewal, owed while the frame held the
        // scene run before it is back; what they release drops with the slot
        // free.
        let acknowledged = self
            .slot
            .completed_owed
            .take()
            .map(|rastered| scene.completed(rastered));
        if let Some(refused) = self.slot.refused_owed.take() {
            scene.refuse(refused);
        }
        let released = self
            .slot
            .renew_owed
            .take()
            .map(|reason| scene.renew_lineage(reason));
        *self.slot.scene.borrow_mut() = Some(scene);
        drop(acknowledged);
        drop(released);
    }
}

#[cfg(test)]
mod tests {
    use flui_layer::{DamageRegion, FullReason, SceneOwner};

    use super::*;

    /// Publishes this scene's root empty, or `foreign` when given: a node
    /// another owner minted.
    fn frame(scene: &mut PresentationScene, foreign: Option<SceneNodeId>) -> Produced {
        let painted = scene.produce(
            |writer| {
                let node = foreign.unwrap_or_else(|| writer.current().root());
                writer.push(SceneOp::Publish {
                    node,
                    content: Arc::new(BoundaryContent::builder().finish()),
                });
                Ok::<_, ()>(true)
            },
            None,
            || (None, None),
        );
        assert_eq!(painted, Ok(true));
        scene.apply(SceneTarget {
            size: (800, 600),
            scale: DevicePixelRatio::ONE,
        })
    }

    /// A readout of `fps` frames per second.
    fn readout(fps: f64) -> Readout {
        Readout::of(&crate::performance_overlay::PerformanceSample {
            fps,
            frame_time_ms: 16.7,
            diagnostic_line: None,
        })
    }

    /// A frame that paints nothing of the pipeline's and composes the
    /// overlay `readout` names, shaped through `text`; the raster side then
    /// shows the version, so the next publication's damage is that frame's
    /// own.
    fn overlay_frame(
        scene: &mut PresentationScene,
        text: &flui_rendering::TextContextHandle,
        readout: Option<Readout>,
    ) -> ScenePublication {
        let painted = scene.produce(
            |_| Ok::<_, ()>(true),
            None,
            || {
                let overlay = readout.map(|content| OverlayFrame {
                    content,
                    text,
                    device_pixel_ratio: 1.0,
                    owner: RenderId::new(1),
                });
                (None, overlay)
            },
        );
        assert_eq!(painted, Ok(true));
        assert!(matches!(
            scene.apply(SceneTarget {
                size: (800, 600),
                scale: DevicePixelRatio::ONE,
            }),
            Produced::Applied { .. }
        ));
        let publication = scene.publication();
        drop(scene.completed(publication.scene.id()));
        publication
    }

    /// The overlay's one chunk in `publication`'s version.
    fn overlay_chunk(publication: &ScenePublication) -> Option<Arc<flui_layer::Chunk>> {
        let version = &publication.scene;
        let node = version.node(version.root())?.content().slots().next()?;
        version
            .node(node)?
            .content()
            .entries()
            .iter()
            .find_map(|entry| match entry {
                flui_layer::Entry::Chunk { chunk, .. } => Some(Arc::clone(chunk)),
                flui_layer::Entry::Effect { .. } | flui_layer::Entry::Slot { .. } => None,
            })
    }

    /// A readout that holds still records nothing and damages nothing; one
    /// that changes, or whose fonts gained a face, records its chunk anew and
    /// damages the overlay's bounds alone. Fails if every frame records the
    /// readout (damage each frame) or if a change of numbers or fonts keeps
    /// the old recording (a stale overlay).
    #[test]
    fn the_overlay_records_its_readout_only_when_it_changes() {
        let fonts = flui_painting::FontCollection::new();
        let text = flui_rendering::TextContextHandle::new(flui_painting::TextContext::new(&fonts));
        let mut scene = PresentationScene::default();
        let first = overlay_frame(&mut scene, &text, Some(readout(60.0)));
        let recorded = overlay_chunk(&first).expect("the overlay is shown");

        let still = overlay_frame(&mut scene, &text, Some(readout(60.0)));
        assert_eq!(
            still.damage,
            DamageRegion::Unchanged,
            "numbers that hold still"
        );
        let kept = overlay_chunk(&still).expect("the overlay is shown");
        assert!(Arc::ptr_eq(&kept, &recorded), "the chunk is kept");

        let bounds = |publication: &ScenePublication| match &publication.damage {
            DamageRegion::Region(region) => region.bounds(),
            other => panic!("the overlay damages a region: {other:?}"),
        };
        let changed = overlay_frame(&mut scene, &text, Some(readout(30.0)));
        assert_ne!(
            overlay_chunk(&changed).map(|chunk| chunk.version()),
            Some(recorded.version()),
            "new numbers record anew"
        );
        let covering =
            flui_layer::DamageRect::try_covering(crate::performance_overlay::BOUNDS, (800, 600))
                .expect("the bounds are finite")
                .expect("the bounds meet the surface");
        assert_eq!(
            bounds(&changed).union(covering),
            covering,
            "the overlay damages inside its bounds: {:?}",
            changed.damage
        );

        let before = overlay_chunk(&changed).expect("the overlay is shown");
        fonts
            .register_font(flui_painting::testing::PROBE_MONO_100)
            .expect("the probe face loads");
        let refonted = overlay_frame(&mut scene, &text, Some(readout(30.0)));
        assert_ne!(
            overlay_chunk(&refonted).map(|chunk| chunk.version()),
            Some(before.version()),
            "a registered face records anew"
        );
    }

    /// While the text context is lent, a shown overlay stays as it is and an
    /// overlay not yet shown waits; turning the overlay off retires its node.
    /// Fails if a lent context panics the frame, drops the shown overlay, or
    /// leaves a retired overlay slotted.
    #[test]
    fn a_lent_text_context_keeps_the_overlay_shown() {
        let text = flui_rendering::TextContextHandle::standalone();
        let mut scene = PresentationScene::default();
        let waiting = text.with(|_| overlay_frame(&mut scene, &text, Some(readout(60.0))));
        assert!(overlay_chunk(&waiting).is_none(), "nothing to show yet");

        let shown = overlay_frame(&mut scene, &text, Some(readout(60.0)));
        let recorded = overlay_chunk(&shown).expect("a free context records it");
        let lent = text.with(|_| overlay_frame(&mut scene, &text, Some(readout(30.0))));
        assert_eq!(lent.damage, DamageRegion::Unchanged);
        let kept = overlay_chunk(&lent).expect("the overlay stays shown");
        assert!(Arc::ptr_eq(&kept, &recorded));

        let off = overlay_frame(&mut scene, &text, None);
        assert!(overlay_chunk(&off).is_none(), "the overlay is retired");
    }

    /// While a frame holds the scene, code it runs that reaches the slot is
    /// refused with `SceneLent`, never a borrow error, and an acknowledgement or a renewal asked
    /// for meanwhile runs when the scene returns, panic or not.
    #[test]
    #[cfg(feature = "hot-reload")]
    fn a_lent_scene_refuses_a_reentry_and_owes_a_renewal() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let slot = SceneSlot::default();
        let lineage = |slot: &SceneSlot| {
            slot.publication()
                .expect("the scene is back")
                .scene
                .id()
                .lineage()
        };
        let first = lineage(&slot);
        {
            let mut scene = slot.lend().expect("free");
            assert!(matches!(frame(&mut scene, None), Produced::Applied { .. }));
            assert_eq!(slot.lend().err(), Some(SceneLent));
            assert_eq!(slot.publication().err(), Some(SceneLent));
            let shown = scene.publication().scene.id();
            drop(slot.completed(shown));
            drop(slot.renew(ResyncReason::Rebuilt));
            assert_eq!(
                scene.publication().scene.id().lineage(),
                first,
                "owed, not run"
            );
        }
        let renewed = lineage(&slot);
        assert_ne!(
            renewed, first,
            "the owed renewal ran when the scene returned"
        );

        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: the test unwinds through a lent scene"
        )]
        let unwound = catch_unwind(AssertUnwindSafe(|| {
            let _scene = slot.lend().expect("free");
            drop(slot.renew(ResyncReason::Rebuilt));
            panic!("paint failed");
        }));
        flui_foundation::panic::discard_payload(unwound.expect_err("the frame panicked"));
        assert_ne!(
            lineage(&slot),
            renewed,
            "a panicking frame returns the scene and runs the renewal"
        );
    }

    /// A refused patch renews the lineage: the version on screen is not a
    /// base of the next publication, which is drawn in full, and the next
    /// patch of the new lineage applies.
    #[test]
    fn a_refused_patch_rebuilds_under_a_new_lineage() {
        let mut scene = PresentationScene::default();
        let Produced::Applied { version, .. } = frame(&mut scene, None) else {
            panic!("the first patch applies");
        };
        let _ = scene.completed(version.id());

        let foreign = SceneOwner::new().writer().mint_node();
        assert!(
            matches!(frame(&mut scene, Some(foreign)), Produced::Refused { .. }),
            "a node of another lineage is refused"
        );
        let publication = scene.publication();
        assert_ne!(
            publication.scene.id().lineage(),
            version.id().lineage(),
            "the scene is rebuilt under a new lineage"
        );
        assert_eq!(publication.base, None);
        assert_eq!(
            publication.damage,
            DamageRegion::Full(FullReason::Resync(ResyncReason::Rebuilt))
        );
        assert!(
            matches!(frame(&mut scene, None), Produced::Applied { .. }),
            "the next patch applies to the new lineage"
        );
    }

    /// A refusal asks for one retry; a second in a row asks for none, since
    /// the rebuild did not cure it, and a patch that applies resets the count.
    #[test]
    fn a_second_refusal_in_a_row_asks_for_no_retry() {
        let mut scene = PresentationScene::default();
        let foreign = SceneOwner::new().writer().mint_node();
        let retries = |scene: &mut PresentationScene| match frame(scene, Some(foreign)) {
            Produced::Refused { retry, .. } => retry,
            Produced::Applied { .. } => panic!("a node of another lineage is refused"),
        };
        assert!(retries(&mut scene), "the first refusal retries");
        assert!(!retries(&mut scene), "the second in a row does not");
        assert!(!retries(&mut scene));
        assert!(matches!(frame(&mut scene, None), Produced::Applied { .. }));
        assert!(retries(&mut scene), "an applied patch resets the count");
    }

    /// A withheld version is republished only for the target it was applied for and while it is
    /// the current version; the debt lasts until a frame applies a version or the raster side
    /// shows it or a later one, and a late older debt never replaces a newer one.
    ///
    /// Fails when the retry republishes a version drawn for another size or scale, or one a newer
    /// version replaced, when a paid debt is owed again, or when a republish or an older debt
    /// pays it.
    #[test]
    fn a_withheld_version_is_republished_only_for_its_own_target() {
        let applied = SceneTarget {
            size: (800, 600),
            scale: DevicePixelRatio::ONE,
        };
        let doubled = DevicePixelRatio::try_new(2.0).expect("a ratio in range");
        let owing = || {
            let mut scene = PresentationScene::default();
            assert!(matches!(frame(&mut scene, None), Produced::Applied { .. }));
            let withheld = scene.publication().scene.id();
            scene.owe(withheld);
            (scene, withheld)
        };
        for (target, owed) in [
            (applied, HeldRetry::Republish),
            (
                SceneTarget {
                    size: (801, 600),
                    ..applied
                },
                HeldRetry::Repaint,
            ),
            (
                SceneTarget {
                    scale: doubled,
                    ..applied
                },
                HeldRetry::Repaint,
            ),
        ] {
            let (scene, _) = owing();
            assert_eq!(scene.held_retry(target), owed, "{target:?}");
            assert_eq!(scene.held_retry(target), owed, "a look pays nothing");
        }

        // A republished frame can be discarded before its submit (a sibling's scene went, the
        // first frame is deferred): only its verdict pays the debt.
        let (mut scene, withheld) = owing();
        assert_eq!(scene.republish().scene.id(), withheld);
        assert_eq!(
            scene.held_retry(applied),
            HeldRetry::Republish,
            "a republish pays nothing"
        );
        let _released = scene.completed(withheld);
        assert_eq!(
            scene.held_retry(applied),
            HeldRetry::None,
            "the republished version shown pays it"
        );

        // A withheld settlement that arrives late does not replace a newer debt.
        let (mut scene, older) = owing();
        let _released = scene.completed(older);
        assert!(matches!(frame(&mut scene, None), Produced::Applied { .. }));
        let newer = scene.publication().scene.id();
        scene.owe(newer);
        scene.owe(older);
        assert_eq!(
            scene.held_retry(applied),
            HeldRetry::Republish,
            "the newer version stays owed"
        );

        let (mut scene, withheld) = owing();
        let _released = scene.completed(withheld);
        assert_eq!(
            scene.held_retry(applied),
            HeldRetry::None,
            "a shown version pays it"
        );

        let (mut scene, withheld) = owing();
        assert!(matches!(frame(&mut scene, None), Produced::Applied { .. }));
        scene.owe(withheld);
        assert_eq!(
            scene.held_retry(applied),
            HeldRetry::Repaint,
            "a newer version replaced the withheld one"
        );
        assert!(matches!(frame(&mut scene, None), Produced::Applied { .. }));
        assert_eq!(
            scene.held_retry(applied),
            HeldRetry::None,
            "an applied version pays it"
        );

        // A settlement that shows a later version pays a debt for an earlier one.
        let (mut scene, withheld) = owing();
        assert!(matches!(frame(&mut scene, None), Produced::Applied { .. }));
        let later = scene.publication().scene.id();
        scene.owe(withheld);
        let _released = scene.completed(later);
        assert_eq!(
            scene.held_retry(applied),
            HeldRetry::None,
            "a later version shown pays it"
        );
    }

    /// The raster side showing a version while a frame holds the scene pays its debt once the
    /// frame gives the scene back.
    ///
    /// Fails when the acknowledgement of a lent scene is dropped, which leaves the shown version
    /// owed and republished by the next frame.
    #[test]
    fn a_version_shown_while_the_scene_is_lent_pays_its_debt_on_return() {
        let target = SceneTarget {
            size: (800, 600),
            scale: DevicePixelRatio::ONE,
        };
        let slot = SceneSlot::default();
        let withheld = {
            let mut scene = slot.lend().expect("the scene is in its slot");
            assert!(matches!(frame(&mut scene, None), Produced::Applied { .. }));
            scene.publication().scene.id()
        };
        slot.owe(withheld).expect("the scene is back");
        {
            let _lent = slot.lend().expect("the scene is in its slot");
            let _released = slot.completed(withheld);
        }
        let held = slot
            .scene
            .borrow()
            .as_ref()
            .map(|scene| scene.held_retry(target));
        assert_eq!(held, Some(HeldRetry::None));
    }
}
