//! Two owner-local trees and an explicitly versioned document.
//!
//! `cargo heli run --locked -p flui-app --example multi_owner_document --features testing -- --headless`
//! exercises the real factory/coordinator with headless rendering. Without
//! `--headless`, the ordinary runner honestly refuses unqualified placement.
//! This example is not a declaration of native support.

#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
mod desktop {
    use anyhow::{Context as _, anyhow};
    use flui_app::{
        AppCapabilities, AppHandle, Application, DocumentOutcome, DocumentPort, DocumentSnapshot,
        RuntimeWindowConfig, SharedDocument, open_dedicated_window,
    };
    use flui_platform::capability::{Capability, ProviderContext};
    use flui_view::prelude::*;
    use std::{cell::RefCell, num::NonZeroUsize, rc::Rc, sync::mpsc};

    enum DocumentPolicy {}
    impl Capability for DocumentPolicy {
        type Handle = Rc<String>;
        const NAME: &'static str = "example-document-policy";
    }
    fn policy() -> Rc<String> {
        Rc::new("explicit application override".to_owned())
    }

    #[derive(Clone, StatefulView)]
    struct DocumentView {
        document: SharedDocument,
    }
    struct DocumentState {
        document: SharedDocument,
        policy: Option<Rc<String>>,
    }
    impl StatefulView for DocumentView {
        type State = DocumentState;
        fn create_state(&self) -> Self::State {
            DocumentState {
                document: self.document.clone(),
                policy: None,
            }
        }
    }
    impl ViewState<DocumentView> for DocumentState {
        fn init_state(&mut self, cx: &dyn LifecycleContext) {
            let value = cx
                .capability::<DocumentPolicy>()
                .expect("BUG: example reconstructs its policy");
            assert_eq!(value.as_str(), "explicit application override");
            self.policy = Some(value);
        }
        fn build(&self, _: &DocumentView, _: &dyn BuildContext) -> impl IntoView {
            let document = self.document.clone();
            let policy = self.policy.clone().expect("BUG: lifecycle precedes build");
            flui_widgets::FutureBuilder::keyed(
                Some(()),
                Rc::new(move || {
                    let document = document.clone();
                    Box::pin(async move {
                        let port = document.bind()?;
                        let snapshot = read(&port).await?;
                        Ok::<String, anyhow::Error>(
                            String::from_utf8_lossy(snapshot.bytes()).into_owned(),
                        )
                    })
                }),
                Rc::new(move |_, snapshot| {
                    let text = snapshot.data().cloned().unwrap_or_else(|| {
                        snapshot
                            .error()
                            .map_or_else(|| "Waiting for document".to_owned(), ToString::to_string)
                    });
                    flui_widgets::Text::new(format!("{}: {text}", policy.as_str())).boxed()
                }),
            )
        }
    }

    async fn read(port: &DocumentPort) -> anyhow::Result<DocumentSnapshot> {
        match port.read()?.await {
            DocumentOutcome::Current(value) => Ok(value),
            other => Err(anyhow!("read refused: {other:?}")),
        }
    }

    async fn edit(first: DocumentPort, second: DocumentPort) -> anyhow::Result<()> {
        let stale = read(&second).await?;
        match first
            .replace(stale.revision(), b"accepted edit".to_vec())?
            .await
        {
            DocumentOutcome::Current(_) => {}
            other => return Err(anyhow!("first edit failed: {other:?}")),
        }
        match second
            .replace(stale.revision(), b"retained conflicting edit".to_vec())?
            .await
        {
            DocumentOutcome::Conflict { current, rejected } => {
                anyhow::ensure!(current.bytes() == b"accepted edit");
                anyhow::ensure!(rejected == b"retained conflicting edit");
                println!("Conflict preserved both the committed document and the rejected edit.");
                Ok(())
            }
            other => Err(anyhow!("stale edit was not rejected: {other:?}")),
        }
    }

    pub(super) fn run() -> anyhow::Result<()> {
        let document = Rc::new(RefCell::new(None::<SharedDocument>));
        let initial = Rc::clone(&document);
        let (finished, outcome) = mpsc::channel();
        let application = Application::new(move |_: &AppHandle| DocumentView {
            document: initial
                .borrow()
                .clone()
                .expect("BUG: on_ready creates the document"),
        })
        .capability::<DocumentPolicy>(|_: &ProviderContext<'_>| Ok(policy()))
        .on_ready(move |handle| {
            let result = (|| -> anyhow::Result<_> {
                let shared = handle.create_document(
                    b"initial document".to_vec(),
                    NonZeroUsize::new(64).expect("BUG: nonzero bound"),
                )?;
                document.replace(Some(shared.clone()));
                let first = shared.bind()?;
                let (ports, port) = mpsc::channel();
                let request = open_dedicated_window(move || {
                    ports
                        .send(
                            shared
                                .bind()
                                .expect("BUG: factory is registered before invocation"),
                        )
                        .expect("BUG: observer alive during launch");
                    (
                        DocumentView { document: shared },
                        RuntimeWindowConfig::new()
                            .with_title("Dedicated document owner")
                            .with_capabilities(
                                AppCapabilities::new().capability::<DocumentPolicy>(
                                    |_: &ProviderContext<'_>| Ok(policy()),
                                ),
                            ),
                    )
                })?;
                Ok((request, first, port))
            })();
            let handle = handle.clone();
            std::thread::spawn(move || {
                // Wait only on this background observer. Neither UI loop blocks.
                let result = result.and_then(|(request, first, port)| {
                    pollster::block_on(request)?;
                    let second = port
                        .recv()
                        .context("destination did not bind its document")?;
                    pollster::block_on(edit(first, second))
                });
                let _ = handle.request_quit();
                let _ = finished.send(result);
            });
        });
        #[cfg(feature = "testing")]
        if std::env::args().any(|arg| arg == "--headless") {
            let run = flui_app::testing::HeadlessRun::start(application);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            while run.is_running() {
                run.run_owner_turn();
                anyhow::ensure!(
                    std::time::Instant::now() < deadline,
                    "headless example timed out"
                );
                std::thread::yield_now();
            }
            anyhow::ensure!(run.join()?.code() == 0, "headless runner failed");
            return outcome.recv().context("observer did not finish")?;
        }
        application.run()?;
        outcome.recv().context("observer did not finish")?
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
fn main() -> anyhow::Result<()> {
    desktop::run()
}

#[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
fn main() {
    eprintln!("Dedicated document owners are unavailable on this host.");
}
