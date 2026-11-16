use flui_platform::{WindowCapability, WindowCommand, WindowMaterial, WindowState};

/// A reader of a window's snapshot switches on a capability the window lacks
/// and admits a command against the copy.
fn forge(state: &WindowState) {
    let mut capabilities = state.capabilities;
    capabilities.material = WindowCapability::Native;
    let _ = WindowCommand::SetMaterial(WindowMaterial::System).admit(&capabilities);
}

fn main() {}
