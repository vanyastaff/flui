//! What the positions of a host's table keep for the life of its loop.

use std::any::Any;
use std::sync::Arc;

/// The objects the positions of a host's table keep for the life of its loop,
/// at most one of each type.
///
/// The loop holds them without naming them, because the owner names no feature
/// (ADR-0151 §10): the host's table puts each in as it builds the loop, and the
/// position, or another file of its feature, takes its own back out by its
/// type. An object the owner calls is not one of these: the owner calls it
/// through a hook trait declared beside the loop.
#[derive(Default)]
pub(crate) struct PositionObjects {
    objects: Vec<Arc<dyn Any + Send + Sync>>,
}

impl PositionObjects {
    /// These objects and `object`, which replaces one of its type.
    #[must_use]
    pub(crate) fn with<T: Any + Send + Sync>(mut self, object: Arc<T>) -> Self {
        self.objects.retain(|held| !held.is::<T>());
        self.objects.push(object);
        self
    }

    /// The object of type `T`, when the table put one in.
    pub(crate) fn get<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        self.objects
            .iter()
            .find(|held| held.is::<T>())
            .and_then(|held| Arc::clone(held).downcast::<T>().ok())
    }
}

impl std::fmt::Debug for PositionObjects {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PositionObjects")
            .field("objects", &self.objects.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::PositionObjects;

    #[derive(Debug, PartialEq)]
    struct Transport(u8);

    #[derive(Debug, PartialEq)]
    struct Receiver(u8);

    #[test]
    fn a_position_takes_back_the_object_of_its_type() {
        let first = Arc::new(Transport(1));
        let objects = PositionObjects::default()
            .with(Arc::clone(&first))
            .with(Arc::new(Receiver(2)));
        let found = objects
            .get::<Transport>()
            .expect("the transport was put in");
        assert!(Arc::ptr_eq(&found, &first), "the same object, not a copy");
        assert_eq!(objects.get::<Receiver>().as_deref(), Some(&Receiver(2)));
        assert_eq!(objects.get::<String>(), None, "a type nobody put in");

        let objects = objects.with(Arc::new(Transport(3)));
        assert_eq!(
            objects.get::<Transport>().as_deref(),
            Some(&Transport(3)),
            "a second object of a type replaces the first"
        );
    }
}
