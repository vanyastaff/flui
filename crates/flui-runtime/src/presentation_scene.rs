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
}

/// What [`PresentationScene::apply`] did with a frame's patch.
pub(crate) enum Produced {
    /// The patch is the new version: report it to the pipeline and publish.
    Applied {
        version: SceneVersion,
        released: ReleasedContents,
    },
    /// The owner refused the patch and renewed its lineage; the pipeline
    /// repaints into it on the next frame.
    Refused { released: ReleasedContents },
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
            .map(|node| node.content().slots().eq(wanted.clone()))
            .unwrap_or(false);
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
            Ok(applied) => Produced::Applied {
                version: self.owner.current().clone(),
                released: applied.released,
            },
            Err(error) => {
                tracing::error!(
                    %error,
                    "BUG: the pipeline's scene patch was refused; the scene is rebuilt"
                );
                self.overlay = None;
                Produced::Refused {
                    released: self.owner.renew(ResyncReason::Rebuilt),
                }
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
}
