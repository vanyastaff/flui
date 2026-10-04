//! Real provider streaming through FLUI's service IO lane and StreamBuilder.
//!
//! FLUI_AI_ENDPOINT is the complete OpenAI-compatible /chat/completions URL.
//! FLUI_AI_MODEL selects a model; FLUI_AI_API_KEY is optional for local servers.
//! FLUI_AI_OFFLINE=1 selects an explicitly labelled deterministic demonstration.
//! Run: cargo run --example ai_streaming

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "support/ai_http.rs"]
mod ai_http;

#[cfg(not(target_arch = "wasm32"))]
mod desktop {
    use std::{
        pin::Pin,
        rc::Rc,
        sync::{Arc, OnceLock},
        task::{Context, Poll},
        time::Duration,
    };

    use flui::app::{ServiceDefinition, ServiceLifetime, TaskHandle, TaskSpawner};
    use flui::prelude::*;
    use flui::widgets::{Stream, StreamBuilder, TextEditingController, column, row};
    use tokio::sync::mpsc;

    const MAX_PROMPT_BYTES: usize = 8 * 1024;
    const MAX_TEXT_BYTES: usize = 64 * 1024;
    const MAX_EVENT_BYTES: usize = 64 * 1024;
    const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
    const MAX_EVENTS: usize = 1024;
    const CHANNEL_CAPACITY: usize = 8;

    #[derive(Clone)]
    enum Provider {
        Offline,
        Http {
            endpoint: reqwest::Url,
            model: String,
            api_key: Option<String>,
            client: Arc<tokio::sync::OnceCell<reqwest::Client>>,
        },
    }

    impl Provider {
        fn from_env() -> Result<Self, String> {
            if std::env::var("FLUI_AI_OFFLINE").as_deref() == Ok("1") {
                return Ok(Self::Offline);
            }
            let endpoint = std::env::var("FLUI_AI_ENDPOINT").map_err(|_| {
                "Set FLUI_AI_ENDPOINT and FLUI_AI_MODEL, or FLUI_AI_OFFLINE=1".to_owned()
            })?;
            let endpoint = reqwest::Url::parse(&endpoint)
                .map_err(|_| "FLUI_AI_ENDPOINT must be a valid HTTP(S) URL".to_owned())?;
            if !matches!(endpoint.scheme(), "http" | "https")
                || !endpoint.username().is_empty()
                || endpoint.password().is_some()
            {
                return Err("Use an HTTP(S) endpoint without credentials in its URL".to_owned());
            }
            let api_key = std::env::var("FLUI_AI_API_KEY")
                .ok()
                .filter(|key| !key.is_empty());
            if endpoint.scheme() == "http" && api_key.is_some() {
                return Err(
                    "API keys require HTTPS; local HTTP is supported without a key".to_owned(),
                );
            }
            let model = std::env::var("FLUI_AI_MODEL")
                .ok()
                .filter(|model| !model.trim().is_empty())
                .ok_or_else(|| "Set FLUI_AI_MODEL to your provider's model name".to_owned())?;
            Ok(Self::Http {
                endpoint,
                model,
                api_key,
                client: Arc::new(tokio::sync::OnceCell::new()),
            })
        }

        fn label(&self) -> &'static str {
            match self {
                Self::Offline => "OFFLINE deterministic stream — no AI provider is called",
                Self::Http { .. } => "Live OpenAI-compatible provider — plain text, no tools",
            }
        }
    }

    // Debug output never contains the API key or potentially credentialed URL.
    impl std::fmt::Debug for Provider {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.label())
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Request {
        revision: u64,
        prompt: String,
    }

    #[derive(Clone, Debug)]
    struct Update {
        text: String,
        complete: bool,
    }

    type Event = Result<Update, String>;

    struct ResponseStream {
        receiver: mpsc::Receiver<Event>,
        // Ownership of this handle couples subscription disposal to actual IO cancellation.
        _task: Option<TaskHandle<()>>,
        yield_next: bool,
        terminal: bool,
    }

    impl Stream for ResponseStream {
        type Item = Event;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Event>> {
            if self.terminal {
                return Poll::Ready(None);
            }
            // StreamBuilder drains ready events in one task poll. Yield between events
            // so a continuously ready network producer cannot monopolize the frame thread.
            if self.yield_next {
                self.yield_next = false;
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            match self.receiver.poll_recv(cx) {
                Poll::Ready(Some(event)) => {
                    self.terminal = event.as_ref().map_or(true, |update| update.complete);
                    self.yield_next = true;
                    Poll::Ready(Some(event))
                }
                Poll::Ready(None) => {
                    self.terminal = true;
                    Poll::Ready(Some(Err(
                        "Provider stream ended without a completion event".to_owned(),
                    )))
                }
                Poll::Pending => Poll::Pending,
            }
        }
    }

    fn response_stream(
        provider: Provider,
        spawner: &OnceLock<TaskSpawner>,
        prompt: String,
    ) -> ResponseStream {
        let (sender, receiver) = mpsc::channel(CHANNEL_CAPACITY);
        let task = if let Some(spawner) = spawner.get() {
            let errors = sender.clone();
            let admission_error = sender.clone();
            if let Ok(task) = spawner.spawn_io("ai-response", move |_cancel| async move {
                if let Err(error) = produce(provider, prompt, sender).await {
                    let _ = errors.send(Err(error)).await;
                }
            }) {
                Some(task)
            } else {
                let _ = admission_error
                    .try_send(Err("Application IO lane refused this request".to_owned()));
                None
            }
        } else {
            let _ = sender.try_send(Err("Application IO service is not ready".to_owned()));
            None
        };
        ResponseStream {
            receiver,
            _task: task,
            yield_next: false,
            terminal: false,
        }
    }

    async fn emit(sender: &mpsc::Sender<Event>, text: &str, complete: bool) -> Result<(), String> {
        sender
            .send(Ok(Update {
                text: text.to_owned(),
                complete,
            }))
            .await
            .map_err(|_| "Response subscription was closed".to_owned())
    }

    async fn produce(
        provider: Provider,
        prompt: String,
        sender: mpsc::Sender<Event>,
    ) -> Result<(), String> {
        if prompt.trim().is_empty() || prompt.len() > MAX_PROMPT_BYTES {
            return Err("Prompt must contain 1–8192 UTF-8 bytes".to_owned());
        }
        emit(&sender, "", false).await?;
        match provider {
            Provider::Offline => {
                let mut text = String::new();
                for part in [
                    "This is an offline demonstration. ",
                    "Tokens arrive on the service IO lane. ",
                    "FLUI displays cumulative snapshots without blocking layout or paint. ",
                    "Stop or send again to cancel the previous subscription.",
                ] {
                    tokio::time::sleep(Duration::from_millis(350)).await;
                    text.push_str(part);
                    emit(&sender, &text, false).await?;
                }
                emit(&sender, &text, true).await
            }
            Provider::Http {
                endpoint,
                model,
                api_key,
                client,
            } => {
                let client = client
                    .get_or_try_init(|| async {
                        reqwest::Client::builder()
                            .timeout(Duration::from_secs(90))
                            .connect_timeout(Duration::from_secs(10))
                            .redirect(reqwest::redirect::Policy::none())
                            .build()
                            .map_err(|_| "Could not create HTTP client".to_owned())
                    })
                    .await?;
                let mut request = client.post(endpoint).json(&serde_json::json!({
                    "model": model,
                    "stream": true,
                    "messages": [{"role": "user", "content": prompt}],
                    "max_tokens": 512
                }));
                if let Some(key) = api_key {
                    request = request.bearer_auth(key);
                }
                let mut response = request
                    .send()
                    .await
                    .map_err(|_| "Provider connection failed or timed out".to_owned())?;
                if !response.status().is_success() {
                    return Err(format!(
                        "Provider returned HTTP {}",
                        response.status().as_u16()
                    ));
                }
                let mut parser = EventParser::default();
                let mut text = String::new();
                let mut received = 0usize;
                let mut event_count = 0usize;
                while let Some(chunk) = response
                    .chunk()
                    .await
                    .map_err(|_| "Provider stream read failed".to_owned())?
                {
                    received = received
                        .checked_add(chunk.len())
                        .filter(|size| *size <= MAX_RESPONSE_BYTES)
                        .ok_or_else(|| "Provider response exceeded 2 MiB".to_owned())?;
                    for event in parser.push(&chunk)? {
                        event_count += 1;
                        if event_count > MAX_EVENTS {
                            return Err("Provider exceeded 1024 stream events".to_owned());
                        }
                        match decode_event(&event)? {
                            Piece::Done => return emit(&sender, &text, true).await,
                            Piece::Text(delta) => {
                                if text.len().saturating_add(delta.len()) > MAX_TEXT_BYTES {
                                    return Err("Generated text exceeded 64 KiB".to_owned());
                                }
                                text.push_str(&delta);
                                emit(&sender, &text, false).await?;
                            }
                            Piece::Other => {}
                        }
                    }
                }
                Err("Provider ended before SSE [DONE]".to_owned())
            }
        }
    }

    // Incremental byte framing keeps split UTF-8 and CRLF boundaries intact.
    #[derive(Default)]
    struct EventParser {
        line: Vec<u8>,
        data: String,
    }

    impl EventParser {
        fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>, String> {
            let mut events = Vec::new();
            for &byte in bytes {
                if byte != b'\n' {
                    if self.line.len() >= MAX_EVENT_BYTES {
                        return Err("SSE line exceeded 64 KiB".to_owned());
                    }
                    self.line.push(byte);
                    continue;
                }
                if self.line.last() == Some(&b'\r') {
                    self.line.pop();
                }
                let line = std::str::from_utf8(&self.line)
                    .map_err(|_| "Provider returned invalid UTF-8".to_owned())?;
                if line.is_empty() {
                    if !self.data.is_empty() {
                        events.push(std::mem::take(&mut self.data));
                    }
                } else if let Some(data) = line.strip_prefix("data:") {
                    let data = data.strip_prefix(' ').unwrap_or(data);
                    if self.data.len().saturating_add(data.len()).saturating_add(1)
                        > MAX_EVENT_BYTES
                    {
                        return Err("SSE event exceeded 64 KiB".to_owned());
                    }
                    if !self.data.is_empty() {
                        self.data.push('\n');
                    }
                    self.data.push_str(data);
                }
                self.line.clear();
            }
            Ok(events)
        }
    }

    enum Piece {
        Text(String),
        Done,
        Other,
    }

    fn decode_event(event: &str) -> Result<Piece, String> {
        if event.trim() == "[DONE]" {
            return Ok(Piece::Done);
        }
        let value: serde_json::Value = serde_json::from_str(event)
            .map_err(|_| "Provider returned malformed SSE JSON".to_owned())?;
        if value.get("error").is_some() {
            return Err("Provider reported a stream error".to_owned());
        }
        if let Some(choice) = value
            .get("choices")
            .and_then(|v| v.as_array())
            .and_then(|v| v.first())
            && let Some(content) = choice.pointer("/delta/content").and_then(|v| v.as_str())
        {
            return Ok(Piece::Text(content.to_owned()));
        }
        Ok(Piece::Other)
    }

    #[derive(Clone, StatelessView)]
    struct ChatApp {
        chat: ChatView,
    }

    impl StatelessView for ChatApp {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            self.chat.clone()
        }
    }

    #[derive(Clone, StatefulView)]
    struct ChatView {
        provider: Provider,
        spawner: Arc<OnceLock<TaskSpawner>>,
    }

    struct ChatState {
        request: Signal<Option<Request>>,
        revision: Signal<u64>,
        prompt: TextEditingController,
    }

    impl StatefulView for ChatView {
        type State = ChatState;
        fn create_state(&self) -> ChatState {
            let prompt = TextEditingController::new();
            prompt.set_text("Explain declarative UI in three short sentences.");
            ChatState {
                request: Signal::default(),
                revision: Signal::default(),
                prompt,
            }
        }
    }

    impl ViewState<ChatView> for ChatState {
        fn init_state(&mut self, ctx: &dyn LifecycleContext) {
            self.request = ctx.signal(None);
            self.revision = ctx.signal(0);
        }

        fn build(&self, view: &ChatView, ctx: &dyn BuildContext) -> impl IntoView {
            let request = self.request;
            let revision = self.revision;
            let prompt = self.prompt.clone();
            let key = request.get(ctx);
            let active = key.is_some();
            let provider = view.provider.clone();
            let spawner = Arc::clone(&view.spawner);
            let subscribed_prompt = key
                .as_ref()
                .map_or_else(String::new, |key| key.prompt.clone());
            let stream = StreamBuilder::<Request, Update, String>::keyed(
                key,
                Rc::new(move || {
                    Box::pin(response_stream(
                        provider.clone(),
                        &spawner,
                        subscribed_prompt.clone(),
                    ))
                }),
                Rc::new(move |_ctx, snapshot| {
                    let status = if !active {
                        "Stopped / ready — press Send".to_owned()
                    } else if let Some(error) = snapshot.error() {
                        format!("Error: {error}")
                    } else if let Some(update) = snapshot.data() {
                        if update.complete {
                            "Complete".to_owned()
                        } else {
                            "Streaming…".to_owned()
                        }
                    } else {
                        "Ready — press Send".to_owned()
                    };
                    let text = snapshot.data().map_or("", |update| update.text.as_str());
                    SingleChildScrollView::new()
                        .child(Column::new(column![
                            Text::new(status),
                            SizedBox::height(12.0),
                            Text::new(text),
                        ]))
                        .boxed()
                }),
            );
            SafeArea::new().child(Column::new(column![
                Text::new("FLUI AI streaming"),
                Text::new(view.provider.label()),
                SizedBox::height(12.0),
                RawTextField::new(self.prompt.clone()),
                Row::new(row![
                    RawButton::new(Text::new("Send / replace request")).on_press(move |cx| {
                        let current = revision.update(cx, |value| {
                            *value = value.saturating_add(1);
                            *value
                        })?;
                        request.set(
                            cx,
                            Some(Request {
                                revision: current,
                                prompt: prompt.text(),
                            }),
                        )
                    }),
                    SizedBox::width(16.0),
                    RawButton::new(Text::new("Stop")).on_press(move |cx| request.set(cx, None)),
                ]),
                SizedBox::height(16.0),
                Expanded::new(stream),
            ]))
        }
    }

    pub fn run() -> Result<(), String> {
        let provider = Provider::from_env()?;
        let spawner = Arc::new(OnceLock::new());
        let publish = Arc::clone(&spawner);
        let service = ServiceDefinition::new(
            "ai-network",
            ServiceLifetime::StopsWithLastWindow,
            move |ctx| {
                let _ = publish.set(ctx.spawner().clone());
                Box::pin(async move {
                    ctx.cancellation().cancelled().await;
                })
            },
        );
        flui::run_app_with_config(
            ChatApp {
                chat: ChatView { provider, spawner },
            },
            AppConfig::default()
                .with_title("FLUI AI streaming")
                .with_size(800, 600)
                .with_service(service),
        );
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn provider_reuses_http_connection_after_invalid_sse_and_delivers_text() {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("provider test runtime starts")
                .block_on(async {
                    let (endpoint, server) = crate::ai_http::spawn_server();
                    let provider = Provider::Http {
                        endpoint: reqwest::Url::parse(&endpoint).expect("loopback provider URL"),
                        model: "test-model".to_owned(),
                        api_key: None,
                        client: Arc::new(tokio::sync::OnceCell::new()),
                    };
                    for request in 0..3 {
                        let (sender, mut receiver) = mpsc::channel(CHANNEL_CAPACITY);
                        let result = tokio::time::timeout(
                            Duration::from_secs(5),
                            produce(provider.clone(), "hello".to_owned(), sender),
                        )
                        .await
                        .expect("provider request completes");
                        if request == 0 {
                            assert!(result.is_err(), "malformed provider JSON is rejected");
                        } else {
                            result.expect("valid provider response recovers");
                            let mut updates = Vec::new();
                            while let Ok(event) = receiver.try_recv() {
                                updates.push(event.expect("successful text update"));
                            }
                            let final_update = updates.last().expect("provider delivers updates");
                            assert!(final_update.complete);
                            assert_eq!(final_update.text, "Привет");
                        }
                    }
                    assert_eq!(
                        server.join().expect("provider server completed"),
                        1,
                        "provider clones must share the HTTP connection pool"
                    );
                });
        }

        #[test]
        fn sse_contract() {
            let wire = "data: {\"choices\":[{\"delta\":{\"content\":\"Привет\"}}]}\r\n\r\ndata: [DONE]\n\n";
            for split in 0..=wire.len() {
                let mut parser = EventParser::default();
                let mut events = parser
                    .push(&wire.as_bytes()[..split])
                    .expect("valid prefix");
                events.extend(
                    parser
                        .push(&wire.as_bytes()[split..])
                        .expect("valid suffix"),
                );
                assert!(
                    matches!(decode_event(&events[0]), Ok(Piece::Text(ref text)) if text == "Привет")
                );
                assert!(matches!(decode_event(&events[1]), Ok(Piece::Done)));
            }
            assert!(
                EventParser::default()
                    .push(&vec![b'x'; MAX_EVENT_BYTES + 1])
                    .is_err()
            );
            assert!(decode_event("{broken").is_err());
        }
    }
}

fn main() -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        desktop::run()
    }
    #[cfg(target_arch = "wasm32")]
    {
        Err("This service-lane example targets desktop; a browser provider adapter is not implemented".to_owned())
    }
}
