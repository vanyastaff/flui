# Desktop MCP как существующая основа AI проверки FLUI

Проверено чтением checkout 1 октября 2026 года. `tools/desktop-mcp` не изменялся,
сборки и live tests не запускались. Отдельного ARCHITECTURE.md в каталоге нет;
основания — README, main, live_windows и support client, ADR-0080/0095,
существующий facade agent_workflow и архитектура flui-testing.

## Что уже реализовано

`flui-desktop-mcp` — MCP stdio server, наблюдающий любое desktop приложение
через OS accessibility/capture/input. Это существующий инструмент пользователя,
не задача построить второй AI test framework. README перечисляет launch/kill,
window discovery, screenshots, tree/find/wait_for, semantic actions и native
pointer/keyboard. Tools публикуют typed output schemas; screenshot возвращает
image и metadata text. Window/element/screenshot handles не переиспользуются;
отсутствующий target отвечает `gone`.

Один desktop worker сериализует UIA/input; очередь ограничена. Native input
требует window/pid target, проверяет foreground, focus, deepest window и
physical input state перед событиями. Accessibility actions способны работать
при перекрытом окне; после них foreground надо проверить снова. Effect codes
partial/may_have_run/ran/incidental уже выражают неоднозначный результат;
readback_failed не является основанием повторять action.

`tests/live_windows.rs` уже запускает a11y_probe, читает semantics, invokes и
clicks, проверяет изменённый count, снимает PNG и умеет преобразовать semantic
screen rect в capture pixels. `tests/support/mod.rs` уже реализует stdio MCP
client и handshake. `tests/native_windows.rs` добавляет Win32 fixture для
text/focus/toggle/wheel/drag. Это правильные точки расширения native проверки.

Параллельно существующий `HeadlessBinding` через manual clock pumps realm;
facade `tests/agent_workflow.rs` монтирует UI, читает semantics, replay pointer
и проверяет render diagnostics. Он не является GPU pixel oracle. Engine
readbacks нужны для pixels, desktop MCP — для OS integration и реального ввода.

ADR-0095 уже добавил in-process SemanticsAgent/DevAgentHook и optional devtools
server с local token endpoint, owner inbox и frame-boundary reads/actions.
Он говорит тем же schema vocabulary, но не является desktop OS capture и
input implementation. `flui mcp` proxy отмечен в ADR как Proposed; нельзя
давать пользователю несуществующую CLI команду как работающую.

## Контракты удобной проверки

| Задача | Уже есть | Что проверять/дополнить при реальном consumer |
|---|---|---|
| Semantic selector плюс pixel oracle | find role/name/automation_id, bounds; PNG с source/scale; live fixture crop helper | Один selector определяет target, независимое pixel ожидание доказывает изображение. Изменение pixels не доказывает правильный glyph/clip/blend. |
| Ожидание кадра | wait_for semantic state; in-process owner drain | Ни wait_for, ни successful invoke не обещают displayed frame. Уточнить accepted/committed/displayed только при нужном workload; desktop ждёт наблюдаемый predicate, bounded capture convergence — эвристика, не vsync ack. |
| Время | Manual clock headless; native desktop real time | Не внедрять virtual clock в чужие приложения. Для игры/AI использовать fake provider и headless fixed-step, затем native smoke той же функции. |
| Input ownership | Target checks, held-input recovery, serialized worker | Один live worker и отсутствие concurrent human input. Отказ foreground — inconclusive/failure с evidence, не successful skipped click. |
| Capture privacy | Можно capture конкретное окно; monitor capture также доступен | Fixture-only window scope; secrets не включать в artifacts. По умолчанию не сохранять monitor или arbitrary app captures. Platform permission не равно разрешению публиковать изображение. |
| Diagnostics | Typed errors/retry/effect; devtools logs без labels/values | Native test show сейчас печатает reply фрагмент — fixture данные допустимы, production replay надо redaction. Не сохранять token/env/text input целиком. |
| Artifacts | PNG delivery, metadata, structured replies | Client-side expected/actual/diff и bounded event log с format/DPI/font/adapter/fixture version. Не расширять transport огромными logs без необходимости. |
| Replay flakes | Session handles, effect semantics, wait tools | Записывать selector/action/predicate, не старые handle IDs или координаты без provenance. После may_have_run сначала read effect; повтор не должен удвоить increment/payment. |
| Failure recovery | Held keys release, worker serialization, launch containment, typed timeout | Проверять next operation после partial/error и fixture exit. Hung native provider остаётся documented limitation; timeout ответа не отменяет action автоматически. |

## Пределы текущей проверки

README: Windows путь поддержан, macOS capture/input/accessibility существенно
ограничены и CI только typechecks; Linux desktop tools пока unsupported.
Нельзя называть live native tests переносимым GPU/OS gate. UIA provider может
блокироваться и возвращать oversized properties до Rust clipping; README
ссылается на isolation issue #1289. Capture source-size precheck может иметь
race resize из-за внутренних allocations xcap. Shutdown может ждать recovery
input/provider неопределённо долго. Эти риски не следует прятать retry loop.

Traversal truncated означает неполное знание: пустой find при truncated не
доказывает отсутствия элемента. Пример «кнопка исчезла» должен различать gone
и budget/provider failure. Pixel captures также не являются проверенным golden
без independent expectation и documented tolerance.

## Как новые демонстрации используют текущий инструмент

Протокол клиента остаётся initialize → tools/call, через существующий support
Client или любой MCP-compatible агент. Минимальный workflow: launch fixture,
wait_for_window(pid), find semantic target, invoke или проверенный native click,
wait_for expected state, screenshot(window), сравнение заданной области,
kill только session-launched process. Для нескольких game/AI/GPU scenes
использовать существующий fixture pattern, а не отдельный automation runtime.

Game demo: semantic Pause/Resume и HUD, затем native drag/keyboard с ownership,
PNG на известном simulation state. GPU demo: selectors выбирают deterministic
scene, pixel oracle проверяет clip/blend/3D overlay, driver details фиксируются.
GenUI demo: fake provider возвращает bounded catalog form, semantic actions
проверяют state feedback; malformed/late response не разрушает последующую
операцию. Реальный model adapter проверяется отдельно, без golden на случайном
ответе LLM.

Работающий documented запуск существующей live проверки:

```text
cargo build --release --example a11y_probe --features material,a11y
cargo nextest run -p flui-desktop-mcp --test live_windows --run-ignored only --no-capture
```

Эти команды здесь только прочитаны, не выполнены. `FLUI_A11Y_PROBE` позволяет
задать actual example executable path при shared target dir. Новые scenarios
добавлять после согласования с текущей работой пользователя в tools; memo
не меняет его API, transport или tests.

## Проверенные локальные основания

- [README инструмента](../../tools/desktop-mcp/README.md).
- [Live Windows test](../../tools/desktop-mcp/tests/live_windows.rs).
- [Stdio test client](../../tools/desktop-mcp/tests/support/mod.rs).
- [Native Windows fixture](../../tools/desktop-mcp/tests/native_windows.rs).
- [Facade workflow](../../tests/agent_workflow.rs).
- [Headless architecture](../../crates/flui-testing/ARCHITECTURE.md).
- [Protocol contract](../adr/ADR-0080-agent-protocol-desktop-contract.md).
- [In-process schema/server contract](../adr/ADR-0095-agent-protocol-schema-crate.md).
