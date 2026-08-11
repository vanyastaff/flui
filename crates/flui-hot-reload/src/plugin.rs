//! Plugin-side API for FLUI hot-reload.
//!
//! Provides two macros:
//!
//! - `scene_plugin!` — wraps a raw `fn(f32, f32) -> Scene` function
//!   (low-level)
//! - `app_plugin!` — wraps a `View + StatelessView` widget in a
//!   self-contained pipeline that runs Build → Layout → Paint → Scene
//!   internally (high-level; requires `app-plugin` feature)

/// Generates the `extern "C"` FFI wrappers for a scene-building function.
///
/// The function must have the signature `fn(f32, f32) -> Scene` where the
/// two arguments are width and height in physical pixels.
///
/// # Generated Symbols
///
/// - `flui_scene_build(width, height) -> *mut c_void` — builds a Scene, returns
///   owned pointer
/// - `flui_scene_version() -> u32` — returns plugin version (for reload
///   detection)
/// - `flui_scene_drop(ptr)` — drops a Scene previously returned by
///   `flui_scene_build`
/// - `flui_scene_free(ptr)` — deallocates the box WITHOUT dropping the Scene
///   (the host moves the value out with `ptr::read` first, so allocation and
///   deallocation both happen inside the plugin image)
/// - `flui_scene_abi_token() -> u64` — the plugin-side
///   [`abi_token`](crate::abi_token); the host refuses to load on mismatch
///
/// # Example
///
/// ```rust,ignore
/// use flui_hot_reload::scene_plugin;
/// use flui_layer::*;
/// use flui_types::geometry::{px, Rect, Size};
/// use flui_types::painting::Paint;
/// use flui_types::styling::Color;
///
/// fn my_scene(width: f32, height: f32) -> Scene {
///     let mut tree = LayerTree::new();
///     let mut canvas_layer = CanvasLayer::new();
///     let canvas = canvas_layer.canvas_mut();
///     canvas.draw_rect(
///         Rect::from_ltrb(px(0.0), px(0.0), px(width), px(height)),
///         &Paint::fill(Color::rgb(128, 0, 128)),
///     );
///     let root = tree.insert(Layer::Canvas(canvas_layer));
///     Scene::new(Size::new(px(width), px(height)), tree, Some(root), 1)
/// }
///
/// scene_plugin!(my_scene);
/// ```
#[macro_export]
macro_rules! scene_plugin {
    ($build_fn:ident) => {
        /// Build a scene and return an opaque pointer to `Box<Scene>`.
        ///
        /// # Safety
        ///
        /// The returned pointer must be passed to `flui_scene_drop` when no longer
        /// needed, or taken ownership of via `Box::from_raw`.
        #[unsafe(no_mangle)]
        pub extern "C" fn flui_scene_build(width: f32, height: f32) -> *mut ::std::ffi::c_void {
            let scene = $build_fn(width, height);
            let boxed = ::std::boxed::Box::new(scene);
            ::std::boxed::Box::into_raw(boxed) as *mut ::std::ffi::c_void
        }

        /// Returns the plugin version number.
        ///
        /// The host uses this to confirm the plugin loaded successfully.
        #[unsafe(no_mangle)]
        pub extern "C" fn flui_scene_version() -> u32 {
            1
        }

        /// Drop a `Scene` previously returned by `flui_scene_build`.
        ///
        /// # Safety
        ///
        /// `ptr` must be a valid pointer returned by `flui_scene_build` that has
        /// not already been dropped. Passing null is safe (no-op).
        #[unsafe(no_mangle)]
        pub extern "C" fn flui_scene_drop(ptr: *mut ::std::ffi::c_void) {
            if !ptr.is_null() {
                #[allow(unsafe_code)]
                unsafe {
                    drop(::std::boxed::Box::from_raw(ptr as *mut ::flui_layer::Scene));
                }
            }
        }

        /// Deallocate the box behind `ptr` WITHOUT dropping the `Scene` inside.
        ///
        /// The host consumes the scene with `ptr::read` and then hands the
        /// now-logically-empty box back here, so the memory is released by the
        /// same image (and the same allocator/layout knowledge) that allocated
        /// it.
        ///
        /// # Safety
        ///
        /// `ptr` must come from `flui_scene_build`, and the host must have
        /// already moved the `Scene` value out — after this call the pointee
        /// is gone. Passing null is safe (no-op).
        #[unsafe(no_mangle)]
        pub extern "C" fn flui_scene_free(ptr: *mut ::std::ffi::c_void) {
            if !ptr.is_null() {
                #[allow(unsafe_code)]
                unsafe {
                    drop(::std::boxed::Box::from_raw(
                        ptr as *mut ::std::mem::MaybeUninit<::flui_layer::Scene>,
                    ));
                }
            }
        }

        /// Plugin-side ABI-compatibility token; the host refuses to load this
        /// library unless it equals the host's own token.
        #[unsafe(no_mangle)]
        pub extern "C" fn flui_scene_abi_token() -> u64 {
            $crate::abi_token()
        }
    };
}

/// Generates `extern "C"` entry points for a Flutter-parity **worker** crate.
///
/// The worker owns reloadable `build()` logic only; the host binary retains
/// element-tree `State`. Call an init function that registers build dispatch
/// (see `examples/hot_reload_counter/`).
///
/// # Generated symbols
///
/// - `flui_worker_init(register)` — register build fns (called on load + every reload)
/// - `flui_worker_version() -> u32`
/// - `flui_worker_fingerprint() -> u64` — optional layout-change detection
///
/// Requires the `app-plugin` feature on `flui-hot-reload`.
#[cfg(feature = "app-plugin")]
#[macro_export]
macro_rules! hot_reload_worker {
    ($init_fn:ident) => {
        $crate::hot_reload_worker!($init_fn, fingerprint: $crate::worker::DEFAULT_FINGERPRINT);
    };
    ($init_fn:ident, fingerprint: $fp:expr) => {
        /// Worker registration hook — runs on load and after every dylib reload.
        ///
        /// `register` is host-owned storage; never write build pointers into
        /// dylib-local `static` variables.
        #[unsafe(no_mangle)]
        pub extern "C" fn flui_worker_init(register: $crate::RegisterWorkerBuildFn) {
            $init_fn(register);
        }

        /// Worker version (for diagnostics).
        #[unsafe(no_mangle)]
        pub extern "C" fn flui_worker_version() -> u32 {
            1
        }

        /// Stable-layout fingerprint for the shared `types` crate.
        #[unsafe(no_mangle)]
        pub extern "C" fn flui_worker_fingerprint() -> u64 {
            $fp
        }

        /// Worker-side ABI-compatibility token; the host refuses to load this
        /// library unless it equals the host's own token.
        #[unsafe(no_mangle)]
        pub extern "C" fn flui_worker_abi_token() -> u64 {
            $crate::abi_token()
        }
    };
}

/// Generates `extern "C"` FFI wrappers for a widget-based hot-reload plugin.
///
/// Unlike `scene_plugin!` which wraps a raw scene-building function, this
/// macro wraps a `View + StatelessView` widget in a self-contained rendering
/// pipeline ([`crate::PluginPipeline`]) that runs the full Build → Layout → Paint →
/// Scene cycle.
///
/// The widget tree is mounted on the first call and rebuilt on subsequent
/// calls. On hot-reload (new `.so` loaded), the plugin's statics are fresh —
/// the pipeline re-mounts from scratch, giving "hot restart" semantics (code
/// updated, state lost).
///
/// # Thread affinity
///
/// The generated pipeline is confined to a single OS thread: whichever thread
/// makes the *first* `flui_app_build` call pins it for the lifetime of the
/// loaded image, and the host must keep driving the plugin from that same
/// thread afterward (in practice, its own UI thread — the plugin is never
/// meant to be called concurrently from more than one). Nothing about the raw
/// C ABI stops a host from calling a second thread, so this is enforced at
/// runtime: a call from any thread other than the pinned one is refused —
/// logged via `tracing::error!` and answered with a null pointer — rather
/// than silently building a second, independent widget tree behind the same
/// opaque symbol. A null return from `flui_app_build` carries no ownership;
/// never pass it to `flui_app_drop`/`flui_app_free`.
///
/// # Unload semantics
///
/// The mounted `PluginPipeline` is **leaked, never dropped** — on thread
/// exit, on hot-reload, and on process exit alike. This is not new: the
/// pipeline was previously behind a plain `static`, and Rust never runs drop
/// glue for `static` items either, so "leak on unload" was always this
/// macro's actual contract (the "hot restart: state lost" line above). What
/// changed is *how* that's now guaranteed: the pipeline lives in a
/// `thread_local!`, and a thread-local wrapping a type that needs dropping
/// registers a TLS destructor — one that lives in this plugin's own image.
/// `dlclose`-ing a DSO with a live TLS destructor registration is unsafe to
/// varying degrees across runtimes (a deferred-unmap runtime can silently
/// keep serving the OLD code on a same-path reload; a non-deferring one can
/// run the destructor after the image is already unmapped). The pipeline is
/// therefore stored as `ManuallyDrop<PluginPipeline>`, which is
/// unconditionally drop-glue-free, so no TLS destructor is ever registered
/// for it — deliberately trading "run `PluginPipeline::drop`" for "never put
/// this image's unload behavior at the mercy of pending TLS destructors".
/// Thread affinity is identified by a drop-free, per-thread monotonic token
/// in the same TLS payload. Calling `std::thread::current` from the cdylib is
/// deliberately avoided: Rust's private std copy can otherwise register a
/// pthread-key destructor whose callback also lives in the unloadable image.
///
/// # Generated Symbols
///
/// - `flui_app_build(width, height) -> *mut c_void` — runs pipeline, returns
///   owned Scene pointer, or null on a wrong-thread call (see
///   [Thread affinity](#thread-affinity))
/// - `flui_app_version() -> u32` — returns plugin version (for reload
///   detection)
/// - `flui_app_drop(ptr)` — drops a Scene previously returned by
///   `flui_app_build`
/// - `flui_app_free(ptr)` — deallocates the box WITHOUT dropping the Scene
///   (host moves the value out first; see `scene_plugin!`)
/// - `flui_app_abi_token() -> u64` — plugin-side
///   [`abi_token`](crate::abi_token); the host refuses to load on mismatch
///
/// # Example
///
/// ```rust,ignore
/// use flui_hot_reload::app_plugin;
/// use flui_view::prelude::*;
///
/// #[derive(Clone)]
/// struct MyApp;
///
/// impl StatelessView for MyApp {
///     fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
///         Center::new().child(Text::new("Hello from hot-reload!"))
///     }
/// }
///
/// impl View for MyApp {
///     fn create_element(&self) -> flui_view::element::ElementKind {
///         flui_view::element::ElementKind::stateless(self)
///     }
/// }
///
/// app_plugin!(MyApp);
/// ```
///
/// Requires the `app-plugin` feature on `flui-hot-reload`.
#[cfg(feature = "app-plugin")]
#[macro_export]
macro_rules! app_plugin {
    ($root_view:expr) => {
        /// Monotonic identity of the TLS lane that won the first build call.
        /// This deliberately does not use `std::thread::current`: a Rust
        /// cdylib's private std copy registers a pthread-key destructor in
        /// its own image when that API is first touched, leaving an invalid
        /// callback after `dlclose` on runtimes that unmap immediately.
        static __FLUI_APP_PINNED_THREAD: ::std::sync::OnceLock<u64> = ::std::sync::OnceLock::new();

        /// Per-image source for never-reused TLS lane identities. Zero stays
        /// reserved for an uninitialized lane.
        static __FLUI_APP_NEXT_THREAD_TOKEN: ::std::sync::atomic::AtomicU64 =
            ::std::sync::atomic::AtomicU64::new(1);

        /// The inner pipeline slot type. The compile-time assertion below is
        /// intentionally over the complete `__FluiAppThreadState`, so adding
        /// any future droppable field to the actual TLS payload fails closed.
        type __FluiAppPipelineSlot =
            ::std::option::Option<::std::mem::ManuallyDrop<$crate::PluginPipeline>>;

        /// The complete TLS payload. Both fields are drop-glue-free: the
        /// token is plain data and the pipeline is explicitly leaked on image
        /// unload, matching the macro's documented lifecycle contract.
        struct __FluiAppThreadState {
            token: ::std::cell::Cell<u64>,
            pipeline: ::std::cell::RefCell<__FluiAppPipelineSlot>,
        }

        ::std::thread_local! {
            /// Thread-confined pipeline storage. `PluginPipeline` bundles a
            /// `WidgetsBinding`/`PipelineOwner` pair that the reference
            /// architecture keeps single-threaded; `thread_local!` makes that
            /// the storage's actual shape instead of layering a lock
            /// discipline over a type that never needed to be `Send`. Only
            /// the pinned thread (enforced in `flui_app_build` below) ever
            /// populates this.
            ///
            /// Wrapped in `ManuallyDrop` deliberately: this code is compiled
            /// INTO the plugin image, and a thread-local with real drop glue
            /// registers a TLS destructor tied to that image. `dlclose` on a
            /// DSO with a live TLS destructor registration is exactly the
            /// unload hazard this exists to dodge — glibc defers the actual
            /// unmap until every thread that ever touched the slot has
            /// exited (so a same-path reload can silently keep serving the
            /// OLD mapped code instead of the rebuilt one — this crate's
            /// whole reason to exist), and a runtime that does NOT defer can
            /// instead run the destructor after the image is already
            /// unmapped (use-after-free). `ManuallyDrop` makes
            /// `mem::needs_drop` false for this slot unconditionally (see the
            /// `const` assertion right below), so the standard library never
            /// registers a destructor for it at all — no deferred unload, no
            /// UAF, on any runtime. This is not a NEW leak: a plain `static`
            /// (the previous `OnceLock<Mutex<PluginPipeline>>`) was never
            /// destructed either — Rust runs no automatic drop glue for
            /// `static` items — so "leak the pipeline, don't run its Drop"
            /// was already the contract `app_plugin!` shipped ("hot restart:
            /// state lost"). This preserves that byte-for-byte; it does not
            /// introduce it.
            static __FLUI_APP_STATE: __FluiAppThreadState = const {
                __FluiAppThreadState {
                    token: ::std::cell::Cell::new(0),
                    pipeline: ::std::cell::RefCell::new(::std::option::Option::None),
                }
            };
        }

        // Compile-time proof the thread-local above truly carries no drop
        // glue — the property the "no TLS destructor in the plugin image"
        // claim above depends on. If a future edit swaps `ManuallyDrop` for
        // something that needs dropping, this fails to compile instead of
        // silently reintroducing the dlclose hazard.
        const _: () = assert!(
            !::std::mem::needs_drop::<__FluiAppThreadState>(),
            "app_plugin!'s complete thread-local state must stay drop-glue-free \
             (wrapped in ManuallyDrop) — a droppable thread-local registers a \
             TLS destructor tied to the plugin image, which dlclose cannot \
             safely unwind past on every runtime"
        );

        /// Build a scene by running the full widget pipeline and return an opaque
        /// pointer to `Box<Scene>`.
        ///
        /// On the first call, mounts the root widget and pins the calling
        /// thread. Subsequent calls from that same thread rebuild dirty
        /// elements and re-layout/repaint as needed. A call from any other
        /// thread is refused — see `app_plugin!`'s "Thread affinity" docs —
        /// and returns null instead of a Scene pointer.
        ///
        /// # Safety
        ///
        /// A non-null returned pointer must be passed to `flui_app_drop` when
        /// no longer needed, or taken ownership of via `Box::from_raw`. A null
        /// return (wrong-thread call) owns nothing and must not be passed to
        /// either.
        #[unsafe(no_mangle)]
        pub extern "C" fn flui_app_build(width: f32, height: f32) -> *mut ::std::ffi::c_void {
            __FLUI_APP_STATE.with(|state| {
                let caller = match state.token.get() {
                    0 => {
                        let token = __FLUI_APP_NEXT_THREAD_TOKEN.fetch_update(
                            ::std::sync::atomic::Ordering::Relaxed,
                            ::std::sync::atomic::Ordering::Relaxed,
                            |next| next.checked_add(1),
                        );
                        let Ok(token) = token else {
                            $crate::__private_tracing::error!(
                                "app_plugin! exhausted its per-image thread-token space; \
                                 refusing to build a Scene"
                            );
                            return ::std::ptr::null_mut();
                        };
                        state.token.set(token);
                        token
                    }
                    token => token,
                };
                let pinned = *__FLUI_APP_PINNED_THREAD.get_or_init(|| caller);
                if pinned != caller {
                    $crate::__private_tracing::error!(
                        pinned,
                        caller,
                        "flui_app_build called from a thread other than the one \
                         that first built this plugin; app_plugin!'s ABI pins to \
                         the first caller — refusing to build a Scene from a \
                         foreign thread"
                    );
                    return ::std::ptr::null_mut();
                }

                let mut slot = state.pipeline.borrow_mut();
                let pipeline = slot.get_or_insert_with(|| {
                    ::std::mem::ManuallyDrop::new($crate::PluginPipeline::mount(
                        $root_view, width, height,
                    ))
                });
                let scene = pipeline.draw_frame(width, height);
                ::std::boxed::Box::into_raw(::std::boxed::Box::new(scene))
                    .cast::<::std::ffi::c_void>()
            })
        }

        /// Returns the plugin version number.
        #[unsafe(no_mangle)]
        pub extern "C" fn flui_app_version() -> u32 {
            1
        }

        /// Drop a `Scene` previously returned by `flui_app_build`.
        ///
        /// # Safety
        ///
        /// `ptr` must be a valid pointer returned by `flui_app_build` that has
        /// not already been dropped. Passing null is safe (no-op).
        #[unsafe(no_mangle)]
        pub extern "C" fn flui_app_drop(ptr: *mut ::std::ffi::c_void) {
            if !ptr.is_null() {
                #[allow(unsafe_code)]
                unsafe {
                    drop(::std::boxed::Box::from_raw(
                        ptr.cast::<::flui_layer::Scene>(),
                    ));
                }
            }
        }

        /// Deallocate the box behind `ptr` WITHOUT dropping the `Scene` inside.
        ///
        /// # Safety
        ///
        /// `ptr` must come from `flui_app_build`, and the host must have
        /// already moved the `Scene` value out — after this call the pointee
        /// is gone. Passing null is safe (no-op).
        #[unsafe(no_mangle)]
        pub extern "C" fn flui_app_free(ptr: *mut ::std::ffi::c_void) {
            if !ptr.is_null() {
                #[allow(unsafe_code)]
                unsafe {
                    drop(::std::boxed::Box::from_raw(
                        ptr.cast::<::std::mem::MaybeUninit<::flui_layer::Scene>>(),
                    ));
                }
            }
        }

        /// Plugin-side ABI-compatibility token; the host refuses to load this
        /// library unless it equals the host's own token.
        #[unsafe(no_mangle)]
        pub extern "C" fn flui_app_abi_token() -> u64 {
            $crate::abi_token()
        }
    };
}

/// Exercises `app_plugin!`'s generated `flui_app_build` in-process, proving
/// the wrong-thread call takes the guarded branch instead of silently
/// mounting a second, independent pipeline behind the same symbol.
///
/// This expands the real macro once (feature-gated the same way production
/// consumers are), so the `extern "C" fn`s under test are the actual
/// generated ABI, not a hand-written stand-in.
#[cfg(all(test, feature = "app-plugin"))]
mod thread_affinity_tests {
    // The generated `#[unsafe(no_mangle)]` extern "C" fns trip the crate's
    // `unsafe_code` warn-by-default lint the moment the macro is expanded
    // in-crate (as opposed to in a downstream cdylib, where these symbols
    // normally live) — same allowance the macro's own `unsafe {}` bodies
    // already carry elsewhere in this file.
    #![allow(unsafe_code)]

    #[derive(Clone)]
    struct ThreadAffinityProbeRoot;

    impl flui_view::StatelessView for ThreadAffinityProbeRoot {
        fn build(&self, _ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
            flui_view::ErrorView::new("thread-affinity probe")
        }
    }

    impl flui_view::View for ThreadAffinityProbeRoot {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }

    crate::app_plugin!(ThreadAffinityProbeRoot);

    #[test]
    fn wrong_thread_call_is_refused_not_silently_mounted() {
        // The first call, from this test's own thread, pins the plugin to
        // it and mounts the pipeline.
        let first = flui_app_build(64.0, 64.0);
        assert!(
            !first.is_null(),
            "the pinning thread's own first call must succeed"
        );
        flui_app_drop(first);

        // A call from a *different* OS thread must be refused: null, never a
        // second, independently-mounted Scene. (nextest gives this test its
        // own process, so `__FLUI_APP_PINNED_THREAD` starts unset here.)
        let refused = std::thread::spawn(|| flui_app_build(64.0, 64.0).is_null())
            .join()
            .expect("BUG: spawned probe thread must not panic");
        assert!(
            refused,
            "a call from a thread other than the pinning one must return \
             null instead of silently building a second pipeline behind the \
             same symbol"
        );

        // The pinned thread keeps working after a foreign-thread call was
        // refused — the guard doesn't wedge the legitimate caller.
        let second = flui_app_build(64.0, 64.0);
        assert!(
            !second.is_null(),
            "the pinning thread must keep working after a foreign-thread \
             call was refused"
        );
        flui_app_drop(second);
    }
}
