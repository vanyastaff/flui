// The entry function takes nothing: the system hands Android's `AndroidApp`
// to the run itself.
#[flui::main]
pub fn run(title: &str) -> Result<(), flui::LaunchError> {
    let _ = title;
    Ok(())
}

fn main() {}
