//! The selection menu of an editable text: a popup a secondary click, the
//! menu key or assistive technology opens, and a toolbar over the selection
//! a touch leaves, both offering the field's editing commands (cut, copy,
//! paste, select all) and the commands its [`DefaultSelectionControls`] and
//! [`EditableText::menu_command`] add. What they look like is the
//! [`SelectionMenu`] given to the field or to the nearest
//! [`DefaultSelectionControls`]; without one the field shows no menu, which
//! a debug build reports once. The menu itself is the shared
//! [`MenuHost`].

use std::cell::RefCell;
use std::rc::Rc;

use flui_foundation::geometry::Offset;
use flui_view::prelude::*;

use flui_interaction::focus::FocusNode;
use flui_platform::text_store::OwnerCalls;

use super::{CompositionCommit, EditableText};
use crate::interaction::{GestureCommand, GestureDetector, MenuCause};
#[cfg(doc)]
use crate::text::DefaultSelectionControls;
use crate::text::controller::TextEditingController;
use crate::text::{MenuHost, SelectionMenu, SelectionMenuPolicy};

impl EditableText {
    /// Show the selection menu with `menu`, in place of the nearest
    /// [`DefaultSelectionControls`]' look.
    ///
    /// A secondary click, the menu key or assistive technology's
    /// `ShowContextMenu` opens it as a popup; a touch selection shows it as
    /// a toolbar over the selection, as [`Self::menu_policy`] says. Either
    /// offers cut, copy, paste and select all while the field can perform
    /// them, then the commands of the nearest [`DefaultSelectionControls`]
    /// and those added with [`Self::menu_command`].
    #[must_use]
    pub fn menu(mut self, menu: impl SelectionMenu) -> Self {
        self.menu.show(menu);
        self
    }

    /// Show no selection menu, though a [`DefaultSelectionControls`] above
    /// gives one.
    #[must_use]
    pub fn no_menu(mut self) -> Self {
        self.menu.hide();
        self
    }

    /// When a touch shows the toolbar, in place of the nearest
    /// [`DefaultSelectionControls`]' policy or the platform's
    /// ([`SelectionMenuPolicy::for_platform`] of the [`PlatformConvention`](flui_view::environment::PlatformConvention)).
    #[must_use]
    pub fn menu_policy(mut self, policy: SelectionMenuPolicy) -> Self {
        self.menu.set_policy(policy);
        self
    }

    /// Also offer `command` in the selection menu, after the editing
    /// commands and those of the nearest [`DefaultSelectionControls`]. It
    /// performs through the actions at the field, so an app's `Actions`
    /// above the field answers its intent.
    #[must_use]
    pub fn menu_command(mut self, command: GestureCommand) -> Self {
        self.menu.add_command(command);
        self
    }
}

/// A secondary press at `position`, over source offset `at`: the field
/// takes the focus, the caret moves there unless the press is on the
/// selection, which the menu then acts on, and the popup opens. A
/// composition is committed first; each step runs though an earlier one's
/// owner code failed, and the first failure is resumed after the popup
/// opened.
pub(super) fn secondary_press(
    ctx: &mut EventCx<'_>,
    menu: &MenuHost,
    position: Offset<f64>,
    at: Option<usize>,
    field: (&Rc<RefCell<TextEditingController>>, &Rc<FocusNode>),
    commit: &CompositionCommit,
) {
    let (controller, focus_node) = field;
    let mut calls = OwnerCalls::new();
    commit.run(&mut calls);
    calls.run(|| {
        let _ = focus_node.request_focus();
    });
    let controller = controller.borrow().clone();
    let selection = controller.selection();
    if let Some(at) = at
        && (selection.is_empty() || !(selection.start..=selection.end).contains(&at))
    {
        calls.run(|| controller.set_caret_byte_offset(at));
    }
    calls.run(|| {
        menu.open(ctx, MenuCause::SecondaryClick, Some(position));
    });
    calls.resume();
}

/// `detector` opening the field's popup on the secondary tap a press in
/// the field began (the field's pointer handler records where), through
/// the arena: a `ContextMenuArea` around the field loses that tap, so one
/// click opens one menu.
pub(super) fn with_secondary_tap(
    detector: GestureDetector,
    menu: Rc<MenuHost>,
    field: (Rc<RefCell<TextEditingController>>, Rc<FocusNode>),
    commit: CompositionCommit,
) -> GestureDetector {
    detector.on_secondary_tap(move |ctx| {
        if let Some((position, at)) = menu.take_secondary() {
            secondary_press(ctx, &menu, position, at, (&field.0, &field.1), &commit);
        }
    })
}
