//! Spawn the Cursor CLI (or fake agent) for a single print-mode turn.

use std::process::Stdio;

use tokio::process::Command;

use crate::Config;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("failed to spawn agent: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("agent exited with status {status}")]
    Exit { status: i32, stderr: String },
    #[error("agent output was not valid utf-8")]
    Utf8,
}

/// Run one non-interactive print turn; return assistant text.
pub async fn print_turn(config: &Config, prompt: &str) -> Result<String, AgentError> {
    let output = Command::new(&config.agent_bin)
        .arg("-p")
        .arg("--output-format")
        .arg("text")
        .arg("--model")
        .arg(&config.cursor_model)
        .arg(prompt)
        .current_dir(&config.workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(AgentError::Exit {
            status: code,
            stderr,
        });
    }

    String::from_utf8(output.stdout).map_err(|_| AgentError::Utf8)
}
