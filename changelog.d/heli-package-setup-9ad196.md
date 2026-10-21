### Removed

- `cargo xtask check-changed`, its verified-result records and the per-user host lock that
  queued heavy xtask commands (`FLUI_XTASK_NO_LOCK`, `FLUI_XTASK_LOCK_FILE`). On a host shared by
  several agents, [heli](https://github.com/vanyastaff/heli) schedules every cargo from one pool,
  shares identical runs and remembers passed ones; AGENTS.md's "Checking a change" lists the
  pre-PR commands to run one at a time.
