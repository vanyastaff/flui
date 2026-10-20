### Fixed

- Web: a protected (password) field now reaches the browser's text surface as bullets of its length instead of empty, so soft-keyboard edits and deletes land at the field's offsets.
- Web: an edit the store refuses (or fails to apply) no longer leaves the browser's text surface ahead of the field; the surface is republished from the store.
- Web: character bounds for a composition containing a surrogate pair (emoji) are reported for every code unit instead of stopping at the pair.
- Web: the input method's candidate window follows the field when the page scrolls or the viewport resizes.
