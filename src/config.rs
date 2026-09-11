//! Runtime configuration (from env in main; explicit in tests).

#[derive(Clone, Debug)]
pub struct Config {
    pub api_key: String,
    pub agent_bin: String,
    pub workspace: String,
    pub model_id: String,
    pub cursor_model: String,
}

impl Config {
    /// Deterministic config for contract tests (not for production).
    pub fn test_default() -> Self {
        Self {
            api_key: "test-bridge-key".to_string(),
            agent_bin: "agent".to_string(),
            workspace: ".".to_string(),
            model_id: "cursor-auto".to_string(),
            cursor_model: "default".to_string(),
        }
    }

    /// Load from environment variables (see `.env.example`).
    pub fn from_env() -> Self {
        let api_key = std::env::var("CURSOR_BRIDGE_API_KEY").unwrap_or_default();
        if api_key.is_empty() {
            tracing::warn!("CURSOR_BRIDGE_API_KEY is empty; all authenticated routes will 401");
        }
        Self {
            api_key,
            agent_bin: std::env::var("CURSOR_BRIDGE_AGENT_BIN").unwrap_or_else(|_| "agent".into()),
            workspace: std::env::var("CURSOR_BRIDGE_WORKSPACE")
                .unwrap_or_else(|_| "./workspace".into()),
            model_id: std::env::var("CURSOR_BRIDGE_MODEL_ID")
                .unwrap_or_else(|_| "cursor-auto".into()),
            cursor_model: std::env::var("CURSOR_BRIDGE_CURSOR_MODEL")
                .unwrap_or_else(|_| "default".into()),
        }
    }
}
