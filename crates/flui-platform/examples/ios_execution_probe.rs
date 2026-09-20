//! UIKit delegate protocol probe, not an OS background-transition test.
//! A bounded external runner reads `tmp/flui-execution-result.txt` in the app
//! data container, then terminates the owned probe (UIApplicationMain does not return).
#[cfg(target_os = "ios")]
mod native {
    use flui_platform::Platform;
    use objc2::MainThreadMarker;
    use objc2_ui_kit::{UIApplication, UISceneDelegate};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use std::time::Duration;

    fn later(callback: impl FnOnce() + Send + 'static) {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(700));
            dispatch2::DispatchQueue::main().exec_async(callback);
        });
    }

    fn after_first_frame(frames: Arc<AtomicUsize>, callback: impl FnOnce() + Send + 'static) {
        std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while frames.load(Ordering::SeqCst) == 0 {
                if std::time::Instant::now() >= deadline {
                    report("FAIL initial CADisplayLink deadline (10 seconds)");
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            dispatch2::DispatchQueue::main().exec_async(callback);
        });
    }

    fn report(message: &str) {
        let path = std::env::temp_dir().join("flui-execution-result.txt");
        if let Err(error) = std::fs::write(&path, message) {
            eprintln!("probe result write failed: {error}");
        }
        println!("FLUI_EXECUTION_RESULT {message}");
    }

    fn observe(event: &str) {
        let marker = MainThreadMarker::new().expect("main queue");
        let application = UIApplication::sharedApplication(marker);
        let scene = application
            .connectedScenes()
            .into_iter()
            .next()
            .expect("connected scene");
        let delegate = scene.delegate().expect("FLUI scene delegate");
        match event {
            "inactive" => delegate.sceneWillResignActive(&scene),
            "active" => delegate.sceneDidBecomeActive(&scene),
            "background" => delegate.sceneDidEnterBackground(&scene),
            "foreground" => delegate.sceneWillEnterForeground(&scene),
            _ => unreachable!("probe event"),
        }
    }

    #[expect(
        unsafe_code,
        reason = "inspect retained UIKit view from the owner-thread raw handle"
    )]
    fn inspect_scene(
        window: &dyn flui_platform::PlatformWindow,
        disconnect: bool,
    ) -> (String, bool) {
        let borrowed = window.window_handle().expect("live owner window handle");
        let raw_window_handle::RawWindowHandle::UiKit(handle) = borrowed.as_raw() else {
            panic!("expected UIKit handle");
        };
        // SAFETY: the borrowed raw handle keeps its logical owner alive; this
        // owner-thread call only inspects the documented UIView pointer.
        let view = unsafe { &*handle.ui_view.as_ptr().cast::<objc2_ui_kit::UIView>() };
        let Some(native_window) = view.window() else {
            return ("no-window".into(), false);
        };
        // SAFETY: UIWindow implements windowScene on every supported iOS target;
        // inspect nullability only, without claiming ownership of the scene.
        let scene: *mut objc2::runtime::AnyObject =
            unsafe { objc2::msg_send![&*native_window, windowScene] };
        if scene.is_null() {
            return ("no-scene".into(), false);
        }
        // SAFETY: the retained UIWindow owns this scene association during this
        // non-reentrant owner-thread query. Read the delegate's class only.
        let delegate: *mut objc2::runtime::AnyObject = unsafe { objc2::msg_send![scene, delegate] };
        if delegate.is_null() {
            return ("no-delegate".into(), false);
        }
        let class = unsafe { &*delegate }
            .class()
            .name()
            .to_string_lossy()
            .into_owned();
        // SAFETY: retain both actual objects before the callback may detach them;
        // only send the optional selector after checking support on that object.
        let supported = unsafe {
            let scene = objc2::rc::Retained::retain(scene).expect("live scene");
            let delegate = objc2::rc::Retained::retain(delegate).expect("live delegate");
            let responds: bool =
                objc2::msg_send![&*delegate, respondsToSelector: objc2::sel!(sceneDidDisconnect:)];
            if disconnect && responds && class == "FluiSceneDelegate" {
                let _: () = objc2::msg_send![&*delegate, sceneDidDisconnect: &*scene];
                true
            } else {
                false
            }
        };
        (class, supported)
    }

    struct DestructionSubscriber(Arc<AtomicUsize>);
    impl tracing::Subscriber for DestructionSubscriber {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            struct Message(String);
            impl tracing::field::Visit for Message {
                fn record_debug(&mut self, _: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                    use std::fmt::Write as _;
                    let _ = write!(self.0, "{value:?}");
                }
            }
            let mut text = Message(String::new());
            event.record(&mut text);
            if text.0.contains("UIKit rejected scene destruction") {
                self.0.fetch_add(1, Ordering::SeqCst);
                panic!("destruction error subscriber");
            }
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    pub(super) fn run() {
        let case = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "transient".into());
        let destruction_errors = Arc::new(AtomicUsize::new(0));
        if case == "scene-destruction-error-panic" {
            tracing::subscriber::set_global_default(DestructionSubscriber(
                destruction_errors.clone(),
            ))
            .expect("isolated subscriber");
        }
        let frames = Arc::new(AtomicUsize::new(0));
        let surfaces = Arc::new(Mutex::new(Vec::new()));
        let executions = Arc::new(Mutex::new(Vec::new()));
        let panic_calls = Arc::new(AtomicUsize::new(0));
        let nested_frames = Arc::new(AtomicUsize::new(usize::MAX));
        let focus_mismatch = Arc::new(AtomicBool::new(false));
        let owner_proxy = Arc::new(Mutex::new(None::<flui_platform::PlatformProxy>));
        let wake_calls = Arc::new(AtomicUsize::new(0));
        let quit_calls = Arc::new(AtomicUsize::new(0));
        let wrong_thread = Arc::new(AtomicBool::new(false));
        let ready_proxy = Arc::clone(&owner_proxy);
        let ready_wakes = Arc::clone(&wake_calls);
        let ready_quits = Arc::clone(&quit_calls);
        let ready_wrong = Arc::clone(&wrong_thread);
        let bootstrap_panic = case == "bootstrap-panic";
        let bootstrap_drops = Arc::new(AtomicUsize::new(0));
        let bootstrap_witness = bootstrap_drops.clone();
        let platform = flui_platform::IOSPlatform::new().expect("platform");
        // Accepted scene consumers are owner-local: an Rc capture is valid
        // without making the native platform itself !Send or !Sync.
        let local_connections = std::rc::Rc::new(std::cell::Cell::new(0_usize));
        let connection_count = Arc::new(AtomicUsize::new(0));
        let aborted_count = Arc::new(AtomicUsize::new(0));
        let mut started = false;
        platform.on_scene_event(Box::new(move |event| {
            if matches!(event, flui_platform::platforms::ios::IOSSceneEvent::InstallationAborted { .. }) { aborted_count.fetch_add(1, Ordering::SeqCst); }
            let flui_platform::platforms::ios::IOSSceneEvent::Connected { window, .. } = event else { return Ok(()) };
            local_connections.set(local_connections.get() + 1);
            connection_count.fetch_add(1, Ordering::SeqCst);
            if case == "scene-install-retry" {
                if !started {
                    started = true;
                    later(move || {
                        let marker = MainThreadMarker::new().expect("owner");
                        let scene = UIApplication::sharedApplication(marker).connectedScenes().into_iter().next().expect("scene");
                        let delegate = scene.delegate().expect("delegate");
                        #[expect(unsafe_code, reason = "owned delegate protocol options")]
                        let options = unsafe { objc2_ui_kit::UISceneConnectionOptions::new(marker) };
                        delegate.scene_willConnectToSession_options(&scene, &scene.session(), &options);
                        delegate.sceneWillEnterForeground(&scene);
                        delegate.sceneDidBecomeActive(&scene);
                    });
                    return Err("deliberate retryable first install failure".into());
                }
                let count = connection_count.clone();
                let aborts = aborted_count.clone();
                later(move || {
                    let connections = count.load(Ordering::SeqCst);
                    let available = window.window_handle().is_ok();
                    let aborts = aborts.load(Ordering::SeqCst);
                    report(&format!("{} case=scene-install-retry connections={connections} aborts={aborts} available={available}", if connections == 2 && aborts == 1 && available { "PASS" } else { "FAIL" }));
                });
                return Ok(());
            }
            if started { return Ok(()) }
            started = true;
            if matches!(case.as_str(), "scene-install-close-failure" | "scene-install-failure" | "scene-install-panic" | "scene-install-close" | "scene-install-quit") {
                let closes = Arc::new(AtomicUsize::new(0));
                let close_calls = Arc::clone(&closes);
                window.on_close(Box::new(move || { close_calls.fetch_add(1, Ordering::SeqCst); }));
                let ticks = Arc::new(AtomicUsize::new(0));
                let frame_calls = Arc::clone(&ticks);
                window.on_request_frame(Box::new(move || { frame_calls.fetch_add(1, Ordering::SeqCst); }));
                let before_visible = window.is_visible();
                let native_hidden = {
                    let borrowed = window.window_handle().expect("provisional owner handle");
                    let raw_window_handle::RawWindowHandle::UiKit(handle) = borrowed.as_raw() else { panic!("UIKit handle") };
                    // SAFETY: installer runs on main and `window` retains the
                    // provisional UIView. Inspect real UIKit visibility, not
                    // the lifecycle cache that only changes after admission.
                    #[expect(unsafe_code, reason = "native publication witness")]
                    let view = unsafe { &*handle.ui_view.as_ptr().cast::<objc2_ui_kit::UIView>() };
                    view.window().is_none_or(|native| native.isHidden())
                };
                let witness = Arc::clone(&window);
                let name = case.clone();
                let connections = connection_count.clone();
                later(move || {
                    if name == "scene-install-close-failure" {
                        let marker = MainThreadMarker::new().expect("owner");
                        let scene = UIApplication::sharedApplication(marker).connectedScenes().into_iter().next().expect("scene");
                        #[expect(unsafe_code, reason = "owned delegate protocol options")]
                        let options = unsafe { objc2_ui_kit::UISceneConnectionOptions::new(marker) };
                        scene.delegate().expect("delegate").scene_willConnectToSession_options(&scene, &scene.session(), &options);
                    }
                    let closes = closes.load(Ordering::SeqCst);
                    let frames = ticks.load(Ordering::SeqCst);
                    let closed = witness.window_handle().is_err();
                    let visible = witness.is_visible();
                    let connections = connections.load(Ordering::SeqCst);
                    let passed = connections == 1 && native_hidden && !before_visible && !visible && closed && closes == 1 && frames == 0;
                    report(&format!("{} case={name} native_hidden={native_hidden} before_visible={before_visible} visible={visible} closed={closed} closes={closes} frames={frames} connections={connections}", if passed { "PASS" } else { "FAIL" }));
                });
                match case.as_str() {
                    "scene-install-close-failure" => { window.close(); return Err("failure after explicit close".into()); }
                    "scene-install-failure" => return Err("deliberate installer failure".into()),
                    "scene-install-panic" => panic!("deliberate installer panic"),
                    "scene-install-close" => window.close(),
                    "scene-install-quit" => owner_proxy.lock().expect("proxy").as_ref().expect("owner ready").request_quit().expect("quit admitted"),
                    _ => unreachable!(),
                }
                return Ok(());
            }
            let owner_proxy = Arc::clone(&owner_proxy);
            let wake_calls = Arc::clone(&wake_calls);
            let quit_calls = Arc::clone(&quit_calls);
            let wrong_thread = Arc::clone(&wrong_thread);
            let destruction_errors = destruction_errors.clone();
            let connection_count = connection_count.clone();
            let case = case.clone();
            let frames = Arc::clone(&frames);
            let surfaces = Arc::clone(&surfaces);
            let executions = Arc::clone(&executions);
            let panic_calls = Arc::clone(&panic_calls);
            let nested_frames = Arc::clone(&nested_frames);
            let focus_mismatch = Arc::clone(&focus_mismatch);

            let focus_window = Arc::clone(&window);
            let mismatched = Arc::clone(&focus_mismatch);
            window.on_active_status_change(Box::new(move |focused| {
                if focus_window.is_focused() != focused { mismatched.store(true, Ordering::SeqCst); }
            }));
            let execution_history = Arc::clone(&executions);
            window.on_execution_state_change(Box::new(move |state| execution_history.lock().expect("execution history").push(state)));
            let counter = Arc::clone(&frames);
            window.on_request_frame(Box::new(move || { counter.fetch_add(1, Ordering::SeqCst); }));
            let history = Arc::clone(&surfaces);
            window.on_surface_status_change(Box::new(move |available| {
                history.lock().expect("history").push(available);
            }));
            after_first_frame(frames.clone(), move || {
                let before = frames.load(Ordering::SeqCst);
                if before == 0 {
                    report("FAIL no initial CADisplayLink frames");
                    return;
                }
                if case == "scene-destruction-error-panic" {
                    window.close();
                    later(move || {
                        let calls = destruction_errors.load(Ordering::SeqCst);
                        report(&format!("{} case=scene-destruction-error-panic calls={calls}", if calls == 1 { "PASS" } else { "FAIL" }));
                    });
                    return;
                }
                if case == "scene-close-reconnect" {
                    let marker = MainThreadMarker::new().expect("owner");
                    let scene = UIApplication::sharedApplication(marker).connectedScenes().into_iter().next().expect("scene");
                    let delegate = scene.delegate().expect("delegate");
                    window.close();
                    #[expect(unsafe_code, reason = "owned delegate protocol options")]
                    let options = unsafe { objc2_ui_kit::UISceneConnectionOptions::new(marker) };
                    delegate.scene_willConnectToSession_options(&scene, &scene.session(), &options);
                    let connections = connection_count.load(Ordering::SeqCst);
                    report(&format!("{} case=scene-close-reconnect connections={connections}", if connections == 1 { "PASS" } else { "FAIL" }));
                    return;
                }
                if case == "scene-close-nested-loop" {
                    let nested = Arc::new(AtomicUsize::new(usize::MAX));
                    let witness = nested.clone();
                    let observed_frames = frames.clone();
                    let owner = window.clone();
                    let once = AtomicBool::new(false);
                    window.on_active_status_change(Box::new(move |focused| {
                        if !focused && !once.swap(true, Ordering::SeqCst) {
                            owner.close();
                            let baseline = observed_frames.load(Ordering::SeqCst);
                            let deadline = objc2_foundation::NSDate::dateWithTimeIntervalSinceNow(0.2);
                            objc2_foundation::NSRunLoop::currentRunLoop().runUntilDate(&deadline);
                            witness.store(observed_frames.load(Ordering::SeqCst) - baseline, Ordering::SeqCst);
                        }
                    }));
                    observe("inactive");
                    let ticks = nested.load(Ordering::SeqCst);
                    report(&format!("{} case=scene-close-nested-loop nested_frames={ticks} closed={}", if ticks == 0 && window.window_handle().is_err() { "PASS" } else { "FAIL" }, window.window_handle().is_err()));
                    return;
                }
                if case == "scene-close-two-capture-panics" {
                    struct Capture(Arc<AtomicUsize>);
                    impl Drop for Capture { fn drop(&mut self) { self.0.fetch_add(1, Ordering::SeqCst); panic!("separate retired capture"); } }
                    let drops = Arc::new(AtomicUsize::new(0));
                    let first = Capture(drops.clone());
                    let second = Capture(drops.clone());
                    window.on_active_status_change(Box::new(move |_| { let _ = &first; }));
                    window.on_visibility_status_change(Box::new(move |_| { let _ = &second; }));
                    let raw_window_handle::RawWindowHandle::UiKit(handle) = window.window_handle().expect("before close").as_raw() else { panic!("UIKit handle") };
                    window.close();
                    // SAFETY: logical window remains retained here and owns its
                    // stable UIView even after terminal native detachment.
                    #[expect(unsafe_code, reason = "native retirement witness")]
                    let detached = unsafe { &*handle.ui_view.as_ptr().cast::<objc2_ui_kit::UIView>() }.window().is_none();
                    let drops = drops.load(Ordering::SeqCst);
                    report(&format!("{} case=scene-close-two-capture-panics drops={drops} detached={detached} closed={}", if drops == 2 && detached && window.window_handle().is_err() { "PASS" } else { "FAIL" }, window.window_handle().is_err()));
                    return;
                }
                if case == "scene-close-panic" {
                    struct Capture { calls: Arc<AtomicUsize>, window: Arc<dyn flui_platform::PlatformWindow> }
                    impl Drop for Capture {
                        fn drop(&mut self) {
                            self.calls.fetch_add(1, Ordering::SeqCst);
                            self.window.close();
                            panic!("deliberate retired capture panic");
                        }
                    }
                    let drops = Arc::new(AtomicUsize::new(0));
                    let capture = Capture { calls: drops.clone(), window: window.clone() };
                    window.on_active_status_change(Box::new(move |_| { let _ = &capture; }));
                    let closes = Arc::new(AtomicUsize::new(0));
                    let count = closes.clone();
                    window.on_close(Box::new(move || { count.fetch_add(1, Ordering::SeqCst); panic!("deliberate close callback panic"); }));
                    window.close();
                    let paused = frames.load(Ordering::SeqCst);
                    later(move || {
                        let closed = window.window_handle().is_err();
                        let delivered = executions.lock().expect("execution history").clone();
                        let after = frames.load(Ordering::SeqCst);
                        let closes = closes.load(Ordering::SeqCst);
                        let drops = drops.load(Ordering::SeqCst);
                        let passed = closed && delivered.last() == Some(&flui_platform::WindowExecutionState::Detached) && closes == 1 && drops == 1 && after == paused;
                        report(&format!("{} case=scene-close-panic closed={closed} delivered={delivered:?} closes={closes} drops={drops} frames={paused}->{after}", if passed { "PASS" } else { "FAIL" }));
                    });
                    return;
                }
                if case == "scene-foreign-link" {
                    let borrowed = window.window_handle().expect("attached handle");
                    let raw_window_handle::RawWindowHandle::UiKit(handle) = borrowed.as_raw() else { panic!("UIKit handle") };
                    // SAFETY: both UIView and new CADisplayLink are live on main.
                    // This distinct native link has never belonged to the attachment.
                    #[expect(unsafe_code, reason = "native link identity protocol oracle")]
                    unsafe {
                        let view = &*handle.ui_view.as_ptr().cast::<objc2_ui_kit::UIView>();
                        let foreign = objc2_quartz_core::CADisplayLink::displayLinkWithTarget_selector(view, objc2::sel!(onDisplayLink:));
                        let _: () = objc2::msg_send![view, onDisplayLink: &*foreign];
                        foreign.invalidate();
                    }
                    let after = frames.load(Ordering::SeqCst);
                    report(&format!("{} case=scene-foreign-link frames={before}->{after}", if before == after { "PASS" } else { "FAIL" }));
                    return;
                }
                if case == "scene-worker-drop" {
                    let marker = MainThreadMarker::new().expect("owner thread");
                    let borrowed = window.window_handle().expect("attached handle");
                    let raw_window_handle::RawWindowHandle::UiKit(handle) = borrowed.as_raw() else {
                        panic!("UIKit handle");
                    };
                    // SAFETY: the owner is retained by `window`, and this is the
                    // UIKit owner thread. Weak records lifetime without keeping
                    // the stable UIView alive itself.
                    #[expect(unsafe_code, reason = "native weak lifetime witness")]
                    let weak = unsafe {
                        objc2::rc::Weak::new(&*handle.ui_view.as_ptr().cast::<objc2_ui_kit::UIView>())
                    };
                    let weak = dispatch2::MainThreadBound::new(weak, marker);
                    window.close();
                    let alive_after_close = weak.get(marker).load().is_some();
                    let unavailable = window.window_handle().is_err();
                    // Deliberately join from main: a synchronous main-thread
                    // destructor would deadlock and the external deadline fails.
                    std::thread::spawn(move || drop(window)).join().expect("worker drop");
                    later(move || {
                        let marker = MainThreadMarker::new().expect("owner thread");
                        let released = weak.get(marker).load().is_none();
                        report(&format!("{} case=scene-worker-drop alive_after_close={alive_after_close} unavailable={unavailable} released={released}", if alive_after_close && unavailable && released { "PASS" } else { "FAIL" }));
                    });
                    return;
                }
                if case == "scene-worker-quit" {
                    let (_, supported) = inspect_scene(window.as_ref(), true);
                    let baseline = frames.load(Ordering::SeqCst);
                    let proxy = owner_proxy.lock().expect("proxy").as_ref().expect("owner ready").clone();
                    let worker_proxy = proxy.clone();
                    let sent = Arc::new(AtomicBool::new(false));
                    let worker_sent = Arc::clone(&sent);
                    std::thread::spawn(move || {
                        let wake = worker_proxy.wake().is_ok();
                        std::thread::sleep(Duration::from_millis(100));
                        let quit = worker_proxy.request_quit().is_ok();
                        worker_sent.store(wake && quit, Ordering::SeqCst);
                    });
                    later(move || {
                        let wakes = wake_calls.load(Ordering::SeqCst);
                        let quits = quit_calls.load(Ordering::SeqCst);
                        let after = frames.load(Ordering::SeqCst);
                        let stale_rejected = proxy.wake().is_err() && proxy.request_quit().is_err();
                        let passed = supported && sent.load(Ordering::SeqCst) && wakes == 1 && quits == 1
                            && !wrong_thread.load(Ordering::SeqCst) && after == baseline && stale_rejected;
                        report(&format!("{} case=scene-worker-quit wakes={wakes} quits={quits} before={baseline} after={after} stale_rejected={stale_rejected}", if passed { "PASS" } else { "FAIL" }));
                    });
                    return;
                }
                if case == "scene-disconnect-reentrant" || case == "scene-disconnect-foreground" {
                    let borrowed = window.window_handle().expect("initial handle");
                    let raw_window_handle::RawWindowHandle::UiKit(handle) = borrowed.as_raw() else { panic!("UIKit handle") };
                    let view_address = handle.ui_view.as_ptr() as usize;
                    let attached_on_release = Arc::new(AtomicBool::new(false));
                    let witness = attached_on_release.clone();
                    let history = surfaces.clone();
                    surfaces.lock().expect("surfaces").clear();
                    window.on_surface_status_change(Box::new(move |available| {
                        history.lock().expect("surfaces").push(available);
                        if !available {
                            // SAFETY: logical window retained by the outer probe owns this
                            // stable UIView; callback executes on the native owner thread.
                            #[expect(unsafe_code, reason = "native attachment ordering witness")]
                            let view = unsafe { &*(view_address as *const objc2_ui_kit::UIView) };
                            witness.store(view.window().is_some(), Ordering::SeqCst);
                        }
                    }));
                    if case == "scene-disconnect-foreground" {
                        window.on_execution_state_change(Box::new(move |state| {
                            if state == flui_platform::WindowExecutionState::Detached {
                                observe("foreground");
                                observe("active");
                            }
                        }));
                        inspect_scene(window.as_ref(), true);
                    } else {
                        let owner = window.clone();
                        let once = AtomicBool::new(false);
                        window.on_active_status_change(Box::new(move |focused| {
                            if !focused && !once.swap(true, Ordering::SeqCst) {
                                inspect_scene(owner.as_ref(), true);
                            }
                        }));
                        observe("inactive");
                    }
                    let baseline = frames.load(Ordering::SeqCst);
                    later(move || {
                        let attached = attached_on_release.load(Ordering::SeqCst);
                        let delivered = surfaces.lock().expect("surfaces").clone();
                        let execution = window.execution_state();
                        let after = frames.load(Ordering::SeqCst);
                        let passed = attached && delivered == [false] && execution == flui_platform::WindowExecutionState::Detached
                            && window.window_handle().is_err() && baseline == after;
                        report(&format!("{} case={case} attached_on_release={attached} surfaces={delivered:?} execution={execution:?} frames={baseline}->{after}", if passed { "PASS" } else { "FAIL" }));
                    });
                    return;
                }
                if case == "scene-disconnect" {
                    let (delegate, supported) = inspect_scene(window.as_ref(), true);
                    let after_callbacks = frames.load(Ordering::SeqCst);
                    // A late notification for the retired attachment must not
                    // recreate a surface or resume frames without reconnection.
                    observe("foreground");
                    observe("active");
                    later(move || {
                        let after = frames.load(Ordering::SeqCst);
                        let execution = window.execution_state();
                        let handle_available = window.window_handle().is_ok();
                        let passed = supported && execution == flui_platform::WindowExecutionState::Detached
                            && after == after_callbacks && !handle_available;
                        report(&format!("{} case=scene-disconnect delegate={delegate} supported={supported} before={before} after_callbacks={after_callbacks} after={after} execution={execution:?} handle_available={handle_available}", if passed { "PASS" } else { "FAIL" }));
                    });
                    return;
                }
                if case == "scene-owned" {
                    let (delegate, _) = inspect_scene(window.as_ref(), false);
                    report(&format!("{} case=scene-owned frames={before} scene_delegate={delegate}", if delegate == "FluiSceneDelegate" { "PASS" } else { "FAIL" }));
                    return;
                }
                surfaces.lock().expect("history").clear();
                if matches!(case.as_str(), "background-nested-active" | "background-nested-inactive" | "nested-foreground-then-panic") {
                    let history = Arc::clone(&executions);
                    let once = AtomicBool::new(false);
                    let invoked = Arc::clone(&panic_calls);
                    let mode = case.clone();
                    window.on_execution_state_change(Box::new(move |state| {
                        history.lock().expect("execution history").push(state);
                        if state == flui_platform::WindowExecutionState::Suspended && !once.swap(true, Ordering::SeqCst) {
                            if mode == "nested-foreground-then-panic" {
                                observe("foreground");
                                invoked.fetch_add(1, Ordering::SeqCst);
                                panic!("nested foreground observer panic");
                            }
                            observe(if mode.ends_with("inactive") { "inactive" } else { "active" });
                        }
                    }));
                    observe("background");
                } else if case == "background-nested-run-loop" {
                    let frames = Arc::clone(&frames);
                    let witnessed = Arc::clone(&nested_frames);
                    let once = AtomicBool::new(false);
                    window.on_active_status_change(Box::new(move |focused| {
                        if !focused && !once.swap(true, Ordering::SeqCst) {
                            observe("background");
                            let before = frames.load(Ordering::SeqCst);
                            let deadline = objc2_foundation::NSDate::dateWithTimeIntervalSinceNow(0.15);
                            objc2_foundation::NSRunLoop::currentRunLoop().runUntilDate(&deadline);
                            witnessed.store(frames.load(Ordering::SeqCst) - before, Ordering::SeqCst);
                        }
                    }));
                    observe("inactive");
                } else if case == "superseded-foreground" {
                    let history = Arc::clone(&executions);
                    let once = AtomicBool::new(false);
                    window.on_execution_state_change(Box::new(move |state| {
                        history.lock().expect("execution history").push(state);
                        if state == flui_platform::WindowExecutionState::Suspended && !once.swap(true, Ordering::SeqCst) {
                            observe("foreground");
                            observe("background");
                        }
                    }));
                    observe("background");
                } else if case.starts_with("foreground-nested-") {
                    observe("background");
                    surfaces.lock().expect("history").clear();
                    let history = Arc::clone(&surfaces);
                    let once = AtomicBool::new(false);
                    let mode = case.clone();
                    window.on_surface_status_change(Box::new(move |available| {
                        history.lock().expect("history").push(available);
                        if available && !once.swap(true, Ordering::SeqCst) {
                            observe(if mode.ends_with("inactive") { "inactive" } else { "active" });
                        }
                    }));
                    observe("foreground");
                } else if case == "duplicate-foreground" {
                    observe("active");
                    observe("foreground");
                    observe("foreground");
                } else if case == "foreground-callback-panic" {
                    observe("background");
                    surfaces.lock().expect("history").clear();
                    let invoked = Arc::clone(&panic_calls);
                    window.on_surface_status_change(Box::new(move |available| {
                        invoked.fetch_add(1, Ordering::SeqCst);
                        assert!(!available, "probe surface observer panic");
                    }));
                    observe("foreground");
                } else if case == "foreground-reentered-background" {
                    observe("background");
                    surfaces.lock().expect("history").clear();
                    let history = Arc::clone(&surfaces);
                    let once = AtomicBool::new(false);
                    window.on_surface_status_change(Box::new(move |available| {
                        history.lock().expect("history").push(available);
                        if available && !once.swap(true, Ordering::SeqCst) { observe("background"); }
                    }));
                    observe("foreground");
                } else if case == "background-reentered-foreground" {
                    let once = AtomicBool::new(false);
                    window.on_execution_state_change(Box::new(move |state| {
                        if state == flui_platform::WindowExecutionState::Suspended && !once.swap(true, Ordering::SeqCst) { observe("foreground"); }
                    }));
                    observe("background");
                } else {
                    observe("inactive");
                    observe("active");
                }
                let after_callbacks = frames.load(Ordering::SeqCst);
                later(move || {
                    let after = frames.load(Ordering::SeqCst);
                    let history = surfaces.lock().expect("history").clone();
                    let preserved = !history.contains(&false);
                    let progressed = after > after_callbacks;
                    let execution = window.execution_state();
                    let delivered = executions.lock().expect("execution history").clone();
                    let passed = if case.starts_with("background-nested-") || case == "superseded-foreground" {
                        execution == flui_platform::WindowExecutionState::Suspended && !progressed && history == [false]
                            && delivered.last() == Some(&execution) && !window.is_focused() && !window.is_visible()
                    } else if case.starts_with("foreground-nested-") {
                        execution == flui_platform::WindowExecutionState::Running && progressed && history == [true]
                            && delivered.last() == Some(&execution) && window.is_focused() == case.ends_with("-active") && window.is_visible()
                    } else if case == "nested-foreground-then-panic" {
                        execution == flui_platform::WindowExecutionState::Running && progressed && history == [true]
                            && delivered.last() == Some(&execution) && panic_calls.load(Ordering::SeqCst) == 1 && window.is_visible()
                    } else if case == "foreground-reentered-background" {
                        execution == flui_platform::WindowExecutionState::Suspended && !progressed && history == [true, false]
                    } else if case == "foreground-callback-panic" {
                        execution == flui_platform::WindowExecutionState::Running && progressed && panic_calls.load(Ordering::SeqCst) == 1
                    } else if case == "duplicate-foreground" {
                        execution == flui_platform::WindowExecutionState::Running && window.is_focused() && preserved && progressed
                    } else {
                        execution == flui_platform::WindowExecutionState::Running && preserved && progressed
                    };
                    let passed = passed && !focus_mismatch.load(Ordering::SeqCst)
                        && (case != "background-nested-run-loop" || nested_frames.load(Ordering::SeqCst) == 0);
                    report(&format!("{} case={case} baseline={before} after_callbacks={after_callbacks} after={after} preserved={preserved} progressed={progressed} execution={execution:?} delivered={delivered:?} surfaces={history:?}", if passed { "PASS" } else { "FAIL" }));
                    drop(window);
                });
            });
            Ok(())
        })).expect("scene consumer");
        Box::new(platform)
            .run(Box::new(move |owner| {
                let thread = std::thread::current().id();
                *ready_proxy.lock().expect("proxy") = Some(owner.proxy());
                let wrong = Arc::clone(&ready_wrong);
                owner.on_wake(Box::new(move || {
                    ready_wakes.fetch_add(1, Ordering::SeqCst);
                    if std::thread::current().id() != thread {
                        wrong.store(true, Ordering::SeqCst);
                    }
                }))?;
                owner.shared().on_quit(Box::new(move || {
                    let quits = ready_quits.fetch_add(1, Ordering::SeqCst) + 1;
                    if std::thread::current().id() != thread {
                        ready_wrong.store(true, Ordering::SeqCst);
                    }
                    if bootstrap_panic {
                        let drops = bootstrap_witness.load(Ordering::SeqCst);
                        report(&format!(
                            "{} case=bootstrap-panic quits={quits} drops={drops}",
                            if quits == 1 && drops == 1 {
                                "PASS"
                            } else {
                                "FAIL"
                            }
                        ));
                    }
                }));
                if bootstrap_panic {
                    struct Retire(Arc<AtomicUsize>);
                    impl Drop for Retire {
                        fn drop(&mut self) {
                            self.0.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                    let _retire = Retire(bootstrap_drops);
                    panic!("deliberate process bootstrap panic");
                }
                Ok(())
            }))
            .expect("UIKit run");
    }
}
fn main() {
    #[cfg(target_os = "ios")]
    native::run();
    #[cfg(not(target_os = "ios"))]
    eprintln!("This protocol probe requires an iOS simulator.");
}
