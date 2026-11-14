### Added

- `flui doctor --domains` fetches each associated domain's `apple-app-site-association` and `assetlinks.json` and checks that they open the application: status 200 without a redirect, an `applinks` entry naming `<team>.<bundle identifier>` with what it opens, a `handle_all_urls` statement for the Android package with its certificate fingerprints. Without the flag nothing is fetched, and the declarations check names it.
