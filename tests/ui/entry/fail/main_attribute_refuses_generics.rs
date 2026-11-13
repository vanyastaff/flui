// A generic entry has no one function for `android_main` to call.
#[flui::main]
pub fn run<T: Default>() -> Result<(), flui::LaunchError> {
    let _ = T::default();
    Ok(())
}

fn main() {}
