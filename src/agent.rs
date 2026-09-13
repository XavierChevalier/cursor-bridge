//! Spawn the Cursor CLI (or fake agent) for print turns and setup login.

use std::process::Stdio;
use std::time::Duration;

use futures_util::StreamExt;
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

/// One Open WebUI-facing piece derived from a Cursor stream-json line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeDelta {
    /// Maps to SSE `delta.reasoning_content` / message.reasoning_content.
    Reasoning(String),
    /// Maps to SSE `delta.content` (assistant text or tool `<details>` blocks).
    Content(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TurnResult {
    pub content: String,
    pub reasoning_content: String,
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

/// Run one non-interactive turn via stream-json and assemble Computer-facing fields.
pub async fn print_turn(
    config: &Config,
    cursor_model: &str,
    prompt: &str,
) -> Result<TurnResult, AgentError> {
    let mut stream = stream_print_turn(config, cursor_model, prompt)?;
    let mut out = TurnResult::default();
    while let Some(item) = stream.next().await {
        match item? {
            BridgeDelta::Reasoning(piece) => out.reasoning_content.push_str(&piece),
            BridgeDelta::Content(piece) => out.content.push_str(&piece),
        }
    }
    Ok(out)
}

/// Live print turn: Cursor `stream-json` + `--stream-partial-output` deltas.
///
/// Yields reasoning, tool traces (as content `<details>`), and assistant text.
/// Duplicate buffered assistant flushes are skipped per Cursor CLI docs.
pub fn stream_print_turn(
    config: &Config,
    cursor_model: &str,
    prompt: &str,
) -> Result<
    std::pin::Pin<Box<dyn futures_util::Stream<Item = Result<BridgeDelta, AgentError>> + Send>>,
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
                    if let Some(delta) = map_stream_json_line(&line) {
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

/// Map one Cursor stream-json NDJSON line into an Open WebUI-facing delta.
pub fn map_stream_json_line(line: &str) -> Option<BridgeDelta> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    match value.get("type")?.as_str()? {
        "thinking" => map_thinking_event(&value),
        "tool_call" => map_tool_call_event(&value),
        "assistant" => extract_stream_json_delta(line).map(BridgeDelta::Content),
        _ => None,
    }
}

fn map_thinking_event(value: &serde_json::Value) -> Option<BridgeDelta> {
    match value
        .get("subtype")
        .and_then(|s| s.as_str())
        .unwrap_or("delta")
    {
        "delta" => {
            let text = value.get("text")?.as_str()?;
            if text.is_empty() {
                None
            } else {
                Some(BridgeDelta::Reasoning(text.to_string()))
            }
        }
        _ => None,
    }
}

fn map_tool_call_event(value: &serde_json::Value) -> Option<BridgeDelta> {
    let subtype = value.get("subtype")?.as_str()?;
    let call_id = value
        .get("call_id")
        .and_then(|v| v.as_str())
        .unwrap_or("tool");
    let tool_obj = value.get("tool_call")?.as_object()?;
    let (raw_name, payload) = tool_obj.iter().next()?;
    let name = humanize_tool_name(raw_name);
    let args = payload.get("args").cloned().unwrap_or(serde_json::json!({}));
    let args_attr = html_attr_escape(&args.to_string());

    match subtype {
        "started" => Some(BridgeDelta::Content(format!(
            "<details type=\"tool_calls\" done=\"false\" id=\"{id}\" name=\"{name}\" arguments=\"{args}\">\n<summary>{name}</summary>\n</details>\n",
            id = html_attr_escape(call_id),
            name = html_attr_escape(&name),
            args = args_attr,
        ))),
        "completed" => {
            let result = payload
                .get("result")
                .map(|r| r.to_string())
                .unwrap_or_else(|| "{}".to_string());
            Some(BridgeDelta::Content(format!(
                "<details type=\"tool_calls\" done=\"true\" id=\"{id}\" name=\"{name}\" arguments=\"{args}\">\n<summary>{name}</summary>\n{result}\n</details>\n",
                id = html_attr_escape(call_id),
                name = html_attr_escape(&name),
                args = args_attr,
                result = html_body_escape(&result),
            )))
        }
        _ => None,
    }
}

fn humanize_tool_name(raw: &str) -> String {
    let trimmed = raw.strip_suffix("ToolCall").unwrap_or(raw);
    if trimmed.is_empty() {
        return "tool".to_string();
    }
    let mut chars = trimmed.chars();
    let Some(first) = chars.next() else {
        return "tool".to_string();
    };
    format!("{}{}", first.to_ascii_uppercase(), chars.as_str())
}

fn html_attr_escape(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn html_body_escape(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Extract a live text delta from one `stream-json` NDJSON assistant line.
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
