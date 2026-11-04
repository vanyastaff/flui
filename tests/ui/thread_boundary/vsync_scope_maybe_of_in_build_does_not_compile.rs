use flui::view::BuildContext;
use flui::view::environment::VsyncScope;

fn read(ctx: &dyn BuildContext) {
    let _ = VsyncScope::maybe_of(ctx);
}

fn main() {
    let _ = read;
}
