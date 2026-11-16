use flui_platform::{AdmittedCommand, WindowCommand};

fn main() {
    let _ = AdmittedCommand {
        command: WindowCommand::Show,
    };
}
