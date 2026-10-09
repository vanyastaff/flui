# Animation: навигация по требованиям и готовности

Актуальная точка входа — [план до production ready](readiness-plan.md), составленный
2026-10-08 на `main` `91bb1fe1d2a349f62915846303f6f25780ae5c6d`.
Состояние: **не готово**; обычные тесты проходят, 32 отключённых контрактных теста падают.

| Документ | Назначение |
|---|---|
| [readiness-audit.md](readiness-audit.md) | Текущие находки, исходный код, ограничения проверки |
| [readiness-evidence.md](readiness-evidence.md) | Воспроизводимые команды и результаты на закреплённом SHA |
| [readiness-plan.md](readiness-plan.md) | Порядок работ, зависимости, критерии приёмки |
| [readiness-matrix.md](readiness-matrix.md) | Маршрут всех 110 требований старой матрицы в новый план |
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

Новый аудит не объявляет все старые требования выполненными по факту merge PR и не меняет
их scope без решения владельца. Дальнейшая сессия начинает с `readiness-plan.md` и запуска
команд из `readiness-evidence.md` на своём актуальном SHA.
