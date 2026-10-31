### Security

- macOS: a focused protected text field holds the process's secure event input (`EnableSecureEventInput`), as `NSSecureTextField` does, so other processes' event taps do not see the password being typed. The hold is given back when the field loses focus or protection, and when its view goes away.
