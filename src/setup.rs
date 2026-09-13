//! One-shot setup UI state: enabled until Cursor login succeeds once.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::agent::{self, LoginProcess};
use crate::Config;

const MODEL_DISCOVERY_TTL: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct SetupGate {
    enabled: Arc<AtomicBool>,
}

impl SetupGate {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(enabled)),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    pub fn disable(&self) {
        self.enabled.store(false, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct ModelCacheInner {
    fetched_at: Option<Instant>,
    /// Merged static + discovered bridge id → Cursor `--model` value.
    models: BTreeMap<String, String>,
}

/// Cached merge of env allowlist + `agent models` discovery.
#[derive(Clone, Default)]
pub struct ModelCatalog {
    cache: Arc<Mutex<ModelCacheInner>>,
}

impl ModelCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Effective allowlist for `/v1/models` and chat resolution.
    pub async fn effective_models(&self, config: &Config) -> BTreeMap<String, String> {
        if !config.discover_models {
            return config.models.clone();
        }

        {
            let guard = self.cache.lock().await;
            if let Some(at) = guard.fetched_at {
                if at.elapsed() < MODEL_DISCOVERY_TTL && !guard.models.is_empty() {
                    return guard.models.clone();
                }
            }
        }

        let mut models = config.models.clone();
        match agent::list_cursor_models(config).await {
            Ok(discovered) => {
                for m in discovered {
                    // Keep a single Auto entry as the configured default bridge id.
                    if m.id == "auto" || m.id == "default" {
                        continue;
                    }
                    models.entry(m.id.clone()).or_insert(m.id);
                }
            }
            Err(err) => {
                tracing::warn!(error = %err, "agent models discovery failed; using static allowlist");
            }
        }

        let mut guard = self.cache.lock().await;
        guard.fetched_at = Some(Instant::now());
        guard.models = models.clone();
        models
    }

    pub async fn resolve_model(&self, config: &Config, requested: &str) -> Option<String> {
        let models = self.effective_models(config).await;
        let key = if requested.is_empty() {
            config.default_model_id.as_str()
        } else {
            requested
        };
        models.get(key).cloned()
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub setup: SetupGate,
    pub models: ModelCatalog,
    /// Active `agent login` child; must stay alive until OAuth completes.
    pub login: Arc<Mutex<Option<LoginProcess>>>,
}

pub struct AppOptions {
    /// Force setup UI on regardless of flag/status (tests).
    pub force_setup: bool,
}

impl Default for AppOptions {
    fn default() -> Self {
        Self { force_setup: false }
    }
}

pub async fn build_state(config: Config, options: AppOptions) -> AppState {
    let flag = config.setup_complete_flag();
    let flag_done = flag.is_file();

    let logged_in = if options.force_setup {
        false
    } else {
        match agent::status(&config).await {
            Ok(status) => status.logged_in,
            Err(err) => {
                tracing::warn!(error = %err, "agent status failed; assuming logged out");
                false
            }
        }
    };

    let enabled = if options.force_setup {
        true
    } else {
        !flag_done && !logged_in
    };

    if enabled {
        tracing::info!("setup UI enabled (one-shot agent login)");
    } else {
        tracing::info!("setup UI disabled");
    }

    AppState {
        config,
        setup: SetupGate::new(enabled),
        models: ModelCatalog::new(),
        login: Arc::new(Mutex::new(None)),
    }
}

pub fn app_state_for_tests(config: Config, force_setup: bool) -> AppState {
    let enabled = if force_setup {
        true
    } else {
        !config.setup_complete_flag().is_file()
    };
    AppState {
        config,
        setup: SetupGate::new(enabled),
        models: ModelCatalog::new(),
        login: Arc::new(Mutex::new(None)),
    }
}

pub async fn mark_setup_complete(state: &AppState) -> std::io::Result<()> {
    std::fs::create_dir_all(&state.config.state_dir)?;
    std::fs::write(state.config.setup_complete_flag(), b"1\n")?;
    state.setup.disable();
    if let Some(proc) = state.login.lock().await.take() {
        proc.abort().await;
    }
    Ok(())
}
