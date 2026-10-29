//! What a Linux display protocol hands the snapshot, and the step that gives
//! each output its id.
//!
//! Both protocol models (`wayland_model`, `x11_model`, each built with its
//! feature) end in a list of [`OutputView`]s; this
//! module turns that list into [`DisplayInfo`]s. An output the protocol keys
//! gets the id of its key; an output it cannot key gets one fresh id that stays
//! its own while the native object it stands for exists.

use std::collections::HashMap;

use flui_foundation::geometry::Bounds;
#[cfg(any(feature = "x11", test))]
use flui_foundation::geometry::{Point, Size};
use flui_platform::{
    CapabilityAbsence, DisplayColor, DisplayId, DisplayInfo, DisplayKey, RefreshPeriod,
};

/// How an output is told apart from the others.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Identity {
    /// The protocol names the output: the same key is the same display for as
    /// long as the registry lives, across a replug.
    Keyed(DisplayKey),
    /// The protocol has no stable name for it (a `wl_output` older than
    /// version 4). `slot` is the number of the native object, which lives as
    /// long as the output does; the output keeps one id for that long and a
    /// new object is a new display.
    Fresh { slot: u32 },
}

/// One output, as a protocol model describes it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OutputView {
    pub identity: Identity,
    /// The numbers a window backend names this output by: the global name of a
    /// `wl_output`, the CRTCs of an X11 monitor.
    pub natives: Vec<u32>,
    pub name: String,
    pub bounds: Bounds<i32>,
    pub usable_bounds: Bounds<i32>,
    pub scale_factor: f64,
    pub refresh_period: Option<RefreshPeriod>,
    pub is_primary: bool,
    /// What names the output again after a restart; see
    /// [`DisplayInfo::persistent_key`].
    pub persistent_key: Option<String>,
    /// The output's colour, or why the protocol gives none; see
    /// [`DisplayInfo::color`].
    pub color: Result<DisplayColor, CapabilityAbsence>,
}

/// The numbers one display answers to, with its id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeIds {
    pub natives: Vec<u32>,
    pub id: DisplayId,
}

/// The displays of one resolved sample, and which native numbers belong to
/// which of them.
#[derive(Debug)]
pub(crate) struct Resolved {
    pub displays: Vec<DisplayInfo>,
    pub natives: Vec<NativeIds>,
}

/// The ids handed to outputs that have no key, by the slot of their native
/// object.
#[derive(Debug, Default)]
pub(crate) struct FreshIds {
    issued: HashMap<u32, DisplayId>,
}

impl FreshIds {
    /// Give every view an id through `mint`, which maps a key to its id and is
    /// `None` once the id space is used up (that output is then left out).
    ///
    /// A [`Identity::Fresh`] output keeps the id it was first given while its
    /// slot stays in the list; a slot that left the list is forgotten, so a
    /// later object with the same number is a new display.
    pub(crate) fn resolve(
        &mut self,
        views: &[OutputView],
        mut mint: impl FnMut(&DisplayKey) -> Option<DisplayId>,
    ) -> Resolved {
        self.issued.retain(|slot, _| {
            views
                .iter()
                .any(|view| view.identity == Identity::Fresh { slot: *slot })
        });
        let mut displays = Vec::with_capacity(views.len());
        let mut natives = Vec::with_capacity(views.len());
        for view in views {
            let id = match &view.identity {
                Identity::Keyed(key) => mint(key),
                Identity::Fresh { slot } => self.issued.get(slot).copied().or_else(|| {
                    let id = mint(&DisplayKey::Unkeyed)?;
                    self.issued.insert(*slot, id);
                    Some(id)
                }),
            };
            let Some(id) = id else { continue };
            let mut info = DisplayInfo::new(id, view.name.clone(), view.bounds, view.scale_factor)
                .with_usable_bounds(view.usable_bounds)
                .with_primary(view.is_primary)
                .with_color(view.color);
            info.refresh_period = view.refresh_period;
            info.persistent_key.clone_from(&view.persistent_key);
            displays.push(info);
            natives.push(NativeIds {
                natives: view.natives.clone(),
                id,
            });
        }
        Resolved { displays, natives }
    }
}

/// The display a native number belongs to: a window names its monitor by the
/// number winit gives it, and any of a display's numbers answers (a tiled X11
/// monitor has several CRTCs).
#[cfg(any(wayland_host, winit_host, test))]
pub(crate) fn display_of_native_number(natives: &[NativeIds], native: u32) -> Option<DisplayId> {
    natives
        .iter()
        .find(|ids| ids.natives.contains(&native))
        .map(|ids| ids.id)
}

/// The overlap of two rectangles, `None` when they share no area. Computed in
/// 64 bits, so rectangles at the edge of the `i32` range do not overflow.
#[cfg(any(feature = "x11", test))]
pub(crate) fn intersection(a: Bounds<i32>, b: Bounds<i32>) -> Option<Bounds<i32>> {
    let edge = |origin: i32, extent: i32| i64::from(origin) + i64::from(extent);
    let left = i64::from(a.origin.x.max(b.origin.x));
    let top = i64::from(a.origin.y.max(b.origin.y));
    let right = edge(a.origin.x, a.size.width).min(edge(b.origin.x, b.size.width));
    let bottom = edge(a.origin.y, a.size.height).min(edge(b.origin.y, b.size.height));
    if right <= left || bottom <= top {
        return None;
    }
    // Both ends lie inside `a`, which is an `i32` rectangle.
    let to_i32 = |value: i64| i32::try_from(value).ok();
    Some(Bounds::new(
        Point::new(to_i32(left)?, to_i32(top)?),
        Size::new(to_i32(right - left)?, to_i32(bottom - top)?),
    ))
}

#[cfg(test)]
mod tests {
    use flui_platform::DisplayRegistry;

    use super::*;

    fn view(identity: Identity, name: &str) -> OutputView {
        let bounds = Bounds::new(Point::new(0, 0), Size::new(1920, 1080));
        OutputView {
            identity,
            natives: vec![1],
            name: name.into(),
            bounds,
            usable_bounds: bounds,
            scale_factor: 1.0,
            refresh_period: None,
            is_primary: false,
            persistent_key: None,
            color: Err(CapabilityAbsence::NotYetOnThisHost),
        }
    }

    fn mint_with(registry: &DisplayRegistry) -> impl FnMut(&DisplayKey) -> Option<DisplayId> + '_ {
        move |key| registry.mint(key).ok()
    }

    #[test]
    fn a_fresh_output_keeps_its_id_while_its_slot_lives() {
        let registry = DisplayRegistry::new();
        let mut fresh = FreshIds::default();
        let views = [view(Identity::Fresh { slot: 7 }, "a")];
        let first = fresh.resolve(&views, mint_with(&registry));
        let again = fresh.resolve(&views, mint_with(&registry));
        assert_eq!(first.displays[0].id, again.displays[0].id);

        // The object goes away and a new one arrives: a new display.
        fresh.resolve(&[], mint_with(&registry));
        let replaced = fresh.resolve(&views, mint_with(&registry));
        assert_ne!(first.displays[0].id, replaced.displays[0].id);
    }

    #[test]
    fn two_fresh_outputs_are_two_displays() {
        let registry = DisplayRegistry::new();
        let mut fresh = FreshIds::default();
        let views = [
            view(Identity::Fresh { slot: 1 }, "a"),
            view(Identity::Fresh { slot: 2 }, "a"),
        ];
        let resolved = fresh.resolve(&views, mint_with(&registry));
        assert_eq!(resolved.displays.len(), 2);
        assert_ne!(resolved.displays[0].id, resolved.displays[1].id);
    }

    #[test]
    fn an_output_without_an_id_is_left_out() {
        let mut fresh = FreshIds::default();
        let views = [view(Identity::Fresh { slot: 1 }, "a")];
        let resolved = fresh.resolve(&views, |_| None);
        assert_eq!(resolved.displays, []);
        assert_eq!(resolved.natives, []);
    }

    #[test]
    fn intersection_does_not_overflow_at_the_edge_of_the_range() {
        // Both right edges (4e9 and 2.5e9) are past `i32::MAX`.
        let far = Bounds::new(Point::new(2_000_000_000, 0), Size::new(2_000_000_000, 10));
        let other = Bounds::new(Point::new(1_000_000_000, 0), Size::new(1_500_000_000, 10));
        let overlap = intersection(far, other).expect("they overlap");
        assert_eq!(overlap.origin.x, 2_000_000_000);
        assert_eq!(overlap.size.width, 500_000_000);
        let apart = Bounds::new(Point::new(100, 0), Size::new(10, 10));
        assert!(intersection(Bounds::new(Point::new(0, 0), Size::new(10, 10)), apart).is_none());
    }
}
