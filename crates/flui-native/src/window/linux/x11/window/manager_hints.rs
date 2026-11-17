//! What an X11 window tells its window manager to hold beyond its mode: its
//! size hints (`WM_NORMAL_HINTS`: the limits, the resize steps and the
//! proportion the user's resize keeps), its level (`_NET_WM_STATE_ABOVE`,
//! `_NET_WM_STATE_BELOW`), its title (`_NET_WM_NAME`, `WM_NAME`) and its
//! decorations (`_MOTIF_WM_HINTS`).

use x11rb::properties::{WmSizeHints, WmSizeHintsSpecification};
use x11rb::protocol::xproto::{self, AtomEnum, ConnectionExt as _, PropMode};
use x11rb::wrapper::ConnectionExt as _;

use flui_foundation::geometry::Size;
use flui_platform::WindowLevel;

use super::super::hints::{motif_hints, resize_steps, size_limits};
use super::super::mode::with_level;
use super::{StateAction, X11Window};

impl X11Window {
    /// The content size of the window now, in device pixels.
    pub(super) fn content_device(&self) -> Size<i32> {
        self.xcb()
            .get_geometry(self.xid)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map_or(Size::new(1, 1), |geometry| {
                Size::new(i32::from(geometry.width), i32::from(geometry.height))
            })
    }

    /// Write `WM_NORMAL_HINTS` from the limits, the resize steps and the
    /// proportion the window keeps, and the place it opens at, `position`,
    /// in device pixels at the window's scale.
    pub(super) fn write_size_hints(&self, position: Option<(i16, i16)>) {
        let (constraints, resizable, increments, aspect) = {
            let record = self.record.borrow();
            (
                record.constraints,
                record.resizable,
                record.resize_increments,
                record.aspect_ratio,
            )
        };
        let scale = self.scale();
        let limits = size_limits(constraints, resizable, self.content_device(), scale);
        let steps = resize_steps(increments, aspect, limits.min, scale);
        let hints = WmSizeHints {
            position: position.map(|(x, y)| {
                (
                    WmSizeHintsSpecification::ProgramSpecified,
                    i32::from(x),
                    i32::from(y),
                )
            }),
            min_size: limits.min,
            max_size: limits.max,
            size_increment: steps.increment,
            base_size: steps.base,
            aspect: steps.aspect.map(|(width, height)| {
                let ratio = x11rb::properties::AspectRatio::new(width, height);
                (ratio, ratio)
            }),
            win_gravity: Some(xproto::Gravity::NORTH_WEST),
            ..WmSizeHints::default()
        };
        if let Err(error) = hints.set_normal_hints(self.xcb(), self.xid) {
            tracing::debug!(%error, "cannot set an X11 window's size hints");
        }
    }

    /// Keep the window above the others, below them, or neither
    /// (`_NET_WM_STATE_ABOVE`, `_NET_WM_STATE_BELOW`): asked of the window
    /// manager for a window it manages, written in the window's own
    /// `_NET_WM_STATE` for one not mapped yet, which the window manager reads
    /// as it maps it. The snapshot reads the level the window manager keeps.
    pub(super) fn set_level(&self, level: WindowLevel) {
        let levels = self.atoms.levels;
        if self.unmapped() {
            if let Some(atoms) = with_level(&self.own_net_wm_state(), level, &levels)
                && let Err(error) = self.xcb().change_property32(
                    PropMode::REPLACE,
                    self.xid,
                    self.atoms.net_wm_state,
                    AtomEnum::ATOM,
                    &atoms,
                )
            {
                tracing::debug!(%error, ?level, "cannot set an X11 window's level before it is mapped");
            }
            return;
        }
        match level {
            WindowLevel::Normal => {
                self.send_state(StateAction::Remove, levels.above, levels.below);
            }
            WindowLevel::AlwaysOnTop => {
                self.send_state(StateAction::Remove, levels.below, 0);
                self.send_state(StateAction::Add, levels.above, 0);
            }
            WindowLevel::AlwaysOnBottom => {
                self.send_state(StateAction::Remove, levels.above, 0);
                self.send_state(StateAction::Add, levels.below, 0);
            }
        }
    }

    /// Set the title in `_NET_WM_NAME` (UTF-8) and `WM_NAME`, which older
    /// window managers read.
    pub(super) fn write_title(&self, title: &str) {
        let net_name = self.xcb().change_property8(
            PropMode::REPLACE,
            self.xid,
            self.atoms.net_wm_name,
            self.atoms.utf8_string,
            title.as_bytes(),
        );
        let name = self.xcb().change_property8(
            PropMode::REPLACE,
            self.xid,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            title.as_bytes(),
        );
        if let Err(error) = net_name.and(name) {
            tracing::debug!(%error, "cannot set an X11 window's title");
        }
    }

    /// Set `_MOTIF_WM_HINTS` to `decorated`.
    pub(super) fn write_decorations(&self, decorated: bool) {
        if let Err(error) = self.xcb().change_property32(
            PropMode::REPLACE,
            self.xid,
            self.atoms.motif_wm_hints,
            self.atoms.motif_wm_hints,
            &motif_hints(decorated),
        ) {
            tracing::debug!(%error, "cannot set an X11 window's decorations");
        }
    }
}
