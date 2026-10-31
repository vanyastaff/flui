//! The Android API level, and the NDK functions that exist only from a given
//! level on.
//!
//! The backend supports `minSdk` 23, but some of the functions it wants
//! (`ANativeWindow_setFrameRate`, `AChoreographer_registerRefreshRateCallback`:
//! API 30) are missing from older systems, and a binary that links one
//! directly does not even load there. Such a function is therefore looked up
//! by name when the platform is built, and only when the system is new enough;
//! what the lookup returns is an [`NdkFn`], a typed handle that exists only if
//! the function does. A caller holds the handle or it has nothing to call, so
//! there is no bare `api >= 30` test to forget.
//!
//! The level is read once, from the NDK or the system property, and kept in
//! the platform (not in a `static`).
//!
//! `android_get_device_api_level` is not in the `libc.so` of an API 23 device:
//! the NDK header defines it as an inline function below API 29, and the
//! library exports it from API 29. It is looked up like the other symbols, and
//! the system property `ro.build.version.sdk`, which `__system_property_get`
//! reads from API 23 on, stands in when it is absent.

#![expect(
    unsafe_code,
    reason = "calls into functions found by name; each block names the declaration it relies on"
)]

use std::ffi::{CStr, c_char, c_int, c_void};
use std::ptr::NonNull;
use std::sync::Arc;

use crate::version::gate::Gate;
use crate::version::gate::android::AndroidApi;

/// The size of the buffer `__system_property_get` fills (`PROP_VALUE_MAX`).
const PROPERTY_VALUE_MAX: usize = 92;

const API_LEVEL_FN: &CStr = c"android_get_device_api_level";
const PROPERTY_GET_FN: &CStr = c"__system_property_get";
const SDK_PROPERTY: &CStr = c"ro.build.version.sdk";

/// Where functions are looked up by name.
///
/// The real source searches the libraries the process can load; a test supplies
/// a table of its own.
pub(crate) trait SymbolSource: Send + Sync {
    /// The address of the function or object called `name`, if some library
    /// exports it.
    fn symbol(&self, name: &CStr) -> Option<NonNull<c_void>>;
}

/// The reading of the level; the type and its gates are in
/// [`version::gate::android`](crate::version::gate::android).
impl AndroidApi {
    /// Read the level of the running system through `source`: the NDK function
    /// when the system exports it, otherwise the system property. `None` when
    /// neither answers.
    pub(crate) fn detect(source: &dyn SymbolSource) -> Option<Self> {
        Self::from_ndk_function(source).or_else(|| Self::from_system_property(source))
    }

    fn from_ndk_function(source: &dyn SymbolSource) -> Option<Self> {
        type LevelFn = unsafe extern "C" fn() -> c_int;
        let function = source.symbol(API_LEVEL_FN)?;
        // SAFETY: `android_get_device_api_level` is declared in `<android/api-level.h>`
        // as `int android_get_device_api_level(void)`, which is `LevelFn`; the
        // address was returned for that name and the library stays loaded for the
        // duration of the call (the source outlives this borrow).
        let level = unsafe {
            let function: LevelFn = std::mem::transmute::<*mut c_void, LevelFn>(function.as_ptr());
            function()
        };
        u32::try_from(level)
            .ok()
            .filter(|level| *level > 0)
            .map(Self::from_level)
    }

    fn from_system_property(source: &dyn SymbolSource) -> Option<Self> {
        type PropertyGetFn = unsafe extern "C" fn(*const c_char, *mut c_char) -> c_int;
        let function = source.symbol(PROPERTY_GET_FN)?;
        let mut value = [0 as c_char; PROPERTY_VALUE_MAX];
        // SAFETY: `__system_property_get` is declared in `<sys/system_properties.h>`
        // as `int __system_property_get(const char* name, char* value)`, which is
        // `PropertyGetFn`; `name` is NUL-terminated and `value` has the
        // `PROP_VALUE_MAX` bytes the function writes at most, NUL included.
        let length = unsafe {
            let function: PropertyGetFn =
                std::mem::transmute::<*mut c_void, PropertyGetFn>(function.as_ptr());
            function(SDK_PROPERTY.as_ptr(), value.as_mut_ptr())
        };
        if length <= 0 {
            return None;
        }
        // SAFETY: a non-negative result means the function wrote a
        // NUL-terminated string into `value`.
        let text = unsafe { CStr::from_ptr(value.as_ptr()) };
        text.to_str()
            .ok()?
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|level| *level > 0)
            .map(Self::from_level)
    }
}

/// The window and choreographer types the NDK functions take. On Android they
/// are the NDK's own; the host tests, which have no NDK, get opaque stand-ins.
#[cfg(target_os = "android")]
pub(crate) use android_activity::ndk_sys::{AChoreographer, ANativeWindow};
/// See the Android re-export.
#[cfg(not(target_os = "android"))]
#[repr(C)]
pub(crate) struct ANativeWindow {
    _opaque: [u8; 0],
}
/// See the Android re-export.
#[cfg(not(target_os = "android"))]
#[repr(C)]
pub(crate) struct AChoreographer {
    _opaque: [u8; 0],
}

/// See the Android re-export.
#[cfg(not(target_os = "android"))]
#[repr(C)]
pub(crate) struct AChoreographerFrameCallbackData {
    _opaque: [u8; 0],
}
/// What a vsync callback is handed; on Android the NDK's own.
#[cfg(target_os = "android")]
pub(crate) use android_activity::ndk_sys::AChoreographerFrameCallbackData;

/// `AFontMatcher` (`<android/font_matcher.h>`), opaque.
#[repr(C)]
pub(crate) struct AFontMatcher {
    _opaque: [u8; 0],
}
/// `AFont` (`<android/font.h>`), opaque.
#[repr(C)]
pub(crate) struct AFont {
    _opaque: [u8; 0],
}

/// `void (*)(int64_t frameTimeNanos, void* data)`, the frame callback of both
/// posts: `AChoreographer_frameCallback64`, and on a 64-bit process
/// `AChoreographer_frameCallback`, whose `long` is the same 64 bits.
pub(crate) type FrameCallback = unsafe extern "C" fn(i64, *mut c_void);

/// `void (*)(const AChoreographerFrameCallbackData*, void* data)`.
pub(crate) type VsyncCallback =
    unsafe extern "C" fn(*const AChoreographerFrameCallbackData, *mut c_void);

/// `void (*)(int64_t vsyncPeriodNanos, void* data)`.
pub(crate) type RefreshRateCallback = unsafe extern "C" fn(i64, *mut c_void);

mod sealed {
    pub trait Sealed {}
}

/// An NDK function that exists only from some API level: its name, its pointer
/// type, and as a [`Gate`] the level it appeared in.
///
/// Sealed: the only implementations are the ones below, which pair each name
/// with the signature the NDK header declares for it. That pairing is what makes
/// [`NdkFn::resolve`] safe.
pub(crate) trait NdkFunction: sealed::Sealed + Gate<Version = AndroidApi> {
    /// The function's pointer type; pointer-sized.
    type Pointer: Copy;
    /// The exported symbol.
    const NAME: &'static CStr;
}

macro_rules! ndk_function {
    ($(#[$doc:meta])* $marker:ident, $name:literal, $since:literal, $pointer:ty) => {
        $(#[$doc])*
        pub(crate) struct $marker;
        impl sealed::Sealed for $marker {}
        impl Gate for $marker {
            type Version = AndroidApi;
            const SINCE: u64 = $since;
        }
        impl NdkFunction for $marker {
            type Pointer = $pointer;
            const NAME: &'static CStr = $name;
        }
    };
}

ndk_function!(
    /// `int32_t ANativeWindow_setFrameRate(ANativeWindow*, float, int8_t)`
    /// (`<android/native_window.h>`).
    SetFrameRate,
    c"ANativeWindow_setFrameRate",
    30,
    unsafe extern "C" fn(*mut ANativeWindow, f32, i8) -> i32
);
ndk_function!(
    /// `AChoreographer* AChoreographer_getInstance(void)`
    /// (`<android/choreographer.h>`).
    ChoreographerInstance,
    c"AChoreographer_getInstance",
    24,
    unsafe extern "C" fn() -> *mut AChoreographer
);
ndk_function!(
    /// `void AChoreographer_registerRefreshRateCallback(AChoreographer*,
    /// AChoreographer_refreshRateCallback, void*)`.
    RegisterRefreshRate,
    c"AChoreographer_registerRefreshRateCallback",
    30,
    unsafe extern "C" fn(*mut AChoreographer, RefreshRateCallback, *mut c_void)
);
ndk_function!(
    /// `void AChoreographer_unregisterRefreshRateCallback(AChoreographer*,
    /// AChoreographer_refreshRateCallback, void*)`.
    UnregisterRefreshRate,
    c"AChoreographer_unregisterRefreshRateCallback",
    30,
    unsafe extern "C" fn(*mut AChoreographer, RefreshRateCallback, *mut c_void)
);

#[cfg(target_pointer_width = "64")]
ndk_function!(
    /// `void AChoreographer_postFrameCallback(AChoreographer*,
    /// AChoreographer_frameCallback, void*)`, whose callback takes the frame
    /// time as a `long`: 64 bits only on a 64-bit process, so a 32-bit one
    /// never posts it.
    PostFrameCallback,
    c"AChoreographer_postFrameCallback",
    24,
    unsafe extern "C" fn(*mut AChoreographer, FrameCallback, *mut c_void)
);
ndk_function!(
    /// `void AChoreographer_postFrameCallback64(AChoreographer*,
    /// AChoreographer_frameCallback64, void*)`.
    PostFrameCallback64,
    c"AChoreographer_postFrameCallback64",
    29,
    unsafe extern "C" fn(*mut AChoreographer, FrameCallback, *mut c_void)
);
ndk_function!(
    /// `void AChoreographer_postVsyncCallback(AChoreographer*,
    /// AChoreographer_vsyncCallback, void*)`.
    PostVsyncCallback,
    c"AChoreographer_postVsyncCallback",
    33,
    unsafe extern "C" fn(*mut AChoreographer, VsyncCallback, *mut c_void)
);
ndk_function!(
    /// `int64_t AChoreographerFrameCallbackData_getFrameTimeNanos(const
    /// AChoreographerFrameCallbackData*)`.
    VsyncFrameTime,
    c"AChoreographerFrameCallbackData_getFrameTimeNanos",
    33,
    unsafe extern "C" fn(*const AChoreographerFrameCallbackData) -> i64
);
ndk_function!(
    /// `size_t AChoreographerFrameCallbackData_getPreferredFrameTimelineIndex(const
    /// AChoreographerFrameCallbackData*)`.
    VsyncPreferredTimeline,
    c"AChoreographerFrameCallbackData_getPreferredFrameTimelineIndex",
    33,
    unsafe extern "C" fn(*const AChoreographerFrameCallbackData) -> usize
);
ndk_function!(
    /// `int64_t AChoreographerFrameCallbackData_getFrameTimelineExpectedPresentationTimeNanos(const
    /// AChoreographerFrameCallbackData*, size_t)`.
    VsyncExpectedPresent,
    c"AChoreographerFrameCallbackData_getFrameTimelineExpectedPresentationTimeNanos",
    33,
    unsafe extern "C" fn(*const AChoreographerFrameCallbackData, usize) -> i64
);
ndk_function!(
    /// `AFontMatcher* AFontMatcher_create(void)` (`<android/font_matcher.h>`).
    FontMatcherCreate,
    c"AFontMatcher_create",
    29,
    unsafe extern "C" fn() -> *mut AFontMatcher
);
ndk_function!(
    /// `void AFontMatcher_destroy(AFontMatcher*)`.
    FontMatcherDestroy,
    c"AFontMatcher_destroy",
    29,
    unsafe extern "C" fn(*mut AFontMatcher)
);
ndk_function!(
    /// `void AFontMatcher_setLocales(AFontMatcher*, const char* languageTags)`.
    FontMatcherSetLocales,
    c"AFontMatcher_setLocales",
    29,
    unsafe extern "C" fn(*mut AFontMatcher, *const c_char)
);
ndk_function!(
    /// `AFont* AFontMatcher_match(const AFontMatcher*, const char* familyName,
    /// const uint16_t* text, uint32_t textLength, uint32_t* runLengthOut)`.
    FontMatcherMatch,
    c"AFontMatcher_match",
    29,
    unsafe extern "C" fn(*const AFontMatcher, *const c_char, *const u16, u32, *mut u32) -> *mut AFont
);
ndk_function!(
    /// `const char* AFont_getFontFilePath(const AFont*)` (`<android/font.h>`).
    FontFilePath,
    c"AFont_getFontFilePath",
    29,
    unsafe extern "C" fn(*const AFont) -> *const c_char
);
ndk_function!(
    /// `size_t AFont_getCollectionIndex(const AFont*)`.
    FontCollectionIndex,
    c"AFont_getCollectionIndex",
    29,
    unsafe extern "C" fn(*const AFont) -> usize
);
ndk_function!(
    /// `void AFont_close(AFont*)`.
    FontClose,
    c"AFont_close",
    29,
    unsafe extern "C" fn(*mut AFont)
);

/// The font matcher's functions, all or none: the system's own answer to
/// which face draws a text, from API 29.
pub(crate) struct FontMatching {
    pub(crate) create: NdkFn<FontMatcherCreate>,
    pub(crate) destroy: NdkFn<FontMatcherDestroy>,
    pub(crate) set_locales: NdkFn<FontMatcherSetLocales>,
    pub(crate) matches: NdkFn<FontMatcherMatch>,
    pub(crate) file_path: NdkFn<FontFilePath>,
    pub(crate) collection_index: NdkFn<FontCollectionIndex>,
    pub(crate) close: NdkFn<FontClose>,
}

impl FontMatching {
    /// The functions from `library`, when `api` has them and the library
    /// exports every one.
    pub(crate) fn resolve(library: &Arc<dyn SymbolSource>, api: AndroidApi) -> Option<Self> {
        Some(Self {
            create: NdkFn::resolve(library, api)?,
            destroy: NdkFn::resolve(library, api)?,
            set_locales: NdkFn::resolve(library, api)?,
            matches: NdkFn::resolve(library, api)?,
            file_path: NdkFn::resolve(library, api)?,
            collection_index: NdkFn::resolve(library, api)?,
            close: NdkFn::resolve(library, api)?,
        })
    }
}

/// A function of the NDK, found by name on a system new enough to have it.
///
/// Holds the source of the symbol, so the library the pointer points into is not
/// unloaded while the handle lives.
pub(crate) struct NdkFn<N: NdkFunction> {
    function: N::Pointer,
    /// Keeps the library loaded: a function pointer does not survive `dlclose`.
    _library: Arc<dyn SymbolSource>,
}

impl<N: NdkFunction> NdkFn<N> {
    /// Look `N` up in `library`, but only when `api` has it.
    pub(crate) fn resolve(library: &Arc<dyn SymbolSource>, api: AndroidApi) -> Option<Self> {
        const {
            assert!(size_of::<N::Pointer>() == size_of::<*mut c_void>());
        }
        api.since::<N>()?;
        let address = library.symbol(N::NAME)?.as_ptr();
        // SAFETY: `N::Pointer` is pointer-sized (asserted above) and, because
        // `NdkFunction` is sealed to the declarations above, the type of the
        // function that `N::NAME` names.
        let function = unsafe { std::mem::transmute_copy::<*mut c_void, N::Pointer>(&address) };
        Some(Self {
            function,
            _library: Arc::clone(library),
        })
    }

    /// The function. The pointer is valid for as long as `self` lives.
    pub(crate) fn get(&self) -> N::Pointer {
        self.function
    }
}

/// The three `AChoreographer` functions needed to watch the refresh rate, all
/// or none.
pub(crate) struct RefreshRateWatch {
    pub(crate) instance: NdkFn<ChoreographerInstance>,
    pub(crate) register: NdkFn<RegisterRefreshRate>,
    pub(crate) unregister: NdkFn<UnregisterRefreshRate>,
}

impl RefreshRateWatch {
    /// Resolve them on a system that has them.
    pub(crate) fn resolve(library: &Arc<dyn SymbolSource>, api: AndroidApi) -> Option<Self> {
        Some(Self {
            instance: NdkFn::resolve(library, api)?,
            register: NdkFn::resolve(library, api)?,
            unregister: NdkFn::resolve(library, api)?,
        })
    }
}

/// How the loop posts a callback for the choreographer's next frame: the
/// newest post the system has.
pub(crate) enum FramePost {
    /// The vsync callback (API 33), whose frame timelines name when the frame
    /// is expected on screen.
    Vsync {
        post: NdkFn<PostVsyncCallback>,
        frame_time: NdkFn<VsyncFrameTime>,
        preferred: NdkFn<VsyncPreferredTimeline>,
        expected_present: NdkFn<VsyncExpectedPresent>,
    },
    /// The frame callback with a 64-bit frame time (API 29).
    Frame64(NdkFn<PostFrameCallback64>),
    /// The first frame callback (API 24), on a 64-bit process only.
    #[cfg(target_pointer_width = "64")]
    Frame(NdkFn<PostFrameCallback>),
}

/// The choreographer of the calling thread and how a frame callback is posted
/// to it, all or none.
pub(crate) struct FrameCallbacks {
    pub(crate) instance: NdkFn<ChoreographerInstance>,
    pub(crate) post: FramePost,
}

impl FrameCallbacks {
    /// Resolve them on a system that has them: the vsync callback with each of
    /// the three readings of its data, else a frame callback.
    pub(crate) fn resolve(library: &Arc<dyn SymbolSource>, api: AndroidApi) -> Option<Self> {
        let instance = NdkFn::resolve(library, api)?;
        let vsync = || {
            Some(FramePost::Vsync {
                post: NdkFn::resolve(library, api)?,
                frame_time: NdkFn::resolve(library, api)?,
                preferred: NdkFn::resolve(library, api)?,
                expected_present: NdkFn::resolve(library, api)?,
            })
        };
        #[cfg(target_pointer_width = "64")]
        let first = || NdkFn::resolve(library, api).map(FramePost::Frame);
        #[cfg(not(target_pointer_width = "64"))]
        let first = || None;
        let post = vsync()
            .or_else(|| NdkFn::resolve(library, api).map(FramePost::Frame64))
            .or_else(first)?;
        Some(Self { instance, post })
    }
}

/// The libraries of the running system, opened once and closed with the last
/// handle that uses them.
#[cfg(target_os = "android")]
pub(crate) struct LoadedLibraries {
    handles: Vec<NonNull<c_void>>,
}

#[cfg(target_os = "android")]
impl LoadedLibraries {
    /// Open `libc.so` (where `android_get_device_api_level` and
    /// `__system_property_get` live) and `libandroid.so` (`ANativeWindow_*` and
    /// `AChoreographer_*`).
    pub(crate) fn open() -> Self {
        let handles = [c"libc.so", c"libandroid.so"]
            .into_iter()
            .filter_map(|name| {
                // SAFETY: `name` is NUL-terminated; the returned handle is
                // closed only in `Drop`, after every function pointer taken
                // from it is gone (they keep this value alive through `NdkFn`).
                NonNull::new(unsafe {
                    libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL)
                })
            })
            .collect();
        Self { handles }
    }
}

#[cfg(target_os = "android")]
impl SymbolSource for LoadedLibraries {
    fn symbol(&self, name: &CStr) -> Option<NonNull<c_void>> {
        self.handles.iter().find_map(|handle| {
            // SAFETY: the handle is a live `dlopen` handle (closed only in
            // `Drop`) and `name` is NUL-terminated.
            NonNull::new(unsafe { libc::dlsym(handle.as_ptr(), name.as_ptr()) })
        })
    }
}

#[cfg(target_os = "android")]
// SAFETY: a `dlopen` handle is an opaque token the dynamic linker accepts from
// any thread, and this value only hands it to `dlsym` (thread-safe) and
// `dlclose` (in `Drop`, when no other reference exists).
unsafe impl Send for LoadedLibraries {}

#[cfg(target_os = "android")]
// SAFETY: see the `Send` impl; `dlsym` on one handle is thread-safe.
unsafe impl Sync for LoadedLibraries {}

#[cfg(target_os = "android")]
impl Drop for LoadedLibraries {
    fn drop(&mut self) {
        for handle in self.handles.drain(..) {
            // SAFETY: the handle came from `dlopen` in `open` and is closed
            // once; every `NdkFn` that points into it holds an `Arc` of this
            // value, so none outlives this call.
            unsafe { libc::dlclose(handle.as_ptr()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// A symbol table the test fills.
    #[derive(Default)]
    struct FakeSymbols(HashMap<&'static CStr, *const ()>);

    impl FakeSymbols {
        fn with(mut self, name: &'static CStr, address: *const ()) -> Self {
            self.0.insert(name, address);
            self
        }
    }

    impl SymbolSource for FakeSymbols {
        fn symbol(&self, name: &CStr) -> Option<NonNull<c_void>> {
            self.0
                .get(name)
                .and_then(|address| NonNull::new(address.cast_mut().cast()))
        }
    }

    // SAFETY: the table holds addresses of functions, which any thread may call.
    unsafe impl Send for FakeSymbols {}
    // SAFETY: as above; the table is only read after construction.
    unsafe impl Sync for FakeSymbols {}

    extern "C" fn level_33() -> c_int {
        33
    }

    extern "C" fn level_zero() -> c_int {
        0
    }

    /// Writes the level 29 for the SDK property and fails for any other name.
    unsafe extern "C" fn property_29(name: *const c_char, value: *mut c_char) -> c_int {
        // SAFETY: the caller passes a NUL-terminated name and a buffer of
        // `PROP_VALUE_MAX` bytes.
        unsafe {
            if CStr::from_ptr(name) != SDK_PROPERTY {
                return 0;
            }
            for (index, byte) in b"29\0".iter().enumerate() {
                *value.add(index) = c_char::from_ne_bytes([*byte]);
            }
        }
        2
    }

    unsafe extern "C" fn property_garbage(_: *const c_char, value: *mut c_char) -> c_int {
        // SAFETY: the caller passes a buffer of `PROP_VALUE_MAX` bytes.
        unsafe {
            for (index, byte) in b"tiramisu\0".iter().enumerate() {
                *value.add(index) = c_char::from_ne_bytes([*byte]);
            }
        }
        8
    }

    unsafe extern "C" fn set_frame_rate_stub(_: *mut ANativeWindow, _: f32, _: i8) -> i32 {
        0
    }

    fn library(symbols: FakeSymbols) -> Arc<dyn SymbolSource> {
        Arc::new(symbols)
    }

    /// An address no test calls: the lookup only has to find it.
    fn address(number: usize) -> *const () {
        std::ptr::without_provenance(number)
    }

    fn api_29_resolves_no_set_frame_rate_handle() {
        let library = library(FakeSymbols::default().with(
            c"ANativeWindow_setFrameRate",
            set_frame_rate_stub as *const (),
        ));
        assert!(
            NdkFn::<SetFrameRate>::resolve(&library, AndroidApi::from_level(29)).is_none(),
            "a system below API 30 got a handle to a function it does not have"
        );
    }

    fn api_30_resolves_the_set_frame_rate_handle() {
        let library = library(FakeSymbols::default().with(
            c"ANativeWindow_setFrameRate",
            set_frame_rate_stub as *const (),
        ));
        let handle = NdkFn::<SetFrameRate>::resolve(&library, AndroidApi::from_level(30))
            .expect("API 30 has the function");
        // SAFETY: the table holds `set_frame_rate_stub`, which ignores its arguments.
        let status = unsafe { (handle.get())(std::ptr::null_mut(), 60.0, 0) };
        assert_eq!(status, 0, "the handle is the function the table named");
    }

    fn a_missing_symbol_gives_no_handle_even_at_api_30() {
        let library = library(FakeSymbols::default());
        assert!(NdkFn::<SetFrameRate>::resolve(&library, AndroidApi::from_level(34)).is_none());
        assert!(RefreshRateWatch::resolve(&library, AndroidApi::from_level(34)).is_none());
    }

    fn the_choreographer_watch_needs_all_three_functions() {
        let all = FakeSymbols::default()
            .with(c"AChoreographer_getInstance", address(1))
            .with(c"AChoreographer_registerRefreshRateCallback", address(2))
            .with(c"AChoreographer_unregisterRefreshRateCallback", address(3));
        let watch = RefreshRateWatch::resolve(&library(all), AndroidApi::from_level(30))
            .expect("API 30 has all three");
        assert_eq!(watch.instance.get() as *const (), address(1));
        assert_eq!(watch.register.get() as *const (), address(2));
        assert_eq!(
            watch.unregister.get() as *const (),
            address(3),
            "each handle is the function of its own name"
        );
        let no_unregister = FakeSymbols::default()
            .with(c"AChoreographer_getInstance", address(1))
            .with(c"AChoreographer_registerRefreshRateCallback", address(2));
        assert!(
            RefreshRateWatch::resolve(&library(no_unregister), AndroidApi::from_level(30))
                .is_none(),
            "a callback that cannot be taken down must not be registered"
        );
        let all = FakeSymbols::default()
            .with(c"AChoreographer_getInstance", address(1))
            .with(c"AChoreographer_registerRefreshRateCallback", address(2))
            .with(c"AChoreographer_unregisterRefreshRateCallback", address(3));
        assert!(RefreshRateWatch::resolve(&library(all), AndroidApi::from_level(29)).is_none());
    }

    /// Every frame callback a system can post, each at an address of its own.
    fn frame_callback_symbols() -> FakeSymbols {
        FakeSymbols::default()
            .with(c"AChoreographer_getInstance", address(1))
            .with(c"AChoreographer_postFrameCallback", address(2))
            .with(c"AChoreographer_postFrameCallback64", address(3))
            .with(c"AChoreographer_postVsyncCallback", address(4))
            .with(
                c"AChoreographerFrameCallbackData_getFrameTimeNanos",
                address(5),
            )
            .with(
                c"AChoreographerFrameCallbackData_getPreferredFrameTimelineIndex",
                address(6),
            )
            .with(
                c"AChoreographerFrameCallbackData_getFrameTimelineExpectedPresentationTimeNanos",
                address(7),
            )
    }

    /// The post each system gets: the newest it has, and the vsync callback
    /// only with all three readings of its data.
    fn the_frame_callback_is_the_newest_the_system_has() {
        let posted = |symbols: FakeSymbols, level| {
            FrameCallbacks::resolve(&library(symbols), AndroidApi::from_level(level)).map(
                |callbacks| match callbacks.post {
                    FramePost::Vsync {
                        post,
                        frame_time,
                        preferred,
                        expected_present,
                    } => {
                        assert_eq!(callbacks.instance.get() as *const (), address(1));
                        assert_eq!(
                            [
                                frame_time.get() as *const (),
                                preferred.get() as *const (),
                                expected_present.get() as *const (),
                            ],
                            [address(5), address(6), address(7)],
                            "each reading is the function of its own name"
                        );
                        post.get() as *const ()
                    }
                    FramePost::Frame64(post) => post.get() as *const (),
                    #[cfg(target_pointer_width = "64")]
                    FramePost::Frame(post) => post.get() as *const (),
                },
            )
        };
        let no_expected_present = {
            let mut symbols = frame_callback_symbols();
            symbols.0.remove(
                c"AChoreographerFrameCallbackData_getFrameTimelineExpectedPresentationTimeNanos",
            );
            symbols
        };
        let no_instance = {
            let mut symbols = frame_callback_symbols();
            symbols.0.remove(c"AChoreographer_getInstance");
            symbols
        };
        // (case, symbols, level, the post's address)
        let rows: [(&str, FakeSymbols, u32, Option<*const ()>); 6] = [
            ("API 34", frame_callback_symbols(), 34, Some(address(4))),
            ("API 33", frame_callback_symbols(), 33, Some(address(4))),
            (
                "API 33 without a reading of the vsync data",
                no_expected_present,
                33,
                Some(address(3)),
            ),
            ("API 29", frame_callback_symbols(), 29, Some(address(3))),
            ("API 23", frame_callback_symbols(), 23, None),
            ("no choreographer", no_instance, 34, None),
        ];
        for (case, symbols, level, expected) in rows {
            assert_eq!(posted(symbols, level), expected, "{case}");
        }
        #[cfg(target_pointer_width = "64")]
        assert_eq!(
            posted(frame_callback_symbols(), 24),
            Some(address(2)),
            "a 64-bit process below API 29 posts the first frame callback"
        );
    }

    fn the_api_level_comes_from_the_ndk_function_when_the_system_has_it() {
        let symbols = FakeSymbols::default()
            .with(c"android_get_device_api_level", level_33 as *const ())
            .with(c"__system_property_get", property_29 as *const ());
        assert_eq!(
            AndroidApi::detect(&symbols),
            Some(AndroidApi::from_level(33)),
            "the property answered although the function was there"
        );
    }

    fn the_api_level_falls_back_to_the_system_property() {
        // An API 23 libc exports no `android_get_device_api_level`.
        let symbols =
            FakeSymbols::default().with(c"__system_property_get", property_29 as *const ());
        assert_eq!(
            AndroidApi::detect(&symbols),
            Some(AndroidApi::from_level(29))
        );
        // A function that answers nothing useful does not hide the property.
        let symbols = FakeSymbols::default()
            .with(c"android_get_device_api_level", level_zero as *const ())
            .with(c"__system_property_get", property_29 as *const ());
        assert_eq!(
            AndroidApi::detect(&symbols),
            Some(AndroidApi::from_level(29))
        );
    }

    fn the_font_matcher_needs_all_seven_functions() {
        const NAMES: [&CStr; 7] = [
            c"AFontMatcher_create",
            c"AFontMatcher_destroy",
            c"AFontMatcher_setLocales",
            c"AFontMatcher_match",
            c"AFont_getFontFilePath",
            c"AFont_getCollectionIndex",
            c"AFont_close",
        ];
        let table = |skip: Option<usize>| {
            let mut symbols = FakeSymbols::default();
            for (index, name) in NAMES.iter().enumerate() {
                if Some(index) != skip {
                    symbols = symbols.with(name, address(index + 1));
                }
            }
            library(symbols)
        };
        let matching = FontMatching::resolve(&table(None), AndroidApi::from_level(29))
            .expect("API 29 has the matcher");
        let found = [
            matching.create.get() as *const (),
            matching.destroy.get() as *const (),
            matching.set_locales.get() as *const (),
            matching.matches.get() as *const (),
            matching.file_path.get() as *const (),
            matching.collection_index.get() as *const (),
            matching.close.get() as *const (),
        ];
        let expected: Vec<*const ()> = (1..=NAMES.len()).map(address).collect();
        assert_eq!(
            found.to_vec(),
            expected,
            "each handle is the function its name names"
        );
        assert!(
            FontMatching::resolve(&table(None), AndroidApi::from_level(28)).is_none(),
            "below API 29 the matcher is not looked up"
        );
        for (missing, name) in NAMES.iter().enumerate() {
            assert!(
                FontMatching::resolve(&table(Some(missing)), AndroidApi::from_level(34)).is_none(),
                "{name:?} is missing, so none of the matcher is used"
            );
        }
    }

    fn an_unreadable_api_level_is_unknown() {
        assert_eq!(AndroidApi::detect(&FakeSymbols::default()), None);
        let symbols =
            FakeSymbols::default().with(c"__system_property_get", property_garbage as *const ());
        assert_eq!(AndroidApi::detect(&symbols), None);
    }

    /// The NDK functions' gates, the rows of `version::gate`'s table for the
    /// markers declared here.
    #[test]
    fn ndk_gate_table() {
        use crate::version::gate::testing::row;

        let api = AndroidApi::from_level;
        let unknown = AndroidApi::UNKNOWN;
        let mut failures = Vec::new();
        failures.extend(row::<SetFrameRate>(
            "SetFrameRate",
            api(29),
            api(30),
            unknown,
        ));
        failures.extend(row::<ChoreographerInstance>(
            "ChoreographerInstance",
            api(23),
            api(24),
            unknown,
        ));
        failures.extend(row::<RegisterRefreshRate>(
            "RegisterRefreshRate",
            api(29),
            api(30),
            unknown,
        ));
        failures.extend(row::<UnregisterRefreshRate>(
            "UnregisterRefreshRate",
            api(29),
            api(30),
            unknown,
        ));
        failures.extend(row::<PostFrameCallback64>(
            "PostFrameCallback64",
            api(28),
            api(29),
            unknown,
        ));
        #[cfg(target_pointer_width = "64")]
        failures.extend(row::<PostFrameCallback>(
            "PostFrameCallback",
            api(23),
            api(24),
            unknown,
        ));
        failures.extend(row::<PostVsyncCallback>(
            "PostVsyncCallback",
            api(32),
            api(33),
            unknown,
        ));
        failures.extend(row::<VsyncFrameTime>(
            "VsyncFrameTime",
            api(32),
            api(33),
            unknown,
        ));
        failures.extend(row::<VsyncPreferredTimeline>(
            "VsyncPreferredTimeline",
            api(32),
            api(33),
            unknown,
        ));
        failures.extend(row::<FontMatcherMatch>(
            "FontMatcherMatch",
            api(28),
            api(29),
            unknown,
        ));
        failures.extend(row::<VsyncExpectedPresent>(
            "VsyncExpectedPresent",
            api(32),
            api(33),
            unknown,
        ));
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn os_capability_table() {
        crate::table_test::run_table(
            "os_capability_table",
            &[
                (
                    "api_29_resolves_no_set_frame_rate_handle",
                    api_29_resolves_no_set_frame_rate_handle as fn(),
                ),
                (
                    "api_30_resolves_the_set_frame_rate_handle",
                    api_30_resolves_the_set_frame_rate_handle as fn(),
                ),
                (
                    "a_missing_symbol_gives_no_handle_even_at_api_30",
                    a_missing_symbol_gives_no_handle_even_at_api_30 as fn(),
                ),
                (
                    "the_choreographer_watch_needs_all_three_functions",
                    the_choreographer_watch_needs_all_three_functions as fn(),
                ),
                (
                    "the_frame_callback_is_the_newest_the_system_has",
                    the_frame_callback_is_the_newest_the_system_has as fn(),
                ),
                (
                    "the_api_level_comes_from_the_ndk_function_when_the_system_has_it",
                    the_api_level_comes_from_the_ndk_function_when_the_system_has_it as fn(),
                ),
                (
                    "the_api_level_falls_back_to_the_system_property",
                    the_api_level_falls_back_to_the_system_property as fn(),
                ),
                (
                    "the_font_matcher_needs_all_seven_functions",
                    the_font_matcher_needs_all_seven_functions as fn(),
                ),
                (
                    "an_unreadable_api_level_is_unknown",
                    an_unreadable_api_level_is_unknown as fn(),
                ),
            ],
        );
    }
}
