//! The apps the scenarios drive, on the headless target here and, on Windows, in a window of
//! their own (`examples/scenario_app.rs`, which includes this file): the same tree on both, so a
//! scenario written once checks both. Every interactive node carries an ADR-0240 identifier,
//! which the scenarios select by.
//!
//! - [`Controls`], the gallery: a button and its count, a switch, a text field and a pad that
//!   takes any pointer;
//! - [`Notes`]: a note field and its length, for keys, chords, text and composition;
//! - [`Roles`]: one node of each role of the Windows lane's role table, for the projection.

use flui_view::prelude::*;
use flui_view::{Signal, StatefulView, ViewState};
use flui_widgets::prelude::HitTestBehavior;
use flui_widgets::{
    Column, Listener, RawButton, RawTextField, Semantics, SizedBox, Text, TextEditingController,
};

use flui_driver::lane::windows::roles::ROLES;

/// The gallery: see the [module docs](self).
#[derive(Clone, StatefulView)]
pub struct Controls;

pub struct ControlsState {
    likes: Signal<u32>,
    subscribed: Signal<bool>,
    presses: Signal<u32>,
    comment: TextEditingController,
}

impl StatefulView for Controls {
    type State = ControlsState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        ControlsState {
            likes: ctx.signal(0),
            subscribed: ctx.signal(false),
            presses: ctx.signal(0),
            comment: TextEditingController::new(),
        }
    }
}

impl ViewState<Controls> for ControlsState {
    fn build(&self, _view: &Controls, ctx: &dyn BuildContext) -> impl IntoView {
        let likes = self.likes;
        let subscribed = self.subscribed;
        let presses = self.presses;
        let on = subscribed.get(ctx);
        Column::new((
            Text::new("Gallery"),
            RawButton::new(Text::new("Like"))
                .identifier("like")
                .on_pressed(move |ctx| likes.update(ctx, |n| *n += 1)),
            Semantics::new()
                .identifier("likes")
                .label(format!("Likes: {}", likes.get(ctx))),
            Semantics::new()
                .identifier("subscribe")
                .label("Subscribe")
                .switch(on)
                .on_click(move |ctx| subscribed.update(ctx, |on| *on = !*on))
                .child(
                    Listener::new()
                        .behavior(HitTestBehavior::Opaque)
                        .on_pointer_up(move |ctx, _| subscribed.update(ctx, |on| *on = !*on))
                        .child(SizedBox::new(160.0, 40.0)),
                ),
            RawTextField::new(self.comment.clone()).identifier("comment"),
            Semantics::new()
                .identifier("pad")
                .label(format!("Pad: {}", presses.get(ctx)))
                .child(
                    Listener::new()
                        .behavior(HitTestBehavior::Opaque)
                        .on_pointer_down(move |ctx, _| presses.update(ctx, |n| *n += 1))
                        .child(SizedBox::new(240.0, 120.0)),
                ),
        ))
    }
}

/// The note field and its length: see the [module docs](self).
#[derive(Clone, StatefulView)]
pub struct Notes;

pub struct NotesState {
    note: TextEditingController,
    length: Signal<usize>,
}

impl StatefulView for Notes {
    type State = NotesState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        NotesState {
            note: TextEditingController::new(),
            length: ctx.signal(0),
        }
    }
}

impl ViewState<Notes> for NotesState {
    fn build(&self, _view: &Notes, ctx: &dyn BuildContext) -> impl IntoView {
        let length = self.length;
        Column::new((
            Text::new("Notes"),
            RawTextField::new(self.note.clone())
                .identifier("note")
                .on_changed(move |ctx, text: &str| length.set(ctx, text.chars().count())),
            Semantics::new()
                .identifier("length")
                .label(format!("Length: {}", length.get(ctx))),
        ))
    }
}

/// One node of each role the Windows lane's role table lists, named after its role.
#[derive(Clone, StatelessView)]
pub struct Roles;

impl StatelessView for Roles {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Column::new(
            ROLES
                .iter()
                .map(|row| {
                    Semantics::new()
                        .role(row.role)
                        .label(role_label(row.role))
                        .child(SizedBox::new(40.0, 4.0))
                        .boxed()
                })
                .collect::<Vec<_>>(),
        )
    }
}

/// The label of the [`Roles`] node of `role`.
pub fn role_label(role: flui_driver::protocol::semantics::SemanticsRole) -> String {
    format!("{} fixture", role.name())
}
