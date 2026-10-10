//! One owner-local alpha sample shared by box and sliver opacity producers.

use std::cell::Cell;

use flui_rendering::{RenderUpdateImpact, pipeline::LocalRenderInvalidationHandle};

pub(crate) struct OpacitySample {
    generation: Cell<u64>,
    alpha: Cell<u8>,
}

impl OpacitySample {
    pub(crate) fn new(value: f64) -> Self {
        Self {
            generation: Cell::new(0),
            alpha: Cell::new(Self::convert(value)),
        }
    }

    pub(crate) fn get(&self) -> u8 {
        self.alpha.get()
    }

    fn convert(value: f64) -> u8 {
        (value.clamp(0.0, 1.0) * 255.0).round() as u8
    }

    pub(crate) fn is_layered(alpha: u8) -> bool {
        alpha > 0 && alpha < 255
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
        let alpha = Self::convert(read());
        if self.generation.get() != generation {
            return false;
        }
        let old = self.alpha.replace(alpha);
        if old == alpha {
            return false;
        }
        // Paint sees the accepted sample even if the wake fails or reenters.
        // The owner retains all owed phases in one admission before that wake.
        let mut impact = if (old == 0) == (alpha == 0) {
            RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
        } else {
            RenderUpdateImpact::PAINT
        };
        if Self::is_layered(old) != Self::is_layered(alpha) {
            impact |= RenderUpdateImpact::COMPOSITING_BITS;
        }
        handle.request_update(impact).is_ok()
    }
}
