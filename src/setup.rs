//! One-shot setup UI state: enabled until Cursor login succeeds once.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::agent;
use crate::Config;

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

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub setup: SetupGate,
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
    }
}

pub fn mark_setup_complete(state: &AppState) -> std::io::Result<()> {
    std::fs::create_dir_all(&state.config.state_dir)?;
    std::fs::write(state.config.setup_complete_flag(), b"1\n")?;
    state.setup.disable();
    Ok(())
}
