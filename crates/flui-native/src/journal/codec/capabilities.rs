//! A window's capabilities on the wire: `capabilities` in the schema, each one a capability or an
//! availability, mirrored field by field through [`CapabilityParts`].

use flui_platform::{Availability, CapabilityParts, WindowCapability};
use flui_protocol::native_recording as wire;

use super::{DtoResult, absence_from_wire, absence_to_wire, unread};

const fn availability_to_wire(availability: Availability) -> wire::Availability {
    match availability {
        Availability::Native => wire::Availability::Native,
        Availability::Emulated => wire::Availability::Emulated,
        Availability::Unavailable => wire::Availability::Unavailable,
    }
}

fn availability_from_wire(availability: wire::Availability) -> DtoResult<Availability> {
    Ok(match availability {
        wire::Availability::Native => Availability::Native,
        wire::Availability::Emulated => Availability::Emulated,
        wire::Availability::Unavailable => Availability::Unavailable,
        other => return Err(unread(other.name())),
    })
}

const fn capability_to_wire(capability: WindowCapability) -> wire::Capability {
    match capability {
        WindowCapability::Native => wire::Capability::Native,
        WindowCapability::Emulated => wire::Capability::Emulated,
        WindowCapability::Absent(absence) => {
            wire::Capability::Unavailable(absence_to_wire(absence))
        }
    }
}

fn capability_from_wire(capability: wire::Capability) -> DtoResult<WindowCapability> {
    Ok(match capability {
        wire::Capability::Native => WindowCapability::Native,
        wire::Capability::Emulated => WindowCapability::Emulated,
        wire::Capability::Unavailable(absence) => {
            WindowCapability::Absent(absence_from_wire(absence)?)
        }
        _ => return Err(unread("Capability")),
    })
}

/// `capabilities` in the schema, through [`CapabilityParts`]: a capability
/// added there fails to compile here until it is mirrored.
pub(super) fn capabilities_to_wire(capabilities: CapabilityParts) -> wire::Capabilities {
    let CapabilityParts {
        sets_title,
        sets_position,
        sets_size,
        sets_constraints,
        sets_resizable,
        sets_frame,
        app_title_bar,
        minimizes,
        maximizes,
        enters_fullscreen,
        hides,
        shows,
        activates,
        closes,
        material,
        confines_cursor,
        locks_cursor,
        keeps_on_top,
        keeps_on_bottom,
        sets_resize_increments,
        sets_aspect_ratio,
        reports_presentation,
        reports_keyboard_inset,
        reports_system_gestures,
        reports_occlusion,
        observes_position,
        observes_minimized,
        places_at_open,
        frame_rate_hint,
        cursor_image,
    } = capabilities;
    wire::Capabilities {
        sets_title: capability_to_wire(sets_title),
        sets_position: capability_to_wire(sets_position),
        sets_size: capability_to_wire(sets_size),
        sets_constraints: capability_to_wire(sets_constraints),
        sets_resizable: capability_to_wire(sets_resizable),
        sets_frame: capability_to_wire(sets_frame),
        app_title_bar: capability_to_wire(app_title_bar),
        minimizes: capability_to_wire(minimizes),
        maximizes: capability_to_wire(maximizes),
        enters_fullscreen: capability_to_wire(enters_fullscreen),
        hides: capability_to_wire(hides),
        shows: capability_to_wire(shows),
        activates: capability_to_wire(activates),
        closes: capability_to_wire(closes),
        material: capability_to_wire(material),
        confines_cursor: capability_to_wire(confines_cursor),
        locks_cursor: capability_to_wire(locks_cursor),
        keeps_on_top: capability_to_wire(keeps_on_top),
        keeps_on_bottom: capability_to_wire(keeps_on_bottom),
        sets_resize_increments: capability_to_wire(sets_resize_increments),
        sets_aspect_ratio: capability_to_wire(sets_aspect_ratio),
        reports_presentation: capability_to_wire(reports_presentation),
        reports_keyboard_inset: capability_to_wire(reports_keyboard_inset),
        reports_system_gestures: capability_to_wire(reports_system_gestures),
        reports_occlusion: capability_to_wire(reports_occlusion),
        observes_position: availability_to_wire(observes_position),
        observes_minimized: availability_to_wire(observes_minimized),
        places_at_open: availability_to_wire(places_at_open),
        frame_rate_hint: availability_to_wire(frame_rate_hint),
        cursor_image: availability_to_wire(cursor_image),
    }
}

pub(super) fn capabilities_from_wire(
    capabilities: wire::Capabilities,
) -> DtoResult<CapabilityParts> {
    let wire::Capabilities {
        sets_title,
        sets_position,
        sets_size,
        sets_constraints,
        sets_resizable,
        sets_frame,
        app_title_bar,
        minimizes,
        maximizes,
        enters_fullscreen,
        hides,
        shows,
        activates,
        closes,
        material,
        confines_cursor,
        locks_cursor,
        keeps_on_top,
        keeps_on_bottom,
        sets_resize_increments,
        sets_aspect_ratio,
        reports_presentation,
        reports_keyboard_inset,
        reports_system_gestures,
        reports_occlusion,
        observes_position,
        observes_minimized,
        places_at_open,
        frame_rate_hint,
        cursor_image,
    } = capabilities;
    Ok(CapabilityParts {
        sets_title: capability_from_wire(sets_title)?,
        sets_position: capability_from_wire(sets_position)?,
        sets_size: capability_from_wire(sets_size)?,
        sets_constraints: capability_from_wire(sets_constraints)?,
        sets_resizable: capability_from_wire(sets_resizable)?,
        sets_frame: capability_from_wire(sets_frame)?,
        app_title_bar: capability_from_wire(app_title_bar)?,
        minimizes: capability_from_wire(minimizes)?,
        maximizes: capability_from_wire(maximizes)?,
        enters_fullscreen: capability_from_wire(enters_fullscreen)?,
        hides: capability_from_wire(hides)?,
        shows: capability_from_wire(shows)?,
        activates: capability_from_wire(activates)?,
        closes: capability_from_wire(closes)?,
        material: capability_from_wire(material)?,
        confines_cursor: capability_from_wire(confines_cursor)?,
        locks_cursor: capability_from_wire(locks_cursor)?,
        keeps_on_top: capability_from_wire(keeps_on_top)?,
        keeps_on_bottom: capability_from_wire(keeps_on_bottom)?,
        sets_resize_increments: capability_from_wire(sets_resize_increments)?,
        sets_aspect_ratio: capability_from_wire(sets_aspect_ratio)?,
        reports_presentation: capability_from_wire(reports_presentation)?,
        reports_keyboard_inset: capability_from_wire(reports_keyboard_inset)?,
        reports_system_gestures: capability_from_wire(reports_system_gestures)?,
        reports_occlusion: capability_from_wire(reports_occlusion)?,
        observes_position: availability_from_wire(observes_position)?,
        observes_minimized: availability_from_wire(observes_minimized)?,
        places_at_open: availability_from_wire(places_at_open)?,
        frame_rate_hint: availability_from_wire(frame_rate_hint)?,
        cursor_image: availability_from_wire(cursor_image)?,
    })
}
