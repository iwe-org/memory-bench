use std::collections::{BTreeSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::Result;

use crate::claude::{self, Invocation};
use crate::locomo::{self, Conversation, Qa};
use crate::prepare;
use crate::records::{existing_ids, AnswerRecord};

const ANSWER_TEMPLATE: &str = include_str!("../prompts/answer.md");
const FULL_CONTEXT_TEMPLATE: &str = include_str!("../prompts/full_context.md");
const CONTEXT_TEMPLATE: &str = include_str!("../prompts/answer_context.md");

pub const BASE_DISALLOWED: &[&str] = &[
    "Bash",
    "Write",
    "Edit",
    "NotebookEdit",
    "WebSearch",
    "WebFetch",
    "Task",
    "TodoWrite",
    "Skill",
    "SlashCommand",
    "BashOutput",
    "KillShell",
    "EnterPlanMode",
    "ExitPlanMode",
    "AskUserQuestion",
];
pub const FILE_TOOLS: &[&str] = &["Grep", "Glob", "Read", "LS"];
const IWE_READ_TOOLS: &[&str] = &[
    "mcp__iwe__iwe_find",
    "mcp__iwe__iwe_retrieve",
    "mcp__iwe__iwe_tree",
    "mcp__iwe__iwe_squash",
    "mcp__iwe__iwe_stats",
];
const IWE_WRITE_TOOLS: &[&str] = &[
    "mcp__iwe__iwe_create",
    "mcp__iwe__iwe_update",
    "mcp__iwe__iwe_delete",
    "mcp__iwe__iwe_rename",
    "mcp__iwe__iwe_extract",
    "mcp__iwe__iwe_inline",
    "mcp__iwe__iwe_normalize",
    "mcp__iwe__iwe_attach",
];
const IWE_QUERY_TOOLS: &[&str] = &["mcp__iwe__iwe_query"];

const MAX_CONSECUTIVE_FAILURES: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, clap::ValueEnum)]
pub enum Arm {
    Fs,
    Iwe,
    FsIwe,
    FullContext,
    Curated,
    CuratedFs,
    CuratedQ,
    CuratedCtx,
}

impl Arm {
    pub fn name(self) -> &'static str {
        match self {
            Arm::Fs => "fs",
            Arm::Iwe => "iwe",
            Arm::FsIwe => "fs-iwe",
            Arm::FullContext => "full-context",
            Arm::Curated => "curated",
            Arm::CuratedFs => "curated-fs",
            Arm::CuratedQ => "curated-q",
            Arm::CuratedCtx => "curated-ctx",
        }
    }

    fn workspace_kind(self) -> Option<&'static str> {
        match self {
            Arm::Fs => Some("fs"),
            Arm::Iwe | Arm::FsIwe => Some("iwe"),
            Arm::Curated | Arm::CuratedFs | Arm::CuratedQ | Arm::CuratedCtx => Some("curated"),
            Arm::FullContext => None,
        }
    }

    fn allowed_tools(self) -> Vec<&'static str> {
        match self {
            Arm::Fs | Arm::CuratedFs => FILE_TOOLS.to_vec(),
            Arm::Iwe | Arm::Curated => IWE_READ_TOOLS.to_vec(),
            Arm::FsIwe => [FILE_TOOLS, IWE_READ_TOOLS].concat(),
            Arm::FullContext => Vec::new(),
            Arm::CuratedQ => [IWE_READ_TOOLS, IWE_QUERY_TOOLS].concat(),
            Arm::CuratedCtx => Vec::new(),
        }
    }

    fn disallowed_tools(self) -> Vec<&'static str> {
        match self {
            Arm::Fs => BASE_DISALLOWED.to_vec(),
            Arm::Iwe | Arm::Curated => {
                [BASE_DISALLOWED, FILE_TOOLS, IWE_WRITE_TOOLS, IWE_QUERY_TOOLS].concat()
            }
            Arm::FsIwe => [BASE_DISALLOWED, IWE_WRITE_TOOLS, IWE_QUERY_TOOLS].concat(),
            Arm::FullContext => [BASE_DISALLOWED, FILE_TOOLS].concat(),
            Arm::CuratedFs => {
                [BASE_DISALLOWED, IWE_READ_TOOLS, IWE_WRITE_TOOLS, IWE_QUERY_TOOLS].concat()
            }
            Arm::CuratedQ => [BASE_DISALLOWED, FILE_TOOLS, IWE_WRITE_TOOLS].concat(),
            Arm::CuratedCtx => [
                BASE_DISALLOWED,
                FILE_TOOLS,
                IWE_READ_TOOLS,
                IWE_WRITE_TOOLS,
                IWE_QUERY_TOOLS,
            ]
            .concat(),
        }
    }

    fn uses_mcp(self) -> bool {
        matches!(self, Arm::Iwe | Arm::FsIwe | Arm::Curated | Arm::CuratedQ)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, clap::ValueEnum)]
pub enum Split {
    Dev,
    Test,
}

impl Split {
    pub fn conversations(self) -> BTreeSet<String> {
        let ids: &[&str] = match self {
            Split::Dev => &["conv-26", "conv-30"],
            Split::Test => &[
                "conv-41", "conv-42", "conv-43", "conv-44", "conv-47", "conv-48", "conv-49",
                "conv-50",
            ],
        };
        ids.iter().map(|id| id.to_string()).collect()
    }
}

pub struct AnswerConfig {
    pub run: PathBuf,
    pub arm: Arm,
    pub model: String,
    pub data: PathBuf,
    pub workspaces: PathBuf,
    pub categories: BTreeSet<u8>,
    pub conversation_filter: Option<BTreeSet<String>>,
    pub limit: Option<usize>,
    pub workers: usize,
    pub max_budget_usd: f64,
    pub timeout_secs: u64,
}

fn build_prompt(config: &AnswerConfig, conversation: &Conversation, qa: &Qa) -> Result<String> {
    let template = match config.arm {
        Arm::FullContext => FULL_CONTEXT_TEMPLATE
            .replace("{context}", &prepare::render_transcript(conversation)),
        Arm::CuratedCtx => {
            let workspace = config
                .workspaces
                .join("curated")
                .join(&conversation.sample_id);
            CONTEXT_TEMPLATE.replace("{context}", &render_dossier(&workspace, &qa.question)?)
        }
        _ => ANSWER_TEMPLATE.to_string(),
    };
    Ok(template
        .replace("{speaker_a}", &conversation.speaker_a)
        .replace("{speaker_b}", &conversation.speaker_b)
        .replace("{question}", &qa.question))
}

const DOSSIER_LIMIT: usize = 5;
const DOSSIER_MAX_TOKENS: usize = 12000;
const DOSSIER_MAX_CHARS: usize = 60000;

fn render_dossier(workspace: &Path, question: &str) -> Result<String> {
    let iwe = prepare::resolve_iwe()?;
    let output = std::process::Command::new(&iwe)
        .args([
            "retrieve",
            "--lexical",
            question,
            "--limit",
            &DOSSIER_LIMIT.to_string(),
            "--expand-references",
            "--expand-included-by",
            "--max-tokens",
            &DOSSIER_MAX_TOKENS.to_string(),
            "-f",
            "json",
        ])
        .current_dir(workspace)
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "iwe retrieve failed in {}: {}",
        workspace.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    let docs: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout)?;
    let mut context = String::new();
    for doc in &docs {
        let body = doc["content"].as_str().unwrap_or_default();
        if context.len() + body.len() > DOSSIER_MAX_CHARS {
            break;
        }
        context.push_str(body);
        context.push_str("\n\n---\n\n");
    }
    anyhow::ensure!(!context.is_empty(), "empty dossier for question");
    Ok(context)
}

fn bench_rev() -> Result<String> {
    let rev = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()?;
    anyhow::ensure!(rev.status.success(), "git rev-parse failed");
    let mut text = String::from_utf8_lossy(&rev.stdout).trim().to_string();
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()?;
    anyhow::ensure!(status.status.success(), "git status failed");
    if !String::from_utf8_lossy(&status.stdout).trim().is_empty() {
        text.push_str("-dirty");
    }
    Ok(text)
}

fn write_meta(config: &AnswerConfig) -> Result<()> {
    let meta_path = config.run.join("meta.json");
    if meta_path.exists() {
        let existing: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&meta_path)?)?;
        anyhow::ensure!(
            existing["arm"] == config.arm.name() && existing["model"] == config.model.as_str(),
            "run dir {} was started with arm={} model={}; use a fresh --run dir",
            config.run.display(),
            existing["arm"],
            existing["model"],
        );
        return Ok(());
    }
    let meta = serde_json::json!({
        "arm": config.arm.name(),
        "model": config.model,
        "categories": config.categories,
        "limit": config.limit,
        "max_budget_usd": config.max_budget_usd,
        "claude_version": claude::claude_version()?,
        "bench_rev": bench_rev()?,
        "started_at_epoch": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
    });
    std::fs::write(meta_path, serde_json::to_string_pretty(&meta)? + "\n")?;
    Ok(())
}

fn answer_one(
    config: &AnswerConfig,
    conversation: &Conversation,
    cwd: &Path,
    mcp_config: Option<&Path>,
    index: usize,
    qa: &Qa,
) -> Result<AnswerRecord> {
    let prompt = build_prompt(config, conversation, qa)?;
    let allowed = config.arm.allowed_tools();
    let disallowed = config.arm.disallowed_tools();
    let result = claude::run(&Invocation {
        cwd,
        prompt: &prompt,
        model: &config.model,
        allowed_tools: &allowed,
        disallowed_tools: &disallowed,
        mcp_config,
        max_budget_usd: config.max_budget_usd,
        timeout: Duration::from_secs(config.timeout_secs),
    })?;
    let answer = result.result.clone().unwrap_or_default();
    anyhow::ensure!(
        !result.is_error,
        "claude returned an error ({}): {}",
        result.subtype,
        answer.chars().take(200).collect::<String>()
    );
    Ok(AnswerRecord {
        id: format!("{}:{index}", conversation.sample_id),
        conversation: conversation.sample_id.clone(),
        category: qa.category,
        question: qa.question.clone(),
        gold_answer: qa.answer.clone(),
        answer: answer.trim().to_string(),
        total_cost_usd: result.total_cost_usd,
        num_turns: result.num_turns,
        duration_ms: result.duration_ms,
        session_id: result.session_id,
        input_tokens: result.usage.input_tokens,
        output_tokens: result.usage.output_tokens,
        cache_creation_input_tokens: result.usage.cache_creation_input_tokens,
        cache_read_input_tokens: result.usage.cache_read_input_tokens,
    })
}

pub fn run(config: &AnswerConfig) -> Result<()> {
    std::fs::create_dir_all(&config.run)?;
    write_meta(config)?;
    let answers_path = config.run.join("answers.jsonl");
    let done = existing_ids(&answers_path)?;
    let writer = Mutex::new(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&answers_path)?,
    );
    let conversations = locomo::load(&config.data, Some(&config.categories))?;
    for conversation in &conversations {
        if config
            .conversation_filter
            .as_ref()
            .map(|f| f.contains(&conversation.sample_id))
            == Some(false)
        {
            continue;
        }
        let cwd = match config.arm.workspace_kind() {
            Some(kind) => {
                let dir = config
                    .workspaces
                    .join(kind)
                    .join(&conversation.sample_id);
                anyhow::ensure!(
                    dir.exists(),
                    "workspace {} missing; run `cargo xtask {}` first",
                    dir.display(),
                    if kind == "curated" { "curate" } else { "prepare" }
                );
                dir
            }
            None => {
                let dir = config.run.join(".ctx");
                std::fs::create_dir_all(&dir)?;
                dir
            }
        };
        let mcp_config = config
            .arm
            .uses_mcp()
            .then(|| cwd.join(".mcp.json"))
            .filter(|p| p.exists());
        anyhow::ensure!(
            !(config.arm.uses_mcp() && mcp_config.is_none()),
            "missing .mcp.json in {}; run `cargo xtask prepare`",
            cwd.display()
        );
        let qa_slice = match config.limit {
            Some(limit) => &conversation.qa[..limit.min(conversation.qa.len())],
            None => &conversation.qa[..],
        };
        let mut pending: VecDeque<(usize, &Qa)> = qa_slice
            .iter()
            .enumerate()
            .filter(|(i, _)| !done.contains(&format!("{}:{i}", conversation.sample_id)))
            .collect();
        if pending.is_empty() {
            println!("{}: already complete", conversation.sample_id);
            continue;
        }
        let total = pending.len();
        let failures = AtomicUsize::new(0);
        let completed = AtomicUsize::new(0);
        let process = |index: usize, qa: &Qa| {
            match answer_one(config, conversation, &cwd, mcp_config.as_deref(), index, qa) {
                Ok(record) => {
                    failures.store(0, Ordering::SeqCst);
                    completed.fetch_add(1, Ordering::SeqCst);
                    let line = serde_json::to_string(&record).expect("serialize record");
                    let mut file = writer.lock().expect("writer lock");
                    writeln!(file, "{line}").expect("append answer");
                }
                Err(error) => {
                    failures.fetch_add(1, Ordering::SeqCst);
                    eprintln!(
                        "{}:{index} failed: {error:#}",
                        conversation.sample_id
                    );
                }
            }
        };
        if let Some((index, qa)) = pending.pop_front() {
            process(index, qa);
        }
        let queue = Mutex::new(pending);
        std::thread::scope(|scope| {
            for _ in 0..config.workers.max(1) {
                scope.spawn(|| loop {
                    if failures.load(Ordering::SeqCst) >= MAX_CONSECUTIVE_FAILURES {
                        break;
                    }
                    let item = queue.lock().expect("queue lock").pop_front();
                    let Some((index, qa)) = item else { break };
                    process(index, qa);
                });
            }
        });
        anyhow::ensure!(
            failures.load(Ordering::SeqCst) < MAX_CONSECUTIVE_FAILURES,
            "aborted after {MAX_CONSECUTIVE_FAILURES} consecutive failures (usage limit?); rerun the same command to resume"
        );
        println!(
            "{}: answered {}/{total} questions",
            conversation.sample_id,
            completed.load(Ordering::SeqCst)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curated_arm_tools() {
        assert_eq!(
            Arm::Curated.allowed_tools(),
            vec![
                "mcp__iwe__iwe_find",
                "mcp__iwe__iwe_retrieve",
                "mcp__iwe__iwe_tree",
                "mcp__iwe__iwe_squash",
                "mcp__iwe__iwe_stats",
            ]
        );
        assert_eq!(
            Arm::Curated.disallowed_tools(),
            vec![
                "Bash",
                "Write",
                "Edit",
                "NotebookEdit",
                "WebSearch",
                "WebFetch",
                "Task",
                "TodoWrite",
                "Skill",
                "SlashCommand",
                "BashOutput",
                "KillShell",
                "EnterPlanMode",
                "ExitPlanMode",
                "AskUserQuestion",
                "Grep",
                "Glob",
                "Read",
                "LS",
                "mcp__iwe__iwe_create",
                "mcp__iwe__iwe_update",
                "mcp__iwe__iwe_delete",
                "mcp__iwe__iwe_rename",
                "mcp__iwe__iwe_extract",
                "mcp__iwe__iwe_inline",
                "mcp__iwe__iwe_normalize",
                "mcp__iwe__iwe_attach",
                "mcp__iwe__iwe_query",
            ]
        );
        assert_eq!(Arm::Curated.workspace_kind(), Some("curated"));
        assert!(Arm::Curated.uses_mcp());
    }

    #[test]
    fn curated_q_arm_tools() {
        assert_eq!(
            Arm::CuratedQ.allowed_tools(),
            vec![
                "mcp__iwe__iwe_find",
                "mcp__iwe__iwe_retrieve",
                "mcp__iwe__iwe_tree",
                "mcp__iwe__iwe_squash",
                "mcp__iwe__iwe_stats",
                "mcp__iwe__iwe_query",
            ]
        );
        assert_eq!(
            Arm::CuratedQ.disallowed_tools(),
            vec![
                "Bash",
                "Write",
                "Edit",
                "NotebookEdit",
                "WebSearch",
                "WebFetch",
                "Task",
                "TodoWrite",
                "Skill",
                "SlashCommand",
                "BashOutput",
                "KillShell",
                "EnterPlanMode",
                "ExitPlanMode",
                "AskUserQuestion",
                "Grep",
                "Glob",
                "Read",
                "LS",
                "mcp__iwe__iwe_create",
                "mcp__iwe__iwe_update",
                "mcp__iwe__iwe_delete",
                "mcp__iwe__iwe_rename",
                "mcp__iwe__iwe_extract",
                "mcp__iwe__iwe_inline",
                "mcp__iwe__iwe_normalize",
                "mcp__iwe__iwe_attach",
            ]
        );
        assert_eq!(Arm::CuratedQ.workspace_kind(), Some("curated"));
        assert!(Arm::CuratedQ.uses_mcp());
    }

    #[test]
    fn curated_fs_arm_tools() {
        assert_eq!(
            Arm::CuratedFs.allowed_tools(),
            vec!["Grep", "Glob", "Read", "LS"]
        );
        assert_eq!(
            Arm::CuratedFs.disallowed_tools(),
            vec![
                "Bash",
                "Write",
                "Edit",
                "NotebookEdit",
                "WebSearch",
                "WebFetch",
                "Task",
                "TodoWrite",
                "Skill",
                "SlashCommand",
                "BashOutput",
                "KillShell",
                "EnterPlanMode",
                "ExitPlanMode",
                "AskUserQuestion",
                "mcp__iwe__iwe_find",
                "mcp__iwe__iwe_retrieve",
                "mcp__iwe__iwe_tree",
                "mcp__iwe__iwe_squash",
                "mcp__iwe__iwe_stats",
                "mcp__iwe__iwe_create",
                "mcp__iwe__iwe_update",
                "mcp__iwe__iwe_delete",
                "mcp__iwe__iwe_rename",
                "mcp__iwe__iwe_extract",
                "mcp__iwe__iwe_inline",
                "mcp__iwe__iwe_normalize",
                "mcp__iwe__iwe_attach",
                "mcp__iwe__iwe_query",
            ]
        );
        assert_eq!(Arm::CuratedFs.workspace_kind(), Some("curated"));
        assert!(!Arm::CuratedFs.uses_mcp());
    }

    #[test]
    fn curated_ctx_arm_tools() {
        assert_eq!(Arm::CuratedCtx.allowed_tools(), Vec::<&str>::new());
        assert_eq!(
            Arm::CuratedCtx.disallowed_tools(),
            vec![
                "Bash",
                "Write",
                "Edit",
                "NotebookEdit",
                "WebSearch",
                "WebFetch",
                "Task",
                "TodoWrite",
                "Skill",
                "SlashCommand",
                "BashOutput",
                "KillShell",
                "EnterPlanMode",
                "ExitPlanMode",
                "AskUserQuestion",
                "Grep",
                "Glob",
                "Read",
                "LS",
                "mcp__iwe__iwe_find",
                "mcp__iwe__iwe_retrieve",
                "mcp__iwe__iwe_tree",
                "mcp__iwe__iwe_squash",
                "mcp__iwe__iwe_stats",
                "mcp__iwe__iwe_create",
                "mcp__iwe__iwe_update",
                "mcp__iwe__iwe_delete",
                "mcp__iwe__iwe_rename",
                "mcp__iwe__iwe_extract",
                "mcp__iwe__iwe_inline",
                "mcp__iwe__iwe_normalize",
                "mcp__iwe__iwe_attach",
                "mcp__iwe__iwe_query",
            ]
        );
        assert_eq!(Arm::CuratedCtx.workspace_kind(), Some("curated"));
        assert!(!Arm::CuratedCtx.uses_mcp());
    }

    #[test]
    fn arm_names() {
        assert_eq!(Arm::Curated.name(), "curated");
        assert_eq!(Arm::CuratedFs.name(), "curated-fs");
        assert_eq!(Arm::CuratedQ.name(), "curated-q");
        assert_eq!(Arm::CuratedCtx.name(), "curated-ctx");
    }

    #[test]
    fn dev_split_resolves() {
        assert_eq!(
            Split::Dev.conversations(),
            BTreeSet::from(["conv-26".to_string(), "conv-30".to_string()])
        );
    }

    #[test]
    fn test_split_resolves() {
        assert_eq!(
            Split::Test.conversations(),
            BTreeSet::from([
                "conv-41".to_string(),
                "conv-42".to_string(),
                "conv-43".to_string(),
                "conv-44".to_string(),
                "conv-47".to_string(),
                "conv-48".to_string(),
                "conv-49".to_string(),
                "conv-50".to_string(),
            ])
        );
    }
}
