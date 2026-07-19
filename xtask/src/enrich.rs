use std::collections::{BTreeSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::answer::{BASE_DISALLOWED, FILE_TOOLS};
use crate::claude::{self, Invocation};
use crate::prepare;
use crate::records::{existing_ids, read_jsonl};

const ENRICH_TEMPLATE: &str = include_str!("../prompts/enrich.md");
const MAX_CONSECUTIVE_FAILURES: usize = 5;
const CANDIDATE_BUFFER: usize = 8;
const QUERY_CHARS: usize = 600;
const SNIPPET_WORDS: usize = 15;
const MIN_SPAN_CHARS: usize = 3;

pub struct EnrichConfig {
    pub workspaces: PathBuf,
    pub source: String,
    pub target: String,
    pub model: String,
    pub candidates: usize,
    pub limit: Option<usize>,
    pub replay: Option<PathBuf>,
    pub workers: usize,
    pub max_budget_usd: f64,
    pub timeout_secs: u64,
}

pub fn parse_links(text: &str) -> Result<Vec<(String, String)>> {
    let start = text.find('[').context("no JSON array in output")?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut end = None;
    for (i, c) in text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(start + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end.context("unterminated JSON array in output")?;
    let value: serde_json::Value = serde_json::from_str(&text[start..=end])?;
    let mut links = Vec::new();
    for entry in value.as_array().context("expected a JSON array")? {
        let span = entry["text"].as_str().unwrap_or_default();
        let key = entry["key"].as_str().unwrap_or_default();
        if !span.is_empty() && !key.is_empty() {
            links.push((span.to_string(), key.to_string()));
        }
    }
    Ok(links)
}

fn link_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut i = 0;
    while let Some(open) = text[i..].find('[') {
        let open = i + open;
        let Some(mid) = text[open..].find("](") else {
            break;
        };
        let mid = open + mid;
        let Some(close) = text[mid..].find(')') else {
            break;
        };
        let close = mid + close;
        spans.push((open, close + 1));
        i = close + 1;
    }
    spans
}

pub fn link_targets_of(text: &str) -> BTreeSet<String> {
    link_spans(text)
        .into_iter()
        .filter_map(|(start, end)| {
            let span = &text[start..end];
            let mid = span.find("](")?;
            Some(span[mid + 2..span.len() - 1].to_string())
        })
        .collect()
}

fn find_outside(text: &str, span: &str, occupied: &[(usize, usize)]) -> Option<usize> {
    let mut from = 0;
    while let Some(pos) = text[from..].find(span) {
        let start = from + pos;
        let end = start + span.len();
        if !occupied.iter().any(|(o_start, o_end)| start < *o_end && end > *o_start) {
            return Some(start);
        }
        from = end;
    }
    None
}

pub fn apply_links(
    content: &str,
    links: &[(String, String)],
    valid_keys: &BTreeSet<String>,
    own_key: &str,
) -> (String, usize) {
    let body_start = content.find("\n\n").map(|p| p + 2).unwrap_or(0);
    let (head, body) = content.split_at(body_start);
    let occupied = link_spans(body);
    let mut matches: Vec<(usize, usize, &str)> = Vec::new();
    for (span, key) in links {
        if span.len() < MIN_SPAN_CHARS || key == own_key || !valid_keys.contains(key) {
            continue;
        }
        if let Some(start) = find_outside(body, span, &occupied) {
            matches.push((start, start + span.len(), key));
        }
    }
    matches.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut out = String::new();
    let mut cursor = 0;
    let mut applied = 0;
    for (start, end, key) in matches {
        if start < cursor {
            continue;
        }
        out.push_str(&body[cursor..start]);
        out.push('[');
        out.push_str(&body[start..end]);
        out.push_str("](");
        out.push_str(key);
        out.push(')');
        cursor = end;
        applied += 1;
    }
    out.push_str(&body[cursor..]);
    (format!("{head}{out}"), applied)
}

fn candidate_block(
    workspace: &Path,
    content: &str,
    own_key: &str,
    wanted: usize,
) -> Result<String> {
    let iwe = prepare::resolve_iwe()?;
    let query: String = content.chars().take(QUERY_CHARS).collect();
    let output = std::process::Command::new(&iwe)
        .args([
            "retrieve",
            "--lexical",
            &query,
            "--limit",
            &(wanted + CANDIDATE_BUFFER).to_string(),
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
    let linked = link_targets_of(content);
    let mut lines = Vec::new();
    for doc in &docs {
        let key = doc["key"].as_str().unwrap_or_default();
        if key.is_empty() || key == own_key || linked.contains(key) {
            continue;
        }
        let text = doc["content"].as_str().unwrap_or_default();
        let title = text
            .lines()
            .next()
            .unwrap_or_default()
            .trim_start_matches('#')
            .trim();
        let body = text.split_once("\n\n").map(|(_, b)| b).unwrap_or_default();
        let snippet: Vec<&str> = body.split_whitespace().take(SNIPPET_WORDS).collect();
        lines.push(format!("- {key} — {title}: {}", snippet.join(" ")));
        if lines.len() >= wanted {
            break;
        }
    }
    Ok(lines.join("\n"))
}

fn enrich_one(
    config: &EnrichConfig,
    target_dir: &Path,
    valid_keys: &BTreeSet<String>,
    key: &str,
) -> Result<serde_json::Value> {
    let page_path = target_dir.join(format!("{key}.md"));
    let content = std::fs::read_to_string(&page_path)?;
    let candidates = candidate_block(target_dir, &content, key, config.candidates)?;
    let prompt = ENRICH_TEMPLATE
        .replace("{key}", key)
        .replace("{content}", &content)
        .replace("{candidates}", &candidates);
    let disallowed = [BASE_DISALLOWED, FILE_TOOLS].concat();
    let result = claude::run(&Invocation {
        cwd: target_dir,
        prompt: &prompt,
        model: &config.model,
        allowed_tools: &[],
        disallowed_tools: &disallowed,
        mcp_config: None,
        max_budget_usd: config.max_budget_usd,
        timeout: Duration::from_secs(config.timeout_secs),
    })?;
    let text = result.result.clone().unwrap_or_default();
    anyhow::ensure!(
        !result.is_error,
        "claude returned an error ({}): {}",
        result.subtype,
        text.chars().take(200).collect::<String>()
    );
    let links = parse_links(&text)?;
    let (updated, applied) = apply_links(&content, &links, valid_keys, key);
    if applied > 0 {
        std::fs::write(&page_path, updated)?;
    }
    Ok(serde_json::json!({
        "id": key,
        "proposed": links.len(),
        "applied": applied,
        "links": links.iter().map(|(text, key)| serde_json::json!({"text": text, "key": key})).collect::<Vec<_>>(),
        "cost": result.total_cost_usd,
    }))
}

fn setup_target(root: &Path, source: &str, target: &str) -> Result<PathBuf> {
    let source_dir = root.join(source);
    let target_dir = root.join(target);
    anyhow::ensure!(
        source_dir.exists(),
        "source store {} missing; run `cargo xtask ingest --linked` first",
        source_dir.display()
    );
    if target_dir.exists() {
        return Ok(target_dir);
    }
    std::fs::create_dir_all(&target_dir)?;
    for entry in std::fs::read_dir(&source_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            std::fs::copy(&path, target_dir.join(path.file_name().context("file name")?))?;
        }
    }
    prepare::init_iwe_bare(&target_dir)?;
    Ok(target_dir)
}

fn run_replay(target_dir: &Path, valid_keys: &BTreeSet<String>, replay: &Path) -> Result<()> {
    let records: Vec<serde_json::Value> = read_jsonl(replay)?;
    let mut applied_total = 0;
    let mut pages = 0;
    for record in &records {
        let key = record["id"].as_str().unwrap_or_default();
        let links: Vec<(String, String)> = record["links"]
            .as_array()
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| {
                        Some((
                            entry["text"].as_str()?.to_string(),
                            entry["key"].as_str()?.to_string(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if key.is_empty() || links.is_empty() {
            continue;
        }
        let path = target_dir.join(format!("{key}.md"));
        if !path.exists() {
            continue;
        }
        let content = std::fs::read_to_string(&path)?;
        let (updated, applied) = apply_links(&content, &links, valid_keys, key);
        if applied > 0 {
            std::fs::write(&path, updated)?;
            applied_total += applied;
            pages += 1;
        }
    }
    println!(
        "replayed {applied_total} links onto {pages} pages in {}",
        target_dir.display()
    );
    Ok(())
}

pub fn run(config: &EnrichConfig) -> Result<()> {
    let root = config.workspaces.join("hotpot");
    let target_dir = setup_target(&root, &config.source, &config.target)?;
    let mut keys = Vec::new();
    for entry in std::fs::read_dir(&target_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            keys.push(
                path.file_stem()
                    .context("file stem")?
                    .to_string_lossy()
                    .to_string(),
            );
        }
    }
    keys.sort();
    let valid_keys: BTreeSet<String> = keys.iter().cloned().collect();
    if let Some(replay) = &config.replay {
        return run_replay(&target_dir, &valid_keys, replay);
    }
    let log_path = root.join(format!("enrich-{}.jsonl", config.target));
    let done = existing_ids(&log_path)?;
    let writer = Mutex::new(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?,
    );
    let mut pending: VecDeque<&String> = keys.iter().filter(|k| !done.contains(k.as_str())).collect();
    if let Some(limit) = config.limit {
        pending.truncate(limit);
    }
    if pending.is_empty() {
        println!("{}: already complete", config.target);
        return Ok(());
    }
    let total = pending.len();
    let failures = AtomicUsize::new(0);
    let completed = AtomicUsize::new(0);
    let applied_total = AtomicUsize::new(0);
    let process = |key: &String| match enrich_one(config, &target_dir, &valid_keys, key) {
        Ok(record) => {
            failures.store(0, Ordering::SeqCst);
            completed.fetch_add(1, Ordering::SeqCst);
            applied_total.fetch_add(
                record["applied"].as_u64().unwrap_or(0) as usize,
                Ordering::SeqCst,
            );
            let line = serde_json::to_string(&record).expect("serialize record");
            let mut file = writer.lock().expect("writer lock");
            writeln!(file, "{line}").expect("append record");
        }
        Err(error) => {
            failures.fetch_add(1, Ordering::SeqCst);
            eprintln!("{key} failed: {error:#}");
        }
    };
    let queue = Mutex::new(pending);
    std::thread::scope(|scope| {
        for _ in 0..config.workers.max(1) {
            scope.spawn(|| loop {
                if failures.load(Ordering::SeqCst) >= MAX_CONSECUTIVE_FAILURES {
                    break;
                }
                let item = queue.lock().expect("queue lock").pop_front();
                let Some(key) = item else { break };
                process(key);
            });
        }
    });
    anyhow::ensure!(
        failures.load(Ordering::SeqCst) < MAX_CONSECUTIVE_FAILURES,
        "aborted after {MAX_CONSECUTIVE_FAILURES} consecutive failures (usage limit?); rerun the same command to resume"
    );
    println!(
        "{}: enriched {}/{total} pages, {} links applied",
        config.target,
        completed.load(Ordering::SeqCst),
        applied_total.load(Ordering::SeqCst)
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_links_with_surrounding_prose() {
        assert_eq!(
            parse_links(
                "Here are the links:\n[{\"text\": \"her debut novel\", \"key\": \"first-light-novel\"}]\nDone."
            )
            .unwrap(),
            vec![("her debut novel".to_string(), "first-light-novel".to_string())]
        );
    }

    #[test]
    fn parses_empty_array() {
        assert_eq!(parse_links("[]").unwrap(), Vec::<(String, String)>::new());
    }

    #[test]
    fn parses_array_with_trailing_prose_containing_brackets() {
        assert_eq!(
            parse_links(
                "[{\"text\": \"the sequel\", \"key\": \"beta-two\"}]\nNote: no other [candidates] apply."
            )
            .unwrap(),
            vec![("the sequel".to_string(), "beta-two".to_string())]
        );
    }

    #[test]
    fn applies_valid_links_outside_existing() {
        let valid = BTreeSet::from(["beta".to_string(), "gamma".to_string()]);
        let content = "# Alpha\n\nAlpha works with [Beta Corp](beta) and the gamma project.";
        let links = vec![
            ("Beta Corp".to_string(), "beta".to_string()),
            ("the gamma project".to_string(), "gamma".to_string()),
            ("missing span".to_string(), "gamma".to_string()),
            ("Alpha works".to_string(), "unknown-key".to_string()),
        ];
        assert_eq!(
            apply_links(content, &links, &valid, "alpha"),
            (
                "# Alpha\n\nAlpha works with [Beta Corp](beta) and [the gamma project](gamma)."
                    .to_string(),
                1,
            )
        );
    }

    #[test]
    fn skips_spans_inside_existing_link_text() {
        let valid = BTreeSet::from(["beta".to_string(), "corp".to_string()]);
        let content = "# Alpha\n\nSee [Beta Corp](beta) for details.";
        let links = vec![("Corp".to_string(), "corp".to_string())];
        assert_eq!(
            apply_links(content, &links, &valid, "alpha"),
            ("# Alpha\n\nSee [Beta Corp](beta) for details.".to_string(), 0)
        );
    }

    #[test]
    fn extracts_existing_link_targets() {
        assert_eq!(
            link_targets_of("# A\n\n[Beta](beta) and [Gamma Inc](gamma-inc)."),
            BTreeSet::from(["beta".to_string(), "gamma-inc".to_string()])
        );
    }
}
