//! Standalone sole-facade native resident-reopen acceptance application.
use flui::app::{
    AppConfig, AppHandle, Application, ExitPolicy, ServiceDefinition, ServiceLifetime,
    StartupWindow,
};
use flui::prelude::*;
use flui::view::{RebuildHandle, RebuildReason};
use flui::widgets::column;
use std::{
    cell::Cell,
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};

static FACTORIES: AtomicUsize = AtomicUsize::new(0);
static INITIALIZED: AtomicUsize = AtomicUsize::new(0);
static DISPOSED: AtomicUsize = AtomicUsize::new(0);
static SERVICE_DROPS: AtomicUsize = AtomicUsize::new(0);
struct ServiceDrop;
impl Drop for ServiceDrop {
    fn drop(&mut self) {
        SERVICE_DROPS.fetch_add(1, Ordering::SeqCst);
        println!("RESIDENT_SERVICE_DROPPED");
    }
}
static SERVICE_STARTS: AtomicUsize = AtomicUsize::new(0);

fn main() {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter("warn,flui.gpu=trace")
        .init();
    println!("RESIDENT_PID={}", std::process::id());
    let config = AppConfig::new()
        .with_title("FLUI Resident Reopen Probe")
        .with_size(520, 360)
        .with_exit_policy(ExitPolicy::ExplicitQuit)
        .with_service(ServiceDefinition::new(
            "probe-resident",
            ServiceLifetime::StopsWithLastWindow,
            |context| {
                println!(
                    "RESIDENT_SERVICE_START={}",
                    SERVICE_STARTS.fetch_add(1, Ordering::SeqCst) + 1
                );
                let lifetime = ServiceDrop;
                Box::pin(async move {
                    let _lifetime = lifetime;
                    context.cancellation().cancelled().await;
                    println!("RESIDENT_SERVICE_CANCELLED");
                })
            },
        ));
    let persistent = Rc::new(Cell::new(0));
    let windowless = std::env::args().any(|arg| arg == "--windowless");
    let worker = Rc::new(std::cell::RefCell::new(None));
    let worker_slot = Rc::clone(&worker);
    Application::new(move |handle| {
        let factory = FACTORIES.fetch_add(1, Ordering::SeqCst) + 1;
        println!("RESIDENT_FACTORY={factory} PERSISTENT={}", persistent.get());
        ProbeRoot {
            persistent: Rc::clone(&persistent),
            handle: handle.clone(),
        }
    })
    .with_config(config)
    .with_startup_window(if windowless {
        StartupWindow::None
    } else {
        StartupWindow::Open
    })
    .on_ready(move |handle| {
        if windowless {
            let handle = handle.clone();
            worker_slot.replace(Some(std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let request = handle
                    .request_show_main_window()
                    .expect("worker show admitted");
                println!("RESIDENT_WORKER_SHOW_ADMITTED");
                drop(request); // Accepted intent survives a dropped receiver.
            })));
        }
    })
    .run()
    .expect("resident app returns normally");
    if let Some(worker) = worker.take() {
        worker.join().expect("show worker");
    }
    assert_eq!(FACTORIES.load(Ordering::SeqCst), 2);
    assert_eq!(INITIALIZED.load(Ordering::SeqCst), 2);
    assert_eq!(DISPOSED.load(Ordering::SeqCst), 2);
    assert_eq!(SERVICE_STARTS.load(Ordering::SeqCst), 1);
    assert_eq!(SERVICE_DROPS.load(Ordering::SeqCst), 1);
    println!("RESIDENT_RETURNED_PID={}", std::process::id());
}

#[derive(Clone, StatelessView)]
struct ProbeRoot {
    persistent: Rc<Cell<usize>>,
    handle: AppHandle,
}
impl StatelessView for ProbeRoot {
    fn build(&self, _: &dyn BuildContext) -> impl IntoView {
        Theme::new(
            ThemeData::light(),
            Counter {
                persistent: Rc::clone(&self.persistent),
                handle: self.handle.clone(),
            },
        )
    }
}
#[derive(Clone, StatefulView)]
struct Counter {
    persistent: Rc<Cell<usize>>,
    handle: AppHandle,
}
struct CounterState {
    local: Rc<Cell<usize>>,
    rebuild: Option<RebuildHandle>,
    generation: usize,
}
impl StatefulView for Counter {
    type State = CounterState;
    fn create_state(&self) -> CounterState {
        CounterState {
            local: Rc::new(Cell::new(0)),
            rebuild: None,
            generation: 0,
        }
    }
}
impl ViewState<Counter> for CounterState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.rebuild = Some(ctx.rebuild_handle());
        self.generation = INITIALIZED.fetch_add(1, Ordering::SeqCst) + 1;
        println!(
            "RESIDENT_INIT={} LOCAL={}",
            self.generation,
            self.local.get()
        );
    }
    fn dispose(&mut self) {
        DISPOSED.fetch_add(1, Ordering::SeqCst);
        println!("RESIDENT_DISPOSE={}", self.generation);
    }
    fn build(&self, view: &Counter, _: &dyn BuildContext) -> impl IntoView {
        let local = Rc::clone(&self.local);
        let persistent = Rc::clone(&view.persistent);
        let rebuild = self.rebuild.clone().expect("init_state before build");
        let generation = self.generation;
        let handle = view.handle.clone();
        ColoredBox::new(Color::rgb(24, 70, 110)).child(Center::new().child(Column::new(column![
            Text::new(format!("Window generation {}", self.generation)),
            Text::new(format!(
                "Local {} / persistent {}",
                self.local.get(),
                view.persistent.get()
            )),
            ElevatedButton::new(Text::new("Increment")).on_pressed(move || {
                local.set(local.get() + 1);
                persistent.set(persistent.get() + 1);
                println!(
                    "RESIDENT_INPUT={} LOCAL={} PERSISTENT={}",
                    generation,
                    local.get(),
                    persistent.get()
                );
                rebuild.schedule(RebuildReason::StateChange);
            }),
            ElevatedButton::new(Text::new("Quit application")).on_pressed(move || {
                handle.request_quit().expect("quit admitted");
            }),
        ])))
    }
}
