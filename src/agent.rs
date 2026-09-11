//! Spawn the Cursor CLI (or fake agent) for print turns and setup login.

use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::timeout;

use crate::Config;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("failed to spawn agent: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("agent exited with status {status}: {stderr}")]
    Exit { status: i32, stderr: String },
    #[error("agent output was not valid utf-8")]
    Utf8,
    #[error("timed out waiting for login URL from agent")]
    LoginUrlTimeout,
    #[error("agent login produced no URL")]
    LoginUrlMissing,
}

#[derive(Debug, Clone)]
pub struct AgentStatus {
    pub logged_in: bool,
    pub summary: String,
}

/// A running `agent login` that must stay alive until the browser OAuth finishes.
pub struct LoginProcess {
    pub url: String,
    child: Child,
}

impl LoginProcess {
    pub async fn abort(mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}

fn agent_command(config: &Config) -> Command {
    let mut cmd = Command::new(&config.agent_bin);
    // Credentials must land in the persisted state dir / HOME, never under the CLI volume.
    cmd.current_dir(&config.workspace)
        .env("HOME", &config.state_dir)
        .env("NO_OPEN_BROWSER", "1")
        .env("FAKE_AGENT_STATE_DIR", &config.state_dir)
        .env("CURSOR_BRIDGE_STATE_DIR", &config.state_dir)
        .stdin(Stdio::null());
    cmd
}

/// Run one non-interactive print turn; return assistant text.
pub async fn print_turn(
    config: &Config,
    cursor_model: &str,
    prompt: &str,
) -> Result<String, AgentError> {
    let output = agent_command(config)
        .arg("-p")
        .arg("--output-format")
        .arg("text")
        .arg("--model")
        .arg(cursor_model)
        .arg(prompt)
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

/// Query `agent status` and classify login state.
pub async fn status(config: &Config) -> Result<AgentStatus, AgentError> {
    let output = agent_command(config)
        .arg("status")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let combined = if stdout.is_empty() {
        stderr.clone()
    } else {
        stdout.clone()
    };

    if !output.status.success() && combined.is_empty() {
        return Err(AgentError::Exit {
            status: output.status.code().unwrap_or(-1),
            stderr,
        });
    }

    let lower = combined.to_ascii_lowercase();
    let logged_in = !lower.contains("not logged in")
        && (lower.contains("logged in") || lower.contains("@"));

    Ok(AgentStatus {
        logged_in,
        summary: sanitize_status_line(&combined),
    })
}

/// Start `agent login`, capture the first https URL, and keep the process running.
pub async fn start_login(config: &Config) -> Result<LoginProcess, AgentError> {
    let mut child = agent_command(config)
        .arg("login")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("agent login stdout missing"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("agent login stderr missing"))?;

    let mut lines = BufReader::new(stdout).lines();
    let mut err_lines = BufReader::new(stderr).lines();

    let found = timeout(Duration::from_secs(30), async {
        loop {
            tokio::select! {
                line = lines.next_line() => {
                    let Some(line) = line? else { break; };
                    if let Some(url) = extract_https_url(&line) {
                        return Ok::<String, std::io::Error>(url);
                    }
                }
                line = err_lines.next_line() => {
                    let Some(line) = line? else { continue; };
                    if let Some(url) = extract_https_url(&line) {
                        return Ok(url);
                    }
                }
            }
        }
        Err(std::io::Error::other("EOF without login URL"))
    })
    .await;

    // Keep draining pipes so a chatty login child cannot block on a full buffer.
    tokio::spawn(async move {
        while let Ok(Some(_)) = lines.next_line().await {}
    });
    tokio::spawn(async move {
        while let Ok(Some(_)) = err_lines.next_line().await {}
    });

    match found {
        Ok(Ok(url)) => Ok(LoginProcess { url, child }),
        Ok(Err(_)) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(AgentError::LoginUrlMissing)
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(AgentError::LoginUrlTimeout)
        }
    }
}

fn extract_https_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let rest = &line[start..];
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
        .unwrap_or(rest.len());
    let url = rest[..end].trim_end_matches(['.', ',', ';', ')']);
    if url.len() > "https://".len() {
        Some(url.to_string())
    } else {
        None
    }
}

fn sanitize_status_line(raw: &str) -> String {
    raw.chars().take(200).collect()
}
