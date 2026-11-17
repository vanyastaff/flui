//! Every refusal of one input, reported together.

/// The refusals a macro has found in its input so far.
///
/// An author shown one refusal a build fixes it, builds again and meets the
/// next; a macro that reads on past a refusal reports all of them at once. The
/// only way to a record is [`Refusals::collect`], which turns what was recorded
/// into the macro's error, so a refusal that was recorded cannot be dropped.
pub(crate) struct Refusals(Option<syn::Error>);

impl Refusals {
    /// Run `read` over an empty record.
    ///
    /// `read` records each refusal it can read on from and returns `Err` with
    /// the one it cannot. The result is `read`'s value when it recorded none
    /// and returned `Ok`, and otherwise every refusal in the order it was
    /// found, the returned one last.
    pub(crate) fn collect<T>(read: impl FnOnce(&mut Self) -> syn::Result<T>) -> syn::Result<T> {
        let mut refusals = Self(None);
        let read = read(&mut refusals);
        match (refusals.0, read) {
            (None, read) => read,
            (Some(recorded), Ok(_)) => Err(recorded),
            (Some(mut recorded), Err(last)) => {
                recorded.combine(last);
                Err(recorded)
            }
        }
    }

    pub(crate) fn push(&mut self, refusal: syn::Error) {
        match &mut self.0 {
            Some(recorded) => recorded.combine(refusal),
            None => self.0 = Some(refusal),
        }
    }

    /// Run `read` over this record; whether it recorded a refusal.
    ///
    /// For a reading whose result stands only when every part of it read: a
    /// fact the author stated, such as a query field's key, is known only from
    /// an attribute that was not refused anywhere.
    pub(crate) fn refused(&mut self, read: impl FnOnce(&mut Self)) -> bool {
        let mut part = Self(None);
        read(&mut part);
        match part.0 {
            Some(refusals) => {
                self.push(refusals);
                true
            }
            None => false,
        }
    }

    /// The value of `result`, or `None` with its refusal recorded.
    pub(crate) fn value<T>(&mut self, result: syn::Result<T>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(refusal) => {
                self.push(refusal);
                None
            }
        }
    }
}

/// Assert that `result` is refused once for each entry of `expected`, in order, each refusal
/// containing its entry's text.
#[cfg(test)]
pub(crate) fn assert_refused<T>(result: syn::Result<T>, expected: &[&str]) {
    let Err(refusals) = result else {
        panic!("BUG: the input expands");
    };
    let refusals: Vec<String> = refusals
        .into_iter()
        .map(|refusal| refusal.to_string())
        .collect();
    assert_eq!(refusals.len(), expected.len(), "{refusals:?}");
    for (refusal, expected) in refusals.iter().zip(expected) {
        assert!(refusal.contains(expected), "{refusal}");
    }
}

#[cfg(test)]
mod tests {
    use proc_macro2::Span;

    use super::*;

    fn refusal(text: &str) -> syn::Error {
        syn::Error::new(Span::call_site(), text)
    }

    #[test]
    fn a_collection_is_its_value_or_every_refusal_in_the_order_found() {
        assert_eq!(Refusals::collect(|_| Ok(7)).ok(), Some(7));
        assert_refused(
            Refusals::collect(|_| Err::<u8, _>(refusal("only"))),
            &["only"],
        );
        assert_refused(
            Refusals::collect(|refusals| {
                refusals.push(refusal("first"));
                assert_eq!(refusals.value(Err::<u8, _>(refusal("second"))), None);
                assert_eq!(refusals.value(Ok(3)), Some(3));
                Ok(7)
            }),
            &["first", "second"],
        );
        assert_refused(
            Refusals::collect(|refusals| {
                refusals.push(refusal("first"));
                Err::<u8, _>(refusal("last"))
            }),
            &["first", "last"],
        );
        assert_refused(
            Refusals::collect(|refusals| {
                refusals.push(refusal("first"));
                assert!(!refusals.refused(|_| {}));
                assert!(refusals.refused(|part| part.push(refusal("part"))));
                Ok(7)
            }),
            &["first", "part"],
        );
    }
}
