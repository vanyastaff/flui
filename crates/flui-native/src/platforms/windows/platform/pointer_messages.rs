//! The window's pointer messages: `WM_POINTER*`, device arrival and removal,
//! the mouse's hover on either route, and the end of every open contact when
//! the window loses focus or is minimised.

use flui_platform::{EventTime, pointer::PointerEvent};
use windows::Win32::{
    Foundation::{HANDLE, HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{
        PDC_ARRIVAL, PDC_REMOVAL, WM_POINTERCAPTURECHANGED, WM_POINTERLEAVE,
    },
};

use super::WindowContext;
use crate::platforms::windows::events;
use crate::shared::pointer_assembly::NativeCancel;
use crate::traits::PlatformInput;

impl WindowContext {
    /// Record whether the mouse is over the window, raising the hover change
    /// only when it changed. Every route the mouse reaches the window by
    /// reports through here: `WM_MOUSEMOVE`/`WM_MOUSELEAVE`, and `WM_POINTER*`
    /// for a mouse under `EnableMouseInPointer`.
    pub(super) fn set_hovered(&self, hovered: bool) {
        if self.is_hovered.replace(hovered) != hovered {
            self.callbacks.dispatch_hover_status_change(hovered);
        }
    }

    /// A `WM_POINTER*` `message` for the pointer `wparam` names.
    pub(super) fn pointer_message(&self, hwnd: HWND, message: u32, wparam: WPARAM) {
        let raw = (wparam.0 & 0xffff) as u32;
        let mouse = events::is_mouse_pointer(raw);
        // The mouse's hover follows the messages that carry it. A capture
        // change says nothing about where it is, and no message follows it
        // (a press dragged outside sends no leave while it is held), so the
        // hover is read from the cursor once its input is delivered.
        let hover =
            (mouse && message != WM_POINTERCAPTURECHANGED).then_some(message != WM_POINTERLEAVE);
        let inputs = events::native_pointer_input(
            hwnd,
            message,
            (raw, mouse),
            self.scale_factor.get(),
            &self.message_clock,
            &self.pointer_registry,
        );
        // The hover is settled before any of the message's input reaches user
        // code: a callback that pumps messages may see the cursor come back,
        // and nothing written after it may undo that.
        if let Some(hovered) = hover {
            self.set_hovered(hovered);
        }
        for input in inputs {
            let deliver = self.pointer_registry.borrow().delivers(&input);
            if deliver {
                self.callbacks.dispatch_input(input);
            }
        }
        if mouse && message == WM_POINTERCAPTURECHANGED {
            self.reconcile_hover(hwnd);
        }
    }

    /// The window lost the mouse capture: no message may say where the
    /// cursor went, so the hover is what the cursor is over now.
    fn reconcile_hover(&self, hwnd: HWND) {
        self.set_hovered(cursor_over_client(hwnd));
    }

    /// `WM_POINTERDEVICECHANGE`: a removed device ends its contacts and
    /// leaves; an arriving one is announced.
    pub(super) fn pointer_device_changed(&self, wparam: WPARAM, lparam: LPARAM) {
        let device = events::native_device(lparam);
        let time = EventTime::from_nanos(self.message_clock.message_time());
        let mut inputs = Vec::new();
        if wparam.0 == PDC_REMOVAL as usize {
            let (ended, removed) = self
                .pointer_registry
                .borrow_mut()
                .remove_device(device, time);
            inputs.extend(ended.into_iter().map(|(_, input)| input));
            inputs.extend(
                removed.map(|change| PlatformInput::Pointer(PointerEvent::DeviceRemoved(change))),
            );
        } else if wparam.0 == PDC_ARRIVAL as usize
            && let Some(kind) =
                events::native_device_kind(HANDLE(lparam.0 as *mut core::ffi::c_void))
        {
            let added = self
                .pointer_registry
                .borrow_mut()
                .add_device(device, kind, time);
            inputs.extend(
                added.map(|change| PlatformInput::Pointer(PointerEvent::DeviceAdded(change))),
            );
        }
        for input in inputs {
            self.callbacks.dispatch_input(input);
        }
    }

    /// End every open pointer sequence and touchpad manipulation as
    /// `FocusLost`: the window lost focus or was minimised, so the releases
    /// may never reach it. What still arrives for an ended contact is an
    /// orphan and delivers nothing.
    pub(super) fn cancel_pointers(&self, hwnd: HWND) {
        // The touchpad first: its manipulation ends through the registry.
        self.cancel_touchpad(hwnd);
        let time = EventTime::from_nanos(self.message_clock.message_time());
        let cancels = self.pointer_registry.borrow_mut().cancel_surface(
            events::SURFACE,
            time,
            NativeCancel::FocusLost,
        );
        for cancel in cancels {
            self.callbacks.dispatch_input(cancel);
        }
    }
}

/// Whether the cursor is over `hwnd`'s client area, the window on top there;
/// `false` when the cursor cannot be read.
fn cursor_over_client(hwnd: HWND) -> bool {
    use windows::Win32::{
        Foundation::{POINT, RECT},
        Graphics::Gdi::ScreenToClient,
        UI::WindowsAndMessaging::{GetClientRect, GetCursorPos, WindowFromPoint},
    };
    let mut cursor = POINT::default();
    let mut client = RECT::default();
    // SAFETY: every out pointer addresses a live local; `hwnd` is the window
    // whose procedure is running.
    unsafe {
        if GetCursorPos(&raw mut cursor).is_err() || WindowFromPoint(cursor) != hwnd {
            return false;
        }
        ScreenToClient(hwnd, &raw mut cursor).as_bool()
            && GetClientRect(hwnd, &raw mut client).is_ok()
            && cursor.x >= client.left
            && cursor.x < client.right
            && cursor.y >= client.top
            && cursor.y < client.bottom
    }
}
