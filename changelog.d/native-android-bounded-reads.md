### Changed

- Android: a clipboard read answers the text the clip's first item holds and opens no other application's content; a clip that is a URI or an intent reads as no text (it read the provider's whole stream before, on the owner thread). A paste from the keyboard takes the same text, and text past the clipboard's limit is refused whole, as on every other host.
- Android: the documents an intent opens are opened off the owner thread, so a slow content provider no longer stalls frames and input. Intents are still heard in the order they arrived; a document still opening 30 seconds after its intent arrived is cancelled and left out, as is one whose read grant has ended.
- A drag from another application offers representations from at most its first 100 types, and none from a type longer than 255 bytes, on every host (Windows read up to 256 format entries, the Web and Android every type).

### Fixed

- Android: a drag naming many media types can no longer overflow the native frame's local references, and a dropped document whose stated size is past the request's limit is refused before any of it is read.
