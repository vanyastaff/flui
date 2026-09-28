# Signal update: контракт после panic

## Вопрос

Если `Signal::update` частично изменил `T`, а пользовательский updater запаниковал, FLUI уже
возвращал изменённое значение в слот, но не ставил readers в rebuild inbox. После containment
модель становилась новой, а UI мог навсегда остаться старым. Нужно было выбрать между rollback,
poisoning, тихим partial commit и partial commit с invalidation.

## Проверенные модели

- [Rust `UnwindSafe`](https://doc.rust-lang.org/std/panic/trait.UnwindSafe.html) предупреждает о
  наблюдении нарушенных логических инвариантов и не является транзакционной гарантией.
- [Leptos `WriteGuard`](https://github.com/leptos-rs/leptos/blob/584c3a2d884b0e4dba9e3f822a9b6982cff072c7/reactive_graph/src/signal/guards.rs#L257-L369)
  уведомляет subscribers при `Drop`, поэтому unwind не оставляет изменение невидимым.
- [Dioxus signal guard](https://github.com/DioxusLabs/dioxus/blob/c607e4ef28616456402b6da32c916e977c95d665/packages/signals/src/signal.rs#L519-L536)
  также обновляет subscribers из drop guard.
- [`tokio::watch::Sender::send_modify`](https://docs.rs/tokio/latest/tokio/sync/watch/struct.Sender.html#method.send_modify)
  явно выбирает противоположный низкоуровневый контракт: partial value остаётся, receivers не
  уведомляются. Для UI runtime, который ловит panic и продолжает работу, это создаёт постоянный
  разрыв model/presentation.
- [GPUI ownership and data flow](https://zed.dev/blog/gpui-ownership) отделяет mutation от
  отложенной FIFO-доставки effects; observers не вызываются реентрантно внутри commit.
- [Compose `MutableSnapshot`](https://developer.android.com/reference/kotlin/androidx/compose/runtime/snapshots/MutableSnapshot)
  показывает, что настоящая изоляция до atomic apply является отдельным механизмом, а не скрытым
  свойством обычной mutable closure.

## Решение

Обычный signal update имеет контракт **commit-on-unwind, notify-after-commit, resume-panic**.
Partial value сохраняется, весь reader set сначала атомарно попадает в rebuild inbox, затем один
wake просит frame, после чего исходная panic возобновляется. Telemetry выполняется после durable
enqueue. Если wake паникует, общий inbox сохраняет wake debt: первый следующий вызов через handle
с hook повторяет wake даже для уже занятых ids. Rollback произвольного `T` и внешних эффектов не
обещается; poisoning не вводится.

Валидное типизированное чтение регистрируется после освобождения graph loan, но до возобновления
panic из пользовательской read closure. Поэтому first build после recovery сохраняет dependency
без требования reentrant `ReadGraph` и может быть повторён следующим write.

Перед `resume_unwind` реализация перечисляет все живые owned-значения и выполняет потенциально
паникующий cleanup под отдельным containment. Первый по времени panic сохраняет приоритет над
`Drop` значения signal, результата read и вторичных panic payload; иначе пользовательский
деструктор мог заменить исходную причину или привести к abort из-за двойной panic.

## Масштабирование и границы

Batch enqueue берёт inbox lock один раз на весь reader set и делает один wake на burst. Стоимость
остаётся `O(readers)`, как требует precise invalidation. Широковещательное состояние для сотен
ячеек остаётся задачей `InheritedView` с field masks по ADR-0074. Транзакции, poisoning и rollback
не добавляются без реального доменного потребителя.

## Почему первоначальное ревью пропустило дефекты

Ревью проверяло названные фазы (`updater`, enqueue, wake, telemetry), но не составляло inventory
всех owned-значений, остающихся живыми в каждой точке `resume_unwind`. Тесты возвращали `Copy`
значения и поэтому не атаковали destructor результата или loaned `T`. Кроме того, durable enqueue
ошибочно приняли за progress guarantee: тест заканчивался сразу после panicking wake и не проходил
последовательность «panic перехвачен → hook восстановлен → тот же id записан снова».

Для следующих unwind-sensitive изменений обязательна матрица: panic каждой фазы отдельно, две
panic одновременно, user-defined panicking `Drop` для каждого generic owned value и повторная
операция после containment. Для отложенной доставки отдельно проверяются durability, liveness,
несколько handles над общим состоянием и handle без hook, который не имеет права погасить debt.
