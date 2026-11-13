use flui_native::{
    HeadlessDisplayControls, HeadlessHostEvents, HeadlessPreferences, HostWindow, OwnerPlatform,
    PlatformProxy,
};
use std::rc::Rc;

fn assert_shared<T: Send + Sync + Clone>() {}

fn owner_context(owner: &OwnerPlatform, window: &Rc<dyn HostWindow>) {
    let _proxy: PlatformProxy = owner.proxy();
    let _ = owner.text_store_host(window);
}

fn main() {
    assert_shared::<PlatformProxy>();
    // What the simulated operating system changes from any thread, as a
    // native display, preference or session source does.
    assert_shared::<HeadlessDisplayControls>();
    assert_shared::<HeadlessPreferences>();
    assert_shared::<HeadlessHostEvents>();
    let _ = owner_context;
}
