//! Cursor Bridge library root.

pub mod agent;
pub mod config;
pub mod routes;
pub mod setup;

pub use config::Config;
pub use setup::{AppOptions, AppState};

/// Build the HTTP application router (production defaults).
pub fn app(config: Config) -> axum::Router {
    // Tests that need async build should call app_with_options; production main awaits build_state.
    // For sync `app()` used by older tests: enable setup only if flag missing (optimistic).
    let flag_done = config.setup_complete_flag().is_file();
    let state = AppState {
        setup: setup::SetupGate::new(!flag_done),
        config,
    };
    routes::router(state)
}

/// Build router with explicit setup options (contract tests).
pub fn app_with_options(config: Config, options: AppOptions) -> axum::Router {
    let enabled = if options.force_setup {
        true
    } else {
        !config.setup_complete_flag().is_file()
    };
    let state = AppState {
        setup: setup::SetupGate::new(enabled),
        config,
    };
    routes::router(state)
}

/// Async builder used by the binary: checks agent status before enabling setup.
pub async fn app_async(config: Config) -> axum::Router {
    let state = setup::build_state(config, AppOptions::default()).await;
    routes::router(state)
}
