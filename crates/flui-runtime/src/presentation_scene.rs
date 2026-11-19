//! A presentation's retained scene: the [`SceneOwner`] its pipeline's paint
//! patches and its sink draws from (ADR-0304 §7, §8).
//!
//! Each frame that paints buffers the pipeline's patch, composes the scene's
//! root around the pipeline's frame node and the debug overlay's node, and
//! applies it. The version is reported back to the pipeline, whose probes,
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

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use flui_foundation::RenderId;
use flui_foundation::SurfaceGeneration;
use flui_foundation::geometry::DevicePixelRatio;
use flui_layer::{
    BoundaryContent, Layer, ReleasedContents, ResyncReason, SceneNodeId, SceneOp, SceneOwner,
    ScenePublication, SceneVersion, SceneVersionId, SceneWriter, SurfaceKey, TransformLayer,
};

/// The retained scene of one presentation, and the node its debug overlay is
/// published at.
#[derive(Debug, Default)]
pub(crate) struct PresentationScene {
    owner: SceneOwner,
    /// The overlay's node while the overlay is shown.
    overlay: Option<SceneNodeId>,
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
}

/// What a version is drawn for: the surface's size in device pixels and the
/// device pixel ratio the pipeline painted it under.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SceneTarget {
    pub(crate) size: (u32, u32),
    pub(crate) scale: DevicePixelRatio,
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

/// The debug overlay a frame composes beside the pipeline's frame node: the
/// readout and the device pixel ratio it is drawn under.
pub(crate) struct OverlayFrame {
    pub(crate) layer: Layer,
    pub(crate) device_pixel_ratio: f64,
    /// The render object the overlay's effects are filed under.
    pub(crate) owner: RenderId,
}

impl PresentationScene {
    /// Runs `paint` with a patch open; when it painted, composes the scene's
    /// root around the frame node and the overlay `compose` names, in the
    /// same patch. Returns `paint`'s answer: whether there is a patch to
    /// [`Self::apply`].
    pub(crate) fn produce<E>(
        &mut self,
        paint: impl FnOnce(&mut SceneWriter<'_>) -> Result<bool, E>,
        compose: impl FnOnce() -> (Option<SceneNodeId>, Option<OverlayFrame>),
    ) -> Result<bool, E> {
        let mut writer = self.owner.writer();
        let painted = paint(&mut writer)?;
        if painted {
            let (frame, overlay) = compose();
            Self::compose(&mut self.overlay, &mut writer, frame, overlay);
        }
        Ok(painted)
    }

    /// Republishes the scene's root around the pipeline's `frame` node and
    /// the overlay, in the patch `writer` buffers, when it does not slot them
    /// already; publishes the overlay's node with this frame's readout.
    fn compose(
        overlay_node: &mut Option<SceneNodeId>,
        writer: &mut SceneWriter<'_>,
        frame: Option<SceneNodeId>,
        overlay: Option<OverlayFrame>,
    ) {
        let lineage = writer.current().id().lineage();
        let previous = overlay_node.take().filter(|node| node.lineage() == lineage);
        let overlay = overlay.map(|overlay| {
            let node = previous.unwrap_or_else(|| writer.mint_node());
            let mut content = BoundaryContent::builder();
            let scaled = (overlay.device_pixel_ratio - 1.0).abs() >= f64::EPSILON;
            if scaled {
                content.push_effect(
                    Layer::Transform(TransformLayer::new(
                        flui_foundation::geometry::Matrix4::scaling(
                            overlay.device_pixel_ratio,
                            overlay.device_pixel_ratio,
                            1.0,
                        ),
                    )),
                    overlay.owner,
                    0,
                );
            }
            content.push_effect(overlay.layer, overlay.owner, u32::from(scaled));
            writer.push(SceneOp::Publish {
                node,
                content: Arc::new(content.finish()),
            });
            node
        });
        if let Some(retired) = previous.filter(|_| overlay.is_none()) {
            writer.push(SceneOp::Retire { node: retired });
        }
        *overlay_node = overlay;

        let current = writer.current();
        let root = current.root();
        let wanted = frame.into_iter().chain(overlay);
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

    /// The withheld version again, with the damage since the acknowledged
    /// base, for a frame that painted nothing. The debt stays until the
    /// frame's verdict: shown, it is paid ([`Self::completed`]); withheld
    /// again, it is owed again.
    pub(crate) fn republish(&self) -> ScenePublication {
        self.owner.publication()
    }

    /// Starts a new lineage, which no debt or target carries over to.
    fn renew_lineage(&mut self, reason: ResyncReason) -> ReleasedContents {
        self.overlay = None;
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
}

impl Default for SceneSlot {
    fn default() -> Self {
        Self {
            scene: RefCell::new(Some(PresentationScene::default())),
            renew_owed: Cell::new(None),
            completed_owed: Cell::new(None),
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
    #[cfg(all(test, feature = "hot-reload"))]
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
        // An acknowledgement, then a renewal, owed while the frame held the
        // scene run before it is back; what they release drops with the slot
        // free.
        let acknowledged = self
            .slot
            .completed_owed
            .take()
            .map(|rastered| scene.completed(rastered));
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
            || (None, None),
        );
        assert_eq!(painted, Ok(true));
        scene.apply(SceneTarget {
            size: (800, 600),
            scale: DevicePixelRatio::ONE,
        })
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
