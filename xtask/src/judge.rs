use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::answer::{BASE_DISALLOWED, FILE_TOOLS};
use crate::claude::{self, Invocation};
use crate::records::{existing_ids, read_jsonl, AnswerRecord, JudgmentRecord};

const JUDGE_TEMPLATE: &str = include_str!("../prompts/judge.md");
const JUDGE_HOTPOT_TEMPLATE: &str = include_str!("../prompts/judge_hotpot.md");
const MAX_CONSECUTIVE_FAILURES: usize = 5;

fn judge_template(run: &Path) -> Result<&'static str> {
    let meta_path = run.join("meta.json");
    if !meta_path.exists() {
        return Ok(JUDGE_TEMPLATE);
    }
    let meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&meta_path)?)?;
    Ok(match meta["dataset"].as_str() {
        Some("hotpot") => JUDGE_HOTPOT_TEMPLATE,
        _ => JUDGE_TEMPLATE,
    })
}

pub fn extract_verdict(text: &str) -> Result<(String, String)> {
    let start = text.find('{').context("no JSON object in judge output")?;
    let end = text.rfind('}').context("no JSON object in judge output")?;
    let value: serde_json::Value = serde_json::from_str(&text[start..=end])?;
    let label = value["label"]
        .as_str()
        .context("judge output has no label")?
        .to_string();
    anyhow::ensure!(
        label == "CORRECT" || label == "WRONG",
        "unexpected judge label: {label}"
    );
    let explanation = value["explanation"].as_str().unwrap_or_default().to_string();
    Ok((label, explanation))
}

pub struct JudgeConfig {
    pub run: PathBuf,
    pub judge_model: String,
    pub workers: usize,
    pub max_budget_usd: f64,
    pub timeout_secs: u64,
}

fn judge_one(
    config: &JudgeConfig,
    template: &str,
    cwd: &Path,
    answer: &AnswerRecord,
) -> Result<JudgmentRecord> {
    let prompt = template
        .replace("{question}", &answer.question)
        .replace("{gold_answer}", &answer.gold_answer)
        .replace("{generated_answer}", &answer.answer);
    let disallowed = [BASE_DISALLOWED, FILE_TOOLS].concat();
    let result = claude::run(&Invocation {
        cwd,
        prompt: &prompt,
        model: &config.judge_model,
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
    let (label, explanation) = extract_verdict(&text)?;
    Ok(JudgmentRecord {
        id: answer.id.clone(),
        label,
        explanation,
        judge_cost_usd: result.total_cost_usd,
    })
}

pub fn run(config: &JudgeConfig) -> Result<()> {
    let template = judge_template(&config.run)?;
    let answers: Vec<AnswerRecord> = read_jsonl(&config.run.join("answers.jsonl"))?;
    let judgments_path = config.run.join("judgments.jsonl");
    let done = existing_ids(&judgments_path)?;
    let cwd = config.run.join(".judge");
    std::fs::create_dir_all(&cwd)?;
    let writer = Mutex::new(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&judgments_path)?,
    );
    let pending: VecDeque<&AnswerRecord> = answers
        .iter()
        .filter(|a| !done.contains(&a.id))
        .collect();
    let total = pending.len();
    let failures = AtomicUsize::new(0);
    let completed = AtomicUsize::new(0);
    let queue = Mutex::new(pending);
    std::thread::scope(|scope| {
        for _ in 0..config.workers.max(1) {
            scope.spawn(|| loop {
                if failures.load(Ordering::SeqCst) >= MAX_CONSECUTIVE_FAILURES {
                    break;
                }
                let item = queue.lock().expect("queue lock").pop_front();
                let Some(answer) = item else { break };
                match judge_one(config, template, &cwd, answer) {
                    Ok(record) => {
                        failures.store(0, Ordering::SeqCst);
                        completed.fetch_add(1, Ordering::SeqCst);
                        let line = serde_json::to_string(&record).expect("serialize record");
                        let mut file = writer.lock().expect("writer lock");
                        writeln!(file, "{line}").expect("append judgment");
                    }
                    Err(error) => {
                        failures.fetch_add(1, Ordering::SeqCst);
                        eprintln!("{} judge failed: {error:#}", answer.id);
                    }
                }
            });
        }
    });
    anyhow::ensure!(
        failures.load(Ordering::SeqCst) < MAX_CONSECUTIVE_FAILURES,
        "aborted after {MAX_CONSECUTIVE_FAILURES} consecutive failures (usage limit?); rerun the same command to resume"
    );
    println!("judged {}/{total} answers", completed.load(Ordering::SeqCst));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_plain_json() {
        assert_eq!(
            extract_verdict(r#"{"explanation": "same date", "label": "CORRECT"}"#).unwrap(),
            ("CORRECT".to_string(), "same date".to_string())
        );
    }

    #[test]
    fn extracts_json_with_surrounding_prose() {
        assert_eq!(
            extract_verdict(
                "Here is my verdict:\n{\"explanation\": \"different topic\", \"label\": \"WRONG\"}\nDone."
            )
            .unwrap(),
            ("WRONG".to_string(), "different topic".to_string())
        );
    }

    #[test]
    fn rejects_missing_label() {
        assert_eq!(
            extract_verdict("no json here").unwrap_err().to_string(),
            "no JSON object in judge output"
        );
    }
}
