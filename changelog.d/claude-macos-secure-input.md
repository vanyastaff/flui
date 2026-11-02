### Security

- macOS: a protected text field focused in the key window holds the process's secure event input (`EnableSecureEventInput`), as `NSSecureTextField` does, so other processes' event taps do not see the password being typed. The hold is given back when the field loses focus or protection, when the window resigns key, and when its view goes away.
