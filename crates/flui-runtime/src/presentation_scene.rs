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

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use flui_foundation::RenderId;
use flui_foundation::SurfaceGeneration;
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

    /// Applies the buffered patch for a surface of `size` device pixels.
    pub(crate) fn apply(&mut self, size: (u32, u32)) -> Produced {
        let surface = SurfaceKey {
            size,
            generation: SurfaceGeneration::ZERO,
        };
        match self.owner.apply(surface) {
            Ok(applied) => {
                self.refusals = 0;
                Produced::Applied {
                    version: self.owner.current().clone(),
                    released: applied.released,
                }
            }
            Err(error) => {
                self.overlay = None;
                self.refusals = self.refusals.saturating_add(1);
                let retry = self.refusals < 2;
                let released = self.owner.renew(ResyncReason::Rebuilt);
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

    /// The raster side shows `rastered`: the log before it is let go.
    pub(crate) fn completed(&mut self, rastered: SceneVersionId) -> ReleasedContents {
        self.owner.completed(Some(rastered))
    }

    #[cfg(feature = "hot-reload")]
    /// Starts a new lineage: every node id the pipeline holds is refused, so
    /// its next frame repaints from the root (a hot reload, ADR-0304 §10).
    pub(crate) fn renew(&mut self, reason: ResyncReason) -> ReleasedContents {
        self.overlay = None;
        self.owner.renew(reason)
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
}

impl Default for SceneSlot {
    fn default() -> Self {
        Self {
            scene: RefCell::new(Some(PresentationScene::default())),
            renew_owed: Cell::new(None),
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
    pub(crate) fn completed(
        &self,
        rastered: SceneVersionId,
    ) -> Result<ReleasedContents, SceneLent> {
        let mut scene = self.scene.borrow_mut();
        Ok(scene.as_mut().ok_or(SceneLent)?.completed(rastered))
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
        // A renewal owed while the frame held the scene runs before it is
        // back; what it releases drops with the slot free.
        let released = self.slot.renew_owed.take().map(|reason| {
            scene.overlay = None;
            scene.owner.renew(reason)
        });
        *self.slot.scene.borrow_mut() = Some(scene);
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
        scene.apply((800, 600))
    }

    /// While a frame holds the scene, code it runs that reaches the slot is
    /// refused with `SceneLent`, never a borrow error, and a renewal asked
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
            assert!(slot.completed(shown).is_err());
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
}
