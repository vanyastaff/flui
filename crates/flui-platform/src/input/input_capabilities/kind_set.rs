//! A set of pointer kinds.

use crate::flag_set::flag_set;
use crate::pointer::PointerKind;

flag_set! {
    /// A set of [`PointerKind`]s, by kind and not by the pen's tool: the devices attached to a
    /// session ([`InputCapabilities::present`](crate::input_capabilities::InputCapabilities::present)), or the
    /// kinds a recognizer admits.
    ///
    /// [`PointerKind::Unknown`] has no member: a device whose kind the platform does not say
    /// is in no set.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_platform::input_capabilities::PointerKindSet;
    /// use flui_platform::pointer::{PenTool, PointerKind};
    ///
    /// let present = PointerKindSet::MOUSE | PointerKindSet::PEN;
    /// assert!(present.contains_kind(PointerKind::Pen { tool: PenTool::Eraser }));
    /// assert!(!present.contains_kind(PointerKind::Touch));
    /// assert_eq!(PointerKindSet::of(PointerKind::Unknown), PointerKindSet::empty());
    /// ```
    pub struct PointerKindSet: u8 {
        /// [`PointerKind::Mouse`].
        const MOUSE = 1;
        /// [`PointerKind::Touch`].
        const TOUCH = 1 << 1;
        /// [`PointerKind::Pen`], either tool.
        const PEN = 1 << 2;
        /// [`PointerKind::Trackpad`].
        const TRACKPAD = 1 << 3;
        /// [`PointerKind::Virtual`].
        const VIRTUAL = 1 << 4;
    }
}

impl PointerKindSet {
    /// The set holding `kind` alone; empty for [`PointerKind::Unknown`].
    #[must_use]
    pub const fn of(kind: PointerKind) -> Self {
        match kind {
            PointerKind::Mouse => Self::MOUSE,
            PointerKind::Touch => Self::TOUCH,
            PointerKind::Pen { .. } => Self::PEN,
            PointerKind::Trackpad => Self::TRACKPAD,
            PointerKind::Virtual => Self::VIRTUAL,
            PointerKind::Unknown => Self::empty(),
        }
    }

    /// Whether `kind` is in the set; never for [`PointerKind::Unknown`].
    #[must_use]
    pub const fn contains_kind(self, kind: PointerKind) -> bool {
        let member = Self::of(kind);
        !member.is_empty() && self.contains(member)
    }
}

impl From<PointerKind> for PointerKindSet {
    fn from(kind: PointerKind) -> Self {
        Self::of(kind)
    }
}
