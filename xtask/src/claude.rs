use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;
use wait_timeout::ChildExt;

const STRIPPED_ENV: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
];

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeResult {
    pub result: Option<String>,
    #[serde(default)]
    pub subtype: String,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub total_cost_usd: f64,
    #[serde(default)]
    pub num_turns: u32,
    #[serde(default)]
    pub duration_ms: u64,
    pub session_id: Option<String>,
    #[serde(default)]
    pub usage: Usage,
}

pub struct Invocation<'a> {
    pub cwd: &'a Path,
    pub prompt: &'a str,
    pub model: &'a str,
    pub allowed_tools: &'a [&'a str],
    pub disallowed_tools: &'a [&'a str],
    pub mcp_config: Option<&'a Path>,
    pub max_budget_usd: f64,
    pub timeout: Duration,
}

pub fn profile_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("MEMBENCH_CLAUDE_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    std::env::current_dir()
        .expect("cwd")
        .join(".claude-profile")
}

pub fn claude_bin() -> String {
    std::env::var("CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string())
}

pub fn claude_version() -> Result<String> {
    let output = Command::new(claude_bin()).arg("--version").output()?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn run(invocation: &Invocation) -> Result<ClaudeResult> {
    let mut command = Command::new(claude_bin());
    command
        .current_dir(invocation.cwd)
        .env("CLAUDE_CONFIG_DIR", profile_dir())
        .env("DISABLE_AUTOUPDATER", "1")
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
        .args(["-p", "--output-format", "json"])
        .args(["--model", invocation.model])
        .args(["--setting-sources", ""])
        .arg("--strict-mcp-config")
        .arg("--no-session-persistence")
        .args(["--max-budget-usd", &invocation.max_budget_usd.to_string()]);
    for key in STRIPPED_ENV {
        command.env_remove(key);
    }
    if !invocation.allowed_tools.is_empty() {
        command.args(["--allowedTools", &invocation.allowed_tools.join(",")]);
    }
    if !invocation.disallowed_tools.is_empty() {
        command.args(["--disallowedTools", &invocation.disallowed_tools.join(",")]);
    }
    if let Some(mcp_config) = invocation.mcp_config {
        let absolute = std::fs::canonicalize(mcp_config)
            .with_context(|| format!("mcp config not found: {}", mcp_config.display()))?;
        command.args(["--mcp-config", &absolute.display().to_string()]);
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn().context("failed to spawn claude")?;
    child
        .stdin
        .take()
        .context("no stdin")?
        .write_all(invocation.prompt.as_bytes())?;
    let mut stdout = child.stdout.take().context("no stdout")?;
    let mut stderr = child.stderr.take().context("no stderr")?;
    let stdout_reader = std::thread::spawn(move || {
        let mut buffer = String::new();
        std::io::Read::read_to_string(&mut stdout, &mut buffer).ok();
        buffer
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buffer = String::new();
        std::io::Read::read_to_string(&mut stderr, &mut buffer).ok();
        buffer
    });

    let status = match child.wait_timeout(invocation.timeout)? {
        Some(status) => status,
        None => {
            child.kill().ok();
            child.wait().ok();
            anyhow::bail!("claude timed out after {:?}", invocation.timeout);
        }
    };
    let stdout_text = stdout_reader.join().expect("stdout reader");
    let stderr_text = stderr_reader.join().expect("stderr reader");

    serde_json::from_str(stdout_text.trim()).with_context(|| {
        format!(
            "cannot parse claude output (exit {status}): stdout={} stderr={}",
            stdout_text.chars().take(500).collect::<String>(),
            stderr_text.chars().take(500).collect::<String>(),
        )
    })
}
