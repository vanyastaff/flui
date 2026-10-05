//! A controlled horizontal numeric slider with pointer, keyboard and assistive input.

use crate::support::{ValueCallback, retirement::Terminal, value_callback};
use crate::{CustomPaint, Directionality, Focus, GestureDetector, LayoutBuilder, Semantics};
use flui_foundation::geometry::{Point, Rect, Size};
use flui_interaction::{
    events::{Key, KeyState, NamedKey},
    routing::{FocusNode, KeyEventResult},
};
use flui_painting::{Canvas, Paint, styling::Color, typography::TextDirection};
use flui_rendering::{
    delegates::CustomPainter, hit_testing::HitTestBehavior, semantics::NumericRange,
};
use flui_view::prelude::*;
use std::{
    any::Any,
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

/// A controlled horizontal slider. Input proposes a value through `on_changed`;
/// only a new range supplied by the parent changes its thumb or semantics value.
/// Missing `on_changed` disables input and focus. Keyboard/assistive increments
/// use the range's step; assistive setters preserve exact admitted values.
#[derive(StatefulView)]
pub struct Slider {
    range: NumericRange,
    direction: Option<TextDirection>,
    label: String,
    on_changed: Terminal<Option<ValueCallback<f64>>>,
}
impl Clone for Slider {
    fn clone(&self) -> Self {
        let on_changed = Terminal::new(self.on_changed.as_ref().map(Rc::clone));
        Self {
            range: self.range,
            direction: self.direction,
            label: self.label.clone(),
            on_changed,
        }
    }
}
impl std::fmt::Debug for Slider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slider")
            .field("range", &self.range)
            .field("direction", &self.direction)
            .field("label", &self.label)
            .field("enabled", &self.on_changed.is_some())
            .finish()
    }
}
impl Slider {
    /// Creates a disabled slider. Its immutable range validates value, bounds and step.
    #[must_use]
    pub fn new(range: NumericRange) -> Self {
        Self {
            range,
            direction: None,
            label: String::new(),
            on_changed: Terminal::new(None),
        }
    }
    /// Receives proposals inside the event's write context, without committing them.
    #[must_use]
    pub fn on_changed<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, f64) -> R + 'static,
        R: EventOutcome,
    {
        let callback = Terminal::new(callback);
        let incoming = Terminal::new(Some(value_callback(move |cx, value| callback(cx, value))));
        let old = std::mem::replace(&mut self.on_changed, incoming);
        drop(old);
        self
    }
    /// Overrides inherited direction. Without an override, depends on Directionality.
    #[must_use]
    pub fn text_direction(mut self, direction: TextDirection) -> Self {
        self.direction = Some(direction);
        self
    }
    /// Labels the numeric control for assistive technology.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }
}

struct Live {
    mounted: Cell<bool>,
    range: Cell<NumericRange>,
    direction: Cell<TextDirection>,
    size: Cell<Size>,
    down_x: Cell<Option<f64>>,
    callback: RefCell<Terminal<Option<ValueCallback<f64>>>>,
    focus: Terminal<Rc<FocusNode>>,
    writer: RefCell<Terminal<Option<WriterSource>>>,
}
impl Drop for Live {
    fn drop(&mut self) {
        // Final ownership can retire user captures even when normal disposal was
        // interrupted. Revoke authority and withdraw every independent owner
        // before running any destructor.
        self.mounted.set(false);
        self.down_x.set(None);
        let callback = self.callback.get_mut().withdraw();
        let focus = self.focus.withdraw();
        let writer = self.writer.get_mut().withdraw();
        drop((callback, focus, writer));
    }
}
impl Live {
    fn enabled(&self) -> bool {
        self.mounted.get() && self.callback.borrow().is_some()
    }
    fn usable(&self) -> bool {
        self.enabled() && self.range.get().min() < self.range.get().max()
    }
    fn pointer_usable(&self) -> bool {
        let size = self.size.get();
        self.usable()
            && size.width.is_finite()
            && size.width > 0.0
            && size.height.is_finite()
            && size.height > 0.0
    }
    fn admits_context(&self, cx: &EventCx<'_>) -> bool {
        self.writer
            .borrow()
            .as_ref()
            .is_some_and(|writer| writer.check_context(cx).is_ok())
    }
    fn propose(&self, cx: &mut EventCx<'_>, value: f64) {
        if !self.usable() || !self.admits_context(cx) || !self.range.get().contains(value) {
            return;
        }
        let callback = Terminal::new(self.callback.borrow().as_ref().map(Rc::clone));
        if let Some(callback) = callback.as_ref() {
            callback(cx, value);
        }
    }
    fn request_focus(&self, cx: &EventCx<'_>) {
        if self.usable() && self.admits_context(cx) {
            self.focus.request_focus();
        }
    }
    fn at_pointer(&self, cx: &mut EventCx<'_>, x: f64) {
        if !self.pointer_usable() || !self.admits_context(cx) || !x.is_finite() {
            return;
        }
        self.request_focus(cx);
        // Focus notification may update this control or close its focus manager.
        if !self.pointer_usable() || !self.admits_context(cx) || !self.focus.is_attached() {
            return;
        }
        let size = self.size.get();
        let radius = thumb_radius(size);
        let mut fraction = ((x - radius) / (size.width - radius * 2.0)).clamp(0.0, 1.0);
        if self.direction.get() == TextDirection::Rtl {
            fraction = 1.0 - fraction;
        }
        self.propose(cx, interpolate(self.range.get(), fraction));
    }
    fn stepped(&self, cx: &mut EventCx<'_>, increase: bool) {
        let range = self.range.get();
        let sum = if increase {
            range.value() + range.step()
        } else {
            range.value() - range.step()
        };
        // Finite addition can overflow, but its direction identifies the correct
        // endpoint. Saturation is explicit and never delivers a non-finite value.
        let value = if sum.is_finite() {
            sum.clamp(range.min(), range.max())
        } else if increase {
            range.max()
        } else {
            range.min()
        };
        self.propose(cx, value);
    }
}

/// Lifecycle state of a mounted Slider.
pub struct SliderState {
    live: Terminal<Rc<Live>>,
    rebuild: Option<RebuildHandle>,
}
impl std::fmt::Debug for SliderState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliderState")
            .field("mounted", &self.live.mounted.get())
            .finish_non_exhaustive()
    }
}
impl StatefulView for Slider {
    type State = SliderState;
    fn create_state(&self) -> SliderState {
        let callback = Terminal::new(self.on_changed.as_ref().map(Rc::clone));
        SliderState {
            live: Terminal::new(Rc::new(Live {
                mounted: Cell::new(true),
                range: Cell::new(self.range),
                direction: Cell::new(TextDirection::Ltr),
                size: Cell::new(Size::ZERO),
                down_x: Cell::new(None),
                callback: RefCell::new(callback),
                focus: Terminal::new(FocusNode::with_debug_label("Slider")),
                writer: RefCell::new(Terminal::new(None)),
            })),
            rebuild: None,
        }
    }
}
impl ViewState<Slider> for SliderState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.rebuild = Some(ctx.rebuild_handle());
        *self.live.writer.borrow_mut() = Terminal::new(Some(ctx.writer_source()));
    }
    fn did_update_view(&mut self, _old: &Slider, view: &Slider) {
        let incoming = Terminal::new(view.on_changed.as_ref().map(Rc::clone));
        self.live.range.set(view.range);
        if view.on_changed.is_none() || view.range.min() == view.range.max() {
            self.live.down_x.set(None);
        }
        let outgoing = std::mem::replace(&mut *self.live.callback.borrow_mut(), incoming);
        drop(outgoing);
    }
    fn dispose(&mut self) {
        self.live.mounted.set(false);
        self.live.down_x.set(None);
        let outgoing = self.live.callback.borrow_mut().withdraw();
        let writer = self.live.writer.borrow_mut().withdraw();
        drop(outgoing);
        drop(writer);
    }
    fn build(&self, view: &Slider, ctx: &dyn BuildContext) -> impl IntoView {
        let direction = view
            .direction
            .unwrap_or_else(|| Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr));
        self.live.direction.set(direction);
        let live = Terminal::new(Rc::clone(&self.live));
        let label = view.label.clone();
        let layout = LayoutBuilder::new(move |_ctx, constraints| {
            let width = if constraints.max_width.is_finite() {
                constraints.max_width
            } else {
                160.0
            };
            let height = if constraints.max_height.is_finite() {
                constraints.max_height
            } else {
                32.0
            };
            let size = constraints.constrain(Size::new(width, height));
            live.size.set(size);
            let range = live.range.get();
            let enabled = live.usable();
            let painter = SliderPainter {
                fraction: fraction(range),
                direction: live.direction.get(),
                enabled,
                focused: live.focus.has_focus(),
            };
            let paint = CustomPaint::new().size(size).painter(Arc::new(painter));
            let child = if enabled {
                let detector = GestureDetector::new()
                    .behavior(HitTestBehavior::Opaque)
                    .child(paint);
                let down = Terminal::new(Rc::clone(&live));
                let tap = Terminal::new(Rc::clone(&live));
                let start = Terminal::new(Rc::clone(&live));
                let update = Terminal::new(Rc::clone(&live));
                let end = Terminal::new(Rc::clone(&live));
                detector
                    .on_horizontal_drag_down(move |_cx, details| {
                        down.down_x.set(Some(details.local_position.dx));
                    })
                    .on_tap(move |cx| {
                        if let Some(x) = tap.down_x.take() {
                            tap.at_pointer(cx, x);
                        }
                    })
                    .on_horizontal_drag_start(move |cx, details| {
                        start.at_pointer(cx, details.local_position.dx);
                    })
                    .on_horizontal_drag_update(move |cx, details| {
                        update.at_pointer(cx, details.local_position.dx);
                    })
                    .on_horizontal_drag_end(move |_cx, _details| end.down_x.set(None))
                    .boxed()
            } else {
                // Disablement retires the recognizers and any pending contact;
                // re-enablement mounts a fresh arena participant.
                paint.boxed()
            };
            let mut semantics = Semantics::new()
                .numeric_range(range)
                .label(label.clone())
                .enabled(enabled)
                .text_direction(match live.direction.get() {
                    TextDirection::Ltr => crate::SemanticsTextDirection::Ltr,
                    TextDirection::Rtl => crate::SemanticsTextDirection::Rtl,
                });
            if enabled {
                let increase = Terminal::new(Rc::clone(&live));
                let decrease = Terminal::new(Rc::clone(&live));
                let set = Terminal::new(Rc::clone(&live));
                semantics = semantics
                    .on_increase(move |cx| increase.stepped(cx, true))
                    .on_decrease(move |cx| decrease.stepped(cx, false))
                    .on_set_numeric_value(move |cx, value| set.propose(cx, value));
            }
            semantics.child(child)
        });
        let key = Terminal::new(Rc::clone(&self.live));
        let focus_live = Terminal::new(Rc::clone(&self.live));
        let rebuild = Terminal::new(
            self.rebuild
                .clone()
                .expect("BUG: Slider built before initialization"),
        );
        Focus::new(layout)
            .focus_node(Rc::clone(&self.live.focus))
            .can_request_focus(self.live.usable())
            .skip_traversal(!self.live.usable())
            .on_focus_change(move |_cx, _focused| {
                if focus_live.mounted.get() {
                    rebuild.schedule(RebuildReason::StateChange);
                }
            })
            .on_key_event(move |cx, event| {
                if event.state != KeyState::Down
                    || !event.modifiers.is_empty()
                    || !key.usable()
                    || !key.admits_context(cx)
                {
                    return KeyEventResult::Ignored;
                }
                match event.key {
                    Key::Named(NamedKey::ArrowRight) => {
                        key.stepped(cx, key.direction.get() != TextDirection::Rtl);
                    }
                    Key::Named(NamedKey::ArrowLeft) => {
                        key.stepped(cx, key.direction.get() == TextDirection::Rtl);
                    }
                    Key::Named(NamedKey::ArrowUp) => key.stepped(cx, true),
                    Key::Named(NamedKey::ArrowDown) => key.stepped(cx, false),
                    Key::Named(NamedKey::Home) => key.propose(cx, key.range.get().min()),
                    Key::Named(NamedKey::End) => key.propose(cx, key.range.get().max()),
                    _ => return KeyEventResult::Ignored,
                }
                KeyEventResult::Handled
            })
    }
}

fn fraction(range: NumericRange) -> f64 {
    if range.min() == range.max() {
        return 0.0;
    }
    let span = range.max() - range.min();
    if span.is_finite() {
        (range.value() - range.min()) / span
    } else {
        let scale = range.min().abs().max(range.max().abs());
        (range.value() / scale - range.min() / scale) / (range.max() / scale - range.min() / scale)
    }
}
fn interpolate(range: NumericRange, fraction: f64) -> f64 {
    if fraction <= 0.0 {
        return range.min();
    }
    if fraction >= 1.0 {
        return range.max();
    }
    let scale = range.min().abs().max(range.max().abs());
    if scale == 0.0 {
        return 0.0;
    }
    let normalized = ((1.0 - fraction) * (range.min() / scale) + fraction * (range.max() / scale))
        .clamp(-1.0, 1.0);
    (normalized * scale).clamp(range.min(), range.max())
}
fn thumb_radius(size: Size) -> f64 {
    8.0_f64.min(size.height / 2.0).min(size.width / 4.0)
}
#[derive(Debug, PartialEq)]
struct SliderPainter {
    fraction: f64,
    direction: TextDirection,
    enabled: bool,
    focused: bool,
}
impl CustomPainter for SliderPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        if !size.width.is_finite()
            || !size.height.is_finite()
            || size.width <= 0.0
            || size.height <= 0.0
        {
            return;
        }
        canvas.save();
        canvas.clip_rect(Rect::from_ltwh(0.0, 0.0, size.width, size.height));
        let radius = thumb_radius(size);
        let half_track = 2.0_f64.min(size.height / 2.0);
        let fraction = if self.direction == TextDirection::Rtl {
            1.0 - self.fraction
        } else {
            self.fraction
        };
        let x = radius + (size.width - radius * 2.0) * fraction;
        let center = Point::new(x, size.height / 2.0);
        let active = if self.enabled {
            Color::rgb(38, 102, 192)
        } else {
            Color::rgb(128, 128, 128)
        };
        canvas.draw_rect(
            Rect::from_ltwh(
                radius,
                size.height / 2.0 - half_track,
                size.width - radius * 2.0,
                half_track * 2.0,
            ),
            &Paint::fill(Color::rgb(180, 180, 180)),
        );
        canvas.draw_circle(center, radius, &Paint::fill(active));
        if self.focused {
            canvas.draw_circle(center, radius + 2.0, &Paint::stroke(active, 1.0));
        }
        canvas.restore();
    }
    fn should_repaint(&self, old: &dyn CustomPainter) -> bool {
        old.as_any().downcast_ref::<Self>() != Some(self)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
