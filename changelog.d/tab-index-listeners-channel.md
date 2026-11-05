### Changed

- The material tab controller's index listeners are the notification channel:
  the hand-rolled listener list, its hand-minted listener ids and its own
  snapshot-and-recovery loop give way to a `ChangeNotifier`, whose delivery's
  containment, ordering and retirement the tab's listeners now share. The
  public `TabController::listen` keeps its owned `Subscription`.
