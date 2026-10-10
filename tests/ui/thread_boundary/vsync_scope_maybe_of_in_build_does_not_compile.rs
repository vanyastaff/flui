use flui::view::BuildContext;
use flui::widgets::VsyncScope;

fn read(ctx: &dyn BuildContext) {
    let _ = VsyncScope::maybe_of(ctx);
}

fn main() {
    let _ = read;
}
