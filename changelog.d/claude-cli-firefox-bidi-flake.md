### Fixed

- `flui run --agent` in Firefox opens the app in a tab of its own instead of the tab Firefox started with, which could still be loading its startup page and replace the app's, so the agent never reached the page.
