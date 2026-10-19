//! A native batch of readings delivered as one move.

use flui_foundation::geometry::Point;
use flui_platform_api::EventTime;
use flui_platform_api::pointer::{PointerMove, PointerPosition, PointerSample};

use super::clock::{ClockChoice, NativeClock};
use super::registry::sample;
use super::{Assembled, ContactRegistry, DecodeError, DecodeFault, NativeReading};

/// The readings a platform coalesced into one event, and the ones it predicts,
/// attached to the move their current reading assembles into.
///
/// Coalesced readings are pushed oldest first. Each is stamped on the
/// contact's clock in order, so the batch keeps its own spacing and never runs
/// backwards; one that fails to decode fails the whole batch, which cancels
/// the contact like any undecodable reading. A prediction that fails is only
/// dropped. The batch is reused: its buffers keep their capacity between
/// events.
#[derive(Debug, Default)]
pub struct MoveBatch {
    coalesced: Vec<NativeReading>,
    predicted: Vec<NativeReading>,
    coalesced_samples: Vec<PointerSample>,
    predicted_samples: Vec<PointerSample>,
}

impl MoveBatch {
    /// An empty batch.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            coalesced: Vec::new(),
            predicted: Vec::new(),
            coalesced_samples: Vec::new(),
            predicted_samples: Vec::new(),
        }
    }

    /// Add a reading the platform coalesced before the current one.
    pub fn push_coalesced(&mut self, reading: NativeReading) {
        self.coalesced.push(reading);
    }

    /// Add a reading the platform predicts after the current one.
    pub fn push_predicted(&mut self, reading: NativeReading) {
        self.predicted.push(reading);
    }

    /// Assemble `current` like [`decode`](super::decode), with this batch's
    /// readings attached to its move, and empty the batch.
    ///
    /// # Errors
    ///
    /// As [`decode`](super::decode); a coalesced reading that fails fails the
    /// batch.
    pub fn decode(
        &mut self,
        contacts: &mut ContactRegistry,
        current: &NativeReading,
    ) -> Result<Assembled, DecodeError> {
        journal!(
            contacts.journal,
            super::capture::Captured::Batch {
                coalesced: &self.coalesced,
                predicted: &self.predicted,
                current,
            }
        );
        let assembled = contacts.assemble(current, Some(self));
        self.coalesced.clear();
        self.predicted.clear();
        self.coalesced_samples.clear();
        self.predicted_samples.clear();
        assembled
    }

    /// Validate and stamp the batch; the time of its last coalesced reading,
    /// which the current one may not precede.
    pub(super) fn measure(
        &mut self,
        clock: &NativeClock,
        choice: ClockChoice,
        floor: Option<EventTime>,
    ) -> Result<Option<EventTime>, DecodeFault> {
        let mut floor = floor;
        self.coalesced_samples.clear();
        for reading in &self.coalesced {
            let time = clock.stamp(reading.time, choice).ok_or(DecodeFault::Time)?;
            let time = floor.map_or(time, |floor| time.max(floor));
            let position = position(reading)?;
            self.coalesced_samples
                .push(sample(time, position, reading)?);
            floor = Some(time);
        }
        self.predicted_samples.clear();
        for reading in &self.predicted {
            let Some(time) = clock.stamp(reading.time, choice) else {
                continue;
            };
            if let Ok(sample) =
                position(reading).and_then(|position| sample(time, position, reading))
            {
                self.predicted_samples.push(sample);
            }
        }
        Ok(floor)
    }

    /// `movement` with the measured readings attached.
    pub(super) fn attach(&mut self, mut movement: PointerMove) -> PointerMove {
        if !self.coalesced_samples.is_empty() {
            movement = movement.with_coalesced(std::mem::take(&mut self.coalesced_samples));
        }
        if !self.predicted_samples.is_empty() {
            movement = movement.with_predicted(std::mem::take(&mut self.predicted_samples));
        }
        movement
    }
}

fn position(reading: &NativeReading) -> Result<PointerPosition, DecodeFault> {
    let raw = reading.position.ok_or(DecodeFault::MissingPosition)?;
    if !(reading.scale.is_finite() && reading.scale > 0.0) {
        return Err(DecodeFault::Scale(reading.scale));
    }
    Ok(PointerPosition::try_new(Point::new(
        raw.x / reading.scale,
        raw.y / reading.scale,
    ))?)
}
