//! Owned UIKit protocol integration, not a simulated OS scene-reclamation claim.
use flui::app::{AppConfig, ServiceDefinition, ServiceLifetime};
use flui::prelude::*;
use flui::view::{LifecycleSubscription, RebuildHandle, RebuildReason};
use objc2::MainThreadMarker;
use objc2_ui_kit::{
    UIApplication, UIApplicationDelegate, UISceneConnectionOptions, UISceneDelegate, UIView,
    UIWindowScene,
};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tracing_subscriber::prelude::*;

thread_local! { static PAYLOAD: std::cell::RefCell<Option<(Rc<Cell<usize>>, RebuildHandle)>> = const { std::cell::RefCell::new(None) }; }
static LAST_LOCAL: AtomicUsize = AtomicUsize::new(0);
static PRESENTS: AtomicUsize = AtomicUsize::new(0);
static INITS: AtomicUsize = AtomicUsize::new(0);
static DISPOSES: AtomicUsize = AtomicUsize::new(0);
static SERVICE_DROPS: AtomicUsize = AtomicUsize::new(0);
struct ServiceRetirement;
impl Drop for ServiceRetirement {
    fn drop(&mut self) {
        SERVICE_DROPS.fetch_add(1, Ordering::SeqCst);
    }
}
static SERVICES: AtomicUsize = AtomicUsize::new(0);
static HISTORY: Mutex<Vec<String>> = Mutex::new(Vec::new());
struct GpuEvidence;
impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for GpuEvidence {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        struct Fields;
        impl tracing::field::Visit for Fields {
            fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "event" && value == "present_submitted" {
                    PRESENTS.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
        if event.metadata().target() == "flui.gpu" {
            event.record(&mut Fields);
        }
    }
}
fn later(callback: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(2));
        dispatch2::DispatchQueue::main().exec_async(callback);
    });
}
fn report(text: String) {
    std::fs::write(std::env::temp_dir().join("flui-execution-result.txt"), text)
        .expect("result marker");
}
#[derive(Clone, StatelessView)]
struct Root {
    persistent: Rc<Cell<usize>>,
}
impl StatelessView for Root {
    fn build(&self, _: &dyn BuildContext) -> impl IntoView {
        Theme::new(
            ThemeData::light(),
            Body {
                persistent: self.persistent.clone(),
            },
        )
    }
}
#[derive(Clone, StatefulView)]
struct Body {
    persistent: Rc<Cell<usize>>,
}
struct State {
    value: Rc<Cell<usize>>,
    rebuild: Option<RebuildHandle>,
    subscription: Option<LifecycleSubscription>,
}
impl StatefulView for Body {
    type State = State;
    fn create_state(&self) -> State {
        State {
            value: Rc::new(Cell::new(0)),
            rebuild: None,
            subscription: None,
        }
    }
}
impl ViewState<Body> for State {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        INITS.fetch_add(1, Ordering::SeqCst);
        self.rebuild = Some(ctx.rebuild_handle());
        let (_, subscription) = ctx
            .lifecycle_handle()
            .expect("mounted lifecycle")
            .subscribe(|state| HISTORY.lock().expect("history").push(format!("{state:?}")))
            .expect("live lifecycle");
        self.subscription = Some(subscription);
        self.value.set(7);
        PAYLOAD.with(|slot| {
            *slot.borrow_mut() = Some((
                self.value.clone(),
                self.rebuild.clone().expect("initialized"),
            ))
        });
    }
    fn dispose(&mut self) {
        DISPOSES.fetch_add(1, Ordering::SeqCst);
        HISTORY.lock().expect("history").push("disposed".into());
        if std::env::args().any(|arg| arg == "scene-app-quit") {
            panic!("deliberate ViewState disposal panic");
        }
    }
    fn build(&self, view: &Body, _: &dyn BuildContext) -> impl IntoView {
        LAST_LOCAL.store(self.value.get(), Ordering::SeqCst);
        view.persistent.set(view.persistent.get() + 1);
        let value = self.value.clone();
        let rebuild = self.rebuild.clone().expect("initialized");
        Center::new().child(
            ElevatedButton::new(Text::new(format!("Retained {}", self.value.get()))).on_pressed(
                move || {
                    value.set(value.get() + 1);
                    rebuild.schedule(RebuildReason::StateChange);
                },
            ),
        )
    }
}
fn main() {
    tracing_subscriber::registry().with(GpuEvidence).init();
    std::thread::spawn(|| {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while PRESENTS.load(Ordering::SeqCst) == 0 {
            if std::time::Instant::now() >= deadline {
                report("FAIL initial GPU present timeout".into());
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        dispatch2::DispatchQueue::main().exec_async(|| {
        PAYLOAD.with(|slot| {
            let (value, rebuild) = slot.borrow().as_ref().expect("mounted payload").clone();
            value.set(42);
            rebuild.schedule(RebuildReason::StateChange);
        });
        later(|| {
        let before = PRESENTS.load(Ordering::SeqCst);
        let mutated = LAST_LOCAL.load(Ordering::SeqCst) == 42;
        HISTORY.lock().expect("history").clear();
        let marker = MainThreadMarker::new().expect("owner");
        let scene = UIApplication::sharedApplication(marker).connectedScenes().into_iter().next().expect("real scene");
        let old_window = scene.downcast_ref::<UIWindowScene>().expect("window scene").windows().into_iter().find(|window| !window.isHidden()).expect("published native window");
        let old_view = old_window.rootViewController().expect("root controller").view().expect("mounted UIView");
        let old_view_address = std::ptr::from_ref::<UIView>(&old_view) as usize;
        drop(old_view);
        let delegate = scene.delegate().expect("owned delegate");
        delegate.sceneDidDisconnect(&scene);
        let detached = PRESENTS.load(Ordering::SeqCst);
        let native = dispatch2::MainThreadBound::new((scene, old_window, old_view_address), marker);
        later(move || {
            let quiet = PRESENTS.load(Ordering::SeqCst) == detached;
            let marker = MainThreadMarker::new().expect("owner");
            let (scene, old_window, old_view_address) = native.get(marker);
            let delegate = scene.delegate().expect("owned delegate retained");
            // SAFETY: empty native connection options for a controlled owned
            // delegate protocol call, on UIKit's main thread.
            let options = unsafe { UISceneConnectionOptions::new(marker) };
            delegate.scene_willConnectToSession_options(scene, &scene.session(), &options);
            delegate.sceneWillEnterForeground(scene);
            delegate.sceneDidBecomeActive(scene);
            let new_window = scene.downcast_ref::<UIWindowScene>().expect("window scene").windows().into_iter().find(|window| !window.isHidden()).expect("reconnected native window");
            let new_view = new_window.rootViewController().expect("new root controller").view().expect("reconnected UIView");
            let stable_view = std::ptr::from_ref::<UIView>(&new_view) as usize == *old_view_address;
            let replaced_window = !std::ptr::eq(&**old_window, &*new_window);
            // The retained old window prevents allocator-address reuse only
            // through comparison. All native witnesses drop before the next turn.
            later(move || {
                let after = PRESENTS.load(Ordering::SeqCst);
                let inits = INITS.load(Ordering::SeqCst);
                let disposes = DISPOSES.load(Ordering::SeqCst);
                let services = SERVICES.load(Ordering::SeqCst);
                let history = HISTORY.lock().expect("history").clone();
                let local = LAST_LOCAL.load(Ordering::SeqCst);
                let passed = stable_view && replaced_window && mutated && local == 42 && before > 1 && quiet && after > detached && inits == 1 && disposes == 0 && services == 1
                    && history.iter().any(|s| s == "Detached") && history.last().is_some_and(|s| s == "Resumed");
                let fresh = std::env::args().any(|arg| arg == "scene-app-fresh");
                let quit = std::env::args().any(|arg| arg == "scene-app-quit");
                if !passed || (!fresh && !quit) {
                report(format!("{} case=scene-app-retention before={before} detached={detached} after={after} quiet={quiet} stable_view={stable_view} replaced_window={replaced_window} mutated={mutated} local={local} inits={inits} disposes={disposes} services={services} history={history:?}", if passed { "PASS" } else { "FAIL" }));
                }
                if passed && quit {
                    let marker = MainThreadMarker::new().expect("owner");
                    let app = UIApplication::sharedApplication(marker);
                    // SAFETY: this fixture owns the UIApplication delegate and
                    // calls it synchronously on UIKit main; the returned object
                    // is retained through this protocol notification.
                    unsafe { app.delegate() }.expect("owned process delegate").applicationWillTerminate(&app);
                    let disposes = DISPOSES.load(Ordering::SeqCst);
                    let retired = SERVICE_DROPS.load(Ordering::SeqCst);
                    let history = HISTORY.lock().expect("history").clone();
                    let ordered = history.windows(2).any(|pair| pair == ["Detached", "disposed"]);
                    report(format!("{} case=scene-app-quit disposes={disposes} service_drops={retired} terminal_before_dispose={ordered}", if disposes == 1 && retired == 1 && ordered { "PASS" } else { "FAIL" }));
                }
                if passed && fresh {
                    let marker = MainThreadMarker::new().expect("owner");
                    let app = UIApplication::sharedApplication(marker);
                    let scene = app.connectedScenes().into_iter().next().expect("connected scene");
                    let error = block2::RcBlock::new(|error: std::ptr::NonNull<objc2_foundation::NSError>| {
                        // SAFETY: UIKit passes a valid NSError during this callback.
                        report(format!("FAIL native session destruction: {}", unsafe { error.as_ref() }.localizedDescription()));
                    });
                    app.requestSceneSessionDestruction_options_errorHandler(&scene.session(), None, Some(&error));
                    later(|| {
                        let marker = MainThreadMarker::new().expect("owner");
                        let error = block2::RcBlock::new(|error: std::ptr::NonNull<objc2_foundation::NSError>| {
                            // SAFETY: UIKit passes a valid NSError during this callback.
                            report(format!("FAIL native session activation: {}", unsafe { error.as_ref() }.localizedDescription()));
                        });
                        #[allow(deprecated)]
                        UIApplication::sharedApplication(marker).requestSceneSessionActivation_userActivity_options_errorHandler(None, None, None, Some(&error));
                        later(|| {
                            let inits = INITS.load(Ordering::SeqCst);
                            let disposes = DISPOSES.load(Ordering::SeqCst);
                            let services = SERVICES.load(Ordering::SeqCst);
                            let local = LAST_LOCAL.load(Ordering::SeqCst);
                            let passed = inits == 2 && disposes == 1 && services == 1 && local == 7;
                            report(format!("{} case=scene-app-fresh inits={inits} disposes={disposes} services={services} local={local}", if passed { "PASS" } else { "FAIL" }));
                        });
                    });
                }
            });
        });
    });
    });
    });
    let config = AppConfig::new().with_service(ServiceDefinition::new(
        "scene-probe",
        ServiceLifetime::KeepsAppAlive,
        |context| {
            SERVICES.fetch_add(1, Ordering::SeqCst);
            let retirement = ServiceRetirement;
            Box::pin(async move {
                let _retirement = retirement;
                context.cancellation().cancelled().await;
            })
        },
    ));
    flui::app::run_app_with_config(
        Root {
            persistent: Rc::new(Cell::new(0)),
        },
        config,
    );
}
