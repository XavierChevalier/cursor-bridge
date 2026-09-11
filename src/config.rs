//! Runtime configuration (from env in main; explicit in tests).

use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct Config {
    pub api_key: String,
    pub agent_bin: String,
    pub workspace: String,
    /// Bridge model id returned when the client omits `model`.
    pub default_model_id: String,
    /// Bridge model id → Cursor CLI `--model` value.
    pub models: BTreeMap<String, String>,
}

impl Config {
    /// Deterministic config for contract tests (not for production).
    pub fn test_default() -> Self {
        let mut models = BTreeMap::new();
        models.insert("cursor-auto".to_string(), "default".to_string());
        Self {
            api_key: "test-bridge-key".to_string(),
            agent_bin: "agent".to_string(),
            workspace: ".".to_string(),
            default_model_id: "cursor-auto".to_string(),
            models,
        }
    }

    /// Load from environment variables (see `.env.example`).
    pub fn from_env() -> Self {
        let api_key = std::env::var("CURSOR_BRIDGE_API_KEY").unwrap_or_default();
        if api_key.is_empty() {
            tracing::warn!("CURSOR_BRIDGE_API_KEY is empty; all authenticated routes will 401");
        }

        let default_model_id = std::env::var("CURSOR_BRIDGE_MODEL_ID")
            .unwrap_or_else(|_| "cursor-auto".into());
        let default_cursor = std::env::var("CURSOR_BRIDGE_CURSOR_MODEL")
            .unwrap_or_else(|_| "default".into());

        let mut models = BTreeMap::new();
        models.insert(default_model_id.clone(), default_cursor);

        // Optional extras: "id:cursor-model,id2:cursor-model2"
        if let Ok(extra) = std::env::var("CURSOR_BRIDGE_EXTRA_MODELS") {
            for part in extra.split(',') {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                if let Some((id, cursor)) = part.split_once(':') {
                    let id = id.trim();
                    let cursor = cursor.trim();
                    if !id.is_empty() && !cursor.is_empty() {
                        models.insert(id.to_string(), cursor.to_string());
                    }
                }
            }
        }

        Self {
            api_key,
            agent_bin: std::env::var("CURSOR_BRIDGE_AGENT_BIN").unwrap_or_else(|_| "agent".into()),
            workspace: std::env::var("CURSOR_BRIDGE_WORKSPACE")
                .unwrap_or_else(|_| "./workspace".into()),
            default_model_id,
            models,
        }
    }

    pub fn resolve_model(&self, requested: &str) -> Option<&str> {
        let key = if requested.is_empty() {
            self.default_model_id.as_str()
        } else {
            requested
        };
        self.models.get(key).map(String::as_str)
    }

    pub fn default_model_id(&self) -> &str {
        &self.default_model_id
    }
}
