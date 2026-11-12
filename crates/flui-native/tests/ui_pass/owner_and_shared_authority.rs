use flui_native::{HostWindow, OwnerPlatform, PlatformProxy};
use std::rc::Rc;

fn assert_shared<T: Send + Sync + Clone>() {}

fn owner_context(owner: &OwnerPlatform, window: &Rc<dyn HostWindow>) {
    let _proxy: PlatformProxy = owner.proxy();
    let _ = owner.text_store_host(window);
}

fn main() {
    assert_shared::<PlatformProxy>();
    let _ = owner_context;
}
