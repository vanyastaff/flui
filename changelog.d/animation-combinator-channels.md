### Fixed

- **Removing a proxy or switch listener during a delivery keeps it until the round ends**
  (`flui-animation`): `ProxyAnimation` and `AnimationSwitch` retired a removed value listener at
  once, so a failure later in the same delivery could no longer retain its captures. Like curved,
  tween and reverse animations, they now defer the retirement to the end of the round, unless the
  round has already failed, in which case the captures are retained immediately.
