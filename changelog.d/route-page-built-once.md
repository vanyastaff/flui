### Fixed

- A route's page builder no longer runs on every frame of its transition: the page is built once
  and each tick rebuilds only the transitions around it, as in Flutter. A push or pop in the app
  scenario bench costs 81% fewer instructions.
