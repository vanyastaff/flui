### Added

- On iOS a scene's links, documents and universal links reach `AppHandler::opened` for its window:
  the URL contexts and web browsing activities it connects with as a cold launch, so its first
  Router opens on the link, and the ones `scene:openURLContexts:` and
  `scene:continueUserActivity:` bring while it runs.
