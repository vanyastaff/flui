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
    };
}

/// Generates `extern "C"` FFI wrappers for a widget-based hot-reload plugin.
///
/// Unlike `scene_plugin!` which wraps a raw scene-building function, this
/// macro wraps a `View + StatelessView` widget in a self-contained rendering
/// pipeline ([`PluginPipeline`]) that runs the full Build → Layout → Paint →
/// Scene cycle.
///
/// The widget tree is mounted on the first call and rebuilt on subsequent
/// calls. On hot-reload (new `.so` loaded), the `OnceLock` is fresh — the
/// pipeline re-mounts from scratch, giving "hot restart" semantics (code
/// updated, state lost).
///
/// # Generated Symbols
///
/// - `flui_app_build(width, height) -> *mut c_void` — runs pipeline, returns
///   owned Scene pointer
/// - `flui_app_version() -> u32` — returns plugin version (for reload
///   detection)
/// - `flui_app_drop(ptr)` — drops a Scene previously returned by
///   `flui_app_build`
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
///         Box::new(Center::new(
///             Text::new("Hello from hot-reload!")
///         ))
///     }
/// }
///
/// impl View for MyApp {
///     fn create_element(&self) -> Box<dyn ElementBase> {
///         Box::new(StatelessElement::new(self, StatelessBehavior))
///     }
/// }
///
/// app_plugin!(MyApp);
/// ```
///
/// Generates `extern "C"` entry points for a Flutter-parity **worker** crate.
///
/// The worker owns reloadable `build()` logic only; the host binary retains
/// element-tree `State`. Call an init function that registers build dispatch
/// (see `examples/hot_reload_counter/`).
///
/// # Generated symbols
///
/// - `flui_worker_init()` — register build fns (called on load + every reload)
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
    };
}

/// Requires the `app-plugin` feature on `flui-hot-reload`.
#[cfg(feature = "app-plugin")]
#[macro_export]
macro_rules! app_plugin {
    ($root_view:expr) => {
        static __FLUI_APP_PIPELINE: ::std::sync::OnceLock<
            ::std::sync::Mutex<$crate::PluginPipeline>,
        > = ::std::sync::OnceLock::new();

        /// Build a scene by running the full widget pipeline and return an opaque
        /// pointer to `Box<Scene>`.
        ///
        /// On the first call, mounts the root widget. Subsequent calls rebuild
        /// dirty elements and re-layout/repaint as needed.
        ///
        /// # Safety
        ///
        /// The returned pointer must be passed to `flui_app_drop` when no longer
        /// needed, or taken ownership of via `Box::from_raw`.
        #[no_mangle]
        pub extern "C" fn flui_app_build(width: f32, height: f32) -> *mut ::std::ffi::c_void {
            let mutex = __FLUI_APP_PIPELINE.get_or_init(|| {
                let pipeline = $crate::PluginPipeline::mount($root_view, width, height);
                ::std::sync::Mutex::new(pipeline)
            });
            let mut pipeline = mutex.lock().expect("PluginPipeline lock poisoned");
            let scene = pipeline.draw_frame(width, height);
            ::std::boxed::Box::into_raw(::std::boxed::Box::new(scene)) as *mut ::std::ffi::c_void
        }

        /// Returns the plugin version number.
        #[no_mangle]
        pub extern "C" fn flui_app_version() -> u32 {
            1
        }

        /// Drop a `Scene` previously returned by `flui_app_build`.
        ///
        /// # Safety
        ///
        /// `ptr` must be a valid pointer returned by `flui_app_build` that has
        /// not already been dropped. Passing null is safe (no-op).
        #[no_mangle]
        pub extern "C" fn flui_app_drop(ptr: *mut ::std::ffi::c_void) {
            if !ptr.is_null() {
                #[allow(unsafe_code)]
                unsafe {
                    drop(::std::boxed::Box::from_raw(ptr as *mut ::flui_layer::Scene));
                }
            }
        }
    };
}
