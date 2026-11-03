### Added

- **`flui-native`**: with no IBus or Fcitx5 on D-Bus, the native X11 host speaks XIM to the input
  method server `XMODIFIERS=@im=` names, over the X transport. Each key is forwarded to the server
  and held until it answers: a key it sends back is typed, one it keeps is taken; its on-the-spot
  preedit composes in the field, its commits replace it, and the caret's place reaches it as the
  preedit spot. A server that goes, or that sends what no message reads as, ends the connection
  without a panic and keys are typed as they come. XIM has no surrounding text, purpose or
  deletion. `flui-app` still picks winit on Linux.
