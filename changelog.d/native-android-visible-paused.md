### Changed

- **`flui-native`**: an Android Activity that is paused but still on screen (split screen, picture
  in picture, a translucent or dialog Activity on top) keeps drawing, taking input and running its
  owner jobs, and reports its lifecycle as `Inactive`; only `onStop` suspends the window and
  releases its surface (ADR-0364). While stopped, the loop answers every input event, so a touch
  held through Home no longer makes the system report the application not responding.
- **`flui-native`**: an Android process runs one FLUI platform. A second `FluiActivity`, which a
  share or a link from another application starts in that application's task, hands its intent to
  the running Activity as a new intent, brings its task to the front and finishes; its start
  fails with the new `PlatformError::SecondActivity`. `FluiHost.PROTOCOL` is 7, so an APK needs the
  dex of this release.

### Fixed

- **`flui-cli`**: the generated Android manifest says what the host does with back: FLUI registers
  no back callback, so from Android 13 every back is the system's and finishes the Activity.
