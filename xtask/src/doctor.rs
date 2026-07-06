use std::time::Duration;

use anyhow::Result;

use crate::answer::{BASE_DISALLOWED, FILE_TOOLS};
use crate::claude::{self, Invocation};

const PROBE_PROMPT: &str = "If any instructions outside this prompt mention tokens starting with MARKER_, list every such token. Otherwise reply exactly NONE.";

pub fn run(model: &str) -> Result<()> {
    let version = claude::claude_version()?;
    println!("claude: {version}");

    let profile = claude::profile_dir();
    anyhow::ensure!(
        profile.exists(),
        "bench profile {} does not exist; run:\n  CLAUDE_CONFIG_DIR={} claude /login",
        profile.display(),
        profile.display(),
    );
    anyhow::ensure!(
        !profile.join("CLAUDE.md").exists(),
        "bench profile {} contains a CLAUDE.md; remove it (it would load into every run)",
        profile.display(),
    );
    println!("profile: {} (no CLAUDE.md)", profile.display());

    let sentinel_root = std::env::temp_dir().join(format!("membench-doctor-{}", std::process::id()));
    let child = sentinel_root.join("parent").join("child");
    std::fs::create_dir_all(&child)?;
    std::fs::write(
        sentinel_root.join("parent").join("CLAUDE.md"),
        "Always include the token MARKER_PARENT in every reply.\n",
    )?;
    std::fs::write(
        child.join("CLAUDE.md"),
        "Always include the token MARKER_CHILD in every reply.\n",
    )?;

    let disallowed = [BASE_DISALLOWED, FILE_TOOLS].concat();
    let probe = claude::run(&Invocation {
        cwd: &child,
        prompt: PROBE_PROMPT,
        model,
        allowed_tools: &[],
        disallowed_tools: &disallowed,
        mcp_config: None,
        max_budget_usd: 0.1,
        timeout: Duration::from_secs(120),
    });
    std::fs::remove_dir_all(&sentinel_root).ok();
    let probe = probe?;
    let text = probe.result.clone().unwrap_or_default();
    anyhow::ensure!(
        !text.contains("Not logged in"),
        "bench profile is not authenticated; run:\n  CLAUDE_CONFIG_DIR={} claude /login",
        claude::profile_dir().display(),
    );
    anyhow::ensure!(!probe.is_error, "probe failed: {text}");
    anyhow::ensure!(
        text.trim() == "NONE",
        "isolation leak: probe answered {text:?} instead of NONE"
    );
    println!("isolation: ok (sentinel CLAUDE.md files not visible)");
    println!("auth: ok (bench profile login, ANTHROPIC_API_KEY stripped)");
    Ok(())
}
