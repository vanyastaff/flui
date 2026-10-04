### Fixed

- The workspace target build excludes its already running xtask driver, avoiding a Windows executable replacement failure when workspace features change the driver's dependency artifacts. Workspace lint and test runs continue to check the driver.
