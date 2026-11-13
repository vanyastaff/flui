//! Navigation — one typed `Router`: a push, a page popped with a result, and a
//! dialog (ADR-0330).
//!
//! The routes are an enum with a path per variant. A page reaches its router
//! with `Router::of(ctx)` and its own route with `PageHandle::of(ctx)` in
//! `build`, as a Flutter page writes `GoRouter.of(context)` and
//! `ModalRoute.of(context)`, and moves the handle into the callback that
//! navigates: a callback runs after the build, with no place in the tree.
//!
//! Run with: cargo run --example navigation --features material

use std::cell::RefCell;
use std::rc::Rc;

use flui::prelude::*;
use flui::view::{AsyncDriver, TaskToken, WriterSource};

#[derive(Routable, Clone, Debug, PartialEq)]
enum AppRoute {
    #[route("/")]
    Inbox,
    #[route("/message/:id")]
    Message { id: u32 },
}

fn main() -> Result<(), LaunchError> {
    let router = Router::new(AppRoute::Inbox, |route: &AppRoute, _ctx| match route {
        AppRoute::Inbox => Inbox.boxed(),
        AppRoute::Message { id } => Message { id: *id }.boxed(),
    });
    run_app(MaterialApp::router(router))
}

/// The inbox opens a message and shows what the message page answered.
#[derive(Clone, StatefulView)]
struct Inbox;

struct InboxState {
    answer: Signal<String>,
    writer: WriterSource,
    driver: Option<AsyncDriver>,
    /// The task awaiting the open message's result; dropping it cancels it.
    waiting: Rc<RefCell<Option<TaskToken>>>,
}

impl StatefulView for Inbox {
    type State = InboxState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        InboxState {
            answer: ctx.signal("Open the message".to_owned()),
            writer: ctx.writer_source(),
            driver: ctx.async_driver(),
            waiting: Rc::default(),
        }
    }
}

impl ViewState<Inbox> for InboxState {
    fn build(&self, _view: &Inbox, ctx: &dyn BuildContext) -> impl IntoView {
        let router = Router::<AppRoute>::of(ctx).expect("BUG: the inbox is a page of the Router");
        let (answer, writer, driver) = (self.answer, self.writer.clone(), self.driver.clone());
        let waiting = Rc::clone(&self.waiting);
        let open = move |_ctx: &mut EventContext<'_>| {
            // The message page pops with whether it was archived.
            let (Ok(result), Some(driver)) = (
                router.push_for_result::<bool>(AppRoute::Message { id: 1 }),
                &driver,
            ) else {
                return;
            };
            let writer = writer.clone();
            let task = driver.spawn_local(Box::pin(async move {
                let text = match result.await {
                    Ok(Some(true)) => "Archived",
                    _ => "Kept",
                };
                // Refused only once the inbox's UI runtime has closed.
                let _ = writer.write(|ctx| answer.set(ctx, text.to_owned()));
            }));
            *waiting.borrow_mut() = Some(task);
        };
        Scaffold::new()
            .app_bar(AppBar::new().title(Text::new("Inbox")))
            .body(Center::new().child(Column::new((
                Text::new(answer.get(ctx)),
                TextButton::new(Text::new("Open message 1")).on_pressed(open),
            ))))
    }
}

/// A message: archiving it asks first, in a dialog, then pops the page with
/// `true`. The app bar's back button pops it with no value.
#[derive(Clone, StatelessView)]
struct Message {
    id: u32,
}

impl StatelessView for Message {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let page = PageHandle::of(ctx).expect("BUG: a message is a page of the Router");
        let ask = move |_ctx: &mut EventContext<'_>| {
            let message = page.clone();
            let _dialog = show_dialog::<(), _, _>(&page, move |ctx| {
                let dialog = PageHandle::of(ctx).expect("BUG: the dialog is a route");
                let (keep, archive) = (dialog.clone(), message.clone());
                AlertDialog::new()
                    .title(Text::new("Archive this message?"))
                    .actions(vec![
                        TextButton::new(Text::new("Cancel"))
                            .on_pressed(move |_ctx| {
                                let _ = keep.pop();
                            })
                            .boxed(),
                        TextButton::new(Text::new("Archive"))
                            .on_pressed(move |_ctx| {
                                // The dialog leaves first, so the message is
                                // on top again when it pops with its result.
                                let _ = dialog.pop();
                                let _ = archive.pop_with(true);
                            })
                            .boxed(),
                    ])
            });
        };
        Scaffold::new()
            .app_bar(AppBar::new().title(Text::new(format!("Message {}", self.id))))
            .body(Center::new().child(TextButton::new(Text::new("Archive")).on_pressed(ask)))
    }
}
