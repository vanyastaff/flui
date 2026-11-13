// The run drives its own loop: an `async` entry would have nothing to poll it.
#[flui::main]
pub async fn run() -> Result<(), flui::LaunchError> {
    Ok(())
}

fn main() {}
