### Fixed

- The back key (Alt+Left, Cmd+[ on Apple, `BrowserBack`) and Escape on a dismissible popup now
  pop a route whose page has nothing focusable, such as an `ErrorView` in place of a page whose
  build failed. The route's key actions used to sit inside its focus scope, so a key with the
  focus on the scope itself never reached them.
