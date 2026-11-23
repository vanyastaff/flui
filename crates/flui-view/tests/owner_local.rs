//! A key and a stream builder hold owner-thread state: they
//! carry no thread bound (ADR-0323 §4), so `Rc` and `Cell` values pass.

use std::{
    cell::Cell,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
};

use flui_foundation::ViewKey;
use flui_view::element::AsyncSnapshot;
use flui_view::{
    ErrorView, ObjectKey, ViewExt,
    element::{SnapshotBuilder, StreamBuilder, StreamFactory},
};
use futures_core::Stream;

/// A stream whose state is owner-local.
struct Countdown(Rc<Cell<u32>>);

impl Stream for Countdown {
    type Item = Result<Rc<u32>, Rc<str>>;

    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let left = self.0.get();
        if left == 0 {
            return Poll::Ready(None);
        }
        self.0.set(left - 1);
        Poll::Ready(Some(Ok(Rc::new(left))))
    }
}

#[test]
fn owner_local_values_build_keys_and_streams() {
    let object = Rc::new(Cell::new(1_u8));
    let key = ObjectKey::new(&object);
    assert!(key.key_eq(&ObjectKey::new(&object)));
    assert!(!key.key_eq(&ObjectKey::new(&Rc::new(Cell::new(1_u8)))));

    let remaining = Rc::new(Cell::new(2));
    let source = Rc::clone(&remaining);
    let make: StreamFactory<Rc<u32>, Rc<str>> =
        Rc::new(move || Box::pin(Countdown(Rc::clone(&source))));
    let builder: SnapshotBuilder<Rc<u32>, Rc<str>> =
        Rc::new(|_, _: &AsyncSnapshot<Rc<u32>, Rc<str>>| ErrorView::new("leaf").boxed());
    let _view = StreamBuilder::keyed(Some(Rc::clone(&object)), make, builder).boxed();

    assert_eq!(remaining.get(), 2, "building the view polls nothing");
}
