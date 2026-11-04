use flui::animation::Vsync;
use flui::view::LifecycleContext;
use flui::view::environment::VsyncScope;

fn read(ctx: &dyn LifecycleContext) -> Option<Vsync> {
    VsyncScope::maybe_of(ctx)
}

fn main() {
    let _ = read;
}
