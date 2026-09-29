//! The mounted `EditableText` against the text-store conformance kit
//! (ADR-0090 §4): the built-in field passes the same kit a third-party field
//! runs, plain and obscured.

use std::cell::Cell;
use std::rc::Rc;

use flui_interaction::routing::FocusNode;
use flui_platform_api::TextStore;
use flui_testing::text_store_kit::{self, FixtureCapabilities, KIT_VERSION, TextStoreFixture};
use flui_widgets::{EditableText, TextEditingController};

use crate::common::harness::{Harness, mount_with_ime};

/// A focused, mounted `EditableText` whose `on_changed` calls are counted.
struct EditableTextFixture {
    harness: Harness,
    controller: TextEditingController,
    changes: Rc<Cell<usize>>,
    obscured: bool,
}

impl EditableTextFixture {
    fn new(obscured: bool) -> Self {
        let controller = TextEditingController::new();
        let focus_node = FocusNode::with_debug_label("kit field");
        let changes = Rc::new(Cell::new(0));
        let counted = Rc::clone(&changes);
        let mut harness = mount_with_ime(
            EditableText::new(controller.clone(), Rc::clone(&focus_node))
                .obscure_text(obscured)
                .on_changed(move |_cx, _| counted.set(counted.get() + 1)),
        );
        focus_node.request_focus();
        harness.tick();
        Self {
            harness,
            controller,
            changes,
            obscured,
        }
    }
}

impl TextStoreFixture for EditableTextFixture {
    fn store(&mut self) -> Rc<dyn TextStore> {
        self.harness
            .active_text_store()
            .expect("the focused field is the active IME client")
    }

    fn reset(&mut self, text: &str) {
        self.store().set_observer(None);
        // Through empty, so the caret lands at the end and any composition
        // ends even when `text` is what the field already holds.
        self.controller.set_text("");
        self.controller.set_text(text);
        self.harness.tick();
    }

    fn app_replace_all(&mut self, text: &str) {
        self.controller.set_text(text);
        self.harness.tick();
    }

    fn pump(&mut self) {
        self.harness.tick();
    }

    fn owner_notifications(&self) -> usize {
        self.changes.get()
    }

    fn capabilities(&self) -> FixtureCapabilities {
        FixtureCapabilities::new()
            .with_geometry(true)
            .with_protected(self.obscured)
    }
}

pub(crate) fn editable_text_conforms_to_kit_v1() {
    text_store_kit::assert_conforms(&mut EditableTextFixture::new(false), KIT_VERSION);
}

pub(crate) fn obscured_editable_text_conforms_to_kit_v1() {
    text_store_kit::assert_conforms(&mut EditableTextFixture::new(true), KIT_VERSION);
}
