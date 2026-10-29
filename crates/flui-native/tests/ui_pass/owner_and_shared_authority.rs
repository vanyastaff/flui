use flui_native::{HostWindow, OwnerPlatform, SharedPlatform};
use std::rc::Rc;

fn assert_shared<T: Send + Sync + Clone>() {}

fn owner_context(owner: &OwnerPlatform, window: &Rc<dyn HostWindow>) {
    let _shared: SharedPlatform = owner.shared();
    let _ = owner.text_store_host(window);
}

fn main() {
    assert_shared::<SharedPlatform>();
    let _ = owner_context;
}
