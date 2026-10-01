//! Submission and quota handoff for the painter's trusted wgpu embedder surface.

use std::sync::Arc;

use super::WgpuPainter;
use crate::{device_domain::DeviceDomain, error::EngineResult};

impl WgpuPainter {
    pub(crate) fn domain(&self) -> &Arc<DeviceDomain> {
        &self.domain
    }

    /// Submit the encoder containing this painter's prepared draws.
    ///
    /// Call inside a successful [`Self::begin_frame`] / [`Self::finish_frame`]
    /// pair, after [`Self::render_to_view`]. This does not finish the frame or
    /// wait for GPU completion. On failure discard other unsubmitted encoders
    /// and call `finish_frame`; already submitted work remains owned until retirement.
    ///
    /// Transfers prepared resource charges to GPU completion. The encoder must
    /// contain every draw prepared since the previous submission. Raw device,
    /// queue and encoder access is a trusted embedder contract; arbitrary external
    /// allocations and submissions are outside the painter's prepared-resource quota.
    ///
    /// # Errors
    /// Returns a resource admission, owner, or device-progress failure. On failure
    /// the consumed encoder is discarded; previously submitted work still retires.
    pub fn submit_encoder(
        &mut self,
        encoder: wgpu::CommandEncoder,
    ) -> EngineResult<wgpu::SubmissionIndex> {
        self.domain.poll()?;
        let permits = self.resources.take_prepared_permits();
        let prepared = self.domain.prepare(vec![encoder.finish()], permits)?;
        Ok(self.domain.submit(prepared)?)
    }

    /// Preserve accounting for trusted callers that submit directly to the queue.
    pub(super) fn retire_prepared_after_external_submit(&mut self) {
        let permits = self.resources.take_prepared_permits();
        if !permits.is_empty() {
            self.domain.retire_after_previous_submissions(permits);
        }
    }
}
