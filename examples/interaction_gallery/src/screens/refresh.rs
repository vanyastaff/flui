//! A list under `RefreshIndicator`: a pull past its top, the Refresh
//! button, F5 or the row's assistive action fetch one more message.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use flui::prelude::*;
use flui::widgets::{
    Alternative, AnimatedBuilder, ControlSpec, GestureAlternatives, RefreshController,
    RefreshIndicator, RefreshIntent, row,
};

use super::{avatar, divider, list_row, page};
use crate::keyed::Keyed;
use crate::style;

/// How many frames a refresh takes: long enough to see the indicator spin.
const FETCH_FRAMES: u32 = 45;

#[derive(Clone, StatefulView)]
pub(crate) struct RefreshScreen;

pub(crate) struct RefreshState {
    /// How many messages the list holds; each refresh adds one at the top.
    count: StateCell<u32>,
    /// How many refreshes have completed.
    refreshes: StateCell<u32>,
    refresh: RefreshController,
}

impl StatefulView for RefreshScreen {
    type State = RefreshState;

    fn create_state(&self) -> Self::State {
        RefreshState {
            count: StateCell::new(6),
            refreshes: StateCell::new(0),
            refresh: RefreshController::new(),
        }
    }
}

impl ViewState<RefreshScreen> for RefreshState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.count.bind(ctx);
        self.refreshes.bind(ctx);
    }

    fn build(&self, _view: &RefreshScreen, _ctx: &dyn BuildContext) -> impl IntoView {
        let count = self.count.get();
        let rows: Vec<BoxedView> = (1..=count)
            .rev()
            .flat_map(|n| {
                [
                    Keyed::new(u64::from(n), message(n)).boxed(),
                    divider().into_view().boxed(),
                ]
            })
            .collect();

        let alternatives = GestureAlternatives::new(Alternative::VisibleControl(ControlSpec::new(
            RefreshIntent::command("Refresh inbox"),
            Padding::symmetric(12.0, 8.0).child(style::tinted("Refresh", style::ACCENT)),
        )))
        .expect("BUG: the refresh command is labelled");
        let (cells, refreshes) = (self.count.clone(), self.refreshes.clone());
        let list = RefreshIndicator::new(
            Scrollable::new()
                .semantic_label("Messages")
                .child(Column::new(rows).cross_axis_alignment(CrossAxisAlignment::Stretch)),
            alternatives,
        )
        .controller(self.refresh.clone())
        .on_refresh(move |_cx| {
            let (cells, refreshes) = (cells.clone(), refreshes.clone());
            async move {
                Frames(FETCH_FRAMES).await;
                cells.update(|n| n + 1);
                refreshes.update(|n| n + 1);
            }
        });

        let refresh = self.refresh.clone();
        let (count, done) = (self.count.get(), self.refreshes.get());
        let readout = AnimatedBuilder::new(self.refresh.listenable(), move || {
            let state = if refresh.is_refreshing() {
                "refreshing"
            } else {
                "idle"
            };
            style::readout(format!("{count} messages, {done} refreshes, {state}"))
        });

        page(
            "Pull to refresh",
            "Pull the list down past its top and let go. The Refresh button, F5 or Ctrl+R \
             while the list has focus do the same.",
            readout,
            list,
        )
    }
}

fn message(n: u32) -> impl IntoView {
    list_row(
        60.0,
        Row::new(row![
            avatar('M', style::ARCHIVE),
            SizedBox::width(12.0),
            style::tinted(format!("Message {n}"), style::INK),
        ]),
    )
}

/// Resolves after a number of frames: the network the gallery does not
/// have. It wakes itself each poll, and a wake schedules the next frame, so
/// it counts frames without a timer and runs alike in a browser.
struct Frames(u32);

impl Future for Frames {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 == 0 {
            return Poll::Ready(());
        }
        self.0 -= 1;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}
