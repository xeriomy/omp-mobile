//! Bind the loopback listener and own the server task for embedding/tests.

use crate::api::{AppState, router};
use crate::auth::AuthStore;
use crate::config::BridgeConfig;
use crate::engine::{Engine, EngineConfig};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::time::{Duration, timeout};

pub struct RunningBridge {
    pub addr: SocketAddr,
    pub pairing_code: String,
    pub launch_summary: String,
    pub engine: Engine,
    shutdown_tx: Option<oneshot::Sender<()>>,
    server: Option<tokio::task::JoinHandle<()>>,
}

impl RunningBridge {
    pub async fn start(config: BridgeConfig) -> Result<Self, std::io::Error> {
        let launch_summary = format!(
            "{} {} (cwd {})",
            config.omp_bin.display(),
            config.omp_args.join(" "),
            config.workspace.display()
        );
        let config = Arc::new(config);
        let (auth, pairing_code) =
            AuthStore::with_generated_code(config.pairing_ttl, config.token_ttl);
        let engine = Engine::start(EngineConfig::from(config.as_ref())).await;

        let listener = TcpListener::bind(config.listen).await?;
        let addr = listener.local_addr()?;
        let state = AppState {
            engine: engine.clone(),
            auth,
            config: Arc::clone(&config),
            listen: addr,
        };
        let app = router(state);
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let result = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await;
            if let Err(error) = result {
                eprintln!("bridge server stopped: {error}");
            }
        });

        Ok(Self {
            addr,
            pairing_code,
            launch_summary,
            engine,
            shutdown_tx: Some(shutdown_tx),
            server: Some(server),
        })
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub async fn shutdown(mut self) {
        if let Some(sender) = self.shutdown_tx.take() {
            let _ = sender.send(());
        }
        if let Some(server) = self.server.take() {
            let _ = timeout(Duration::from_secs(3), server).await;
        }
        self.engine.shutdown().await;
    }
}

impl From<&BridgeConfig> for EngineConfig {
    fn from(config: &BridgeConfig) -> Self {
        Self {
            omp_bin: config.omp_bin.clone(),
            omp_args: config.omp_args.clone(),
            workspace: config.workspace.clone(),
            startup_timeout: config.startup_timeout,
            request_timeout: config.request_timeout,
            max_frame_bytes: config.max_frame_bytes,
            max_reassembled_bytes: config.max_reassembled_bytes,
        }
    }
}
