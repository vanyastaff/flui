use flui_platform::{HostWindow, OwnerPlatform, SharedPlatform};
use std::sync::Arc;

fn assert_shared<T: Send + Sync + Clone>() {}

fn owner_context(owner: &OwnerPlatform, window: &Arc<dyn HostWindow>) {
    let _shared: SharedPlatform = owner.shared();
    let _ = owner.text_store_host(window);
}

fn main() {
    assert_shared::<SharedPlatform>();
    let _ = owner_context;
}
