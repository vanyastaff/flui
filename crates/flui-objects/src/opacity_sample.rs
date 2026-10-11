//! One owner-local alpha sample shared by box and sliver opacity producers.

use std::cell::Cell;

use flui_rendering::{
    RenderUpdateImpact, pipeline::LocalRenderInvalidationHandle, traits::PaintOpacity,
};

pub(crate) struct OpacitySample {
    generation: Cell<u64>,
    alpha: Cell<PaintOpacity>,
}

impl OpacitySample {
    pub(crate) fn new(value: f64) -> Self {
        Self {
            generation: Cell::new(0),
            alpha: Cell::new(PaintOpacity::new(value)),
        }
    }

    pub(crate) fn get(&self) -> f64 {
        self.alpha.get().alpha()
    }

    pub(crate) fn is_layered(alpha: f64) -> bool {
        alpha > 0.0 && alpha < 1.0
    }

    pub(crate) fn sample(
        &self,
        read: impl FnOnce() -> f64,
        handle: &LocalRenderInvalidationHandle,
    ) -> bool {
        let Some(generation) = self.generation.get().checked_add(1) else {
            return false;
        };
        self.generation.set(generation);
        let alpha = PaintOpacity::new(read());
        if self.generation.get() != generation {
            return false;
        }
        let old = self.alpha.replace(alpha);
        if old == alpha {
            return false;
        }
        // Paint sees the accepted sample even if the wake fails or reenters.
        // The owner retains all owed phases in one admission before that wake.
        let mut impact = if (old.alpha() == 0.0) == (alpha.alpha() == 0.0) {
            RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
        } else {
            RenderUpdateImpact::PAINT | RenderUpdateImpact::SEMANTICS
        };
        if Self::is_layered(old.alpha()) != Self::is_layered(alpha.alpha()) {
            impact |= RenderUpdateImpact::COMPOSITING_BITS;
        }
        handle.request_update(impact).is_ok()
    }
}
