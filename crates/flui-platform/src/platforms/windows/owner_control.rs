//! Dedicated message-only HWND: its userdata is never a visible WindowContext.
use crate::{
    PlatformError,
    shared::{owner_signal::OwnerSignal, panic_boundary::contain_owner_callback},
};
use parking_lot::Mutex;
use std::sync::{Arc, OnceLock};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW,
            HWND_MESSAGE, PostMessageW, PostQuitMessage, RegisterClassW, SetWindowLongPtrW,
            WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_NCDESTROY, WNDCLASSW,
        },
    },
    core::w,
};
const WAKE: u32 = WM_APP + 19;
static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();

pub(super) struct OwnerControl {
    pub(super) signal: Arc<OwnerSignal>,
    address: Arc<Mutex<Option<isize>>>,
}
impl OwnerControl {
    pub(super) fn new() -> Result<Self, PlatformError> {
        let registration = REGISTERED.get_or_init(|| {
            // SAFETY: class name lives forever; this exact procedure implements its ABI.
            unsafe {
                let instance = GetModuleHandleW(None).map_err(|error| error.to_string())?;
                let class = WNDCLASSW {
                    lpfnWndProc: Some(procedure),
                    hInstance: instance.into(),
                    lpszClassName: w!("FLUIOwnerSignalWindow"),
                    ..Default::default()
                };
                if RegisterClassW(&raw const class) == 0 {
                    Err(windows::core::Error::from_thread().to_string())
                } else {
                    Ok(())
                }
            }
        });
        registration
            .as_ref()
            .map_err(|message| PlatformError::Init {
                message: message.clone(),
            })?;
        let address = Arc::new(Mutex::new(None::<isize>));
        let target = Arc::clone(&address);
        let signal = OwnerSignal::new(Arc::new(move || {
            let address = target.lock();
            let raw = address.ok_or_else(|| PlatformError::EventLoop {
                message: "owner message window is closed".into(),
            })?;
            // SAFETY: admission is closed before this address is retired. Posting
            // carries no pointer or user closure and never synchronously invokes the procedure.
            unsafe { PostMessageW(Some(HWND(raw as *mut _)), WAKE, WPARAM(0), LPARAM(0)) }.map_err(
                |error| PlatformError::EventLoop {
                    message: error.to_string(),
                },
            )
        }));
        // SAFETY: dedicated registered class, owner-created HWND; userdata is one
        // strong Arc reference, released exactly once by WM_NCDESTROY.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None).map_err(|error| PlatformError::Init {
                message: error.to_string(),
            })?;
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("FLUIOwnerSignalWindow"),
                w!("FLUI owner"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance.into()),
                None,
            )
            .map_err(|error| PlatformError::Init {
                message: error.to_string(),
            })?;
            SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                Arc::into_raw(Arc::clone(&signal)) as isize,
            );
            hwnd
        };
        {
            let mut address_slot = address.lock();
            *address_slot = Some(hwnd.0 as isize);
        }
        Ok(Self { signal, address })
    }
    pub(super) fn close(&self) {
        self.signal.close();
        let address = self.address.lock().take();
        if let Some(address) = address {
            // SAFETY: native destruction occurs on the recorded owner, outside
            // the address/state locks. WM_NCDESTROY owns userdata reclamation.
            if self.signal.owner() == std::thread::current().id() {
                if let Err(error) = unsafe { DestroyWindow(HWND(address as *mut _)) } {
                    tracing::error!(%error, "owner message window destruction failed");
                }
            } else {
                tracing::error!("owner message window dropped off owner; native resource retained");
            }
        }
    }
}
impl Drop for OwnerControl {
    fn drop(&mut self) {
        self.close();
    }
}

// SAFETY: Win32 invokes this exact signature for the dedicated registered class.
unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let mut result = LRESULT(0);
    contain_owner_callback(|| {
        // SAFETY: only this class stores OwnerSignal userdata. Same-owner
        // dispatch pins another Arc before callbacks can enter a nested loop.
        unsafe {
            let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const OwnerSignal;
            if message == WM_NCDESTROY && !pointer.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Arc::from_raw(pointer));
            } else if message == WAKE && !pointer.is_null() {
                Arc::increment_strong_count(pointer);
                let signal = Arc::from_raw(pointer);
                if signal.drive() {
                    signal.close();
                    PostQuitMessage(0);
                }
                return;
            }
            result = DefWindowProcW(hwnd, message, wparam, lparam);
        }
    });
    result
}
