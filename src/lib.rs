//! Cursor Bridge library root.

pub mod agent;
pub mod config;
pub mod routes;
pub mod setup;

pub use config::Config;
pub use setup::{AppOptions, AppState};

/// Build the HTTP application router (production defaults).
pub fn app(config: Config) -> axum::Router {
    let state = setup::app_state_for_tests(config, false);
    routes::router(state)
}

/// Build router with explicit setup options (contract tests).
pub fn app_with_options(config: Config, options: AppOptions) -> axum::Router {
    let state = setup::app_state_for_tests(config, options.force_setup);
    routes::router(state)
}

/// Async builder used by the binary: checks agent status before enabling setup.
pub async fn app_async(config: Config) -> axum::Router {
    let state = setup::build_state(config, AppOptions::default()).await;
    routes::router(state)
}
