# Повторный adversarial review engine plan

Дата: 2026-10-01. Три агента повторно проверили обновлённые планы и closure прежних
замечаний. [API](engine-plan-rereview-api.ru.md),
[correctness](engine-plan-rereview-correctness.ru.md),
[recovery](engine-plan-rereview-recovery.ru.md) содержат closure tables и новые
failure scenarios. Первоначальные замечания закрыты **на уровне решения**, не кода.

## Что второй проход добавил

Найдено 11 замечаний с пересечениями, сведённых в восемь решений master:

1. Command buffers и budget charges передаются одним consuming opaque token,
   привязанным к owner/epoch/target; independently supplied пары запрещены.
2. Submission owner покрывает промежуточные clear/effect/backdrop/blit passes,
   а не только final. Уже submitted работу нельзя discard как неотправленную.
3. Last committed target защищён candidate/staging target и charged copy/space.
   Late failure не откатывает GPU; candidate не становится committed результатом.
4. Нельзя просто объединить queue submits: разные blur calls переиспользуют
   mutable uniform slots. Freeze per encoded use предшествует submit fusion.
   Dispatcher error должен прервать frame, а не продолжить copy/submission.
5. Shared device имеет один instance-owned DeviceDomain, отдельные realm quotas
   и wakes. Per-painter quota не объявляется device-total memory cap.
6. Начальный PreparedIrBudget явно ограничен новыми IR allocations/temporaries;
   whole-ledger coverage имеет отдельную delivery с inventory legacy exclusions.
7. Retirement survives last-window close, callbacks не держат owner cycles,
   native poll nonblocking, browser progress отдельно. Deadline не completion.
8. Wgpu30 error guards pop на owning thread до edge future; raw Device/Queue
   считается trusted authority, managed operations не раскрывают его через Deref.

Решения интегрированы в [master](engine-foundation-implementation.ru.md),
[IR](engine-frame-ir-migration.ru.md) и
[resources](engine-resource-contract-migration.ru.md). Точные Rust signatures
и allocation inventory фиксируются до coding; нет заявления, что документация
обеспечила типовую или runtime гарантию.

## Проверяемые ограничения

Verified static evidence: ранние production submits существуют в renderer,
layer dispatcher и offscreen paths; backdrop flush логирует render error и затем
продолжает copy. Wgpu30.0.1 published source подтверждает thread-local !Send
ErrorScopeGuard, explicit pop, callback progress requirements и Device::destroy.
Future migration failure scenarios не воспроизведены — это acceptance matrices,
не новые выполненные GPU regressions. Production код в ходе повторного review
не менялся, новые сборки агентами не запускались.

Performance baseline/thresholds, failure injections, multi-owner close/loss,
two-blur uniform isolation и candidate commit proof ещё выполнить. Поэтому план
пригоден для следующего шага — inventory и bounded implementation — но green
current suite не объявляет будущую foundation реализованной или бездефектной.
