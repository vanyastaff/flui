# Демонстрация AI streaming через работающий FLUI runtime

Пример [ai_streaming.rs](../../examples/ai_streaming.rs) подключает реальный
OpenAI-compatible Chat Completions SSE endpoint к обычным FLUI widgets.
Он не добавляет runtime, engine hook, crate с моделью или process-global executor.
Текстовый ввод, Send/replace, Stop, streaming output и error state проходят через
`run_app_with_config`, `ServiceDefinition`, `TaskSpawner::spawn_io` и
`StreamBuilder::keyed`. Это демонстрация интеграции внешнего AI сервиса,
а не встроенного inference, GenUI, автономных tools или production agent authorization.

## Запуск

Детерминированный режим без сети и модели, явно обозначенный в окне:

```powershell
$env:FLUI_AI_OFFLINE = '1'
cargo run --example ai_streaming
```

Live provider: endpoint — полный URL `chat/completions`, не base URL.
Модель задаётся пользователем; секрет не хранится в репозитории.

```powershell
Remove-Item Env:FLUI_AI_OFFLINE -ErrorAction SilentlyContinue
$env:FLUI_AI_ENDPOINT = 'https://your-provider.example/v1/chat/completions'
$env:FLUI_AI_MODEL = 'your-model-name'
# Установите FLUI_AI_API_KEY в окружении через свой способ управления секретами.
cargo run --example ai_streaming
```

Для локального OpenAI-compatible сервера допустим HTTP без API key.
HTTP с key, credentials в URL и redirects отклоняются. Пример не печатает
request body, URL, key или provider error body. Live Send отправляет введённый
prompt настроенному provider; только этот режим способен потреблять его квоту.
Необходим Chat Completions SSE с `delta.content` и завершающим `[DONE]`;
Responses API, tool calls, image/audio input и provider-specific events не реализованы.

## Существующие механизмы и границы

Service factory получает штатный `ServiceContext::spawner()` и публикует handle
в принадлежащий приложению `OnceLock`. Factory завершается установкой handle;
service future ждёт cancellation приложения. IO request исполняется на штатной
IO lane, а не в frame-thread `AsyncDriver` без network reactor.

`StreamBuilder` получает request revision как key. Новый request, Stop (`None`)
и dispose снимают старую subscription. Она владеет `TaskHandle`; Drop этого
handle запрашивает настоящую отмену IO future на await boundary. Уже полученные
события старой subscription не применяются через существующую generation
проверку `AsyncSlot`. Last-window close отменяет service и loop tasks.
Отмена соединения не обещает отменить биллинг или inference уже принятого
provider request: удалённый сервер определяет свой жизненный цикл.

Bridge — bounded Tokio channel из восьми cumulative snapshots. Producer ждёт
свободного места; incremental deltas не теряются от coalescing последнего
widget snapshot. Consumer делает Pending между событиями, чтобы постоянно
готовый stream не захватил один frame-thread poll. Ошибки отображаются в UI;
ошибочный HTTP status показывается без response body.

Лимиты примера: prompt 8 KiB UTF-8, generated text 64 KiB, SSE line/event 64 KiB,
response wire 2 MiB, не более 1024 SSE data events на запрос (включая события без
текстовой delta), connect timeout 10 s, request timeout 90 s, provider
`max_tokens=512`. Byte limits гарантируют локальные bounds, а `max_tokens`
зависит от поддержки provider. Parser поддерживает разбиение UTF-8 и CRLF
между network chunks; oversized/malformed payload возвращает error.

Существующий `AgentServer` остаётся отдельным debug-only agent endpoint.
Пример не подключает его к model output и не выдаёт сети право действий.
Чтобы честно заявить tool execution, следующая реализация должна связывать
конкретные существующие protocol actions с host authorization, live
preconditions и reviewable результатом. Этот пример такого заявления не делает.

## Проверка

Выполнено `cargo test --locked --example ai_streaming`: sse_contract 1/1 passed.
`cargo clippy --locked --example ai_streaming --example embedded_gpu_scene -- -D warnings`
прошёл. Native build с a11y и existing desktop-mcp smoke выполнены: Send показывает
streaming, replacement request 2 завершается без request 1 в UI, Stop остаётся
Stopped, provider 503 показывает error. Controlled localhost SSE использует
реальный reqwest transport, без внешних credentials/billing; server наблюдал
disconnect у заменённого и остановленного запросов. Сценарий и artifacts находятся
в ignored target/engine-audit/demo_mcp_smoke.py и target/engine-audit/mcp-smoke;
network-observations.json содержит request ordinals и write failures.

```powershell
cargo test --example ai_streaming
cargo clippy --example ai_streaming -- -D warnings
cargo xtask check-changed
```

`sse_contract` проверяет каждый возможный split byte offset русскоязычного
SSE+CRLF ответа, completion, malformed JSON и oversized line. Для честного
acceptance нужны runtime/UI проверки: Send показывает несколько snapshots;
Stop предотвращает дальнейший ответ; Send во время stream заменяет запрос;
ошибка connection/status/JSON видна; close прекращает owner work.
Внешний configured LLM provider не проверен: localhost fixture подтверждает
transport/lifecycle, не model quality. Normal window close не проверен — fixture
cleanup завершал только owned processes. Native malformed/oversize/recovery не
объявляются покрытыми parser unit test. Smoke script — воспроизводимый ignored
artifact, не новая checked-in CI family; дальнейшая migration должна закрепить
нужные contracts в existing tests/tooling.

Публичные фундаментальные API здесь не изменены. Если пример выявит отсутствие
доставки cancellation/wake/error через существующий контракт, это исправляется
в его владельце с behavior test, а не новым параллельным executor.
