//! A `GestureDetector` playground that counts each gesture it recognises —
//! tap, double tap, long press, pan and a two-finger scale — and tints the
//! pad with the last one.

use flui::interaction::{ScaleEndDetails, ScaleUpdateDetails};
use flui::prelude::*;
use flui::widgets::{TransformSlot, column};

use super::page;
use crate::style;

/// A gesture the pad recognised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Gesture {
    Tap,
    DoubleTap,
    LongPress,
    Pan,
    Scale,
}

impl Gesture {
    const fn name(self) -> &'static str {
        match self {
            Self::Tap => "tap",
            Self::DoubleTap => "double tap",
            Self::LongPress => "long press",
            Self::Pan => "pan",
            Self::Scale => "scale",
        }
    }

    /// The pad's tint after this gesture, so the last one shows at a glance.
    pub(crate) const fn tint(self) -> Color {
        match self {
            Self::Tap => Color::rgb(204, 232, 214),
            Self::DoubleTap => Color::rgb(252, 228, 196),
            Self::LongPress => Color::rgb(246, 214, 214),
            Self::Pan => Color::rgb(226, 214, 246),
            Self::Scale => Color::rgb(204, 236, 240),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Counts {
    taps: u32,
    double_taps: u32,
    long_presses: u32,
    pans: u32,
    scales: u32,
    last: Option<Gesture>,
    /// The last pan's travel, in logical pixels.
    pan_distance: f64,
    /// The last scale's factor.
    scale: f64,
}

impl Counts {
    fn saw(&mut self, gesture: Gesture) {
        self.last = Some(gesture);
        match gesture {
            Gesture::Tap => self.taps += 1,
            Gesture::DoubleTap => self.double_taps += 1,
            Gesture::LongPress => self.long_presses += 1,
            Gesture::Pan => self.pans += 1,
            Gesture::Scale => self.scales += 1,
        }
    }
}

#[derive(Clone, StatefulView)]
pub(crate) struct GesturesScreen;

pub(crate) struct GesturesState {
    counts: Signal<Counts>,
}

impl StatefulView for GesturesScreen {
    type State = GesturesState;

    fn create_state(&self) -> Self::State {
        GesturesState {
            counts: Signal::default(),
        }
    }
}

impl ViewState<GesturesScreen> for GesturesState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.counts = ctx.signal(Counts {
            scale: 1.0,
            ..Counts::default()
        });
    }

    fn build(&self, _view: &GesturesScreen, ctx: &dyn BuildContext) -> impl IntoView {
        let counts = self.counts;
        let now = counts.get(ctx);
        let readout = format!(
            "Taps {}, double taps {}, long presses {}, pans {}, scales {}; last: {}",
            now.taps,
            now.double_taps,
            now.long_presses,
            now.pans,
            now.scales,
            now.last.map_or("none", Gesture::name),
        );
        let detail = format!(
            "Last pan {} px, last scale {:.2}x",
            now.pan_distance.round(),
            now.scale
        );
        let saw = move |gesture| move |cx: &mut EventCx<'_>| counts.update(cx, |c| c.saw(gesture));
        let pad = GestureDetector::new()
            .behavior(HitTestBehavior::Opaque)
            .on_tap(saw(Gesture::Tap))
            .on_double_tap(saw(Gesture::DoubleTap))
            .on_long_press(saw(Gesture::LongPress))
            .drag(
                DragSlot::pan()
                    .on_start(move |cx, _details: DragStartDetails| {
                        counts.update(cx, |c| c.pan_distance = 0.0)
                    })
                    .on_update(move |cx, details: DragUpdateDetails| {
                        counts.update(cx, |c| {
                            c.pan_distance += details.delta.dx.hypot(details.delta.dy);
                        })
                    })
                    .on_end(move |cx, _details: DragEndDetails| {
                        counts.update(cx, |c| c.saw(Gesture::Pan))
                    }),
            )
            .transform(
                TransformSlot::new()
                    .on_update(move |cx, details: ScaleUpdateDetails| {
                        counts.update(cx, |c| c.scale = details.scale)
                    })
                    .on_end(move |cx, _details: ScaleEndDetails| {
                        counts.update(cx, |c| c.saw(Gesture::Scale))
                    }),
            )
            .child(
                Container::new()
                    .decoration(style::rounded(
                        now.last.map_or(style::ACCENT_SOFT, Gesture::tint),
                        16.0,
                    ))
                    .alignment(Alignment::CENTER)
                    .child(
                        Column::new(column![
                            style::strong("Gesture pad"),
                            SizedBox::height(6.0),
                            style::caption(detail),
                        ])
                        .main_axis_size(MainAxisSize::Min),
                    ),
            );
        page(
            "Gestures",
            "Tap, double-tap, long-press or drag on the pad, or pinch it with two fingers. The \
             pad takes the colour of the last gesture.",
            style::readout(readout),
            pad,
        )
    }
}
