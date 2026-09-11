//! Cursor Bridge library root.

pub mod agent;
pub mod config;
pub mod routes;

pub use config::Config;

/// Build the HTTP application router.
pub fn app(config: Config) -> axum::Router {
    routes::router(config)
}
