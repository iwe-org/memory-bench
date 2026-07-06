use std::collections::{BTreeSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::answer::{BASE_DISALLOWED, FILE_TOOLS};
use crate::claude::{self, Invocation};
use crate::locomo::{self, Conversation, Session};
use crate::prepare;
use crate::records;

const CURATE_TEMPLATE: &str = include_str!("../prompts/curate.md");
const CONSOLIDATE_TEMPLATE: &str = include_str!("../prompts/consolidate.md");
const CONSOLIDATE_MARKER: u32 = 10_000;

const CURATOR_TOOLS: &[&str] = &[
    "mcp__iwe__iwe_find",
    "mcp__iwe__iwe_retrieve",
    "mcp__iwe__iwe_tree",
    "mcp__iwe__iwe_squash",
    "mcp__iwe__iwe_stats",
    "mcp__iwe__iwe_create",
    "mcp__iwe__iwe_update",
    "mcp__iwe__iwe_delete",
    "mcp__iwe__iwe_rename",
    "mcp__iwe__iwe_query",
];
const DISALLOWED_IWE_TOOLS: &[&str] = &[
    "mcp__iwe__iwe_extract",
    "mcp__iwe__iwe_inline",
    "mcp__iwe__iwe_normalize",
    "mcp__iwe__iwe_attach",
];

const MAX_CONSECUTIVE_FAILURES: usize = 3;

pub struct CurateConfig {
    pub data: PathBuf,
    pub workspaces: PathBuf,
    pub conversation_filter: Option<BTreeSet<String>>,
    pub model: String,
    pub workers: usize,
    pub max_budget_usd: f64,
    pub timeout_secs: u64,
    pub consolidate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CurateRecord {
    session: u32,
    total_cost_usd: f64,
    num_turns: u32,
    duration_ms: u64,
    summary: String,
}

fn render_transcript(session: &Session) -> String {
    let mut lines = Vec::new();
    for turn in &session.turns {
        let mut line = format!("{}: {}", turn.speaker, turn.text);
        if let Some(caption) = &turn.blip_caption {
            line.push_str(&format!(" [shared photo: {caption}]"));
        }
        lines.push(line);
    }
    lines.join("\n")
}

fn substitute(template: &str, conversation: &Conversation, session: &Session) -> String {
    template
        .replace("{speaker_a}", &conversation.speaker_a)
        .replace("{speaker_b}", &conversation.speaker_b)
        .replace("{session_date}", &session.timestamp)
        .replace("{transcript}", &render_transcript(session))
}

fn md_page_count(workspace: &Path) -> Result<usize> {
    Ok(std::fs::read_dir(workspace)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "md"))
        .count())
}

fn curate_session(
    config: &CurateConfig,
    conversation: &Conversation,
    workspace: &Path,
    session: &Session,
) -> Result<CurateRecord> {
    let pages_before = md_page_count(workspace)?;
    let prompt = substitute(CURATE_TEMPLATE, conversation, session);
    let disallowed = [BASE_DISALLOWED, FILE_TOOLS, DISALLOWED_IWE_TOOLS].concat();
    let mcp_config = workspace.join(".mcp.json");
    let result = claude::run(&Invocation {
        cwd: workspace,
        prompt: &prompt,
        model: &config.model,
        allowed_tools: CURATOR_TOOLS,
        disallowed_tools: &disallowed,
        mcp_config: Some(&mcp_config),
        max_budget_usd: config.max_budget_usd,
        timeout: Duration::from_secs(config.timeout_secs),
    })?;
    let summary = result.result.clone().unwrap_or_default();
    anyhow::ensure!(
        !result.is_error,
        "claude returned an error ({}): {}",
        result.subtype,
        summary.chars().take(200).collect::<String>()
    );
    anyhow::ensure!(
        md_page_count(workspace)? > pages_before,
        "session {} produced no new pages (MCP tools unavailable?): {}",
        session.number,
        summary.chars().take(200).collect::<String>()
    );
    Ok(CurateRecord {
        session: session.number,
        total_cost_usd: result.total_cost_usd,
        num_turns: result.num_turns,
        duration_ms: result.duration_ms,
        summary: summary.trim().to_string(),
    })
}

fn consolidate_conversation(
    config: &CurateConfig,
    conversation: &Conversation,
    workspace: &Path,
) -> Result<CurateRecord> {
    let prompt = CONSOLIDATE_TEMPLATE
        .replace("{speaker_a}", &conversation.speaker_a)
        .replace("{speaker_b}", &conversation.speaker_b);
    let disallowed = [BASE_DISALLOWED, FILE_TOOLS, DISALLOWED_IWE_TOOLS].concat();
    let mcp_config = workspace.join(".mcp.json");
    let result = claude::run(&Invocation {
        cwd: workspace,
        prompt: &prompt,
        model: &config.model,
        allowed_tools: CURATOR_TOOLS,
        disallowed_tools: &disallowed,
        mcp_config: Some(&mcp_config),
        max_budget_usd: config.max_budget_usd,
        timeout: Duration::from_secs(config.timeout_secs),
    })?;
    let summary = result.result.clone().unwrap_or_default();
    anyhow::ensure!(
        !result.is_error,
        "claude returned an error ({}): {}",
        result.subtype,
        summary.chars().take(200).collect::<String>()
    );
    Ok(CurateRecord {
        session: CONSOLIDATE_MARKER,
        total_cost_usd: result.total_cost_usd,
        num_turns: result.num_turns,
        duration_ms: result.duration_ms,
        summary: summary.trim().to_string(),
    })
}

fn curate_conversation(config: &CurateConfig, conversation: &Conversation) -> Result<()> {
    let workspace = config
        .workspaces
        .join("curated")
        .join(&conversation.sample_id);
    std::fs::create_dir_all(&workspace)?;
    prepare::init_iwe(&workspace)?;
    prepare::write_mcp_config(&workspace)?;
    let log_path = workspace.join("curate-log.jsonl");
    let logged: Vec<CurateRecord> = records::read_jsonl(&log_path)?;
    let done: BTreeSet<u32> = logged.iter().map(|record| record.session).collect();
    let mut total_cost: f64 = logged.iter().map(|record| record.total_cost_usd).sum();
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let mut failures = 0;
    for session in &conversation.sessions {
        if done.contains(&session.number) {
            continue;
        }
        loop {
            match curate_session(config, conversation, &workspace, session) {
                Ok(record) => {
                    failures = 0;
                    total_cost += record.total_cost_usd;
                    let line = serde_json::to_string(&record)?;
                    writeln!(log, "{line}")?;
                    println!(
                        "{} session {}: {}",
                        conversation.sample_id, session.number, record.summary
                    );
                    break;
                }
                Err(error) => {
                    failures += 1;
                    eprintln!(
                        "{} session {} failed ({failures}/{MAX_CONSECUTIVE_FAILURES}): {error:#}",
                        conversation.sample_id, session.number
                    );
                    anyhow::ensure!(
                        failures < MAX_CONSECUTIVE_FAILURES,
                        "aborted after {MAX_CONSECUTIVE_FAILURES} consecutive failures; rerun the same command to resume"
                    );
                }
            }
        }
    }
    if config.consolidate && !done.contains(&CONSOLIDATE_MARKER) {
        let record = consolidate_conversation(config, conversation, &workspace)?;
        total_cost += record.total_cost_usd;
        let line = serde_json::to_string(&record)?;
        writeln!(log, "{line}")?;
        println!(
            "{} consolidation: {}",
            conversation.sample_id, record.summary
        );
    }
    println!(
        "{}: curated {} sessions, total cost ${total_cost:.2}",
        conversation.sample_id,
        conversation.sessions.len()
    );
    Ok(())
}

pub fn run(config: &CurateConfig) -> Result<()> {
    let conversations = locomo::load(&config.data, None)?;
    let selected: VecDeque<&Conversation> = conversations
        .iter()
        .filter(|conversation| {
            config
                .conversation_filter
                .as_ref()
                .map(|f| f.contains(&conversation.sample_id))
                != Some(false)
        })
        .collect();
    let queue = Mutex::new(selected);
    let failed = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..config.workers.max(1) {
            scope.spawn(|| loop {
                let item = queue.lock().expect("queue lock").pop_front();
                let Some(conversation) = item else { break };
                if let Err(error) = curate_conversation(config, conversation) {
                    eprintln!("{}: {error:#}", conversation.sample_id);
                    failed
                        .lock()
                        .expect("failed lock")
                        .push(conversation.sample_id.clone());
                }
            });
        }
    });
    let failed = failed.into_inner().expect("failed conversations");
    anyhow::ensure!(
        failed.is_empty(),
        "conversations failed: {}; rerun the same command to resume",
        failed.join(", ")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locomo::{Qa, Turn};

    fn conversation() -> Conversation {
        Conversation {
            sample_id: "conv-1".to_string(),
            speaker_a: "Alice".to_string(),
            speaker_b: "Bob".to_string(),
            sessions: vec![Session {
                number: 1,
                timestamp: "1:00 pm on 1 May, 2023".to_string(),
                turns: vec![
                    Turn {
                        speaker: "Alice".to_string(),
                        text: "Hi Bob!".to_string(),
                        blip_caption: None,
                    },
                    Turn {
                        speaker: "Bob".to_string(),
                        text: "Look at this!".to_string(),
                        blip_caption: Some("a red bicycle".to_string()),
                    },
                ],
            }],
            qa: Vec::<Qa>::new(),
        }
    }

    #[test]
    fn renders_transcript() {
        assert_eq!(
            render_transcript(&conversation().sessions[0]),
            "Alice: Hi Bob!\n\
             Bob: Look at this! [shared photo: a red bicycle]"
        );
    }

    #[test]
    fn substitutes_prompt() {
        assert_eq!(
            substitute(
                "{speaker_a} and {speaker_b} met on {session_date}.\n\n{transcript}\n",
                &conversation(),
                &conversation().sessions[0]
            ),
            "Alice and Bob met on 1:00 pm on 1 May, 2023.\n\
             \n\
             Alice: Hi Bob!\n\
             Bob: Look at this! [shared photo: a red bicycle]\n"
        );
    }
}
