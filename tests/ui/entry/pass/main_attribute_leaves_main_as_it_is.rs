// On the desktop `#[flui::main]` leaves the function as it is, so a one-file
// application keeps its `fn main` and `cargo run` starts it.
use flui::prelude::*;

#[derive(Clone, StatelessView)]
struct Hello;

impl StatelessView for Hello {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Text::new("Hello")
    }
}

#[flui::main]
fn main() -> Result<(), flui::LaunchError> {
    // trybuild runs a passing case; this one only proves it builds.
    if std::env::var_os("FLUI_ENTRY_CASE_STARTS").is_some() {
        flui::run_app(Hello)?;
    }
    Ok(())
}
