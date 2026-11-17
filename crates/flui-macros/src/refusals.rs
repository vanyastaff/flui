//! Every refusal of one input, reported together.

/// The refusals a derive has found in its input so far.
///
/// An author shown one refusal a build fixes it, builds again and meets the
/// next; a derive that reads on past a refusal reports all of them at once.
#[derive(Default)]
pub(crate) struct Refusals(Option<syn::Error>);

impl Refusals {
    pub(crate) fn push(&mut self, refusal: syn::Error) {
        match &mut self.0 {
            Some(all) => all.combine(refusal),
            None => self.0 = Some(refusal),
        }
    }

    /// The value of `result`, or `None` with its refusal recorded.
    pub(crate) fn value<T>(&mut self, result: syn::Result<T>) -> Option<T> {
        result.map_err(|refusal| self.push(refusal)).ok()
    }

    /// `Err` with every refusal recorded, in the order they were found.
    pub(crate) fn into_result(self) -> syn::Result<()> {
        self.0.map_or(Ok(()), Err)
    }
}
