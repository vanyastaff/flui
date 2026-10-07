use std::num::NonZeroU64;

use flui_interaction::ids::HandlerId;

fn main() {
    let raw = NonZeroU64::MIN;
    let _ = HandlerId::new(1);
    let _ = HandlerId::try_new(1);
    let _ = HandlerId::from(raw);
    let _: HandlerId = raw.into();
    let _: Result<HandlerId, _> = raw.try_into();
}
