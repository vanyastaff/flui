// Without the `unstable-windows` feature an application has one window: the
// facade publishes no policy for a second one until ADR-0091 §3–§5 are built;
// with it this compiles.
use flui::app::WindowPolicy;

fn main() {
    let _ = std::any::type_name::<WindowPolicy>();
}
