use flui::animation::Vsync;
use flui::view::LifecycleContext;
use flui::widgets::VsyncScope;

fn read(ctx: &dyn LifecycleContext) -> Option<Vsync> {
    VsyncScope::maybe_of(ctx)
}

fn main() {
    let _ = read;
}
