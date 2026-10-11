# ADR-0187: Missing inherited lookups depend on ancestry

- **Status:** Accepted
- **Date:** 2026-10-10
- **Related:** [ADR-0074](ADR-0074-realm-scoped-signals.md).

## Context

A mounted Hero can begin outside a route and later move into a HeroScope through
GlobalKey reparenting. Its initial `depend_on` returns `None`. Recording only
successful provider lookups leaves the element without a dependency reactivation
marker, so its lifecycle hook never adopts the new route. Build-time optional
inherited reads have the same missing-provider case.

## Decision

A failed `depend_on` records dependence on ancestry. The element-tree build owner
keeps sparse sets for missing lifecycle reads and missing build reads, alongside
its reverse provider index. A miss owns no provider and creates no forward
notification edge. `get` remains a one-time lookup without registration.

The existing read lifetime rules apply to misses. Lifecycle reads accumulate
until teardown. Each successful build replaces its missing build read set;
recovery from an authored build failure preserves the previous set and adds any
new reads. Buffered records distinguish a provider identity from absence, so
lifecycle reads during a build drain follow the same policy as direct contexts.

Deactivation withdraws both provider edges and missing-read markers before
reactivation can refresh dependencies. Reactivation consumes the existing
dependency-change marker and invokes the lifecycle hook before building against
the new ancestry. Permanent teardown removes active and inactive markers.

## Alternatives

Refreshing every reactivated node would invoke dependency hooks for nodes that
never read inherited ancestry. Re-resolving HeroScope from Hero's build would
leave the same problem in other optional inherited consumers. Tracking misses
inside the existing dependency owner covers both context paths and retains
reset-on-build behavior without adding storage to every element node.

## Verification

`missing_inherited_reads_refresh_lifecycle_and_survive_build_recovery` covers
direct dependency lookup, non-subscribing lookup, build-time lookup, repeated
activation and recovery before a previously accepted read. The mounted widget
cases `reparenting_an_unscoped_hero_adopts_its_first_route` and
`reparenting_refreshes_a_build_time_miss_but_prunes_an_unread_one` verify route
adoption, preserved state and changed child geometry, then prune an unread miss.
