#![cfg(test)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui::animation::Vsync;
use flui::interaction::{Code, FocusNode, Key, KeyState, KeyboardEvent, Location, Modifiers};
use flui::material::{ElevatedButton, TextFormField, Theme, ThemeData};
use flui::prelude::*;
use flui::testing::a11y::{Action, ActionRequest, TreeId, invoke_semantics_action};
use flui::testing::widgets::{LaidOut, lay_out_animated, tight};
use flui::view::EventError;
use flui::widgets::{
    Form, FormHandle, Router, RouterHandle, TextEditingController, VsyncScope, column,
};

#[derive(Clone, Debug, PartialEq, Routable)]
enum NoteRoute {
    #[route("/")]
    Edit,
    #[route("/saved")]
    Saved,
}

#[derive(Clone, Default)]
struct Observed {
    router: Rc<RefCell<Option<RouterHandle<NoteRoute>>>>,
    saves: Rc<Cell<usize>>,
}

#[derive(Clone, StatefulView)]
struct Notes {
    controller: TextEditingController,
    focus: Rc<FocusNode>,
    observed: Observed,
}

#[derive(Default)]
struct NotesState {
    saved: Signal<String>,
}

impl StatefulView for Notes {
    type State = NotesState;

    fn create_state(&self) -> NotesState {
        NotesState::default()
    }
}

impl ViewState<Notes> for NotesState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        self.saved = cx.signal(String::new());
    }

    fn build(&self, view: &Notes, _cx: &dyn BuildContext) -> impl IntoView {
        let view = view.clone();
        let saved = self.saved;
        Router::new(NoteRoute::Edit, move |route: &NoteRoute, _cx| match route {
            NoteRoute::Edit => Editor {
                controller: view.controller.clone(),
                focus: view.focus.clone(),
                observed: view.observed.clone(),
                saved,
            }
            .boxed(),
            NoteRoute::Saved => SavedNote { saved }.boxed(),
        })
    }
}

#[derive(Clone, StatelessView)]
struct SavedNote {
    saved: Signal<String>,
}

impl StatelessView for SavedNote {
    fn build(&self, cx: &dyn BuildContext) -> impl IntoView {
        Text::new(format!("Saved: {}", self.saved.get(cx)))
    }
}

#[derive(Clone, StatefulView)]
struct Editor {
    controller: TextEditingController,
    focus: Rc<FocusNode>,
    observed: Observed,
    saved: Signal<String>,
}

#[derive(Default)]
struct EditorState {
    form: FormHandle,
    router: Option<RouterHandle<NoteRoute>>,
    observed: Observed,
}

impl StatefulView for Editor {
    type State = EditorState;

    fn create_state(&self) -> EditorState {
        EditorState {
            observed: self.observed.clone(),
            ..EditorState::default()
        }
    }
}

impl ViewState<Editor> for EditorState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        self.router = Some(Router::<NoteRoute>::handle(cx).expect("editor under router"));
        *self.observed.router.borrow_mut() = self.router.clone();
    }

    fn build(&self, view: &Editor, _cx: &dyn BuildContext) -> impl IntoView {
        let router = self.router.clone().expect("mounted editor");
        let form = self.form.clone();
        let saved = view.saved;
        let saves = view.observed.saves.clone();
        Form::new(Column::new(column![
            SizedBox::new(300.0, 60.0).child(
                ElevatedButton::new(Text::new("Save note")).on_pressed(
                    move |cx| -> Result<(), EventError> {
                        if form.validate() {
                            form.save(cx)?;
                            router.push(NoteRoute::Saved).expect("mounted router");
                        }
                        Ok(())
                    },
                ),
            ),
            TextFormField::new(view.controller.clone())
                .focus_node(view.focus.clone())
                .validator(|text| {
                    (text.trim().len() < 3).then(|| "Use at least three characters".to_owned())
                })
                .on_saved(move |cx, text| {
                    saved.set(cx, text.clone())?;
                    saves.set(saves.get() + 1);
                    Ok::<(), flui::view::SignalError>(())
                }),
        ]))
        .handle(self.form.clone())
    }
}

#[test]
fn notes_edit_validation_save_and_navigation_use_real_dispatch() {
    notes_workflow(false);
}

#[test]
fn notes_edit_validation_save_and_navigation_use_semantic_actions() {
    notes_workflow(true);
}

fn activate_save(tree: &mut LaidOut, semantics: bool) {
    if semantics {
        let tree_snapshot = tree.a11y_tree().expect("semantics enabled");
        let button = tree_snapshot
            .find_by_label("Save note")
            .expect("save button semantics");
        assert!(button.supports_action(Action::Click));
        invoke_semantics_action(
            &tree.pipeline_owner(),
            ActionRequest {
                action: Action::Click,
                target_tree: TreeId::ROOT,
                target_node: button.id(),
                data: None,
            },
        )
        .expect("save action resolves");
    } else {
        tree.dispatch_pointer_down(150.0, 30.0);
        tree.dispatch_pointer_up(150.0, 30.0);
    }
    // Does not dirty the root: the action must schedule its own delivery.
    tree.tick();
}

fn notes_workflow(semantics: bool) {
    let controller = TextEditingController::new();
    let focus = FocusNode::with_debug_label("note title");
    let observed = Observed::default();
    let vsync = Vsync::new();
    let mut tree = lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            Theme::new(
                ThemeData::light(),
                Notes {
                    controller: controller.clone(),
                    focus: focus.clone(),
                    observed: observed.clone(),
                },
            ),
        ),
        tight(300.0, 300.0),
        vsync,
    );
    let router = observed.router.borrow().clone().expect("mounted editor");
    tree.enable_semantics();
    tree.tick();

    // Focus acquisition is public; the edit itself follows the actual key path.
    focus.request_focus();
    let type_text = |text: &str| KeyboardEvent {
        code: Code::KeyA,
        key: Key::Character(text.to_owned()),
        state: KeyState::Down,
        location: Location::Standard,
        modifiers: Modifiers::empty(),
        repeat: false,
        is_composing: false,
    };
    tree.focus_manager().dispatch_key_event(&type_text("a"));
    assert_eq!(controller.text(), "a");
    activate_save(&mut tree, semantics);
    tree.tick();
    assert_eq!(observed.saves.get(), 0, "invalid form must not save");
    assert_eq!(
        router.current(),
        NoteRoute::Edit,
        "invalid form must not navigate"
    );
    assert!(tree.find_text("Use at least three characters").is_some());

    focus.request_focus();
    tree.focus_manager().dispatch_key_event(&type_text("bc"));
    assert_eq!(controller.text(), "abc");
    activate_save(&mut tree, semantics);
    for _ in 0..10 {
        tree.pump_for(Duration::from_millis(50));
    }
    assert_eq!(
        observed.saves.get(),
        1,
        "valid save callback runs exactly once"
    );
    assert_eq!(router.current(), NoteRoute::Saved);
    assert_eq!(router.location().as_str(), "/saved");
    let rendered = tree
        .find_text("Saved: abc")
        .expect("saved signal reaches new page");
    assert!(
        tree.try_size(rendered)
            .is_some_and(|size| size.width > 0.0)
    );
}
