use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use flui_foundation::PresentationId;
use flui_platform::PlatformWindow;
use flui_runtime::testing::TestWindow;
use flui_runtime::ui_runtime::UiRuntime;
use flui_view::{
    NavigationIntent, NavigationOp, NavigationRefused, UrlOwner, UrlOwnerRegistration,
};

fn intent() -> NavigationIntent {
    NavigationIntent {
        op: NavigationOp::Pop,
    }
}

fn secondary_window() -> Rc<dyn PlatformWindow> {
    Rc::new(TestWindow::new().focused(false))
}

fn install_secondary(runtime: &mut UiRuntime) -> PresentationId {
    let presentation = runtime.assemble_presentation(secondary_window());
    runtime.install_presentation(presentation)
}

fn register(
    runtime: &UiRuntime,
    presentation: PresentationId,
    owner: impl UrlOwner,
) -> UrlOwnerRegistration {
    runtime
        .presentation_widgets_for_test(presentation)
        .with_build_owner(|build_owner| {
            build_owner
                .capabilities()
                .url_owner
                .as_ref()
                .expect("runtime installs the presentation URL owner slot")
                .try_register(Rc::new(owner))
        })
        .expect("the first URL owner registers")
}

#[derive(Clone, Default)]
struct UrlOwnerLog {
    calls: Rc<RefCell<Vec<&'static str>>>,
}

impl UrlOwner for UrlOwnerLog {
    fn navigate(&self, intent: NavigationIntent) -> Result<(), NavigationRefused> {
        let operation = match intent.op {
            NavigationOp::Pop => "pop",
            _ => "other",
        };
        self.calls.borrow_mut().push(operation);
        Ok(())
    }
}

#[test]
fn navigation_addressed_to_the_second_presentation_reaches_only_its_url_owner() {
    let mut runtime = UiRuntime::for_test();
    let primary = runtime.presentation_id();
    let secondary = install_secondary(&mut runtime);
    let primary_log = UrlOwnerLog::default();
    let secondary_log = UrlOwnerLog::default();
    let _registration = register(&runtime, primary, primary_log.clone());
    let _registration = register(&runtime, secondary, secondary_log.clone());

    let result = runtime.navigate(secondary, intent());

    assert!(result.is_ok(), "the addressed URL owner accepts");
    assert!(primary_log.calls.borrow().is_empty());
    assert_eq!(secondary_log.calls.borrow().as_slice(), ["pop"]);
}

#[test]
fn live_presentation_without_a_router_refuses_instead_of_using_a_sibling() {
    let mut runtime = UiRuntime::for_test();
    let primary = runtime.presentation_id();
    let secondary = install_secondary(&mut runtime);
    let secondary_log = UrlOwnerLog::default();
    let _registration = register(&runtime, secondary, secondary_log.clone());

    let result = runtime.navigate(primary, intent());

    assert!(matches!(result, Err(NavigationRefused::NoUrlOwner)));
    assert!(secondary_log.calls.borrow().is_empty());
}

#[test]
fn invalid_and_closed_presentations_refuse_without_disturbing_the_sibling() {
    let mut runtime = UiRuntime::for_test();
    let primary = runtime.presentation_id();
    let secondary = install_secondary(&mut runtime);
    let primary_log = UrlOwnerLog::default();
    let secondary_log = UrlOwnerLog::default();
    let _registration = register(&runtime, primary, primary_log.clone());
    let _registration = register(&runtime, secondary, secondary_log.clone());
    let invalid = PresentationId::new_gen(primary.index() + 100, primary.generation());

    let invalid_result = runtime.navigate(invalid, intent());
    assert!(matches!(
        invalid_result,
        Err(NavigationRefused::NoPresentation)
    ));
    assert!(primary_log.calls.borrow().is_empty());
    assert!(secondary_log.calls.borrow().is_empty());

    assert!(runtime.close_presentation_entered(secondary));
    let closed_result = runtime.navigate(secondary, intent());
    assert!(matches!(
        closed_result,
        Err(NavigationRefused::NoPresentation)
    ));
    assert!(primary_log.calls.borrow().is_empty());
    assert!(secondary_log.calls.borrow().is_empty());

    assert!(runtime.navigate(primary, intent()).is_ok());
    assert_eq!(primary_log.calls.borrow().as_slice(), ["pop"]);
    assert!(secondary_log.calls.borrow().is_empty());
}

struct ReentrantUrlOwner {
    runtime: Weak<UiRuntime>,
    presentation: PresentationId,
    entered: Cell<bool>,
    calls: Rc<Cell<usize>>,
    nested: Rc<RefCell<Option<Result<(), NavigationRefused>>>>,
}

impl UrlOwner for ReentrantUrlOwner {
    fn navigate(&self, _intent: NavigationIntent) -> Result<(), NavigationRefused> {
        self.calls.set(self.calls.get() + 1);
        if !self.entered.replace(true) {
            let runtime = self.runtime.upgrade().expect("test runtime stays live");
            let nested = runtime.navigate(self.presentation, intent());
            self.nested.replace(Some(nested));
            self.entered.set(false);
        }
        Ok(())
    }
}

#[test]
fn url_owner_can_navigate_reentrantly_after_runtime_guards_are_released() {
    let runtime = Rc::new(UiRuntime::for_test());
    let presentation = runtime.presentation_id();
    let calls = Rc::new(Cell::new(0));
    let nested = Rc::new(RefCell::new(None));
    let _registration = register(
        &runtime,
        presentation,
        ReentrantUrlOwner {
            runtime: Rc::downgrade(&runtime),
            presentation,
            entered: Cell::new(false),
            calls: Rc::clone(&calls),
            nested: Rc::clone(&nested),
        },
    );

    let result = runtime.navigate(presentation, intent());

    assert!(result.is_ok(), "both owner callbacks return");
    assert_eq!(calls.get(), 2, "the nested navigation reached the owner");
    assert!(matches!(nested.borrow().as_ref(), Some(Ok(()))));
}
