# Animation: навигация по требованиям и готовности

Актуальная точка входа — [контракты и границы приёмки](readiness-status.md).
Реализация A0–A7 интегрирована; 110 исходным требованиям назначены исполняемые
сценарии, сохранённые авторские API либо согласованные исключения. A8 завершён:
датированные измерения, Windows native/GPU и локальный consumer gate успешны.
A9 завершён в принятом scope: конечная code revision `71f13790b` прошла CI;
результаты отдельных revisions и границы других платформ указаны в выпускных свидетельствах.
Их результаты фиксируются в [выпускных свидетельствах](release-evidence.md).
Исторические 32 красных контракта относятся к аудиту 2026-10-08, а не к текущему main.

| Документ | Назначение |
|---|---|
| [readiness-status.md](readiness-status.md) | Текущие контракты всех 110 требований и принятые границы |
| [release-evidence.md](release-evidence.md) | Выпускные измерения, native/GPU и CI на закреплённых revisions |
| [readiness-audit.md](readiness-audit.md) | Исторические находки на базе 2026-10-08 |
| [readiness-evidence.md](readiness-evidence.md) | Исторические воспроизводимые команды и результаты |
| [readiness-plan.md](readiness-plan.md) | Порядок работ, зависимости, критерии приёмки |
| [readiness-matrix.md](readiness-matrix.md) | Маршрут всех 110 требований старой матрицы в новый план |
| [catalog/](catalog/requirements.md) | Каталог готового движения: маршруты, рябь, списки, эффекты, скелетоны (2026-10-13) |
| [animation-competitors.md](animation-competitors.md) | Проверка animation-части сравнительного документа по первичным источникам; выводы для FLUI |
| [market.md](market.md) | Восстановленная историческая матрица от 2026-10-06; её статусы не текущие |
| [historical-orchestration.md](historical-orchestration.md) | Восстановленные решения владельца, ограничения scope и границы send-flip |
| [orchestration.md](orchestration.md) | Исходный процесс и более позднее решение об одном SystemPreferences |
| [tasks.md](tasks.md) | Исторический реестр D-01…D-47, база `9a4daa3ed` |

## Происхождение восстановленных документов

В текущем `main` до этого восстановления были только `orchestration.md` и `tasks.md`.
В Git найден недостижимый из веток коммит `801543a3f` от 2026-10-06
(`docs: flui-animation decisions from the conventions audit`). Из него восстановлены
`market.md`, `review.md`, `conventions-audit.md` и документы 12 тем:
composition, controller-robustness, curves, frame-path-state, integration, interpolation,
listener-delivery, motion-clock, ownership, physics, reduce-motion, retarget.
Его полная оркестрация сохранена отдельно как `historical-orchestration.md`.

Это исторические исходные требования, а не отчёт о завершении или новая команда реализации.
Их строки исходного кода, даты, номера ADR и межсессионные назначения необходимо сверять
с текущим деревом. В частности, ADR-0143 сейчас занят словарём ввода; старый резерв номера
для listener-delivery использовать нельзя. `review.md` — ревью старых спек, текущий аудит
находится в `readiness-audit.md`.

При расхождении приоритет: прямые решения владельца → действующие принятые ADR и текущий
контракт владельца подсистемы → новый план с явно отмеченными предложениями → историческая
спецификация. Позднейшее решение об одном host `SystemPreferences` заменяет старый
`SystemMotion`/оконные методы в восстановленной reduce-motion-спеке.

Текущая матрица не объявляет все старые требования выполненными по факту merge PR
и не меняет их scope без решения владельца. Дальнейшая сессия читает
`readiness-status.md`, `release-evidence.md` и оставшиеся пункты `readiness-plan.md`;
исторические команды перезапускаются только для конкретного непроверенного контракта.
