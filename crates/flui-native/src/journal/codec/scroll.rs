//! A scroll row's wire form: the step `ContactRegistry::scroll` replays, field by field.

use flui_platform::EventTime;
use flui_platform::input::pointer::{
    ScrollDelta, ScrollInversion, ScrollPhase, ScrollPrecision, ScrollUnit,
};
use flui_protocol::native_recording as wire;

use super::{DtoError, DtoResult, modifiers_from_wire, modifiers_to_wire, nanos, unread};
use crate::input::assembly::{ContactScroll, NativeContact, NativeDevice, NativeSurface};

pub(super) fn scroll_to_wire(reading: &ContactScroll) -> DtoResult<wire::Scroll> {
    let phase = reading
        .phase
        .map(|phase| {
            Ok(match phase {
                ScrollPhase::Began => wire::ScrollPhase::Began,
                ScrollPhase::Changed => wire::ScrollPhase::Changed,
                ScrollPhase::Ended => wire::ScrollPhase::Ended,
                ScrollPhase::Cancelled => wire::ScrollPhase::Cancelled,
                ScrollPhase::MomentumBegan => wire::ScrollPhase::MomentumBegan,
                ScrollPhase::MomentumChanged => wire::ScrollPhase::MomentumChanged,
                ScrollPhase::MomentumEnded => wire::ScrollPhase::MomentumEnded,
                _ => return Err(DtoError::Unmirrored("ScrollPhase")),
            })
        })
        .transpose()?;
    let (precise, notched) = match reading.precision {
        ScrollPrecision::Precise => (true, false),
        ScrollPrecision::Notched => (false, true),
        ScrollPrecision::Unknown => (false, false),
        _ => return Err(DtoError::Unmirrored("ScrollPrecision")),
    };
    Ok(wire::Scroll {
        surface: reading.surface.0,
        device: reading.device.map(|device| device.0),
        contact: reading.contact.0,
        phase,
        unit: match reading.delta.unit() {
            ScrollUnit::Lines => wire::ScrollUnit::Lines,
            ScrollUnit::Pixels => wire::ScrollUnit::Pixels,
            ScrollUnit::Pages => wire::ScrollUnit::Pages,
            ScrollUnit::Detents => wire::ScrollUnit::Detents,
            _ => return Err(DtoError::Unmirrored("ScrollUnit")),
        },
        delta: [reading.delta.x(), reading.delta.y()],
        precise,
        notched,
        inversion: match reading.inversion {
            ScrollInversion::Unknown => None,
            ScrollInversion::FromDevice => Some(wire::ScrollInversion::FromDevice),
            ScrollInversion::InvertedFromDevice => Some(wire::ScrollInversion::InvertedFromDevice),
            _ => return Err(DtoError::Unmirrored("ScrollInversion")),
        },
        time_ns: nanos(reading.time),
        modifiers: modifiers_to_wire(reading.modifiers)?,
    })
}

pub(super) fn scroll_from_wire(scroll: wire::Scroll) -> DtoResult<ContactScroll> {
    let unit = match scroll.unit {
        wire::ScrollUnit::Lines => ScrollUnit::Lines,
        wire::ScrollUnit::Pixels => ScrollUnit::Pixels,
        wire::ScrollUnit::Pages => ScrollUnit::Pages,
        wire::ScrollUnit::Detents => ScrollUnit::Detents,
        other => return Err(unread(other.name())),
    };
    let phase = scroll
        .phase
        .map(|phase| {
            Ok(match phase {
                wire::ScrollPhase::Began => ScrollPhase::Began,
                wire::ScrollPhase::Changed => ScrollPhase::Changed,
                wire::ScrollPhase::Ended => ScrollPhase::Ended,
                wire::ScrollPhase::Cancelled => ScrollPhase::Cancelled,
                wire::ScrollPhase::MomentumBegan => ScrollPhase::MomentumBegan,
                wire::ScrollPhase::MomentumChanged => ScrollPhase::MomentumChanged,
                wire::ScrollPhase::MomentumEnded => ScrollPhase::MomentumEnded,
                other => return Err(unread(other.name())),
            })
        })
        .transpose()?;
    Ok(ContactScroll {
        surface: NativeSurface(scroll.surface),
        device: scroll.device.map(NativeDevice),
        contact: NativeContact(scroll.contact),
        phase,
        delta: ScrollDelta::try_new(unit, scroll.delta[0], scroll.delta[1])
            .map_err(|error| DtoError::Invalid(format!("scroll delta: {error}")))?,
        precision: match (scroll.precise, scroll.notched) {
            (true, false) => ScrollPrecision::Precise,
            (false, true) => ScrollPrecision::Notched,
            (false, false) => ScrollPrecision::Unknown,
            (true, true) => {
                return Err(DtoError::Invalid(
                    "a scroll is precise or notched, not both".into(),
                ));
            }
        },
        inversion: match scroll.inversion {
            None => ScrollInversion::Unknown,
            Some(wire::ScrollInversion::FromDevice) => ScrollInversion::FromDevice,
            Some(wire::ScrollInversion::InvertedFromDevice) => ScrollInversion::InvertedFromDevice,
            Some(other) => return Err(unread(other.name())),
        },
        time: EventTime::from_nanos(scroll.time_ns),
        modifiers: modifiers_from_wire(&scroll.modifiers)?,
    })
}
