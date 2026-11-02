//! Owner-local input profile controls for interactive native qualification.
#![allow(
    unsafe_code,
    reason = "owner-local COM qualification calls with checked STA lifetime"
)]
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    CLSID_TF_InputProcessorProfiles, GUID_TFCAT_TIP_KEYBOARD, ITfInputProcessorProfileMgr,
    TF_INPUTPROCESSORPROFILE, TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE, TF_PROFILETYPE_INPUTPROCESSOR,
};
use windows_core::GUID;

mod tsf;
pub use tsf::{
    NativeTextGrant, NativeTextGrantError, NativeTextGrantRequest, request_focused_text_through_tsf,
};

const CLASS: GUID = GUID::from_u128(0x03b5835f_f03c_411b_9ce2_aa23e1171e36);
const PROFILE: GUID = GUID::from_u128(0xa76c93d9_5523_4e90_aafa_4db112f9ac76);

/// CPU charged to the calling thread, excluding time waiting for scheduling or GPU.
/// Windows reports kernel plus user FILETIME units of 100 ns; timer resolution
/// can be coarser than that unit and must be retained in measurement tolerances.
pub fn current_thread_cpu_time() -> Result<std::time::Duration, String> {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: the pseudo-handle denotes this live thread; all output buffers
    // live through the synchronous query, with no ownership transfer.
    unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &raw mut created,
            &raw mut exited,
            &raw mut kernel,
            &raw mut user,
        )
    }
    .map_err(|error| error.to_string())?;
    let ticks =
        |value: FILETIME| (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime);
    let nanos = ticks(kernel)
        .checked_add(ticks(user))
        .and_then(|total| total.checked_mul(100))
        .ok_or_else(|| "thread CPU time exceeded the duration representation".to_owned())?;
    Ok(std::time::Duration::from_nanos(nanos))
}

/// Geometry observed directly from a qualification HWND, in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeWindowGeometry {
    /// Whether Win32 currently reports the window iconic.
    pub minimized: bool,
    /// Outer screen rectangle: left, top, right, bottom.
    pub outer: [i32; 4],
    /// Client width and height.
    pub client: (i32, i32),
}

/// Query OS geometry without changing focus, placement or the message queue.
pub fn window_geometry(window: flui_platform::WindowId) -> Result<NativeWindowGeometry, String> {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect, IsIconic};
    let hwnd = HWND(window.0 as *mut _);
    let (mut outer, mut client) = (RECT::default(), RECT::default());
    // SAFETY: both writable rectangles live through the calls. Win32 validates
    // the opaque HWND; this observer never dereferences native owner state.
    unsafe {
        GetWindowRect(hwnd, &raw mut outer).map_err(|error| error.to_string())?;
        GetClientRect(hwnd, &raw mut client).map_err(|error| error.to_string())?;
        Ok(NativeWindowGeometry {
            minimized: IsIconic(hwnd).as_bool(),
            outer: [outer.left, outer.top, outer.right, outer.bottom],
            client: (client.right - client.left, client.bottom - client.top),
        })
    }
}

/// Post Win32's restore system command to a qualification window.
/// This exercises the native system-command route while its UI frame is suspended.
pub fn request_window_restore(window: flui_platform::WindowId) -> Result<(), String> {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SC_RESTORE, WM_SYSCOMMAND};
    // SAFETY: a scalar system command has no borrowed payload. Win32 owns
    // queued delivery on the HWND's thread; no owner-local value crosses here.
    unsafe {
        PostMessageW(
            Some(HWND(window.0 as *mut _)),
            WM_SYSCOMMAND,
            WPARAM(SC_RESTORE as usize),
            LPARAM(0),
        )
    }
    .map_err(|error| error.to_string())
}

/// An edit provider observed by an independent UI Automation client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeEditField {
    /// Current text reported by UIA's value pattern.
    pub value: String,
    /// UIA's current keyboard-focus property, without a client-side cache.
    pub keyboard_focus: bool,
}

/// Read a live qualification window from a separate, non-owner thread.
/// UIA marshals calls to the provider; no COM interface crosses this API.
pub fn read_edit_fields(window: flui_platform::WindowId) -> Result<Vec<NativeEditField>, String> {
    fn read(window: flui_platform::WindowId) -> windows_core::Result<Vec<NativeEditField>> {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
        use windows::Win32::System::Variant::VARIANT;
        use windows::Win32::UI::Accessibility::{
            CUIAutomation, IUIAutomation, IUIAutomationValuePattern, TreeScope_Descendants,
            UIA_ControlTypePropertyId, UIA_EditControlTypeId, UIA_ValuePatternId,
        };
        use windows_core::Interface as _;
        struct Apartment;
        impl Drop for Apartment {
            fn drop(&mut self) {
                // SAFETY: this guard balances the successful initialization
                // on this thread, after every local UIA interface is dropped.
                unsafe { CoUninitialize() };
            }
        }
        // SAFETY: no borrowed pointer; the guard balances this MTA reference.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
        let _apartment = Apartment;
        // SAFETY: window is a native qualification receipt, not a transferred
        // COM interface. UIA marshals access; every returned interface stays
        // on this observer thread, and a closed HWND yields a typed COM error.
        unsafe {
            let client: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            let root = client.ElementFromHandle(HWND(window.0 as *mut _))?;
            let condition = client.CreatePropertyCondition(
                UIA_ControlTypePropertyId,
                &VARIANT::from(UIA_EditControlTypeId.0),
            )?;
            let fields = root.FindAll(TreeScope_Descendants, &condition)?;
            let count = fields.Length()?;
            if !(0..=64).contains(&count) {
                return Err(windows_core::Error::from_hresult(
                    windows::Win32::Foundation::E_FAIL,
                ));
            }
            let mut observations = Vec::with_capacity(count as usize);
            for index in 0..count {
                let field = fields.GetElement(index)?;
                let value: IUIAutomationValuePattern =
                    field.GetCurrentPattern(UIA_ValuePatternId)?.cast()?;
                observations.push(NativeEditField {
                    value: value.CurrentValue()?.to_string(),
                    keyboard_focus: field.CurrentHasKeyboardFocus()?.as_bool(),
                });
            }
            Ok(observations)
        }
    }
    read(window).map_err(|error| error.to_string())
}

/// Results from real UIA actions, collected on a non-owner observer thread.
#[derive(Clone, Debug)]
pub struct NativeEditActions {
    /// Value pattern after the edit reached the provider.
    pub value: String,
    /// Selected text read back through the text pattern.
    pub selected: String,
    /// Selection rectangles in native screen coordinates: x, y, width, height.
    pub bounds: Vec<[f64; 4]>,
    /// Focus read back after UIA's focus action.
    pub keyboard_focus: bool,
    /// Other editable providers, in their original provider order.
    pub other_values: Vec<String>,
}

/// Change one live editor through an independent UIA client, select its first
/// five characters and focus it. All COM interfaces remain on this caller.
/// The caller must be a non-owner thread; native qualification supplies the HWND.
pub fn exercise_edit_field(
    window: flui_platform::WindowId,
    field_index: usize,
    text: &str,
) -> Result<NativeEditActions, String> {
    use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, HWND};
    use windows::Win32::System::Com::{
        COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize, SAFEARRAY,
    };
    use windows::Win32::System::Ole::{
        SafeArrayDestroy, SafeArrayGetDim, SafeArrayGetElement, SafeArrayGetLBound,
        SafeArrayGetUBound, SafeArrayGetVartype,
    };
    use windows::Win32::System::Variant::{VARIANT, VT_R8};
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationTextPattern, IUIAutomationValuePattern,
        TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start, TextUnit_Character,
        TreeScope_Descendants, UIA_ControlTypePropertyId, UIA_EditControlTypeId, UIA_TextPatternId,
        UIA_ValuePatternId,
    };
    use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindowVisible};
    use windows_core::{BSTR, Interface as _};
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            // SAFETY: created only after successful initialization on this
            // thread; every local interface is dropped before this guard.
            unsafe { CoUninitialize() };
        }
    }
    struct Rectangles(*mut SAFEARRAY);
    impl Drop for Rectangles {
        fn drop(&mut self) {
            // SAFETY: owns the non-null array returned by GetBoundingRectangles;
            // no array access or borrowed data survives this guard.
            let _ = unsafe { SafeArrayDestroy(self.0) };
        }
    }
    fn wait(mut observes: impl FnMut() -> windows_core::Result<bool>) -> windows_core::Result<()> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        loop {
            if observes()? {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(windows_core::Error::from_hresult(E_FAIL));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    let execute = || -> windows_core::Result<NativeEditActions> {
        if text.chars().count() < 5 || text.encode_utf16().count() > 4096 || field_index >= 64 {
            return Err(windows_core::Error::from_hresult(E_INVALIDARG));
        }
        // SAFETY: balances the successful MTA initialization on this thread.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
        let _apartment = Apartment;
        // SAFETY: the caller supplies a live native qualification receipt. UIA
        // marshals provider calls; interfaces and their inputs stay on this MTA.
        // SAFEARRAY shape and element type are checked before copying doubles.
        unsafe {
            let hwnd = HWND(window.0 as *mut _);
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                return Err(windows_core::Error::from_hresult(E_FAIL));
            }
            let client: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            let root = client.ElementFromHandle(hwnd)?;
            let condition = client.CreatePropertyCondition(
                UIA_ControlTypePropertyId,
                &VARIANT::from(UIA_EditControlTypeId.0),
            )?;
            let fields = root.FindAll(TreeScope_Descendants, &condition)?;
            let count = fields.Length()?;
            if !(1..=64).contains(&count) || field_index >= count as usize {
                return Err(windows_core::Error::from_hresult(E_INVALIDARG));
            }
            let field = fields.GetElement(field_index as i32)?;
            let value: IUIAutomationValuePattern =
                field.GetCurrentPattern(UIA_ValuePatternId)?.cast()?;
            value.SetValue(&BSTR::from(text))?;
            wait(|| Ok(value.CurrentValue()? == text))?;
            let pattern: IUIAutomationTextPattern =
                field.GetCurrentPattern(UIA_TextPatternId)?.cast()?;
            let range = pattern.DocumentRange()?;
            if range.GetText(4096)? != text {
                return Err(windows_core::Error::from_hresult(E_FAIL));
            }
            range.MoveEndpointByRange(
                TextPatternRangeEndpoint_End,
                &range,
                TextPatternRangeEndpoint_Start,
            )?;
            if range.MoveEndpointByUnit(TextPatternRangeEndpoint_End, TextUnit_Character, 5)? != 5 {
                return Err(windows_core::Error::from_hresult(E_FAIL));
            }
            range.Select()?;
            let expected: String = text.chars().take(5).collect();
            wait(|| {
                let selected = pattern.GetSelection()?;
                Ok(selected.Length()? == 1 && selected.GetElement(0)?.GetText(4096)? == expected)
            })?;
            let selected = pattern.GetSelection()?.GetElement(0)?;
            let pointer = selected.GetBoundingRectangles()?;
            if pointer.is_null() {
                return Err(windows_core::Error::from_hresult(E_FAIL));
            }
            let rectangles = Rectangles(pointer);
            if SafeArrayGetDim(rectangles.0) != 1 || SafeArrayGetVartype(rectangles.0)? != VT_R8 {
                return Err(windows_core::Error::from_hresult(E_FAIL));
            }
            let lower = SafeArrayGetLBound(rectangles.0, 1)?;
            let upper = SafeArrayGetUBound(rectangles.0, 1)?;
            let length = i64::from(upper) - i64::from(lower) + 1;
            if !(4..=256).contains(&length) || length % 4 != 0 {
                return Err(windows_core::Error::from_hresult(E_FAIL));
            }
            let mut bounds = Vec::with_capacity(length as usize / 4);
            for offset in (0..length).step_by(4) {
                let mut rectangle = [0.0_f64; 4];
                for (component, number) in rectangle.iter_mut().enumerate() {
                    let index = (i64::from(lower) + offset + component as i64) as i32;
                    SafeArrayGetElement(
                        rectangles.0,
                        &raw const index,
                        std::ptr::from_mut(number).cast(),
                    )?;
                }
                if !rectangle.iter().all(|number| number.is_finite())
                    || rectangle[2] <= 0.0
                    || rectangle[3] <= 0.0
                {
                    return Err(windows_core::Error::from_hresult(E_FAIL));
                }
                bounds.push(rectangle);
            }
            field.SetFocus()?;
            wait(|| Ok(field.CurrentHasKeyboardFocus()?.as_bool()))?;
            let mut other_values = Vec::with_capacity(count as usize - 1);
            for index in 0..count {
                if index == field_index as i32 {
                    continue;
                }
                let other: IUIAutomationValuePattern = fields
                    .GetElement(index)?
                    .GetCurrentPattern(UIA_ValuePatternId)?
                    .cast()?;
                other_values.push(other.CurrentValue()?.to_string());
            }
            Ok(NativeEditActions {
                value: value.CurrentValue()?.to_string(),
                selected: selected.GetText(4096)?.to_string(),
                bounds,
                keyboard_focus: field.CurrentHasKeyboardFocus()?.as_bool(),
                other_values,
            })
        }
    };
    execute().map_err(|error| error.to_string())
}

/// Whether the calling thread still owns a single-threaded COM apartment.
/// Queries only; an implicit process MTA does not retain a native UI owner.
pub fn current_thread_has_sta_apartment() -> bool {
    use windows::Win32::System::Com::{
        APTTYPE, APTTYPE_MAINSTA, APTTYPE_STA, APTTYPEQUALIFIER, CoGetApartmentType,
    };
    let (mut kind, mut qualifier) = (APTTYPE::default(), APTTYPEQUALIFIER::default());
    // SAFETY: both out-pointers refer to initialized, writable stack locals.
    let active = unsafe { CoGetApartmentType(&raw mut kind, &raw mut qualifier) }.is_ok();
    eprintln!(
        "native apartment observation: active={active} kind={kind:?} qualifier={qualifier:?}"
    );
    active && matches!(kind, APTTYPE_STA | APTTYPE_MAINSTA)
}

/// Failure to select and verify the real Microsoft Japanese input processor.
#[derive(Debug, thiserror::Error)]
#[error("Japanese IME qualification profile: {0}")]
pub struct JapaneseImeError(String);

/// Selects Japanese IME on the calling STA only, restoring its previous profile
/// on drop. Retain this on the owner until the interactive session ends.
pub struct JapaneseIme {
    profiles: ITfInputProcessorProfileMgr,
    previous: TF_INPUTPROCESSORPROFILE,
    local: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl std::fmt::Debug for JapaneseIme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JapaneseIme").finish_non_exhaustive()
    }
}
impl JapaneseIme {
    /// Open the calling thread's IME after its native window and text focus
    /// exist. Selecting a language profile alone does not enable composition.
    pub fn open_keyboard(&self) -> Result<(), JapaneseImeError> {
        use windows::Win32::System::Variant::VARIANT;
        use windows::Win32::UI::TextServices::{
            CLSID_TF_ThreadMgr, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE, ITfCompartmentMgr,
            ITfThreadMgr,
        };
        use windows_core::Interface as _;
        // SAFETY: this !Send guard belongs to the already initialized owner
        // STA. The temporary activation is balanced below on every result.
        let manager: ITfThreadMgr =
            unsafe { CoCreateInstance(&CLSID_TF_ThreadMgr, None, CLSCTX_INPROC_SERVER) }
                .map_err(|error| JapaneseImeError(error.to_string()))?;
        // SAFETY: manager is an STA-local COM interface.
        let client =
            unsafe { manager.Activate() }.map_err(|error| JapaneseImeError(error.to_string()))?;
        let opened = (|| -> windows_core::Result<()> {
            let compartments: ITfCompartmentMgr = manager.cast()?;
            // SAFETY: the GUID and initialized integer variant remain live
            // through synchronous calls to this STA's keyboard compartment.
            unsafe {
                let keyboard = compartments.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE)?;
                keyboard.SetValue(client, &VARIANT::from(1_i32))?;
                let value = keyboard.GetValue()?;
                if i32::try_from(&value)? != 1 {
                    return Err(windows_core::Error::from_hresult(
                        windows::Win32::Foundation::E_FAIL,
                    ));
                }
            }
            Ok(())
        })();
        // SAFETY: balances exactly the successful Activate above on this STA.
        let deactivated = unsafe { manager.Deactivate() };
        opened
            .and(deactivated)
            .map_err(|error| JapaneseImeError(error.to_string()))
    }

    /// Requires a bootstrapped Win32 owner STA and an installed Japanese IME.
    /// No process-wide or session-wide profile change is attempted.
    pub fn try_new() -> Result<Self, JapaneseImeError> {
        // SAFETY: the native owner initialized COM; all interfaces and pointers
        // remain local to this STA, enforced by the Rc marker.
        let profiles: ITfInputProcessorProfileMgr = unsafe {
            CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)
        }
        .map_err(|error| JapaneseImeError(error.to_string()))?;
        let mut previous = TF_INPUTPROCESSORPROFILE::default();
        // SAFETY: the live local out-pointer and interface belong to this STA.
        unsafe { profiles.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &raw mut previous) }
            .map_err(|error| JapaneseImeError(error.to_string()))?;
        let guard = Self {
            profiles,
            previous,
            local: std::marker::PhantomData,
        };
        // SAFETY: both GUIDs are live constants; neither FORPROCESS nor
        // FORSESSION is supplied, so only this STA's profile changes.
        unsafe {
            guard.profiles.ActivateProfile(
                TF_PROFILETYPE_INPUTPROCESSOR,
                0x0411,
                &CLASS,
                &PROFILE,
                HKL::default(),
                TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE,
            )
        }
        .map_err(|error| JapaneseImeError(error.to_string()))?;
        let mut active = TF_INPUTPROCESSORPROFILE::default();
        // SAFETY: the live local out-pointer and interface belong to this STA.
        unsafe {
            guard
                .profiles
                .GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &raw mut active)
        }
        .map_err(|error| JapaneseImeError(error.to_string()))?;
        if active.dwProfileType != TF_PROFILETYPE_INPUTPROCESSOR
            || active.langid != 0x0411
            || active.clsid != CLASS
            || active.guidProfile != PROFILE
        {
            return Err(JapaneseImeError(String::from(
                "active profile differs from requested Japanese TIP",
            )));
        }
        Ok(guard)
    }
}
impl Drop for JapaneseIme {
    fn drop(&mut self) {
        // SAFETY: the !Send guard cannot leave its creating STA. The saved
        // profile and both GUID pointers remain valid for this synchronous call.
        let restored = unsafe {
            self.profiles.ActivateProfile(
                self.previous.dwProfileType,
                self.previous.langid,
                &raw const self.previous.clsid,
                &raw const self.previous.guidProfile,
                self.previous.hkl,
                TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE,
            )
        };
        if let Err(error) = restored {
            tracing::warn!(%error, "could not restore qualification input profile");
        }
    }
}
