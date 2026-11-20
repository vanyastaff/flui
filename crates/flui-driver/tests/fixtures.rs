//! The apps the driver's tests drive: a counter, and a gallery of controls standing in for
//! `interaction_gallery` until it lands. Each carries the same row of faults, so every row of the
//! failure table runs on both: a button that ends the app, a button whose handler panics, a
//! button that replaces itself, and a surface that counts the contacts it saw pressed and
//! cancelled.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use flui_view::prelude::*;
use flui_view::{Signal, StatefulView, ViewState, WindowHandle};
use flui_widgets::prelude::HitTestBehavior;
use flui_widgets::{Column, Listener, RawButton, Semantics, SizedBox, Text};

/// What a fixture's handlers saw, read from the test's thread.
#[derive(Clone, Default)]
pub struct Probe {
    presses: Arc<AtomicU32>,
    downs: Arc<AtomicU32>,
    cancels: Arc<AtomicU32>,
    explosions: Arc<AtomicU32>,
}

impl Probe {
    /// Times the counter's button ran its handler.
    pub fn presses(&self) -> u32 {
        self.presses.load(Ordering::SeqCst)
    }

    /// Contacts the hold surface saw go down.
    pub fn downs(&self) -> u32 {
        self.downs.load(Ordering::SeqCst)
    }

    /// Contacts the hold surface saw cancelled.
    pub fn cancels(&self) -> u32 {
        self.cancels.load(Ordering::SeqCst)
    }

    /// Times the faulty button's handler ran (and panicked).
    pub fn explosions(&self) -> u32 {
        self.explosions.load(Ordering::SeqCst)
    }
}

/// The counter: `Count: n` and an `Increment` button, with the faults row.
#[derive(Clone, StatefulView)]
pub struct Counter {
    pub probe: Probe,
}

pub struct CounterState {
    count: Signal<u32>,
    faults: Faults,
}

impl StatefulView for Counter {
    type State = CounterState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        CounterState {
            count: ctx.signal(0),
            faults: Faults::new(ctx),
        }
    }
}

impl ViewState<Counter> for CounterState {
    fn build(&self, view: &Counter, ctx: &dyn BuildContext) -> impl IntoView {
        let count = self.count;
        let presses = Arc::clone(&view.probe.presses);
        Column::new((
            Text::new(format!("Count: {}", count.get(ctx))),
            RawButton::new(Text::new("Increment"))
                .identifier("increment")
                .on_pressed(move |ctx| {
                    presses.fetch_add(1, Ordering::SeqCst);
                    count.update(ctx, |n| *n += 1)
                }),
            self.faults.build(&view.probe, ctx),
        ))
    }
}

/// The gallery: a few controls of different kinds, with the faults row.
#[derive(Clone, StatefulView)]
pub struct Gallery {
    pub probe: Probe,
}

pub struct GalleryState {
    likes: Signal<u32>,
    shares: Signal<u32>,
    faults: Faults,
}

impl StatefulView for Gallery {
    type State = GalleryState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        GalleryState {
            likes: ctx.signal(0),
            shares: ctx.signal(0),
            faults: Faults::new(ctx),
        }
    }
}

impl ViewState<Gallery> for GalleryState {
    fn build(&self, view: &Gallery, ctx: &dyn BuildContext) -> impl IntoView {
        let likes = self.likes;
        let shares = self.shares;
        let presses = Arc::clone(&view.probe.presses);
        Column::new((
            Text::new("Gallery"),
            Text::new(format!("Count: {}", likes.get(ctx))),
            RawButton::new(Text::new("Like"))
                .identifier("increment")
                .on_pressed(move |ctx| {
                    presses.fetch_add(1, Ordering::SeqCst);
                    likes.update(ctx, |n| *n += 1)
                }),
            Text::new(format!("Shares: {}", shares.get(ctx))),
            RawButton::new(Text::new("Share"))
                .on_pressed(move |ctx| shares.update(ctx, |n| *n += 1)),
            RawButton::new(Text::new("Share"))
                .on_pressed(move |ctx| shares.update(ctx, |n| *n += 2)),
            self.faults.build(&view.probe, ctx),
        ))
    }
}

/// The faults row's state: whether `Replace me` replaced itself, and the window `Quit` closes.
pub struct Faults {
    replaced: Signal<bool>,
    window: Option<WindowHandle>,
}

impl Faults {
    fn new(ctx: &dyn LifecycleContext) -> Self {
        Self {
            replaced: ctx.signal(false),
            window: ctx.window(),
        }
    }

    /// The faults row: `Quit`, which closes the app's window; `Explode`, whose handler panics;
    /// `Replace me`, which replaces itself with `Replaced`; and `Hold`, a surface that counts the
    /// contacts pressed on it and cancelled.
    fn build(&self, probe: &Probe, ctx: &dyn BuildContext) -> BoxedView {
        faults(probe, self.replaced, self.window.clone(), ctx)
    }
}

fn faults(
    probe: &Probe,
    replaced: Signal<bool>,
    window: Option<WindowHandle>,
    ctx: &dyn BuildContext,
) -> BoxedView {
    let explosions = Arc::clone(&probe.explosions);
    let downs = Arc::clone(&probe.downs);
    let cancels = Arc::clone(&probe.cancels);
    // A view of another type, so the button's element is removed rather than updated in place.
    let replacing = if replaced.get(ctx) {
        Text::new("Replaced").boxed()
    } else {
        RawButton::new(Text::new("Replace me"))
            .on_pressed(move |ctx| replaced.set(ctx, true))
            .boxed()
    };
    Column::new((
        RawButton::new(Text::new("Quit")).on_pressed(move |_| {
            if let Some(window) = &window {
                let _ = window.request_close();
            }
        }),
        RawButton::new(Text::new("Explode")).on_pressed(move |_| -> () {
            explosions.fetch_add(1, Ordering::SeqCst);
            panic!("the Explode button's handler panics");
        }),
        replacing,
        Semantics::new().identifier("hold").label("Hold").child(
            Listener::new()
                .behavior(HitTestBehavior::Opaque)
                .on_pointer_down(move |_, _| {
                    downs.fetch_add(1, Ordering::SeqCst);
                })
                .on_pointer_cancel(move |_, _| {
                    cancels.fetch_add(1, Ordering::SeqCst);
                })
                .child(SizedBox::new(120.0, 40.0)),
        ),
    ))
    .boxed()
}
