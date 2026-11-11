//! The raster side's half of the damage contract (ADR-0304 §7): the version
//! whose image it holds, and whether a publication's damage applies to it.

use crate::scene::{ScenePublication, SceneVersionId};
use crate::scene_snapshot::{DamageRegion, FullReason, ResyncReason};

use super::DamageMode;

/// The version a raster side shows, and the policy it repaints by.
///
/// A [`ScenePublication`] carries the damage since the version the producer
/// last heard was shown. That damage applies to an image of that version or
/// of a later one of the same lineage, on a surface of the same size: the
/// changes since a later version are a subset of it. Anything else is drawn
/// in full, for the reason it names.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RasterBase {
    mode: DamageMode,
    shown: Option<Shown>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shown {
    version: SceneVersionId,
    surface: (u32, u32),
}

impl RasterBase {
    /// A raster side that holds nothing yet.
    #[must_use]
    pub fn new(mode: DamageMode) -> Self {
        Self { mode, shown: None }
    }

    /// Switches damage on or off.
    pub fn set_mode(&mut self, mode: DamageMode) {
        self.mode = mode;
    }

    /// The damage to draw `publication` with on a surface of `surface`
    /// physical pixels `(width, height)`.
    #[must_use]
    pub fn damage_for(&self, publication: &ScenePublication, surface: (u32, u32)) -> DamageRegion {
        let DamageMode::On { full_above } = self.mode else {
            return DamageRegion::Full(FullReason::DamageOff);
        };
        if surface.0 == 0 || surface.1 == 0 {
            return DamageRegion::Full(FullReason::SurfaceChanged);
        }
        let Some(base) = publication.base else {
            return publication.damage;
        };
        let Some(shown) = self.shown else {
            return DamageRegion::Full(FullReason::Resync(ResyncReason::RasterReset));
        };
        if shown.surface != surface {
            return DamageRegion::Full(FullReason::SurfaceChanged);
        }
        if base.lineage() != shown.version.lineage() || base.seq() > shown.version.seq() {
            return DamageRegion::Full(FullReason::Resync(ResyncReason::BaseMismatch));
        }
        match publication.damage {
            DamageRegion::Region(region) => {
                let surface_area = f64::from(surface.0) * f64::from(surface.1);
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "an area compared against a fraction of the surface"
                )]
                let area = region.area() as f64;
                if area > full_above * surface_area {
                    DamageRegion::Full(FullReason::AboveThreshold)
                } else {
                    DamageRegion::Region(region)
                }
            }
            other => other,
        }
    }

    /// The image of `version`, drawn on a surface of `surface` pixels, is on
    /// screen.
    pub fn shown(&mut self, version: SceneVersionId, surface: (u32, u32)) {
        self.shown = Some(Shown { version, surface });
    }

    /// The raster side lost what it held: a device or surface reset.
    pub fn forget(&mut self) {
        self.shown = None;
    }

    /// The version whose image is on screen, if any.
    #[must_use]
    pub fn shown_version(&self) -> Option<SceneVersionId> {
        self.shown.map(|shown| shown.version)
    }
}
