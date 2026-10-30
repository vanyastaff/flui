//! An inline closure with no annotations, handed to a framework-dispatched
//! event setter, writes a signal through the context the framework passes,
//! capturing nothing but the `Copy` signal (ADR-0086 §6). One setter per
//! family of the census, each in both return forms: the write's result and
//! `()`. A setter that stops inferring `cx` stops this consumer compiling.

use flui::cupertino::CupertinoButton;
use flui::interaction::KeyEventResult;
use flui::material::ElevatedButton;
use flui::view::{Signal, SignalWriteExt as _};
use flui::widgets::{
    AnimatedSize, Dismissible, Draggable, EditableText, Focus, Form, GestureDetector,
    InteractiveViewer, Listener, MouseRegion, PageView, PopScope, RawButton,
    RawTextField, RefreshIndicator, Semantics,
};

fn gestures(detector: GestureDetector, count: Signal<u32>) -> GestureDetector {
    detector
        .on_tap(move |cx| count.set(cx, 1))
        .on_long_press(move |cx| {
            let _ = count.set(cx, 2);
        })
}

fn pointers(listener: Listener, region: MouseRegion, count: Signal<u32>) -> (Listener, MouseRegion) {
    (
        listener.on_pointer_down(move |cx, _| count.set(cx, 1)),
        region.on_enter(move |cx, _, _| {
            let _ = count.set(cx, 2);
        }),
    )
}

fn focus(focus: Focus, count: Signal<u32>) -> Focus {
    focus
        .on_focus_change(move |cx, focused| count.set(cx, u32::from(focused)))
        .on_key_event(move |cx, _| {
            let _ = count.set(cx, 3);
            KeyEventResult::Handled
        })
}

fn dragging(
    draggable: Draggable,
    dismissible: Dismissible,
    viewer: InteractiveViewer,
    count: Signal<u32>,
) -> (Draggable, Dismissible, InteractiveViewer) {
    (
        draggable.on_drag_started(move |cx| count.set(cx, 1)),
        dismissible.on_dismissed(move |cx, _| {
            let _ = count.set(cx, 2);
        }),
        viewer.on_interaction_start(move |cx, _| count.set(cx, 3)),
    )
}

fn navigation(
    pop: PopScope,
    pages: PageView,
    refresh: RefreshIndicator,
    count: Signal<u32>,
) -> (PopScope, PageView, RefreshIndicator) {
    (
        pop.on_pop_invoked(move |cx, popped| count.set(cx, u32::from(popped))),
        pages.on_page_changed(move |cx, page| {
            let _ = count.set(cx, u32::try_from(page).unwrap_or(u32::MAX));
        }),
        refresh.on_refresh(move |cx| {
            let _ = count.set(cx, 1);
            async {}
        }),
    )
}

fn semantics(node: Semantics, count: Signal<u32>) -> Semantics {
    node.on_click(move |cx| count.set(cx, 1))
}

fn text(
    editable: EditableText,
    field: RawTextField,
    form: Form,
    count: Signal<u32>,
) -> (EditableText, RawTextField, Form) {
    (
        editable
            .on_changed(move |cx, value| count.set(cx, u32::try_from(value.len()).unwrap_or(u32::MAX)))
            .on_submitted(move |cx, _| {
                let _ = count.set(cx, 0);
            }),
        field.on_changed(move |cx, _| count.set(cx, 1)),
        form.on_changed(move |cx| {
            let _ = count.set(cx, 2);
        }),
    )
}

fn buttons(
    raw: RawButton,
    material: ElevatedButton,
    cupertino: CupertinoButton,
    count: Signal<u32>,
) -> (RawButton, ElevatedButton, CupertinoButton) {
    (
        raw.on_press(move |cx| count.set(cx, 1)),
        material.on_pressed(move |cx| {
            let _ = count.set(cx, 2);
        }),
        cupertino.on_pressed(move |cx| count.set(cx, 3)),
    )
}

fn animation(size: AnimatedSize, count: Signal<u32>) -> AnimatedSize {
    size.on_end(move |cx| count.set(cx, 1))
}

#[test]
fn event_setters_infer_cx_through_the_facade() {
    // Compiling the setters above is the check.
    let _ = (gestures, pointers, focus, dragging, navigation, semantics, text, buttons, animation);
}
