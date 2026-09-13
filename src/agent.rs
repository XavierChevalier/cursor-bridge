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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredModel {
    pub id: String,
    pub label: String,
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
    // Non-interactive HTTP turns cannot answer CLI trust or permission prompts.
    // Workspace is operator-chosen (CURSOR_BRIDGE_WORKSPACE); trust it and force
    // allow tools unless denied in ~/.cursor/cli-config.json.
    let output = agent_command(config)
        .arg("-p")
        .arg("--trust")
        .arg("--force")
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

/// Live print turn: Cursor `stream-json` + `--stream-partial-output` deltas.
///
/// Yields assistant text pieces as they arrive. Duplicate buffered flushes
/// (pre-tool `model_call_id`, final flush without `timestamp_ms`) are skipped
/// per Cursor CLI output-format docs.
pub fn stream_print_turn(
    config: &Config,
    cursor_model: &str,
    prompt: &str,
) -> Result<
    std::pin::Pin<Box<dyn futures_util::Stream<Item = Result<String, AgentError>> + Send>>,
    AgentError,
> {
    let mut child = agent_command(config)
        .arg("-p")
        .arg("--trust")
        .arg("--force")
        .arg("--output-format")
        .arg("stream-json")
        .arg("--stream-partial-output")
        .arg("--model")
        .arg(cursor_model)
        .arg(prompt)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("agent stdout missing"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("agent stderr missing"))?;

    let stream = async_stream::stream! {
        let mut lines = BufReader::new(stdout).lines();
        let stderr_task = tokio::spawn(async move {
            let mut err_lines = BufReader::new(stderr).lines();
            let mut buf = String::new();
            while let Ok(Some(line)) = err_lines.next_line().await {
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(&line);
                if buf.len() > 8_192 {
                    break;
                }
            }
            buf
        });

        let mut io_error: Option<std::io::Error> = None;
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if let Some(delta) = extract_stream_json_delta(&line) {
                        yield Ok(delta);
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    io_error = Some(err);
                    break;
                }
            }
        }

        let status = match child.wait().await {
            Ok(status) => status,
            Err(err) => {
                yield Err(AgentError::Spawn(err));
                return;
            }
        };
        let stderr = stderr_task.await.unwrap_or_default();

        if let Some(err) = io_error {
            yield Err(AgentError::Spawn(err));
            return;
        }

        if !status.success() {
            yield Err(AgentError::Exit {
                status: status.code().unwrap_or(-1),
                stderr,
            });
        }
    };

    Ok(Box::pin(stream))
}

/// Extract a live text delta from one `stream-json` NDJSON line.
pub fn extract_stream_json_delta(line: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    if value.get("type")?.as_str()? != "assistant" {
        return None;
    }
    // Streaming deltas carry timestamp_ms and omit model_call_id.
    // Buffered flushes before tools include model_call_id; the final flush
    // omits timestamp_ms. Both are duplicates of prior delta text.
    if value.get("timestamp_ms").is_none() || value.get("model_call_id").is_some() {
        return None;
    }

    let mut text = String::new();
    let content = value.pointer("/message/content")?.as_array()?;
    for part in content {
        if part.get("type").and_then(|t| t.as_str()) != Some("text") {
            continue;
        }
        if let Some(piece) = part.get("text").and_then(|t| t.as_str()) {
            text.push_str(piece);
        }
    }
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Parse `agent models` / `--list-models` text (`id - label` lines).
pub fn parse_models_output(stdout: &str) -> Vec<DiscoveredModel> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line.eq_ignore_ascii_case("available models") {
            continue;
        }
        let Some((id, label)) = line.split_once(" - ") else {
            continue;
        };
        let id = id.trim();
        let label = label.trim();
        if id.is_empty() || label.is_empty() {
            continue;
        }
        if !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            continue;
        }
        out.push(DiscoveredModel {
            id: id.to_string(),
            label: label.to_string(),
        });
    }
    out
}

/// Run `agent models` and return account-available Cursor model ids.
pub async fn list_cursor_models(config: &Config) -> Result<Vec<DiscoveredModel>, AgentError> {
    let output = agent_command(config)
        .arg("models")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() && stdout.trim().is_empty() {
        return Err(AgentError::Exit {
            status: output.status.code().unwrap_or(-1),
            stderr: stderr.into_owned(),
        });
    }

    Ok(parse_models_output(&stdout))
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

#[cfg(test)]
mod tests {
    use super::extract_stream_json_delta;

    #[test]
    fn extract_keeps_partial_delta() {
        let line = r#"{"type":"assistant","timestamp_ms":1,"message":{"role":"assistant","content":[{"type":"text","text":"Hel"}]}}"#;
        assert_eq!(extract_stream_json_delta(line).as_deref(), Some("Hel"));
    }

    #[test]
    fn extract_skips_tool_flush_and_final_flush() {
        let tool_flush = r#"{"type":"assistant","timestamp_ms":1,"model_call_id":"x","message":{"role":"assistant","content":[{"type":"text","text":"Hel"}]}}"#;
        let final_flush = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Hello"}]}}"#;
        assert_eq!(extract_stream_json_delta(tool_flush), None);
        assert_eq!(extract_stream_json_delta(final_flush), None);
    }
}
